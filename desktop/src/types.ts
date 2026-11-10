// 与 Rust 端 model.rs / download.rs / db.rs / settings.rs 的结构一一对应。

export type MediaKind = 'video' | 'images' | 'audio' | 'file' | 'playlist'
export type Protocol = 'http' | 'hls' | 'ytdlp' | 'ftp' | 'sftp' | 'stream' | 'ed2k' | 'bt'
export type AssetKind = 'video' | 'image' | 'audio' | 'cover' | 'subtitle' | 'file'

export interface Asset {
  id: string
  kind: AssetKind
  url: string
  label: string
  ext: string
  width: number | null
  height: number | null
  index: number | null
  protocol: Protocol
  headers: [string, string][]
  quality: string | null
  vcodec: string | null
  acodec: string | null
  bitrate: number | null
  filesize: number | null
  fps: number | null
  hasAudio: boolean | null
  pairAudio: string | null
  formatId: string | null
  /** 期望的文件哈希，如 sha256:…；下载完成后校验 */
  checksum?: string | null
  extra: Record<string, unknown> | null
}

export interface PlaylistEntry {
  id: string
  title: string
  url: string
  durationMs: number | null
  thumbnail: string | null
  index: number
}

export interface SeriesInfo {
  name: string
  season: number | null
  episode: number | null
}

export interface MediaInfo {
  platform: string
  platformName: string
  id: string
  sourceUrl: string
  title: string
  author: string
  cover: string | null
  durationMs: number | null
  kind: MediaKind
  width: number | null
  height: number | null
  publishedAt: number | null
  assets: Asset[]
  entries: PlaylistEntry[]
  series: SeriesInfo | null
  extractor: string | null
  chapters: Chapter[]
}

export interface Chapter {
  title: string
  startMs: number
  endMs: number
}

/** 只下载 / 保留的时间段 */
export interface Clip {
  startMs: number
  endMs: number | null
  /** 重新编码，起点精确到帧 */
  precise: boolean
}

export type TaskStatus = 'queued' | 'running' | 'paused' | 'done' | 'failed' | 'canceled'

export type ErrorKind =
  | 'need_login'
  | 'geo_blocked'
  | 'not_found'
  | 'encrypted'
  | 'rate_limited'
  | 'network'
  | 'parser_broken'
  | 'unsupported'
  | 'need_update'
  | 'disk'
  | 'invalid'
  | 'other'

/** Rust 端 AppError 的序列化形式。 */
export interface AppErrorPayload {
  kind: ErrorKind
  message: string
}

export interface TaskSnapshot {
  id: number
  platform: string
  platformName: string
  /** 登录用的网站（内置平台 ID 或域名） */
  site: string
  mediaId: string
  title: string
  author: string
  cover: string | null
  assetId: string
  assetLabel: string
  assetKind: AssetKind
  filePath: string
  status: TaskStatus
  received: number
  total: number | null
  speed: number
  error: string | null
  errorKind: ErrorKind | null
  note: string | null
  /** 下载成功但有附带问题（如字幕没有处理成功） */
  warning: string | null
  resumable: boolean | null
  step: 'download' | 'merge' | 'post' | null
  inputs: number
  priority: number
  startAt: number | null
  createdAt: number
  finishedAt: number | null
  batchId?: string | null
  batchSeq?: number | null
}

export interface HistoryItem {
  id: number
  platform: string
  mediaId: string
  title: string
  author: string
  cover: string | null
  kind: MediaKind
  sourceUrl: string
  createdAt: number
  info: MediaInfo
}

export interface LibraryItem {
  id: number
  platform: string
  mediaId: string
  assetId: string
  title: string
  author: string
  cover: string | null
  path: string
  size: number
  finishedAt: number
  exists: boolean
  kind: string
  source: string
  sourceUrl: string
  platformName: string
  coverPath: string | null
  favorite: boolean
  /** 0–5，0 表示未评分 */
  rating: number
  note: string
  tags: string[]
  durationMs: number | null
}

export interface LibraryFilter {
  query: string
  platform: string | null
  kind: string | null
  source: string | null
  since: number | null
  missingOnly: boolean
  tag: string | null
  favoriteOnly: boolean
  minRating: number
  sort: 'finished' | 'size' | 'title' | 'rating' | null
}

export interface TagCount {
  name: string
  count: number
}

export interface Bucket {
  key: string
  name: string
  count: number
  size: number
}

export interface LibraryStats {
  count: number
  totalSize: number
  missing: number
  byKind: Bucket[]
  byPlatform: Bucket[]
  byAuthor: Bucket[]
  byMonth: Bucket[]
  largest: { id: number; title: string; path: string; size: number; exists: boolean }[]
}

export interface TrashItem {
  id: number
  title: string
  originalPath: string
  size: number
  deletedAt: number
}

export interface CueHit {
  itemId: number | null
  title: string
  path: string
  lang: string
  startMs: number
  text: string
}

export interface MovePlan {
  id: number
  from: string
  to: string
}

export interface MoveReport {
  moved: number
  skipped: string[]
}

export interface ImportReport {
  added: number
  subtitles: number
  skipped: number
}

export interface DupGroup {
  kind: 'exact' | 'similar'
  items: LibraryItem[]
}

export interface DeleteReport {
  removed: number
  trashed: number
  failed: string[]
}

export interface SceneList {
  durationMs: number
  scenes: Chapter[]
}

export interface BackupInfo {
  appVersion: string
  createdAt: number
  size: number
}

export interface DanmakuStyle {
  fontSize: number
  opacity: number
  scrollSecs: number
  area: number
  font: string
}

export type ToolOp =
  | { op: 'compress'; quality: 'small' | 'balanced' | 'high'; maxHeight: number | null }
  | { op: 'gif'; startMs: number; durationMs: number; width: number; fps: number }
  | { op: 'landscape'; width: number; height: number; mode: 'blur' | 'black' }
  | { op: 'speed'; factor: number }
  | { op: 'loudness'; target: number; tp?: number; lra?: number }
  | { op: 'audio_fx'; spec: AudioFxSpec }
  | { op: 'rotate'; mode: 'cw' | 'ccw' | 'flip180' | 'hflip' | 'vflip' }
  | { op: 'convert'; format: string; reencode: boolean }
  | { op: 'frame'; atMs: number; format: 'jpg' | 'png' }
  | { op: 'frames'; everySecs: number; format: 'jpg' | 'png' }
  | { op: 'concat'; reencode: boolean }
  | { op: 'trim'; startMs: number; endMs: number | null; precise: boolean }
  | { op: 'mute' }
  | { op: 'chapters'; chapters: Chapter[] }
  | { op: 'normalize'; spec: NormSpec; preset?: string | null }
  | { op: 'stabilize'; strength: 'light' | 'normal' | 'strong' }
  | { op: 'edit'; project: EditProject }
  | { op: 'tags'; title?: string | null; artist?: string | null; album?: string | null; year?: string | null; genre?: string | null; comment?: string | null; cover?: string | null }

// ---------- 音效 / 混响 ----------

export interface ReverbSpec {
  /** 混响相对干声的电平（dB），0 约等于混响和原声一样响 */
  mixDb: number
  timeMs: number
  /** 高频衰减得比低频快多少：1 = 一样，越小越“闷” */
  hfRatio: number
  preDelayMs: number
  inGainDb: number
}

