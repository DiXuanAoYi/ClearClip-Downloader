import type { ToolboxTab } from '../stores/app'
import type { FrameOp, NormSpec, ToolOp } from '../types'

/** 预览档位：实时（改参数立刻重画一帧）/ 试一小段（处理几秒再对比）/ 不需要 / 自带（页面已有播放或监视器）。 */
export type PreviewTier = 'live' | 'trial' | 'none' | 'own'

export interface ToolDef {
  id: string
  name: string
  group: string
  /** 对应的页面（工具组件） */
  tab: ToolboxTab
  /** 视频与音频页里的子工具 */
  tool?: string
  /** AI 页的模式 */
  aiMode?: 'transcribe' | 'summarize' | 'correct'
  /** 视频规整页的区块 */
  section?: 'normalize' | 'stabilize'
  /** 名字右边的小字 */
  tag?: string
  preview: PreviewTier
  keywords: string
  desc: string
}

export interface ToolGroup {
  id: string
  name: string
  color: string
}

export const GROUPS: ToolGroup[] = [
  { id: 'cut', name: '剪辑', color: '#3b6cf6' },
  { id: 'encode', name: '压制与转换', color: '#e8743b' },
  { id: 'picture', name: '画面与调色', color: '#9b5de5' },
  { id: 'sound', name: '声音', color: '#14a38b' },
  { id: 'ai', name: '字幕与 AI', color: '#d9488f' },
]

export const TOOLS: ToolDef[] = [
  { id: 'edit', name: '剪辑（时间线）', group: 'cut', tab: 'edit', tag: '多轨', preview: 'own', keywords: 'jianji timeline 时间线 多轨 转场 文字 edit cut clip timeline multitrack transition text', desc: '' },
  { id: 'quick', name: '快速处理', group: 'cut', tab: 'video', tool: 'trim', tag: '裁剪 拼接', preview: 'live', keywords: 'kuaisu 裁剪 拼接 倍速 旋转 翻转 去除音轨 trim concat speed rotate mute quick cut merge join flip', desc: '' },
  { id: 'gif', name: '转 GIF', group: 'cut', tab: 'video', tool: 'gif', preview: 'trial', keywords: 'gif dongtu 动图 animated', desc: '' },
  { id: 'frame', name: '截图', group: 'cut', tab: 'video', tool: 'frame', preview: 'live', keywords: 'jietu screenshot frame 截取 帧 capture snapshot', desc: '' },
  { id: 'compress', name: '压缩视频', group: 'encode', tab: 'video', tool: 'compress', preview: 'trial', keywords: 'yasuo compress 体积 缩小 码率 shrink size bitrate smaller', desc: '' },
  { id: 'convert', name: '转换格式', group: 'encode', tab: 'video', tool: 'convert', preview: 'none', keywords: 'zhuanhuan convert mp4 mkv mp3 提取音频 封装 format extract audio remux', desc: '' },
  { id: 'landscape', name: '竖屏转横屏', group: 'encode', tab: 'video', tool: 'landscape', preview: 'live', keywords: 'shuping hengping 竖屏 横屏 16:9 模糊背景 portrait landscape vertical horizontal blur background', desc: '' },
  { id: 'normalize', name: '视频规整', group: 'picture', tab: 'normalize', section: 'normalize', preview: 'live', keywords: 'guizheng 规整 调色 降噪 hdr 帧率 色阶 统一 normalize denoise fps color unify', desc: '' },
  { id: 'lut', name: 'LUT 工作室', group: 'picture', tab: 'lut', preview: 'live', keywords: 'lut tiaose 调色 滤镜 cube color grading filter', desc: '' },
  { id: 'stabilize', name: '防抖', group: 'picture', tab: 'normalize', section: 'stabilize', preview: 'trial', keywords: 'fangdou 防抖 稳定 抖动 stabilize shake', desc: '' },
  { id: 'loudness', name: '响度标准化', group: 'sound', tab: 'video', tool: 'loudness', preview: 'trial', keywords: 'xiangdu loudness lufs 音量 标准化 响度 volume level', desc: '' },
  { id: 'audiofx', name: '音效 / 混响', group: 'sound', tab: 'video', tool: 'audiofx', preview: 'trial', keywords: 'yinxiao hunxiang 混响 降噪 人声 reverb effects denoise vocal', desc: '' },
  { id: 'tags', name: '音频标签', group: 'sound', tab: 'tags', preview: 'none', keywords: 'biaoqian tag 标签 封面 歌手 专辑 id3 cover artist album metadata', desc: '' },
  { id: 'subtitle', name: '字幕校对与调整', group: 'ai', tab: 'subtitle', tag: '8 项', preview: 'own', keywords: 'zimu subtitle 字幕 校对 平移 校准 合并 双语 弹幕 srt vtt ass proofread shift sync merge bilingual', desc: '' },
  { id: 'transcribe', name: '生成字幕', group: 'ai', tab: 'ai', aiMode: 'transcribe', preview: 'none', keywords: 'shengcheng zimu 生成字幕 语音识别 转写 whisper transcribe speech recognition generate subtitles', desc: '' },
  { id: 'correct', name: 'AI 纠错', group: 'ai', tab: 'ai', aiMode: 'correct', preview: 'none', keywords: 'jiucuo 纠错 标点 同音字 ai correct proofread punctuation', desc: '' },
  { id: 'summarize', name: '摘要与章节', group: 'ai', tab: 'ai', aiMode: 'summarize', preview: 'none', keywords: 'zhaiyao 摘要 章节 总结 chapters summary summarize chapters', desc: '' },
]

