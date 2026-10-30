// 剪辑工程的纯计算：时间线排布、新建片段、分割、取时间等。
// 排布规则和后端 `src-tauri/src/edit/spec.rs` 的 `layout` 保持一致，改那边时要一起改这里。
// 这个文件不要引入别的运行时模块（用 node 直接跑 scripts 里的检查时需要）。

import type { EditAudio, EditBeats, EditChroma, EditClip, EditKeyframe, EditKeyframes, EditOutput, EditOverlay, EditProject, EditRegion, EditSource, EditText, NRect, RegionEffect, TimeSpan, TrackPt, TranscriptWord } from '../types'

export const MIN_MS = 100
export const MIN_TRANSITION_MS = 100
export const MAX_CLIPS = 300
export const MAX_TEXTS = 500
export const MAX_AUDIO = 20
/** 叠加素材最多几个、几条轨道、最晚从哪里开始（和后端 `MAX_OVERLAYS` / `MAX_OVERLAY_TRACKS` / `MAX_START_MS` 一致） */
export const MAX_OVERLAYS = 100
export const MAX_OVERLAY_TRACKS = 8
export const MAX_START_MS = 36_000_000
/** 一个片段最多几个区域（和后端 `MAX_REGIONS` 一致） */
export const MAX_REGIONS = 8
/** 图片、文字默认的时长 */
export const DEFAULT_STILL_MS = 5000
export const DEFAULT_TEXT_MS = 3000

export const TRANSITIONS: { id: string; label: string }[] = [
  { id: 'fade', label: '淡入淡出' },
  { id: 'fadeblack', label: '闪黑' },
  { id: 'fadewhite', label: '闪白' },
  { id: 'dissolve', label: '溶解' },
  { id: 'wipeleft', label: '向左擦除' },
  { id: 'wiperight', label: '向右擦除' },
  { id: 'wipeup', label: '向上擦除' },
  { id: 'wipedown', label: '向下擦除' },
  { id: 'slideleft', label: '向左滑动' },
  { id: 'slideright', label: '向右滑动' },
  { id: 'slideup', label: '向上滑动' },
  { id: 'slidedown', label: '向下滑动' },
  { id: 'circleopen', label: '圆形展开' },
  { id: 'circleclose', label: '圆形收拢' },
  { id: 'radial', label: '时钟擦除' },
  { id: 'pixelize', label: '马赛克' },
]