export interface AudioFxSpec {
  denoise: 'off' | 'light' | 'medium' | 'strong'
  enhance: boolean
  reverb: ReverbSpec | null
  /** 目标响度（LUFS）；null 表示不做响度标准化 */
  normalize: number | null
  /** 真峰值上限（dBTP），不填用 -1.5 */
  normalizeTp?: number | null
  /** 响度范围目标（LU），不填用 11 */
  normalizeLra?: number | null
  preset?: string | null
}

export interface AudioFxPreview {
  before: string
  after: string
  notes: string[]
}

export interface TimeSpan {
  startMs: number
  endMs: number
}

export interface ReframeResult {
  region: EditRegion
  note: string | null
  tracked: boolean
}

export type ToolJob = ToolOp & { inputs: string[]; outputDir?: string | null }

// ---------- 视频规整 ----------

export interface NormSpec {
  size: 'keep' | 'limit' | 'fit' | 'blur' | 'fill'
  width: number
  height: number
  shortSide: number
  followOrientation: boolean
  fps: 'keep' | 'auto' | 'fixed'
  fpsValue: number
  hdr: boolean
  fixColor: boolean
  levels: number
  /** 输出电平：tv 为 16–235（电视范围，游戏、美术等素材的要求，默认），pc 为 0–255，keep 沿用素材 */
  outRange: 'tv' | 'pc' | 'keep'
  /** 降噪档位 */
  denoise: 'off' | 'light' | 'medium' | 'strong' | 'best'
  /** 分段色彩匹配的强度（0 关闭）：把和整体色调不一致的镜头校正到和整体一致 */
  matchColor: number
  /** 统一调节：作用在所有没有单独调节的镜头上 */
  matchTone: ToneAdjust
  /** 单独调节的镜头（只能选检测出来和整体不一致的镜头） */
  matchShots: ShotAdjust[]
  /** 排除在外的时间段：不参与统计，也不被校正，保持原样 */
  matchSkip: SkipSpan[]
  lut: string | null
  lutStrength: number
  autocrop: boolean
  audioRate: number
  audioChannels: number
  loudness: number | null
  fixSync: boolean
  codec: 'keep' | 'h264' | 'hevc'
  quality: 'small' | 'balanced' | 'high'
}

/** 手动调节，每项在 -1 到 1 之间，0 为不动。 */
export interface ToneAdjust {
  brightness: number
  contrast: number
  saturation: number
  temperature: number
  tint: number
}

/** 单独调节的镜头：`id` 是检测结果里镜头开头的毫秒数；`strength` 为 null 时和统一的校正强度一样。 */
export interface ShotAdjust {
  id: number
  strength: number | null
  tone: ToneAdjust
}

/** 排除的一段时间（毫秒）；`endMs` 为 null 表示一直到结尾。 */
export interface SkipSpan {
  startMs: number
  endMs: number | null
}

export interface ColorTimelineSeg {
  startMs: number
  endMs: number
  off: boolean
}

export interface ColorFlaggedShot {
  id: number
  startMs: number
  endMs: number
  defects: string[]
}

/** 各个镜头的色彩是否一致的检测结果。 */
export interface ColorReport {
  durationMs: number
  shotsTotal: number
  timeline: ColorTimelineSeg[]
  flagged: ColorFlaggedShot[]
  /** 排除的时间段（结尾已补成视频时长）和因此没有参与统计的镜头数 */
  skipped: { startMs: number; endMs: number }[]
  shotsExcluded: number
  note: string | null
}

export interface NormPreset {
  id: string
  name: string
  desc: string
  spec: NormSpec
}

export interface VideoStreamFacts {
  codec: string
  width: number
  height: number
  pixFmt: string
  bitDepth: number
  range: string | null
  matrix: string | null
  primaries: string | null
  transfer: string | null
  fps: number | null
  tbr: number | null
  rotation: number
  sar: [number, number] | null
  bitrateKbps: number | null
  hdr: 'none' | 'pq' | 'hlg'
  dolbyVision: boolean
  dolbyProfile: number | null
}

export interface AudioStreamFacts {
  codec: string
  sampleRate: number
  channels: number
  bitrateKbps: number | null
}

export interface VideoFactsInfo {
  durationMs: number | null
  container: string
  video: VideoStreamFacts | null
  audio: AudioStreamFacts | null
}

export interface VideoAnalysis {
  vfr: { variable: boolean; medianFps: number; minFps: number; maxFps: number; frames: number } | null
  crop: { w: number; h: number; x: number; y: number; srcW: number; srcH: number } | null
  loudness: { inputI: number; inputTp: number; inputLra: number; inputThresh: number; targetOffset: number } | null
}

export interface VideoIssue {
  id: string
  label: string
  detail: string
  level: 'info' | 'warn'
}

export interface VideoReport {
  facts: VideoFactsInfo
  analysis: VideoAnalysis
  issues: VideoIssue[]
  recommended: string
}

export interface VideoPreview {
  before: string
  after: string
  notes: string[]
  warnings: string[]
  changed: boolean
}

/** 工具箱预览：取哪一帧、做什么变换 */
export type FrameOp = { kind: 'none' } | { kind: 'rotate'; mode: string } | { kind: 'landscape'; width: number; height: number; mode: string }

export interface FramePreview {
  before: string
  after: string
  changed: boolean
}

/** “试一小段”的结果 */
export interface ToolTrial {
  before: string
  after: string
  afterIsImage: boolean
  beforeBytes: number
  afterBytes: number
  secs: number
  totalSecs: number | null
  note: string | null
}

export interface CapsSummary {
  version: string
  edition: 'full' | 'lite'
  h264: string | null
  hevc: string | null
  hardware: string[]
  tonemap: boolean
  lut: boolean
  levels: boolean
  cropDetect: boolean
  stabilize: 'vidstab' | 'deshake' | 'none'
  notes: string[]
}

export interface JobSnap {
  id: number
  title: string
  op: string
  status: 'queued' | 'running' | 'done' | 'failed' | 'canceled'
  percent: number
  output: string | null
  error: string | null
  note: string | null
  finishedAt: number | null
  /** 稳定的类型编号（filetr / dictation …）；按它筛选，不要用显示用的 op */
  kind: string
  lane: 'interactive' | 'ai' | 'heavy'
  input: string | null
  params: Record<string, unknown> | null
}

export interface MediaInfoLite {
  durationMs: number | null
  width: number | null
  height: number | null
  hasVideo: boolean
  hasAudio: boolean
  tags: Record<string, string>
}

export type SubOp =
  | { op: 'shift'; offsetMs: number }
  | { op: 'rescale'; factor: number }
  | { op: 'align'; aFrom: number; aTo: number; bFrom: number; bTo: number }
  | { op: 'merge'; second: string }
  | { op: 'clean'; dropHearing: boolean }
  | { op: 'convert' }
  | { op: 'danmaku'; width?: number | null; height?: number | null }

export type SubJob = SubOp & { input: string; format?: string | null }

export interface SiteRule {
  name: string
  enabled: boolean
  pattern: string
  videoRegex: string
  titleRegex: string
  coverRegex: string
  referer: string
  userAgent: string
}

export interface NotifyChannel {
  id: string
  name: string
  enabled: boolean
  kind: 'webhook' | 'telegram' | 'bark' | 'serverchan' | 'wecom' | 'dingtalk' | 'feishu' | 'ntfy'
  target: string
}

