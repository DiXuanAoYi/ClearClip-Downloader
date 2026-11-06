// 响度标准化的行业预设：目标响度 + 真峰值上限 + 响度范围。工具箱的“响度标准化”和“音效 / 混响”里的响度标准化共用。
// 数值取自各行业常用的规范或惯例；平台的实际规则会变，拿不准时以平台当前的要求为准，这里是“够用的起点”。

export interface LoudnessPreset {
  id: string
  name: string
  /** 目标响度（LUFS） */
  lufs: number
  /** 真峰值上限（dBTP） */
  tp: number
  /** 响度范围目标（LU） */
  lra: number
  note: string
}

export const LOUDNESS_PRESETS: LoudnessPreset[] = [
  { id: 'web', name: '手机和网页通用', lufs: -16, tp: -1.5, lra: 11, note: '大多数情况的默认选择：不刺耳，也不会太小声。' },
  { id: 'stream', name: '流媒体（YouTube、Spotify 等）', lufs: -14, tp: -1, lra: 11, note: '主流流媒体平台会把过响的内容压低，做到这个值就不会被再调一遍。' },
  { id: 'podcast', name: '播客、有声书、网课', lufs: -16, tp: -1, lra: 7, note: '以说话为主：响度范围收窄，轻声和大声之间差得少，戴耳机听也舒服。' },
  { id: 'short', name: '短视频（偏响亮）', lufs: -12, tp: -1, lra: 8, note: '声音更饱满，适合手机外放；代价是动态变小。' },
  { id: 'ebu', name: '电视广播 EBU R128（欧洲）', lufs: -23, tp: -1, lra: 15, note: '欧洲电视广播的节目响度标准。' },
  { id: 'atsc', name: '电视广播 ATSC A/85（美国）', lufs: -24, tp: -2, lra: 15, note: '美国电视广播的节目响度标准。' },
  { id: 'cn-tv', name: '电视播出（中国 GY/T 262）', lufs: -24, tp: -2, lra: 15, note: '国内电视播出的节目响度要求，约 -24 LUFS。' },
  { id: 'night', name: '夜间 / 小声', lufs: -20, tp: -2, lra: 7, note: '整体偏小声、起伏小，夜里不吵人。' },
]

export const CUSTOM_LOUDNESS = 'custom'

/** 这组数正好等于哪个预设；都不是就是“自定义”。 */
export function loudnessPresetId(lufs: number, tp: number, lra: number): string {
  const hit = LOUDNESS_PRESETS.find((x) => x.lufs === lufs && x.tp === tp && x.lra === lra)
  return hit ? hit.id : CUSTOM_LOUDNESS
}

export function findLoudnessPreset(id: string): LoudnessPreset | undefined {
  return LOUDNESS_PRESETS.find((x) => x.id === id)
}