/** 剪辑能读的文件格式（扩展名）。要和后端 `edit/cmds.rs` 的 `media_kind` 保持一致；真正能不能解码最后以 ffmpeg 为准，读不出来会说明原因。 */
export const EDIT_VIDEO_EXTS = ['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'ts', 'm4v', 'wmv', 'mpg', 'mpeg', '3gp', '3g2', 'm2ts', 'mts', 'ogv', 'vob', 'f4v', 'asf', 'divx', 'mxf']
export const EDIT_IMAGE_EXTS = ['jpg', 'jpeg', 'jfif', 'png', 'webp', 'gif', 'bmp', 'tif', 'tiff', 'avif', 'heic', 'heif']
export const EDIT_AUDIO_EXTS = ['mp3', 'm4a', 'flac', 'wav', 'ogg', 'opus', 'aac', 'wma', 'aif', 'aiff', 'ac3', 'mka', 'amr']

/** 按扩展名判断文件是视频、图片还是音频；不是剪辑能用的格式返回 null。 */
export function editMediaKind(path: string): 'video' | 'image' | 'audio' | null {
  const ext = /\.([^./\\]+)$/.exec(path)?.[1]?.toLowerCase() ?? ''
  if (EDIT_VIDEO_EXTS.includes(ext)) return 'video'
  if (EDIT_IMAGE_EXTS.includes(ext)) return 'image'
  if (EDIT_AUDIO_EXTS.includes(ext)) return 'audio'
  return null
}

export interface Placed {
  startMs: number
  durMs: number
  /** 开头和上一个片段重叠多少（转场时长，已按能放下的长度收紧） */
  overlapMs: number
  endMs: number
}

export const clamp = (v: number, lo: number, hi: number) => (Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : lo)

export function defaultOutput(): EditOutput {
  return { width: 0, height: 0, fps: 0, fit: 'contain', quality: 'balanced', codec: 'h264', format: 'mp4' }
}

export function emptyProject(): EditProject {
  return { title: '', clips: [], overlays: [], texts: [], audio: [], out: defaultOutput() }
}

/** 补全旧版本或手改过的工程文件里缺的字段。 */
export function normalizeProject(raw: Partial<EditProject> | null | undefined): EditProject {
  const p = emptyProject()
  const r = raw ?? {}
  p.title = typeof r.title === 'string' ? r.title : ''
  p.out = { ...defaultOutput(), ...(r.out ?? {}) }
  p.clips = (r.clips ?? []).map((c, i) => ({ ...blankClip(), ...c, id: c.id ?? i + 1, regions: (c.regions ?? []).map((g, j) => ({ ...blankRegion(), ...g, id: g.id ?? j + 1 })) }))
  p.overlays = (r.overlays ?? []).map((o, i) => ({ ...blankOverlay(), ...o, id: o.id ?? i + 1, keyframes: kfClean(o.keyframes), chroma: { ...blankChroma(), ...(o.chroma ?? {}) } }))
  p.texts = (r.texts ?? []).map((t, i) => ({ ...blankText(), ...t, id: t.id ?? i + 1, keyframes: kfClean(t.keyframes) }))
  p.audio = (r.audio ?? []).map((a, i) => ({ ...blankAudio(), ...a, id: a.id ?? i + 1, beats: cleanBeats(a.beats) }))
  return p
}

function blankClip(): EditClip {
  return {
    id: 0,
    path: '',
    kind: 'video',
    inMs: 0,
    outMs: 0,
    speed: 1,
    volume: 1,
    mute: false,
    fadeInMs: 0,
    fadeOutMs: 0,
    rotate: 0,
    flipH: false,
    flipV: false,
    brightness: 0,
    contrast: 1,
    saturation: 1,
    denoise: 'off',
    enhance: false,
    noiseFloorDb: null,
    reverse: false,
    freeze: false,
    regions: [],
    transition: null,
  }
}

export function blankOverlay(): EditOverlay {
  return {
    id: 0,
    path: '',
    kind: 'video',
    track: 1,
    startMs: 0,
    inMs: 0,
    outMs: 0,
    speed: 1,
    looped: false,
    volume: 1,
    mute: false,
    fadeInMs: 0,
    fadeOutMs: 0,
    animIn: 'none',
    animOut: 'none',
    animMs: 500,
    x: 0.5,
    y: 0.5,
    scale: 0.4,
    rotate: 0,
    opacity: 1,
    flipH: false,
    flipV: false,
    brightness: 0,
    contrast: 1,
    saturation: 1,
    keyframes: {},
    chroma: blankChroma(),
  }
}

export function blankChroma(): EditChroma {
  return { on: false, color: '#00FF00', similarity: 0.18, blend: 0.08, despill: true }
}

function blankText(): EditText {
  return {
    id: 0,
    text: '',
    startMs: 0,
    endMs: DEFAULT_TEXT_MS,
    x: 0.5,
    y: 0.85,
    size: 6,
    color: '#FFFFFF',
    opacity: 1,
    outline: true,
    outlineColor: '#000000',
    boxed: false,
    boxColor: '#000000',
    boxOpacity: 0.5,
    font: null,
    words: [],
    highlight: '',
    fadeMs: 0,
    keyframes: {},
  }
}

function blankAudio(): EditAudio {
  return { id: 0, path: '', startMs: 0, inMs: 0, outMs: null, volume: 1, fadeInMs: 0, fadeOutMs: 0, looped: false, duck: false, beats: null }
}

export function nextId(p: EditProject): number {
  let m = 0
  for (const c of p.clips) m = Math.max(m, c.id)
  for (const o of p.overlays) m = Math.max(m, o.id)
  for (const t of p.texts) m = Math.max(m, t.id)
  for (const a of p.audio) m = Math.max(m, a.id)
  return m + 1
}

/** 素材放上时间线时的默认片段：视频取整段，图片显示 5 秒。 */
export function clipFromSource(src: EditSource, id: number): EditClip {
  const c = blankClip()
  c.id = id
  c.path = src.path
  if (src.kind === 'image') {
    c.kind = 'image'
    c.outMs = DEFAULT_STILL_MS
  } else {
    c.outMs = Math.max(MIN_MS, src.durationMs ?? DEFAULT_STILL_MS)
  }
  return c
}

export function textAt(id: number, startMs: number): EditText {
  const t = blankText()
  t.id = id
  t.text = '输入文字'
  t.startMs = startMs
  t.endMs = startMs + DEFAULT_TEXT_MS
  return t
}

/** 字幕文件里的一条 → 画面底部的一条文字（字号比普通文字小一点，带描边）。 */
export function textFromCue(id: number, cue: { startMs: number; endMs: number; text: string }, offsetMs: number): EditText {
  const t = blankText()
  t.id = id
  t.text = cue.text
  t.startMs = cue.startMs + offsetMs
  t.endMs = cue.endMs + offsetMs
  t.size = 5
  t.y = 0.9
  return t
}

export function audioFromSource(src: EditSource, id: number, startMs: number): EditAudio {
  const a = blankAudio()
  a.id = id
  a.path = src.path
  a.startMs = startMs
  return a
}

/** 片段在时间线上占的时长（毫秒）。 */
export function clipDuration(c: EditClip): number {
  const span = Math.max(0, c.outMs - c.inMs)
  return c.kind === 'image' || c.freeze ? span : Math.round(span / Math.max(0.01, c.speed))
}

export function layout(clips: EditClip[]): Placed[] {
  const d = clips.map(clipDuration)
  const out: Placed[] = []
  let prevOverlap = 0
  clips.forEach((c, i) => {
    let overlap = 0
    if (i > 0) {
      const want = c.transition?.durationMs ?? 0
      const fit = Math.min(want, Math.max(0, d[i - 1] - prevOverlap), d[i])
      overlap = fit < MIN_TRANSITION_MS ? 0 : fit
    }
    const startMs = i === 0 ? 0 : out[i - 1].endMs - overlap
    out.push({ startMs, durMs: d[i], overlapMs: overlap, endMs: startMs + d[i] })
    prevOverlap = overlap
  })
  return out
}

export function totalMs(clips: EditClip[]): number {
  const l = layout(clips)
  return l.length ? l[l.length - 1].endMs : 0
}

/** 时间线上 t 时刻看到的是哪个片段（转场重叠处取后一个）。超出范围返回 -1。 */
export function clipIndexAt(placed: Placed[], t: number): number {
  for (let i = placed.length - 1; i >= 0; i--) {
    if (t >= placed[i].startMs && t < placed[i].endMs) return i
  }
  return -1
}

/** t 时刻对应片段里的素材时间（毫秒）。 */
export function sourceTimeAt(c: EditClip, pl: Placed, t: number): number {
  if (c.kind === 'image') return 0
  if (c.freeze) return c.inMs
  const local = clamp(t - pl.startMs, 0, pl.durMs)
  if (c.reverse) return clamp(c.outMs - local * c.speed, c.inMs, c.outMs)
  return clamp(c.inMs + local * c.speed, c.inMs, c.outMs)
}

/** 画面淡入淡出在 t 时刻的亮度系数（0–1）。 */
export function fadeFactor(c: EditClip, pl: Placed, t: number): number {
  const local = t - pl.startMs
  let f = 1
  if (c.fadeInMs > 0) f = Math.min(f, local / c.fadeInMs)
  if (c.fadeOutMs > 0) f = Math.min(f, (pl.durMs - local) / c.fadeOutMs)
  return clamp(f, 0, 1)
}

/** 在时间线 t 处把片段一分为二。离两端不足 MIN_MS 时返回 null。第二段不带转场，淡入归第一段、淡出归第二段。 */
export function splitClip(c: EditClip, pl: Placed, t: number, newId: number): [EditClip, EditClip] | null {
  const local = t - pl.startMs
  if (local < MIN_MS || pl.durMs - local < MIN_MS) return null
  const first: EditClip = { ...c, transition: c.transition ? { ...c.transition } : null, regions: c.regions.map(copyRegion) }
  const second: EditClip = { ...c, id: newId, transition: null, regions: c.regions.map(copyRegion) }
  if (c.kind === 'image') {
    first.outMs = local
    second.outMs = pl.durMs - local
  } else if (c.freeze) {
    // 定格：两半都停在同一帧，各管一段时长
    first.outMs = c.inMs + local
    second.outMs = c.inMs + pl.durMs - local
  } else if (c.reverse) {
    // 倒放：时间线上前半段对应素材的后半段
    const cut = Math.round(c.outMs - local * c.speed)
    first.inMs = cut
    second.outMs = cut
  } else {
    const cut = Math.round(c.inMs + local * c.speed)
    first.outMs = cut
    second.inMs = cut
  }
  first.fadeOutMs = 0
  second.fadeInMs = 0
  if (c.kind === 'video' && !c.freeze) {
    // 区域的轨迹用的是素材时间，两半各留下自己那一段（边上多留一个点，插值才对得上）
    first.regions = first.regions.map((r) => ({ ...r, track: trimTrack(r.track, first.inMs, first.outMs) }))
    second.regions = second.regions.map((r) => ({ ...r, track: trimTrack(r.track, second.inMs, second.outMs) }))
  }
  return [first, second]
}

/**
 * 把一个主轨片段按“保留这些素材时间段”重新切成几个片段（段与段之间的部分被丢掉）。
 * 第一段保留淡入和转场，最后一段保留淡出；区域的轨迹各留各的那一段。`ids` 是给新片段用的编号（第一段沿用原来的）。
 * 太短（< MIN_MS）的段被丢掉；没有可保留的返回空数组。图片不处理（返回原样）。
 */
export function keepRanges(c: EditClip, keeps: [number, number][], ids: () => number): EditClip[] {
  if (c.kind !== 'video' || c.reverse || c.freeze) return [c]
  const spans = keeps
    .map(([a, b]) => [Math.max(c.inMs, Math.round(a)), Math.min(c.outMs, Math.round(b))] as [number, number])
    .filter(([a, b]) => b - a >= MIN_MS)
    .sort((x, y) => x[0] - y[0])
  const out = spans.map(([a, b], i) => {
    const piece: EditClip = { ...c, id: i === 0 ? c.id : ids(), inMs: a, outMs: b, transition: i === 0 && c.transition ? { ...c.transition } : null, regions: c.regions.map(copyRegion) }
    if (i > 0) piece.fadeInMs = 0
    if (i < spans.length - 1) piece.fadeOutMs = 0
    piece.regions = piece.regions.map((r) => ({ ...r, track: trimTrack(r.track, a, b) }))
    return piece
  })
  return out
}

/** 变速曲线预设：把片段按素材时间等分成几段，每段一个速度（名字、各段速度）。 */
export const SPEED_RAMPS: { id: string; label: string; steps: number[]; hint: string }[] = [
  { id: 'speed-up', label: '渐快', steps: [0.5, 0.7, 1, 1.5, 2, 3], hint: '由慢到快' },
  { id: 'slow-down', label: '渐慢', steps: [3, 2, 1.5, 1, 0.7, 0.5], hint: '由快到慢' },
  { id: 'bullet', label: '子弹时间', steps: [2, 1, 0.4, 0.4, 1, 2], hint: '快 → 慢 → 快，中间最慢' },
  { id: 'pulse', label: '高潮加速', steps: [0.5, 1, 2, 2, 1, 0.5], hint: '慢 → 快 → 慢，中间最快' },
  { id: 'hit', label: '定点慢放', steps: [1, 1, 0.35, 0.35, 1, 1], hint: '只有中段慢放' },
]

/**
 * 变速曲线：把一个视频片段按素材时间等分成 `steps.length` 段，各段用各自的速度（已有的速度乘上去）。
 * 第一段保留转场和淡入，最后一段保留淡出；`ids` 给新片段用（第一段沿用原编号）。太短或不是普通视频片段返回 null。
 */
export function speedRamp(c: EditClip, steps: number[], ids: () => number): EditClip[] | null {
  if (c.kind !== 'video' || c.reverse || c.freeze || steps.length < 2) return null
  const span = c.outMs - c.inMs
  if (span < MIN_MS * steps.length) return null
  const edges = steps.map((_, i) => Math.round(c.inMs + (span * i) / steps.length)).concat(c.outMs)
  const keeps = steps.map((_, i) => [edges[i], edges[i + 1]] as [number, number])
  const pieces = keepRanges(c, keeps, ids)
  if (pieces.length !== steps.length) return null
  return pieces.map((p, i) => ({ ...p, speed: clamp(c.speed * steps[i], 0.25, 4) }))
}

/** 在时间线 t 处插一段定格：把片段切开，中间放一个只有一帧、停 `holdMs` 毫秒的片段。贴着片段两端时插在那一端。返回新的片段列表（替换原来那一个），不是普通视频片段返回 null。 */
export function freezeAt(c: EditClip, pl: Placed, t: number, holdMs: number, ids: () => number): EditClip[] | null {
  if (c.kind !== 'video' || c.freeze) return null
  const frameMs = sourceTimeAt(c, pl, t)
  const hold: EditClip = { ...c, id: ids(), inMs: Math.round(frameMs), outMs: Math.round(frameMs + Math.max(MIN_MS, holdMs)), speed: 1, reverse: false, freeze: true, fadeInMs: 0, fadeOutMs: 0, transition: null, regions: [] }
  const parts = splitClip(c, pl, t, ids())
  if (!parts) {
    const local = t - pl.startMs
    return local < pl.durMs / 2 ? [{ ...hold, transition: c.transition ? { ...c.transition } : null }, { ...c, transition: null, fadeInMs: 0 }] : [c, hold]
  }
  return [parts[0], hold, parts[1]]
}

/** 在这些素材时间点把片段切开（贴着两端或太近的忽略）。返回切出的片段，没有有效切点返回 null。 */
export function cutAtSourceTimes(c: EditClip, cuts: number[], ids: () => number): EditClip[] | null {
  if (c.kind !== 'video' || c.reverse || c.freeze) return null
  const pts = [...new Set(cuts.map(Math.round))].filter((t) => t - c.inMs >= MIN_MS && c.outMs - t >= MIN_MS).sort((a, b) => a - b)
  const good: number[] = []
  for (const t of pts) if (!good.length || t - good[good.length - 1] >= MIN_MS) good.push(t)
  if (!good.length) return null
  const edges = [c.inMs, ...good, c.outMs]
  return keepRanges(c, edges.slice(0, -1).map((a, i) => [a, edges[i + 1]] as [number, number]), ids)
}

/**
 * 去掉静音段：每个静音段两头各多留 `keepMs / 2`（让停顿不至于生硬），其余剪掉。
 * 返回新的片段列表（没有静音可去时返回 null）和被去掉的素材时长（毫秒）。
 */
export function removeSilences(c: EditClip, silences: TimeSpan[], keepMs: number, ids: () => number): { clips: EditClip[]; removedMs: number } | null {
  if (c.kind !== 'video' || c.reverse || c.freeze) return null
  const half = Math.max(0, keepMs) / 2
  const cuts: [number, number][] = []
  for (const s of [...silences].sort((a, b) => a.startMs - b.startMs)) {
    // 静音贴着片段的开头 / 结尾时，那一侧不用再留停顿
    const a = s.startMs <= c.inMs ? c.inMs : s.startMs + half
    const b = s.endMs >= c.outMs ? c.outMs : s.endMs - half
    if (b - a >= MIN_MS) cuts.push([a, b])
  }
  if (!cuts.length) return null
  const keeps: [number, number][] = []
  let cursor = c.inMs
  for (const [a, b] of cuts) {
    if (a > cursor) keeps.push([cursor, a])
    cursor = Math.max(cursor, b)
  }
  if (cursor < c.outMs) keeps.push([cursor, c.outMs])
  const clips = keepRanges(c, keeps, ids)
  if (!clips.length) return null
  const kept = clips.reduce((n, k) => n + (k.outMs - k.inMs), 0)
  return { clips, removedMs: c.outMs - c.inMs - kept }
}

// ---------- 文稿：词、句、口头禅、智能字幕 ----------

/** 句末标点：遇到就另起一行 */
const SENTENCE_END = /[。！？!?.…；;]$/
const CLAUSE_END = /[，、,：:]$/
const isCjk = (ch: string) => /[\u2e80-\u9fff\uf900-\ufaff\uac00-\ud7af]/.test(ch)
/** 一个字在画面上大约占几个“全角字”的宽度 */
const visualWidth = (s: string) => [...s].reduce((n, ch) => n + (isCjk(ch) ? 1 : /\s/.test(ch) ? 0.3 : 0.55), 0)

export interface Sentence {
  /** 在词表里的下标范围（含两端） */
  from: number
  to: number
  startMs: number
  endMs: number
}

/** 把词分成句：句末标点、停顿超过 `gapMs`、或太长就断开。 */
export function sentencesOf(words: TranscriptWord[], gapMs = 700, maxWords = 40): Sentence[] {
  const out: Sentence[] = []
  let from = 0
  for (let i = 0; i < words.length; i++) {
    const last = i === words.length - 1
    const w = words[i]
    const gap = last ? 0 : words[i + 1].startMs - w.endMs
    if (last || SENTENCE_END.test(w.t.trim()) || gap >= gapMs || i - from + 1 >= maxWords) {
      out.push({ from, to: i, startMs: words[from].startMs, endMs: w.endMs })
      from = i + 1
    }
  }
  return out
}

/** 和片段取的素材范围有重叠的词（下标）。 */
export function wordsInClip(words: TranscriptWord[], c: { inMs: number; outMs: number }): number[] {
  const out: number[] = []
  words.forEach((w, i) => {
    if (w.endMs > c.inMs && w.startMs < c.outMs) out.push(i)
  })
  return out
}

/** 口头禅的默认词表（逗号分隔，用户可改） */
export const DEFAULT_FILLERS = '嗯,呃,啊,额,那个,就是,然后呢,um,uh,er,erm,you know'

export const parseFillers = (s: string): string[] =>
  [...new Set(s.split(/[,，、\n]/).map((x) => normWord(x)).filter(Boolean))].sort((a, b) => b.length - a.length)

const normWord = (s: string) => s.replace(/[\s\p{P}]/gu, '').toLowerCase()

/**
 * 在词里找口头禅：几个相邻的词接起来正好等于词表里的某一项（中文每个字是一个词，所以“那个”是两个词）。
 * 返回命中的词的下标（升序、不重复）。
 */
export function findFillers(words: TranscriptWord[], list: string[]): number[] {
  const hit = new Set<number>()
  const norm = words.map((w) => normWord(w.t))
  for (let i = 0; i < words.length; i++) {
    for (const f of list) {
      let acc = ''
      for (let j = i; j < words.length && acc.length < f.length; j++) {
        acc += norm[j]
        if (!f.startsWith(acc)) break
        if (acc === f) {
          for (let k = i; k <= j; k++) hit.add(k)
          break
        }
      }
    }
  }
  return [...hit].sort((a, b) => a - b)
}

/** 选中的词 → 要剪掉的素材时间段（相邻的词合成一段）。 */
export function wordSpans(words: TranscriptWord[], picked: Iterable<number>): TimeSpan[] {
  const idx = [...new Set(picked)].filter((i) => i >= 0 && i < words.length).sort((a, b) => a - b)
  const out: TimeSpan[] = []
  let prev = -2
  for (const i of idx) {
    const w = words[i]
    if (i === prev + 1 && out.length) out[out.length - 1].endMs = Math.max(out[out.length - 1].endMs, w.endMs)
    else out.push({ startMs: w.startMs, endMs: w.endMs })
    prev = i
  }
  return out
}

export interface CaptionStyle {
  id: string
  label: string
  hint: string
  patch: Partial<EditText>
}

export const CAPTION_STYLES: CaptionStyle[] = [
  { id: 'plain', label: '简洁描边', hint: '白字黑边，朴素耐看', patch: { size: 5.5, y: 0.88, color: '#FFFFFF', outline: true, boxed: false } },
  { id: 'bar', label: '底条', hint: '半透明黑色底条，复杂背景上也看得清', patch: { size: 5, y: 0.9, color: '#FFFFFF', outline: false, boxed: true, boxColor: '#000000', boxOpacity: 0.6 } },
  { id: 'yellow', label: '大号黄字', hint: '短视频常见的醒目样式', patch: { size: 7.5, y: 0.78, color: '#FFE14D', outline: true, outlineColor: '#000000', boxed: false } },
  { id: 'hl', label: '逐字高亮', hint: '说到哪个词，哪个词变成黄色', patch: { size: 6.5, y: 0.8, color: '#FFFFFF', highlight: '#FFD400', outline: true, boxed: false } },
  { id: 'karaoke', label: '卡拉 OK', hint: '逐字变色，整句淡入淡出', patch: { size: 7, y: 0.76, color: '#FFFFFF', highlight: '#FF4D8D', outline: true, boxed: false, fadeMs: 160 } },
]

/** 素材时间 → 时间线位置（超出片段的收到片段两端） */
export function timelineOfSource(c: EditClip, pl: Placed, srcMs: number): number {
  if (c.freeze) return pl.startMs
  if (c.reverse) return clamp(pl.startMs + (c.outMs - srcMs) / Math.max(0.01, c.speed), pl.startMs, pl.endMs)
  return clamp(pl.startMs + (srcMs - c.inMs) / Math.max(0.01, c.speed), pl.startMs, pl.endMs)
}

/**
 * 按词生成字幕：每行不超过画面宽度的 88%，遇到标点、停顿或太长就换一条。
 * 时间换算到时间线上（考虑片段入点和变速）；带高亮色的样式会记下每个词的起止。
 */
export function captionsFromWords(
  words: TranscriptWord[],
  c: EditClip,
  pl: Placed,
  style: CaptionStyle,
  frame: { w: number; h: number },
  ids: () => number,
): EditText[] {
  const base = blankText()
  const patch = { ...base, ...style.patch }
  const px = (patch.size / 100) * frame.h
  const limit = Math.max(4, (frame.w * 0.88) / Math.max(8, px))
  const lines: TranscriptWord[][] = []
  let cur: TranscriptWord[] = []
  let width = 0
  const flush = () => {
    if (cur.length) lines.push(cur)
    cur = []
    width = 0
  }
  const inRange = words.filter((w) => w.endMs > c.inMs && w.startMs < c.outMs)
  inRange.forEach((w, i) => {
    const vw = visualWidth(w.t.trim()) + (cur.length && !isCjk(w.t.trim()[0] ?? '') ? 0.3 : 0)
    if (cur.length && width + vw > limit) flush()
    cur.push(w)
    width += vw
    const t = w.t.trim()
    const gap = i + 1 < inRange.length ? inRange[i + 1].startMs - w.endMs : 0
    if (SENTENCE_END.test(t) || gap >= 600 || (CLAUSE_END.test(t) && width > limit * 0.6)) flush()
  })
  flush()
  const out: EditText[] = []
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n]
    const start = timelineOfSource(c, pl, line[0].startMs)
    const nextStart = n + 1 < lines.length ? timelineOfSource(c, pl, lines[n + 1][0].startMs) : pl.endMs
    let end = timelineOfSource(c, pl, line[line.length - 1].endMs)
    // 两句之间的空隙很短就让字幕多停一会儿，免得闪烁
    end = Math.max(end, Math.min(nextStart, end + 250))
    if (end - start < MIN_MS) continue
    const pieces = line.map((w, i) => ({ t: i === 0 ? w.t.trimStart() : w.t, a: Math.round(timelineOfSource(c, pl, w.startMs) - start), b: Math.round(timelineOfSource(c, pl, w.endMs) - start) }))
    const t = blankText()
    Object.assign(t, patch)
    t.id = ids()
    t.text = pieces.map((p) => p.t).join('')
    t.startMs = Math.round(start)
    t.endMs = Math.round(end)
    if (patch.highlight) t.words = pieces
    else t.highlight = ''
    out.push(t)
  }
  return out
}

