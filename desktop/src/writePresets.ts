/** 写作浮窗里的动作和中文场景模板。 */
export interface WriteAction {
  id: string
  name: string
  /** 走语法检查（kind=grammar）而不是改写 */
  grammar?: boolean
}

export const ACTIONS: WriteAction[] = [
  { id: 'rewrite', name: '改写' },
  { id: 'reply', name: '回复' },
  { id: 'grammar', name: '语法检查', grammar: true },
  { id: 'optimize', name: '润色' },
  { id: 'concise', name: '更简洁' },
  { id: 'shorten', name: '缩短' },
  { id: 'translate', name: '翻译' },
  { id: 'summarize', name: '总结' },
  { id: 'continue', name: '续写' },
  { id: 'polite', name: '更礼貌' },
  { id: 'persuasive', name: '更有说服力' },
  { id: 'persona', name: '人设' },
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
export function buildWriteRequest(opts: { action: WriteAction; text: string; preset?: Preset; extra?: string; target?: string; app?: string; tone?: string; persona?: string }) {
  const { action, text, preset, extra, target, app, tone, persona } = opts
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
      tone: !action.grammar && tone ? tone : 'auto',
      page: app ?? '',
      persona: action.id === 'persona' ? (persona ?? '') : '',
    },
  }
}

// ---------- 写作窗的四个标签 ----------

export type Tab = 'reply' | 'rewrite' | 'check' | 'more' | 'persona'
export const TABS: { id: Tab; name: string }[] = [
  { id: 'reply', name: '帮我回' },
  { id: 'rewrite', name: '改写润色' },
  { id: 'check', name: '检查' },
  { id: 'more', name: '翻译 · 更多' },
  { id: 'persona', name: '人设' },
]
/** 每个标签里能选的动作（ACTIONS 的 id） */
export const TAB_ACTIONS: Record<Tab, string[]> = {
  reply: ['reply'],
  rewrite: ['rewrite', 'optimize', 'concise', 'shorten', 'polite', 'persuasive'],
  check: ['grammar'],
  more: ['translate', 'summarize', 'continue'],
  persona: ['persona'],
}
export const tabOf = (actionId: string): Tab => (Object.keys(TAB_ACTIONS) as Tab[]).find((t) => TAB_ACTIONS[t].includes(actionId)) ?? 'rewrite'
export const actionById = (id: string): WriteAction => ACTIONS.find((a) => a.id === id) ?? ACTIONS[0]

// ---------- 帮我回：对方是谁 × 我的意思 × 怎么说 ----------

export interface Opt {
  id: string
  name: string
}
export const RELATIONS: Opt[] = [
  { id: 'superior', name: '上级' },
  { id: 'colleague', name: '同事' },
  { id: 'client', name: '客户' },
  { id: 'friend', name: '朋友' },
  { id: 'family', name: '家人' },
  { id: 'stranger', name: '不太熟' },
]
export const INTENTS: Opt[] = [
  { id: 'agree', name: '答应' },
  { id: 'conditional', name: '有条件地答应' },
  { id: 'later', name: '再想想' },
  { id: 'ask', name: '问清楚' },
  { id: 'decline', name: '拒绝' },
]
/** “怎么说”：帮我回和改写润色共用（后端 tone_line 认这些 id） */
export const TONES: Opt[] = [
  { id: 'proper', name: '正式得体' },
  { id: 'eq', name: '高情商' },
  { id: 'banter', name: '幽默接话' },
  { id: 'brief', name: '简短干脆' },
  { id: 'heartfelt', name: '温暖真诚' },
]
export const LENGTHS: Opt[] = [
  { id: 'short', name: '一句话' },
  { id: 'medium', name: '正常' },
  { id: 'long', name: '详细' },
]
export const TWEAKS: Opt[] = [
  { id: 'softer', name: '更软一点' },
  { id: 'firmer', name: '更硬一点' },
  { id: 'shorter', name: '再短点' },
  { id: 'plain', name: '去掉客套' },
  { id: 'emoji', name: '加个表情' },
]
export const WHEN = ['今晚', '明天', '这周内']
export const ALTS: Opt[] = [
  { id: '', name: '不给' },
  { id: 'another_time', name: '改天' },
  { id: 'someone_else', name: '推荐别人' },
]

