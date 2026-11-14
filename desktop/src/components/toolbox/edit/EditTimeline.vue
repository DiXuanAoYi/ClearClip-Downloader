<script setup lang="ts">
// 时间线：标尺（标记、入点出点）、文字轨、视频轨（叠加轨 V2…、主轨 V1）、音频轨。每一类都可以有很多条轨道，
// 每条轨道有自己的名字和开关；拖动调整顺序、位置和长度，素材可以拖到别的轨道，或者拖到轨道之间新建一条。
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api } from '../../../api'
import type { EditApi } from '../../../composables/useEditProject'
import type { EditAudio, EditClip, EditMarker, EditOverlay, EditText, EditTrack, EditTrackKind, EditTrackRole } from '../../../types'
import { beatSnapPoints, beatsOnTimeline, kfEval, MIN_MS, movePairBy, audioSpan, clamp, editPoints, overlayDuration, overlayEnd, overlaySourceLimit, snap, tickStep } from '../../../utils/edit'
import { maxNo, nextNo, roomFor, shownNos, swapTracks, TRACK_ROLES, trackCode, trackOf, trackTitle } from '../../../utils/tracks'
import { BUILTIN_TEMPLATES, loadTemplates, saveTemplates, templateFrom } from '../../../utils/trackTemplates'
import TrackHead from './TrackHead.vue'
import TrackMenu, { type MenuItem } from './TrackMenu.vue'

const props = withDefaults(defineProps<{ ed: EditApi; pps: number; snapOn?: boolean; tool?: 'select' | 'blade' | 'slip' | 'roll' | 'slide'; showWave?: boolean; showVol?: boolean }>(), { snapOn: true, tool: 'select', showWave: true, showVol: true })
const emit = defineEmits<{ zoom: [pps: number] }>()
const ed = props.ed

const LABEL_W = 176
const SNAP_PX = 7
const scroller = ref<HTMLElement | null>(null)
const bodyEl = ref<HTMLElement | null>(null)
const rulerLane = ref<HTMLElement | null>(null)

const px = (ms: number) => (ms * props.pps) / 1000
const msOf = (x: number) => (x / props.pps) * 1000

const contentW = computed(() => Math.max(600, px(ed.extent.value + 6000)))

// ---------- 标尺 ----------