/** 文字块排到哪一行：时间上重叠的错开，免得在时间线上叠在一起。 */
export function textLanes(texts: EditText[]): number[] {
  const order = texts.map((_, i) => i).sort((a, b) => texts[a].startMs - texts[b].startMs)
  const laneEnd: number[] = []
  const lane: number[] = new Array(texts.length).fill(0)
  for (const i of order) {
    let l = laneEnd.findIndex((e) => e <= texts[i].startMs)
    if (l < 0) {
      l = laneEnd.length
      laneEnd.push(0)
    }
    laneEnd[l] = texts[i].endMs
    lane[i] = l
  }
  return lane
}

/** 音频轨在时间线上显示的长度（循环的一直到视频结束）。 */
export function audioSpan(a: EditAudio, src: EditSource | undefined, total: number): number {
  const seg = a.outMs !== null ? a.outMs - a.inMs : src?.durationMs ? Math.max(0, src.durationMs - a.inMs) : null
  const avail = Math.max(0, total - a.startMs)
  if (a.looped) return avail
  return seg === null ? avail : Math.min(seg, Math.max(avail, MIN_MS))
}

/** 输出画面的宽高（界面里监视器的比例用）；和后端 `output_size` 一致。 */
export function outputSize(p: EditProject, sources: Record<string, EditSource | undefined>): { w: number; h: number } {
  const even = (n: number) => Math.max(2, Math.floor(n / 2) * 2)
  if (p.out.width > 0 && p.out.height > 0) return { w: even(p.out.width), h: even(p.out.height) }
  const first = p.clips[0]
  const s = first ? sources[first.path] : undefined
  if (first && s && s.width > 0 && s.height > 0) {
    const swap = first.rotate === 90 || first.rotate === 270
    return { w: even(Math.min(7680, swap ? s.height : s.width)), h: even(Math.min(7680, swap ? s.width : s.height)) }
  }
  return { w: 1280, h: 720 }
}

export function mediaPaths(p: EditProject): string[] {
  const set = new Set<string>()
  for (const c of p.clips) if (c.path) set.add(c.path)
  for (const o of p.overlays) if (o.path) set.add(o.path)
  for (const a of p.audio) if (a.path) set.add(a.path)
  for (const t of p.texts) if (t.font) set.add(t.font)
  return [...set]
}

export function moveItem<T>(list: T[], from: number, to: number): T[] {
  const next = [...list]
  const [x] = next.splice(from, 1)
  next.splice(clamp(to, 0, next.length), 0, x)
  return next
}

/** 毫秒 → `m:ss.mmm`（精确到毫秒，输入框里用）。 */
export function msText(ms: number): string {
  const t = Math.max(0, Math.round(ms))
  const m = Math.floor(t / 60000)
  const s = Math.floor((t % 60000) / 1000)
  const r = t % 1000
  return `${m}:${String(s).padStart(2, '0')}.${String(r).padStart(3, '0')}`
}

/** 解析 `83`、`83.5`、`1:23`、`1:23.456`、`1:02:03`（秒、分:秒、时:分:秒）。无法识别返回 null。 */
export function parseMs(input: string): number | null {
  const s = input.trim()
  if (!s) return null
  const parts = s.split(':')
  if (parts.length > 3 || parts.some((p) => !/^\d+(\.\d+)?$/.test(p.trim()))) return null
  const nums = parts.map(Number)
  if (nums.slice(0, -1).some((n) => !Number.isInteger(n))) return null
  return Math.round(nums.reduce((acc, n) => acc * 60 + n, 0) * 1000)
}

/** 刻度间隔（毫秒）：让相邻刻度大约相隔 80 像素以上。 */
export function tickStep(pxPerSec: number): number {
  const steps = [100, 200, 500, 1000, 2000, 5000, 10000, 15000, 30000, 60000, 120000, 300000, 600000, 1800000, 3600000]
  const want = (80 / pxPerSec) * 1000
  return steps.find((s) => s >= want) ?? steps[steps.length - 1]
}

/** 把 v 吸附到最近的候选值（距离在 range 以内）。 */
export function snap(v: number, candidates: number[], range: number): number {
  let best = v
  let bd = range + 1
  for (const c of candidates) {
    const d = Math.abs(c - v)
    if (d <= range && d < bd) {
      best = c
      bd = d
    }
  }
  return best
}


// ---------- 叠加轨 ----------
// 叠加素材（画中画、贴纸、GIF、水印）自己有起点和轨道，不跟主轨排队。规则和后端 `spec.rs` 的 `Overlay` / `overlay_order` 保持一致：
// 轨号小的在下、大的在上；同一轨道上开始早的在下；叠加素材不会撑长成片，超出主轨结尾的部分被截掉。

export const isGif = (path: string) => /\.gif$/i.test(path)

/** 叠加素材在时间线上占的时长（毫秒）：图片就是显示时长，视频按速度折算。 */
export function overlayDuration(o: EditOverlay): number {
  const span = Math.max(0, o.outMs - o.inMs)
  return o.kind === 'image' ? span : Math.round(span / Math.max(0.01, o.speed))
}

export const overlayEnd = (o: EditOverlay) => o.startMs + overlayDuration(o)

/** 旋转角度换算到 0–360 度。 */
export const overlayAngle = (o: EditOverlay) => ((o.rotate % 360) + 360) % 360

/** 恰好是 90 / 180 / 270 度（后端用转置做，不重采样）。 */
export function quarterTurn(o: { rotate: number }): 90 | 180 | 270 | null {
  const a = ((o.rotate % 360) + 360) % 360
  for (const q of [90, 180, 270] as const) if (Math.abs(a - q) < 1e-6) return q
  return null
}