/** 聊天和邮件程序：快捷键从这些程序取到文字时直接进“帮我回”。键是进程名（小写）。 */
const APPS: Record<string, { name: string; email?: boolean }> = {
  'weixin.exe': { name: '微信' },
  'wechat.exe': { name: '微信' },
  'wechatapp.exe': { name: '微信' },
  'qq.exe': { name: 'QQ' },
  'qqnt.exe': { name: 'QQ' },
  'tim.exe': { name: 'TIM' },
  'dingtalk.exe': { name: '钉钉' },
  'feishu.exe': { name: '飞书' },
  'lark.exe': { name: '飞书' },
  'wxwork.exe': { name: '企业微信' },
  'telegram.exe': { name: 'Telegram' },
  'slack.exe': { name: 'Slack' },
  'teams.exe': { name: 'Teams' },
  'ms-teams.exe': { name: 'Teams' },
  'whatsapp.exe': { name: 'WhatsApp' },
  'outlook.exe': { name: 'Outlook', email: true },
  'olk.exe': { name: 'Outlook', email: true },
  'foxmail.exe': { name: 'Foxmail', email: true },
  'thunderbird.exe': { name: 'Thunderbird', email: true },
  'hxoutlook.exe': { name: '邮件', email: true },
  'mailmaster.exe': { name: '网易邮箱大师', email: true },
}
export function appInfo(exe: string | undefined): { name: string; chat: boolean; email: boolean } {
  const k = (exe ?? '').trim().toLowerCase()
  const a = APPS[k] ?? APPS[k.split(/[\\/]/).pop() ?? '']
  if (a) return { name: a.name, chat: true, email: !!a.email }
  return { name: k.replace(/\.exe$/, ''), chat: false, email: false }
}
/** 记住“对方是谁”用的键：程序 + 对方称呼（不知道称呼时只有程序）。 */
export const relationKey = (exe: string | undefined, who: string) => [(exe ?? '').trim().toLowerCase(), who.trim()].filter(Boolean).join('|')

export interface ReplyForm {
  relation: string
  intent: string
  /** 有条件：条件；再想想：什么时候答复；拒绝：理由 */
  detail: string
  alt: string
  questions: { text: string; on: boolean }[]
  context: string
  tone: string
  length: string
  /** 还想说 */
  extra: string
}
export const newReplyForm = (): ReplyForm => ({ relation: '', intent: '', detail: '', alt: '', questions: [], context: '', tone: 'eq', length: 'medium', extra: '' })

/** 每种意思下面那个小框：标签、提示、是否必填。没有小框返回 null。 */
export function detailField(intent: string): { label: string; placeholder: string; required: boolean } | null {
  switch (intent) {
    case 'conditional':
      return { label: '条件', placeholder: '比如：下周一调休一天', required: true }
    case 'later':
      return { label: '答复时间', placeholder: '什么时候给对方答复', required: false }
    case 'decline':
      return { label: '理由', placeholder: '可不说；不写就不编理由', required: false }
    default:
      return null
  }
}

/** 还差什么才能生成；可以生成返回空字符串。 */
export function replyMissing(f: ReplyForm, text: string): string {
  if (!text.trim()) return '先把收到的消息放进来'
  if (f.intent === 'conditional' && !f.detail.trim()) return '写上条件'
  if (f.intent === 'ask' && !f.questions.some((q) => q.on && q.text.trim())) return '勾选或写上要问的问题'
  return ''
}

/** 上级、客户面前开玩笑可能不合适：只提醒，不拦。 */
export const humorRisk = (f: Pick<ReplyForm, 'relation' | 'tone'>) => f.tone === 'banter' && ['superior', 'client'].includes(f.relation)

export function buildReplyRequest(f: ReplyForm, text: string, exe?: string) {
  const info = appInfo(exe)
  const detail = ['conditional', 'later', 'decline'].includes(f.intent) ? f.detail.trim() : ''
  return {
    kind: 'write',
    text,
    write: {
      action: 'reply',
      // 一次三版：稳妥 / 热情 / 简短
      candidates: 3,
      tone: f.tone || 'auto',
      length: f.length || 'auto',
      extra: f.extra.trim(),
      page: exe ?? '',
      reply: {
        relation: f.relation,
        intent: f.intent,
        detail,
        alt: f.intent === 'decline' ? f.alt : '',
        questions: f.intent === 'ask' ? f.questions.filter((q) => q.on && q.text.trim()).map((q) => q.text.trim()) : [],
        context: f.context.trim(),
        email: info.email,
      },
    },
  }
}

export function buildTweakRequest(tweak: string, reply: string, received: string, exe?: string) {
  return { kind: 'write', text: reply, write: { action: 'tweak', page: exe ?? '', reply: { tweak, received: received.trim().slice(0, 4000), email: appInfo(exe).email } } }
}

export const buildReadRequest = (text: string, context: string) => ({ kind: 'replyread', text, write: { action: 'reply', reply: { context: context.trim() } } })

export interface ReplyRead {
  ask: string
  facts: string
  unclear: string[]
  relation: string
  who: string
}
/** “看懂了”那一行：要什么（加粗）+ 细节和语气。 */
export function readParts(r: ReplyRead | null): { ask: string; rest: string } | null {
  if (!r || (!r.ask && !r.facts)) return null
  return { ask: r.ask, rest: r.facts ? `${r.ask ? '，' : ''}${r.facts}。` : '。' }
}