export interface NotifySettings {
  channels: NotifyChannel[]
  onDone: boolean
  onFailed: boolean
  onLive: boolean
  onSub: boolean
  onAccount: boolean
}

export interface UploadSettings {
  enabled: boolean
  kind: 'webdav' | 'folder'
  url: string
  user: string
  remoteDir: string
  kinds: string[]
  deleteAfter: boolean
}

export interface RuleWhen {
  platform: string
  author: string
  title: string
  kind: string
  source: string
  minSizeMb: number
}

export interface RuleThen {
  addTags: string[]
  favorite: boolean
  moveTo: string
  extractAudio: string
  upload: boolean
  notify: boolean
  normalize: string
}

export interface AutoRule {
  id: string
  name: string
  enabled: boolean
  when: RuleWhen
  then: RuleThen
}

export interface GlossaryTerm {
  source: string
  target: string
  note: string
  /** 下级术语（分层，最多 6 层） */
  children?: GlossaryTerm[]
  /** 只作分组：不当术语用，下级照常用 */
  group?: boolean
}

export interface Glossary {
  id: string
  name: string
  description: string
  enabled: boolean
  origin: 'user' | 'community' | 'extracted'
  remoteId: string
  matches: string[]
  lang: string
  terms: GlossaryTerm[]
  updated: number
  /** 所在文件夹，多层用 / 分隔；空 = 最外层 */
  folder: string
}

export interface GlossaryCandidate {
  id: string
  source: string
  target: string
  note: string
  site: string
  count: number
  found: number
}

export interface ExtractSite {
  pattern: string
  enabled: boolean
  glossary: string
  last: number
  found: number
}

export interface GlossaryData {
  glossaries: Glossary[]
  candidates: GlossaryCandidate[]
  sites: ExtractSite[]
  /** 建了还没放术语库的文件夹 */
  folders: string[]
}

export interface CommunityGlossary {
  id: string
  name: string
  description: string
  author: string
  matches: string[]
  langs: string[]
}

export interface PoolEntry {
  id: string
  preset: string
  /** '' / openai = 对话模型；google / microsoft / deepl / deeplx = 机器翻译 */
  kind: string
  name: string
  baseUrl: string
  model: string
  sttModel: string
  enabled: boolean
  tasks: string[]
  priority: number
  dailyLimit: number
  /** 每月字符上限（0 = 不限），机器翻译用 */
  monthlyChars: number
}

export interface PoolPreset {
  id: string
  kind: string
  name: string
  baseUrl: string
  model: string
  signupUrl: string
  keyUrl: string
  note: string
  free: boolean
  local: boolean
  tasks: string[]
}

export interface PoolStatus {
  id: string
  hasKey: boolean
  used: number
  ok: number
  fail: number
  coolingSecs: number
  coolReason: string
  badKey: boolean
  lastError: string
  charsUsed: number
}

export interface AiSettings {
  pool: PoolEntry[]
  supadataOn: boolean
  glossaryOn: boolean
  expert: string
  expertAuto: boolean
  baseUrl: string
  model: string
  targetLang: string
  bilingual: boolean
  batchSize: number
  sttEngine: 'api' | 'local'
  sttBaseUrl: string
  sttModel: string
  sttLanguage: string
  whisperBin: string
  whisperModel: string
  autoTranslate: boolean
  autoTranscribe: boolean
  correctMode: 'conservative' | 'aggressive' | 'rules'
  correctAuto: boolean
  whisperVadModel: string
  /** 语音转文字的字幕句长 */
  cueLength: 'short' | 'standard' | 'long'
  /** 仍然过长的句子用 AI 按语义断句 */
  cueSemantic: boolean
  /** 本机识别用哪个模型："" 自动 | whisper_tiny | whisper_base | whisper_small | sensevoice */
  sttLocalModel: string
}

export interface SubgenOpts {
  language?: string
  /** off / rules / conservative / aggressive；不填 = 识别结果没标点时按设置里的力度纠错 */
  correct?: string
  resegment?: boolean
  /** 语义断句（AI），默认关 */
  semantic?: boolean
  /** 逐词时间：拆开的字幕对到真实词间停顿（仅本机 whisper.cpp），默认关 */
  wordTiming?: boolean
  /** 反思翻译（译完再检查润色），默认关 */
  reflect?: boolean
  targets: string[]
  display?: 'orig' | 'trans' | 'orig-top' | 'trans-top'
  formats: string[]
}

export interface SubTrackInfo {
  path: string
  label: string
  ext: string
  cues: number
  bilingual: boolean
  size: number
}

export interface SubIssue {
  index: number
  kind: 'reversed' | 'overlap' | 'empty' | 'too-long' | 'too-short' | 'too-fast'
  message: string
}

export interface BurnStyle {
  font: string
  size: number
  size2: number
  color: string
  color2: string
  outline: number
  shadow: number
  boxed: boolean
  bold: boolean
  margin: number
  position: 'bottom' | 'top'
}

export interface MuxOpts {
  mode: 'soft' | 'hard'
  tracks: string[]
  style?: BurnStyle | null
}

export interface AiStatus {
  chatReady: boolean
  chatKey: boolean
  sttReady: boolean
  sttKey: boolean
  whisperFound: boolean
  whisperPath?: string | null
  whisperModelPath?: string | null
}

export interface LibrarySettings {
  useTrash: boolean
  trashKeepDays: number
  reorganizeTemplate: string
  probeNewFiles: boolean
}

export interface PlatformCount {
  platform: string
  name: string
  count: number
}

export interface PairedDevice {
  id: string
  name: string
  addedAt: number
  lastSeen: number
}

export interface PhoneSettings {
  enabled: boolean
  port: number
  token: string
  devices: PairedDevice[]
  host: string
  receiveDir: string
  maxUploadMb: number
  autoExtract: boolean
  outboxHours: number
}

export interface PairRequest {
  deviceId: string
  name: string
  ip: string
  /** 6 位验证码，手机网页上显示同一个 */
  code: string
}

export interface PhoneInfo {
  enabled: boolean
  running: boolean
  port: number
  url: string | null
  apiUrl: string | null
  token: string
  qrSvg: string | null
  devices: PairedDevice[]
  pending: PairRequest[]
  error: string | null
  /** 二维码里使用的电脑地址 */
  ip: string | null
  /** 手动指定的地址（空表示自动） */
  host: string
  addresses: LanAddr[]
  /** 没有找到真实局域网网卡，当前地址可能是虚拟网卡 */
  suspicious: boolean
  receiveDir: string
  maxUploadMb: number
  autoExtract: boolean
  outboxHours: number
}

export interface LanAddr {
  ip: string
  name: string
  likely: boolean
}

export interface PhoneOptions {
  host: string
  receiveDir: string
  maxUploadMb: number
  autoExtract: boolean
  outboxHours: number
}

export type TransferKind = 'text' | 'image' | 'video' | 'audio' | 'archive' | 'file'

export interface TransferItem {
  id: number
  /** in：手机发给电脑；out：电脑发给手机 */
  direction: 'in' | 'out'
  kind: TransferKind
  name: string
  path: string
  text: string
  size: number
  done: number
  status: 'receiving' | 'done' | 'failed' | 'ready'
  message: string | null
  extra: string | null
  deviceName: string
  downloads: number
  createdAt: number
  expiresAt: number
  active: boolean
  missing: boolean
}