/** 从下到上的叠放顺序（下标）：轨号小的在下，同一轨道上开始早的在下；和后端 `overlay_order` 一致。 */
export function overlayOrder(list: EditOverlay[]): number[] {
  return list.map((_, i) => i).sort((a, b) => list[a].track - list[b].track || list[a].startMs - list[b].startMs || a - b)
}

/** 新叠加素材默认的宽度（占画面宽度的比例）：不超过 0.4，竖长的素材再收一点，高度不超过画面的一半。 */
export function defaultOverlayScale(src: { w: number; h: number }, frame: { w: number; h: number }): number {
  const sw = src.w > 0 ? src.w : 16
  const sh = src.h > 0 ? src.h : 9
  const byHeight = 0.5 * (frame.h / Math.max(1, frame.w)) * (sw / sh)
  return Math.round(clamp(Math.min(0.4, byHeight), 0.05, 1) * 1000) / 1000
}

/**
 * 素材放上叠加轨时的默认设置：视频取整段，图片显示 5 秒；GIF 动图默认循环（凑够约 3 秒）并静音。
 * `index` 是此刻已经同时出现的叠加素材数，用来让新的错开一点，不正好盖在上一个上面。
 */
export function overlayFromSource(src: EditSource, id: number, startMs: number, track: number, frame: { w: number; h: number }, index = 0): EditOverlay {
  const o = blankOverlay()
  o.id = id
  o.path = src.path
  o.track = clamp(Math.round(track), 1, MAX_OVERLAY_TRACKS)
  o.startMs = clamp(Math.round(startMs), 0, MAX_START_MS)
  if (src.kind === 'image') {
    o.kind = 'image'
    o.outMs = DEFAULT_STILL_MS
  } else {
    const d = Math.max(MIN_MS, src.durationMs ?? DEFAULT_STILL_MS)
    o.outMs = d
    if (isGif(src.path)) {
      o.looped = true
      o.mute = true
      o.outMs = d * Math.max(1, Math.ceil(3000 / d))
    }
  }
  o.scale = defaultOverlayScale({ w: src.width, h: src.height }, frame)
  const nudge = (index % 5) * 0.07
  o.x = clamp(0.5 + nudge, 0, 1)
  o.y = clamp(0.5 + nudge, 0, 1)
  return o
}

/** 这条轨道上 [s, e) 这段时间里有没有别的叠加素材。 */
export function trackBusy(list: EditOverlay[], track: number, s: number, e: number, skipId?: number): boolean {
  return list.some((o) => o.id !== skipId && o.track === track && o.startMs < e && overlayEnd(o) > s)
}

/** 想放在 wanted 轨：被占了就找最近的空轨（先往上，再往下）；都满了仍然用 wanted（重叠的会按开始先后叠放）。 */
export function pickTrack(list: EditOverlay[], wanted: number, s: number, e: number, skipId?: number): number {
  const w = clamp(Math.round(wanted), 1, MAX_OVERLAY_TRACKS)
  if (!trackBusy(list, w, s, e, skipId)) return w
  for (let t = w + 1; t <= MAX_OVERLAY_TRACKS; t++) if (!trackBusy(list, t, s, e, skipId)) return t
  for (let t = w - 1; t >= 1; t--) if (!trackBusy(list, t, s, e, skipId)) return t
  return w
}

/** 同一条轨道上时间重叠的素材在时间线上错开显示：返回每个叠加素材在自己轨道里的行号（0 起）。 */
export function overlayLanes(list: EditOverlay[]): number[] {
  const lane: number[] = new Array(list.length).fill(0)
  const byTrack = new Map<number, number[]>()
  list.forEach((o, i) => byTrack.set(o.track, [...(byTrack.get(o.track) ?? []), i]))
  for (const idx of byTrack.values()) {
    const order = [...idx].sort((a, b) => list[a].startMs - list[b].startMs || a - b)
    const ends: number[] = []
    for (const i of order) {
      let l = ends.findIndex((e) => e <= list[i].startMs)
      if (l < 0) {
        l = ends.length
        ends.push(0)
      }
      ends[l] = overlayEnd(list[i])
      lane[i] = l
    }
  }
  return lane
}

/** 叠加素材的最大可用长度（素材本身的长度）：循环的、图片不受限制。 */
export function overlaySourceLimit(o: EditOverlay, src: EditSource | undefined): number {
  if (o.kind === 'image' || o.looped) return Infinity
  return src?.durationMs ?? Infinity
}

/** 在时间线 t 处把叠加素材一分为二。离两端不足 MIN_MS 时返回 null。第二段接着播：循环的素材会算好循环到哪里了。淡入归第一段、淡出归第二段。 */
export function splitOverlay(o: EditOverlay, t: number, newId: number, srcDurMs?: number | null): [EditOverlay, EditOverlay] | null {
  const dur = overlayDuration(o)
  const local = t - o.startMs
  if (local < MIN_MS || dur - local < MIN_MS) return null
  const first: EditOverlay = { ...o }
  const second: EditOverlay = { ...o, id: newId, startMs: Math.round(t) }
  if (o.kind === 'image') {
    first.outMs = Math.round(local)
    second.outMs = Math.round(dur - local)
  } else {
    const played = local * o.speed
    const remain = o.outMs - o.inMs - played
    const loop = o.looped && srcDurMs ? srcDurMs - o.inMs : 0
    const phase = loop > 0 ? played % loop : played
    first.outMs = Math.round(o.inMs + played)
    second.inMs = Math.round(o.inMs + phase)
    second.outMs = Math.round(second.inMs + remain)
  }
  first.fadeOutMs = 0
  first.animOut = 'none'
  second.fadeInMs = 0
  second.animIn = 'none'
  // 关键帧在切开处补一个等值的点，两半的曲线才接得上；后半的时间从它自己的开头算
  const [ka, kb] = kfSplit(o.keyframes, local)
  first.keyframes = ka
  second.keyframes = kb
  return [first, second]
}

/**
 * 叠加素材在舞台里的摆法：以 (cx, cy) 为中心放一个“素材原方向”的盒子（ew × eh），再 `rotate · scale(翻转)`。
 * 宽度按成片画面算：旋转 90 / 270 度后看上去的宽度 = 画面宽 × 缩放（和后端 `overlay_size` 一致）。
 */
export function overlayBox(o: { x: number; y: number; scale: number; rotate: number; flipH: boolean; flipV: boolean }, src: { w: number; h: number } | undefined, stage: { w: number; h: number }) {
  const sw = src && src.w > 0 ? src.w : 16
  const sh = src && src.h > 0 ? src.h : 9
  const q = quarterTurn(o)
  const turned = q === 90 || q === 270
  const k = (o.scale * stage.w) / (turned ? sh : sw)
  return {
    cx: o.x * stage.w,
    cy: o.y * stage.h,
    ew: sw * k,
    eh: sh * k,
    transform: `rotate(${o.rotate}deg) scale(${o.flipH ? -1 : 1}, ${o.flipV ? -1 : 1})`,
  }
}

/** 把叠加素材缩放到恰好放进画面（contain）或铺满画面（cover）时的 scale。 */
export function overlayFitScale(o: { rotate: number }, src: { w: number; h: number } | undefined, frame: { w: number; h: number }, mode: 'contain' | 'cover'): number {
  const sw = src && src.w > 0 ? src.w : 16
  const sh = src && src.h > 0 ? src.h : 9
  const q = quarterTurn(o)
  const [vw, vh] = q === 90 || q === 270 ? [sh, sw] : [sw, sh]
  const byHeight = (frame.h / Math.max(1, frame.w)) * (vw / vh)
  return Math.round(clamp(mode === 'contain' ? Math.min(1, byHeight) : Math.max(1, byHeight), 0.02, 3) * 1000) / 1000
}

/** 主轨片段 → 叠加素材（放在原来的时间上，铺满画面）。区域、转场不能带过去。 */
export function clipToOverlay(c: EditClip, pl: Placed, id: number, track: number, src: EditSource | undefined, frame: { w: number; h: number }): EditOverlay {
  const o = blankOverlay()
  o.id = id
  o.path = c.path
  o.kind = c.kind
  o.track = clamp(Math.round(track), 1, MAX_OVERLAY_TRACKS)
  o.startMs = clamp(Math.round(pl.startMs), 0, MAX_START_MS)
  o.inMs = c.inMs
  o.outMs = c.outMs
  o.speed = c.speed
  o.volume = c.volume
  o.mute = c.mute
  o.fadeInMs = c.fadeInMs
  o.fadeOutMs = c.fadeOutMs
  o.rotate = c.rotate
  o.flipH = c.flipH
  o.flipV = c.flipV
  o.brightness = c.brightness
  o.contrast = c.contrast
  o.saturation = c.saturation
  o.scale = overlayFitScale(o, src && { w: src.width, h: src.height }, frame, 'contain')
  return o
}

/** 叠加素材 → 主轨片段（接到主轨末尾时用）。位置、大小、透明度、循环用不上；循环的素材取到素材结尾。 */
export function overlayToClip(o: EditOverlay, id: number, srcDurMs?: number | null): EditClip {
  const c = blankClip()
  c.id = id
  c.path = o.path
  c.kind = o.kind
  c.inMs = o.inMs
  c.outMs = o.kind === 'video' && srcDurMs ? Math.min(o.outMs, srcDurMs) : o.outMs
  c.speed = o.speed
  c.volume = o.volume
  c.mute = o.mute
  c.fadeInMs = o.fadeInMs
  c.fadeOutMs = o.fadeOutMs
  const q = quarterTurn(o)
  c.rotate = q ?? 0
  c.flipH = o.flipH
  c.flipV = o.flipV
  c.brightness = o.brightness
  c.contrast = o.contrast
  c.saturation = o.saturation
  return c
}

// ---------- 关键帧 ----------
// 一个属性随时间变化的几个“关键点”，中间按缓动曲线补值。时间相对叠加素材 / 文字自己的开始（毫秒，按成片时间算）。
// 求值要和后端 `edit/keyframes.rs` 一致。

export const KEY_EASES: { id: EditKeyframe['ease']; label: string }[] = [
  { id: 'linear', label: '匀速' },
  { id: 'in', label: '先慢后快' },
  { id: 'out', label: '先快后慢' },
  { id: 'inout', label: '两头慢' },
  { id: 'hold', label: '保持（到下一点再跳）' },
]
export const MAX_KEYFRAMES = 64

export interface KeyProp {
  id: string
  label: string
  min: number
  max: number
  step: number
  digits: number
  unit?: string
  /** 倍数（界面上显示 value × mult，比如位置显示成百分比） */
  mult?: number
}
export const OVERLAY_KEY_PROPS: KeyProp[] = [
  { id: 'x', label: '水平位置', min: -1, max: 2, step: 0.01, digits: 0, unit: '%', mult: 100 },
  { id: 'y', label: '垂直位置', min: -1, max: 2, step: 0.01, digits: 0, unit: '%', mult: 100 },
  { id: 'scale', label: '大小', min: 0.02, max: 3, step: 0.01, digits: 0, unit: '%', mult: 100 },
  { id: 'rotate', label: '旋转', min: -3600, max: 3600, step: 1, digits: 0, unit: '°' },
  { id: 'opacity', label: '不透明度', min: 0, max: 1, step: 0.01, digits: 0, unit: '%', mult: 100 },
  { id: 'volume', label: '音量', min: 0, max: 4, step: 0.05, digits: 2, unit: '×' },
]
export const TEXT_KEY_PROPS: KeyProp[] = OVERLAY_KEY_PROPS.filter((p) => ['x', 'y', 'opacity'].includes(p.id))

