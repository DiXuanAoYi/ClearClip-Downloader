// 剪辑工程的状态：工程内容、素材信息、选中项、播放位置、撤销 / 重做、打开 / 保存、草稿。

import { computed, reactive, ref, watch } from 'vue'
import { api, errorText } from '../api'
import type { EditAudio, EditClip, EditKeyframe, EditMarker, EditOverlay, EditProject, EditRegion, EditSource, EditText, EditTrack, EditTrackKind, EditTrackRole, TimeSpan } from '../types'
import { audioSpan, overlayFitScale, blankChroma } from '../utils/edit'
import { dropTrack, effectiveProject, freeTrack, maxNo, metaFor, minNo, nextNo, shiftUp, swapTracks, trackOf } from '../utils/tracks'
import {
  MAX_AUDIO,
  MAX_CLIPS,
  MAX_OVERLAYS,
  MAX_OVERLAY_TRACKS,
  MAX_START_MS,
  MAX_TEXTS,
  MIN_MS,
  applyToAllClips,
  audioFromSource,
  clamp,
  clearTransitions,
  clipDuration,
  clipFromSource,
  clipIndexAt,
  clipToOverlay,
  copyRegion,
  cutAtSourceTimes,
  detachedAudio,
  editPoints,
  kfDelete,
  kfSplit,
  kfSet,
  emptyProject,
  layout,
  mediaPaths,
  moveItem,
  nextId,
  nextRegionId,
  removeSilences,
  rippleBy,
  freezeAt,
  speedRamp,
  curveSteps,
  rampPieceCount,
  type SpeedPoint,
  textFromCue,
  normalizeProject,
  outputSize,
  overlayDuration,
  overlayEnd,
  overlayFromSource,
  overlayLanes,
  overlayToClip,
  pickTrack,
  playbackFps,
  shiftAfter,
  splitClip,
  splitOverlay,
  stepFrame,
  textAt,
  totalMs,
  trimClipAt,
  trimOverlayAt,
  neighborPoint,
  arrangeToBeats,
  beatsOnTimeline,
  fitAudio,
  makeCompoundIn,
  enterCompoundProject,
  leaveCompoundProject,
  foldToRoot,
  pruneCompounds,
} from '../utils/edit'
import type { CompoundFrame } from '../utils/edit'

export interface Sel {
  kind: 'clip' | 'overlay' | 'text' | 'audio'
  id: number
}

const PROXY_KEY = 'clearclip.edit.proxyBig'
const METER_KEY = 'clearclip.edit.meters'
const DRAFT_KEY = 'clearclip.edit.draft'
const RECENT_KEY = 'clearclip.edit.recent'
const RECENT_LIMIT = 8
const HISTORY_LIMIT = 100

function readProxyBig(): boolean {
  return readFlag(PROXY_KEY)
}
function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === '1'
  } catch {
    return false
  }
}

function readRecent(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]')
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === 'string').slice(0, RECENT_LIMIT) : []
  } catch {
    return []
  }
}

export interface AddReport {
  added: number
  errors: string[]
}

