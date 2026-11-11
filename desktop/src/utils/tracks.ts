// 轨道：视频（主轨 V1 + 叠加轨 V2…）、音频、文字，每一类可以有很多条。纯计算，不碰界面。
// 规则和后端 `edit/spec.rs` 的 `TrackMeta` / `Project::effective` 保持一致。
import type { EditAudio, EditOverlay, EditProject, EditText, EditTrack, EditTrackKind, EditTrackRole } from '../types'

/** 视频轨：主轨（0）+ 叠加轨 1–11，一共 12 条 */
export const MAX_VIDEO_TRACK_NO = 11
export const MAX_AUDIO_TRACKS = 12
export const MAX_TEXT_TRACKS = 8

export const TRACK_ROLES: { id: EditTrackRole; name: string; desc: string }[] = [
  { id: 'pip', name: '画中画', desc: '缩成 40%，放在画面一角，可拖动位置' },
  { id: 'full', name: '全屏', desc: '铺满画面，盖住下面的轨道（B-roll、插入镜头）' },
  { id: 'adjust', name: '调整图层', desc: '不放画面，只放效果：调色，对下面所有轨道生效' },
  { id: 'chroma', name: '绿幕', desc: '素材默认抠像（色键），叠在下面的轨道上' },
]
export const roleName = (r: EditTrackRole) => TRACK_ROLES.find((x) => x.id === r)?.name ?? ''

export const maxNo = (kind: EditTrackKind) => (kind === 'video' ? MAX_VIDEO_TRACK_NO : kind === 'audio' ? MAX_AUDIO_TRACKS : MAX_TEXT_TRACKS)
export const minNo = (kind: EditTrackKind) => (kind === 'video' ? 0 : 1)

export function blankTrack(kind: EditTrackKind, no: number): EditTrack {
  return { kind, no, name: '', role: '', mute: false, solo: false, lock: false, hidden: false, magnet: kind === 'video' && no === 0, volume: 1, pan: 0, duck: false, color: '', height: 'm' }
}

/** 轨道的编号牌：V1 / V2 / A1 / T1 */
export function trackCode(kind: EditTrackKind, no: number): string {
  return kind === 'video' ? `V${no + 1}` : kind === 'audio' ? `A${no}` : `T${no}`
}

export function trackOf(p: Pick<EditProject, 'tracks'>, kind: EditTrackKind, no: number): EditTrack {
  return p.tracks.find((t) => t.kind === kind && t.no === no) ?? blankTrack(kind, no)
}

/** 显示用的名字：自己起的，没有就是编号牌的叫法（主轨 / 画中画…） */
export function trackTitle(t: EditTrack): string {
  if (t.name.trim()) return t.name.trim()
  if (t.kind === 'video') return t.no === 0 ? '主轨' : roleName(t.role) || '视频'
  return t.kind === 'audio' ? '音频' : '文字'
}

/** 这一类里每个素材所在的轨道编号 */
export function itemNos(p: EditProject, kind: EditTrackKind): number[] {
  if (kind === 'video') return p.overlays.map((o) => o.track)
  if (kind === 'audio') return p.audio.map((a) => a.track)
  return p.texts.map((t) => t.track)
}

/**
 * 时间线上要显示哪几条轨道（编号从小到大）：登记过的、有素材的，再加上这一类最基本的一条（V1 / A1 / T1）。
 * 空的叠加轨也留着，免得刚建好就消失。
 */
export function shownNos(p: EditProject, kind: EditTrackKind): number[] {
  const set = new Set<number>(p.tracks.filter((t) => t.kind === kind).map((t) => t.no))
  for (const n of itemNos(p, kind)) set.add(n)
  set.add(minNo(kind) === 0 ? 0 : 1)
  return [...set].filter((n) => n >= minNo(kind) && n <= maxNo(kind)).sort((a, b) => a - b)
}

/** 这一类里还能再建几条 */
export function roomFor(p: EditProject, kind: EditTrackKind): number {
  return maxNo(kind) - minNo(kind) + 1 - shownNos(p, kind).length
}

/** 新建一条轨道用哪个编号：最小的没用过的（音频、文字）/ 最上面再加一条（视频）。没有位置返回 null。 */
export function nextNo(p: EditProject, kind: EditTrackKind): number | null {
  const used = new Set(shownNos(p, kind))
  if (kind === 'video') {
    const top = Math.max(...used)
    return top < MAX_VIDEO_TRACK_NO ? top + 1 : null
  }
  for (let n = 1; n <= maxNo(kind); n++) if (!used.has(n)) return n
  return null
}

type Span = { start: number; end: number }

/** 一个素材占的时间段（毫秒），时长由 `dur` 算出 */
export function itemSpans(p: EditProject, kind: EditTrackKind, no: number, dur: { overlay: (o: EditOverlay) => number; audio: (a: EditAudio) => number }, skipId?: number): Span[] {
  if (kind === 'video') return p.overlays.filter((o) => o.track === no && o.id !== skipId).map((o) => ({ start: o.startMs, end: o.startMs + dur.overlay(o) }))
  if (kind === 'audio') return p.audio.filter((a) => a.track === no && a.id !== skipId).map((a) => ({ start: a.startMs, end: a.startMs + dur.audio(a) }))
  return p.texts.filter((t: EditText) => t.track === no && t.id !== skipId).map((t) => ({ start: t.startMs, end: t.endMs }))
}

export const overlaps = (spans: Span[], s: number, e: number) => spans.some((x) => x.start < e && x.end > s)

/**
 * 想把一段时间放到第 `want` 条轨道：这条这段时间是空的就用它，否则找最近的一条空轨道（先往上、再往下），
 * 锁定的轨道不放。都放不下返回 null（调用方新建一条）。
 */