export interface TransferList {
  incoming: TransferItem[]
  outgoing: TransferItem[]
  receiveDir: string
}

export interface TransferProgress {
  id: number
  done: number
  total: number
}

export interface TransferReceived {
  id: number
  name: string
  kind: TransferKind
}

export interface HealthResult {
  platform: string
  name: string
  sample: string | null
  ok: boolean
  millis: number
  message: string
  kind: ErrorKind | null
}

export type ParseMode = 'local' | 'remote' | 'local_then_remote'

export type Route = { kind: 'direct' } | { kind: 'system' } | { kind: 'proxy'; id: string }

export interface RouteRule {
  pattern: string
  route: Route
}

export interface ProxyDef {
  id: string
  name: string
  url: string
}

export interface NetworkSettings {
  defaultRoute: Route
  rules: RouteRule[]
  proxies: ProxyDef[]
}

export type ConflictPolicy = 'rename' | 'skip' | 'overwrite'

export interface RouteSpeed {
  route: Route
  name: string
  ok: boolean
  ttfbMs: number
  kbps: number
  bytes: number
  error: string | null
}

export interface SpeedWindow {
  days: number[]
  start: string
  end: string
  /** KB/s，0 表示不限 */
  limitKbps: number
}

export interface RouteTest {
  route: Route
  status: number
  millis: number
}

export interface AccountStatus {
  loggedIn: boolean
  userName: string | null
  vip: string | null
}

export interface Settings {
  downloadDir: string
  subfolderByPlatform: boolean
  filenameTemplate: string
  concurrency: number
  skipExisting: boolean
  watchClipboard: boolean
  autoDownload: boolean
  notifyOnComplete: boolean
  parseMode: ParseMode
  remoteEndpoint: string
  theme: 'system' | 'dark' | 'light'
  closeToTray: boolean
  shortcut: string
  shotHotkey: string
  shotAutoCopy: boolean
  shotHistory: boolean
  checkUpdate: boolean
  disclaimerAccepted: boolean
  autoResume: boolean
  maxRetries: number
  retryDelaySecs: number
  keepPartOnCancel: boolean
  recordSamples: boolean
  network: NetworkSettings
  segments: number
  segmentMinMb: number
  speedLimitKbps: number
  perSiteConcurrency: number
  siteRequestIntervalMs: number
  tempDir: string
  conflictPolicy: ConflictPolicy
  diskReserveMb: number
  qualityPreset: QualityPreset
  preferH264: boolean
  mergeContainer: string
  audioFormat: string
  hlsConcurrency: number
  hlsSkipAds: boolean
  ed2kServers: string[]
  ed2kServerMetUrl: string
  btTrackerAutoUpdate: boolean
  btTrackerSources: string[]
  btTrackerIntervalDays: number
  btCustomTrackers: string[]
  seriesTemplate: string
  useYtdlp: boolean
  genericSniffer: boolean
  componentMirrors: string[]
  ffmpegEdition: 'lite' | 'full'
  clipboardAllSites: boolean
  wechatOn: boolean
  clipboardDomains: string[]
  phone: PhoneSettings
  embedMetadata: boolean
  writeInfoJson: boolean
  writeNfo: boolean
  openFolderOnDone: boolean
  postScriptEnabled: boolean
  postScript: string
  preventSleep: boolean
  subscriptionsEnabled: boolean
  launchAtLogin: boolean
  liveMaxRecordings: number
  liveCleanupDays: number
  inbox: InboxSettings
  subtitleLangs: string[]
  subtitleAuto: boolean
  subtitleMode: 'off' | 'file' | 'embed' | 'burn'
  subtitleConvert: boolean
  danmakuAss: boolean
  verifyDownloads: boolean
  videoCacheMb: number
  extDataMb: number
  videoCacheAuto: boolean
  library: LibrarySettings
  danmaku: DanmakuStyle
  maxSizeMb: number
  autoDowngrade: boolean
  ai: AiSettings
  customSites: SiteRule[]
  notify: NotifySettings
  upload: UploadSettings
  rules: AutoRule[]
  speedSchedule: SpeedWindow[]
  batchPreview: boolean
  batchFolderMin: number
  batchJitterMin: number
  batchJitterMax: number
  runWindowOn: boolean
  runWindow: SpeedWindow
  meteredMode: boolean
  security: SecuritySettings
  writing: WritingSettings
  /** 实时屏幕翻译 */
  live: ScreenSettings
  onboarded: boolean
  language: 'zh' | 'en' | 'ja'
  floatBall: boolean
}

/** 写作浮窗（桌面端入口）的设置 */
export interface WritingSettings {
  hotkey: string
  badge: boolean
  pausedUntil: number
  offApps: string[]
  route: 'local_first' | 'local_only' | 'cloud_only'
  candidates: number
  style: string
  pin: boolean
  autoClose: boolean
  win: number[]
  lastAction: string
  extras: string[]
  /** 帮我回记住的“对方是谁”：程序|称呼 → superior / colleague / … */
  relations?: Record<string, string>
  /** 写作记忆开关（默认关） */
  memory?: boolean
  /** 记忆：关于我的笔记 */
  memoryNotes?: string
  /** 记忆：以前采用并写回去的写法 */
  memorySamples?: { app: string; text: string }[]
}

/** 快捷键取到的内容 */
export interface WritePending {
  text: string
  fromSelection: boolean
  app: string
  canReplace: boolean
  note: string
}

export interface SecuritySettings {
  /** 空闲多少分钟后自动锁定，0 表示不自动锁定 */
  autoLockMinutes: number
  lockOnHide: boolean
  privacyMode: boolean
  contentProtection: boolean
  /** 老板键：立刻隐藏窗口并锁定；空表示不启用 */
  panicShortcut: string
  safeboxAutoLockMinutes: number
}

export interface LockStatus {
  enabled: boolean
  locked: boolean
  lockedOutSecs: number
}

export interface SafeStatus {
  exists: boolean
  unlocked: boolean
  count: number
  totalBytes: number
  lockedOutSecs: number
}

export interface SafeEntry {
  id: string
  name: string
  size: number
  addedAt: number
  kind: 'video' | 'audio' | 'image' | 'other'
  from: 'file' | 'library'
}

export interface WipeOptions {
  tasks: boolean
  history: boolean
  library: boolean
  inbox: boolean
  subscriptions: boolean
  cookies: boolean
  secrets: boolean
  thumbnails: boolean
  logs: boolean
  safebox: boolean
  quit: boolean
}

export interface WipeReport {
  done: string[]
  skipped: string[]
}

export interface TimeWindow {
  /** 1 = 周一 … 7 = 周日；空表示每天 */
  days: number[]
  start: string
  end: string
}

export interface LiveSettings {
  autoRecord: boolean
  quality: string
  checkIntervalS: number
  segmentMinutes: number
  segmentMb: number
  convertMp4: boolean
  mergeSegments: boolean
  normalize: string
  dir: string
  notify: boolean
  schedule: TimeWindow[]
}

export interface LiveStream {
  quality: string
  rank: number
  url: string
  format: 'flv' | 'hls'
}

export interface LiveStatus {
  platform: string
  platformName: string
  roomId: string
  streamer: string
  title: string
  cover: string | null
  avatar: string | null
  live: boolean
  streams: LiveStream[]
}