const EASES = new Set(['linear', 'in', 'out', 'inout', 'hold'])

/** 整理一个属性的关键点：去掉不是数的、按时间排序、同一时刻只留最后一个。 */
export function kfTidy(points: EditKeyframe[] | undefined): EditKeyframe[] {
  const v = (points ?? []).filter((k) => Number.isFinite(k.value) && Number.isFinite(k.tMs)).map((k) => ({ tMs: Math.max(0, Math.round(k.tMs)), value: k.value, ease: EASES.has(k.ease) ? k.ease : ('linear' as const) }))
  v.sort((a, b) => a.tMs - b.tMs)
  const out: EditKeyframe[] = []
  for (const k of v) {
    if (out.length && out[out.length - 1].tMs === k.tMs) out[out.length - 1] = k
    else out.push(k)
  }
  return out.slice(0, MAX_KEYFRAMES)
}

/** 整理整个关键帧表（读旧项目 / 外来数据时用）：空的属性去掉。 */
export function kfClean(map: EditKeyframes | undefined | null): EditKeyframes {
  const out: EditKeyframes = {}
  if (map && typeof map === 'object') {
    for (const [prop, pts] of Object.entries(map)) {
      const t = kfTidy(Array.isArray(pts) ? pts : [])
      if (t.length) out[prop] = t
    }
  }
  return out
}

function easeAt(kind: string, p: number): number {
  if (kind === 'in') return p * p
  if (kind === 'out') return 1 - (1 - p) * (1 - p)
  if (kind === 'inout') return p * p * (3 - 2 * p)
  if (kind === 'hold') return 0
  return p
}

/** t（毫秒）时刻的值；第一个点之前取第一个点、最后一个点之后取最后一个点。没有关键点返回 null。`points` 要已整理过。 */
export function kfEval(points: EditKeyframe[] | undefined, t: number): number | null {
  if (!points?.length) return null
  if (t <= points[0].tMs) return points[0].value
  for (let i = 0; i + 1 < points.length; i++) {
    const a = points[i]
    const b = points[i + 1]
    if (t < b.tMs) return a.value + (b.value - a.value) * easeAt(a.ease, clamp((t - a.tMs) / (b.tMs - a.tMs), 0, 1))
  }
  return points[points.length - 1].value
}

/** 在 t 处放（或改）一个关键点，返回新的表；原来的不改。`value` 在范围外会收紧。 */
export function kfSet(map: EditKeyframes, prop: string, t: number, value: number, ease?: EditKeyframe['ease']): EditKeyframes {
  const cur = map[prop] ?? []
  const at = Math.max(0, Math.round(t))
  const old = cur.find((k) => k.tMs === at)
  const pts = kfTidy([...cur.filter((k) => k.tMs !== at), { tMs: at, value, ease: ease ?? old?.ease ?? 'linear' }])
  return { ...map, [prop]: pts }
}

/** 删掉 t 处的关键点（删光了这个属性就回到固定值）。 */
export function kfDelete(map: EditKeyframes, prop: string, t: number): EditKeyframes {
  const pts = (map[prop] ?? []).filter((k) => k.tMs !== Math.round(t))
  const out = { ...map }
  if (pts.length) out[prop] = pts
  else delete out[prop]
  return out
}

/** 把一个属性的关键点在 `at` 处切成两半（切开处补一个等值点，后半的时间从 0 算）。 */
export function kfSplit(map: EditKeyframes, at: number): [EditKeyframes, EditKeyframes] {
  const a: EditKeyframes = {}
  const b: EditKeyframes = {}
  const cut = Math.round(at)
  for (const [prop, raw] of Object.entries(map ?? {})) {
    const pts = kfTidy(raw)
    const v = kfEval(pts, cut)
    if (v === null) continue
    const prev = [...pts].reverse().find((k) => k.tMs < cut)
    const ease = prev?.ease ?? 'linear'
    a[prop] = [...pts.filter((k) => k.tMs < cut), { tMs: cut, value: v, ease }]
    b[prop] = [{ tMs: 0, value: v, ease }, ...pts.filter((k) => k.tMs > cut).map((k) => ({ ...k, tMs: k.tMs - cut }))]
  }
  return [a, b]
}

/** 带关键帧的素材 / 文字，在 `local`（距它开始多久，毫秒）这一刻各属性的实际值；没有关键帧的属性原样。 */
export function withKeys<T extends { keyframes: EditKeyframes }>(item: T, local: number): T {
  const keys = Object.entries(item.keyframes ?? {})
  if (!keys.length) return item
  const out: Record<string, unknown> = { ...item }
  for (const [prop, pts] of keys) {
    const v = kfEval(pts, local)
    if (v !== null) out[prop] = v
  }
  return out as T
}

/** 有关键帧的属性名集合 */
export const keyedProps = (item: { keyframes?: EditKeyframes }): Set<string> => new Set(Object.entries(item.keyframes ?? {}).filter(([, v]) => v.length).map(([k]) => k))

// ---------- 实时画面 ----------
// 监视器不再先渲染成片再播放，而是直接把每个素材叠在一起显示：主轨的片段（含转场）、叠加轨、文字，声音也实时混合。
// 下面是“某一时刻看到什么”的纯计算（`liveFrame`）和转场在 CSS 里的样子（`transitionLook`）；
// 转场的几何形状是对着 ffmpeg 的 xfade 实际输出量出来的，导出时仍然由 ffmpeg 渲染，这里只是近似。

/** 主轨上此刻看得见的一层 */
export interface LiveMain {
  /** 在 `clips` 里的下标 */
  idx: number
  clip: EditClip
  /** 此刻对应素材里的时间（毫秒） */
  srcMs: number
  /** 淡入淡出的亮度系数 0–1 */
  fade: number
  /** solo：单独显示；out：转场里离开的那个；in：转场里进来的那个 */
  role: 'solo' | 'out' | 'in'
  /** 转场进度 0–1（solo 时为 0） */
  p: number
  /** 转场名称（role 为 out / in 时有意义） */
  transition: string
}

/** 叠加轨上此刻看得见的一个素材 */
export interface LiveOverlay {
  overlay: EditOverlay
  srcMs: number
  /** 淡入淡出的不透明度系数 0–1 */
  fade: number
  /** 从下到上的叠放序号（0 最下） */
  rank: number
  /** 距它开始多久（毫秒） */
  local: number
}

export interface LiveFrame {
  main: LiveMain[]
  overlays: LiveOverlay[]
}

/** 叠加素材在 `local`（距它开始多久，毫秒）这一刻对应素材里的时间。循环的素材回绕到入点。 */
export function overlaySourceMs(o: EditOverlay, local: number, srcDurMs?: number | null): number {
  if (o.kind === 'image') return 0
  const played = Math.max(0, local) * o.speed
  const loop = o.looped && srcDurMs ? srcDurMs - o.inMs : 0
  if (loop > 0) return o.inMs + (played % loop)
  return Math.min(o.inMs + played, o.outMs)
}

/** 时间线 t 处看到的东西。`srcDur` 用来查素材的长度（循环叠加素材回绕时用）。 */
export function liveFrame(p: EditProject, pl: Placed[], t: number, srcDur: (path: string) => number | null | undefined = () => null): LiveFrame {
  const main: LiveMain[] = []
  const clips = p.clips
  let i = clipIndexAt(pl, t)
  let tt = t
  if (i < 0 && clips.length) {
    i = clips.length - 1
    tt = pl[i].endMs - 1
  }
  if (i >= 0) {
    const mk = (idx: number, role: LiveMain['role'], prog: number): LiveMain => {
      const c = clips[idx]
      const q = pl[idx]
      return { idx, clip: c, srcMs: sourceTimeAt(c, q, tt), fade: fadeFactor(c, q, clamp(tt, q.startMs, q.endMs)), role, p: prog, transition: clips[Math.max(idx, i)].transition?.kind ?? 'fade' }
    }
    const ov = pl[i].overlapMs
    if (i > 0 && ov > 0 && tt < pl[i].startMs + ov) {
      const prog = clamp((tt - pl[i].startMs) / ov, 0, 1)
      main.push(mk(i - 1, 'out', prog), mk(i, 'in', prog))
    } else main.push(mk(i, 'solo', 0))
  }
  const overlays: LiveOverlay[] = []
  const order = overlayOrder(p.overlays)
  order.forEach((idx, rank) => {
    const o = p.overlays[idx]
    const dur = overlayDuration(o)
    const local = t - o.startMs
    if (local < 0 || local >= dur) return
    let f = 1
    if (o.fadeInMs > 0) f = Math.min(f, local / o.fadeInMs)
    if (o.fadeOutMs > 0) f = Math.min(f, (dur - local) / o.fadeOutMs)
    overlays.push({ overlay: withKeys(o, local), srcMs: overlaySourceMs(o, local, srcDur(o.path)), fade: clamp(f, 0, 1), rank, local })
  })
  return { main, overlays }
}

/** 主轨一层此刻的音量系数（0–1，已含片段音量、淡入淡出和转场里的声音交叉淡化；浏览器里音量最大 1）。 */
export function mainGain(m: LiveMain): number {
  if (m.clip.mute) return 0
  const x = m.role === 'out' ? 1 - m.p : m.role === 'in' ? m.p : 1
  return clamp(m.clip.volume * m.fade * x, 0, 1)
}

/** 入场 / 出场动画此刻“还没走完的比例”：1 = 在画面外（或缩到最小），0 = 在正常位置。入场先快后慢，出场先慢后快，和后端 `anim_left` 一致。 */
export function overlayAnimLeft(o: EditOverlay, local: number, dur: number): { in: number; out: number } {
  const d = Math.max(1, Math.min(o.animMs, dur / 2))
  const pin = clamp(local / d, 0, 1)
  const pout = clamp((local - (dur - d)) / d, 0, 1)
  return { in: o.animIn === 'none' ? 0 : (1 - pin) ** 2, out: o.animOut === 'none' ? 0 : pout ** 2 }
}

/** 动画带来的位移（舞台像素）和缩放系数。`box` 是 `overlayBox` 的结果。 */
export function overlayAnimPose(l: LiveOverlay, box: { cx: number; cy: number; ew: number; eh: number }, stage: { w: number; h: number }): { dx: number; dy: number; zoom: number } {
  const o = l.overlay
  const r = overlayAnimLeft(o, l.local, overlayDuration(o))
  let dx = 0
  let dy = 0
  let zoom = 1
  for (const [name, left] of [[o.animIn, r.in], [o.animOut, r.out]] as const) {
    if (name === 'slide-left') dx -= left * (box.cx + box.ew / 2)
    else if (name === 'slide-right') dx += left * (stage.w - box.cx + box.ew / 2)
    else if (name === 'slide-up') dy -= left * (box.cy + box.eh / 2)
    else if (name === 'slide-down') dy += left * (stage.h - box.cy + box.eh / 2)
    else if (name === 'zoom') zoom -= left
  }
  return { dx, dy, zoom: Math.max(0.02, zoom) }
}

