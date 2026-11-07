import type { ReverbSpec } from '../types'

/** 混响预设（工具箱“音效 / 混响”和剪辑共用）：混合 dB / 混响时间 ms / 高频保留 / 预延迟 ms */
export const REVERBS = [
  { id: 'room', name: '房间', mix: -9, time: 500, hf: 0.7, pre: 8 },
  { id: 'bath', name: '浴室', mix: -5, time: 1400, hf: 0.9, pre: 5 },
  { id: 'living', name: '客厅', mix: -10, time: 700, hf: 0.55, pre: 12 },
  { id: 'class', name: '教室', mix: -8, time: 1100, hf: 0.65, pre: 18 },
  { id: 'studio', name: '录音棚', mix: -14, time: 350, hf: 0.5, pre: 4 },
  { id: 'plate', name: '板式', mix: -7, time: 1800, hf: 0.85, pre: 20 },
  { id: 'hall', name: '音乐厅', mix: -8, time: 2600, hf: 0.6, pre: 30 },
  { id: 'church', name: '教堂', mix: -6, time: 4500, hf: 0.5, pre: 45 },
]

/** 预设 → 混响参数；不认识的 id 返回 null（= 不加混响）。 */
export function reverbOf(id: string): ReverbSpec | null {
  const r = REVERBS.find((x) => x.id === id)
  return r ? { mixDb: r.mix, timeMs: r.time, hfRatio: r.hf, preDelayMs: r.pre, inGainDb: 0 } : null
}

/** 混响参数是哪个预设（对不上任何预设时返回 'custom'，没有混响返回 'off'）。 */
export function reverbId(r: ReverbSpec | null | undefined): string {
  if (!r) return 'off'
  return REVERBS.find((x) => x.mix === r.mixDb && x.time === r.timeMs && x.hf === r.hfRatio && x.pre === r.preDelayMs)?.id ?? 'custom'
}
