<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import { useAppStore, useParseStore } from '../stores/app'
import type { Asset, BatchProgress, Settings, Clip, CreatorTabs, DetectedLink, HistoryItem, ListOptions, MediaInfo } from '../types'
import { formatBytes, formatDate, formatDuration } from '../utils/format'
import { formatClock } from '../utils/time'
import { SUB_DEFAULTS } from '../utils/subs'
import ErrorAlert from '../components/ErrorAlert.vue'
import SubtitlePicker, { type SubMode } from '../components/SubtitlePicker.vue'
import ClipPicker from '../components/ClipPicker.vue'
import PlaylistPicker from '../components/PlaylistPicker.vue'
import BatchWorkspace from '../components/BatchWorkspace.vue'
import BatchList from '../components/BatchList.vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'

const app = useAppStore()
const parse = useParseStore()

const selected = ref<Set<string>>(new Set())
const links = ref<DetectedLink[]>([])
const preview = ref(false)
const enqueuing = ref(false)
const audioOnly = ref(false)
const slideSecs = ref(3)
const slideMusic = ref(true)
const sliding = ref(false)
const musicAsset = computed(() => result.value?.assets.find((a) => a.kind === 'audio') ?? null)

async function makeSlideshow() {
  if (!result.value) return
  const ids = images.value.filter((a) => selected.value.has(a.id)).map((a) => a.id)
  if (!ids.length) {
    ElMessage.warning('请至少选择一张图片。')
    return
  }
  sliding.value = true
  try {
    const path = await api.makeSlideshow(result.value, ids, slideMusic.value && musicAsset.value ? musicAsset.value.id : null, slideSecs.value)
    ElMessage.success({ message: `已生成：${path}`, duration: 5000 })
    api.revealFile(path).catch(() => {})
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    sliding.value = false
  }
}
const entrySel = ref<Set<string>>(new Set())
/** 展开过、只勾了一部分的列表 */
const entryOnly = ref<Record<string, string[]>>({})
const entryTotal = ref({ videos: 0, lists: 0, approx: false })

// ---------- 创作者主页的分栏 ----------
const creatorTabs = ref<CreatorTabs | null>(null)
async function loadTabs(r: MediaInfo | null) {
  creatorTabs.value = null
  if (!r || r.kind !== 'playlist') return
  try {
    const t = await api.creatorTabs(r.sourceUrl)
    if (result.value === r && t.tabs.length) creatorTabs.value = t
  } catch {
    /* 没有分栏就按普通列表显示 */
  }
}
const tabLoading = ref('')
async function openTab(t: { key: string; url: string }) {
  if (t.key === creatorTabs.value?.active || parse.loading) return
  tabLoading.value = t.key
  const keep = creatorTabs.value
  await parse.parse(t.url)
  tabLoading.value = ''
  // 这一栏读不出来：保留分栏，错误提示显示在下面，可以切回去
  if (!parse.result) creatorTabs.value = keep
}

// ---------- 每个视频怎么下（只用于这一次；也可以存成默认） ----------
type Pick<T> = T | 'settings'
type VideoQuality = Exclude<NonNullable<ListOptions['quality']>, 'audio'>
const optQuality = ref<Pick<VideoQuality>>('settings')
const optSubtitle = ref<Pick<NonNullable<ListOptions['subtitle']>>>('settings')
const optSound = ref<'video' | 'audio'>('video')
const optAudioFmt = ref<NonNullable<ListOptions['audioFormat']>>('mp3')
const optSaveDefault = ref(false)
const optFolder = ref(true)
const optLayout = ref<NonNullable<ListOptions['layout']>>('platform_author_list')
const optNumbered = ref(true)
const QUALITY_CHOICES: { v: Pick<VideoQuality>; t: string }[] = [
  { v: 'settings', t: '按设置' },
  { v: 'best', t: '最高' },
  { v: '1080', t: '不超过 1080p' },
  { v: '720', t: '720p 省空间' },
]
const SUB_CHOICES: { v: Pick<NonNullable<ListOptions['subtitle']>>; t: string }[] = [
  { v: 'settings', t: '按设置' },
  { v: 'off', t: '不要字幕' },
  { v: 'file', t: '单独文件' },
  { v: 'embed', t: '内嵌到视频' },
  { v: 'burn', t: '烧录进画面' },
]
const LAYOUTS: { v: NonNullable<ListOptions['layout']>; t: string; p: string }[] = [
  { v: 'platform_author_list', t: '平台 / 创作者 / 列表', p: 'YouTube/彼德/AI新聞/' },
  { v: 'author_list', t: '创作者 / 列表', p: '彼德/AI新聞/' },
  { v: 'list', t: '只用列表名', p: 'AI新聞/' },
]
function buildListOptions(): ListOptions {
  const o: ListOptions = { folder: optFolder.value, layout: optLayout.value, numbered: optNumbered.value, only: entryOnly.value }
  if (optSound.value === 'audio') {
    o.quality = 'audio'
    o.audioFormat = optAudioFmt.value
  } else if (optQuality.value !== 'settings') o.quality = optQuality.value
  if (optSubtitle.value !== 'settings') o.subtitle = optSubtitle.value
  return o
}
const effQualityText = computed(() => {
  if (optSound.value === 'audio') return `只要音频 · ${optAudioFmt.value.toUpperCase()}`
  return optQuality.value === 'settings' ? presetText.value : (QUALITY_CHOICES.find((c) => c.v === optQuality.value)?.t ?? '')
})
const effSubText = computed(() => (optSubtitle.value === 'settings' ? subPolicyText.value : (SUB_CHOICES.find((c) => c.v === optSubtitle.value)?.t ?? '')))
const downloadedEntries = ref<Set<string>>(new Set())
const subMode = ref<SubMode>('file')
const preferredSubs = ref<string[]>([])
const clip = ref<Clip | null>(null)
const splitOn = ref(false)
/** 切到“只要音频”前选的清晰度（切回来时恢复） */
let prevVideo: string | null = null
const showAllFormats = ref(false)
const batchInfo = ref<BatchProgress | null>(null)
let unlistenBatch: UnlistenFn | undefined