export interface LiveRoom {
  id: number
  url: string
  platform: string
  platformName: string
  streamer: string
  title: string
  avatar: string | null
  cover: string | null
  settings: LiveSettings
  monitoring: boolean
  createdAt: number
  state: 'offline' | 'live' | 'recording' | 'error' | 'checking'
  lastCheck: number | null
  error: string | null
  recStarted: number | null
  recBytes: number
  recFile: string | null
  recSegments: number
  qualities: string[]
}

export interface Recording {
  id: number
  liveId: number
  streamer: string
  title: string
  startedAt: number
  endedAt: number | null
  files: string[]
  size: number
  status: string
}

export interface SubSettings {
  intervalHours: number
  firstRun: 'new_only' | 'latest' | 'all'
  firstN: number
  maxAuto: number
  include: string[]
  exclude: string[]
  minDurationS: number | null
  maxDurationS: number | null
  maxAgeDays: number | null
  quality: QualityPreset | null
  dir: string
  template: string
  notify: boolean
  keepLatest: number
  keepDays: number
}

export interface Subscription {
  id: number
  url: string
  title: string
  platform: string
  platformName: string
  avatar: string | null
  settings: SubSettings
  status: 'active' | 'paused' | 'error'
  lastCheck: number | null
  nextCheck: number | null
  lastError: string | null
  failCount: number
  newCount: number
  createdAt: number
  downloaded: number
  pending: number
  ignored: number
  checking: boolean
}

export interface SubEntry {
  id: string
  title: string
  url: string
  thumbnail: string | null
  publishedAt: number | null
  durationMs: number | null
}

export interface ListResult {
  title: string
  platform: string
  platformName: string
  avatar: string | null
  entries: SubEntry[]
}

export type SubItemStatus = 'seen' | 'pending' | 'queued' | 'downloaded' | 'ignored' | 'failed'

export interface SubItem {
  itemId: string
  title: string
  url: string
  thumbnail: string | null
  publishedAt: number | null
  durationMs: number | null
  status: SubItemStatus
  reason: string | null
  createdAt: number
}

export type QualityPreset = 'best' | 'max1080' | 'small' | 'audio'

export interface PostOptions {
  extractAudio: string | null
  embedMetadata: boolean
  /** 选中的字幕：soft 内嵌为字幕轨 / burn 烧录进画面；为空时字幕单独保存为文件 */
  subMode?: 'soft' | 'burn' | null
  clip?: Clip | null
  /** 按这些章节另外拆分成多个文件 */
  splitChapters?: Chapter[]
}

export type ToolId = 'yt-dlp' | 'ffmpeg'

export interface ToolStatus {
  id: ToolId
  installed: boolean
  managed: boolean
  path: string | null
  version: string | null
  hasPrevious: boolean
  autoInstall: boolean
  note: string | null
  busy: boolean
  edition: 'full' | 'lite' | null
}

export interface ToolProgress {
  tool: ToolId
  stage: 'prepare' | 'download' | 'verify' | 'extract' | 'done'
  received: number
  total: number | null
}

export interface BatchProgress {
  title: string
  done: number
  total: number
  queued: number
  skipped: number
  failed: string[]
  finished: boolean
}

export interface SaveSettingsResult {
  settings: Settings
  shortcutError: string | null
}

export interface EnqueueResult {
  tasks: TaskSnapshot[]
  alreadyDownloaded: number
  alreadyQueued: number
}

export interface OrphanPart {
  path: string
  size: number
  modified: number
}

export interface AccountSummary {
  id: string
  site: string
  siteName: string
  label: string
  cookieCount: number
  updatedAt: number
  userName: string | null
  expiresAt: number | null
  isDefault: boolean
  checkedAt: number | null
  /** 最近一次检查结果：true 有效 / false 已失效 / null 未检查 */
  valid: boolean | null
  /** 能否检查登录状态 */
  checkable: boolean
}

export interface DetectedLink {
  url: string
  platform: string
  platformName: string
}

export interface AppInfo {
  version: string
  providers: { id: string; name: string }[]
  repo: string
  os: string
  keyInKeyring: boolean
  shortcutError: string | null
  /** 构建所对应的提交（短哈希） */
  commit: string
  /** 调试版 */
  debug: boolean
}

export interface UpdateInfo {
  current: string
  latest: string | null
  hasUpdate: boolean
  url: string
  canInstall: boolean
}

export interface ClipboardLink {
  text: string
  links: DetectedLink[]
}

export interface AutoResult {
  url: string
  ok: boolean
  message: string
}

/** 迷你窗页面刚加载好时要显示的内容（窗口是用到时才创建的，创建时发出的事件它收不到）。 */
export interface MiniState {
  link: ClipboardLink | null
  result: AutoResult | null
}

export type InboxSource = 'clipboard' | 'phone' | 'extension' | 'manual' | 'api' | 'cli'
export type InboxStatus = 'pending_pair' | 'rejected' | 'confirm' | 'playlist' | 'resolving' | 'queued' | 'downloading' | 'done' | 'failed' | 'ignored'

/** 收到的链接 */
export interface InboxItem {
  id: number
  source: InboxSource
  device: string
  deviceName: string
  text: string
  url: string
  site: string
  status: InboxStatus
  message: string | null
  errorKind: ErrorKind | null
  title: string | null
  platform: string | null
  mediaId: string | null
  taskIds: number[]
  seenBefore: number
  createdAt: number
  updatedAt: number
}

export interface InboxFilter {
  source: InboxSource | 'all' | null
  status: 'all' | 'unhandled' | 'failed' | 'done' | 'active' | 'ignored' | null
  query: string
  limit: number | null
}

export interface InboxCounts {
  unhandled: number
  total: number
}

export interface InboxSettings {
  recordClipboard: boolean
  keepDays: number
  maxItems: number
}

export interface LogLine {
  /** Unix 毫秒 */
  at: number
  text: string
}

export interface RequestInfo {
  label: string
  protocol: string
  url: string
  headers: [string, string][]
  route: string
}

export interface TaskDetail {
  task: TaskSnapshot
  sourceUrl: string
  log: LogLine[]
  requests: RequestInfo[]
  partFiles: string[]
  post: string[]
}

export interface PlayerCue {
  startMs: number
  endMs: number
  text: string
}

export interface PlayerTrack {
  label: string
  path: string
  cues: PlayerCue[]
}

export interface PlayerSource {
  path: string
  /** 播放地址（本机媒体服务） */
  url: string
  title: string
  kind: 'video' | 'audio'
  ext: string
  tracks: PlayerTrack[]
}

// ---------- LUT 工作室 ----------

export interface SplitTone {
  /** 色相 0–360 */
  hue: number
  /** 0–1，0 为不染色 */
  amount: number
}

export interface LutAdjust {
  exposure: number
  black: number
  white: number
  gamma: number
  contrast: number
  shadows: number
  highlights: number
  temperature: number
  tint: number
  saturation: number
  vibrance: number
  hue: number
  shadowTone: SplitTone
  highlightTone: SplitTone
  strength: number
}

export interface LutLayer {
  path: string
  strength: number
}

export interface LutReference {
  path: string
  /** 明暗匹配强度 0–1 */
  tone: number
  /** 色彩匹配强度 0–1 */
  color: number
}

export interface LutSample {
  path: string
  atMs: number
}

export interface LutRecipe {
  name: string
  size: 17 | 33 | 65
  base: LutLayer[]
  adjust: LutAdjust
  reference: LutReference | null
  sample: LutSample | null
}