/** 叠加素材此刻的音量系数。 */
export function overlayGain(l: LiveOverlay): number {
  return l.overlay.mute ? 0 : clamp(l.overlay.volume * l.fade, 0, 1)
}

/** 转场里一层的样式（CSS 属性，驼峰写法）。 */
export type LayerFx = Record<string, string>
export interface TransitionLook {
  /** 离开的那一层 */
  a: LayerFx
  /** 进来的那一层（盖在上面） */
  b: LayerFx
  /** 垫在两层下面的颜色（闪黑 / 闪白用），null 为默认的黑色舞台 */
  backdrop: string | null
}

const smoothstep = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a), 0, 1)
  return t * t * (3 - 2 * t)
}
const percent = (v: number) => `${(v * 100).toFixed(2)}%`
const masked = (image: string): LayerFx => ({ maskImage: image, WebkitMaskImage: image })

/**
 * 转场进行到 `p`（0–1）时两层的样子。形状对着 xfade 量过：
 * 擦除是进来的画面从一侧逐渐露出来（边缘直线移动，画面本身不动）；滑动是两个画面一起平移；
 * 圆形的边缘是柔和的，半径随进度大约按 3p 增长；时钟擦除从 12 点钟方向顺时针转，大约在 p=0.13 开始、0.93 结束；
 * 闪黑 / 闪白是前 20% 渐变成黑 / 白，之后再从黑 / 白里慢慢出现；
 * 溶解（逐点随机替换）和像素化（马赛克块）用淡入淡出近似。
 */
export function transitionLook(kind: string, p: number): TransitionLook {
  const q = clamp(p, 0, 1)
  const plain = (b: LayerFx = {}, a: LayerFx = {}, backdrop: string | null = null): TransitionLook => ({ a, b, backdrop })
  switch (kind) {
    case 'wipeleft':
      return plain({ clipPath: `inset(0 0 0 ${percent(1 - q)})` })
    case 'wiperight':
      return plain({ clipPath: `inset(0 ${percent(1 - q)} 0 0)` })
    case 'wipeup':
      return plain({ clipPath: `inset(${percent(1 - q)} 0 0 0)` })
    case 'wipedown':
      return plain({ clipPath: `inset(0 0 ${percent(1 - q)} 0)` })
    case 'slideleft':
      return plain({ transform: `translateX(${percent(1 - q)})` }, { transform: `translateX(${percent(-q)})` })
    case 'slideright':
      return plain({ transform: `translateX(${percent(q - 1)})` }, { transform: `translateX(${percent(q)})` })
    case 'slideup':
      return plain({ transform: `translateY(${percent(1 - q)})` }, { transform: `translateY(${percent(-q)})` })
    case 'slidedown':
      return plain({ transform: `translateY(${percent(q - 1)})` }, { transform: `translateY(${percent(q)})` })
    case 'circleopen': {
      const s = (v: number) => `${(v * 100).toFixed(1)}%`
      return plain(masked(`radial-gradient(circle farthest-corner at 50% 50%, #000 ${s(3 * q - 1.5)}, rgba(0,0,0,0.5) ${s(3 * q - 1)}, transparent ${s(3 * q - 0.5)})`))
    }
    case 'circleclose': {
      const s = (v: number) => `${(v * 100).toFixed(1)}%`
      return plain(masked(`radial-gradient(circle farthest-corner at 50% 50%, transparent ${s(1.5 - 3 * q)}, rgba(0,0,0,0.5) ${s(2 - 3 * q)}, #000 ${s(2.5 - 3 * q)})`))
    }
    case 'radial': {
      const deg = clamp(450 * q - 60, 0, 360).toFixed(1)
      return plain(masked(`conic-gradient(from 0deg at 50% 50%, #000 0deg ${deg}deg, transparent ${deg}deg)`))
    }
    case 'fadeblack':
    case 'fadewhite':
      return plain({ opacity: String(smoothstep(0.2, 1, q)) }, { opacity: String(1 - smoothstep(0, 0.2, q)) }, kind === 'fadewhite' ? '#fff' : null)
    case 'pixelize': {
      const blur = (10 * Math.sin(Math.PI * q)).toFixed(1)
      return plain({ opacity: String(smoothstep(0.35, 0.65, q)), filter: `blur(${blur}px)` }, { filter: `blur(${blur}px)` })
    }
    default:
      // fade、dissolve，以及没见过的名字：线性淡入淡出
      return plain({ opacity: String(q) })
  }
}

// ---------- 跟着物体走的区域 ----------
// 轨迹点的位置是相对整个画面的比例（左上角为原点），方向是素材的显示方向（旋转翻转之前），时间是素材里的时间。
// 插值、放大的算法和后端 `src-tauri/src/track/mod.rs` 的 `interpolate`、`edit/region.rs` 的 `box_at` 保持一致。

/** 拖动框时，这个时刻前后多长时间内自动追踪出来的点会被让开，路径从手动的点平滑过渡过去 */
export const PIN_EASE_MS = 300
/** 框的最小尺寸（占画面的比例） */
export const MIN_BOX = 0.01
/** 一次追踪最长（毫秒），和后端一致 */
export const MAX_TRACK_SPAN_MS = 30 * 60 * 1000

export const REGION_EFFECTS: { id: RegionEffect; label: string; hint: string }[] = [
  { id: 'mosaic', label: '马赛克', hint: '把区域打上马赛克（遮住脸、车牌、水印）' },
  { id: 'blur', label: '模糊', hint: '把区域虚化' },
  { id: 'tone', label: '局部调色', hint: '只调区域里的亮度、对比度、饱和度' },
  { id: 'focus', label: '跟随聚焦', hint: '放大并让画面始终跟着这个区域走（也可以把横屏素材裁成竖屏）' },
]

export function blankRegion(): EditRegion {
  return {
    id: 0,
    name: '',
    track: [],
    shape: 'rect',
    feather: 0.2,
    grow: 0,
    invert: false,
    effect: 'mosaic',
    strength: 0.5,
    brightness: 0,
    contrast: 1,
    saturation: 1,
    zoom: 2,
    reframe: false,
    smooth: 0.6,
    startMs: null,
    endMs: null,
  }
}

export function copyRegion(r: EditRegion): EditRegion {
  return { ...r, track: r.track.map((p) => ({ ...p })) }
}

export function nextRegionId(c: EditClip): number {
  return c.regions.reduce((m, r) => Math.max(m, r.id), 0) + 1
}

/** 轨迹在 t（素材时间，毫秒）处的矩形：相邻两点之间直线插值，两头停在第一个 / 最后一个点。没有点返回 null。 */
export function trackAt(pts: TrackPt[], t: number): NRect | null {
  const first = pts[0]
  const last = pts[pts.length - 1]
  if (!first || !last) return null
  const rect = (p: TrackPt): NRect => ({ x: p.x, y: p.y, w: p.w, h: p.h })
  if (t <= first.tMs) return rect(first)
  if (t >= last.tMs) return rect(last)
  // 二分找到 t 所在的区间：第一个时间 > t 的点
  let lo = 0
  let hi = pts.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (pts[mid].tMs <= t) lo = mid + 1
    else hi = mid
  }
  const a = pts[lo - 1]
  const b = pts[lo]
  const span = b.tMs - a.tMs
  const f = span > 0 ? (t - a.tMs) / span : 0
  const l = (u: number, v: number) => u + (v - u) * f
  return { x: l(a.x, b.x), y: l(a.y, b.y), w: l(a.w, b.w), h: l(a.h, b.h) }
}

/** 效果真正作用的范围：追踪的框按“扩大”系数放大（中心不变）。 */
export function grownBox(r: EditRegion, t: number): NRect | null {
  const b = trackAt(r.track, t)
  if (!b) return null
  const k = 1 + r.grow
  const w = b.w * k
  const h = b.h * k
  return { x: b.x + (b.w - w) / 2, y: b.y + (b.h - h) / 2, w, h }
}

/** 效果在这个素材时刻生效吗（聚焦不受限制）。 */
export function regionActiveAt(r: EditRegion, t: number): boolean {
  return (r.startMs === null || t >= r.startMs) && (r.endMs === null || t < r.endMs)
}

/** 把框收进画面：尺寸不小于 MIN_BOX、不大于整个画面，位置不超出画面。 */
export function clampBox(b: NRect): NRect {
  const w = clamp(b.w, MIN_BOX, 1)
  const h = clamp(b.h, MIN_BOX, 1)
  return { x: clamp(b.x, 0, 1 - w), y: clamp(b.y, 0, 1 - h), w, h }
}

/** 由对角两点（拖出来的框）得到矩形。 */
export function boxFromCorners(ax: number, ay: number, bx: number, by: number): NRect {
  return { x: Math.min(ax, bx), y: Math.min(ay, by), w: Math.abs(bx - ax), h: Math.abs(by - ay) }
}

/** 新区域：参照时刻 `refMs` 只有一个手动指定的点。 */
export function newRegion(id: number, rect: NRect, refMs: number, effect: RegionEffect = 'mosaic'): EditRegion {
  const r = blankRegion()
  const b = clampBox(rect)
  r.id = id
  r.effect = effect
  r.track = [{ tMs: Math.round(refMs), ...b, pin: true }]
  if (effect === 'tone') r.brightness = 0.2
  return r
}

/** 手动把区域放到 `rect`：在 `tMs` 放一个手动点，前后 PIN_EASE_MS 内自动追踪出来的点让开。 */
export function setPin(track: TrackPt[], tMs: number, rect: NRect): TrackPt[] {
  const t = Math.round(tMs)
  const keep = track.filter((p) => p.pin || Math.abs(p.tMs - t) > PIN_EASE_MS)
  const next = keep.filter((p) => p.tMs !== t)
  next.push({ tMs: t, ...rect, pin: true })
  return next.sort((a, b) => a.tMs - b.tMs)
}

/** 改区域大小：整段一起变（每个点保持中心不变）。 */
export function resizeAll(track: TrackPt[], w: number, h: number): TrackPt[] {
  const nw = clamp(w, MIN_BOX, 1)
  const nh = clamp(h, MIN_BOX, 1)
  return track.map((p) => {
    const cx = p.x + p.w / 2
    const cy = p.y + p.h / 2
    return { ...p, x: clamp(cx - nw / 2, 0, 1 - nw), y: clamp(cy - nh / 2, 0, 1 - nh), w: nw, h: nh }
  })
}

/** 追踪出来的结果并进现有轨迹：范围外的老点保留；范围内用新点，但手动点（及其前后让开的范围）保持原样。 */
export function mergeTracked(old: TrackPt[], fresh: TrackPt[], fromMs: number, toMs: number): TrackPt[] {
  const outside = old.filter((p) => p.tMs < fromMs || p.tMs > toMs)
  const pins = old.filter((p) => p.pin && p.tMs >= fromMs && p.tMs <= toMs)
  const freshPins = fresh.filter((p) => p.pin)
  // 这次追踪的参照点（也是手动点）盖过同一时刻的老点
  const keepPins = pins.filter((p) => !freshPins.some((f) => f.tMs === p.tMs))
  const tracked = fresh.filter((p) => p.pin || !keepPins.some((q) => Math.abs(q.tMs - p.tMs) <= PIN_EASE_MS))
  const all = [...outside, ...keepPins, ...tracked].sort((a, b) => a.tMs - b.tMs)
  // 同一时刻只留一个（手动点优先）
  const out: TrackPt[] = []
  for (const p of all) {
    const last = out[out.length - 1]
    if (last && last.tMs === p.tMs) {
      if (p.pin && !last.pin) out[out.length - 1] = p
    } else out.push(p)
  }
  return out
}

