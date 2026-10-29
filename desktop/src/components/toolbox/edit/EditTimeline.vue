<script setup lang="ts">
// 时间线：标尺、文字行、叠加轨（画中画、贴纸、GIF，可以有多条）、主轨（视频 / 图片）、音频行。拖动调整顺序、位置和长度，点击定位播放位置。
import { computed, nextTick, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { EditApi } from '../../../composables/useEditProject'
import type { EditAudio, EditClip, EditOverlay, EditText } from '../../../types'
import { MAX_OVERLAY_TRACKS, MIN_MS, audioSpan, clamp, editPoints, overlayDuration, overlayEnd, overlaySourceLimit, snap, textLanes, tickStep } from '../../../utils/edit'

const props = withDefaults(defineProps<{ ed: EditApi; pps: number; snapOn?: boolean }>(), { snapOn: true })
const emit = defineEmits<{ zoom: [pps: number] }>()
const ed = props.ed

const LABEL_W = 64
const SNAP_PX = 7
const scroller = ref<HTMLElement | null>(null)
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

// ---------- 行 ----------

const LANE_H = 30

const textLane = computed(() => textLanes(ed.project.value.texts))
const textRows = computed(() => Math.max(1, ...textLane.value.map((l) => l + 1)))
const audioRows = computed(() => Math.max(1, ed.project.value.audio.length))

/** 拖动叠加素材期间锁定显示几条轨：不然拖进最上面那条空轨时又冒出新的一条，鼠标底下的轨道一直在变，素材会一路往上跳。 */
const dragTop = ref<number | null>(null)
/** 显示哪几条叠加轨（从上到下）：用到的最高一条再多一条空的（拖进去就新建），至少一条。 */
const overlayTracks = computed(() => {
  const used = ed.project.value.overlays.reduce((m, o) => Math.max(m, o.track), 0)
  const top = dragTop.value ?? Math.min(MAX_OVERLAY_TRACKS, Math.max(1, used + (used > 0 ? 1 : 0)))
  return Array.from({ length: top }, (_, i) => top - i)
})
/** 每条叠加轨里的素材（同一轨道上时间重叠的会错开成多行） */
const byTrack = computed(() => {
  const m = new Map<number, { o: EditOverlay; lane: number }[]>()
  ed.project.value.overlays.forEach((o, i) => m.set(o.track, [...(m.get(o.track) ?? []), { o, lane: ed.overlayLane.value[i] ?? 0 }]))
  return m
})
const laneCount = (tr: number) => Math.max(1, ...(byTrack.value.get(tr) ?? []).map((x) => x.lane + 1))
const trackH = (tr: number) => laneCount(tr) * LANE_H
const anyOverlay = computed(() => ed.project.value.overlays.length > 0)

/** 点时间线时让输入框失去焦点，这样空格、S、Delete 等快捷键马上可用。 */
function blurInput() {
  const a = document.activeElement as HTMLElement | null
  if (a && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)) a.blur()
}

const isSel = (kind: 'clip' | 'overlay' | 'text' | 'audio', id: number) => ed.sel.value?.kind === kind && ed.sel.value.id === id

// ---------- 拖动 ----------