export interface LutLook {
  id: string
  name: string
  desc: string
  adjust: LutAdjust
}

export interface LutInfo {
  title: string
  size: number
}

export interface LutSaved {
  path: string
  size: number
  bytes: number
}

// ---------- 剪辑 ----------

export interface EditTransition {
  kind: string
  durationMs: number
}

/** 轨迹上的一个点：这一时刻区域的矩形。位置是相对整个画面的比例（0–1，左上角为原点），方向是素材的显示方向（旋转翻转之前），时间是素材里的时间 */
export interface TrackPt {
  tMs: number
  x: number
  y: number
  w: number
  h: number
  /** 追踪器在这里没有把握，位置是推测的 */
  lost?: boolean
  /** 用户手动指定（校正）的点：重新追踪不会改动 */
  pin?: boolean
}

export interface NRect {
  x: number
  y: number
  w: number
  h: number
}

export type RegionEffect = 'mosaic' | 'blur' | 'tone' | 'focus'

/** 跟着物体走的区域 */
export interface EditRegion {
  id: number
  name: string
  track: TrackPt[]
  shape: 'rect' | 'ellipse'
  feather: number
  grow: number
  invert: boolean
  effect: RegionEffect
  strength: number
  brightness: number
  contrast: number
  saturation: number
  zoom: number
  reframe: boolean
  smooth: number
  startMs: number | null
  endMs: number | null
}

export interface TrackCandidate {
  rect: NRect
  score: number
}

export interface TrackDetect {
  candidates: TrackCandidate[]
  /** 镜头在动的程度 */
  camera: number
  note: string | null
}

export interface TrackResult {
  points: TrackPt[]
  lostMs: number
  note: string | null
}

export interface EditClip {
  id: number
  path: string
  kind: 'video' | 'image'
  inMs: number
  outMs: number
  speed: number
  volume: number
  mute: boolean
  fadeInMs: number
  fadeOutMs: number
  rotate: 0 | 90 | 180 | 270
  flipH: boolean
  flipV: boolean
  brightness: number
  contrast: number
  saturation: number
  /** 声音处理：降噪（关 / 轻 / 中 / 强）、人声增强；底噪电平是实测的（dBFS），降噪按它设置 */
  denoise: 'off' | 'light' | 'medium' | 'strong'
  enhance: boolean
  noiseFloorDb: number | null
  /** 倒放（声音也倒过来）；定格：只取入点处那一帧，停住 `outMs - inMs` 毫秒，没有声音 */
  reverse: boolean
  freeze: boolean
  regions: EditRegion[]
  /** 关键帧：zoom / panX / panY（时间相对片段开头） */
  keyframes: EditKeyframes
  bg: EditBackground
  transition: EditTransition | null
  /** 套一张 `.cube`（空 = 不套）和它的强度 0–1 */
  lut: string
  lutMix: number
  /** 混响（和工具箱“音效 / 混响”同一套参数）；null = 不加 */
  reverb: ReverbSpec | null
  /** 防抖强度；导出时才做（预览里不做） */
  stabilize: 'off' | 'light' | 'normal' | 'strong'
}

/** 关键帧：某个属性在某个时刻（毫秒，相对素材 / 文字自己的开始）的值，`ease` 是到下一个关键帧之间用的曲线 */
export type KeyEase = 'linear' | 'in' | 'out' | 'inout' | 'hold'
export interface EditKeyframe {
  tMs: number
  value: number
  ease: KeyEase
}
/** 属性名（x / y / scale / rotate / opacity / volume）→ 关键帧列表 */
export type EditKeyframes = Record<string, EditKeyframe[]>

export interface EditText {
  id: number
  text: string
  startMs: number
  endMs: number
  x: number
  y: number
  size: number
  color: string
  opacity: number
  outline: boolean
  outlineColor: string
  boxed: boolean
  boxColor: string
  boxOpacity: number
  font: string | null
  /** 逐字高亮：每个词相对这条文字出现时间的起止（毫秒）；词接起来就是 text */
  words: EditTextWord[]
  /** 说到的词变成这个颜色；空表示不高亮 */
  highlight: string
  /** 淡入淡出（毫秒） */
  fadeMs: number
  /** 关键帧：x / y / opacity；逐字高亮的文字不支持 */
  keyframes: EditKeyframes
}

export interface EditTextWord {
  t: string
  a: number
  b: number
}

/** 文稿里的一个词，时间是素材自己的时间（毫秒） */
export interface TranscriptWord {
  t: string
  startMs: number
  endMs: number
}

export interface Transcript {
  words: TranscriptWord[]
  /** false：接口 / 引擎只给了整句时间，词是按字数摊出来的 */
  wordLevel: boolean
  engine: string
  fromMs: number
  toMs: number
}

export interface SttStatus {
  ready: boolean
  engine: string
  reason: string
}

export interface EditAudio {
  id: number
  path: string
  startMs: number
  inMs: number
  outMs: number | null
  volume: number
  fadeInMs: number
  fadeOutMs: number
  looped: boolean
  duck: boolean
  /** 混响；null = 不加 */
  reverb: ReverbSpec | null
  /** 节拍分析结果（`beatsMs` 是素材里的时间，不随配乐在时间线上的位置变）；没分析过是空。只存在工程里，导出用不到 */
  beats?: EditBeats | null
}

/** 精彩片段分析：文稿里的一句话（素材时间）和候选结果 */
export interface HighlightSentence {
  startMs: number
  endMs: number
  text: string
}
export interface HighlightReq {
  path: string
  fromMs: number
  toMs: number
  sentences: HighlightSentence[]
  count: number
  minSecs: number
  maxSecs: number
}
export interface HighlightCandidate {
  startMs: number
  endMs: number
  /** 0–100 */
  score: number
  title: string
  reasons: string[]
}

export interface EditBeats {
  bpm: number
  /** 0–1，越大节奏越清晰 */
  confidence: number
  beatsMs: number[]
}

/** 叠加轨上的素材（画中画、贴纸、GIF、水印）：盖在主轨画面上面，位置、大小和时间都自由。轨号越大越在上面 */
export type OverlayAnim = 'none' | 'slide-left' | 'slide-right' | 'slide-up' | 'slide-down' | 'zoom'

export interface EditOverlay {
  id: number
  path: string
  kind: 'video' | 'image'
  /** 叠加轨编号 1–8 */
  track: number
  /** 在时间线上从哪里开始（毫秒） */
  startMs: number
  inMs: number
  outMs: number
  speed: number
  /** 素材不够长时重复播放（GIF、短视频） */
  looped: boolean
  volume: number
  mute: boolean
  fadeInMs: number
  fadeOutMs: number
  /** 入场 / 出场动画：none、slide-left / slide-right / slide-up / slide-down（从画面外滑入 / 滑出）、zoom（由小变大 / 变小消失） */
  animIn: OverlayAnim
  animOut: OverlayAnim
  /** 每段动画的时长（毫秒） */
  animMs: number
  /** 中心点在画面里的位置（占画面宽、高的比例，0.5 = 正中） */
  x: number
  y: number
  /** 素材宽度占画面宽度的比例 */
  scale: number
  /** 顺时针旋转的角度（度） */
  rotate: number
  opacity: number
  flipH: boolean
  flipV: boolean
  brightness: number
  contrast: number
  saturation: number
  /** 关键帧：x / y / scale / rotate / opacity / volume；有关键帧的属性不再用上面的固定值 */
  keyframes: EditKeyframes
  /** 抠像（绿幕 / 蓝幕） */
  chroma: EditChroma
  /** 形状蒙版 */
  mask: EditShapeMask
}