export function freeTrack(p: EditProject, kind: EditTrackKind, want: number, s: number, e: number, dur: Parameters<typeof itemSpans>[3], skipId?: number): number | null {
  const nos = shownNos(p, kind).filter((n) => !trackOf(p, kind, n).lock && !(kind === 'video' && n === 0))
  const order = [...nos].sort((a, b) => Math.abs(a - want) - Math.abs(b - want) || b - a)
  for (const n of order) if (!overlaps(itemSpans(p, kind, n, dur, skipId), s, e)) return n
  return null
}

/** 旧工程迁移：没有轨道编号的素材，按时间重叠与否分到不同轨道（一轨里不重叠），尽量少用轨道。 */
export function packTracks(items: { id: number; start: number; end: number }[], max: number): Map<number, number> {
  const out = new Map<number, number>()
  const ends: number[] = []
  for (const it of [...items].sort((a, b) => a.start - b.start || a.id - b.id)) {
    let k = ends.findIndex((e) => e <= it.start)
    if (k < 0) {
      k = Math.min(ends.length, max - 1)
      if (k === ends.length) ends.push(0)
    }
    ends[k] = Math.max(ends[k], it.end)
    out.set(it.id, k + 1)
  }
  return out
}

/** 在 `no` 的位置插入一条空轨道：这一类里编号 >= no 的轨道和素材整体加 1。 */
export function shiftUp(p: EditProject, kind: EditTrackKind, no: number): void {
  for (const t of p.tracks) if (t.kind === kind && t.no >= no) t.no++
  if (kind === 'video') {
    for (const o of p.overlays) if (o.track >= no) o.track++
  } else if (kind === 'audio') {
    for (const a of p.audio) if (a.track >= no) a.track++
  } else {
    for (const t of p.texts) if (t.track >= no) t.track++
  }
}

/** 删掉编号为 `no` 的轨道（连同上面的素材），更高的往下补。返回被删的素材数。 */
export function dropTrack(p: EditProject, kind: EditTrackKind, no: number): number {
  let n = 0
  p.tracks = p.tracks.filter((t) => !(t.kind === kind && t.no === no))
  for (const t of p.tracks) if (t.kind === kind && t.no > no) t.no--
  if (kind === 'video') {
    n = p.overlays.filter((o) => o.track === no).length
    p.overlays = p.overlays.filter((o) => o.track !== no)
    for (const o of p.overlays) if (o.track > no) o.track--
  } else if (kind === 'audio') {
    n = p.audio.filter((a) => a.track === no).length
    p.audio = p.audio.filter((a) => a.track !== no)
    for (const a of p.audio) if (a.track > no) a.track--
  } else {
    n = p.texts.filter((t) => t.track === no).length
    p.texts = p.texts.filter((t) => t.track !== no)
    for (const t of p.texts) if (t.track > no) t.track--
  }
  return n
}

/** 把 `a`、`b` 两条轨道对调（轨道设置和素材一起换位置）。 */
export function swapTracks(p: EditProject, kind: EditTrackKind, a: number, b: number): void {
  const flip = (n: number) => (n === a ? b : n === b ? a : n)
  for (const t of p.tracks) if (t.kind === kind) t.no = flip(t.no)
  if (kind === 'video') for (const o of p.overlays) o.track = flip(o.track)
  else if (kind === 'audio') for (const x of p.audio) x.track = flip(x.track)
  else for (const t of p.texts) t.track = flip(t.track)
}

/** 取（没有就先登记）一条轨道的设置，用来修改。 */
export function metaFor(p: EditProject, kind: EditTrackKind, no: number): EditTrack {
  let t = p.tracks.find((x) => x.kind === kind && x.no === no)
  if (!t) {
    t = blankTrack(kind, no)
    p.tracks.push(t)
  }
  return t
}

/**
 * 预览 / 导出真正用的工程：隐藏的视频轨、文字轨上的素材不要，静音的轨不出声，音频轨的音量和自动避让落到每条音频上，
 * 文字按轨道从下到上排。`preview` 为真时再看独奏：有轨道独奏，就只听独奏的音频轨。和后端 `Project::effective` 一致。
 */
export function effectiveProject(p: EditProject, preview = false): EditProject {
  const q: EditProject = { ...p, clips: p.clips.map((c) => ({ ...c })), overlays: p.overlays.map((o) => ({ ...o })), texts: [...p.texts], audio: p.audio.map((a) => ({ ...a, keyframes: { ...a.keyframes } })) }
  const m = (k: EditTrackKind, n: number) => p.tracks.find((t) => t.kind === k && t.no === n)
  if (m('video', 0)?.mute) for (const c of q.clips) c.mute = true
  q.overlays = q.overlays.filter((o) => !m('video', o.track)?.hidden)
  for (const o of q.overlays) if (m('video', o.track)?.mute) o.mute = true
  const solo = preview && p.tracks.some((t) => t.kind === 'audio' && t.solo)
  q.audio = q.audio.filter((a) => !m('audio', a.track)?.mute && (!solo || m('audio', a.track)?.solo))
  for (const a of q.audio) {
    const t = m('audio', a.track)
    if (t) {
      a.volume = Math.min(4, Math.max(0, a.volume * t.volume))
      a.duck = a.duck || t.duck
      // 音量线上的每个点也乘上轨道音量
      if (a.keyframes.volume?.length) a.keyframes.volume = a.keyframes.volume.map((k) => ({ ...k, value: Math.min(4, Math.max(0, k.value * t.volume)) }))
    }
  }
  q.texts = q.texts.filter((t) => !m('text', t.track)?.hidden)
  q.texts.sort((a, b) => a.track - b.track)
  return q
}