const ticks = computed(() => {
  const step = tickStep(props.pps)
  const n = Math.ceil((contentW.value / props.pps) * 1000 / step)
  return Array.from({ length: n + 1 }, (_, i) => ({ ms: i * step, x: px(i * step), label: tickLabel(i * step, step) }))
})
function tickLabel(ms: number, step: number): string {
  const total = ms / 1000
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const sec = step < 1000 ? s.toFixed(1).padStart(4, '0') : String(Math.floor(s)).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`
}

// ---------- 轨道 ----------

interface Row {
  key: string
  kind: EditTrackKind
  no: number
  main: boolean
  meta: EditTrack
}

const P = computed(() => ed.project.value)
const metaOf = (kind: EditTrackKind, no: number) => trackOf(P.value, kind, no)

/** 从上到下：文字轨（编号大的在上）、视频叠加轨（编号大的在上）、主轨、音频轨（编号小的在上） */
const rows = computed<Row[]>(() => {
  const mk = (kind: EditTrackKind, no: number): Row => ({ key: `${kind}${no}`, kind, no, main: kind === 'video' && no === 0, meta: metaOf(kind, no) })
  const text = shownNos(P.value, 'text').reverse().map((n) => mk('text', n))
  const video = shownNos(P.value, 'video').reverse().map((n) => mk('video', n))
  const audio = shownNos(P.value, 'audio').map((n) => mk('audio', n))
  return [...text, ...video, ...audio]
})

const H = {
  text: { s: 20, m: 26, l: 40 },
  lane: { s: 24, m: 30, l: 44 },
  main: { s: 46, m: 58, l: 80 },
  audio: { s: 22, m: 30, l: 56 },
} as const

/** 每条叠加轨里的素材（同一轨道上时间重叠的会错开成多行） */
const byTrack = computed(() => {
  const m = new Map<number, { o: EditOverlay; lane: number }[]>()
  P.value.overlays.forEach((o, i) => m.set(o.track, [...(m.get(o.track) ?? []), { o, lane: ed.overlayLane.value[i] ?? 0 }]))
  return m
})
const laneCount = (no: number) => Math.max(1, ...(byTrack.value.get(no) ?? []).map((x) => x.lane + 1))
const laneH = (r: Row) => H.lane[r.meta.height]
function rowH(r: Row): number {
  if (r.kind === 'text') return H.text[r.meta.height]
  if (r.kind === 'audio') return H.audio[r.meta.height]
  if (r.main) return H.main[r.meta.height]
  return laneCount(r.no) * laneH(r)
}
const textsOn = (no: number) => P.value.texts.filter((t) => t.track === no)
const audioOn = (no: number) => P.value.audio.filter((a) => a.track === no)

/** 点时间线时让输入框失去焦点，这样空格、S、Delete 等快捷键马上可用。 */
function blurInput() {
  const a = document.activeElement as HTMLElement | null
  if (a && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)) a.blur()
}

const isSel = (kind: 'clip' | 'overlay' | 'text' | 'audio', id: number) => ed.isSelected(kind, id)
/** 在组里的项：编组编号（用来显示小标记）。 */
const groupNo = (x: { group?: number }) => x.group
/** Shift+点击：多选。返回 true 表示已处理，不再往下走（不拖动）。 */
function shiftPick(e: PointerEvent, kind: 'clip' | 'overlay' | 'text' | 'audio', id: number): boolean {
  if (!e.shiftKey) return false
  ed.toggleSel(kind, id)
  return true
}
const lockedNo = (kind: EditTrackKind, no: number) => metaOf(kind, no).lock

// ---------- 拖动 ----------

function drag(e: PointerEvent, onMove: (dx: number, ev: PointerEvent) => void, onEnd?: (moved: boolean) => void) {
  const x0 = e.clientX
  let moved = false
  const move = (ev: PointerEvent) => {
    const dx = ev.clientX - x0
    if (!moved && Math.abs(dx) < 3 && Math.abs(ev.clientY - e.clientY) < 3) return
    moved = true
    onMove(dx, ev)
  }
  const up = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    window.removeEventListener('pointercancel', up)
    onEnd?.(moved)
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
  window.addEventListener('pointercancel', up)
}

/** 鼠标在时间线上的时间（毫秒）。 */
function timeAt(clientX: number): number {
  const r = rulerLane.value?.getBoundingClientRect()
  return r ? Math.max(0, msOf(clientX - r.left)) : 0
}

function seek(ms: number) {
  ed.playhead.value = clamp(Math.round(ms), 0, Math.max(ed.total.value, 0))
}

function scrub(e: PointerEvent) {
  seek(timeAt(e.clientX))
  drag(e, (_dx, ev) => seek(timeAt(ev.clientX)))
}

function bgDown(e: PointerEvent) {
  ed.sel.value = null
  scrub(e)
}

/** 吸附用的候选时间：播放位置，加上主轨片段、叠加素材、文字、配乐的边界，和标记。 */
function snapPoints(skip?: { kind: 'text' | 'audio' | 'overlay'; id: number }): number[] {
  return [ed.playhead.value, ...P.value.markers.map((m) => m.ms), ...editPoints(P.value, ed.placed.value, skip), ...beatSnapPoints(P.value, skip?.kind === 'audio' ? skip.id : undefined)]
}
/** 吸附开着时把 v 吸到附近的候选值上，关着就原样返回。 */
const snapTo = (v: number, pts: number[]) => (props.snapOn ? snap(v, pts, msOf(SNAP_PX)) : v)

// ---------- 拖到别的轨道 / 轨道之间新建 ----------

interface Hit {
  /** 落在哪条轨道（insert 为真时：要新建的轨道插在这个编号的位置） */
  no: number
  insert: boolean
  /** 插入线的位置（相对时间线内容区顶部，像素） */
  y: number
}
const EDGE = 6
const insY = ref<number | null>(null)

function hitTrack(clientY: number, kind: EditTrackKind): Hit | null {
  const body = bodyEl.value
  if (!body) return null
  const base = body.getBoundingClientRect().top
  const els = [...body.querySelectorAll<HTMLElement>(`.row[data-kind="${kind}"]`)]
  if (!els.length) return null
  const list = els.map((el) => ({ no: Number(el.dataset.no), r: el.getBoundingClientRect() }))
  const desc = kind !== 'audio'
  const room = roomFor(P.value, kind) > 0
  const above = (i: number): Hit | null => (room ? { no: desc ? list[i].no + 1 : list[i].no, insert: true, y: list[i].r.top - base } : null)
  const below = (i: number): Hit | null => {
    if (!room || (kind === 'video' && list[i].no === 0)) return null
    return { no: desc ? list[i].no : list[i].no + 1, insert: true, y: list[i].r.bottom - base }
  }
  const first = list[0]
  const last = list[list.length - 1]
  if (clientY < first.r.top) return clientY > first.r.top - 28 ? above(0) : null
  if (clientY >= last.r.bottom) return clientY < last.r.bottom + 28 ? below(list.length - 1) : null
  for (let i = 0; i < list.length; i++) {
    const { r, no } = list[i]
    if (clientY < r.top || clientY >= r.bottom) continue
    if (clientY < r.top + EDGE) return above(i)
    if (clientY >= r.bottom - EDGE) return below(i)
    if (kind === 'video' && no === 0) return null
    return lockedNo(kind, no) ? null : { no, insert: false, y: 0 }
  }
  return null
}

/** 拖素材时看鼠标在哪：更新插入线，返回要不要挪轨道。 */
function follow(clientY: number, kind: EditTrackKind): Hit | null {
  const h = hitTrack(clientY, kind)
  insY.value = h?.insert ? h.y : null
  return h
}
/** 刀片：点在哪就在哪切开。 */
function blade(kind: 'clip' | 'overlay' | 'audio' | 'text', id: number, e: PointerEvent) {
  const t = Math.round(timeAt(e.clientX))
  if (!ed.bladeAt(kind, id, t)) ElMessage.info('这里切不开：离素材两端太近（至少 0.1 秒），或者是循环的配乐。')
}
function settleDrag(kind: 'overlay' | 'audio' | 'text', id: number, h: Hit | null, curTrack: number) {
  insY.value = null
  if (h?.insert) ed.moveToTrack(kind, id, h.no, true)
  else if (ed.crowded(kind, id)) ed.moveToTrack(kind, id, curTrack)
}

// ---------- 主轨片段 ----------

const dragIdx = ref(-1)
const dragDx = ref(0)
const dropAt = ref(-1)

function dropIndex(clientX: number, from: number): number {
  const tc = timeAt(clientX)
  const pl = ed.placed.value
  let n = 0
  pl.forEach((p, i) => {
    if (i !== from && (p.startMs + p.endMs) / 2 < tc) n++
  })
  return n
}
function dropX(idx: number, from: number): number {
  const others = ed.placed.value.filter((_, i) => i !== from)
  if (!others.length) return 0
  return idx < others.length ? px(others[idx].startMs) : px(others[others.length - 1].endMs)
}

function clipDown(e: PointerEvent, i: number) {
  const c = ed.project.value.clips[i]
  if (props.tool === 'blade' && !lockedNo('video', 0)) return blade('clip', c.id, e)
  if (shiftPick(e, 'clip', c.id)) return
  ed.sel.value = { kind: 'clip', id: c.id }
  if (props.tool === 'slip' && c.kind === 'video' && !c.freeze && !lockedNo('video', 0)) return slip(e, c)
  if (props.tool === 'slide' && !lockedNo('video', 0) && slide(e, c)) return
  const t0 = timeAt(e.clientX)
  if (lockedNo('video', 0)) return drag(e, () => {}, (moved) => !moved && seek(t0))
  drag(
    e,
    (dx, ev) => {
      dragIdx.value = i
      dragDx.value = dx
      dropAt.value = dropIndex(ev.clientX, i)
    },
    (moved) => {
      const to = dropAt.value
      const from = dragIdx.value
      dragIdx.value = -1
      dragDx.value = 0
      dropAt.value = -1
      if (!moved) seek(t0)
      else if (from >= 0 && to >= 0 && to !== from) ed.reorderClip(from, to)
    },
  )
}

/** 滑移：拖的是素材的取景窗口——画面在片段里前后挪，片段在时间线上的位置和长度都不变。 */
function slip(e: PointerEvent, c: EditClip) {
  const o = { i: c.inMs, o: c.outMs }
  const limit = ed.sources[c.path]?.durationMs ?? Infinity
  drag(e, (dx) => {
    const d = msOf(dx) * c.speed * (c.reverse ? 1 : -1)
    const shift = clamp(d, -o.i, limit - o.o)
    ed.patchClip(c.id, { inMs: Math.round(o.i + shift), outMs: Math.round(o.o + shift) }, `slip:${c.id}`)
  })
}

/** 拖动一对相邻的视频片段 a、b 之间的切点：a 的出点和 b 的入点一起前后挪，前一个多一点，后一个就少一点，总长不变。 */
function movePair(e: PointerEvent, a: EditClip, b: EditClip, selId: number) {
  const o = { ao: a.outMs, bi: b.inMs }
  const limitA = ed.sources[a.path]?.durationMs ?? Infinity
  ed.sel.value = { kind: 'clip', id: selId }
  drag(e, (dx) => {
    const r = movePairBy(a, b, o, msOf(dx), limitA)
    ed.patchClip(a.id, { outMs: r.aOut }, `roll:${a.id}`)
    ed.patchClip(b.id, { inMs: r.bIn }, `roll:${a.id}`)
  })
}
const rollable = (a: EditClip | undefined, b: EditClip | undefined): a is EditClip => !!a && !!b && a.kind === 'video' && b.kind === 'video' && !a.freeze && !b.freeze && !a.reverse && !b.reverse

/** 滚动：拖两个相邻片段之间的切点——前一个多一点，后一个就少一点，总长不变。 */
function roll(e: PointerEvent, c: EditClip, side: 'l' | 'r') {
  const list = ed.project.value.clips
  const i = list.findIndex((x) => x.id === c.id)
  const a = side === 'l' ? list[i - 1] : c
  const b = side === 'l' ? c : list[i + 1]
  if (!rollable(a, b)) return false
  movePair(e, a, b!, c.id)
  return true
}

/** 滑动：片段自己不变，在前后两个片段之间挪——前一个的出点和后一个的入点一起动，总长不变。头尾的片段（没有两个邻居）不能滑动。 */
function slide(e: PointerEvent, c: EditClip): boolean {
  const list = ed.project.value.clips
  const i = list.findIndex((x) => x.id === c.id)
  const a = list[i - 1]
  const b = list[i + 1]
  if (!rollable(a, b)) return false
  movePair(e, a, b, c.id)
  return true
}

function trimClip(e: PointerEvent, c: EditClip, side: 'l' | 'r') {
  if (props.tool === 'roll' && roll(e, c, side)) return
  const o = { inMs: c.inMs, outMs: c.outMs }
  const limit = ed.sources[c.path]?.durationMs ?? Infinity
  const minSpan = Math.max(MIN_MS, MIN_MS * c.speed)
  ed.sel.value = { kind: 'clip', id: c.id }
  if (lockedNo('video', 0)) return
  drag(e, (dx) => {
    const d = msOf(dx)
    if (c.kind === 'image') {
      const out = side === 'r' ? o.outMs + d : o.outMs - d
      ed.patchClip(c.id, { outMs: Math.round(Math.max(MIN_MS, out)) }, `trim:${c.id}`)
    } else if (c.freeze) {
      // 定格：拖的是停住多久
      const out = side === 'r' ? o.outMs + d : o.outMs - d
      ed.patchClip(c.id, { outMs: Math.round(Math.max(o.inMs + MIN_MS, out)) }, `trim:${c.id}`)
    } else if (c.reverse) {
      // 倒放：时间线的左边对应素材的结尾
      if (side === 'l') ed.patchClip(c.id, { outMs: Math.round(clamp(o.outMs - d * c.speed, o.inMs + minSpan, limit)) }, `trim:${c.id}`)
      else ed.patchClip(c.id, { inMs: Math.round(clamp(o.inMs - d * c.speed, 0, o.outMs - minSpan)) }, `trim:${c.id}`)
    } else if (side === 'l') {
      ed.patchClip(c.id, { inMs: Math.round(clamp(o.inMs + d * c.speed, 0, o.outMs - minSpan)) }, `trim:${c.id}`)
    } else {
      ed.patchClip(c.id, { outMs: Math.round(clamp(o.outMs + d * c.speed, o.inMs + minSpan, limit)) }, `trim:${c.id}`)
    }
  })
}

const thumbOf = (c: EditClip) => {
  const t = ed.sources[c.path]?.thumb
  return t ? `url("${convertFileSrc(t)}")` : 'none'
}
const clipName = (c: EditClip) => (c.compound !== undefined ? `▣ ${ed.project.value.compounds.find((x) => x.id === c.compound)?.name ?? '复合片段'}` : (ed.sources[c.path]?.name ?? c.path.split(/[\\/]/).pop() ?? c.path))
/** 双击复合片段：进去编辑它里面的内容。 */
function enterIfCompound(c: EditClip) {
  if (c.compound !== undefined) ed.enterCompound(c.id)
}
const isBroken = (path: string) => path in ed.broken

// ---------- 叠加素材 ----------

function overlayDown(e: PointerEvent, o: EditOverlay) {
  if (props.tool === 'blade' && !lockedNo('video', o.track)) return blade('overlay', o.id, e)
  if (shiftPick(e, 'overlay', o.id)) return
  if (!ed.isSelected('overlay', o.id) || !ed.extra.value.length) ed.sel.value = { kind: 'overlay', id: o.id }
  const t0 = timeAt(e.clientX)
  if (lockedNo('video', o.track)) return drag(e, () => {}, (moved) => !moved && seek(t0))
  const s0 = o.startMs
  const len = overlayDuration(o)
  const pts = snapPoints({ kind: 'overlay', id: o.id })
  let hit: Hit | null = null
  drag(
    e,
    (dx, ev) => {
      let s = Math.max(0, s0 + msOf(dx))
      const a = snapTo(s, pts)
      const b = snapTo(s + len, pts) - len
      s = a !== s ? a : b !== s ? b : s
      hit = follow(ev.clientY, 'video')
      const patch: Partial<EditOverlay> = { startMs: Math.max(0, Math.round(s)) }
      if (hit && !hit.insert) patch.track = hit.no
      ed.patchOverlay(o.id, patch, `omove:${o.id}`)
    },
    (moved) => {
      if (!moved) {
        // 没拖动：多选时点一下就只选这一项（同组的仍一起选中）
        if (ed.extra.value.length) ed.sel.value = { kind: 'overlay', id: o.id }
        return seek(t0)
      }
      settleDrag('overlay', o.id, hit, ed.overlayOf(o.id)?.track ?? o.track)
    },
  )
}

function overlayTrim(e: PointerEvent, o: EditOverlay, side: 'l' | 'r') {
  ed.sel.value = { kind: 'overlay', id: o.id }
  if (lockedNo('video', o.track)) return
  const o0 = { s: o.startMs, i: o.inMs, o: o.outMs, dur: overlayDuration(o) }
  const limit = o.adjust ? Infinity : overlaySourceLimit(o, ed.sources[o.path])
  const speed = o.kind === 'image' ? 1 : o.speed
  const minSpan = Math.max(MIN_MS, MIN_MS * speed)
  const pts = snapPoints({ kind: 'overlay', id: o.id })
  drag(e, (dx) => {
    if (side === 'r') {
      const end0 = o0.s + o0.dur
      const d = snapTo(end0 + msOf(dx), pts) - end0
      ed.patchOverlay(o.id, { outMs: Math.round(clamp(o0.o + d * speed, o0.i + minSpan, limit)) }, `otrim:${o.id}`)
      return
    }
    // 左边缘：开头往后收（或往前放开）。能从素材里多取一点的（没循环的视频）改入点，否则缩短出点
    const from = snapTo(o0.s + msOf(dx), pts) - o0.s
    const lowest = o.kind === 'video' && !o.looped ? -Math.min(o0.s, o0.i / speed) : -o0.s
    const delta = clamp(from, lowest, o0.dur - minSpan / speed)
    if (o.kind === 'video' && !o.looped) ed.patchOverlay(o.id, { startMs: Math.round(o0.s + delta), inMs: Math.round(o0.i + delta * speed) }, `otrim:${o.id}`)
    else ed.patchOverlay(o.id, { startMs: Math.round(o0.s + delta), outMs: Math.round(o0.o - delta * speed) }, `otrim:${o.id}`)
  })
}

const ovName = (o: EditOverlay) => (o.adjust ? '调整图层' : (ed.sources[o.path]?.name ?? o.path.split(/[\\/]/).pop() ?? o.path))
const ovThumb = (o: EditOverlay) => {
  const t = o.adjust ? undefined : ed.sources[o.path]?.thumb
  return t ? `url("${convertFileSrc(t)}")` : 'none'
}
/** 叠加素材条上的关键帧菱形（时间线上的位置，同一时刻不同属性只画一个） */
function keyMarks(o: EditOverlay): { key: number; t: number }[] {
  const ts = new Set<number>()
  for (const pts of Object.values(o.keyframes ?? {})) for (const k of pts) ts.add(k.tMs)
  return [...ts].sort((a, b) => a - b).map((t) => ({ key: t, t }))
}
/** 叠加素材超出主轨结尾的那段（像素宽度，没有超出返回 0）：成片里没有这一段 */
function cutW(o: EditOverlay): number {
  const total = ed.total.value
  if (!total) return 0
  return Math.max(0, px(Math.min(overlayEnd(o) - total, overlayDuration(o))))
}

// ---------- 文字 ----------

function textDown(e: PointerEvent, t: EditText) {
  if (props.tool === 'blade' && !lockedNo('text', t.track)) return blade('text', t.id, e)
  if (shiftPick(e, 'text', t.id)) return
  if (!ed.isSelected('text', t.id) || !ed.extra.value.length) ed.sel.value = { kind: 'text', id: t.id }
  const t0 = timeAt(e.clientX)
  if (lockedNo('text', t.track)) return drag(e, () => {}, (moved) => !moved && seek(t0))
  const o = { s: t.startMs, e: t.endMs }
  const pts = snapPoints({ kind: 'text', id: t.id })
  let hit: Hit | null = null
  drag(
    e,
    (dx, ev) => {
      const len = o.e - o.s
      let s = Math.max(0, o.s + msOf(dx))
      // 开头或结尾靠近别的边界时吸附过去
      const a = snapTo(s, pts)
      const b = snapTo(s + len, pts) - len
      s = a !== s ? a : b !== s ? b : s
      s = Math.max(0, Math.round(s))
      hit = follow(ev.clientY, 'text')
      const patch: Partial<EditText> = { startMs: s, endMs: s + len }
      if (hit && !hit.insert) patch.track = hit.no
      ed.patchText(t.id, patch, `tmove:${t.id}`)
    },
    (moved) => {
      if (!moved) {
        // 没拖动：多选时点一下就只选这一项（同组的仍一起选中）
        if (ed.extra.value.length) ed.sel.value = { kind: 'text', id: t.id }
        return seek(t0)
      }
      settleDrag('text', t.id, hit, ed.textOf(t.id)?.track ?? t.track)
    },
  )
}
function textTrim(e: PointerEvent, t: EditText, side: 'l' | 'r') {
  ed.sel.value = { kind: 'text', id: t.id }
  if (lockedNo('text', t.track)) return
  const o = { s: t.startMs, e: t.endMs }
  const pts = snapPoints({ kind: 'text', id: t.id })
  drag(e, (dx) => {
    const v = snapTo(Math.max(0, (side === 'l' ? o.s : o.e) + msOf(dx)), pts)
    if (side === 'l') ed.patchText(t.id, { startMs: Math.round(clamp(v, 0, o.e - MIN_MS)) }, `ttrim:${t.id}`)
    else ed.patchText(t.id, { endMs: Math.round(Math.max(o.s + MIN_MS, v)) }, `ttrim:${t.id}`)
  })
}

// ---------- 音频 ----------

const aSpan = (a: EditAudio) => audioSpan(a, ed.sources[a.path], ed.total.value)

function audioDown(e: PointerEvent, a: EditAudio) {
  if (props.tool === 'blade' && !lockedNo('audio', a.track)) return blade('audio', a.id, e)
  if (shiftPick(e, 'audio', a.id)) return
  if (!ed.isSelected('audio', a.id) || !ed.extra.value.length) ed.sel.value = { kind: 'audio', id: a.id }
  const t0 = timeAt(e.clientX)
  if (lockedNo('audio', a.track)) return drag(e, () => {}, (moved) => !moved && seek(t0))
  const s0 = a.startMs
  const pts = snapPoints({ kind: 'audio', id: a.id })
  let hit: Hit | null = null
  drag(
    e,
    (dx, ev) => {
      const s = Math.round(Math.max(0, snapTo(s0 + msOf(dx), pts)))
      hit = follow(ev.clientY, 'audio')
      const patch: Partial<EditAudio> = { startMs: s }
      if (hit && !hit.insert) patch.track = hit.no
      ed.patchAudio(a.id, patch, `amove:${a.id}`)
    },
    (moved) => {
      if (!moved) {
        // 没拖动：多选时点一下就只选这一项（同组的仍一起选中）
        if (ed.extra.value.length) ed.sel.value = { kind: 'audio', id: a.id }
        return seek(t0)
      }
      settleDrag('audio', a.id, hit, ed.audioOf(a.id)?.track ?? a.track)
    },
  )
}
function audioTrim(e: PointerEvent, a: EditAudio, side: 'l' | 'r') {
  ed.sel.value = { kind: 'audio', id: a.id }
  if (lockedNo('audio', a.track)) return
  const o = { s: a.startMs, i: a.inMs }
  const span = aSpan(a)
  const limit = ed.sources[a.path]?.durationMs ?? Infinity
  drag(e, (dx) => {
    const d = msOf(dx)
    if (side === 'l') {
      // 左边缘：开头往后收（或往前放开），素材的起点跟着变，声音不动
      const shift = clamp(d, -Math.min(o.s, o.i), span - MIN_MS)
      ed.patchAudio(a.id, { startMs: Math.round(o.s + shift), inMs: Math.round(o.i + shift), outMs: Math.round(o.i + span) }, `atrim:${a.id}`)
    } else {
      ed.patchAudio(a.id, { outMs: Math.round(clamp(o.i + span + d, o.i + MIN_MS, limit)), looped: false }, `atrim:${a.id}`)
    }
  })
}
const audioName = (a: EditAudio) => ed.sources[a.path]?.name ?? a.path.split(/[\\/]/).pop() ?? a.path



// ---------- 音频：波形和音量线 ----------

const waves = reactive<Record<string, string>>({})
const waving = new Set<string>()
const WAVE_H = 60
function waveUrl(a: EditAudio): string | undefined {
  if (!props.showWave || a.looped || isBroken(a.path) || !ed.sources[a.path]) return undefined
  const w = clamp(Math.round(px(aSpan(a)) / 100) * 100, 100, 4000)
  const to = a.outMs ?? a.inMs + aSpan(a)
  const key = `${a.path}|${a.inMs}|${to}|${w}`
  if (waves[key]) return waves[key]
  if (!waving.has(key)) {
    waving.add(key)
    api
      .editWaveform(a.path, a.inMs, to, w, WAVE_H)
      .then((f) => {
        if (f) waves[key] = convertFileSrc(f)
      })
      .catch(() => undefined) // 忽略：波形图是可选的
  }
  return undefined
}
const audioStyle = (a: EditAudio) => {
  const u = waveUrl(a)
  return { left: px(a.startMs) + 'px', width: Math.max(10, px(aSpan(a))) + 'px', backgroundImage: u ? `url("${u}")` : 'none' }
}

/** 音量线：0–200% 映射到轨道高度（100% 在正中）。有关键点就按关键点，没有就是一条横线。 */
const VOL_MAX = 2
const volY = (v: number, h: number) => (1 - clamp(v, 0, VOL_MAX) / VOL_MAX) * (h - 8) + 4
function volPoints(a: EditAudio, h: number): string {
  const w = Math.max(10, px(aSpan(a)))
  const pts = a.keyframes.volume
  if (!pts?.length) return `0,${volY(a.volume, h)} ${w},${volY(a.volume, h)}`
  const span = aSpan(a)
  const line = [`0,${volY(pts[0].value, h)}`, ...pts.map((k) => `${px(k.tMs)},${volY(k.value, h)}`)]
  const lastT = pts[pts.length - 1].tMs
  if (lastT < span) line.push(`${w},${volY(pts[pts.length - 1].value, h)}`)
  return line.join(' ')
}
const volVisible = (r: Row) => props.showVol && r.meta.height !== 's'

function volDown(e: PointerEvent, a: EditAudio, h: number) {
  if (shiftPick(e, 'audio', a.id)) return
  // 点在线上：在这个时间加一个点，并且可以接着拖
  ed.sel.value = { kind: 'audio', id: a.id }
  if (lockedNo('audio', a.track)) return
  const box = (e.currentTarget as SVGElement).getBoundingClientRect()
  const tAt = (cx: number) => Math.round(clamp(msOf(cx - box.left), 0, aSpan(a)))
  const vAt = (cy: number) => clamp((1 - (cy - box.top - 4) / (h - 8)) * VOL_MAX, 0, VOL_MAX)
  const t0 = tAt(e.clientX)
  const cur = kfEval(a.keyframes.volume, t0) ?? a.volume
  ed.setKey('audio', a.id, 'volume', t0, Math.round(cur * 100) / 100, undefined, `vol:${a.id}:${t0}`)
  dragPoint(e, a, t0, tAt, vAt)
}
function dragPoint(e: PointerEvent, a: EditAudio, startT: number, tAt: (x: number) => number, vAt: (y: number) => number) {
  let t = startT
  drag(e, (_dx, ev) => {
    const nt = tAt(ev.clientX)
    const v = Math.round(vAt(ev.clientY) * 100) / 100
    if (nt !== t) {
      ed.deleteKey('audio', a.id, 'volume', t)
      t = nt
    }
    ed.setKey('audio', a.id, 'volume', t, v, undefined, `vol:${a.id}`)
  })
}
function volPointDown(e: PointerEvent, a: EditAudio, tMs: number, h: number) {
  ed.sel.value = { kind: 'audio', id: a.id }
  if (lockedNo('audio', a.track)) return
  const box = (e.currentTarget as SVGElement).ownerSVGElement!.getBoundingClientRect()
  dragPoint(
    e,
    a,
    tMs,
    (cx) => Math.round(clamp(msOf(cx - box.left), 0, aSpan(a))),
    (cy) => clamp((1 - (cy - box.top - 4) / (h - 8)) * VOL_MAX, 0, VOL_MAX),
  )
}
function volPointDelete(a: EditAudio, tMs: number) {
  if (!lockedNo('audio', a.track)) ed.deleteKey('audio', a.id, 'volume', tMs)
}

// ---------- 轨道头：开关、菜单、类型 ----------

const heads = new Map<string, InstanceType<typeof TrackHead>>()
const setHead = (key: string) => (el: unknown) => {
  if (el) heads.set(key, el as InstanceType<typeof TrackHead>)
  else heads.delete(key)
}

const menu = ref<{ x: number; y: number; title?: string; items: MenuItem[] } | null>(null)

function setTrack(r: Row, patch: Partial<EditTrack>) {
  ed.patchTrack(r.kind, r.no, patch)
}

async function confirmRemove(r: Row) {
  const n = r.kind === 'video' ? P.value.overlays.filter((o) => o.track === r.no).length : r.kind === 'audio' ? audioOn(r.no).length : textsOn(r.no).length
  if (n > 0) {
    try {
      await ElMessageBox.confirm(`${trackCode(r.kind, r.no)} 上有 ${n} 个素材，删除轨道会一起删掉。可以用“撤销”找回。`, '删除轨道', { confirmButtonText: '删除', cancelButtonText: '取消', type: 'warning' })
    } catch {
      return
    }
  }
  ed.removeTrack(r.kind, r.no)
}

function roleItems(r: Row): MenuItem[] {
  return [
    ...TRACK_ROLES.map((x) => ({ label: x.name, desc: x.desc, checked: r.meta.role === x.id, run: () => void changeRole(r, x.id) })),
    { divider: true, label: '' },
    { label: '不设类型', checked: !r.meta.role, run: () => void changeRole(r, '') },
  ]
}

async function changeRole(r: Row, role: EditTrackRole) {
  if (role === 'adjust' && P.value.overlays.some((o) => o.track === r.no && !o.adjust)) {
    try {
      await ElMessageBox.confirm(`${trackCode('video', r.no)} 上已经有素材。改成“调整图层”会把它们删掉（可以撤销）。`, '改成调整图层', { confirmButtonText: '清掉并改成调整图层', cancelButtonText: '取消', type: 'warning' })
    } catch {
      return
    }
    ed.clearVideoTrack(r.no)
  }
  ed.setTrackRole(r.no, role)
  if (role === 'adjust' && !P.value.overlays.some((o) => o.track === r.no)) ed.addAdjust(r.no)
}

function typeMenu(e: MouseEvent, r: Row) {
  const b = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu.value = { x: b.left, y: b.bottom + 4, title: `${trackCode(r.kind, r.no)} 轨道类型`, items: roleItems(r) }
}

function rowMenu(e: MouseEvent, r: Row) {
  const k = r.kind
  const code = trackCode(k, r.no)
  const noun = k === 'video' ? '视频轨' : k === 'audio' ? '音频轨' : '文字轨'
  const full = roomFor(P.value, k) <= 0
  const lowest = k === 'video' ? 1 : 1
  const items: MenuItem[] = [{ label: '重命名', run: () => heads.get(r.key)?.rename() }]
  if (k === 'video' && !r.main) items.push({ label: r.meta.magnet ? '关闭磁吸' : '打开磁吸', desc: '素材首尾相接，删除、插入时后面的跟着动', run: () => setTrack(r, { magnet: !r.meta.magnet }) })
  if (k === 'audio') items.push({ label: r.meta.duck ? '关闭自动避让' : '自动避让', desc: '主轨有声音（人声）时，自动压低这条轨上的所有音频', run: () => setTrack(r, { duck: !r.meta.duck }) })
  items.push({ divider: true, label: '' })
  const upNo = k === 'audio' ? r.no : r.no + 1
  const downNo = k === 'audio' ? r.no + 1 : r.no
  items.push(
    { label: `在上方插入${noun}`, disabled: full || (k === 'video' && r.main && false), run: () => ed.insertTrack(k, upNo) ?? ElMessage.warning(`${noun}最多 ${maxNo(k) + (k === 'video' ? 1 : 0)} 条`) },
    { label: `在下方插入${noun}`, disabled: full || (k === 'video' && r.main), run: () => ed.insertTrack(k, Math.max(lowest, downNo)) ?? ElMessage.warning(`${noun}最多 ${maxNo(k) + (k === 'video' ? 1 : 0)} 条`) },
  )
  if (!r.main) {
    const nos = shownNos(P.value, k)
    const idx = nos.indexOf(r.no)
    const upOk = k === 'audio' ? idx > 0 : idx < nos.length - 1
    const downOk = k === 'audio' ? idx < nos.length - 1 : idx > (k === 'video' ? 1 : 0)
    const step = (dir: 1 | -1) => () => {
      const to = k === 'audio' ? nos[idx + (dir === 1 ? -1 : 1)] : nos[idx + dir]
      if (to !== undefined) swapWith(r, to)
    }
    items.push({ label: '上移一条', disabled: !upOk, run: step(1) }, { label: '下移一条', disabled: !downOk, run: step(-1) })
  }
  items.push({ divider: true, label: '' })
  for (const [h, name] of [['s', '小'], ['m', '中'], ['l', '大']] as const) items.push({ label: `轨道高度：${name}`, checked: r.meta.height === h, run: () => setTrack(r, { height: h }) })
  items.push({ divider: true, label: '' }, { label: '选中这条轨道上的全部素材', run: () => selectTrack(r), disabled: !hasItems(r) })
  if (!r.main) items.push({ label: `删除 ${code}…`, danger: true, run: () => void confirmRemove(r) })
  menu.value = { x: e.clientX, y: e.clientY, title: `${code} ${trackTitle(r.meta)}`, items }
}

function swapWith(r: Row, to: number) {
  // 对调两条轨道：设置和素材一起换位置
  ed.mutate((p) => swapTracks(p, r.kind, r.no, to))
}
const hasItems = (r: Row) => (r.kind === 'video' ? (r.main ? P.value.clips.length > 0 : P.value.overlays.some((o) => o.track === r.no)) : r.kind === 'audio' ? audioOn(r.no).length > 0 : textsOn(r.no).length > 0)
function selectTrack(r: Row) {
  const first = r.kind === 'video' ? (r.main ? P.value.clips[0] : P.value.overlays.find((o) => o.track === r.no)) : r.kind === 'audio' ? audioOn(r.no)[0] : textsOn(r.no)[0]
  if (first) ed.sel.value = { kind: r.kind === 'video' ? (r.main ? 'clip' : 'overlay') : r.kind, id: first.id }
}

const addMenu = (e: MouseEvent) => {
  const b = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu.value = {
    x: b.left,
    y: b.bottom + 4,
    title: '新建视频轨',
    items: [
      ...TRACK_ROLES.map((x) => ({
        label: x.name,
        desc: x.desc,
        run: () => {
          const no = ed.addTrack('video', x.id)
          if (no === null) ElMessage.warning('视频轨最多 12 条（含主轨）')
          else if (x.id === 'adjust') ed.addAdjust(no)
        },
      })),
    ],
  }
}
function addPlain(kind: EditTrackKind) {
  if (ed.addTrack(kind) === null) ElMessage.warning(kind === 'audio' ? '音频轨最多 12 条' : '文字轨最多 8 条')
}
const canAdd = (kind: EditTrackKind) => nextNo(P.value, kind) !== null


// ---------- 混音器 ----------

const mixerOpen = ref(false)
const audioTracks = computed(() => shownNos(P.value, 'audio').map((no) => ({ no, meta: metaOf('audio', no), n: audioOn(no).length })))
const hasSolo = computed(() => P.value.tracks.some((t) => t.kind === 'audio' && t.solo))
/** 电平表：dBFS 转成 0–100 的高度（−60 以下为 0，0 dBFS 为满）。 */
const meterPct = (db: number | undefined) => Math.round(Math.max(0, Math.min(100, ((db ?? -90) + 60) * (100 / 60))))
const meterClass = (key: string) => {
  const db = ed.levels[key] ?? -90
  return { hot: db >= -6 && db < 0, clip: Date.now() - (ed.clips[key] ?? 0) < 2000 }
}
const dB = (v: number) => (v < 0.001 ? '−∞' : `${(20 * Math.log10(v)).toFixed(1)} dB`)

// ---------- 轨道模板 ----------

const templates = ref(loadTemplates())
function templateMenu(e: MouseEvent) {
  const b = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const apply = (tracks: EditTrack[], name: string) => () => {
    const n = ed.applyTrackTemplate(tracks)
    ElMessage.success(`已套用“${name}”：${n} 条轨道`)
  }
  const items: MenuItem[] = [
    ...BUILTIN_TEMPLATES.map((x) => ({ label: x.name, desc: x.desc, run: apply(x.tracks, x.name) })),
    ...(templates.value.length ? [{ divider: true, label: '' } as MenuItem, ...templates.value.map((x) => ({ label: x.name, desc: x.desc, run: apply(x.tracks, x.name) }))] : []),
    { divider: true, label: '' },
    { label: '把当前轨道设置存成模板…', run: () => void saveTemplate() },
    ...templates.value.map((x) => ({ label: `删除模板“${x.name}”`, danger: true, run: () => removeTemplate(x.id) })),
  ]
  menu.value = { x: b.left, y: b.bottom + 4, title: '轨道模板', items }
}
async function saveTemplate() {
  try {
    const { value } = await ElMessageBox.prompt('给这套轨道设置起个名字（素材不会存进模板）', '存成模板', { inputValue: '我的模板', confirmButtonText: '保存', cancelButtonText: '取消' })
    templates.value = [...templates.value, templateFrom(value, P.value.tracks)]
    saveTemplates(templates.value)
    ElMessage.success('已存成模板')
  } catch {
    /* 取消 */
  }
}
function removeTemplate(id: string) {
  templates.value = templates.value.filter((x) => x.id !== id)
  saveTemplates(templates.value)
}

// ---------- 标记、入点出点 ----------

const MARK_COLOR: Record<EditMarker['color'], string> = { red: '#e5484d', orange: '#d9762b', yellow: '#d9a21a', green: '#2f8a57', blue: '#2457e6', purple: '#7c4dcc' }
const markX = (m: EditMarker) => px(m.ms)

async function renameMarker(m: EditMarker) {
  try {
    const { value } = await ElMessageBox.prompt('给这个标记写一句备注（可以留空）', `标记 ${tickLabel(m.ms, 1000)}`, { inputValue: m.name, confirmButtonText: '确定', cancelButtonText: '取消' })
    ed.patchMarker(m.id, { name: value.trim() })
  } catch {
    /* 取消 */
  }
}
function markerMenu(e: MouseEvent, m: EditMarker) {
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    title: m.name || `标记 ${tickLabel(m.ms, 1000)}`,
    items: [
      { label: '跳到这里', run: () => seek(m.ms) },
      { label: '写备注…', run: () => void renameMarker(m) },
      { divider: true, label: '' },
      ...(Object.keys(MARK_COLOR) as EditMarker['color'][]).map((c) => ({ label: { red: '红', orange: '橙', yellow: '黄', green: '绿', blue: '蓝', purple: '紫' }[c] + '色', checked: m.color === c, run: () => ed.patchMarker(m.id, { color: c }) })),
      { divider: true, label: '' },
      { label: '删除标记', danger: true, run: () => ed.removeMarker(m.id) },
    ],
  }
}
function markerDown(e: PointerEvent, m: EditMarker) {
  seek(m.ms)
  const x0 = e.clientX
  const ms0 = m.ms
  drag(e, (dx) => ed.patchMarker(m.id, { ms: Math.max(0, Math.round(ms0 + msOf(dx))) }), () => void x0)
}

const rangeOn = computed(() => P.value.out.fromMs > 0 || P.value.out.toMs > 0)
const rangeTo = computed(() => (P.value.out.toMs > 0 ? P.value.out.toMs : ed.total.value))
function rangeDrag(e: PointerEvent, side: 'in' | 'out') {
  drag(e, (_dx, ev) => {
    const t = Math.round(timeAt(ev.clientX))
    if (side === 'in') ed.setIn(Math.min(t, Math.max(0, rangeTo.value - MIN_MS)))
    else ed.setOut(t)
  })
}

// ---------- 小地图和“适应” ----------

const scrollX = ref(0)
const viewW = ref(600)
function onScroll() {
  scrollX.value = scroller.value?.scrollLeft ?? 0
}
let ro: ResizeObserver | null = null
onMounted(() => {
  if (!scroller.value) return
  viewW.value = scroller.value.clientWidth
  ro = new ResizeObserver(() => {
    viewW.value = scroller.value?.clientWidth ?? viewW.value
  })
  ro.observe(scroller.value)
})
onBeforeUnmount(() => ro?.disconnect())

const MINI_H = 26
const miniScale = computed(() => (viewW.value - 24) / Math.max(1, LABEL_W + contentW.value))
const miniItems = computed(() => {
  const p = P.value
  const out: { l: number; w: number; t: number; c: string }[] = []
  const add = (s: number, e: number, row: number, c: string) => out.push({ l: px(s) * miniScale.value, w: Math.max(1.5, px(e - s) * miniScale.value), t: 3 + row * 7, c })
  p.clips.forEach((_, i) => add(ed.placed.value[i].startMs, ed.placed.value[i].endMs, 0, '#3f6f5e'))
  p.overlays.forEach((o) => add(o.startMs, overlayEnd(o), 1, o.adjust ? '#b4561a' : '#2f6f8f'))
  p.audio.forEach((a) => add(a.startMs, a.startMs + aSpan(a), 2, '#b9812d'))
  p.texts.forEach((t) => add(t.startMs, t.endMs, 3, '#4f7cc9'))
  return out
})
const miniView = computed(() => ({ left: scrollX.value * miniScale.value + 'px', width: Math.max(8, (viewW.value - LABEL_W) * miniScale.value) + 'px' }))
function miniDown(e: PointerEvent) {
  const el = scroller.value
  const box = (e.currentTarget as HTMLElement).getBoundingClientRect()
  if (!el) return
  const go = (cx: number) => {
    el.scrollLeft = Math.max(0, (cx - box.left) / miniScale.value - (viewW.value - LABEL_W) / 2)
  }
  go(e.clientX)
  drag(e, (_dx, ev) => go(ev.clientX))
}
/** 把整条时间线缩到刚好放进窗口 */
function fit() {
  const span = Math.max(1000, ed.extent.value + 1000)
  const next = Math.round(clamp(((viewW.value - LABEL_W - 24) / span) * 1000, 5, 400))
  emit('zoom', next)
  nextTick(() => {
    if (scroller.value) scroller.value.scrollLeft = 0
  })
}

// ---------- 跟随播放 ----------

watch(
  () => ed.playhead.value,
  async (p) => {
    const el = scroller.value
    if (!el || !ed.playing.value) return
    const x = px(p) + LABEL_W
    if (x < el.scrollLeft + LABEL_W || x > el.scrollLeft + el.clientWidth - 40) {
      await nextTick()
      el.scrollLeft = Math.max(0, x - el.clientWidth / 3)
    }
  },
)

// ---------- Ctrl + 滚轮缩放 ----------

/** 按住 Ctrl（Mac 上是 ⌘）滚滚轮缩放时间线，鼠标指着的那个时刻保持在原地。 */
function onWheel(e: WheelEvent) {
  if (!e.ctrlKey && !e.metaKey) return
  e.preventDefault()
  const el = scroller.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const x = e.clientX - rect.left
  const anchor = msOf(x + el.scrollLeft - LABEL_W)
  const factor = e.deltaY < 0 ? 1.18 : 1 / 1.18
  let next = Math.round(clamp(props.pps * factor, 5, 400))
  if (next === props.pps) next = clamp(props.pps + (e.deltaY < 0 ? 1 : -1), 5, 400)
  if (next === props.pps) return
  emit('zoom', next)
  nextTick(() => {
    el.scrollLeft = Math.max(0, px(anchor) + LABEL_W - x)
  })
}

// ---------- 从资源管理器拖文件进来 ----------

export interface DropTarget {
  lane: 'main' | 'overlay' | 'audio' | 'text'
  /** 轨道编号（叠加轨 / 音频轨 / 文字轨） */
  track?: number
  /** 落点的时间（毫秒） */
  ms: number
  /** 落在主轨第几个位置之前（lane 为 main 时） */
  index?: number
}

/** 这个屏幕位置（CSS 像素）落在时间线的哪一行、哪个时间。不在时间线里返回 null。 */
function targetAt(clientX: number, clientY: number): DropTarget | null {
  const box = scroller.value?.getBoundingClientRect()
  if (!box || clientX < box.left || clientX > box.right || clientY < box.top || clientY > box.bottom) return null
  const row = (document.elementFromPoint(clientX, clientY) as HTMLElement | null)?.closest<HTMLElement>('.row[data-kind]')
  if (!row || clientX < box.left + LABEL_W - scroller.value!.scrollLeft) return null
  const kind = row.dataset.kind as EditTrackKind
  const no = Number(row.dataset.no)
  const ms = Math.round(timeAt(clientX))
  if (kind === 'text' || lockedNo(kind, no)) return null
  if (kind === 'video' && no === 0) {
    const index = ed.placed.value.filter((p) => (p.startMs + p.endMs) / 2 < ms).length
    return { lane: 'main', ms, index }
  }
  return { lane: kind === 'video' ? 'overlay' : kind, track: no, ms }
}

/** 正在拖文件时高亮落点所在的行。 */
const hot = ref('')
function hover(clientX: number | null, clientY = 0) {
  const t = clientX === null ? null : targetAt(clientX, clientY)
  hot.value = t ? (t.lane === 'main' ? 'video0' : `${t.lane === 'overlay' ? 'video' : t.lane}${t.track}`) : ''
}

/** 滚到让播放位置可见（跳到某处时用）。 */
function reveal() {
  const el = scroller.value
  if (!el) return
  const x = px(ed.playhead.value) + LABEL_W
  if (x < el.scrollLeft + LABEL_W || x > el.scrollLeft + el.clientWidth - 40) el.scrollLeft = Math.max(0, x - el.clientWidth / 2)
}
defineExpose({ reveal, targetAt, hover, fit })
</script>

<template>
  <div class="tlw">
    <div class="tbar">
      <button type="button" class="ab" :disabled="!canAdd('video')" @click="addMenu">＋ 视频轨 ▾</button>
      <button type="button" class="ab" :disabled="!canAdd('audio')" @click="addPlain('audio')">＋ 音频轨</button>
      <button type="button" class="ab" :disabled="!canAdd('text')" @click="addPlain('text')">＋ 文字轨</button>
      <button type="button" class="ab" :class="{ on: mixerOpen }" :aria-pressed="mixerOpen" title="每条音频轨的音量、左右声道、静音、独奏、自动避让" @click="mixerOpen = !mixerOpen">混音器</button>
      <button type="button" class="ab" title="套用或保存一套轨道设置" @click="templateMenu">轨道模板 ▾</button>
      <span class="sp" />
      <span v-if="rangeOn" class="rng">入点 {{ tickLabel(ed.project.value.out.fromMs, 1000) }} · 出点 {{ ed.project.value.out.toMs ? tickLabel(ed.project.value.out.toMs, 1000) : '结尾' }}<button type="button" class="lk" @click="ed.clearRange()">清除</button></span>
      <button type="button" class="ab" title="把整条时间线缩到刚好放进窗口" @click="fit">适应</button>
    </div>


    <div v-if="ed.compoundStack.value.length" class="crumb" role="navigation" aria-label="复合片段层级">
      <button type="button" class="b" title="回到外面一层" @click="ed.exitCompound()">‹ 返回</button>
      <span class="mute">主时间线</span>
      <template v-for="(n, i) in ed.compoundPath.value" :key="i">
        <span class="mute">›</span>
        <b :class="{ cur: i === ed.compoundPath.value.length - 1 }">{{ n }}</b>
      </template>
      <span class="mute small">正在编辑复合片段里面的内容；导出、预览用的是放回去以后的整个工程</span>
    </div>
    <div v-if="mixerOpen" class="mixer" role="group" aria-label="混音器">
      <div v-for="t in audioTracks" :key="t.no" class="ch" :class="{ off: t.meta.mute || (hasSolo && !t.meta.solo) }">
        <div class="cn"><b>{{ trackCode('audio', t.no) }}</b><span class="ellipsis">{{ trackTitle(t.meta) }}</span><span v-if="ed.meterOn.value" class="meter" :class="meterClass('a' + t.no)" role="meter" :aria-valuenow="Math.round(ed.levels['a' + t.no] ?? -90)" aria-valuemin="-90" aria-valuemax="0" :title="`${Math.round(ed.levels['a' + t.no] ?? -90)} dBFS`"><i :style="{ width: meterPct(ed.levels['a' + t.no]) + '%' }" /></span></div>
        <div class="cb">
          <button type="button" class="b" :class="{ on: t.meta.mute }" :aria-pressed="t.meta.mute" title="静音" @click="ed.patchTrack('audio', t.no, { mute: !t.meta.mute })">M</button>
          <button type="button" class="b solo" :class="{ on: t.meta.solo }" :aria-pressed="t.meta.solo" title="独奏（只影响预览）" @click="ed.patchTrack('audio', t.no, { solo: !t.meta.solo })">S</button>
        </div>
        <label class="sl"><span>音量</span><el-slider :model-value="t.meta.volume" :min="0" :max="2" :step="0.01" :show-tooltip="false" size="small" @update:model-value="(v: number | number[]) => ed.patchTrack('audio', t.no, { volume: v as number }, `mx:${t.no}`)" /><em>{{ dB(t.meta.volume) }}</em></label>
        <label class="sl"><span>声道</span><el-slider :model-value="t.meta.pan" :min="-1" :max="1" :step="0.01" :show-tooltip="false" size="small" @update:model-value="(v: number | number[]) => ed.patchTrack('audio', t.no, { pan: v as number }, `mp:${t.no}`)" /><em>{{ t.meta.pan === 0 ? '居中' : t.meta.pan < 0 ? `左 ${Math.round(-t.meta.pan * 100)}` : `右 ${Math.round(t.meta.pan * 100)}` }}</em></label>
        <el-checkbox :model-value="t.meta.duck" size="small" title="主轨有声音（人声）时，自动压低这条轨上的所有音频" @update:model-value="(v: unknown) => ed.patchTrack('audio', t.no, { duck: !!v })">自动避让</el-checkbox>
        <span class="cc">{{ t.n }} 段</span>
      </div>
      <div class="mout">
        <el-checkbox v-model="ed.meterOn.value" size="small" title="实验功能：预览的声音先经过 Web Audio 量电平再出来。改动后要重新打开剪辑页才生效；如果读不到声音会自动关掉。">电平表（实验，重新打开剪辑页后生效）</el-checkbox>
        <span v-if="ed.meterOn.value" class="mo"><b>主输出</b><span class="meter" :class="meterClass('out')" role="meter" :aria-valuenow="Math.round(ed.levels.out ?? -90)" aria-valuemin="-90" aria-valuemax="0"><i :style="{ width: meterPct(ed.levels.out) + '%' }" /></span><em>{{ Math.round(ed.levels.out ?? -90) <= -90 ? '−∞' : Math.round(ed.levels.out ?? -90) + ' dBFS' }}</em></span>
      </div>
      <p class="mnote">独奏只影响预览，导出不受影响；静音的轨道预览和导出都没有声音。整体响度在“输出”页的“响度标准化”里设。</p>
    </div>
    <div ref="scroller" class="tl" :class="{ blade: tool === 'blade' }" @pointerdown.capture="blurInput" @wheel="onWheel" @scroll.passive="onScroll">
      <div ref="bodyEl" class="body" :style="{ width: LABEL_W + contentW + 'px' }">
        <!-- 标尺 -->
        <div class="row ruler-row">
          <div class="lab">
            <span class="mk-cnt">{{ ed.project.value.markers.length ? `标记 ${ed.project.value.markers.length}` : '' }}</span>
          </div>
          <div ref="rulerLane" class="lane ruler" data-lane="ruler" :style="{ width: contentW + 'px' }" @pointerdown.prevent="scrub">
            <span v-if="rangeOn" class="range" :style="{ left: px(ed.project.value.out.fromMs) + 'px', width: Math.max(2, px(rangeTo - ed.project.value.out.fromMs)) + 'px' }">
              <i class="rh l" title="入点" @pointerdown.stop.prevent="rangeDrag($event, 'in')" />
              <i class="rh r" title="出点" @pointerdown.stop.prevent="rangeDrag($event, 'out')" />
            </span>
            <span v-for="t in ticks" :key="t.ms" class="tick" :style="{ left: t.x + 'px' }">{{ t.label }}</span>
            <button
              v-for="m in ed.project.value.markers"
              :key="m.id"
              type="button"
              class="mk"
              :style="{ left: markX(m) + 'px', '--mc': MARK_COLOR[m.color] }"
              :title="(m.name || '标记') + ' · ' + tickLabel(m.ms, 1000) + '（拖动移动，右键更多，双击写备注）'"
              @pointerdown.stop.prevent="markerDown($event, m)"
              @dblclick.stop.prevent="renameMarker(m)"
              @contextmenu.stop.prevent="markerMenu($event, m)"
            >
              <span v-if="m.name" class="mt">{{ m.name }}</span>
            </button>
          </div>
        </div>

        <div v-for="r in rows" :key="r.key" class="row" :class="[r.kind, { hot: hot === r.key, locked: r.meta.lock, off: r.meta.hidden, main: r.main }]" :data-kind="r.kind" :data-no="r.no">
          <div class="lab" :style="{ height: rowH(r) + 'px' }">
            <TrackHead :ref="setHead(r.key)" :meta="r.meta" :main="r.main" @patch="setTrack(r, $event)" @menu="rowMenu($event, r)" @type="typeMenu($event, r)" />
          </div>

          <!-- 文字轨 -->
          <div v-if="r.kind === 'text'" class="lane" data-lane="text" :data-track="r.no" :style="{ width: contentW + 'px', height: rowH(r) + 'px' }" @pointerdown.prevent="bgDown">
            <div v-if="r.no === 1 && !ed.project.value.texts.length" class="empty">点上方“文字”，或导入字幕，文字会出现在这里</div>
            <div
              v-for="t in textsOn(r.no)"
              :key="t.id"
              class="blk txt"
              :class="{ sel: isSel('text', t.id), grouped: groupNo(t) !== undefined }"
              :style="{ left: px(t.startMs) + 'px', width: Math.max(8, px(t.endMs - t.startMs)) + 'px' }"
              :title="t.text"
              @pointerdown.stop.prevent="textDown($event, t)"
            >
              <i class="h l" @pointerdown.stop.prevent="textTrim($event, t, 'l')" />
              <span v-if="t.text" class="ellipsis" data-no-i18n>{{ t.text }}</span>
              <span v-else class="ellipsis">（空）</span>
              <i class="h r" @pointerdown.stop.prevent="textTrim($event, t, 'r')" />
            </div>
          </div>

          <!-- 主轨 V1 -->
          <div v-else-if="r.main" class="lane" data-lane="main" :style="{ width: contentW + 'px', height: rowH(r) + 'px' }" @pointerdown.prevent="bgDown">
            <div v-if="!ed.project.value.clips.length" class="empty">点上方“添加素材”，或把视频、图片拖到这里</div>
            <div
              v-for="(c, i) in ed.project.value.clips"
              :key="c.id"
              class="blk clip"
              :class="{ sel: isSel('clip', c.id), grouped: groupNo(c) !== undefined, cmp: c.compound !== undefined, img: c.kind === 'image', bad: isBroken(c.path), moving: dragIdx === i }"
              :style="{
                left: px(ed.placed.value[i].startMs) + 'px',
                width: Math.max(10, px(ed.placed.value[i].durMs)) + 'px',
                backgroundImage: thumbOf(c),
                transform: dragIdx === i ? `translateX(${dragDx}px)` : undefined,
                zIndex: dragIdx === i ? 5 : i + 1,
              }"
              :title="clipName(c)"
              @pointerdown.stop.prevent="clipDown($event, i)"
              @dblclick.stop.prevent="enterIfCompound(c)"
            >
              <span v-if="ed.placed.value[i].overlapMs > 0" class="xf" :style="{ width: px(ed.placed.value[i].overlapMs) + 'px' }" title="转场" />
              <i class="h l" @pointerdown.stop.prevent="trimClip($event, c, 'l')" />
              <span class="cap ellipsis">{{ isBroken(c.path) ? '找不到文件 · ' : '' }}{{ c.freeze ? '❄ 定格 · ' : c.reverse ? '⏪ 倒放 · ' : '' }}{{ clipName(c) }}</span>
              <i class="h r" @pointerdown.stop.prevent="trimClip($event, c, 'r')" />
            </div>
            <div v-if="dropAt >= 0" class="drop" :style="{ left: dropX(dropAt, dragIdx) + 'px' }" />
          </div>

          <!-- 视频叠加轨 V2… -->
          <div v-else-if="r.kind === 'video'" class="lane" data-lane="overlay" :data-track="r.no" :style="{ width: contentW + 'px', height: rowH(r) + 'px' }" @pointerdown.prevent="bgDown">
            <div v-if="!(byTrack.get(r.no) ?? []).length" class="empty">{{ r.meta.role === 'adjust' ? '调整图层：点轨道头的菜单加一段，或右键轨道头' : r.meta.role === 'chroma' ? '绿幕素材放这里，默认抠像' : '把视频、图片、GIF 拖到这里（画中画、贴纸、水印）' }}</div>
            <div
              v-for="x in byTrack.get(r.no) ?? []"
              :key="x.o.id"
              class="blk ovl"
              :class="{ sel: isSel('overlay', x.o.id), grouped: groupNo(x.o) !== undefined, img: x.o.kind === 'image', bad: isBroken(x.o.path), adj: x.o.adjust }"
              :style="{
                left: px(x.o.startMs) + 'px',
                width: Math.max(10, px(overlayDuration(x.o))) + 'px',
                top: x.lane * laneH(r) + 2 + 'px',
                height: laneH(r) - 4 + 'px',
                bottom: 'auto',
                backgroundImage: ovThumb(x.o),
              }"
              :title="ovName(x.o)"
              @pointerdown.stop.prevent="overlayDown($event, x.o)"
            >
              <i class="h l" @pointerdown.stop.prevent="overlayTrim($event, x.o, 'l')" />
              <span class="cap ellipsis">{{ !x.o.adjust && isBroken(x.o.path) ? '找不到文件 · ' : '' }}{{ ovName(x.o) }}</span>
              <i v-for="k in keyMarks(x.o)" :key="k.key" class="kfm" :style="{ left: px(k.t) + 'px' }" />
              <span v-if="cutW(x.o) > 0" class="cut" :style="{ width: cutW(x.o) + 'px' }" title="超出了主轨的结尾，成片里没有这一段" />
              <i class="h r" @pointerdown.stop.prevent="overlayTrim($event, x.o, 'r')" />
            </div>
          </div>

          <!-- 音频轨 -->
          <div v-else class="lane" data-lane="audio" :data-track="r.no" :style="{ width: contentW + 'px', height: rowH(r) + 'px' }" @pointerdown.prevent="bgDown">
            <div v-if="r.no === 1 && !ed.project.value.audio.length" class="empty">点上方“添加素材”选配乐，或把音频拖到这里</div>
            <div
              v-for="a in audioOn(r.no)"
              :key="a.id"
              class="blk aud"
              :class="{ sel: isSel('audio', a.id), grouped: groupNo(a) !== undefined, bad: isBroken(a.path), wave: !!waveUrl(a) }"
              :style="audioStyle(a)"
              :title="audioName(a)"
              @pointerdown.stop.prevent="audioDown($event, a)"
            >
              <i class="h l" @pointerdown.stop.prevent="audioTrim($event, a, 'l')" />
              <i v-for="b in beatsOnTimeline(a)" :key="b" class="bt" :style="{ left: px(b - a.startMs) + 'px' }" />
              <span class="cap ellipsis">{{ audioName(a) }}{{ a.duck || r.meta.duck ? ' · 自动避让' : '' }}</span>
              <svg v-if="volVisible(r)" class="vol" :width="Math.max(10, px(aSpan(a)))" :height="rowH(r)" :viewBox="`0 0 ${Math.max(10, px(aSpan(a)))} ${rowH(r)}`">
                <polyline class="vl" :points="volPoints(a, rowH(r))" />
                <polyline class="vh" :points="volPoints(a, rowH(r))" @pointerdown.stop.prevent="volDown($event, a, rowH(r))" />
                <circle v-for="k in a.keyframes.volume ?? []" :key="k.tMs" class="vp" :cx="px(k.tMs)" :cy="volY(k.value, rowH(r))" r="4" @pointerdown.stop.prevent="volPointDown($event, a, k.tMs, rowH(r))" @dblclick.stop.prevent="volPointDelete(a, k.tMs)" />
              </svg>
              <i v-if="!a.looped" class="h r" @pointerdown.stop.prevent="audioTrim($event, a, 'r')" />
            </div>
          </div>
        </div>

        <div v-if="insY !== null" class="insline" :style="{ top: insY + 'px', left: LABEL_W + 'px' }"><span>松手：新建一条轨道</span></div>
        <div class="ph" :style="{ left: LABEL_W + px(ed.playhead.value) + 'px' }"><i /></div>
      </div>
    </div>

    <div class="mini" :style="{ height: MINI_H + 'px' }" title="时间线小地图：点一下或拖动，快速跳到别处" @pointerdown.prevent="miniDown">
      <i v-for="(m, i) in miniItems" :key="i" class="mi" :style="{ left: m.l + 'px', width: m.w + 'px', top: m.t + 'px', background: m.c }" />
      <i class="mv" :style="miniView" />
    </div>

    <TrackMenu v-if="menu" :x="menu.x" :y="menu.y" :title="menu.title" :items="menu.items" @close="menu = null" />
  </div>
</template>

<style scoped>
.tl {
  position: relative;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  background: var(--cc-card);
  max-height: 360px;
  user-select: none;
}
.body {
  position: relative;
  min-width: 100%;
}
.row {
  display: flex;
  border-bottom: 1px solid var(--cc-line);
}
.row:last-of-type {
  border-bottom: 0;
}
.lab {
  position: sticky;
  left: 0;
  z-index: 20;
  flex: none;
  width: 176px;
  box-sizing: border-box;
  padding: 0;
  font-size: 11.5px;
  color: var(--cc-mute);
  background: var(--cc-card);
  border-right: 1px solid var(--cc-line);
  display: flex;
  align-items: center;
}
.lane {
  position: relative;
  flex: none;
}
.ruler-row .lane {
  height: 24px;
  cursor: col-resize;
  background: color-mix(in srgb, var(--cc-line) 35%, transparent);
}
.ruler-row .lab {
  height: 24px;
}
.tick {
  position: absolute;
  top: 0;
  bottom: 0;
  padding-left: 4px;
  border-left: 1px solid var(--cc-line);
  font-size: 10.5px;
  line-height: 24px;
  color: var(--cc-mute);
  pointer-events: none;
  font-family: var(--cc-mono);
}
.empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  padding-left: 12px;
  font-size: 12px;
  color: var(--cc-mute);
  pointer-events: none;
  white-space: nowrap;
}
.blk {
  position: absolute;
  top: 2px;
  bottom: 2px;
  box-sizing: border-box;
  border-radius: 5px;
  overflow: hidden;
  cursor: grab;
  display: flex;
  align-items: center;
  min-width: 0;
}
.crumb { display: flex; gap: 8px; align-items: center; padding: 4px 8px; border: 1px solid var(--cc-acc); border-radius: 8px; background: var(--cc-card); font-size: 12px; flex-wrap: wrap }
.crumb .b { border: 1px solid var(--cc-line); background: var(--cc-card); color: var(--cc-fg); border-radius: 10px; padding: 1px 10px; cursor: pointer }
.crumb b.cur { color: var(--cc-acc) }
.blk.cmp { background-image: repeating-linear-gradient(135deg, rgba(255, 255, 255, 0.14) 0 6px, transparent 6px 12px) !important }
.blk.grouped {
  box-shadow: inset 0 -3px 0 #d98a1f;
}
.blk.sel {
  outline: 2px solid var(--cc-acc);
  outline-offset: -1px;
  z-index: 8 !important;
}
.cap {
  position: relative;
  padding: 0 10px;
  font-size: 11.5px;
  color: #fff;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.75);
  min-width: 0;
}
.txt {
  background: #5b4fb0;
  border: 1px solid #a08cff80;
  color: #fff;
  font-size: 11.5px;
}
.txt span {
  padding: 0 9px;
  min-width: 0;
}
.clip {
  background-color: #1d6b66;
  background-repeat: no-repeat;
  background-position: left center;
  background-size: auto 100%;
  border: 1px solid rgba(0, 0, 0, 0.25);
  align-items: flex-end;
}
.clip.img {
  background-color: #35607f;
}
.ovl {
  background-color: #1f5f7a;
  background-repeat: no-repeat;
  background-position: left center;
  background-size: auto 100%;
  border: 1px solid rgba(0, 0, 0, 0.25);
}
.ovl.img {
  background-color: #8a5a7c;
}
.bt {
  position: absolute;
  top: 3px;
  bottom: 3px;
  width: 1px;
  background: rgba(255, 255, 255, 0.55);
  pointer-events: none;
}
.kfm {
  position: absolute;
  bottom: 2px;
  width: 7px;
  height: 7px;
  margin-left: -3px;
  background: var(--cc-amber);
  border: 1px solid rgba(0, 0, 0, 0.55);
  transform: rotate(45deg);
  pointer-events: none;
}
.cut {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  background: repeating-linear-gradient(135deg, rgba(0, 0, 0, 0.55) 0 4px, rgba(0, 0, 0, 0.25) 4px 8px);
  pointer-events: none;
}
.hot .lane {
  outline: 2px dashed var(--cc-acc);
  outline-offset: -2px;
  background: color-mix(in srgb, var(--cc-acc) 10%, transparent);
}
.clip.bad,
.ovl.bad,
.aud.bad {
  background-color: #8c3b36;
  background-image: repeating-linear-gradient(135deg, rgba(255, 255, 255, 0.12) 0 6px, transparent 6px 12px) !important;
}
.clip.moving {
  opacity: 0.75;
  cursor: grabbing;
}
.clip .cap {
  align-self: flex-end;
  padding-bottom: 3px;
}
.aud {
  background: #a8741f;
}
.xf {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  background: repeating-linear-gradient(135deg, rgba(255, 255, 255, 0.55) 0 4px, rgba(255, 255, 255, 0.15) 4px 8px);
  border-right: 1px solid rgba(255, 255, 255, 0.9);
  pointer-events: none;
}
.h {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 7px;
  cursor: ew-resize;
  background: rgba(255, 255, 255, 0);
  z-index: 3;
}
.h:hover,
.sel .h {
  background: rgba(255, 255, 255, 0.55);
}
.h.l {
  left: 0;
}
.h.r {
  right: 0;
}
.drop {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 3px;
  margin-left: -1px;
  background: var(--cc-acc);
  z-index: 30;
  pointer-events: none;
}
.ph {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0;
  z-index: 15;
  pointer-events: none;
  border-left: 2px solid #e5484d;
  margin-left: -1px;
}
.ph i {
  position: absolute;
  left: -6px;
  top: 0;
  border: 6px solid transparent;
  border-top-color: #e5484d;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tlw {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.tbar {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.tbar .sp { flex: 1 }
.ab {
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
  background: var(--cc-card);
  color: var(--cc-fg);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}
.ab:hover:not(:disabled) { border-color: var(--cc-acc); color: var(--cc-acc) }
.ab:disabled { opacity: 0.45; cursor: not-allowed }
.rng { font-size: 12px; color: var(--cc-acc); display: inline-flex; gap: 8px; align-items: center }
.lk { border: 0; background: none; color: var(--cc-acc); cursor: pointer; font: inherit; text-decoration: underline; padding: 0 }
.mk-cnt { padding-left: 8px; font-size: 11px; color: var(--cc-mute) }
.range {
  position: absolute;
  top: 0;
  height: 6px;
  background: var(--cc-acc);
  opacity: 0.8;
  z-index: 2;
}
.rh { position: absolute; top: -2px; width: 6px; height: 12px; background: var(--cc-acc); border-radius: 2px; cursor: ew-resize }
.rh.l { left: -3px }
.rh.r { right: -3px }
.mk {
  position: absolute;
  bottom: 0;
  height: 11px;
  min-width: 8px;
  margin-left: -4px;
  padding: 0;
  border: 0;
  background: var(--mc);
  clip-path: polygon(0 0, 100% 0, 100% 62%, 50% 100%, 0 62%);
  cursor: pointer;
  z-index: 4;
}
.mk .mt {
  position: absolute;
  left: 10px;
  top: -13px;
  white-space: nowrap;
  font-size: 10px;
  color: #fff;
  background: var(--mc);
  padding: 0 5px;
  border-radius: 3px;
  clip-path: none;
}
.row.hot .lane, .row.hot .lab { outline: 2px dashed var(--cc-acc); outline-offset: -2px; background: color-mix(in srgb, var(--cc-acc) 10%, transparent) }
.row.locked .lane { background-image: repeating-linear-gradient(135deg, rgba(0, 0, 0, 0.05) 0 6px, transparent 6px 12px) }
.row.off .lane { opacity: 0.55 }
.row.text .lane { background-color: color-mix(in srgb, #4f7cc9 6%, transparent) }
.row.audio .lane { background-color: color-mix(in srgb, #b9812d 6%, transparent) }
.ovl.adj { background-color: #b4561a; background-image: repeating-linear-gradient(135deg, rgba(255, 255, 255, 0.18) 0 6px, transparent 6px 12px) !important; border-style: dashed }
.insline {
  position: absolute;
  height: 3px;
  right: 0;
  margin-top: -1px;
  background: var(--cc-acc);
  z-index: 40;
  pointer-events: none;
}
.insline span { position: absolute; left: 8px; top: -18px; font-size: 11px; font-weight: 700; color: var(--cc-on-acc); background: var(--cc-acc); padding: 1px 8px; border-radius: 9px; white-space: nowrap }
.mini {
  position: relative;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
  background: color-mix(in srgb, var(--cc-line) 35%, transparent);
  overflow: hidden;
  cursor: pointer;
  touch-action: none;
}
.mi { position: absolute; height: 5px; border-radius: 2px; opacity: 0.9; pointer-events: none }
.mv { position: absolute; top: 0; bottom: 0; border: 2px solid var(--cc-acc); background: color-mix(in srgb, var(--cc-acc) 14%, transparent); border-radius: 4px; box-sizing: border-box; pointer-events: none }

.aud.wave { background-size: 100% 100%; background-repeat: no-repeat; background-color: #8a6422 }
.vol { position: absolute; left: 0; top: 0; overflow: visible; pointer-events: none }
.vl { fill: none; stroke: #ffd36b; stroke-width: 1.5; pointer-events: none }
.vh { fill: none; stroke: transparent; stroke-width: 9; pointer-events: stroke; cursor: ns-resize }
.vp { fill: #fff; stroke: #d9a21a; stroke-width: 2; pointer-events: all; cursor: grab }
.tl.blade .blk { cursor: crosshair }

.ab.on { border-color: var(--cc-acc); color: var(--cc-acc); background: color-mix(in srgb, var(--cc-acc) 10%, var(--cc-card)) }
.mixer { display: flex; flex-wrap: wrap; gap: 8px; padding: 8px; border: 1px solid var(--cc-line); border-radius: 8px; background: var(--cc-card) }
.mixer .ch { display: flex; flex-direction: column; gap: 4px; width: 210px; padding: 6px 8px; border: 1px solid var(--cc-line); border-radius: 8px; font-size: 12px }
.mixer .ch.off { opacity: 0.55 }
.mixer .cn { display: flex; gap: 6px; align-items: center }
.mixer .cn b { color: #b9812d }
.mixer .meter { flex: 1; min-width: 40px; height: 6px; border-radius: 3px; background: var(--cc-line); overflow: hidden; display: inline-block }
.mixer .meter i { display: block; height: 100%; background: #3a9d6e; transition: width 60ms linear }
.mixer .meter.hot i { background: #d9a21a }
.mixer .meter.clip i { background: #e5484d }
.mixer .mout { flex-basis: 100%; display: flex; gap: 14px; align-items: center; flex-wrap: wrap }
.mixer .mout .mo { display: flex; gap: 8px; align-items: center; flex: 1; min-width: 220px }
.mixer .mout .mo em { font-style: normal; color: var(--cc-sub, #888); font-size: 11px; min-width: 64px }
.mixer .cb { display: flex; gap: 4px }
.mixer .b { width: 22px; height: 20px; padding: 0; border: 1px solid var(--cc-line); border-radius: 4px; background: var(--cc-card); color: var(--cc-mute); font: 700 10px/1 system-ui, sans-serif; cursor: pointer }
.mixer .b.on { background: color-mix(in srgb, var(--cc-acc) 18%, var(--cc-card)); color: var(--cc-acc); border-color: var(--cc-acc) }
.mixer .b.solo.on { background: #e0a21a; color: #fff; border-color: #e0a21a }
.mixer .sl { display: grid; grid-template-columns: 30px 1fr 52px; align-items: center; gap: 6px }
.mixer .sl span { color: var(--cc-mute) }
.mixer .sl em { font-style: normal; font-size: 11px; text-align: right; font-variant-numeric: tabular-nums }
.mixer .cc { color: var(--cc-mute); font-size: 11px }
.mixer .mnote { flex-basis: 100%; margin: 0; font-size: 11.5px; color: var(--cc-mute) }
</style>