/** 抠像：把 `color` 附近的颜色变透明。相似度 0.01–0.6，边缘柔化 0–0.5，去溢色压掉留下部分边缘的绿 / 蓝反光 */
/** 叠加素材的形状蒙版（位置、大小占素材自己画面的比例） */
export interface EditShapeMask {
  on: boolean
  shape: 'rect' | 'ellipse'
  x: number
  y: number
  w: number
  h: number
  feather: number
  invert: boolean
}

export interface EditChroma {
  on: boolean
  color: string
  similarity: number
  blend: number
  despill: boolean
}

/** 自动识别绿幕的结果 */
export interface KeyGuess {
  color: string
  similarity: number
  blend: number
  coverage: number
}

/** 换背景：前景按颜色（chroma）或人像模型（ai）抠出，背景换成纯色 / 模糊原画面 / 图片 */
export interface EditBackground {
  on: boolean
  matte: 'chroma' | 'ai'
  /** 人像抠图用的模型 id（`matte` 为 `ai` 时） */
  model: string
  chroma: EditChroma
  mode: 'color' | 'blur' | 'image'
  color: string
  image: string
  blur: number
  feather: number
}

export interface EditOutput {
  width: number
  height: number
  fps: number
  fit: 'contain' | 'cover' | 'blur'
  quality: 'small' | 'balanced' | 'high'
  codec: 'h264' | 'hevc'
  format: 'mp4' | 'mkv'
  /** 导出什么：成片 / GIF / 只导出声音（m4a）/ 一帧图片（png） */
  kind: 'video' | 'gif' | 'audio' | 'frame'
  /** 只导出时间线上的这一段（毫秒）；toMs 为 0 = 到结尾。导出一帧时只看 fromMs */
  fromMs: number
  toMs: number
  /** 声音响度标准化（LUFS）；0 = 不处理 */
  loudness: number
  gifWidth: number
  gifFps: number
}

export interface EditProject {
  title: string
  clips: EditClip[]
  overlays: EditOverlay[]
  texts: EditText[]
  audio: EditAudio[]
  out: EditOutput
}

/** 素材文件的信息（`edit_probe`） */
export interface EditSource {
  path: string
  name: string
  kind: 'video' | 'audio' | 'image'
  durationMs: number | null
  width: number
  height: number
  fps: number | null
  hasVideo: boolean
  hasAudio: boolean
  hdr: boolean
  thumb: string | null
  url: string
}

export interface OpenedEdit {
  project: EditProject
  missing: string[]
}

/** BT tracker 列表的状态 */
export interface TrackerStatus {
  updatedAt: number | null
  source: string
  listCount: number
  effectiveCount: number
}

/** 本机的 AI 模型（人像抠图 / 人声分离）：装没装、从哪下 */
export interface ModelStatus {
  id: string
  label: string
  kind: 'matte' | 'vocals' | 'denoise' | 'whisper'
  url: string
  sizeHint: string
  note: string
  installed: boolean
  sizeBytes: number
}
export interface WhisperCliStatus {
  installed: boolean
  path: string | null
  autoInstall: boolean
  note: string
}
export interface SenseVoiceStatus {
  runtime: boolean
  runtimePath: string | null
  model: boolean
  ready: boolean
  autoInstall: boolean
  note: string
}
export interface ModelProgress {
  id: string
  got: number
  total: number
}

/** 人声分离的结果：两个 WAV 文件 */
export interface VocalsResult {
  vocals: string
  instrumental: string
  durationMs: number
}

// ---------- 批量下载 ----------

export interface BatchOpts {
  name: string
  dir: string
  folder: boolean
  /** '' 用设置里的 / best / 1080 / 720 / audio */
  quality: string
  concurrency: number
  jitterMin: number
  jitterMax: number
  speedLimitKbps: number
  /** none / notify / open / sleep / shutdown */
  afterDone: string
  useWindow: boolean
  startPaused: boolean
  template: string
}

export type PreviewStatus = 'ok' | 'queued' | 'done' | 'dup' | 'login' | 'bad' | 'file' | 'playlist'

export interface PreviewItem {
  key: string
  index: number
  url: string
  status: PreviewStatus
  title: string
  platform: string
  platformName: string
  site: string
  durationMs: number | null
  sizeEst: number | null
  quality: string
  error: string | null
  media: MediaInfo | null
  entries: PlaylistEntry[]
}

export interface BatchItem {
  url: string
  title: string
  media: MediaInfo | null
}

export interface BatchResult {
  id: string
  name: string
  queued: number
  alreadyDownloaded: number
  alreadyQueued: number
  pending: number
}

export interface FailedLink {
  url: string
  title: string
  error: string
}

export interface BatchView {
  id: string
  name: string
  source: string
  createdAt: number
  opts: BatchOpts
  total: number
  queued: number
  running: number
  paused: number
  done: number
  failed: number
  pending: number
  failedLinks: FailedLink[]
  baseDir: string
}

/** 打开“批量添加”面板的请求 */
export interface BatchRequest {
  urls: string[]
  source: string
  truncated?: boolean
}

export interface AiExpert {
  id: string
  name: string
  description: string
  origin: 'builtin' | 'user' | 'community'
  guide: string
  matches: string[]
  remoteId: string
  updated: number
}
export interface ExpertsState {
  experts: AiExpert[]
  selected: string
  auto: boolean
  communityPage: string
}

/** 写作任务的轮询结果 */
export interface WriteJob {
  state: 'running' | 'done' | 'error'
  done: number
  total: number
  note: string
  result: { text?: string; style?: string; candidates?: { text: string; guard?: { missing: string[]; ratio: number; warn: boolean } | null }[]; corrected?: string; issues?: { from: string; to: string; reason: string }[] } | null
  error: string | null
}

export interface VocabWord {
  id: number
  word: string
  phonetic: string
  pos: string
  meaning: string
  ctxMeaning: string
  sentence: string
  sourceTitle: string
  sourceUrl: string
  timeSec: number
  created: number
  due: number
  intervalDays: number
  reps: number
  lapses: number
  level: string
  /** 三个评分按钮各自的“下次复习”提示：不认识 / 模糊 / 认识 */
  next: [string, string, string]
}

export interface VocabStats {
  total: number
  due: number
  fresh: number
  learned: number
  today: number
}

// ---------- 实时屏幕翻译 ----------

export interface OcrApi {
  provider: 'ocrspace' | 'baidu' | 'google'
  key: string
  secret: string
  endpoint: string
}

export interface ScreenProfile {
  exe: string
  mode: string
  ocr: string
  ocrLang: string
  targetLang: string
  region: [number, number, number, number]
  target: string
}

export interface ShotEntry {
  at: number
  src: string
  text: string
  saved: boolean
}