const result = computed(() => parse.result)
const images = computed(() => result.value?.assets.filter((a) => a.kind === 'image') ?? [])
const videoFormats = computed(() => result.value?.assets.filter((a) => a.kind === 'video') ?? [])
async function toggleAutoSub(v: boolean) {
  const s = app.settings
  if (!s) return
  try {
    await app.patch({ ai: { ...s.ai, autoTranscribe: v } })
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
const subtitleAssets = computed(() => result.value?.assets.filter((a) => a.kind === 'subtitle') ?? [])
const extras = computed(() => result.value?.assets.filter((a) => a.kind !== 'image' && a.kind !== 'video' && a.kind !== 'subtitle') ?? [])
const videoSelected = computed(() => videoFormats.value.some((v) => selected.value.has(v.id)))
const canClip = computed(() => !!result.value && !isPlaylist.value && (result.value.kind === 'video' || result.value.kind === 'audio') && !audioOnly.value)
/** 只下载一段时能不能直接只取这一段：选中的音视频都走 yt-dlp（YouTube 等）；不能就下完整视频后在后台剪 */
const clipDirect = computed(() => {
  const av = (result.value?.assets ?? []).filter((a) => selected.value.has(a.id) && (a.kind === 'video' || a.kind === 'audio'))
  return av.length > 0 && av.every((a) => a.protocol === 'ytdlp')
})
/** 预览用的资源：优先用户选中的清晰度。FTP / SFTP / ed2k / BT 要连上对方才有数据，其余（含 m3u8、yt-dlp 的流）都能预览。 */
const previewable = (a: Asset) => (a.kind === 'video' || a.kind === 'audio') && !['ftp', 'sftp', 'ed2k', 'bt'].includes(a.protocol)
const previewAsset = computed(() => {
  const list = result.value?.assets.filter(previewable) ?? []
  return list.find((a) => selected.value.has(a.id)) ?? list.find((a) => a.kind === 'video') ?? list[0]
})
const NATIVE_EXTS = ['mp4', 'm4v', 'webm', 'mov', 'mp3', 'm4a', 'aac', 'ogg', 'opus', 'wav', 'flac']
/** 这个资源是不是由清影转成一小段再播放（而不是直接代理原文件）。 */
const previewIsClip = (a: Asset, transcoded: boolean) => transcoded || a.protocol !== 'http' || !!a.pairAudio || !NATIVE_EXTS.includes(a.ext.toLowerCase())
const previewUrl = ref('')
const previewKind = ref<'video' | 'audio'>('video')
const previewError = ref('')
const previewFail = ref('')
const previewBusy = ref('')
const previewNote = ref('')
const previewImages = ref<Record<string, string>>({})
let previewTranscoded = false
let previewRun = 0

async function loadPreview(transcode: boolean) {
  const r = result.value
  const a = previewAsset.value
  if (!r || !a) return
  const run = ++previewRun
  previewTranscoded = transcode
  previewFail.value = ''
  previewError.value = ''
  previewUrl.value = ''
  const clip = previewIsClip(a, transcode)
  previewBusy.value = clip ? (transcode ? '正在转成通用格式（取前 90 秒），可能要十几秒…' : '正在从源站取前 90 秒并转成可播放的格式，可能要十几秒…') : '正在加载预览…'
  try {
    const src = await api.previewAsset(r, a.id, transcode)
    if (run !== previewRun) return
    previewKind.value = src.kind === 'audio' ? 'audio' : 'video'
    previewNote.value = clip ? '这是前 90 秒的预览片段；完整内容下载后播放。' : ''
    previewUrl.value = src.url
  } catch (e) {
    if (run !== previewRun) return
    previewError.value = errorText(e)
  } finally {
    if (run === previewRun) previewBusy.value = ''
  }
}

watch(preview, async (on) => {
  previewRun++
  previewUrl.value = ''
  previewError.value = ''
  previewFail.value = ''
  previewBusy.value = ''
  previewNote.value = ''
  previewImages.value = {}
  const r = result.value
  if (!on || !r) return
  if (previewAsset.value) await loadPreview(false)
  else if (!images.value.length) previewError.value = '这个资源（FTP / SFTP / ed2k / BT 等）要连上对方才有数据，不能预览，下载后即可播放。'
  // 图片经本机服务代理：防盗链的站点直接显示会是裂图
  for (const img of images.value) {
    api
      .previewAsset(r, img.id)
      .then((s) => {
        if (preview.value) previewImages.value = { ...previewImages.value, [img.id]: s.url }
      })
      .catch(() => {})
  }
})

function onPreviewError(e: Event) {
  const err = (e.target as HTMLMediaElement).error
  // 直接播放 / 原样封装播不出来（编码不支持、封装不对）：自动换成转码再试一次
  if (!previewTranscoded && err?.code !== 1) {
    void loadPreview(true)
    return
  }
  previewFail.value = err?.code === 4 ? '预览打不开：这种编码没法转成可播放的格式，不影响下载。' : `预览加载失败（错误码 ${err?.code ?? '?'}），可能是链接已过期或网站不允许预览，不影响下载。`
}
const entries = computed(() => result.value?.entries ?? [])
const isPlaylist = computed(() => result.value?.kind === 'playlist')
const kindText = computed(() => {
  const r = result.value
  if (!r) return ''
  if (r.kind === 'playlist') return `列表 ${r.entries.length} 条`
  if (r.kind === 'audio') return '音频'
  if (r.kind === 'images') return `图集 ${images.value.length} 张`
  return '视频'
})
const selectedImages = computed(() => images.value.filter((a) => selected.value.has(a.id)).length)
const supported = computed(() => (app.info?.providers.map((p) => p.name).join('、') ?? '') + (app.settings?.useYtdlp ? '，其他网站用 yt-dlp' : ''))

async function loadPlaylistState(r: MediaInfo) {
  downloadedEntries.value = new Set()
  if (r.kind !== 'playlist' || !r.entries.length) return
  try {
    const done = new Set(await api.downloadedEntries(r.platform, r.entries))
    if (result.value !== r) return
    downloadedEntries.value = done
    // 跳过已下载时，默认只选没下载过的，省去逐条解析的时间
    if (done.size && app.settings?.skipExisting) entrySel.value = new Set(r.entries.filter((e) => !done.has(e.id)).map((e) => e.id))
  } catch {
    /* 查不到不影响下载 */
  }
}

async function loadPreferredSubs(r: MediaInfo) {
  preferredSubs.value = []
  if (!r.assets.some((a) => a.kind === 'subtitle')) return
  try {
    const ids = await api.preferredSubtitles(r)
    if (result.value !== r) return
    preferredSubs.value = ids
    // 设置里开启了“下载视频时同时处理字幕”：默认选中偏好语言的字幕
    if (app.settings?.subtitleMode !== 'off' && ids.length && r.kind === 'video') {
      const next = new Set(selected.value)
      ids.forEach((id) => next.add(id))
      selected.value = next
    }
  } catch {
    /* 没有偏好字幕也能手动选 */
  }
}

watch(
  result,
  (r) => {
    selected.value = new Set(r ? defaultSelection(r) : [])
    entrySel.value = new Set(r?.entries.map((e) => e.id) ?? [])
    clip.value = null
    splitOn.value = false
    subMode.value = app.settings?.subtitleMode === 'embed' ? 'soft' : app.settings?.subtitleMode === 'burn' ? 'burn' : 'file'
    if (r) {
      loadPlaylistState(r)
      loadPreferredSubs(r)
    }
    loadTabs(r)
    audioOnly.value = false
    prevVideo = null
    showAllFormats.value = false
    if (r && r.kind === 'video' && app.settings?.qualityPreset === 'audio') setSound('audio')
  },
  { immediate: true },
)

// ---------- 主页的“单个解析 / 批量下载”：批量就地完成，不再另开页面 ----------
const ws = ref<InstanceType<typeof BatchWorkspace> | null>(null)
const batchOn = ref(false)
const batchText = ref('')
function setMode(m: 'single' | 'batch') {
  if (m === 'single') {
    ws.value?.cancel()
    batchOn.value = false // 切走后工作区会被卸载，来不及发出“已关闭”
  }
  app.homeMode = m
}
let batchTimer: number | undefined
/** 批量模式下粘贴 / 输入：识别出链接就直接进入预览 */
function onBatchText() {
  window.clearTimeout(batchTimer)
  batchTimer = window.setTimeout(async () => {
    const found = batchText.value.trim() ? await api.batchExtract(batchText.value).catch(() => []) : []
    if (found.length) {
      batchText.value = ''
      app.openBatch(found, '粘贴')
    }
  }, 250)
}
async function importBatchFile() {
  const p = await openDialog({ multiple: false, filters: [{ name: '链接清单', extensions: ['txt', 'csv', 'md', 'html', 'htm'] }] })
  if (!p || typeof p !== 'string') return
  try {
    const [list, cut] = await api.batchReadFile(p)
    if (!list.length) {
      ElMessage.info('文件里没有找到链接')
      return
    }
    app.openBatch(list, '导入文件', cut)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

let detectTimer: number | undefined
watch(
  () => parse.text,
  (t) => {
    window.clearTimeout(detectTimer)
    detectTimer = window.setTimeout(async () => {
      links.value = t.trim() ? await api.detectLinks(t) : []
      // 粘贴了多条链接：自动切到批量下载，就地预览
      if (links.value.length > 1 && !batchOn.value) {
        const urls = links.value.map((l) => l.url)
        parse.text = ''
        links.value = []
        app.openBatch(urls, '粘贴')
      }
    }, 200)
  },
)

function bestAudio(r: MediaInfo): Asset | undefined {
  return r.assets.filter((a) => a.kind === 'audio').sort((a, b) => (b.bitrate ?? 0) - (a.bitrate ?? 0))[0]
}

/** 默认选中：视频选第一个格式（后端已按清晰度预设排序），图集选全部图片，音频选第一个音轨。 */
function defaultSelection(r: MediaInfo): string[] {
  if (r.kind === 'images') return r.assets.filter((a) => a.kind === 'image').map((a) => a.id)
  const want = r.kind === 'audio' ? 'audio' : 'video'
  const first = r.assets.find((a) => a.kind === want)
  return first ? [first.id] : []
}

function pickSubs(ids: string[]) {
  const next = new Set(selected.value)
  for (const a of subtitleAssets.value) next.delete(a.id)
  ids.forEach((id) => next.add(id))
  selected.value = next
}

const MANY = 500
async function downloadEntries() {
  if (!result.value) return
  const ids = entries.value.filter((e) => entrySel.value.has(e.id)).map((e) => e.id)
  if (!ids.length) {
    ElMessage.warning('请至少选择一个条目。')
    return
  }
  if (entryTotal.value.videos > MANY) {
    try {
      await ElMessageBox.confirm(`确认加入 ${entryTotal.value.videos} 个视频？一次加这么多会占满下载队列，可以先少选一些。`, '加入很多视频', { type: 'warning', confirmButtonText: '确认加入', cancelButtonText: '再看看' })
    } catch {
      return
    }
  }
  enqueuing.value = true
  try {
    if (optSaveDefault.value) {
      const patch: Partial<Settings> = {}
      if (optSound.value === 'audio') {
        patch.qualityPreset = 'audio'
        patch.audioFormat = optAudioFmt.value
      } else if (optQuality.value !== 'settings') patch.qualityPreset = ({ best: 'best', '1080': 'max1080', '720': 'small' } as const)[optQuality.value]
      if (optSubtitle.value !== 'settings') patch.subtitleMode = optSubtitle.value
      if (Object.keys(patch).length) await app.patch(patch)
    }
    const n = await api.enqueueEntries(result.value, ids, buildListOptions())
    ElMessage.success(`正在加入队列（${n} 项${entryTotal.value.lists ? '，列表会自动展开' : ''}）…`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    enqueuing.value = false
  }
}

onMounted(async () => {
  unlistenBatch = await events.onBatch((p) => {
    batchInfo.value = p
    if (p.finished) {
      const skipped = p.skipped ? `，${p.skipped} 项之前已下载或已在队列中` : ''
      const from = p.lists ? `（来自 ${p.lists} 个列表，各放进自己的文件夹）` : ''
      if (p.failed.length) ElMessage.warning({ message: `“${p.title}”：已加入 ${p.queued} 个视频${from}${skipped}，${p.failed.length} 项没成功：${p.failed.slice(0, 3).join('；')}${p.failed.length > 3 ? ' …' : ''}`, duration: 10000, showClose: true })
      else if (p.skipped) ElMessage.success(`“${p.title}”：已加入 ${p.queued} 个视频${from}${skipped}`)
      else ElMessage.success(`“${p.title}”：已加入 ${p.queued} 个视频${from}`)
      window.setTimeout(() => (batchInfo.value = null), 1500)
    }
  })
})
onUnmounted(() => unlistenBatch?.())

// ---------- 新版界面用到的派生数据 ----------

/** 声音：视频（含声音）/ 只要音频。只要音频时优先换成单独的音频轨（下载小），没有时下载视频后提取声音。 */
const audioTrack = computed(() => (result.value ? bestAudio(result.value) : undefined))
/** 单独的音频轨（按音质从高到低）。选“只要音频”时，清晰度那一块换成它们 */
const audioTracks = computed(() => (result.value?.assets.filter((a) => a.kind === 'audio') ?? []).sort((a, b) => (b.bitrate ?? 0) - (a.bitrate ?? 0)))
const showAudioList = computed(() => audioOnly.value && result.value?.kind === 'video' && audioTracks.value.length > 0)
/** 音频轨只能选一个（点已选中的不取消，只要音频总得有一条） */
function pickAudio(id: string) {
  const next = new Set(selected.value)
  for (const a of audioTracks.value) next.delete(a.id)
  next.add(id)
  selected.value = next
}
const usingAudioTrack = computed(() => audioOnly.value && !!audioTrack.value && selected.value.has(audioTrack.value.id) && !videoSelected.value)
function setSound(mode: 'video' | 'audio') {
  const r = result.value
  if (!r) return
  const next = new Set(selected.value)
  const track = bestAudio(r)
  if (mode === 'audio') {
    if (audioOnly.value) return
    audioOnly.value = true
    if (track) {
      prevVideo = videoFormats.value.find((v) => next.has(v.id))?.id ?? null
      for (const v of videoFormats.value) next.delete(v.id)
      next.add(track.id)
    }
  } else {
    if (!audioOnly.value) return
    audioOnly.value = false
    if (audioTracks.value.some((a) => next.has(a.id)) && !videoFormats.value.some((v) => next.has(v.id))) {
      for (const a of audioTracks.value) next.delete(a.id)
      const back = prevVideo ?? videoFormats.value[0]?.id
      if (back) next.add(back)
    }
    prevVideo = null
  }
  selected.value = next
}
const audioFmt = computed(() => (app.settings?.audioFormat ?? 'mp3').toUpperCase())

/** 清晰度列表：多于 6 种时先显示 6 种 */
const shownFormats = computed(() => (showAllFormats.value ? videoFormats.value : videoFormats.value.slice(0, 6)))
const defaultVideoId = computed(() => videoFormats.value[0]?.id ?? '')
const topVideoId = computed(() => [...videoFormats.value].sort((a, b) => (b.height ?? 0) - (a.height ?? 0) || (b.filesize ?? 0) - (a.filesize ?? 0))[0]?.id ?? '')
function formatSpec(a: Asset) {
  const parts: string[] = []
  if (a.width && a.height) parts.push(`${a.width}×${a.height}`)
  const codec = prettyCodec(a.vcodec)
  if (codec && !a.label.includes(codec)) parts.push(codec)
  if (a.fps) parts.push(`${a.fps}fps`)
  if (a.hasAudio === false) parts.push('需合并音频')
  if (a.protocol === 'hls') parts.push('M3U8')
  if (a.protocol === 'ytdlp') parts.push('yt-dlp 下载')
  return parts.join(' · ') || a.ext.toUpperCase()
}
function sizeOf(a: Asset) {
  if (a.filesize) return formatBytes(a.filesize)
  if (a.bitrate) return `${a.bitrate} kbps`
  return '—'
}

/** 封面：竖图（图集、竖屏视频）放在 4:3 的框里居中，横图按 16:9；都不裁切 */
const portrait = computed(() => {
  const r = result.value
  if (!r) return false
  return r.kind === 'images' || (!!r.width && !!r.height && r.height > r.width)
})
const coverBadge = computed(() => {
  const r = result.value
  if (!r) return ''
  if (r.kind === 'images') return `图集 ${images.value.length} 张`
  const h = Math.max(r.height ?? 0, ...videoFormats.value.map((v) => Math.min(v.height ?? 0, v.width ?? 0) || (v.height ?? 0)))
  const short = r.width && r.height ? Math.min(r.width, r.height) : h
  const q = Math.max(short, 0)
  if (q >= 2160) return '4K'
  if (q >= 1440) return '2K'
  if (q >= 1080) return '1080P'
  if (q >= 720) return '720P'
  return ''
})
const extractorText = computed(() => {
  const e = result.value?.extractor
  if (!e || e === 'native') return '无水印'
  if (e.startsWith('yt-dlp')) return 'yt-dlp 解析'
  if (e === 'generic') return '网页嗅探'
  return ''
})
/** 附带下载：图集里的背景音乐单独一块，只要音频时用到的音频轨不重复列出 */
const extrasShown = computed(() =>
  extras.value.filter((a) => !(result.value?.kind === 'images' && a.id === musicAsset.value?.id) && !(result.value?.kind !== 'audio' && a.kind === 'audio')),
)

const SUBMODE_TEXT: Record<SubMode, string> = { file: '单独文件', soft: '内嵌到视频', burn: '烧录进画面' }
const pickedSubs = computed(() => (audioOnly.value ? [] : subtitleAssets.value.filter((a) => selected.value.has(a.id))))
/** 底部摘要：会下载哪些 */
const summary = computed(() => {
  const r = result.value
  if (!r) return ''
  const parts: string[] = []
  const v = videoFormats.value.find((x) => selected.value.has(x.id))
  if (audioOnly.value) parts.push(`音频（转 ${audioFmt.value}）`)
  else if (v) parts.push(`${v.label} 视频`)
  if (images.value.length && selectedImages.value) parts.push(`${selectedImages.value} 张图片`)
  if (pickedSubs.value.length) parts.push(`${pickedSubs.value.length} 条字幕（${SUBMODE_TEXT[subMode.value]}）`)
  for (const a of extras.value) {
    if (!selected.value.has(a.id)) continue
    if (usingAudioTrack.value && a.id === audioTrack.value?.id) continue
    parts.push(a.id === musicAsset.value?.id && r.kind === 'images' ? '背景音乐' : a.label)
  }
  let text = parts.join(' + ')
  if (canClip.value && clip.value) text += ` · 只下载 ${formatClock(clip.value.startMs)}–${clip.value.endMs ? formatClock(clip.value.endMs) : '结尾'}`
  if (canClip.value && splitOn.value && r.chapters.length) text += ' · 按章节拆分'
  return text
})
const summaryHead = computed(() => summary.value.split(' + ')[0])
const summaryRest = computed(() => summary.value.slice(summaryHead.value.length))
/** 大约多大：选中的都有大小时给出总数，部分没有时说“至少” */
const sizeText2 = computed(() => {
  const r = result.value
  if (!r) return ''
  let sum = 0
  let unknown = 0
  for (const a of r.assets) {
    if (!selected.value.has(a.id) || a.kind === 'subtitle') continue
    if (a.filesize) sum += a.filesize
    else unknown++
    const pair = a.pairAudio ? r.assets.find((x) => x.id === a.pairAudio) : undefined
    if (pair && !selected.value.has(pair.id)) {
      if (pair.filesize) sum += pair.filesize
      else unknown++
    }
  }
  if (!sum) return unknown ? '大小未知' : ''
  return unknown ? `至少 ${formatBytes(sum)}` : `约 ${formatBytes(sum)}`
})
/** 保存位置（和后端一样：下载目录，按平台分子文件夹；列表再加一层列表名） */
const saveDir = computed(() => {
  const s = app.settings
  const r = result.value
  if (!s || !r) return ''
  const sep = s.downloadDir.includes('\\') ? '\\' : '/'
  const parts = [s.downloadDir.replace(/[\\/]+$/, '')]
  if (r.kind === 'playlist' && optFolder.value) {
    // 和后端 folder_parts 一致：平台 / 创作者 / 列表名（列表名里已有作者名就不重复）
    const list = entryTotal.value.lists && entries.value.every((e) => e.isList) ? '〈各列表名〉' : r.title
    const author = r.author && !list.includes(r.author) ? r.author : ''
    if (optLayout.value === 'platform_author_list' && r.platformName) parts.push(r.platformName)
    if (optLayout.value !== 'list' && author) parts.push(author)
    parts.push(list)
    return parts.filter(Boolean).join(sep)
  }
  if (s.subfolderByPlatform && r.platformName) parts.push(r.platformName)
  if (r.kind === 'playlist') parts.push(r.title)
  return parts.filter(Boolean).join(sep)
})
const PRESET_TEXT: Record<string, string> = { best: '最高清晰度', max1080: '不超过 1080p', small: '体积小', audio: '只要音频' }
const presetText = computed(() => PRESET_TEXT[app.settings?.qualityPreset ?? 'best'] ?? '按设置')
const subPolicyText = computed(() => {
  const m = app.settings?.subtitleMode ?? 'off'
  if (m === 'off') return '不下载字幕'
  return `偏好语言 · ${m === 'file' ? '单独文件' : m === 'embed' ? '内嵌到视频' : '烧录进画面'}`
})
const nothingPicked = computed(() => !result.value?.assets.some((a) => selected.value.has(a.id)))

function openSource() {
  if (result.value?.sourceUrl) api.openUrl(result.value.sourceUrl).catch((e) => ElMessage.error(errorText(e)))
}
const subscribing = ref(false)
async function subscribe() {
  const r = result.value
  if (!r) return
  subscribing.value = true
  try {
    await api.subsAdd(r.sourceUrl, r.title || null, { ...SUB_DEFAULTS, include: [], exclude: [] })
    ElMessage.success('已订阅：以后有新视频会自动下载，可以在“订阅”里改设置。')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    subscribing.value = false
  }
}
function invertImages() {
  const next = new Set(selected.value)
  for (const a of images.value) {
    if (next.has(a.id)) next.delete(a.id)
    else next.add(a.id)
  }
  selected.value = next
}

function toggle(id: string) {
  const next = new Set(selected.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selected.value = next
}

/** 清晰度只能选一个；再次点击已选中的格式则取消（只下载音乐、封面等）。 */
function pickVideo(id: string) {
  const next = new Set(selected.value)
  const had = next.has(id)
  for (const v of videoFormats.value) next.delete(v.id)
  if (!had) next.add(id)
  selected.value = next
}

/** avc1.64001F → H.264 这类可读名称。 */
function prettyCodec(c: string | null): string {
  if (!c) return ''
  const l = c.toLowerCase()
  if (l.startsWith('avc') || l.startsWith('h264') || l === 'h.264') return 'H.264'
  if (l.startsWith('hev') || l.startsWith('hvc') || l.startsWith('h265') || l === 'h.265') return 'H.265'
  if (l.startsWith('av01') || l === 'av1') return 'AV1'
  if (l.startsWith('vp9') || l.startsWith('vp09')) return 'VP9'
  return c.split('.')[0].toUpperCase()
}

function selectAllImages(on: boolean) {
  const next = new Set(selected.value)
  for (const a of images.value) {
    if (on) next.add(a.id)
    else next.delete(a.id)
  }
  selected.value = next
}

async function download() {
  if (!result.value) return
  // 只要音频时不下载字幕（界面上字幕那块是灰的）
  const ids = result.value.assets.filter((a) => selected.value.has(a.id) && !(audioOnly.value && a.kind === 'subtitle')).map((a) => a.id)
  if (ids.length === 0) {
    ElMessage.warning('请至少选择一项要下载的内容。')
    return
  }
  enqueuing.value = true
  try {
    const fmt = app.settings?.audioFormat ?? 'mp3'
    const hasSubs = ids.some((id) => subtitleAssets.value.some((a) => a.id === id))
    const r = await api.enqueue(result.value, ids, {
      extractAudio: audioOnly.value ? fmt : null,
      embedMetadata: false,
      subMode: hasSubs && subMode.value !== 'file' ? subMode.value : null,
      clip: canClip.value ? clip.value : null,
      splitChapters: canClip.value && splitOn.value ? result.value.chapters : [],
    })
    const added = r.tasks.length
    const parts: string[] = []
    if (r.alreadyQueued) parts.push(`${r.alreadyQueued} 项已在队列中`)
    if (r.alreadyDownloaded) parts.push(`${r.alreadyDownloaded} 项之前已下载`)
    if (added > 0 && parts.length) ElMessage.success(`已加入下载 ${added} 项；${parts.join('，')}，已跳过`)
    else if (added > 0) ElMessage.success(`已加入下载队列：${added} 项`)
    else if (r.alreadyQueued) ElMessage.info(`所选内容已在下载队列中。${r.alreadyDownloaded ? `另有 ${r.alreadyDownloaded} 项之前已下载。` : ''}`)
    else ElMessage.info('所选内容之前都已下载过，已跳过。可在设置中关闭“跳过已下载”。')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    enqueuing.value = false
  }
}

async function copyLinks() {
  if (!result.value) return
  const urls = result.value.assets.filter((a) => selected.value.has(a.id)).map((a) => a.url)
  const text = (urls.length ? urls : result.value.assets.map((a) => a.url)).join('\n')
  try {
    await api.copyText(text)
    ElMessage.success('直链已复制。直链有时效，请尽快使用。')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function clearRecent() {
  try {
    await ElMessageBox.confirm('清空“最近解析”？这些记录同时在“媒体库 → 历史”里，会一起清掉；已下载的文件不受影响。', '清空最近解析', { type: 'warning', confirmButtonText: '清空', cancelButtonText: '取消' })
  } catch {
    return
  }
  try {
    await api.clearHistory()
    await parse.loadRecent()
    ElMessage.success('已清空')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function openRecent(item: HistoryItem) {
  parse.result = item.info
  parse.text = item.sourceUrl
  parse.error = ''
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    parse.parse()
  }
}

function recentKind(r: HistoryItem) {
  if (r.kind === 'playlist') return `列表 ${r.info.entries.length} 条`
  if (r.kind === 'audio') return '音频'
  if (r.kind === 'images') return `图集 ${r.info.assets.filter((a) => a.kind === 'image').length} 张`
  return '视频'
}
</script>

<template>
  <div class="page">
    <!-- 单个解析 / 批量下载 -->
    <div class="modes" role="tablist">
      <button type="button" role="tab" :class="{ on: app.homeMode === 'single' }" :aria-selected="app.homeMode === 'single'" @click="setMode('single')">单个解析</button>
      <button type="button" role="tab" :class="{ on: app.homeMode === 'batch' }" :aria-selected="app.homeMode === 'batch'" @click="setMode('batch')">批量下载</button>
      <span class="mute modes-hint">{{ app.homeMode === 'single' ? '粘贴多条链接会自动切到“批量下载”' : '粘贴多条链接、导入 .txt / .csv，去重预览后统一加入队列' }}</span>
    </div>

    <!-- 粘贴栏 -->
    <template v-if="app.homeMode === 'single'">
    <div class="paste">
      <div class="pastebox">
        <svg class="ico" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" /><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" /></svg>
        <el-input
          v-model="parse.text"
          type="textarea"
          :autosize="{ minRows: 1, maxRows: 4 }"
          resize="none"
          placeholder="粘贴分享文案或任意视频网页链接（抖音、B站、YouTube…），按 Enter 解析"
          aria-label="分享链接"
          class="paste-input"
          @keydown="onKey"
        />
      </div>
      <el-button type="primary" class="pz-strong go" :loading="parse.loading" @click="parse.parse()">解析</el-button>
      <el-button v-if="parse.text || parse.result" class="clr" @click="parse.clear()">清空</el-button>
    </div>
    <div class="hint">
      <span v-if="app.settings?.watchClipboard" class="ok">● 剪贴板监听已开启</span>
      <span v-else>剪贴板监听已关闭</span>
      <span>Enter 解析 · Shift+Enter 换行</span>
      <span v-if="app.settings?.shortcut">全局快捷键 <span class="mono">{{ app.settings.shortcut.replace('CommandOrControl', 'Ctrl') }}</span></span>
      <el-popover placement="bottom-start" :width="360" trigger="click">
        <template #reference>
          <button type="button" class="pz-link">支持哪些网站</button>
        </template>
        <div class="sites">{{ supported || '读取中…' }}</div>
      </el-popover>
    </div>
    </template>

    <template v-else>
      <div v-if="!batchOn" class="bpaste">
        <el-input v-model="batchText" type="textarea" :autosize="{ minRows: 2, maxRows: 6 }" resize="none" placeholder="粘贴多条链接（一行一条或混在文案里都行），或把 .txt / .csv 文件拖到窗口里" aria-label="批量链接" @input="onBatchText" />
        <el-button class="bimp" @click="importBatchFile">导入文件…</el-button>
      </div>
      <BatchWorkspace ref="ws" @active="batchOn = $event" />
    </template>

    <ErrorAlert v-if="parse.error && app.homeMode === 'single'" :message="parse.error" :kind="parse.errorKind" :site="parse.errorSite" />

    <div v-if="app.homeMode === 'single' && parse.loading && !result" class="layout">
      <div class="work card skeleton"><el-skeleton animated :rows="6" /></div>
      <div class="dl card skeleton"><el-skeleton animated :rows="8" /></div>
    </div>

    <div v-if="app.homeMode === 'single' && result" class="layout">
      <!-- 左：作品 -->
      <section class="work card" aria-label="作品信息">
        <button type="button" class="cover" :class="{ portrait }" :aria-label="result.kind === 'images' ? '大图预览' : '预览'" @click="preview = true" @dblclick.stop>
          <img v-if="result.cover" :src="result.cover" referrerpolicy="no-referrer" alt="" />
          <span v-else class="nocover">没有封面</span>
          <span v-if="result.kind === 'video' || result.kind === 'audio'" class="play" aria-hidden="true">
            <svg width="20" height="20" viewBox="0 0 24 24"><path d="M8 5v14l11-7z" fill="currentColor" /></svg>
          </span>
          <span v-if="isPlaylist" class="count"><span class="mono">{{ entries.length }}</span><small>个视频</small></span>
          <span v-if="coverBadge" class="badge">{{ coverBadge }}</span>
          <span v-if="result.durationMs" class="dur mono">{{ formatDuration(result.durationMs) }}</span>
        </button>
        <div class="info">
          <h1 class="title selectable" :title="result.title">{{ result.title }}</h1>
          <div class="who">
            <span v-if="result.author" class="author">{{ result.author }}</span>
            <span v-if="result.author && result.publishedAt">·</span>
            <span v-if="result.publishedAt">{{ formatDate(result.publishedAt) }} 发布</span>
          </div>
          <div class="tags">
            <span class="pz-tag acc">{{ result.platformName }} · {{ kindText }}</span>
            <span v-if="extractorText" class="pz-tag">{{ extractorText }}</span>
            <span v-if="result.width && result.height" class="pz-tag mono">{{ result.width }}×{{ result.height }}</span>
            <span v-if="result.chapters?.length" class="pz-tag">{{ result.chapters.length }} 个章节</span>
            <span v-if="subtitleAssets.length" class="pz-tag">{{ subtitleAssets.length }} 条字幕</span>
            <span v-if="result.kind === 'images' && musicAsset" class="pz-tag">背景音乐</span>
            <span v-if="isPlaylist && downloadedEntries.size" class="pz-tag ok">之前下载过 {{ downloadedEntries.size }} 个</span>
          </div>
          <div class="acts">
            <template v-if="isPlaylist">
              <el-button class="grow" :loading="subscribing" @click="subscribe">订阅更新</el-button>
            </template>
            <template v-else>
              <el-button class="grow" @click="preview = true">{{ result.kind === 'images' ? '大图预览' : '预览' }}</el-button>
              <el-button class="grow" @click="copyLinks">复制直链</el-button>
            </template>
            <el-button class="icon" aria-label="打开原网页" title="打开原网页" @click="openSource">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
            </el-button>
          </div>
        </div>
      </section>

      <!-- 右：下载内容 -->
      <section class="dl card" :aria-label="isPlaylist ? '选择要下载的视频' : '下载内容'">
        <div class="dhead">
          <h2>{{ isPlaylist ? '选择要下载的视频' : '下载内容' }}</h2>
          <span class="pz-note">{{ isPlaylist ? (downloadedEntries.size && app.settings?.skipExisting ? '默认选中没下载过的' : '默认全选') : result.kind === 'images' ? '点图片取消 / 选中，双击看大图' : '按设置默认选好了，改完点“下载所选”' }}</span>
        </div>

        <template v-if="isPlaylist">
          <div v-if="creatorTabs" class="tabs" role="tablist" aria-label="创作者主页分栏">
            <button
              v-for="t in creatorTabs.tabs"
              :key="t.key"
              type="button"
              role="tab"
              class="tab"
              :class="{ on: t.key === creatorTabs.active }"
              :aria-selected="t.key === creatorTabs.active"
              :disabled="parse.loading && t.key !== creatorTabs.active"
              @click="openTab(t)"
            >
              {{ t.label }}<span v-if="tabLoading === t.key" class="pz-note"> 读取中…</span>
            </button>
          </div>
          <div class="sec">
            <PlaylistPicker v-model:selected="entrySel" v-model:only="entryOnly" v-model:total="entryTotal" :entries="entries" :downloaded="downloadedEntries" />
          </div>
          <div class="sec">
            <div class="pz-sh">
              <span class="pz-st">每个视频怎么下</span>
              <span class="pz-sa"><button type="button" class="pz-link" @click="app.goSettings('download')">改默认设置</button></span>
            </div>
            <div class="opts">
              <div class="opt">
                <span class="olabel" id="o-sound">声音</span>
                <div class="chips" role="radiogroup" aria-labelledby="o-sound">
                  <button type="button" role="radio" class="chip" :class="{ on: optSound === 'video' }" :aria-checked="optSound === 'video'" @click="optSound = 'video'">视频（含声音）</button>
                  <button type="button" role="radio" class="chip" :class="{ on: optSound === 'audio' }" :aria-checked="optSound === 'audio'" @click="optSound = 'audio'">只要音频</button>
                  <el-select v-if="optSound === 'audio'" v-model="optAudioFmt" size="small" class="fmt" aria-label="音频格式">
                    <el-option v-for="f in ['mp3', 'm4a', 'opus', 'flac']" :key="f" :label="f.toUpperCase()" :value="f" />
                  </el-select>
                </div>
              </div>
              <div v-if="optSound === 'video'" class="opt">
                <span class="olabel" id="o-q">清晰度</span>
                <div class="chips" role="radiogroup" aria-labelledby="o-q">
                  <button v-for="c in QUALITY_CHOICES" :key="c.v" type="button" role="radio" class="chip" :class="{ on: optQuality === c.v }" :aria-checked="optQuality === c.v" @click="optQuality = c.v">
                    {{ c.t }}<small v-if="c.v === 'settings'"> · {{ presetText }}</small>
                  </button>
                </div>
              </div>
              <div v-if="optSound === 'video'" class="opt">
                <span class="olabel" id="o-s">字幕</span>
                <div class="chips" role="radiogroup" aria-labelledby="o-s">
                  <button v-for="c in SUB_CHOICES" :key="c.v" type="button" role="radio" class="chip" :class="{ on: optSubtitle === c.v }" :aria-checked="optSubtitle === c.v" @click="optSubtitle = c.v">
                    {{ c.t }}<small v-if="c.v === 'settings'"> · {{ subPolicyText }}</small>
                  </button>
                </div>
                <span class="pz-note">字幕语言按设置里的偏好语言。</span>
              </div>
              <label class="chk"><el-checkbox v-model="optSaveDefault" />同时存成默认设置（以后按这个来）</label>
            </div>
          </div>
          <div class="sec">
            <div class="pz-sh"><span class="pz-st">保存到哪里</span></div>
            <div class="opts">
              <label class="chk"><el-switch v-model="optFolder" size="small" />按列表放进各自的文件夹</label>
              <template v-if="optFolder">
                <div class="chips col" role="radiogroup" aria-label="文件夹怎么排">
                  <button v-for="l in LAYOUTS" :key="l.v" type="button" role="radio" class="chip lay" :class="{ on: optLayout === l.v }" :aria-checked="optLayout === l.v" @click="optLayout = l.v">
                    <b>{{ l.t }}</b><span class="mono">{{ l.p }}</span>
                  </button>
                </div>
                <label class="chk"><el-checkbox v-model="optNumbered" />文件名带列表里的序号（01、02…）</label>
                <span class="pz-note">文件夹名里不能用的字符会自动替换；同名文件夹已有就接着放进去，重名文件自动加（1）。</span>
              </template>
            </div>
          </div>
        </template>

        <template v-else>
          <!-- 清晰度 / 音频：单选列表。选“只要音频”时同一块换成音频轨 -->
          <div v-if="videoFormats.length || showAudioList" class="sec">
            <div class="pz-sh">
              <span class="pz-st">{{ showAudioList ? '音频' : '清晰度' }}<small>{{ showAudioList ? audioTracks.length : videoFormats.length }} 种</small></span>
              <span v-if="!showAudioList && videoFormats.length > 6" class="pz-sa"><button type="button" class="pz-link" @click="showAllFormats = !showAllFormats">{{ showAllFormats ? '收起' : `显示全部 ${videoFormats.length} 种` }}</button></span>
            </div>
            <template v-if="showAudioList">
              <p class="pz-note">只要音频：下载单独的音频轨，完成后转成 {{ audioFmt }}，不需要选清晰度。</p>
              <div class="rows" role="radiogroup" aria-label="音频">
                <button
                  v-for="(a, i) in audioTracks"
                  :key="a.id"
                  type="button"
                  role="radio"
                  class="frow"
                  :class="{ on: selected.has(a.id) }"
                  :aria-checked="selected.has(a.id)"
                  @click="pickAudio(a.id)"
                >
                  <span class="dot" />
                  <span class="fname">
                    <span class="ellipsis">{{ a.label }}</span>
                    <span v-if="i === 0" class="pz-tag" :class="{ acc: selected.has(a.id) }">音质最高</span>
                  </span>
                  <span class="mono spec ellipsis">{{ a.ext.toUpperCase() }}{{ a.bitrate ? ` · ${a.bitrate} kbps` : '' }} → {{ audioFmt }}</span>
                  <span class="mono size">{{ sizeOf(a) }}</span>
                </button>
              </div>
            </template>
            <template v-else>
              <p v-if="audioOnly" class="pz-note">这个视频没有单独的音频轨：会先下载所选清晰度再提取声音，选低一点的更快。</p>
              <div class="rows" role="radiogroup" aria-label="清晰度">
                <button
                  v-for="v in shownFormats"
                  :key="v.id"
                  type="button"
                  role="radio"
                  class="frow"
                  :class="{ on: selected.has(v.id) }"
                  :aria-checked="selected.has(v.id)"
                  @click="pickVideo(v.id)"
                >
                  <span class="dot" />
                  <span class="fname">
                    <span class="ellipsis">{{ v.label }}</span>
                    <span v-if="v.id === defaultVideoId" class="pz-tag" :class="{ acc: selected.has(v.id) }">默认</span>
                    <span v-else-if="v.id === topVideoId" class="pz-tag">最高</span>
                  </span>
                  <span class="mono spec ellipsis">{{ formatSpec(v) }}</span>
                  <span class="mono size">{{ sizeOf(v) }}</span>
                </button>
              </div>
            </template>
          </div>

          <!-- 声音 -->
          <div v-if="result.kind === 'video'" class="sec">
            <div class="pz-sh"><span class="pz-st">声音</span></div>
            <div class="inline">
              <div class="pz-seg" role="radiogroup" aria-label="声音">
                <button type="button" role="radio" :aria-checked="!audioOnly" :class="{ on: !audioOnly }" @click="setSound('video')">视频（含声音）</button>
                <button type="button" role="radio" :aria-checked="audioOnly" :class="{ on: audioOnly }" @click="setSound('audio')">只要音频 · {{ audioFmt }}</button>
              </div>
              <span class="pz-note">选“只要音频”时下载后转成 {{ audioFmt }}，不下载字幕</span>
            </div>
          </div>

          <!-- 图片 -->
          <div v-if="images.length" class="sec">
            <div class="pz-sh">
              <span class="pz-st">图片<small>已选 {{ selectedImages }} / {{ images.length }}</small></span>
              <span class="pz-sa">
                <button type="button" class="pz-link" @click="selectAllImages(true)">全选</button>
                <button type="button" class="pz-link" @click="selectAllImages(false)">全不选</button>
                <button type="button" class="pz-link" @click="invertImages">反选</button>
              </span>
            </div>
            <div class="grid">
              <button
                v-for="img in images"
                :key="img.id"
                type="button"
                class="ph"
                :class="{ sel: selected.has(img.id) }"
                :aria-pressed="selected.has(img.id)"
                :aria-label="img.label"
                @click="toggle(img.id)"
                @dblclick="preview = true"
              >
                <img :src="img.url" referrerpolicy="no-referrer" loading="lazy" alt="" />
                <span class="ck" aria-hidden="true" />
                <span class="no mono">{{ (img.index ?? 0) + 1 }}</span>
              </button>
            </div>
          </div>

          <!-- 背景音乐（图集） -->
          <div v-if="result.kind === 'images' && musicAsset" class="sec">
            <div class="pz-sh"><span class="pz-st">背景音乐</span></div>
            <div class="inline">
              <el-button class="round" circle aria-label="试听背景音乐" @click="preview = true">
                <svg width="12" height="12" viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14l11-7z" fill="currentColor" /></svg>
              </el-button>
              <div class="music">
                <span class="ellipsis">{{ musicAsset.label }}</span>
                <span class="mono pz-note">{{ musicAsset.ext.toUpperCase() }}<template v-if="musicAsset.filesize"> · {{ formatBytes(musicAsset.filesize) }}</template></span>
              </div>
              <button type="button" class="pz-chip" :class="{ on: selected.has(musicAsset.id) }" :aria-pressed="selected.has(musicAsset.id)" @click="toggle(musicAsset.id)"><span class="pz-box" />一起下载</button>
            </div>
          </div>

          <!-- 字幕 -->
          <div v-if="subtitleAssets.length" class="sec" :class="{ off: audioOnly }">
            <SubtitlePicker
              v-model:mode="subMode"
              :assets="subtitleAssets"
              :selected="selected"
              :has-video="videoSelected"
              :preferred="preferredSubs"
              :disabled="audioOnly ? '只要音频时不下载字幕。' : ''"
              @toggle="toggle"
              @pick="pickSubs"
            />
          </div>
          <div v-else-if="videoFormats.length && app.settings && !audioOnly" class="sec" data-testid="parse-nosub">
            <div class="pz-sh"><span class="pz-st">字幕</span></div>
            <div class="inline">
              <span class="pz-note">这个视频没有可下载的字幕。</span>
              <el-switch :model-value="app.settings.ai.autoTranscribe" @update:model-value="(v: string | number | boolean) => toggleAutoSub(!!v)" />
              <span>下载完成后用 AI 自动生成字幕</span>
            </div>
            <span class="pz-note">需要在“设置 → AI”配好本机 whisper 或云端识别；也可以下载完后在队列里点“生成字幕”。</span>
          </div>

          <!-- 范围 -->
          <div v-if="result.kind === 'video' || result.kind === 'audio'" class="sec" :class="{ off: !canClip }">
            <div class="pz-sh">
              <span class="pz-st">范围</span>
              <span v-if="result.durationMs" class="pz-sa"><span class="mono pz-note">全长 {{ formatDuration(result.durationMs) }}</span></span>
            </div>
            <ClipPicker v-if="canClip" v-model:clip="clip" v-model:split="splitOn" :duration-ms="result.durationMs" :chapters="result.chapters ?? []" :direct="clipDirect" />
            <p v-else class="pz-note">只要音频时下载完整的声音。</p>
          </div>

          <!-- 附带下载 / 音频 -->
          <div v-if="extrasShown.length" class="sec">
            <div class="pz-sh"><span class="pz-st">{{ result.kind === 'audio' ? '音频' : '附带下载' }}</span></div>
            <div class="chips">
              <button
                v-for="a in extrasShown"
                :key="a.id"
                type="button"
                class="pz-chip"
                :class="{ on: selected.has(a.id) }"
                :aria-pressed="selected.has(a.id)"
                :disabled="usingAudioTrack && a.id === audioTrack?.id"
                @click="toggle(a.id)"
              >
                <span class="pz-box" />
                <span class="ellipsis">{{ a.label }}</span>
                <span class="mono pz-note">{{ a.ext.toUpperCase() }}<template v-if="a.filesize"> · {{ formatBytes(a.filesize) }}</template></span>
              </button>
            </div>
          </div>

          <!-- 合成视频（图集） -->
          <div v-if="images.length > 1" class="sec">
            <div class="pz-sh"><span class="pz-st">合成视频<small>可选</small></span></div>
            <div class="inline">
              <label class="lbl">每张显示 <el-input-number v-model="slideSecs" :min="0.5" :max="30" :step="0.5" :controls="false" class="secs" aria-label="每张显示秒数" /> 秒</label>
              <el-checkbox v-if="musicAsset" v-model="slideMusic">配上背景音乐</el-checkbox>
              <span class="pz-note">用选中的 {{ selectedImages }} 张，约 {{ Math.round(selectedImages * slideSecs) }} 秒，需要 ffmpeg</span>
              <el-button class="push" :loading="sliding" :disabled="!selectedImages" @click="makeSlideshow">生成视频</el-button>
            </div>
          </div>
        </template>

        <!-- 底部操作栏 -->
        <div class="foot">
          <div class="sum">
            <span v-if="isPlaylist"><b>{{ entryTotal.lists ? `已选 ${entryTotal.lists} 个列表（共 ${entryTotal.approx ? '约 ' : ''}${entryTotal.videos} 个视频）` : `${entrySel.size} 个视频` }}</b> · {{ effQualityText }}<template v-if="optSound === 'video'"> · {{ effSubText }}</template></span>
            <span v-else-if="summary"><b>{{ summaryHead }}</b>{{ summaryRest }}</span>
            <span v-else class="pz-note">还没有选要下载的内容</span>
            <span class="pz-note where">
              <template v-if="!isPlaylist && sizeText2">{{ sizeText2 }} · </template>保存到 <span class="mono ellipsis path" :title="saveDir">{{ saveDir }}</span>
              <button type="button" class="pz-link" @click="app.goSettings('download')">更改</button>
            </span>
          </div>
          <el-button v-if="isPlaylist" type="primary" class="pz-strong big" :loading="enqueuing" :disabled="(!!batchInfo && !batchInfo.finished) || !entrySel.size" @click="downloadEntries">
            {{ batchInfo && !batchInfo.finished ? `解析中 ${batchInfo.done}/${batchInfo.total}` : entryTotal.lists ? `下载所选 ${entryTotal.videos} 个` : `下载所选 ${entrySel.size} 个` }}
          </el-button>
          <el-button v-else type="primary" class="pz-strong big" :loading="enqueuing" :disabled="nothingPicked" @click="download">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11" /><path d="M7 10l5 5 5-5" /><path d="M5 20h14" /></svg>
            <span>下载所选</span>
          </el-button>
        </div>
      </section>
    </div>

    <section v-if="app.homeMode === 'single' && !result && !parse.loading && parse.recent.length" class="recent card">
      <div class="pz-sh rhead">
        <span class="pz-st">最近解析</span>
        <span class="pz-sa"><button type="button" class="pz-link" @click="clearRecent">清空</button></span>
      </div>
      <button v-for="r in parse.recent" :key="r.id" type="button" class="ritem" @click="openRecent(r)">
        <img v-if="r.cover" :src="r.cover" class="thumb" referrerpolicy="no-referrer" alt="" />
        <div v-else class="thumb" />
        <span class="ellipsis">{{ r.title }}</span>
        <span class="pz-tag">{{ r.info.platformName }} · {{ recentKind(r) }}</span>
      </button>
    </section>

    <section v-if="app.homeMode === 'single' && !result && !parse.loading && !parse.recent.length && !parse.error" class="empty card">
      <h3>复制分享链接，就能保存无水印作品</h3>
      <ol>
        <li>在 App 里点“分享 → 复制链接”。</li>
        <li>回到这里粘贴（开启剪贴板监听后会自动识别）。</li>
        <li>选择视频、图片或背景音乐，点击“下载所选”。</li>
      </ol>
    </section>

    <el-dialog v-model="preview" :title="result?.title" width="720px" align-center destroy-on-close>
      <div v-if="result" class="preview">
        <template v-if="previewAsset">
          <p v-if="previewError" class="selectable mute">{{ previewError }}</p>
          <p v-else-if="previewFail" class="selectable mute">{{ previewFail }}</p>
          <video v-if="previewUrl && !previewFail && previewKind === 'video'" :src="previewUrl" controls autoplay @error="onPreviewError" />
          <audio v-else-if="previewUrl && !previewFail" :src="previewUrl" controls autoplay @error="onPreviewError" />
          <p v-if="previewBusy" class="mute">{{ previewBusy }}</p>
          <p v-else-if="previewUrl && previewNote && !previewFail" class="mute">{{ previewNote }}</p>
        </template>
        <p v-else-if="previewError" class="selectable mute">{{ previewError }}</p>
        <el-carousel v-if="images.length" height="460px" indicator-position="outside" :autoplay="false">
          <el-carousel-item v-for="img in images" :key="img.id">
            <img :src="previewImages[img.id] || img.url" referrerpolicy="no-referrer" alt="" />
          </el-carousel-item>
        </el-carousel>
      </div>
    </el-dialog>

    <!-- 批次：批量模式，或单个模式的空闲主页（没有解析结果时）都显示 -->
    <BatchList v-if="app.homeMode === 'batch' || (!result && !parse.loading && !parse.error)" />
  </div>
</template>

<style scoped>
.page {
  padding: 24px 32px 32px;
  max-width: 1104px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

/* 粘贴栏 */
.paste {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}
.pastebox {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  min-height: 44px;
  padding: 0 14px;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  background: var(--cc-card);
  box-sizing: border-box;
}
.pastebox:focus-within {
  border-color: var(--cc-acc);
}
.pastebox .ico {
  flex: none;
  margin-top: 14px;
  color: var(--cc-mute);
}
.paste-input :deep(.el-textarea__inner) {
  padding: 11px 0;
  border: 0;
  box-shadow: none;
  background: transparent;
  font-size: 13px;
  line-height: 20px;
  min-height: 42px !important;
}
.paste .go {
  min-width: 88px;
  height: 44px;
  font-size: 14px;
}
.paste .clr {
  height: 44px;
  margin-left: 0;
}
.hint {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 18px;
  font-size: 12px;
  color: var(--cc-mute);
  margin-top: -8px;
}
.hint .ok {
  color: var(--cc-ok);
}
.sites {
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--cc-fg2);
  user-select: text;
}
.batch {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 14px;
  padding: 12px 16px;
}
.batch .el-button {
  margin-left: auto;
}

/* 两栏 */
.layout {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  align-items: flex-start;
}
.work {
  flex: 1 1 280px;
  max-width: 320px;
  overflow: hidden;
  position: sticky;
  top: 0;
}
.dl {
  flex: 999 1 520px;
  min-width: 0;
  /* clip 而不是 hidden：hidden 会让底部操作栏的 sticky 失效 */
  overflow: clip;
  display: flex;
  flex-direction: column;
}
.skeleton {
  padding: 20px;
}

/* 作品卡 */
.cover {
  position: relative;
  display: block;
  width: 100%;
  aspect-ratio: 16 / 9;
  padding: 0;
  border: 0;
  background: var(--cc-cover);
  cursor: pointer;
  overflow: hidden;
}
.cover.portrait {
  aspect-ratio: 4 / 3;
}
.cover img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}
.nocover {
  position: absolute;
  left: 12px;
  bottom: 12px;
  color: #ffffff99;
  font-size: 12px;
}
.play {
  position: absolute;
  left: 50%;
  top: 50%;
  width: 52px;
  height: 52px;
  margin: -26px 0 0 -26px;
  border-radius: 50%;
  background: #ffffffe6;
  color: #1d2420;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.15s;
}
.cover:hover .play {
  transform: scale(1.06);
}
.count {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 38%;
  background: #000000a6;
  color: #fff;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
}
.count .mono {
  font-size: 22px;
  font-weight: 600;
}
.count small {
  font-size: 12px;
}
.badge,
.dur {
  position: absolute;
  height: 22px;
  padding: 0 8px;
  border-radius: 4px;
  background: #000000b3;
  color: #fff;
  font-size: 11.5px;
  display: flex;
  align-items: center;
}
.badge {
  left: 10px;
  top: 10px;
  font-weight: 600;
}
.dur {
  right: 10px;
  bottom: 10px;
  font-size: 12px;
}
.info {
  padding: 16px 18px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.title {
  margin: 0;
  font-size: 15px;
  line-height: 1.45;
  font-weight: 600;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
  word-break: break-word;
}
.who {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  font-size: 12.5px;
  color: var(--cc-mute);
}
.who .author {
  color: var(--cc-fg);
}
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.acts {
  display: flex;
  gap: 8px;
  padding-top: 10px;
  margin-top: 2px;
  border-top: 1px solid var(--cc-line-soft);
}
.acts .el-button {
  margin: 0;
  height: 36px;
}
.acts .grow {
  flex: 1;
}
.acts .icon {
  width: 36px;
  padding: 0;
}

/* 下载内容 */
.dhead {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  justify-content: space-between;
  gap: 4px 12px;
  padding: 18px 24px 14px;
}
.dhead h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.sec {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 20px 24px;
  border-top: 1px solid var(--cc-line-soft);
}
.sec.off .pz-st {
  color: var(--cc-mute);
}
.sec p.pz-note {
  margin: 0;
}
.inline {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px 12px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.chips .pz-chip {
  max-width: 360px;
}

/* 清晰度列表 */
.rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.frow {
  display: grid;
  grid-template-columns: 20px minmax(96px, 150px) minmax(0, 1fr) 84px;
  align-items: center;
  gap: 12px;
  min-height: 44px;
  padding: 0 14px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  font: inherit;
  color: inherit;
  text-align: left;
  cursor: pointer;
}
.frow:hover:not(:disabled) {
  background: var(--cc-hover);
}
.frow.on {
  background: var(--cc-acc-soft);
  border-color: var(--cc-acc-line);
}
.frow:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}
.frow .dot {
  width: 16px;
  height: 16px;
  border-radius: 50%;
  border: 1.5px solid var(--cc-dot);
  box-sizing: border-box;
}
.frow.on .dot {
  border: 5px solid var(--cc-acc-strong);
}
.fname {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-weight: 600;
}
.spec {
  font-size: 12px;
  color: var(--cc-mute);
}
.frow.on .spec {
  color: var(--cc-fg2);
}
.size {
  text-align: right;
}

/* 图片 */
.grid {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 10px;
}
.ph {
  position: relative;
  aspect-ratio: 3 / 4;
  padding: 0;
  border: 0;
  border-radius: 8px;
  overflow: hidden;
  background: var(--cc-fill);
  cursor: pointer;
}
.ph img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  opacity: 0.45;
  transition: opacity 0.15s;
}
.ph.sel img {
  opacity: 1;
}
.ph .ck {
  position: absolute;
  right: 6px;
  top: 6px;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 2px solid #fff;
  background: #00000040;
  box-sizing: border-box;
}
.ph.sel .ck {
  background: var(--cc-acc-strong);
}
.ph.sel .ck::after {
  content: '';
  position: absolute;
  left: 4px;
  top: 4px;
  width: 7px;
  height: 4px;
  border: 2px solid #fff;
  border-top: 0;
  border-right: 0;
  transform: rotate(-45deg);
}
.ph .no {
  position: absolute;
  left: 6px;
  bottom: 6px;
  min-width: 20px;
  height: 18px;
  padding: 0 5px;
  border-radius: 4px;
  background: #000000a6;
  color: #fff;
  font-size: 11px;
  display: flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
}

/* 背景音乐、合成视频、列表设置 */
.music {
  flex: 1;
  min-width: 160px;
  display: flex;
  flex-direction: column;
}
.round {
  width: 36px;
  height: 36px;
}
.lbl {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  color: var(--cc-fg2);
}
.secs {
  width: 64px;
}
.push {
  margin-left: auto;
}
.tabs {
  display: flex;
  gap: 4px;
  border-bottom: 1px solid var(--cc-line-soft);
  margin-bottom: 12px;
}
.tab {
  padding: 8px 14px;
  border: 0;
  border-bottom: 2px solid transparent;
  background: none;
  color: var(--cc-fg2);
  font: inherit;
  cursor: pointer;
}
.tab.on {
  color: var(--cc-accent, #2457e6);
  border-bottom-color: var(--cc-accent, #2457e6);
  font-weight: 600;
}
.tab:disabled {
  opacity: 0.5;
  cursor: default;
}
.opts {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.opt {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.olabel {
  font-size: 12px;
  color: var(--cc-fg2);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.chips.col {
  flex-direction: column;
  align-items: stretch;
}
.chip {
  padding: 5px 12px;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  background: var(--cc-fill);
  color: var(--cc-fg);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.chip small {
  color: var(--cc-mute);
}
.chip.on {
  border-color: var(--cc-accent, #2457e6);
  background: var(--cc-accent-soft, rgba(36, 87, 230, 0.1));
  font-weight: 600;
}
.chip.lay {
  display: flex;
  justify-content: space-between;
  align-items: center;
  text-align: left;
}
.chip.lay .mono {
  font-size: 12px;
  color: var(--cc-mute);
}
.chk {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
}
.fmt {
  width: 90px;
}
.fields {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}
.fld {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  font-size: 12px;
  color: var(--cc-fg2);
}
.fld b {
  display: flex;
  align-items: center;
  height: 36px;
  padding: 0 12px;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
  background: var(--cc-fill);
  font-size: 13px;
  font-weight: 400;
  color: var(--cc-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 底部操作栏：内容长时停在窗口底部 */
.foot {
  position: sticky;
  bottom: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px 20px;
  padding: 16px 24px;
  border-top: 1px solid var(--cc-line);
  background: var(--cc-foot);
}
.sum {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1 1 280px;
}
.sum b {
  font-weight: 600;
}
.where {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}
.where .path {
  max-width: 420px;
}
.where .pz-link {
  margin-left: 4px;
  flex: none;
}
.big {
  min-width: 148px;
  height: 44px;
  font-size: 14px;
  gap: 8px;
}
.big svg {
  margin-right: 6px;
}

/* 最近解析、空状态 */
.recent {
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.rhead {
  margin-bottom: 6px;
}
.ritem {
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr) auto;
  align-items: center;
  gap: 12px;
  min-height: 48px;
  padding: 6px 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font: inherit;
  color: inherit;
  text-align: left;
  cursor: pointer;
}
.ritem:hover {
  background: var(--cc-hover);
}
.ritem .thumb {
  width: 64px;
  height: 36px;
  border-radius: 4px;
}
.empty {
  padding: 24px 28px;
}
.empty h3 {
  margin: 0 0 10px;
  font-size: 15px;
}
.empty ol {
  margin: 0;
  padding-left: 20px;
  color: var(--cc-fg2);
  line-height: 1.9;
}

/* 预览 */
.preview audio {
  width: 100%;
}
.preview video,
.preview img {
  width: 100%;
  max-height: 460px;
  object-fit: contain;
  background: #000;
  display: block;
}

@media (max-width: 860px) {
  .work {
    max-width: none;
    position: static;
  }
  .grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  .fields {
    grid-template-columns: minmax(0, 1fr);
  }
  .frow {
    grid-template-columns: 20px minmax(0, 1fr) 72px;
  }
  .frow .spec {
    display: none;
  }
}
.modes {
  display: flex;
  align-items: center;
  gap: 4px;
}
.modes button {
  border: 0;
  background: var(--cc-line);
  color: var(--cc-mute);
  padding: 7px 18px;
  font-size: 13.5px;
  font-weight: 600;
  cursor: pointer;
}
.modes button:first-child {
  border-radius: 10px 0 0 10px;
}
.modes button:nth-child(2) {
  border-radius: 0 10px 10px 0;
  margin-left: -4px;
}
.modes button.on {
  background: var(--cc-card);
  color: var(--cc-fg);
  box-shadow: 0 0 0 1px var(--cc-line), 0 1px 3px rgba(0, 0, 0, 0.08);
  position: relative;
  z-index: 1;
}
.modes-hint {
  margin-left: 10px;
  font-size: 12.5px;
}
.bpaste {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}
.bpaste :deep(.el-textarea) {
  flex: 1;
}
.bimp {
  height: 54px;
}
</style>
