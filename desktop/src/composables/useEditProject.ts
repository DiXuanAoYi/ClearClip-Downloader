// 剪辑工程的状态：工程内容、素材信息、选中项、播放位置、撤销 / 重做、打开 / 保存、草稿。

import { computed, reactive, ref, watch } from 'vue'
import { api, errorText } from '../api'
import type { EditAudio, EditClip, EditOverlay, EditProject, EditRegion, EditSource, EditText, TimeSpan } from '../types'
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
  emptyProject,
  layout,
  mediaPaths,
  moveItem,
  nextId,
  nextRegionId,
  removeSilences,
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
} from '../utils/edit'

export interface Sel {
  kind: 'clip' | 'overlay' | 'text' | 'audio'
  id: number
}

const DRAFT_KEY = 'clearclip.edit.draft'
const RECENT_KEY = 'clearclip.edit.recent'
const RECENT_LIMIT = 8
const HISTORY_LIMIT = 100

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
  /** 工程里用到、但现在找不到或读不出来的文件 */
  const broken = reactive<Record<string, string>>({})
  const sel = ref<Sel | null>(null)
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
      if (c) Object.assign(c, patch)
    }, key)
  }
  function patchText(id: number, patch: Partial<EditText>, key = '') {
    mutate((p) => {
      const c = p.texts.find((x) => x.id === id)
      if (c) Object.assign(c, patch)
    }, key)
  }
  function patchAudio(id: number, patch: Partial<EditAudio>, key = '') {
    mutate((p) => {
      const c = p.audio.find((x) => x.id === id)
      if (c) Object.assign(c, patch)
    }, key)
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
  async function addMusic(paths: string[], startMs?: number): Promise<AddReport> {
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
          p.audio.push(a)
          sel.value = { kind: 'audio', id: a.id }
          report.added++
        })
      })
    } finally {
      busy.value = false
    }
    return report
  }

  function addText(): boolean {
    if (project.value.texts.length >= MAX_TEXTS) return false
    mutate((p) => {
      const t = textAt(nextId(p), Math.round(playhead.value))
      p.texts.push(t)
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
        for (const c of take) p.texts.push(textFromCue(nextId(p), c, offsetMs))
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

  function removeSelected() {
    const s = sel.value
    if (!s) return
    mutate((p) => {
      if (s.kind === 'clip') p.clips = p.clips.filter((c) => c.id !== s.id)
      else if (s.kind === 'overlay') p.overlays = p.overlays.filter((c) => c.id !== s.id)
      else if (s.kind === 'text') p.texts = p.texts.filter((c) => c.id !== s.id)
      else p.audio = p.audio.filter((c) => c.id !== s.id)
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
  function applyToAll(what: 'transition' | 'tone' | 'turn'): number {
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
    await api.editSave(path, JSON.parse(snapshot()))
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
          const p = project.value
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
    undo,
    redo,
    clipOf,
    overlayOf,
    textOf,
    audioOf,
    patchClip,
    cutClipAt,
    removeClipSilences,
    putRegion,
    patchOverlay,
    patchText,
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
    splitAtPlayhead,
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