function drag(e: PointerEvent, onMove: (dx: number, ev: PointerEvent) => void, onEnd?: (moved: boolean) => void) {
  const x0 = e.clientX
  let moved = false
  const move = (ev: PointerEvent) => {
    const dx = ev.clientX - x0
    if (!moved && Math.abs(dx) < 3) return
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

/** 吸附用的候选时间：播放位置，加上主轨片段、叠加素材、文字、配乐的边界。 */
function snapPoints(skip?: { kind: 'text' | 'audio' | 'overlay'; id: number }): number[] {
  return [ed.playhead.value, ...editPoints(ed.project.value, ed.placed.value, skip)]
}
/** 吸附开着时把 v 吸到附近的候选值上，关着就原样返回。 */
const snapTo = (v: number, pts: number[]) => (props.snapOn ? snap(v, pts, msOf(SNAP_PX)) : v)

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
  ed.sel.value = { kind: 'clip', id: c.id }
  const t0 = timeAt(e.clientX)
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

function trimClip(e: PointerEvent, c: EditClip, side: 'l' | 'r') {
  const o = { inMs: c.inMs, outMs: c.outMs }
  const limit = ed.sources[c.path]?.durationMs ?? Infinity
  const minSpan = Math.max(MIN_MS, MIN_MS * c.speed)
  ed.sel.value = { kind: 'clip', id: c.id }
  drag(e, (dx) => {
    const d = msOf(dx)
    if (c.kind === 'image') {
      const out = side === 'r' ? o.outMs + d : o.outMs - d
      ed.patchClip(c.id, { outMs: Math.round(Math.max(MIN_MS, out)) }, `trim:${c.id}`)
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
const clipName = (c: EditClip) => ed.sources[c.path]?.name ?? c.path.split(/[\\/]/).pop() ?? c.path
const isBroken = (path: string) => path in ed.broken

// ---------- 叠加素材 ----------

/** 鼠标在哪条叠加轨上（按纵向位置）：在最上面一条之上算最上面一条，在最下面一条之下算 1 号。 */
function trackAtY(clientY: number, fallback: number): number {
  const rows = [...(scroller.value?.querySelectorAll<HTMLElement>('.ov-row') ?? [])]
  if (!rows.length) return fallback
  for (const r of rows) {
    const b = r.getBoundingClientRect()
    if (clientY >= b.top && clientY < b.bottom) return Number(r.dataset.track)
  }
  return clientY < rows[0].getBoundingClientRect().top ? Number(rows[0].dataset.track) : Number(rows[rows.length - 1].dataset.track)
}

function overlayDown(e: PointerEvent, o: EditOverlay) {
  ed.sel.value = { kind: 'overlay', id: o.id }
  const s0 = o.startMs
  const len = overlayDuration(o)
  const pts = snapPoints({ kind: 'overlay', id: o.id })
  const t0 = timeAt(e.clientX)
  dragTop.value = overlayTracks.value[0]
  drag(
    e,
    (dx, ev) => {
      let s = Math.max(0, s0 + msOf(dx))
      const a = snapTo(s, pts)
      const b = snapTo(s + len, pts) - len
      s = a !== s ? a : b !== s ? b : s
      ed.patchOverlay(o.id, { startMs: Math.max(0, Math.round(s)), track: trackAtY(ev.clientY, o.track) }, `omove:${o.id}`)
    },
    (moved) => {
      dragTop.value = null
      if (!moved) seek(t0)
    },
  )
}

function overlayTrim(e: PointerEvent, o: EditOverlay, side: 'l' | 'r') {
  const o0 = { s: o.startMs, i: o.inMs, o: o.outMs, dur: overlayDuration(o) }
  const limit = overlaySourceLimit(o, ed.sources[o.path])
  const speed = o.kind === 'image' ? 1 : o.speed
  const minSpan = Math.max(MIN_MS, MIN_MS * speed)
  const pts = snapPoints({ kind: 'overlay', id: o.id })
  ed.sel.value = { kind: 'overlay', id: o.id }
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

const ovName = (o: EditOverlay) => ed.sources[o.path]?.name ?? o.path.split(/[\\/]/).pop() ?? o.path
const ovThumb = (o: EditOverlay) => {
  const t = ed.sources[o.path]?.thumb
  return t ? `url("${convertFileSrc(t)}")` : 'none'
}
/** 叠加素材超出主轨结尾的那段（像素宽度，没有超出返回 0）：成片里没有这一段 */
function cutW(o: EditOverlay): number {
  const total = ed.total.value
  if (!total) return 0
  return Math.max(0, px(Math.min(overlayEnd(o) - total, overlayDuration(o))))
}

// ---------- 文字 ----------

function textDown(e: PointerEvent, t: EditText) {
  ed.sel.value = { kind: 'text', id: t.id }
  const o = { s: t.startMs, e: t.endMs }
  const pts = snapPoints({ kind: 'text', id: t.id })
  const t0 = timeAt(e.clientX)
  drag(
    e,
    (dx) => {
      const len = o.e - o.s
      let s = Math.max(0, o.s + msOf(dx))
      // 开头或结尾靠近别的边界时吸附过去
      const a = snapTo(s, pts)
      const b = snapTo(s + len, pts) - len
      s = a !== s ? a : b !== s ? b : s
      s = Math.max(0, Math.round(s))
      ed.patchText(t.id, { startMs: s, endMs: s + len }, `tmove:${t.id}`)
    },
    (moved) => {
      if (!moved) seek(t0)
    },
  )
}
function textTrim(e: PointerEvent, t: EditText, side: 'l' | 'r') {
  const o = { s: t.startMs, e: t.endMs }
  const pts = snapPoints({ kind: 'text', id: t.id })
  ed.sel.value = { kind: 'text', id: t.id }
  drag(e, (dx) => {
    const v = snapTo(Math.max(0, (side === 'l' ? o.s : o.e) + msOf(dx)), pts)
    if (side === 'l') ed.patchText(t.id, { startMs: Math.round(clamp(v, 0, o.e - MIN_MS)) }, `ttrim:${t.id}`)
    else ed.patchText(t.id, { endMs: Math.round(Math.max(o.s + MIN_MS, v)) }, `ttrim:${t.id}`)
  })
}

// ---------- 音频 ----------

const aSpan = (a: EditAudio) => audioSpan(a, ed.sources[a.path], ed.total.value)

function audioDown(e: PointerEvent, a: EditAudio) {
  ed.sel.value = { kind: 'audio', id: a.id }
  const s0 = a.startMs
  const pts = snapPoints({ kind: 'audio', id: a.id })
  const t0 = timeAt(e.clientX)
  drag(
    e,
    (dx) => {
      const s = Math.round(Math.max(0, snapTo(s0 + msOf(dx), pts)))
      ed.patchAudio(a.id, { startMs: s }, `amove:${a.id}`)
    },
    (moved) => {
      if (!moved) seek(t0)
    },
  )
}
function audioTrim(e: PointerEvent, a: EditAudio, side: 'l' | 'r') {
  const o = { s: a.startMs, i: a.inMs }
  const span = aSpan(a)
  const limit = ed.sources[a.path]?.durationMs ?? Infinity
  ed.sel.value = { kind: 'audio', id: a.id }
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
  /** 叠加轨编号（lane 为 overlay 时） */
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
  const lane = (document.elementFromPoint(clientX, clientY) as HTMLElement | null)?.closest<HTMLElement>('[data-lane]')
  const kind = lane?.dataset.lane as DropTarget['lane'] | 'ruler' | undefined
  if (!lane || !kind || kind === 'ruler') return null
  const ms = Math.round(timeAt(clientX))
  if (kind === 'main') {
    const index = ed.placed.value.filter((p) => (p.startMs + p.endMs) / 2 < ms).length
    return { lane: 'main', ms, index }
  }
  return { lane: kind, ms, track: kind === 'overlay' ? Number(lane.dataset.track) : undefined }
}

/** 正在拖文件时高亮落点所在的行。 */
const hot = ref('')
function hover(clientX: number | null, clientY = 0) {
  const t = clientX === null ? null : targetAt(clientX, clientY)
  hot.value = t ? (t.lane === 'overlay' ? `overlay:${t.track}` : t.lane) : ''
}

/** 滚到让播放位置可见（跳到某处时用）。 */
function reveal() {
  const el = scroller.value
  if (!el) return
  const x = px(ed.playhead.value) + LABEL_W
  if (x < el.scrollLeft + LABEL_W || x > el.scrollLeft + el.clientWidth - 40) el.scrollLeft = Math.max(0, x - el.clientWidth / 2)
}
defineExpose({ reveal, targetAt, hover })
</script>

<template>
  <div ref="scroller" class="tl" @pointerdown.capture="blurInput" @wheel="onWheel">
    <div class="body" :style="{ width: LABEL_W + contentW + 'px' }">
      <!-- 标尺 -->
      <div class="row ruler-row">
        <div class="lab" />
        <div ref="rulerLane" class="lane ruler" data-lane="ruler" :style="{ width: contentW + 'px' }" @pointerdown.prevent="scrub">
          <span v-for="t in ticks" :key="t.ms" class="tick" :style="{ left: t.x + 'px' }">{{ t.label }}</span>
        </div>
      </div>

      <!-- 文字 -->
      <div v-for="r in textRows" :key="'t' + r" class="row text-row">
        <div class="lab">{{ r === 1 ? '文字' : '' }}</div>
        <div class="lane" data-lane="text" :style="{ width: contentW + 'px' }" @pointerdown.prevent="bgDown">
          <div
            v-for="(t, i) in ed.project.value.texts"
            v-show="textLane[i] === r - 1"
            :key="t.id"
            class="blk txt"
            :class="{ sel: isSel('text', t.id) }"
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
      </div>

      <!-- 叠加轨：轨号越大越在上面，盖在主轨画面上 -->
      <div v-for="tr in overlayTracks" :key="'o' + tr" class="row ov-row" :class="{ hot: hot === 'overlay:' + tr }" :data-track="tr">
        <div class="lab" :style="{ height: trackH(tr) + 'px' }">叠加 {{ tr }}</div>
        <div class="lane" data-lane="overlay" :data-track="tr" :style="{ width: contentW + 'px', height: trackH(tr) + 'px' }" @pointerdown.prevent="bgDown">
          <div v-if="tr === overlayTracks[0] && !anyOverlay" class="empty">点上方“添加素材”选叠加素材，或把文件拖到这里（画中画、贴纸、GIF 动图）</div>
          <div
            v-for="x in byTrack.get(tr) ?? []"
            :key="x.o.id"
            class="blk ovl"
            :class="{ sel: isSel('overlay', x.o.id), img: x.o.kind === 'image', bad: isBroken(x.o.path) }"
            :style="{
              left: px(x.o.startMs) + 'px',
              width: Math.max(10, px(overlayDuration(x.o))) + 'px',
              top: x.lane * LANE_H + 2 + 'px',
              height: LANE_H - 4 + 'px',
              bottom: 'auto',
              backgroundImage: ovThumb(x.o),
            }"
            :title="ovName(x.o)"
            @pointerdown.stop.prevent="overlayDown($event, x.o)"
          >
            <i class="h l" @pointerdown.stop.prevent="overlayTrim($event, x.o, 'l')" />
            <span class="cap ellipsis">{{ isBroken(x.o.path) ? '找不到文件 · ' : '' }}{{ ovName(x.o) }}</span>
            <span v-if="cutW(x.o) > 0" class="cut" :style="{ width: cutW(x.o) + 'px' }" title="超出了主轨的结尾，成片里没有这一段" />
            <i class="h r" @pointerdown.stop.prevent="overlayTrim($event, x.o, 'r')" />
          </div>
        </div>
      </div>

      <!-- 主轨 -->
      <div class="row main-row" :class="{ hot: hot === 'main' }">
        <div class="lab">视频</div>
        <div class="lane" data-lane="main" :style="{ width: contentW + 'px' }" @pointerdown.prevent="bgDown">
          <div v-if="!ed.project.value.clips.length" class="empty">点上方“添加素材”，或把视频、图片拖到这里</div>
          <div
            v-for="(c, i) in ed.project.value.clips"
            :key="c.id"
            class="blk clip"
            :class="{ sel: isSel('clip', c.id), img: c.kind === 'image', bad: isBroken(c.path), moving: dragIdx === i }"
            :style="{
              left: px(ed.placed.value[i].startMs) + 'px',
              width: Math.max(10, px(ed.placed.value[i].durMs)) + 'px',
              backgroundImage: thumbOf(c),
              transform: dragIdx === i ? `translateX(${dragDx}px)` : undefined,
              zIndex: dragIdx === i ? 5 : i + 1,
            }"
            :title="clipName(c)"
            @pointerdown.stop.prevent="clipDown($event, i)"
          >
            <span v-if="ed.placed.value[i].overlapMs > 0" class="xf" :style="{ width: px(ed.placed.value[i].overlapMs) + 'px' }" title="转场" />
            <i class="h l" @pointerdown.stop.prevent="trimClip($event, c, 'l')" />
            <span class="cap ellipsis">{{ isBroken(c.path) ? '找不到文件 · ' : '' }}{{ clipName(c) }}</span>
            <i class="h r" @pointerdown.stop.prevent="trimClip($event, c, 'r')" />
          </div>
          <div v-if="dropAt >= 0" class="drop" :style="{ left: dropX(dropAt, dragIdx) + 'px' }" />
        </div>
      </div>

      <!-- 音频 -->
      <div v-for="r in audioRows" :key="'a' + r" class="row audio-row" :class="{ hot: hot === 'audio' }">
        <div class="lab">{{ r === 1 ? '音频' : '' }}</div>
        <div class="lane" data-lane="audio" :style="{ width: contentW + 'px' }" @pointerdown.prevent="bgDown">
          <div v-if="r === 1 && !ed.project.value.audio.length" class="empty">点上方“添加素材”选配乐，或把音频拖到这里</div>
          <div
            v-if="ed.project.value.audio[r - 1]"
            class="blk aud"
            :class="{ sel: isSel('audio', ed.project.value.audio[r - 1].id), bad: isBroken(ed.project.value.audio[r - 1].path) }"
            :style="{ left: px(ed.project.value.audio[r - 1].startMs) + 'px', width: Math.max(10, px(aSpan(ed.project.value.audio[r - 1]))) + 'px' }"
            :title="audioName(ed.project.value.audio[r - 1])"
            @pointerdown.stop.prevent="audioDown($event, ed.project.value.audio[r - 1])"
          >
            <i class="h l" @pointerdown.stop.prevent="audioTrim($event, ed.project.value.audio[r - 1], 'l')" />
            <span class="cap ellipsis">{{ audioName(ed.project.value.audio[r - 1]) }}</span>
            <i v-if="!ed.project.value.audio[r - 1].looped" class="h r" @pointerdown.stop.prevent="audioTrim($event, ed.project.value.audio[r - 1], 'r')" />
          </div>
        </div>
      </div>

      <div class="ph" :style="{ left: LABEL_W + px(ed.playhead.value) + 'px' }"><i /></div>
    </div>
  </div>
</template>

<style scoped>
.tl {
  position: relative;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  background: var(--cc-card);
  max-height: 330px;
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
  width: 64px;
  box-sizing: border-box;
  padding: 0 6px;
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
.text-row .lane,
.text-row .lab {
  height: 26px;
}
.main-row .lane,
.main-row .lab {
  height: 58px;
}
.audio-row .lane,
.audio-row .lab {
  height: 30px;
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
  background: #4f7cc9;
  color: #fff;
  font-size: 11.5px;
}
.txt span {
  padding: 0 9px;
  min-width: 0;
}
.clip {
  background-color: #3f6f5e;
  background-repeat: no-repeat;
  background-position: left center;
  background-size: auto 100%;
  border: 1px solid rgba(0, 0, 0, 0.25);
  align-items: flex-end;
}
.clip.img {
  background-color: #6b5a8e;
}
.ovl {
  background-color: #2f6f8f;
  background-repeat: no-repeat;
  background-position: left center;
  background-size: auto 100%;
  border: 1px solid rgba(0, 0, 0, 0.25);
}
.ovl.img {
  background-color: #8a5a7c;
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
  background: #b9812d;
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
</style>