/** 视频与音频页里，归到“快速处理”的子工具。 */
export const QUICK_TOOLS = ['trim', 'concat', 'speed', 'rotate', 'mute']

export const byId = (id: string): ToolDef => TOOLS.find((t) => t.id === id) ?? TOOLS[1]

/** 旧的入口（媒体库“用工具箱处理”等）传来的 tab / tool / aiMode → 新的工具。 */
export function idFor(tab?: ToolboxTab, tool?: string, aiMode?: string, section?: string): string {
  switch (tab ?? 'video') {
    case 'edit':
      return 'edit'
    case 'lut':
      return 'lut'
    case 'subtitle':
      return 'subtitle'
    case 'tags':
      return 'tags'
    case 'normalize':
      return section === 'stabilize' ? 'stabilize' : 'normalize'
    case 'ai':
      return TOOLS.find((t) => t.tab === 'ai' && t.aiMode === (aiMode ?? 'transcribe'))?.id ?? 'transcribe'
    default: {
      if (!tool) return 'compress'
      if (QUICK_TOOLS.includes(tool)) return 'quick'
      return TOOLS.find((t) => t.tab === 'video' && t.tool === tool)?.id ?? 'compress'
    }
  }
}

/** 工具箱首页的四张快捷卡：剪辑、截图、转 GIF、压缩视频。 */
export const QUICK_CARDS: { id: string; sub: string; icon: string }[] = [
  { id: 'edit', sub: '时间线剪辑', icon: 'M3 6h18v12H3zM8 6v12M16 6v12' },
  { id: 'frame', sub: '从视频截取画面', icon: 'M4 8h3l2-3h6l2 3h3v11H4zM12 16a3 3 0 1 0 0-6 3 3 0 0 0 0 6z' },
  { id: 'gif', sub: '片段转动图', icon: 'M4 6h16v12H4zM9 10v4M9 10h2M9 12h1.5M14 10v4' },
  { id: 'compress', sub: '缩小体积', icon: 'M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5' },
]

export function matches(t: ToolDef, q: string): boolean {
  const s = q.trim().toLowerCase()
  if (!s) return true
  return t.name.toLowerCase().includes(s) || t.keywords.toLowerCase().includes(s)
}

// ---------- 文件篮：按类型挑出工具能处理的文件 ----------

export type FileKind = 'video' | 'audio' | 'image' | 'subtitle' | 'other'
const EXTS: Record<Exclude<FileKind, 'other'>, string[]> = {
  video: ['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'ts', 'm4v', 'wmv', 'mpg', 'mpeg', '3gp'],
  audio: ['mp3', 'm4a', 'flac', 'opus', 'ogg', 'wav', 'aac', 'wma'],
  image: ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif', 'tif', 'tiff'],
  subtitle: ['srt', 'vtt', 'ass', 'ssa', 'xml'],
}

export function kindOf(path: string): FileKind {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  for (const k of Object.keys(EXTS) as (keyof typeof EXTS)[]) if (EXTS[k].includes(ext)) return k
  return 'other'
}

/** 从文件篮里挑出属于这些类型的文件，顺序不变。 */
export function pickKinds(paths: string[], kinds: FileKind[]): string[] {
  return paths.filter((p) => kinds.includes(kindOf(p)))
}

// ---------- 最近用过 ----------

const RECENT_KEY = 'clearclip.toolbox.recent'

export function loadRecent(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]')
    return Array.isArray(v) ? v.filter((id) => typeof id === 'string' && TOOLS.some((t) => t.id === id)).slice(0, 3) : []
  } catch {
    return []
  }
}

export function pushRecent(id: string): string[] {
  const next = [id, ...loadRecent().filter((x) => x !== id)].slice(0, 3)
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next))
  } catch {
    /* 存不了就算了，只是少记一次 */
  }
  return next
}

// ---------- 预览 ----------

/** 工具把当前参数交给预览区。预览区按种类决定怎么画。 */
export type PreviewSpec =
  /** 实时：取一帧，做旋转 / 竖转横，前后对比 */
  | { kind: 'frame'; op: FrameOp }
  /** 实时：视频规整的调色、降噪等（后端已有的单帧对比） */
  | { kind: 'normalize'; spec: NormSpec }
  /** 实时：就是这个时间点的画面（截图） */
  | { kind: 'still'; atMs: number }
  /** 实时：裁剪的开始和结束两帧 */
  | { kind: 'trim'; startMs: number; endMs: number | null }
  /** 试一小段：压缩 / 倍速 / 转 GIF；startMs 给了就用它，否则用预览区的时间点 */
  | { kind: 'trial'; op: ToolOp; startMs?: number; durMs?: number }
  /** 试一小段：响度标准化，听前后 */
  | { kind: 'loudness'; target: number; tp?: number; lra?: number }

export const PREVIEW_KEY = 'toolPreview'

/** 每个工具能处理的文件类型（文件篮里其他类型的文件会淡显并跳过）。 */
export const ACCEPTS: Record<string, FileKind[]> = {
  quick: ['video', 'audio'],
  gif: ['video'],
  frame: ['video'],
  compress: ['video'],
  convert: ['video', 'audio'],
  landscape: ['video'],
  normalize: ['video'],
  stabilize: ['video'],
  lut: ['image', 'video'],
  loudness: ['video', 'audio'],
  audiofx: ['video', 'audio'],
  tags: ['audio'],
  subtitle: ['subtitle'],
  transcribe: ['video', 'audio'],
  correct: ['subtitle'],
  summarize: ['subtitle'],
}