/** 只留下 [fromMs, toMs] 这一段用得到的点：范围里的，加上两边各最近的一个（让两端的插值不变）。 */
export function trimTrack(track: TrackPt[], fromMs: number, toMs: number): TrackPt[] {
  if (track.length <= 2) return track.map((p) => ({ ...p }))
  const firstIn = track.findIndex((p) => p.tMs >= fromMs)
  let lastIn = -1
  for (let i = track.length - 1; i >= 0; i--) {
    if (track[i].tMs <= toMs) {
      lastIn = i
      break
    }
  }
  // 整条轨迹都在范围一边：留下离范围最近的那个点
  if (firstIn < 0) return [{ ...track[track.length - 1] }]
  if (lastIn < 0) return [{ ...track[0] }]
  const from = Math.max(0, firstIn - 1)
  const to = Math.min(track.length - 1, lastIn + 1)
  return track.slice(from, Math.max(from, to) + 1).map((p) => ({ ...p }))
}

/** 追踪不到（丢失）的时间段（素材毫秒）：连续丢失的点合成一段。 */
export function lostSpans(track: TrackPt[]): [number, number][] {
  const out: [number, number][] = []
  let start: number | null = null
  for (let i = 0; i < track.length; i++) {
    const p = track[i]
    if (p.lost && !p.pin) {
      if (start === null) start = i > 0 ? track[i - 1].tMs : p.tMs
    } else if (start !== null) {
      out.push([start, p.tMs])
      start = null
    }
  }
  if (start !== null) out.push([start, track[track.length - 1].tMs])
  return out
}

/** 轨迹覆盖了素材的哪一段（没有点返回 null）。 */
export function trackSpan(track: TrackPt[]): [number, number] | null {
  return track.length ? [track[0].tMs, track[track.length - 1].tMs] : null
}

/** 只有一个点（还没追踪过，或者手动放的一个固定位置）。 */
export const isStatic = (track: TrackPt[]) => track.length <= 1

/** 画路径用：点太多时均匀抽稀（保留首尾）。 */
export function thin<T>(list: T[], max: number): T[] {
  if (list.length <= max) return list
  const out: T[] = []
  const step = (list.length - 1) / (max - 1)
  for (let i = 0; i < max; i++) out.push(list[Math.round(i * step)])
  return out
}

/** 区域给人看的名字：没起名就用序号（效果已经在旁边的标签里写了）。 */
export function regionTitle(r: EditRegion, index: number): string {
  return r.name.trim() || `#${index + 1}`
}

// ---------- 监视器里框和素材画面的对应 ----------

export interface MonitorGeom {
  /** 视频元素（旋转翻转之前）在舞台里的位置和大小 */
  box: { left: number; top: number; w: number; h: number }
  /** 画面内容在视频元素里的位置和大小（按“完整显示 / 铺满”缩放之后） */
  content: { left: number; top: number; w: number; h: number }
  /** 视频元素的 transform（旋转、翻转） */
  transform: string
  /** 舞台上的像素位置（相对舞台左上角）→ 素材画面里的比例（可以超出 0–1） */
  toNorm(px: number, py: number): { x: number; y: number }
  /** 素材画面里的比例 → 舞台上的像素位置 */
  fromNorm(x: number, y: number): { px: number; py: number }
}

/**
 * 监视器里素材画面的几何：和 EditMonitor 里视频元素的摆法一致——元素按“转之前”的宽高居中摆放，
 * 画面按 object-fit 放进元素，再整体 `scale(翻转) rotate(旋转)`。框的位置据此换算，旋转、翻转、留黑边、铺满都对得上。
 */
export function monitorGeom(stage: { w: number; h: number }, c: { rotate: number; flipH: boolean; flipV: boolean }, src: { w: number; h: number }, fit: 'contain' | 'cover' | 'blur'): MonitorGeom {
  const turned = c.rotate === 90 || c.rotate === 270
  const bw = turned ? stage.h : stage.w
  const bh = turned ? stage.w : stage.h
  const sw = Math.max(1, src.w)
  const sh = Math.max(1, src.h)
  const k = fit === 'cover' ? Math.max(bw / sw, bh / sh) : Math.min(bw / sw, bh / sh)
  const cw = sw * k
  const ch = sh * k
  const th = (c.rotate * Math.PI) / 180
  const cos = Math.round(Math.cos(th) * 1e12) / 1e12
  const sin = Math.round(Math.sin(th) * 1e12) / 1e12
  const sx = c.flipH ? -1 : 1
  const sy = c.flipV ? -1 : 1
  return {
    box: { left: (stage.w - bw) / 2, top: (stage.h - bh) / 2, w: bw, h: bh },
    content: { left: (bw - cw) / 2, top: (bh - ch) / 2, w: cw, h: ch },
    transform: `scale(${sx}, ${sy}) rotate(${c.rotate}deg)`,
    toNorm(px, py) {
      // transform 是 scale·rotate：先转再翻，反过来先翻再反向转
      const ux = sx * (px - stage.w / 2)
      const uy = sy * (py - stage.h / 2)
      const lx = ux * cos + uy * sin
      const ly = -ux * sin + uy * cos
      return { x: lx / cw + 0.5, y: ly / ch + 0.5 }
    },
    fromNorm(x, y) {
      const lx = (x - 0.5) * cw
      const ly = (y - 0.5) * ch
      const rx = lx * cos - ly * sin
      const ry = lx * sin + ly * cos
      return { px: stage.w / 2 + sx * rx, py: stage.h / 2 + sy * ry }
    },
  }
}

/**
 * 跟随聚焦的取景窗口大小（占素材画面的比例）：不跟随成片比例时是整个画面除以放大倍数；
 * 跟随成片比例（`aspect` = 成片宽 / 高，已换算成素材方向）时先取画面里最大的这个比例的矩形，再除以放大倍数。
 * 和后端 `edit/region.rs` 的 `focus_window` 一致（后端算整数像素，这里只用来在监视器里画示意框）。
 */
export function focusWindow(zoom: number, src: { w: number; h: number }, aspect: number | null): { w: number; h: number } {
  const sw = Math.max(1, src.w)
  const sh = Math.max(1, src.h)
  let bw = sw
  let bh = sh
  if (aspect !== null && Number.isFinite(aspect) && aspect > 0) {
    if (aspect >= sw / sh) bh = sw / aspect
    else bw = sh * aspect
  }
  const z = Math.max(1, zoom)
  return { w: Math.min(1, bw / z / sw), h: Math.min(1, bh / z / sh) }
}

/** 取景窗口在 t 时刻的位置：以区域中心为中心，收在画面里（没有做镜头平滑，只是示意）。 */
export function focusRect(r: EditRegion, t: number, win: { w: number; h: number }): NRect | null {
  const b = trackAt(r.track, t)
  if (!b) return null
  return { x: clamp(b.x + b.w / 2 - win.w / 2, 0, 1 - win.w), y: clamp(b.y + b.h / 2 - win.h / 2, 0, 1 - win.h), w: win.w, h: win.h }
}

/** 成片比例换算到素材方向（旋转 90 / 270 度时宽高对调）。 */
export function aspectInSource(outW: number, outH: number, rotate: number): number {
  const a = outH > 0 ? outW / outH : 1
  return rotate === 90 || rotate === 270 ? 1 / a : a
}

// ---------- 整理操作 ----------
// 裁到播放位置、分离音频、波纹删除、跳到剪辑点、逐帧、把设置应用到全部片段。都是纯计算，界面和测试共用。

/** 把片段在时间线 t 处裁掉一边：`in` 去掉 t 之前的，`out` 去掉 t 之后的。保留 id、转场和另一端的淡入淡出；离两端不足 MIN_MS 返回 null。 */
export function trimClipAt(c: EditClip, pl: Placed, t: number, side: 'in' | 'out'): EditClip | null {
  const parts = splitClip(c, pl, t, c.id)
  if (!parts) return null
  if (side === 'in') return { ...parts[1], transition: c.transition ? { ...c.transition } : null, fadeInMs: c.fadeInMs }
  return { ...parts[0], fadeOutMs: c.fadeOutMs }
}

/** 同上，裁叠加素材：`in` 之后它从 t 开始（位置跟着变），`out` 之后它在 t 结束。循环的素材会算好循环到哪里了。 */
export function trimOverlayAt(o: EditOverlay, t: number, side: 'in' | 'out', srcDurMs?: number | null): EditOverlay | null {
  const parts = splitOverlay(o, t, o.id, srcDurMs)
  if (!parts) return null
  if (side === 'in') return { ...parts[1], id: o.id, fadeInMs: o.fadeInMs, animIn: o.animIn }
  return { ...parts[0], fadeOutMs: o.fadeOutMs, animOut: o.animOut }
}

/**
 * 把片段 / 叠加素材自带的声音单独拿出来，变成配乐轨上的一条（`startMs` 是它在时间线上开始的位置）。
 * 不能分离时返回原因：图片和没有声音的素材；变速的（配乐没有变速，声音会对不上画面）；已经静音的；循环播放的叠加素材。
 */
export function detachedAudio(
  item: { path: string; kind: 'video' | 'image'; inMs: number; outMs: number; speed: number; volume: number; mute: boolean; fadeInMs: number; fadeOutMs: number; looped?: boolean },
  startMs: number,
  id: number,
  src?: EditSource,
): EditAudio | string {
  if (item.kind === 'image') return '图片没有声音。'
  if (src && !src.hasAudio) return '这个素材里没有声音。'
  if (item.mute) return '已经静音了，没有声音可以分离。'
  if (item.looped) return '循环播放的叠加素材不能分离声音。'
  if (Math.abs(item.speed - 1) > 1e-6) return '变速的素材不能分离声音（声音会和画面对不上），请先把速度调回 1×。'
  const a = blankAudio()
  a.id = id
  a.path = item.path
  a.startMs = Math.max(0, Math.round(startMs))
  a.inMs = item.inMs
  a.outMs = src?.durationMs ? Math.min(item.outMs, src.durationMs) : item.outMs
  a.volume = item.volume
  a.fadeInMs = item.fadeInMs
  a.fadeOutMs = item.fadeOutMs
  return a
}

/** 主轨某处变长（delta > 0）或变短（delta < 0）后，起点在 `from` 及以后的叠加素材、文字、配乐跟着后移 / 前移。返回移动了几项。 */
export function rippleBy(p: EditProject, from: number, delta: number): number {
  if (delta < 0) return shiftAfter(p, from, -delta)
  if (delta === 0) return 0
  let n = 0
  for (const o of p.overlays) {
    if (o.startMs >= from) {
      o.startMs = Math.min(MAX_START_MS, o.startMs + delta)
      n++
    }
  }
  for (const t of p.texts) {
    if (t.startMs >= from) {
      const len = t.endMs - t.startMs
      t.startMs += delta
      t.endMs = t.startMs + len
      n++
    }
  }
  for (const a of p.audio) {
    if (a.startMs >= from) {
      a.startMs += delta
      n++
    }
  }
  return n
}