export function useEditProject() {
  const project = ref<EditProject>(emptyProject())
  const sources = reactive<Record<string, EditSource>>({})
  /** 系统播放器解不了的素材：转好的预览小文件的播放地址（只在监视器里用，导出用原素材） */
  const proxies = reactive<Record<string, string>>({})
  const proxying = reactive<Record<string, boolean>>({})
  async function makeProxy(path: string): Promise<string | null> {
    if (proxies[path]) return proxies[path]
    if (proxying[path]) return null
    proxying[path] = true
    try {
      proxies[path] = await api.editProxy(path)
      return proxies[path]
    } finally {
      delete proxying[path]
    }
  }
  /** 大素材（高过 1080 行）自动生成预览用的小文件。关着时只有系统播放器解不了的素材才需要手动生成。 */
  const proxyBig = ref(readProxyBig())
  watch(proxyBig, (v) => {
    try {
      localStorage.setItem(PROXY_KEY, v ? '1' : '0')
    } catch {
      /* 记不住不影响使用 */
    }
  })
  /** 值得换成小文件预览的素材：视频，且分辨率高过 1080p（HDR 的不在这里处理，它另有转换流程）。 */
  const needsProxy = (s: EditSource) => s.kind === 'video' && s.hasVideo && !s.hdr && Math.max(s.width, s.height) > 1920
  async function autoProxy() {
    if (!proxyBig.value) return
    for (const path of Object.keys(sources)) {
      const s = sources[path]
      if (!s || !needsProxy(s) || proxies[path] || proxying[path] || !proxyBig.value) continue
      try {
        await makeProxy(path)
      } catch {
        /* 生成不了就继续用原素材预览，不打扰 */
      }
    }
  }
  watch([proxyBig, () => Object.keys(sources).length], () => void autoProxy())
  /** 混音器里的电平表（实验）：默认关。开着时预览的声音会先经过 Web Audio 再出来，要重新打开剪辑页才生效。 */
  const meterOn = ref(readFlag(METER_KEY))
  watch(meterOn, (v) => {
    try {
      localStorage.setItem(METER_KEY, v ? '1' : '0')
    } catch {
      /* 记不住不影响使用 */
    }
  })
  /** 各路电平（dBFS，没声音 −90）：a1、a2… 是音频轨，out 是主输出；只在播放时更新。 */
  const levels = reactive<Record<string, number>>({})
  /** 各路最近一次削波（超过 0 dBFS）的时刻，毫秒时间戳 */
  const clips = reactive<Record<string, number>>({})
  /** 工程里用到、但现在找不到或读不出来的文件 */
  const broken = reactive<Record<string, string>>({})
  const sel = ref<Sel | null>(null)
  /** 和 sel 一起被选中的其他项（Shift 点选加进来的、同组的） */
  const extra = ref<Sel[]>([])
  const playhead = ref(0)
  const playing = ref(false)
  const filePath = ref('')
  const dirty = ref(false)
  const busy = ref(false)
  /** 最近打开或保存过的工程文件（新的在前，最多 8 个），记在本机浏览器存储里 */
  const recent = ref<string[]>(readRecent())

  function saveRecent() {
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recent.value))
    } catch {
      /* 记不住最近的工程不影响使用 */
    }
  }
  function remember(path: string) {
    recent.value = [path, ...recent.value.filter((x) => x !== path)].slice(0, RECENT_LIMIT)
    saveRecent()
  }
  function forget(path?: string) {
    recent.value = path === undefined ? [] : recent.value.filter((x) => x !== path)
    saveRecent()
  }

  const placed = computed(() => layout(project.value.clips))
  /** 监视器和试听用的工程：隐藏的轨道去掉、静音的不出声、独奏只听独奏的 */
  const live = computed(() => effectiveProject(project.value, true))
  const total = computed(() => totalMs(project.value.clips))
  /** 叠加素材在各自轨道里的行号（同一轨道上时间重叠的错开显示） */
  const overlayLane = computed(() => overlayLanes(project.value.overlays))
  /** 时间线上所有东西里最晚结束的时间（叠加素材、文字、音频可以超出主轨） */
  const extent = computed(() => {
    let m = total.value
    for (const o of project.value.overlays) m = Math.max(m, overlayEnd(o))
    for (const t of project.value.texts) m = Math.max(m, t.endMs)
    for (const a of project.value.audio) m = Math.max(m, a.startMs + 1000)
    return m
  })

  // ---------- 撤销 / 重做 ----------

  const undoStack = ref<string[]>([])
  const redoStack = ref<string[]>([])
  let lastKey = ''
  let lastAt = 0
  const snapshot = () => JSON.stringify(project.value)

  // ---------- 复合片段 ----------
  /** 进入复合片段以后，每一层外面那层的信息（最外面的在最前）。空 = 正在编辑最外层的工程。 */
  const compoundStack = ref<CompoundFrame[]>([])
  /** 最外层的工程：正在编辑某个复合片段时，把子时间线一层层放回去。保存、导出、预览、草稿都用它，不用 `project`。 */
  const rootProject = computed<EditProject>(() => pruneCompounds(compoundStack.value.length ? foldToRoot(project.value, compoundStack.value) : project.value))
  const rootJson = computed(() => JSON.stringify(rootProject.value))
  /** 面包屑：主时间线 › 复合片段名 › … */
  const compoundPath = computed(() => compoundStack.value.map((f) => f.name))
  /** 进出复合片段后撤销记录从头开始（里外两层的撤销记录不能混在一起） */
  function resetHistory() {
    undoStack.value = []
    redoStack.value = []
    lastKey = ''
  }

  /** 修改工程。`key` 相同且间隔很短的连续修改（拖滑块、拖片段边缘）合并成一步撤销。 */
  function mutate(fn: (p: EditProject) => void, key = '') {
    const now = Date.now()
    const merge = key !== '' && key === lastKey && now - lastAt < 800
    if (!merge) {
      undoStack.value.push(snapshot())
      if (undoStack.value.length > HISTORY_LIMIT) undoStack.value.shift()
      redoStack.value = []
    }
    lastKey = key
    lastAt = now
    fn(project.value)
    dirty.value = true
  }

  function restore(json: string) {
    project.value = normalizeProject(JSON.parse(json))
    dirty.value = true
    lastKey = ''
    if (sel.value && !find(sel.value)) sel.value = null
    extra.value = extra.value.filter((x) => find(x))
    playhead.value = clamp(playhead.value, 0, Math.max(0, total.value))
  }
  function undo() {
    const prev = undoStack.value.pop()
    if (prev === undefined) return
    redoStack.value.push(snapshot())
    restore(prev)
  }
  function redo() {
    const next = redoStack.value.pop()
    if (next === undefined) return
    undoStack.value.push(snapshot())
    restore(next)
  }
  const canUndo = computed(() => undoStack.value.length > 0)
  const canRedo = computed(() => redoStack.value.length > 0)

  // ---------- 取项目 ----------

  function find(s: Sel): EditClip | EditOverlay | EditText | EditAudio | undefined {
    const p = project.value
    return (s.kind === 'clip' ? p.clips : s.kind === 'overlay' ? p.overlays : s.kind === 'text' ? p.texts : p.audio).find((x) => x.id === s.id)
  }
  const clipOf = (id: number) => project.value.clips.find((c) => c.id === id)
  const overlayOf = (id: number) => project.value.overlays.find((c) => c.id === id)
  const textOf = (id: number) => project.value.texts.find((c) => c.id === id)
  const audioOf = (id: number) => project.value.audio.find((c) => c.id === id)
  // ---------- 多选与编组 ----------

  const sameSel = (a: Sel, b: Sel) => a.kind === b.kind && a.id === b.id
  const selection = computed<Sel[]>(() => (sel.value ? [sel.value, ...extra.value.filter((x) => !sameSel(x, sel.value as Sel))] : []))
  const isSelected = (kind: Sel['kind'], id: number) => selection.value.some((x) => x.kind === kind && x.id === id)
  const itemLists = (p: EditProject): [Sel['kind'], { id: number; group?: number }[]][] => [
    ['clip', p.clips],
    ['overlay', p.overlays],
    ['text', p.texts],
    ['audio', p.audio],
  ]
  /** 同一组的所有项 */
  function groupMembers(g: number): Sel[] {
    const out: Sel[] = []
    for (const [kind, list] of itemLists(project.value)) for (const x of list) if (x.group === g) out.push({ kind, id: x.id })
    return out
  }
  /** 和 s 同组的其他项（没有编组返回空） */
  function groupMates(s: Sel): Sel[] {
    const g = find(s)?.group
    return g === undefined ? [] : groupMembers(g).filter((x) => !sameSel(x, s))
  }
  // 点选一项时，同组的一起选中（要同步更新，后面的 Shift 点选才能在它上面继续改）
  watch(sel, (s) => (extra.value = s ? groupMates(s) : []), { flush: 'sync' })
  /** Shift 点选：把一项加进选择，或从选择里去掉（同组的一起）。 */
  function toggleSel(kind: Sel['kind'], id: number) {
    const s: Sel = { kind, id }
    const mates = [s, ...groupMates(s)]
    if (!sel.value) {
      sel.value = s
      return
    }
    const cur = selection.value
    if (cur.some((x) => sameSel(x, s))) {
      const rest = cur.filter((x) => !mates.some((m) => sameSel(m, x)))
      if (!rest.length) {
        sel.value = null
        return
      }
      sel.value = rest[0]
      extra.value = rest.slice(1)
    } else {
      extra.value = [...cur.slice(1), ...mates]
    }
  }
  /** 把选中的主轨片段（要连着）和选中的叠加素材、文字、配乐合成一个复合片段。成功返回 null，不行返回原因。 */
  function makeCompound(): string | null {
    const r = makeCompoundIn(project.value, selection.value)
    if (typeof r === 'string') return r
    mutate((p) => {
      p.clips = r.project.clips
      p.overlays = r.project.overlays
      p.texts = r.project.texts
      p.audio = r.project.audio
      p.compounds = r.project.compounds
    })
    sel.value = { kind: 'clip', id: r.clipId }
    return null
  }
  /** 进入复合片段，编辑它里面的内容（撤销记录从头开始）。 */
  function enterCompound(clipId: number): boolean {
    const r = enterCompoundProject(project.value, clipId)
    if (!r) return false
    compoundStack.value.push(r.frame)
    project.value = r.inner
    sel.value = null
    playhead.value = 0
    playing.value = false
    resetHistory()
    return true
  }
  /** 退出一层复合片段，回到外面。 */
  function exitCompound(): boolean {
    const f = compoundStack.value.pop()
    if (!f) return false
    playing.value = false
    project.value = leaveCompoundProject(project.value, f)
    resetHistory()
    dirty.value = true
    sel.value = project.value.clips.some((c) => c.id === f.clipId) ? { kind: 'clip', id: f.clipId } : null
    playhead.value = clamp(playhead.value, 0, Math.max(0, total.value))
    return true
  }
  /** 选中的几项编成一组；少于两项返回 false。 */
  function groupSelected(): boolean {
    const list = selection.value
    if (list.length < 2) return false
    let next = 1
    for (const [, items] of itemLists(project.value)) for (const x of items) next = Math.max(next, (x.group ?? 0) + 1)
    mutate((p) => {
      for (const [kind, items] of itemLists(p)) for (const x of items) if (list.some((s) => s.kind === kind && s.id === x.id)) x.group = next
    })
    return true
  }
  /** 把选中的项所在的组拆开。没有编组返回 false。 */
  function ungroupSelected(): boolean {
    const gs = new Set(selection.value.map((s) => find(s)?.group).filter((g): g is number => g !== undefined))
    if (!gs.size) return false
    mutate((p) => {
      for (const [, items] of itemLists(p)) for (const x of items) if (x.group !== undefined && gs.has(x.group)) delete x.group
    })
    return true
  }
  /** 选中的项里，除 anchor 以外在时间线上自由摆放的（叠加素材、文字、配乐）。主轨片段的位置由顺序决定，不跟着动。 */
  function followersOf(p: EditProject, anchor: Sel): (EditOverlay | EditText | EditAudio)[] {
    const out: (EditOverlay | EditText | EditAudio)[] = []
    for (const s of selection.value) {
      if (sameSel(s, anchor) || s.kind === 'clip') continue
      const it = (s.kind === 'overlay' ? p.overlays : s.kind === 'text' ? p.texts : p.audio).find((x) => x.id === s.id)
      if (it) out.push(it)
    }
    return out
  }
  /** 拖动 anchor 时，其余选中的自由项平移同样的时间（不会移到 0 之前）。 */
  function shiftFollowers(p: EditProject, anchor: Sel, delta: number) {
    if (!delta) return
    const fs = followersOf(p, anchor)
    if (!fs.length) return
    const d = Math.max(delta, -Math.min(...fs.map((x) => x.startMs)))
    for (const x of fs) {
      x.startMs = Math.round(x.startMs + d)
      if ('endMs' in x && 'text' in x) x.endMs = Math.round(x.endMs + d)
    }
  }

  const selectedClip = computed(() => (sel.value?.kind === 'clip' ? clipOf(sel.value.id) : undefined))
  const selectedOverlay = computed(() => (sel.value?.kind === 'overlay' ? overlayOf(sel.value.id) : undefined))
  const selectedText = computed(() => (sel.value?.kind === 'text' ? textOf(sel.value.id) : undefined))
  const selectedAudio = computed(() => (sel.value?.kind === 'audio' ? audioOf(sel.value.id) : undefined))

  function patchClip(id: number, patch: Partial<EditClip>, key = '') {
    mutate((p) => {
      const c = p.clips.find((x) => x.id === id)
      if (c) Object.assign(c, patch)
    }, key)
  }

  /** 把片段在这些素材时间点切开（场景分割）。返回切出了几段，没有有效切点返回 0。 */
  function cutClipAt(id: number, cuts: number[]): number {
    const c = clipOf(id)
    if (!c) return 0
    let n = 0
    mutate((p) => {
      const i = p.clips.findIndex((x) => x.id === id)
      let next = nextId(p)
      const pieces = cutAtSourceTimes(p.clips[i], cuts, () => next++)
      if (!pieces) return
      p.clips.splice(i, 1, ...pieces)
      n = pieces.length
    })
    return n
  }

  /** 去掉片段里的静音段；后面的叠加素材、文字、配乐一起前移。返回去掉了多少毫秒（时间线上的），没有可去的返回 0。 */
  function removeClipSilences(id: number, spans: TimeSpan[], keepMs: number): number {
    const i = project.value.clips.findIndex((x) => x.id === id)
    if (i < 0) return 0
    const from = placed.value[i].endMs
    const before = total.value
    let removed = 0
    mutate((p) => {
      let next = nextId(p)
      const r = removeSilences(p.clips[i], spans, keepMs, () => next++)
      if (!r) return
      p.clips.splice(i, 1, ...r.clips)
      removed = Math.max(0, before - totalMs(p.clips))
      shiftAfter(p, from, removed)
    })
    return removed
  }

  /** 给一个片段套变速曲线（拆成几段不同速度）。后面的内容跟着前后移。返回新的片段数，不能套返回 0。 */
  function applySpeedRamp(id: number, steps: number[]): number {
    const i = project.value.clips.findIndex((x) => x.id === id)
    if (i < 0) return 0
    const from = placed.value[i].endMs
    const before = total.value
    let n = 0
    mutate((p) => {
      let next = nextId(p)
      const pieces = speedRamp(p.clips[i], steps, () => next++)
      if (!pieces) return
      p.clips.splice(i, 1, ...pieces)
      n = pieces.length
      rippleBy(p, from, totalMs(p.clips) - before)
    })
    return n
  }

  /**
   * 给一个片段套平滑变速曲线：按曲线取样拆成 12–60 个小段，相邻两段速度只差一点点，看起来是连续变速。
   * 返回新的片段数；太短、已经倒放 / 定格、或者会让片段超过上限时返回 0。
   */
  function applySmoothRamp(id: number, points: SpeedPoint[]): number {
    const i = project.value.clips.findIndex((x) => x.id === id)
    const c = project.value.clips[i]
    if (!c || c.kind !== 'video' || c.reverse || c.freeze || points.length < 2) return 0
    const n = rampPieceCount(c.outMs - c.inMs)
    if (!n || project.value.clips.length - 1 + n > MAX_CLIPS) return 0
    return applySpeedRamp(id, curveSteps(points, n))
  }

  /** 在播放头处定格 `holdMs` 毫秒（只对播放头所在的普通视频片段有效）。成功返回 true。 */
  function freezeAtPlayhead(holdMs: number): boolean {
    const t = playhead.value
    const i = clipIndexAt(placed.value, t)
    if (i < 0) return false
    const from = placed.value[i].endMs
    const before = total.value
    let ok = false
    mutate((p) => {
      let next = nextId(p)
      const pieces = freezeAt(p.clips[i], placed.value[i], t, holdMs, () => next++)
      if (!pieces) return
      p.clips.splice(i, 1, ...pieces)
      ok = true
      rippleBy(p, from, totalMs(p.clips) - before)
    })
    return ok
  }

  /** 让主轨按配乐的节拍切换（每个片段占 `perClip` 拍）。返回结果统计，没有可排的返回 null。 */
  function arrangeToBeatsOf(audioId: number, perClip: number): { arranged: number; short: number; left: number } | null {
    const a = project.value.audio.find((x) => x.id === audioId)
    const beats = a ? beatsOnTimeline(a) : []
    if (!a || beats.length < 2) return null
    let res: { arranged: number; short: number; left: number } | null = null
    const before = total.value
    mutate((p) => {
      const r = arrangeToBeats(p.clips, beats, perClip, (path) => sources[path]?.durationMs ?? undefined)
      if (!r) return
      p.clips = r.clips
      rippleBy(p, before, totalMs(p.clips) - before)
      res = { arranged: r.arranged, short: r.short, left: r.left }
    })
    return res
  }

  /** 让配乐适配成片长度（太短循环、太长截断 + 结尾淡出）。成功返回 true。 */
  function fitAudioToVideo(audioId: number): boolean {
    const a = project.value.audio.find((x) => x.id === audioId)
    if (!a) return false
    const patch = fitAudio(a, sources[a.path], total.value)
    if (!patch) return false
    patchAudio(audioId, patch)
    return true
  }

  /** 加一批自动生成的字幕（文字）。`replaceIds` 里的旧字幕先去掉。返回实际加入的条数（受 MAX_TEXTS 限制）。 */
  function addCaptions(make: (ids: () => number) => EditText[], replaceIds: number[] = []): number {
    let added = 0
    mutate((p) => {
      const drop = new Set(replaceIds)
      p.texts = p.texts.filter((t) => !drop.has(t.id))
      let next = nextId(p)
      const list = make(() => next++).slice(0, Math.max(0, MAX_TEXTS - p.texts.length))
      p.texts.push(...list)
      settleNew(p, 'text', list.map((t) => t.id))
      added = list.length
    })
    return added
  }

  /** 给片段放一个自动生成的区域（聚焦只能有一个：旧的聚焦被替掉）。 */
  function putRegion(clipId: number, region: EditRegion) {
    mutate((p) => {
      const c = p.clips.find((x) => x.id === clipId)
      if (!c) return
      const r = { ...region, id: nextRegionId(c) }
      if (r.effect === 'focus') c.regions = c.regions.filter((x) => x.effect !== 'focus')
      c.regions.push(r)
    })
  }

  function patchOverlay(id: number, patch: Partial<EditOverlay>, key = '') {
    mutate((p) => {
      const c = p.overlays.find((x) => x.id === id)
      if (!c) return
      const before = c.startMs
      Object.assign(c, patch)
      if (key.startsWith('omove:') && patch.startMs !== undefined) shiftFollowers(p, { kind: 'overlay', id }, c.startMs - before)
    }, key)
  }
  /** 播放头相对这个素材 / 文字开头的位置（毫秒，收紧在它自己的时长里） */
  function localMs(startMs: number, durMs: number): number {
    return Math.round(Math.min(Math.max(0, playhead.value - startMs), durMs))
  }
  /** 改叠加素材的位置 / 大小 / 旋转 / 不透明度 / 音量：有关键帧的属性在播放头处加（或改）一个关键点，没有的直接改固定值 */
  function setOverlayProps(id: number, patch: Partial<Record<'x' | 'y' | 'scale' | 'rotate' | 'opacity' | 'volume', number>>, key = '') {
    mutate((p) => {
      const o = p.overlays.find((x) => x.id === id)
      if (!o) return
      const at = localMs(o.startMs, overlayDuration(o))
      for (const [prop, v] of Object.entries(patch) as [string, number][]) {
        if (o.keyframes[prop]?.length) o.keyframes = kfSet(o.keyframes, prop, at, v)
        else (o as unknown as Record<string, number>)[prop] = v
      }
    }, key)
  }
  function setTextProps(id: number, patch: Partial<Record<'x' | 'y' | 'opacity', number>>, key = '') {
    mutate((p) => {
      const t = p.texts.find((x) => x.id === id)
      if (!t) return
      const at = localMs(t.startMs, t.endMs - t.startMs)
      for (const [prop, v] of Object.entries(patch) as [string, number][]) {
        if (t.keyframes[prop]?.length) t.keyframes = kfSet(t.keyframes, prop, at, v)
        else (t as unknown as Record<string, number>)[prop] = v
      }
    }, key)
  }
  /** 在 `tMs`（相对开头）给某个属性放一个关键点 / 改它的曲线 / 删掉 */
  function setKey(kind: 'overlay' | 'text' | 'clip' | 'audio', id: number, prop: string, tMs: number, value: number, ease?: EditKeyframe['ease'], key = '') {
    mutate((p) => {
      const it = kind === 'overlay' ? p.overlays.find((x) => x.id === id) : kind === 'clip' ? p.clips.find((x) => x.id === id) : kind === 'audio' ? p.audio.find((x) => x.id === id) : p.texts.find((x) => x.id === id)
      if (it) it.keyframes = kfSet(it.keyframes, prop, tMs, value, ease)
    }, key)
  }
  function deleteKey(kind: 'overlay' | 'text' | 'clip' | 'audio', id: number, prop: string, tMs: number) {
    mutate((p) => {
      const it = kind === 'overlay' ? p.overlays.find((x) => x.id === id) : kind === 'clip' ? p.clips.find((x) => x.id === id) : kind === 'audio' ? p.audio.find((x) => x.id === id) : p.texts.find((x) => x.id === id)
      if (it) it.keyframes = kfDelete(it.keyframes, prop, tMs)
    })
  }
  /** 把某个属性所有的关键点去掉，回到固定值（固定值设成 `keep`，比如当前这一刻的值） */
  function clearKeys(kind: 'overlay' | 'text' | 'clip' | 'audio', id: number, prop: string, keep?: number) {
    mutate((p) => {
      const it = kind === 'overlay' ? p.overlays.find((x) => x.id === id) : kind === 'clip' ? p.clips.find((x) => x.id === id) : kind === 'audio' ? p.audio.find((x) => x.id === id) : p.texts.find((x) => x.id === id)
      if (!it) return
      const rest = { ...it.keyframes }
      delete rest[prop]
      it.keyframes = rest
      if (keep !== undefined && kind !== 'clip') (it as unknown as Record<string, number>)[prop] = keep
    })
  }
  function patchText(id: number, patch: Partial<EditText>, key = '') {
    mutate((p) => {
      const c = p.texts.find((x) => x.id === id)
      if (!c) return
      const before = c.startMs
      Object.assign(c, patch)
      if (key.startsWith('tmove:') && patch.startMs !== undefined) shiftFollowers(p, { kind: 'text', id }, c.startMs - before)
    }, key)
  }
  function patchAudio(id: number, patch: Partial<EditAudio>, key = '') {
    mutate((p) => {
      const c = p.audio.find((x) => x.id === id)
      if (!c) return
      const before = c.startMs
      Object.assign(c, patch)
      if (key.startsWith('amove:') && patch.startMs !== undefined) shiftFollowers(p, { kind: 'audio', id }, c.startMs - before)
    }, key)
  }


  // ---------- 轨道 ----------

  const durOf = {
    overlay: (o: EditOverlay) => overlayDuration(o),
    audio: (a: EditAudio) => audioSpan(a, sources[a.path], total.value),
  }

  function patchTrack(kind: EditTrackKind, no: number, patch: Partial<EditTrack>, key = '') {
    mutate((p) => Object.assign(metaFor(p, kind, no), patch), key)
  }

  /** 新建一条轨道（放在这一类的最上面 / 最后）。成功返回编号，已经满了返回 null。 */
  function addTrack(kind: EditTrackKind, role: EditTrackRole = ''): number | null {
    let made: number | null = null
    mutate((p) => {
      const no = nextNo(p, kind)
      if (no === null) return
      const t = metaFor(p, kind, no)
      if (kind === 'video') t.role = role
      made = no
    })
    return made
  }

  /** 在 `no` 的位置插入一条空轨道，原来 `no` 和更高的轨道往上挪一位。 */
  function insertTrack(kind: EditTrackKind, no: number, role: EditTrackRole = ''): number | null {
    let made: number | null = null
    mutate((p) => {
      const at = Math.max(minNo(kind) + (kind === 'video' ? 1 : 0), no)
      if (at > maxNo(kind) || nextNo(p, kind) === null) return
      shiftUp(p, kind, at)
      const t = metaFor(p, kind, at)
      if (kind === 'video') t.role = role
      made = at
    })
    return made
  }

  /** 删掉一条轨道和上面的素材；返回被删的素材数。 */
  function removeTrack(kind: EditTrackKind, no: number): number {
    let n = 0
    mutate((p) => {
      n = dropTrack(p, kind, no)
    })
    sel.value = sel.value && find(sel.value) ? sel.value : null
    return n
  }

  /** 轨道上移（dir = 1，编号 +1）或下移（-1）一格。 */
  function moveTrack(kind: EditTrackKind, no: number, dir: 1 | -1): boolean {
    const to = no + dir
    if (to < minNo(kind) + (kind === 'video' ? 1 : 0) || to > maxNo(kind) || no < minNo(kind) + (kind === 'video' ? 1 : 0)) return false
    mutate((p) => swapTracks(p, kind, no, to))
    return true
  }

  /** 视频轨改类型：画中画缩小、全屏铺满、绿幕默认抠像；调整图层只能给空轨道（有素材的要先清掉）。 */
  function setTrackRole(no: number, role: EditTrackRole): boolean {
    const here = project.value.overlays.filter((o) => o.track === no)
    if (role === 'adjust' && here.some((o) => !o.adjust)) return false
    mutate((p) => {
      metaFor(p, 'video', no).role = role
      const frame = outputSize(p, sources)
      for (const o of p.overlays.filter((x) => x.track === no && !x.adjust)) {
        if (role === 'pip') {
          if (o.scale > 0.9) o.scale = 0.4
        } else if (role === 'full') {
          const sr = sources[o.path]
          o.scale = overlayFitScale(o, sr && { w: sr.width, h: sr.height }, frame, 'contain')
          o.x = 0.5
          o.y = 0.5
        } else if (role === 'chroma') {
          o.chroma = { ...blankChroma(), ...o.chroma, on: true }
        }
      }
    })
    return true
  }

  /** 清掉一条视频轨上的素材，好把它改成调整图层。 */
  function clearVideoTrack(no: number): number {
    let n = 0
    mutate((p) => {
      n = p.overlays.filter((o) => o.track === no).length
      p.overlays = p.overlays.filter((o) => o.track !== no)
    })
    return n
  }

  /** 在调整图层轨上加一段（默认 5 秒，从播放位置开始）。 */
  function addAdjust(no: number, startMs?: number, durMs = 5000): number | null {
    if (project.value.overlays.length >= MAX_OVERLAYS) return null
    let id: number | null = null
    mutate((p) => {
      const o = overlayFromSource({ path: '', name: '调整图层', kind: 'image', width: 0, height: 0, durationMs: null, hasAudio: false } as unknown as EditSource, nextId(p), startMs ?? Math.round(playhead.value), no, { w: 16, h: 9 })
      o.adjust = true
      o.path = ''
      o.kind = 'image'
      o.outMs = durMs
      o.scale = 1
      o.x = 0.5
      o.y = 0.5
      o.track = freeTrack(p, 'video', no, o.startMs, o.startMs + durMs, durOf) ?? no
      metaFor(p, 'video', o.track).role = 'adjust'
      p.overlays.push(o)
      id = o.id
      sel.value = { kind: 'overlay', id: o.id }
    })
    return id
  }

  /** 这个素材的轨道上，同一时间有没有别的素材（重叠）。 */
  function crowded(kind: 'overlay' | 'audio' | 'text', id: number): boolean {
    const p = project.value
    const k: EditTrackKind = kind === 'overlay' ? 'video' : kind
    const it = find({ kind, id } as Sel) as EditOverlay | EditAudio | EditText | undefined
    if (!it) return false
    const [s, e] = kind === 'overlay' ? [(it as EditOverlay).startMs, overlayEnd(it as EditOverlay)] : kind === 'audio' ? [(it as EditAudio).startMs, (it as EditAudio).startMs + durOf.audio(it as EditAudio)] : [(it as EditText).startMs, (it as EditText).endMs]
    const spans = k === 'video' ? p.overlays.filter((o) => o.track === (it as EditOverlay).track && o.id !== id).map((o) => ({ start: o.startMs, end: overlayEnd(o) })) : k === 'audio' ? p.audio.filter((a) => a.track === (it as EditAudio).track && a.id !== id).map((a) => ({ start: a.startMs, end: a.startMs + durOf.audio(a) })) : p.texts.filter((t) => t.track === (it as EditText).track && t.id !== id).map((t) => ({ start: t.startMs, end: t.endMs }))
    return spans.some((x) => x.start < e && x.end > s)
  }

  /**
   * 把一个素材放到 `no` 号轨道（`insert` 为真：先在这个位置插入一条新轨道）。
   * 目标轨道这段时间被占了，就找最近的空轨道，没有就新建一条。锁定的轨道放不进去。返回最终放在了哪条，放不了返回 null。
   */
  function moveToTrack(kind: 'overlay' | 'audio' | 'text', id: number, no: number, insert = false, key = ''): number | null {
    const tk: EditTrackKind = kind === 'overlay' ? 'video' : kind
    const it = find({ kind, id } as Sel) as EditOverlay | EditAudio | EditText | undefined
    if (!it) return null
    if (trackOf(project.value, tk, (it as { track: number }).track).lock) return null
    let result: number | null = null
    mutate((p) => {
      let target = clamp(no, tk === 'video' ? 1 : 1, maxNo(tk))
      if (insert) {
        if (nextNo(p, tk) === null) return
        shiftUp(p, tk, target)
        metaFor(p, tk, target)
      }
      const x = (kind === 'overlay' ? p.overlays : kind === 'audio' ? p.audio : p.texts).find((q) => q.id === id) as EditOverlay & EditAudio & EditText
      const [s, e] = kind === 'overlay' ? [x.startMs, overlayEnd(x)] : kind === 'audio' ? [x.startMs, x.startMs + durOf.audio(x)] : [x.startMs, x.endMs]
      const free = freeTrack(p, tk, target, s, e, durOf, id)
      if (free !== null && (free === target || !insert)) target = free
      else if (free === null) {
        const n = nextNo(p, tk)
        if (n === null) return
        metaFor(p, tk, n)
        target = n
      }
      x.track = target
      result = target
    }, key)
    return result
  }


  /** 新加进来的素材如果和同一条轨道上的别的素材时间重叠，挪到最近的空轨道（没有就新建一条）。 */
  function settleNew(p: EditProject, kind: EditTrackKind, ids: number[]) {
    const list = (kind === 'audio' ? p.audio : p.texts) as (EditAudio & EditText)[]
    const span = (x: EditAudio & EditText): [number, number] => (kind === 'audio' ? [x.startMs, x.startMs + durOf.audio(x)] : [x.startMs, x.endMs])
    for (const x of list.filter((q) => ids.includes(q.id))) {
      const [a, b] = span(x)
      const spans = list.filter((q) => q.track === x.track && q.id !== x.id && !(ids.includes(q.id) && q.id > x.id)).map(span)
      if (!spans.some(([s2, e2]) => s2 < b && e2 > a)) continue
      const free = freeTrack(p, kind, x.track, a, b, durOf, x.id)
      if (free !== null) x.track = free
      else {
        const n = nextNo(p, kind)
        if (n !== null) {
          metaFor(p, kind, n)
          x.track = n
        }
      }
    }
  }

  /**
   * 套用轨道模板：模板里的轨道按编号建好、设置覆盖进去；已经有素材的轨道只改设置，素材不动。
   * 返回新建（或改了设置）的轨道数。
   */
  function applyTrackTemplate(list: EditTrack[]): number {
    let n = 0
    mutate((p) => {
      for (const t of list) {
        if (t.no > maxNo(t.kind) || t.no < minNo(t.kind)) continue
        const m = metaFor(p, t.kind, t.no)
        Object.assign(m, { ...t, kind: t.kind, no: t.no })
        n++
      }
    })
    return n
  }

  /** 把素材放到一条新建的轨道上（这一类已经满了返回 null）。 */
  function moveToNewTrack(kind: 'overlay' | 'audio' | 'text', id: number): number | null {
    const tk: EditTrackKind = kind === 'overlay' ? 'video' : kind
    const it = find({ kind, id } as Sel) as { track: number } | undefined
    if (!it || trackOf(project.value, tk, it.track).lock) return null
    let made: number | null = null
    mutate((p) => {
      const n = nextNo(p, tk)
      if (n === null) return
      metaFor(p, tk, n)
      const x = (kind === 'overlay' ? p.overlays : kind === 'audio' ? p.audio : p.texts).find((q) => q.id === id) as { track: number } | undefined
      if (x) x.track = n
      made = n
    })
    return made
  }

  // ---------- 标记 ----------

  function addMarker(ms?: number, name = '', color: EditMarker['color'] = 'orange'): number {
    let id = 0
    mutate((p) => {
      id = p.markers.reduce((m, x) => Math.max(m, x.id), 0) + 1
      p.markers.push({ id, ms: Math.max(0, Math.round(ms ?? playhead.value)), name, color })
      p.markers.sort((a, b) => a.ms - b.ms)
    })
    return id
  }
  function patchMarker(id: number, patch: Partial<EditMarker>) {
    mutate((p) => {
      const m = p.markers.find((x) => x.id === id)
      if (m) Object.assign(m, patch)
      p.markers.sort((a, b) => a.ms - b.ms)
    })
  }
  function removeMarker(id: number) {
    mutate((p) => {
      p.markers = p.markers.filter((x) => x.id !== id)
    })
  }

  // ---------- 入点 / 出点（只导出、只预览这一段） ----------

  function setIn(ms?: number) {
    mutate((p) => {
      const t = Math.max(0, Math.round(ms ?? playhead.value))
      p.out.fromMs = t
      if (p.out.toMs && p.out.toMs <= t + MIN_MS) p.out.toMs = 0
    })
  }
  function setOut(ms?: number) {
    mutate((p) => {
      const t = Math.max(0, Math.round(ms ?? playhead.value))
      if (t > p.out.fromMs + MIN_MS) p.out.toMs = t
    })
  }
  function clearRange() {
    mutate((p) => {
      p.out.fromMs = 0
      p.out.toMs = 0
    })
  }

  // ---------- 素材 ----------

  async function probe(path: string): Promise<EditSource> {
    const known = sources[path]
    if (known) return known
    try {
      const s = await api.editProbe(path)
      sources[path] = s
      delete broken[path]
      return s
    } catch (e) {
      broken[path] = errorText(e)
      throw e
    }
  }

  /** 并发读取一批素材（每次最多 3 个）。返回的数组和 paths 一一对应，读不出来的是错误信息。 */
  async function probeAll(paths: string[]): Promise<(EditSource | string)[]> {
    const out: (EditSource | string)[] = new Array(paths.length)
    let next = 0
    const worker = async () => {
      while (next < paths.length) {
        const i = next++
        try {
          out[i] = await probe(paths[i])
        } catch (e) {
          out[i] = errorText(e)
        }
      }
    }
    await Promise.all([worker(), worker(), worker()])
    return out
  }

  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p

  /** 把视频、图片放到主轨（默认末尾，`at` 指定了就插在这个位置）；音频文件自动放到音频轨。 */
  async function addClips(paths: string[], at?: number): Promise<AddReport> {
    const report: AddReport = { added: 0, errors: [] }
    busy.value = true
    try {
      const infos = await probeAll(paths)
      mutate((p) => {
        let pos = at === undefined ? p.clips.length : clamp(at, 0, p.clips.length)
        infos.forEach((info, i) => {
          if (typeof info === 'string') {
            report.errors.push(`${baseName(paths[i])}：${info}`)
            return
          }
          if (info.kind === 'audio') {
            if (p.audio.length >= MAX_AUDIO) return report.errors.push(`音频轨最多 ${MAX_AUDIO} 条，${info.name} 没有加入。`)
            const a = audioFromSource(info, nextId(p), 0)
            p.audio.push(a)
            settleNew(p, 'audio', [a.id])
            report.added++
            return
          }
          if (p.clips.length >= MAX_CLIPS) return report.errors.push(`片段最多 ${MAX_CLIPS} 个，${info.name} 没有加入。`)
          const c = clipFromSource(info, nextId(p))
          p.clips.splice(pos++, 0, c)
          sel.value = { kind: 'clip', id: c.id }
          report.added++
        })
      })
    } finally {
      busy.value = false
    }
    return report
  }

  /**
   * 把视频、图片、GIF 放到叠加轨：从 `startMs`（默认播放位置）开始，多个文件首尾相接；
   * 想放的轨道（默认 1 号）这段时间被占了，就放到最近的空轨道。
   */
  async function addOverlays(paths: string[], at: { track?: number; startMs?: number } = {}): Promise<AddReport> {
    const report: AddReport = { added: 0, errors: [] }
    busy.value = true
    try {
      const infos = await probeAll(paths)
      mutate((p) => {
        let start = clamp(Math.round(at.startMs ?? playhead.value), 0, MAX_START_MS)
        infos.forEach((info, i) => {
          if (typeof info === 'string') return report.errors.push(`${baseName(paths[i])}：${info}`)
          if (info.kind === 'audio') return report.errors.push(`${info.name}：这是音频文件，请用“添加配乐”。`)
          if (p.overlays.length >= MAX_OVERLAYS) return report.errors.push(`叠加素材最多 ${MAX_OVERLAYS} 个，${info.name} 没有加入。`)
          const frame = outputSize(p, sources)
          // 同一时间已经有几个叠加素材了：新的错开一点，不正好盖住它们
          const trial = overlayFromSource(info, 0, start, 1, frame)
          const end = start + overlayDuration(trial)
          const together = p.overlays.filter((x) => x.startMs < end && overlayEnd(x) > start).length
          const o = overlayFromSource(info, nextId(p), start, at.track ?? 1, frame, together)
          o.track = pickTrack(p.overlays, o.track, o.startMs, end)
          p.overlays.push(o)
          sel.value = { kind: 'overlay', id: o.id }
          report.added++
          start = clamp(end, 0, MAX_START_MS)
        })
      })
    } finally {
      busy.value = false
    }
    return report
  }

  /** 添加配乐（音频文件，或者想用它声音的视频）。从 `startMs`（默认播放位置）开始。 */
  async function addMusic(paths: string[], startMs?: number, select = true, track?: number): Promise<AddReport> {
    const report: AddReport = { added: 0, errors: [] }
    busy.value = true
    try {
      const infos = await probeAll(paths)
      mutate((p) => {
        infos.forEach((info, i) => {
          if (typeof info === 'string') return report.errors.push(`${baseName(paths[i])}：${info}`)
          if (!info.hasAudio) return report.errors.push(`${info.name}：这个文件里没有声音。`)
          if (p.audio.length >= MAX_AUDIO) return report.errors.push(`音频轨最多 ${MAX_AUDIO} 条，${info.name} 没有加入。`)
          const a = audioFromSource(info, nextId(p), Math.round(Math.max(0, startMs ?? playhead.value)))
          a.track = clamp(track ?? 1, 1, 12)
          p.audio.push(a)
          settleNew(p, 'audio', [a.id])
          if (select) sel.value = { kind: 'audio', id: a.id }
          report.added++
        })
      })
    } finally {
      busy.value = false
    }
    return report
  }

  function addText(track?: number): boolean {
    if (project.value.texts.length >= MAX_TEXTS) return false
    mutate((p) => {
      const t = textAt(nextId(p), Math.round(playhead.value))
      t.track = clamp(track ?? 1, 1, 8)
      p.texts.push(t)
      settleNew(p, 'text', [t.id])
      sel.value = { kind: 'text', id: t.id }
    })
    return true
  }

  /** 导入字幕文件：每条字幕变成一条文字。最多放得下 MAX_TEXTS 条，超出的丢掉。返回 { added, skipped }。 */
  async function importSubtitles(path: string, offsetMs = 0): Promise<{ added: number; skipped: number }> {
    const cues = await api.editSubtitleCues(path)
    const room = Math.max(0, MAX_TEXTS - project.value.texts.length)
    const take = cues.slice(0, room)
    if (take.length) {
      mutate((p) => {
        const ids: number[] = []
        for (const c of take) {
          const t = textFromCue(nextId(p), c, offsetMs)
          p.texts.push(t)
          ids.push(t.id)
        }
        settleNew(p, 'text', ids)
        sel.value = null
      })
    }
    return { added: take.length, skipped: cues.length - take.length }
  }

  // ---------- 编辑操作 ----------

  /** 在播放位置分割。选中了叠加素材 / 片段就分割它，否则分割播放位置下的主轨片段。成功返回 true。 */
  function splitAtPlayhead(): boolean {
    const t = Math.round(playhead.value)
    const ov = selectedOverlay.value
    if (ov) {
      const parts = splitOverlay(ov, t, nextId(project.value), sources[ov.path]?.durationMs)
      if (!parts) return false
      mutate((p) => {
        const i = p.overlays.findIndex((x) => x.id === ov.id)
        if (i >= 0) p.overlays.splice(i, 1, parts[0], parts[1])
      })
      sel.value = { kind: 'overlay', id: parts[1].id }
      return true
    }
    const pl = placed.value
    const idx = selectedClip.value ? project.value.clips.findIndex((c) => c.id === selectedClip.value?.id) : clipIndexAt(pl, t)
    if (idx < 0) return false
    const parts = splitClip(project.value.clips[idx], pl[idx], t, nextId(project.value))
    if (!parts) return false
    mutate((p) => {
      p.clips.splice(idx, 1, parts[0], parts[1])
    })
    sel.value = { kind: 'clip', id: parts[1].id }
    return true
  }


  /** 刀片：在 `ms` 处把这个素材切成两段（片段、叠加素材、配乐、文字都行）。切不开（离两端太近、循环的配乐）返回 false。 */
  function bladeAt(kind: 'clip' | 'overlay' | 'audio' | 'text', id: number, ms: number): boolean {
    const t = Math.round(ms)
    if (kind === 'clip' || kind === 'overlay') {
      playhead.value = clamp(t, 0, Math.max(0, total.value))
      sel.value = { kind, id }
      return splitAtPlayhead()
    }
    let ok = false
    mutate((p) => {
      if (kind === 'audio') {
        const a = p.audio.find((x) => x.id === id)
        if (!a || a.looped) return
        const span = durOf.audio(a)
        if (t < a.startMs + MIN_MS || t > a.startMs + span - MIN_MS) return
        const cut = t - a.startMs
        const b: EditAudio = { ...a, keyframes: {}, beats: null, id: nextId(p), startMs: t, inMs: a.inMs + cut, fadeInMs: 0 }
        a.outMs = a.inMs + cut
        a.fadeOutMs = 0
        // 音量线在切开处分成两半
        const [k1, k2] = kfSplit(a.keyframes ?? {}, cut)
        a.keyframes = k1
        b.keyframes = k2
        p.audio.splice(p.audio.indexOf(a) + 1, 0, b)
        sel.value = { kind: 'audio', id: b.id }
        ok = true
      } else {
        const x = p.texts.find((q) => q.id === id)
        if (!x || t < x.startMs + MIN_MS || t > x.endMs - MIN_MS) return
        const b: EditText = { ...x, keyframes: {}, id: nextId(p), startMs: t, words: [], highlight: '' }
        x.endMs = t
        x.words = []
        x.highlight = ''
        const [k1, k2] = kfSplit(x.keyframes ?? {}, t - x.startMs)
        x.keyframes = k1
        b.keyframes = k2
        p.texts.splice(p.texts.indexOf(x) + 1, 0, b)
        sel.value = { kind: 'text', id: b.id }
        ok = true
      }
    })
    return ok
  }

  function removeSelected() {
    const list = selection.value
    if (!list.length) return
    const has = (kind: Sel['kind'], id: number) => list.some((s) => s.kind === kind && s.id === id)
    mutate((p) => {
      p.clips = p.clips.filter((c) => !has('clip', c.id))
      p.overlays = p.overlays.filter((c) => !has('overlay', c.id))
      p.texts = p.texts.filter((c) => !has('text', c.id))
      p.audio = p.audio.filter((c) => !has('audio', c.id))
    })
    sel.value = null
    playhead.value = clamp(playhead.value, 0, total.value)
  }

  /**
   * 波纹删除选中的片段：主轨后面的片段本来就会补上来，这里再把整个在它之后才开始的叠加素材、文字、配乐一起前移同样的时间，
   * 让它们和原来对应的画面保持对齐。落在被删片段时间范围里的不动。选中的不是主轨片段返回 null，否则返回一起前移了几个。
   */
  function rippleDeleteSelected(): number | null {
    const s = sel.value
    if (!s || s.kind !== 'clip') return null
    const i = project.value.clips.findIndex((c) => c.id === s.id)
    if (i < 0) return null
    const from = placed.value[i].endMs
    const before = total.value
    let moved = 0
    mutate((p) => {
      p.clips = p.clips.filter((c) => c.id !== s.id)
      moved = shiftAfter(p, from, before - totalMs(p.clips))
    })
    sel.value = null
    playhead.value = clamp(playhead.value, 0, total.value)
    return moved
  }

  /** 把播放位置之前 / 之后的部分裁掉。选中了叠加素材 / 片段就裁它，否则裁播放位置下的主轨片段。成功返回 true。 */
  function trimAtPlayhead(side: 'in' | 'out'): boolean {
    const t = Math.round(playhead.value)
    const ov = selectedOverlay.value
    if (ov) {
      const next = trimOverlayAt(ov, t, side, sources[ov.path]?.durationMs)
      if (!next) return false
      mutate((p) => {
        const k = p.overlays.findIndex((x) => x.id === ov.id)
        if (k >= 0) p.overlays.splice(k, 1, next)
      })
      return true
    }
    const pl = placed.value
    const idx = selectedClip.value ? project.value.clips.findIndex((c) => c.id === selectedClip.value?.id) : clipIndexAt(pl, t)
    if (idx < 0) return false
    const next = trimClipAt(project.value.clips[idx], pl[idx], t, side)
    if (!next) return false
    mutate((p) => {
      p.clips.splice(idx, 1, next)
    })
    sel.value = { kind: 'clip', id: next.id }
    return true
  }

  /** 把选中的片段 / 叠加素材的声音分离到配乐轨（原来的那个静音）。成功返回 null，否则返回原因。 */
  function detachSelectedAudio(): string | null {
    const c = selectedClip.value
    const ov = selectedOverlay.value
    if (!c && !ov) return '请先选中一个视频片段或叠加素材。'
    if (project.value.audio.length >= MAX_AUDIO) return `音频轨最多 ${MAX_AUDIO} 条。`
    const id = nextId(project.value)
    const idx = c ? project.value.clips.findIndex((x) => x.id === c.id) : -1
    const a = c ? detachedAudio(c, placed.value[idx].startMs, id, sources[c.path]) : detachedAudio(ov!, ov!.startMs, id, sources[ov!.path])
    if (typeof a === 'string') return a
    mutate((p) => {
      p.audio.push(a)
      const target = c ? p.clips.find((x) => x.id === c.id) : p.overlays.find((x) => x.id === ov!.id)
      if (target) target.mute = true
    })
    sel.value = { kind: 'audio', id: a.id }
    return null
  }

  /** 把选中片段的转场 / 调色 / 画面方向应用到其他所有片段，返回变了几个。 */
  function applyToAll(what: 'transition' | 'tone' | 'turn' | 'lut'): number {
    const c = selectedClip.value
    if (!c) return 0
    let n = 0
    mutate((p) => {
      n = applyToAllClips(p, c.id, what)
    })
    return n
  }
  function removeAllTransitions(): number {
    let n = 0
    mutate((p) => {
      n = clearTransitions(p)
    })
    return n
  }

  /** 跳到前一个 / 后一个剪辑点（主轨片段、叠加素材、文字的起止）。没有更远的了返回 null。 */
  function cutPointFrom(dir: -1 | 1): number | null {
    // 播放位置不会超出主轨结尾，更远的剪辑点去不了
    const pts = editPoints(project.value, placed.value).filter((x) => x <= total.value)
    return neighborPoint(pts, playhead.value, dir)
  }
  /** 往前 / 往后一帧后的播放位置 */
  function frameFrom(dir: -1 | 1): number {
    return stepFrame(playhead.value, playbackFps(project.value, placed.value, playhead.value, sources), dir)
  }

  function duplicateSelected(): boolean {
    const s = sel.value
    if (!s) return false
    let ok = false
    mutate((p) => {
      const id = nextId(p)
      if (s.kind === 'clip') {
        const i = p.clips.findIndex((c) => c.id === s.id)
        if (i < 0 || p.clips.length >= MAX_CLIPS) return
        p.clips.splice(i + 1, 0, { ...p.clips[i], id, transition: null, regions: p.clips[i].regions.map(copyRegion) })
      } else if (s.kind === 'overlay') {
        const o = p.overlays.find((c) => c.id === s.id)
        if (!o || p.overlays.length >= MAX_OVERLAYS) return
        // 接在原来的后面；那一段这条轨道被占了就换一条
        const start = clamp(overlayEnd(o), 0, MAX_START_MS)
        const copy = { ...o, id, startMs: start }
        copy.track = pickTrack(p.overlays, o.track, start, start + overlayDuration(o))
        p.overlays.push(copy)
      } else if (s.kind === 'text') {
        const t = p.texts.find((c) => c.id === s.id)
        if (!t || p.texts.length >= MAX_TEXTS) return
        const len = t.endMs - t.startMs
        p.texts.push({ ...t, id, startMs: t.endMs, endMs: t.endMs + len })
      } else {
        const a = p.audio.find((c) => c.id === s.id)
        if (!a || p.audio.length >= MAX_AUDIO) return
        p.audio.push({ ...a, id })
      }
      sel.value = { kind: s.kind, id }
      ok = true
    })
    return ok
  }

  function reorderClip(from: number, to: number) {
    if (from === to || from < 0 || from >= project.value.clips.length) return
    mutate((p) => {
      p.clips = moveItem(p.clips, from, to)
    })
  }

  /** 把主轨上的片段移到叠加轨（留在原来的时间上，缩放到刚好放进画面）。区域不能带过去。返回丢掉了几个区域，失败返回 null。 */
  function clipToOverlayTrack(id: number): number | null {
    const i = project.value.clips.findIndex((c) => c.id === id)
    if (i < 0 || project.value.overlays.length >= MAX_OVERLAYS) return null
    const c = project.value.clips[i]
    const pl = placed.value[i]
    const frame = outputSize(project.value, sources)
    const o = clipToOverlay(c, pl, c.id, 1, sources[c.path], frame)
    mutate((p) => {
      o.track = pickTrack(p.overlays, 1, o.startMs, o.startMs + overlayDuration(o))
      p.clips.splice(i, 1)
      p.overlays.push(o)
    })
    sel.value = { kind: 'overlay', id: o.id }
    return c.regions.length
  }

  /** 把叠加素材放到主轨：按它的开始时间插在合适的位置。返回是否成功。 */
  function overlayToMain(id: number): boolean {
    const o = overlayOf(id)
    if (!o || project.value.clips.length >= MAX_CLIPS) return false
    const c = overlayToClip(o, o.id, sources[o.path]?.durationMs)
    const mid = placed.value.map((pl) => (pl.startMs + pl.endMs) / 2)
    const at = mid.filter((m) => m < o.startMs).length
    mutate((p) => {
      p.overlays = p.overlays.filter((x) => x.id !== id)
      p.clips.splice(at, 0, c)
    })
    sel.value = { kind: 'clip', id: c.id }
    return true
  }

  /** 把一个素材路径换成另一个（找不到文件时重新指定），所有用到它的片段、叠加素材、音频一起换。 */
  async function relink(oldPath: string, newPath: string) {
    await probe(newPath)
    mutate((p) => {
      for (const c of p.clips) if (c.path === oldPath) c.path = newPath
      for (const o of p.overlays) if (o.path === oldPath) o.path = newPath
      for (const a of p.audio) if (a.path === oldPath) a.path = newPath
    })
    delete broken[oldPath]
  }

  /** 工程里可用的素材信息（缩略图、时长等）。 */
  const sourceOf = (path: string): EditSource | undefined => sources[path]

  // ---------- 新建 / 打开 / 保存 ----------

  function reset(next: EditProject, path = '') {
    compoundStack.value = []
    project.value = next
    filePath.value = path
    sel.value = null
    playhead.value = 0
    playing.value = false
    undoStack.value = []
    redoStack.value = []
    lastKey = ''
    dirty.value = false
  }

  function newProject() {
    reset(emptyProject())
  }

  /** 读取工程里所有素材的信息；找不到的记入 `broken`。 */
  async function loadSources(p: EditProject, missing: string[] = []) {
    for (const m of missing) broken[m] = '找不到文件'
    const paths = mediaPaths(p).filter((m) => !missing.includes(m) && !/\.(ttf|otf|ttc)$/i.test(m))
    busy.value = true
    try {
      await probeAll(paths)
    } finally {
      busy.value = false
    }
  }

  async function openProject(path: string): Promise<string[]> {
    const r = await api.editOpen(path)
    const p = normalizeProject(r.project)
    for (const k of Object.keys(broken)) delete broken[k]
    reset(p, path)
    remember(path)
    await loadSources(p, r.missing)
    return r.missing
  }

  async function saveProject(path: string) {
    await api.editSave(path, JSON.parse(rootJson.value))
    filePath.value = path
    dirty.value = false
    remember(path)
  }

  // ---------- 草稿（退出或崩溃后能找回） ----------

  let draftTimer: ReturnType<typeof setTimeout> | undefined
  watch(
    project,
    () => {
      clearTimeout(draftTimer)
      draftTimer = setTimeout(() => {
        try {
          const p = rootProject.value
          if (!p.clips.length && !p.overlays.length && !p.texts.length && !p.audio.length) localStorage.removeItem(DRAFT_KEY)
          else localStorage.setItem(DRAFT_KEY, JSON.stringify({ project: p, filePath: filePath.value }))
        } catch {
          /* 存不了草稿不影响使用 */
        }
      }, 800)
    },
    { deep: true },
  )

  function readDraft(): { project: EditProject; filePath: string } | null {
    try {
      const raw = localStorage.getItem(DRAFT_KEY)
      if (!raw) return null
      const d = JSON.parse(raw) as { project?: EditProject; filePath?: string }
      const p = normalizeProject(d.project)
      return p.clips.length || p.overlays.length || p.texts.length || p.audio.length ? { project: p, filePath: d.filePath ?? '' } : null
    } catch {
      return null
    }
  }
  function clearDraft() {
    try {
      localStorage.removeItem(DRAFT_KEY)
    } catch {
      /* 同上 */
    }
  }
  async function restoreDraft(d: { project: EditProject; filePath: string }) {
    const gone = await api.editMissing(mediaPaths(d.project).filter((m) => !/\.(ttf|otf|ttc)$/i.test(m)))
    for (const k of Object.keys(broken)) delete broken[k]
    reset(d.project, d.filePath)
    dirty.value = true
    await loadSources(d.project, gone)
  }

  return {
    project,
    sources,
    broken,
    sel,
    extra,
    selection,
    isSelected,
    toggleSel,
    makeCompound,
    enterCompound,
    exitCompound,
    compoundStack,
    compoundPath,
    rootProject,
    rootJson,
    groupSelected,
    ungroupSelected,
    groupMates,
    playhead,
    playing,
    filePath,
    dirty,
    busy,
    recent,
    forgetRecent: forget,
    placed,
    total,
    extent,
    overlayLane,
    canUndo,
    canRedo,
    selectedClip,
    selectedOverlay,
    selectedText,
    selectedAudio,
    mutate,
    live,
    patchTrack,
    addTrack,
    insertTrack,
    removeTrack,
    moveTrack,
    setTrackRole,
    clearVideoTrack,
    addAdjust,
    crowded,
    moveToTrack,
    moveToNewTrack,
    applyTrackTemplate,
    addMarker,
    patchMarker,
    removeMarker,
    setIn,
    setOut,
    clearRange,
    undo,
    redo,
    clipOf,
    overlayOf,
    textOf,
    audioOf,
    patchClip,
    cutClipAt,
    removeClipSilences,
    addCaptions,
    putRegion,
    patchOverlay,
    patchText,
    setOverlayProps,
    setTextProps,
    setKey,
    applySpeedRamp,
    applySmoothRamp,
    freezeAtPlayhead,
    arrangeToBeatsOf,
    fitAudioToVideo,
    deleteKey,
    clearKeys,
    patchAudio,
    probe,
    addClips,
    addOverlays,
    addMusic,
    addText,
    importSubtitles,
    proxies,
    proxying,
    makeProxy,
    proxyBig,
    autoProxy,
    meterOn,
    levels,
    clips,
    splitAtPlayhead,
    bladeAt,
    removeSelected,
    rippleDeleteSelected,
    trimAtPlayhead,
    detachSelectedAudio,
    applyToAll,
    removeAllTransitions,
    cutPointFrom,
    frameFrom,
    duplicateSelected,
    reorderClip,
    clipToOverlayTrack,
    overlayToMain,
    relink,
    sourceOf,
    newProject,
    openProject,
    saveProject,
    readDraft,
    clearDraft,
    restoreDraft,
    clipDuration,
    MIN_MS,
    MAX_OVERLAY_TRACKS,
  }
}

export type EditApi = ReturnType<typeof useEditProject>
