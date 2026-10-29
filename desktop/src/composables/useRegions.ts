// 剪辑里“跟着物体走的区域”的状态和操作：框选 / 感知运动物体、手动校正、追踪、增删改。
// 轨迹本身存在片段的 `regions` 里（跟着工程一起保存、撤销）；这里只放界面上的临时状态。

import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import type { EditClip, EditRegion, NRect, RegionEffect, TrackCandidate } from '../types'
import { MAX_REGIONS, clamp, clipIndexAt, lostSpans, mergeTracked, newRegion, nextRegionId, resizeAll, setPin, sourceTimeAt, trackAt } from '../utils/edit'
import type { EditApi } from './useEditProject'

export interface RegionNote {
  kind: 'info' | 'warn' | 'error'
  text: string
}

/** 追踪任务的编号：后端用它区分进度和取消 */
let seq = (Math.floor(Date.now() / 1000) % 1_000_000) * 1000
/** 播放位置离手动点多近算“在这个点上” */
const PIN_NEAR_MS = 400

export function useRegions(ed: EditApi) {
  /** 选中的区域（所选片段里的） */
  const selId = ref<number | null>(null)
  /** 等着用户在监视器上框一个区域 */
  const drawing = ref(false)
  const drawEffect = ref<RegionEffect>('mosaic')
  const candidates = ref<TrackCandidate[]>([])
  const detecting = ref(false)
  const note = ref<RegionNote | null>(null)
  /** “区域”页是不是正在显示：显示时监视器上才出现框，删除键也先删区域 */
  const tabActive = ref(false)
  const tracking = reactive({ running: false, id: 0, percent: 0, clipId: 0, regionId: 0 })
  /** 最近一次追踪的结果（只在这次打开期间显示） */
  const results = reactive<Record<string, { lostMs: number; note: string | null }>>({})

  const clip = computed<EditClip | undefined>(() => (ed.selectedClip.value?.kind === 'video' ? ed.selectedClip.value : undefined))
  const region = computed<EditRegion | undefined>(() => clip.value?.regions.find((r) => r.id === selId.value))
  const index = computed(() => (clip.value ? ed.project.value.clips.findIndex((c) => c.id === clip.value?.id) : -1))
  const placed = computed(() => (index.value >= 0 ? ed.placed.value[index.value] : undefined))

  /** 播放位置在所选片段里（监视器里看到的就是它） */
  const here = computed(() => {
    const pl = ed.placed.value
    if (index.value < 0 || !pl.length) return false
    const t = ed.playhead.value
    const i = clipIndexAt(pl, t)
    return i === index.value || (i < 0 && index.value === pl.length - 1 && t >= pl[pl.length - 1].endMs)
  })
  /** 播放位置对应的素材时间（毫秒） */
  const srcMs = computed(() => (clip.value && placed.value ? Math.round(sourceTimeAt(clip.value, placed.value, ed.playhead.value)) : 0))

  /** 素材时间 → 时间线位置（毫秒） */
  function timelineAt(ms: number): number {
    const c = clip.value
    const pl = placed.value
    if (!c || !pl) return 0
    return clamp(pl.startMs + (ms - c.inMs) / Math.max(0.01, c.speed), pl.startMs, pl.endMs)
  }

  const keyOf = (clipId: number, regionId: number) => `${clipId}:${regionId}`
  const result = computed(() => (clip.value && region.value ? results[keyOf(clip.value.id, region.value.id)] : undefined))

  // ---------- 选择 ----------

  watch(
    () => `${ed.sel.value?.kind}:${ed.sel.value?.id}`,
    () => {
      selId.value = null
      cancelDraw()
      note.value = null
    },
  )
  watch(
    () => clip.value?.regions.map((r) => r.id).join(','),
    () => {
      if (selId.value !== null && !region.value) selId.value = null
    },
  )

  function select(id: number | null) {
    selId.value = id
    if (id !== null) cancelDraw()
    note.value = null
  }

  // ---------- 感知运动物体 ----------

  let detectSeq = 0
  let detectTimer: ReturnType<typeof setTimeout> | undefined

  async function detect() {
    const c = clip.value
    if (!c || !here.value) return
    const my = ++detectSeq
    detecting.value = true
    note.value = null
    try {
      const r = await api.trackDetect(c.path, srcMs.value)
      if (my !== detectSeq) return
      candidates.value = r.candidates
      if (r.note) note.value = { kind: 'info', text: r.note }
      else if (!r.candidates.length) note.value = { kind: 'info', text: '这一帧没有感知到明显在动的物体。可以换一个物体在动的时刻，或者直接在画面上拖出一个框。' }
    } catch (e) {
      if (my !== detectSeq) return
      candidates.value = []
      note.value = { kind: 'error', text: errorText(e) }
    } finally {
      if (my === detectSeq) detecting.value = false
    }
  }

  function startDraw(effect?: RegionEffect) {
    if (!clip.value) return
    if (clip.value.regions.length >= MAX_REGIONS) {
      note.value = { kind: 'warn', text: `一个片段最多 ${MAX_REGIONS} 个区域。` }
      return
    }
    if (effect) drawEffect.value = effect
    selId.value = null
    drawing.value = true
    candidates.value = []
    note.value = null
    if (here.value) detect()
  }

  function cancelDraw() {
    drawing.value = false
    candidates.value = []
    detectSeq++
    detecting.value = false
    clearTimeout(detectTimer)
  }

  // 框选期间播放位置换了：稍等一下（拖动时间线不要一直算）再重新感知
  watch([() => srcMs.value, () => clip.value?.id, () => ed.playing.value], () => {
    if (!drawing.value) return
    candidates.value = []
    detectSeq++
    clearTimeout(detectTimer)
    if (ed.playing.value || !here.value) {
      detecting.value = false
      return
    }
    detecting.value = true
    detectTimer = setTimeout(detect, 450)
  })

  // ---------- 增删改 ----------

  /** 在播放位置放一个新区域，框是 `rect`。 */
  function add(rect: NRect, effect: RegionEffect = drawEffect.value): EditRegion | null {
    const c = clip.value
    if (!c || !here.value) {
      note.value = { kind: 'warn', text: '请先把播放位置移到这个片段里。' }
      return null
    }
    if (c.regions.length >= MAX_REGIONS) {
      note.value = { kind: 'warn', text: `一个片段最多 ${MAX_REGIONS} 个区域。` }
      return null
    }
    const made: { r: EditRegion | null } = { r: null }
    ed.mutate((p) => {
      const cc = p.clips.find((x) => x.id === c.id)
      if (!cc) return
      const r = newRegion(nextRegionId(cc), rect, clamp(srcMs.value, cc.inMs, cc.outMs), effect)
      // 聚焦一个片段只能有一个：新建的聚焦替掉旧的
      if (effect === 'focus') cc.regions = cc.regions.filter((x) => x.effect !== 'focus')
      cc.regions.push(r)
      made.r = r
    })
    cancelDraw()
    if (made.r) selId.value = made.r.id
    return made.r
  }

  function patch(id: number, p: Partial<EditRegion>, key: string) {
    const c = clip.value
    if (!c) return
    ed.mutate((proj) => {
      const cc = proj.clips.find((x) => x.id === c.id)
      const r = cc?.regions.find((x) => x.id === id)
      if (!cc || !r) return
      Object.assign(r, p)
      // 改成聚焦时，别的聚焦让位
      if (p.effect === 'focus') cc.regions = cc.regions.filter((x) => x.id === id || x.effect !== 'focus')
    }, `region:${key}:${c.id}:${id}`)
  }

  function remove(id: number) {
    const c = clip.value
    if (!c) return
    if (tracking.running && tracking.regionId === id) cancelTrack()
    ed.mutate((proj) => {
      const cc = proj.clips.find((x) => x.id === c.id)
      if (cc) cc.regions = cc.regions.filter((r) => r.id !== id)
    })
    if (selId.value === id) selId.value = null
  }

  /** 把区域在播放位置这一刻放到 `rect`（手动点）。`only` 为真时大小也只改这一刻，否则整段一起变。 */
  function place(id: number, rect: NRect, opts: { resize?: boolean; only?: boolean } = {}) {
    const c = clip.value
    if (!c || !here.value) return
    const t = clamp(srcMs.value, c.inMs, c.outMs)
    ed.mutate((proj) => {
      const cc = proj.clips.find((x) => x.id === c.id)
      const r = cc?.regions.find((x) => x.id === id)
      if (!r) return
      if (opts.resize && !opts.only) r.track = resizeAll(r.track, rect.w, rect.h)
      r.track = setPin(r.track, t, rect)
    }, `regionbox:${c.id}:${id}`)
  }

  /** 去掉播放位置上的手动点（轨迹回到自动追踪的结果）。 */
  function unpin(id: number) {
    const c = clip.value
    if (!c) return
    const t = srcMs.value
    ed.mutate((proj) => {
      const r = proj.clips.find((x) => x.id === c.id)?.regions.find((x) => x.id === id)
      if (!r || r.track.length <= 1) return
      // 找最近的手动点
      let best = -1
      let bd = PIN_NEAR_MS
      r.track.forEach((p, i) => {
        if (p.pin && Math.abs(p.tMs - t) <= bd) {
          bd = Math.abs(p.tMs - t)
          best = i
        }
      })
      if (best >= 0) r.track.splice(best, 1)
    })
  }

  /** 播放位置附近有没有手动点。 */
  const pinHere = computed(() => !!region.value && region.value.track.length > 1 && region.value.track.some((p) => p.pin && Math.abs(p.tMs - srcMs.value) <= 1500))

  // ---------- 追踪 ----------

  let unlisten: UnlistenFn | undefined
  onMounted(async () => {
    unlisten = await events.onTrackProgress((p) => {
      if (tracking.running && p.id === tracking.id) tracking.percent = clamp(p.percent, 0, 100)
    })
  })
  onBeforeUnmount(() => {
    unlisten?.()
    clearTimeout(detectTimer)
    if (tracking.running) api.trackCancel(tracking.id).catch(() => undefined)
  })

  /** 从播放位置这一帧的框出发，向前向后追踪整个片段。结果并进轨迹，已有的手动点不动。 */
  async function follow(id: number) {
    const c = clip.value
    const r = c?.regions.find((x) => x.id === id)
    if (!c || !r || tracking.running) return
    if (!here.value) {
      note.value = { kind: 'warn', text: '请先把播放位置移到这个片段里、物体清楚可见的那一帧，再开始追踪。' }
      return
    }
    const refMs = clamp(srcMs.value, c.inMs, c.outMs)
    const rect = trackAt(r.track, refMs)
    if (!rect) return
    const { inMs, outMs, id: clipId } = c
    const run = { running: true, id: ++seq, percent: 0, clipId, regionId: id }
    Object.assign(tracking, run)
    note.value = null
    try {
      const res = await api.trackFollow({ id: run.id, path: c.path, fromMs: inMs, toMs: outMs, refMs, rect })
      if (!ed.clipOf(clipId)?.regions.some((x) => x.id === id)) return
      ed.mutate((proj) => {
        const target = proj.clips.find((x) => x.id === clipId)?.regions.find((x) => x.id === id)
        if (target) target.track = mergeTracked(target.track, res.points, inMs, outMs)
      })
      results[keyOf(clipId, id)] = { lostMs: res.lostMs, note: res.note }
      if (res.note) note.value = { kind: 'warn', text: res.note }
    } catch (e) {
      const msg = errorText(e)
      note.value = msg === 'canceled' ? { kind: 'info', text: '已取消追踪。' } : { kind: 'error', text: msg }
    } finally {
      tracking.running = false
      tracking.percent = 0
    }
  }

  async function cancelTrack() {
    if (tracking.running) await api.trackCancel(tracking.id).catch(() => undefined)
  }

  /** 丢失的时间段（素材毫秒）里，播放位置之后的下一段（没有就回到第一段）。 */
  function nextLost(id: number): number | null {
    const r = clip.value?.regions.find((x) => x.id === id)
    if (!r) return null
    const spans = lostSpans(r.track)
    if (!spans.length) return null
    const after = spans.find(([a]) => a > srcMs.value + 40)
    return (after ?? spans[0])[0]
  }

  return {
    selId,
    drawing,
    drawEffect,
    candidates,
    detecting,
    note,
    tabActive,
    tracking,
    results,
    clip,
    region,
    result,
    index,
    placed,
    here,
    srcMs,
    pinHere,
    timelineAt,
    select,
    startDraw,
    cancelDraw,
    detect,
    add,
    patch,
    remove,
    place,
    unpin,
    follow,
    cancelTrack,
    nextLost,
  }
}

export type RegionApi = ReturnType<typeof useRegions>