export interface ScreenSettings {
  hotkey: string
  hotkeyPick: string
  hotkeyHide: string
  hotkeyMode: string
  hotkeyShot: string
  floatButton: boolean
  mode: 'standard' | 'subtitle' | 'manga' | 'voice'
  ocr: 'system' | 'enhanced' | 'api' | 'vision'
  ocrLang: string
  ocrApi: OcrApi
  targetLang: string
  fps: number
  stableMs: number
  changePct: number
  keepMs: number
  fontMin: number
  fontScale: number
  bilingual: boolean
  skipSame: boolean
  skipIcons: boolean
  opacity: number
  subPlace: 'cover' | 'bar'
  subBilingual: boolean
  subFps: number
  subStableMs: number
  subKeepMs: number
  subDedupe: boolean
  subMerge: boolean
  subBar: [number, number, number, number]
  subFont: number
  mangaEngine: 'vision' | 'local'
  mangaAck: boolean
  mangaErase: 'fill' | 'inpaint' | 'none'
  mangaRtl: boolean
  mangaFont: 'hand' | 'sans' | 'serif'
  mangaSfx: boolean
  mangaFollow: boolean
  mangaNames: boolean
  /** 增强识字用哪个语言包：auto 按识字语言选 / default 默认 / 指定包名 */
  ocrPack: string
  audioLang: string
  audioStt: 'local' | 'cloud'
  audioBilingual: boolean
  /** 听什么：电脑正在播放的声音 / 麦克风 */
  audioSource: 'system' | 'mic'
  /** 边说边出字 */
  audioDraft: boolean
  /** 翻译时带上最近几句当上文（0 = 不带） */
  audioContext: number
  profiles: ScreenProfile[]
  history: boolean
}

export interface ScreenTarget {
  kind: 'region' | 'window' | 'screen' | ''
  rect: [number, number, number, number]
  hwnd: number
  exe: string
  title: string
}

export interface ScreenStatus {
  supported: boolean
  running: boolean
  mode: string
  target: ScreenTarget | null
  hidden: boolean
  paused: boolean
  msg: string
  audio: boolean
}

export interface ScreenBlock {
  x: number
  y: number
  w: number
  h: number
  text: string
  orig: string
  fg: string
  bg: string
  font: number
  vertical: boolean
  lines: number
  center: boolean
  /** 原文的对齐：0 靠左 / 1 居中 / 2 靠右 */
  align?: number
}

export interface ScreenShotFrame {
  rect: [number, number, number, number]
  /** 干净的画面（没有译文）：data:image/png;base64,… */
  png: string
}

export interface ScreenShotSaved {
  path: string
  copied: boolean
}

export interface AppGlossaryPack {
  id: string
  name: string
  exes: string[]
  count: number
  installed: boolean
}

export interface ScreenPayload {
  seq: number
  mode: string
  rect: [number, number, number, number]
  blocks: ScreenBlock[]
  bilingual: boolean
  opacity: number
  fontMin: number
  cjk: boolean
  erase: boolean
  fontFamily: string
}

export interface ScreenSub {
  seq: number
  text: string
  orig: string
  bilingual: boolean
  font: number
  source: string
  /** 边说边出字的草稿：还没说完，没有译文 */
  draft?: boolean
}

/** 清影写作的人设 */
export interface WritePersona {
  id: string
  name: string
  desc: string
  /** reply：输入是收到的消息；create：输入是话题 / 情况 / 草稿 */
  mode: 'reply' | 'create'
  hint: string
}

/** 语音翻译：本次会话里的一句 */
export interface VoiceEntry {
  startMs: number
  endMs: number
  src: string
  text: string
}

export interface ScreenWindow {
  hwnd: number
  title: string
  exe: string
  x: number
  y: number
  w: number
  h: number
}

export interface ScreenPick {
  dataUrl: string
  x: number
  y: number
  w: number
  h: number
  windows: ScreenWindow[]
}

export interface ScreenTry {
  lines: string[]
  ms: number
  w: number
  h: number
  engine: string
}

export interface ScreenEntry {
  at: number
  mode: string
  exe: string
  src: string
  text: string
}

/** 增强识字的语言包 */
export interface OcrPack {
  id: string
  name: string
  installed: boolean
  size: number
}

export interface OcrStatus {
  installed: boolean
  dir: string
  size: number
  supported: boolean
}

export interface FileTransPage {
  src: string[]
  trans: (string | null)[]
  /** 对应 PDF 的第几页（从 0 起）；文字版 PDF 里没有文字的页不占位 */
  page?: number | null
  /** 这一页处理过了（译完或失败）；还没轮到的页是 false */
  done?: boolean
  /** 按设置不翻译的片段序号（页眉页脚、公式代码、参考文献） */
  skip?: number[]
  /** 译后检查发现的问题：片段序号 → 提示（只提示，不阻断） */
  qa?: Record<string, string[]>
}

export interface FileTransView {
  input: string
  target: string
  bilingual: boolean
  pages: FileTransPage[]
  failed: number
  v?: number
  /** 片段是按 PDF 版面的块切的 */
  layout?: boolean
  totalPages?: number
}

/** 文件翻译的选项（和后端 filetr::Opts 一一对应） */
export interface FtOpts {
  bilingual: boolean
  target: string
  pdfMode: 'inplace' | 'interleave' | 'txt'
  ocr: string
  preview: boolean
  /** 只翻译这些页，如 `3-12, 15`；留空 = 全部 */
  pages: string
  skipHeaders: boolean
  skipCode: boolean
  skipRefs: boolean
  /** 有文字层的 PDF 里夹着扫描页时自动识字并一起翻译 */
  scanAuto: boolean
}

/** 开始前的估算 */
export interface FtPlan {
  pages: number
  selectedPages: number
  segments: number
  chars: number
  cached: number
  skipped: number
  requests: number
  scanned: boolean
  layout: boolean
  donePages: number
}

/** 任务记录（重启后还在） */
export interface FtRecent {
  path: string
  title: string
  target: string
  opts: FtOpts
  at: number
  output: string | null
  last: string
  error: string
  state: 'stopped' | 'failed' | 'ready' | 'generated'
  pagesTotal: number
  pagesDone: number
  failed: number
  missing: boolean
}

export interface PdfxStatus {
  supported: boolean
  installed: boolean
  path: string
  fontOk: boolean
  note: string
}

export interface VideoCacheInfo {
  videoKey: string
  url: string
  title: string
  hasSummary: boolean
  hasSubs: boolean
  lines: number
  srcLang: string
  dstLang: string
  bytes: number
  updated: number
}
export interface VideoCacheFull {
  info: VideoCacheInfo
  summary: { data?: { res?: Record<string, unknown>; title?: string; lang?: string } } | null
  subs: { lines?: { t: number; text: string }[]; trans?: string[] } | null
}

export interface Excerpt {
  id: string
  text: string
  /** 来源：程序名或网页标题 */
  app: string
  note: string
  created: number
  /** 网页地址（浏览器里存的才有） */
  url?: string
}

export interface ModelInfo {
  id: string
  caps: string[]
  free: boolean
}
export interface ProbeResult {
  ok: boolean
  ms: number
  text: string
  vision: boolean | null
}

/** PDF 版面画布：一页的原页图和每块的位置、背景色、原文、译文 */
export interface PdfLayout {
  pages: number
  viewPage: number
  w: number
  h: number
  image: string
  blocks: { index: number; rect: [number, number, number, number]; size: number; rgb: [number, number, number]; bg: [number, number, number]; src: string; trans: string; skipped: boolean; adjusted: boolean }[]
  /** 这一页处理过了（译完或失败） */
  done: boolean
  /** 要告诉用户的情况（没译、重新匹配过、扫描页没识字…），没有就是空 */
  note: string
}
