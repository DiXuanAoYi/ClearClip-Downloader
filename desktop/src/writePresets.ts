/** 写作浮窗里的动作和中文场景模板。 */
export interface WriteAction {
  id: string
  name: string
  /** 走语法检查（kind=grammar）而不是改写 */
  grammar?: boolean
}

export const ACTIONS: WriteAction[] = [
  { id: 'rewrite', name: '改写' },
  { id: 'grammar', name: '语法检查', grammar: true },
  { id: 'optimize', name: '润色' },
  { id: 'concise', name: '更简洁' },
  { id: 'shorten', name: '缩短' },
  { id: 'translate', name: '翻译' },
  { id: 'summarize', name: '总结' },
  { id: 'continue', name: '续写' },
  { id: 'polite', name: '更礼貌' },
  { id: 'persuasive', name: '更有说服力' },
]

export interface Preset {
  id: string
  name: string
  /** 写进 custom_format 的文体名 */
  format: string
  /** 额外要求 */
  hint: string
}

/** 中文常见场景：每个给出文体名和写法要点。 */
export const PRESETS: Preset[] = [
  { id: 'xhs', name: '小红书', format: '小红书笔记', hint: '开头一句抓人的话；分短段，适度使用表情；结尾带 3 到 5 个话题标签；口语、真诚、不夸大。' },
  { id: 'weibo', name: '微博', format: '微博', hint: '简短有力，一两句话说清重点，可带一个话题标签，不超过 140 字。' },
  { id: 'moments', name: '朋友圈', format: '朋友圈文案', hint: '自然、有生活感，像本人随手写的，不要营销腔，不超过 100 字。' },
  { id: 'wxtitle', name: '公众号标题', format: '微信公众号标题', hint: '给出标题，信息清楚、有吸引力但不做标题党，每个不超过 22 字。' },
  { id: 'zhihu', name: '知乎回答', format: '知乎回答', hint: '先给结论，再分点论证；有依据、有例子，语气理性克制。' },
  { id: 'weekly', name: '周报', format: '工作周报', hint: '按“本周完成 / 遇到的问题 / 下周计划”分条列出，用数据和结果说话，不空话。' },
  { id: 'resume', name: '简历描述', format: '简历经历描述', hint: '动词开头，写清做了什么、怎么做、结果如何，尽量量化，每条一行。' },
  { id: 'service', name: '客服回复', format: '客服回复', hint: '先致歉或致谢，再给明确的解决办法和时间；礼貌、专业、简洁。' },
]

export function presetById(id: string): Preset | undefined {
  return PRESETS.find((p) => p.id === id)
}

/** 合成发给后端的请求。`style` 的个人风格由后端并入。 */
export function buildWriteRequest(opts: { action: WriteAction; text: string; preset?: Preset; extra?: string; target?: string; app?: string }) {
  const { action, text, preset, extra, target, app } = opts
  const hint = [preset?.hint, extra?.trim()].filter(Boolean).join('\n')
  return {
    kind: action.grammar ? 'grammar' : 'write',
    text,
    write: {
      action: action.grammar ? 'rewrite' : action.id,
      format: preset ? 'custom' : 'auto',
      custom_format: preset?.format ?? '',
      extra: hint,
      target: action.id === 'translate' ? (target ?? '') : '',
      page: app ?? '',
    },
  }
}