/** 把 `from`（含）之后开始的叠加素材、文字、配乐整体往前移 `delta` 毫秒（波纹删除用）。返回移动了几个。 */
export function shiftAfter(p: EditProject, from: number, delta: number): number {
  if (delta <= 0) return 0
  let n = 0
  for (const o of p.overlays) {
    if (o.startMs >= from) {
      o.startMs = Math.max(0, o.startMs - delta)
      n++
    }
  }
  for (const t of p.texts) {
    if (t.startMs >= from) {
      const len = t.endMs - t.startMs
      t.startMs = Math.max(0, t.startMs - delta)
      t.endMs = t.startMs + len
      n++
    }
  }
  for (const a of p.audio) {
    if (a.startMs >= from) {
      a.startMs = Math.max(0, a.startMs - delta)
      n++
    }
  }
  return n
}

/** 时间线上所有“剪辑点”：0、主轨片段的起止、叠加素材和文字的起止、配乐的开头。`skip` 里的那一项不算（拖它自己时吸附用）。已去重、从小到大。 */
export function editPoints(p: EditProject, pl: Placed[], skip?: { kind: 'text' | 'audio' | 'overlay'; id: number }): number[] {
  const v: number[] = [0]
  for (const q of pl) v.push(q.startMs, q.endMs)
  for (const o of p.overlays) if (!(skip?.kind === 'overlay' && skip.id === o.id)) v.push(o.startMs, overlayEnd(o))
  for (const t of p.texts) if (!(skip?.kind === 'text' && skip.id === t.id)) v.push(t.startMs, t.endMs)
  for (const a of p.audio) if (!(skip?.kind === 'audio' && skip.id === a.id)) v.push(a.startMs)
  return [...new Set(v.map(Math.round))].sort((a, b) => a - b)
}

/** t 前面 / 后面最近的剪辑点（`dir` 为 -1 / 1）；离 t 不到 1 毫秒的不算。没有返回 null。 */
export function neighborPoint(points: number[], t: number, dir: -1 | 1): number | null {
  if (dir > 0) return points.find((x) => x > t + 0.5) ?? null
  for (let i = points.length - 1; i >= 0; i--) if (points[i] < t - 0.5) return points[i]
  return null
}

/** 往前 / 往后走一帧（对齐到帧边界）。`fps` 不可用时按 30 帧。 */
export function stepFrame(t: number, fps: number | null | undefined, dir: -1 | 1): number {
  const rate = fps && fps > 0 && fps <= 240 ? fps : 30
  const fm = 1000 / rate
  const k = t / fm
  const f = dir > 0 ? Math.floor(k + 1e-6) + 1 : Math.ceil(k - 1e-6) - 1
  return Math.max(0, Math.round(f * fm))
}

/** 逐帧用的帧率：成片指定了就用它，否则取播放位置下那个片段的帧率。 */
export function playbackFps(p: EditProject, pl: Placed[], t: number, sources: Record<string, EditSource | undefined>): number | null {
  if (p.out.fps > 0) return p.out.fps
  const i = clipIndexAt(pl, t)
  const c = p.clips[i >= 0 ? i : 0]
  return (c && sources[c.path]?.fps) || null
}

/**
 * 把一个片段的设置应用到其他所有片段：`transition` 转场（第一个片段前面没有衔接，不用）、`tone` 亮度 / 对比度 / 饱和度、`turn` 旋转和翻转。
 * 返回有多少个片段变了。
 */
export function applyToAllClips(p: EditProject, fromId: number, what: 'transition' | 'tone' | 'turn'): number {
  const src = p.clips.find((c) => c.id === fromId)
  if (!src) return 0
  let n = 0
  p.clips.forEach((c, i) => {
    if (c.id === fromId) return
    if (what === 'transition') {
      if (i === 0) return
      const same = (c.transition?.kind ?? '') === (src.transition?.kind ?? '') && (c.transition?.durationMs ?? 0) === (src.transition?.durationMs ?? 0)
      if (same) return
      c.transition = src.transition ? { ...src.transition } : null
    } else if (what === 'tone') {
      if (c.brightness === src.brightness && c.contrast === src.contrast && c.saturation === src.saturation) return
      c.brightness = src.brightness
      c.contrast = src.contrast
      c.saturation = src.saturation
    } else {
      if (c.rotate === src.rotate && c.flipH === src.flipH && c.flipV === src.flipV) return
      c.rotate = src.rotate
      c.flipH = src.flipH
      c.flipV = src.flipV
    }
    n++
  })
  return n
}

/** 去掉所有片段的转场。返回有多少个片段变了。 */
export function clearTransitions(p: EditProject): number {
  let n = 0
  for (const c of p.clips) {
    if (c.transition) {
      c.transition = null
      n++
    }
  }
  return n
}


// ---------- 节拍卡点 ----------

/** 读旧工程 / 外来数据里的节拍：不合格的丢掉 */
export function cleanBeats(b: EditBeats | null | undefined): EditBeats | null {
  if (!b || !Array.isArray(b.beatsMs)) return null
  const v = [...new Set(b.beatsMs.filter((x) => Number.isFinite(x) && x >= 0).map(Math.round))].sort((x, y) => x - y)
  if (v.length < 2) return null
  return { bpm: Number.isFinite(b.bpm) ? b.bpm : 0, confidence: Number.isFinite(b.confidence) ? clamp(b.confidence, 0, 1) : 0, beatsMs: v }
}

/** 节拍清晰度低于这个就提示“结果不太可信”（和后端 `LOW_CONFIDENCE` 一致） */
export const LOW_BEAT_CONFIDENCE = 0.12

/** 配乐的节拍在时间线上的位置（毫秒）：只留配乐用到的那一段（入点到出点），循环播放的不重复标。 */
export function beatsOnTimeline(a: EditAudio): number[] {
  if (!a.beats) return []
  const out: number[] = []
  for (const b of a.beats.beatsMs) {
    if (b < a.inMs || (a.outMs !== null && b > a.outMs)) continue
    out.push(a.startMs + b - a.inMs)
  }
  return out
}

/** 吸附用：所有配乐的节拍点（跳过正在拖的那条） */
export function beatSnapPoints(p: EditProject, skipAudioId?: number): number[] {
  const v: number[] = []
  for (const a of p.audio) if (a.id !== skipAudioId) v.push(...beatsOnTimeline(a))
  return v
}

export interface BeatArrange {
  clips: EditClip[]
  /** 对齐到节拍的片段数 */
  arranged: number
  /** 素材不够长、没法撑满一格的视频片段数 */
  short: number
  /** 节拍用完了、保持原样的片段数 */
  left: number
}

/**
 * 让主轨按节拍切换：第一个片段从 0 到第 `perClip` 个拍子，之后每个片段占 `perClip` 拍。
 * 图片、定格直接设时长；视频缩短到那一格（不能拉长，素材不够长就用满、并算进 `short`，后面的片段从实际结尾接着排）。
 * 被排过的片段去掉转场（转场会让切换点和拍子错开）。`srcDur` 给出素材总长（毫秒），未知返回 undefined。
 */
export function arrangeToBeats(clips: EditClip[], beats: number[], perClip: number, srcDur: (path: string) => number | undefined): BeatArrange | null {
  const per = Math.max(1, Math.round(perClip))
  const out: EditClip[] = clips.map((c) => ({ ...c }))
  let cur = 0
  let arranged = 0
  let short = 0
  let left = 0
  for (let i = 0; i < out.length; i++) {
    const c = out[i]
    const target = beats[(i + 1) * per - 1]
    if (target === undefined || target - cur < MIN_MS) {
      left++
      cur += clipDuration(c) - (i > 0 ? Math.min(c.transition?.durationMs ?? 0, clipDuration(c)) : 0)
      continue
    }
    const slot = target - cur
    if (c.kind === 'image' || c.freeze) {
      c.outMs = c.inMs + slot
    } else {
      const need = Math.round(slot * Math.max(0.01, c.speed))
      const total = srcDur(c.path)
      const avail = total === undefined ? c.outMs - c.inMs : Math.max(MIN_MS, total - c.inMs)
      // 视频只缩短：原来选的范围比一格短就保持原样
      const have = c.outMs - c.inMs
      const want = Math.min(need, have)
      if (need > have) short++
      if (c.reverse) c.inMs = c.outMs - want
      else c.outMs = c.inMs + Math.min(want, avail)
    }
    c.transition = null
    c.fadeOutMs = Math.min(c.fadeOutMs, clipDuration(c))
    c.fadeInMs = Math.min(c.fadeInMs, clipDuration(c))
    cur += clipDuration(c)
    arranged++
  }
  return arranged ? { clips: out, arranged, short, left } : null
}

/**
 * 让配乐适配成片长度：太短就循环；太长就在成片结尾（尽量落在最后一个拍子上）截掉，并加结尾淡出。
 * 返回要改的字段；`total` 是成片总长（毫秒），`src` 是配乐素材。
 */
export function fitAudio(a: EditAudio, src: EditSource | undefined, total: number): Partial<EditAudio> | null {
  const avail = total - a.startMs
  if (avail < MIN_MS * 2) return null
  const have = a.outMs !== null ? a.outMs - a.inMs : src?.durationMs ? src.durationMs - a.inMs : null
  const fade = Math.min(1500, Math.round(avail / 3))
  if (have === null) return { fadeOutMs: fade }
  if (have < avail) return { looped: true, outMs: a.outMs ?? src?.durationMs ?? null, fadeOutMs: fade }
  let end = a.inMs + avail
  const beats = beatsOnTimeline({ ...a, outMs: null }).filter((t) => t <= total && t > a.startMs + avail / 2)
  if (beats.length) end = a.inMs + (beats[beats.length - 1] - a.startMs)
  return { looped: false, outMs: Math.round(Math.max(a.inMs + MIN_MS, end)), fadeOutMs: Math.min(fade, Math.max(0, Math.round(end - a.inMs))) }
}

// ---------- 抠像（实时预览） ----------

/**
 * 实时预览用的近似抠像：浏览器里没有 `chromakey`，用一个 SVG `feColorMatrix` 按“离键色的色相方向有多远”算透明度。
 * 返回 5×4 矩阵（20 个数）；和导出的结果有差别，导出为准。
 */
export function chromaMatrix(c: EditChroma): number[] {
  const h = c.color.replace('#', '')
  const rgb = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255)
  const mean = (rgb[0] + rgb[1] + rgb[2]) / 3
  let d = rgb.map((v) => v - mean)
  let n = Math.hypot(d[0], d[1], d[2])
  // 黑 / 白 / 灰这类没有色相的键色：改用亮度方向
  if (n < 0.05) {
    d = [0.3, 0.59, 0.11].map((v) => (mean > 0.5 ? v : -v))
    n = 1
  }
  d = d.map((v) => v / n)
  const kk = d[0] * rgb[0] + d[1] * rgb[1] + d[2] * rgb[2]
  const mid = kk * (1 - 1.5 * clamp(c.similarity, 0.01, 0.6))
  const w = Math.max(0.03, c.blend * 1.5 * Math.abs(kk) + 0.05)
  const k = (mid + w / 2) / w
  return [1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, -d[0] / w, -d[1] / w, -d[2] / w, 0, k]
}
