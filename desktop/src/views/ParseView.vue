<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import { useAppStore, useParseStore } from '../stores/app'
import type { Asset, BatchProgress, Clip, DetectedLink, HistoryItem, MediaInfo } from '../types'
import { formatBytes, formatDate, formatDuration } from '../utils/format'
import ErrorAlert from '../components/ErrorAlert.vue'
import SubtitlePicker, { type SubMode } from '../components/SubtitlePicker.vue'
import ClipPicker from '../components/ClipPicker.vue'
import PlaylistPicker from '../components/PlaylistPicker.vue'

const app = useAppStore()
const parse = useParseStore()

const selected = ref<Set<string>>(new Set())
const links = ref<DetectedLink[]>([])
const batchRunning = ref(false)
const batchDone = ref(0)
const preview = ref(false)
const enqueuing = ref(false)
const audioOnly = ref(false)
const slideOpen = ref(false)
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
    slideOpen.value = false
    ElMessage.success({ message: `已生成：${path}`, duration: 5000 })
    api.revealFile(path).catch(() => {})
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    sliding.value = false
  }
}
const entrySel = ref<Set<string>>(new Set())
const downloadedEntries = ref<Set<string>>(new Set())
const subMode = ref<SubMode>('file')
const preferredSubs = ref<string[]>([])
const clip = ref<Clip | null>(null)
const splitOn = ref(false)
const batchInfo = ref<BatchProgress | null>(null)
let unlistenBatch: UnlistenFn | undefined

const result = computed(() => parse.result)
const images = computed(() => result.value?.assets.filter((a) => a.kind === 'image') ?? [])
const videoFormats = computed(() => result.value?.assets.filter((a) => a.kind === 'video') ?? [])
const subtitleAssets = computed(() => result.value?.assets.filter((a) => a.kind === 'subtitle') ?? [])
const extras = computed(() => result.value?.assets.filter((a) => a.kind !== 'image' && a.kind !== 'video' && a.kind !== 'subtitle') ?? [])
const videoSelected = computed(() => videoFormats.value.some((v) => selected.value.has(v.id)))
const canClip = computed(() => !!result.value && !isPlaylist.value && (result.value.kind === 'video' || result.value.kind === 'audio') && !audioOnly.value)
const videoAsset = computed(() => result.value?.assets.find((a) => a.kind === 'video' && a.protocol === 'http'))
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
    audioOnly.value = false
    if (r && r.kind === 'video' && app.settings?.qualityPreset === 'audio') {
      // “只要音频”：优先选单独的音频轨，没有时下载视频后提取音频
      const audio = bestAudio(r)
      if (audio) selected.value = new Set([audio.id])
      else audioOnly.value = true
    }
  },
  { immediate: true },
)

let detectTimer: number | undefined
watch(
  () => parse.text,
  (t) => {
    window.clearTimeout(detectTimer)
    detectTimer = window.setTimeout(async () => {
      links.value = t.trim() ? await api.detectLinks(t) : []
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

async function downloadEntries() {
  if (!result.value) return
  const ids = entries.value.filter((e) => entrySel.value.has(e.id)).map((e) => e.id)
  if (!ids.length) {
    ElMessage.warning('请至少选择一个条目。')
    return
  }
  enqueuing.value = true
  try {
    const n = await api.enqueueEntries(result.value, ids)
    ElMessage.success(`正在逐条解析 ${n} 个条目并加入队列…`)
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
      if (p.failed.length) ElMessage.warning({ message: `“${p.title}”：已加入 ${p.queued} 项${skipped}，${p.failed.length} 条失败。${p.failed.slice(0, 3).join('；')}`, duration: 8000 })
      else if (p.skipped) ElMessage.success(`“${p.title}”：已加入 ${p.queued} 项${skipped}`)
      else ElMessage.success(`“${p.title}”：${p.total} 条已全部加入下载队列`)
      window.setTimeout(() => (batchInfo.value = null), 1500)
    }
  })
})
onUnmounted(() => unlistenBatch?.())

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

function formatMeta(a: Asset) {
  const parts: string[] = []
  if (a.width && a.height) parts.push(`${a.width}×${a.height}`)
  const codec = prettyCodec(a.vcodec)
  if (codec && !a.label.includes(codec)) parts.push(codec)
  if (a.fps) parts.push(`${a.fps}fps`)
  if (a.filesize) parts.push(formatBytes(a.filesize))
  else if (a.bitrate) parts.push(`${a.bitrate} kbps`)
  if (a.hasAudio === false) parts.push('需合并音频')
  if (a.protocol === 'hls') parts.push('M3U8')
  if (a.protocol === 'ytdlp') parts.push('yt-dlp 下载')
  return parts.join(' · ') || a.ext.toUpperCase()
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
  const ids = result.value.assets.filter((a) => selected.value.has(a.id)).map((a) => a.id)
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

async function batch() {
  batchRunning.value = true
  batchDone.value = 0
  let failed = 0
  for (const l of links.value) {
    try {
      await api.resolveAndEnqueue(l.url)
    } catch {
      failed++
    }
    batchDone.value++
  }
  batchRunning.value = false
  await parse.loadRecent()
  if (failed) ElMessage.warning(`已加入 ${links.value.length - failed} 条，${failed} 条解析失败`)
  else ElMessage.success(`${links.value.length} 条链接已全部加入下载队列`)
  app.view = 'queue'
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

function sizeText(a: Asset) {
  if (a.width && a.height) return `${a.width}×${a.height}`
  return a.ext.toUpperCase()
}
</script>

<template>
  <div class="page">
    <div class="paste">
      <el-input
        v-model="parse.text"
        type="textarea"
        :autosize="{ minRows: 1, maxRows: 4 }"
        resize="none"
        placeholder="粘贴分享文案或任意视频网页链接（抖音、B站、YouTube…），按 Enter 解析"
        class="paste-input"
        @keydown="onKey"
      />
      <el-button type="primary" size="large" :loading="parse.loading" @click="parse.parse()">解析</el-button>
      <el-button v-if="parse.text || parse.result" size="large" @click="parse.clear()">清空</el-button>
    </div>
    <div class="hint">
      <span v-if="app.settings?.watchClipboard" class="ok">● 剪贴板监听已开启</span>
      <span v-else>剪贴板监听已关闭</span>
      <span>支持：{{ supported }}</span>
      <span>Enter 解析 · Shift+Enter 换行</span>
      <span v-if="app.settings?.shortcut">全局快捷键 {{ app.settings.shortcut.replace('CommandOrControl', 'Ctrl') }}</span>
    </div>

    <div v-if="links.length > 1" class="batch card">
      <span>检测到 <b>{{ links.length }}</b> 条链接</span>
      <span class="mute">批量模式会按默认选项（视频 / 全部图片）直接加入下载队列。</span>
      <el-button type="primary" plain :loading="batchRunning" @click="batch">
        {{ batchRunning ? `处理中 ${batchDone}/${links.length}` : '全部解析并下载' }}
      </el-button>
    </div>

    <ErrorAlert v-if="parse.error" :message="parse.error" :kind="parse.errorKind" :site="parse.errorSite" />

    <div v-if="parse.loading && !result" class="result card skeleton">
      <el-skeleton animated :rows="5" />
    </div>

    <section v-if="result" class="result card">
      <div class="cover-wrap" @click="preview = true">
        <img v-if="result.cover" :src="result.cover" class="thumb cover" referrerpolicy="no-referrer" alt="" />
        <div v-else class="thumb cover" />
        <span v-if="result.kind === 'video'" class="play">▶</span>
        <span v-if="result.durationMs" class="dur mono">{{ formatDuration(result.durationMs) }}</span>
      </div>
      <div class="meta">
        <div class="chips">
          <span class="chip acc">{{ result.platformName }} · {{ kindText }}</span>
          <span v-if="result.extractor === 'native' || !result.extractor" class="chip">无水印</span>
          <span v-else-if="result.extractor.startsWith('yt-dlp')" class="chip">yt-dlp 解析</span>
          <span v-else-if="result.extractor === 'generic'" class="chip">网页嗅探</span>
          <span v-if="result.width && result.height" class="chip">{{ result.width }}×{{ result.height }}</span>
        </div>
        <h3 class="title selectable">{{ result.title }}</h3>
        <div class="mute">
          <span v-if="result.author">@{{ result.author }}</span>
          <span v-if="result.publishedAt">{{ result.author ? ' · ' : '' }}{{ formatDate(result.publishedAt) }} 发布</span>
        </div>

        <div v-if="images.length" class="grid-head">
          <span>已选 {{ selectedImages }} / {{ images.length }} 张</span>
          <el-button link type="primary" @click="selectAllImages(selectedImages < images.length)">
            {{ selectedImages < images.length ? '全选' : '全不选' }}
          </el-button>
        </div>
        <div v-if="images.length" class="grid">
          <button
            v-for="img in images"
            :key="img.id"
            type="button"
            class="ph"
            :class="{ sel: selected.has(img.id) }"
            :aria-pressed="selected.has(img.id)"
            :aria-label="img.label"
            @click="toggle(img.id)"
          >
            <img :src="img.url" referrerpolicy="no-referrer" loading="lazy" alt="" />
            <span class="no mono">{{ (img.index ?? 0) + 1 }}</span>
          </button>
        </div>

        <div v-if="videoFormats.length" class="fmt-head">{{ videoFormats.length > 1 ? `清晰度（${videoFormats.length} 种）` : '视频' }}</div>
        <div v-if="videoFormats.length" class="formats" role="radiogroup">
          <button
            v-for="v in videoFormats"
            :key="v.id"
            type="button"
            role="radio"
            class="opt fmt"
            :class="{ on: selected.has(v.id) }"
            :aria-checked="selected.has(v.id)"
            @click="pickVideo(v.id)"
          >
            <span>{{ v.label }}</span>
            <small class="mono">{{ formatMeta(v) }}</small>
          </button>
        </div>
        <div v-if="extras.length" class="fmt-head">其他内容</div>
        <div class="opts">
          <button
            v-for="a in extras"
            :key="a.id"
            type="button"
            class="opt"
            :class="{ on: selected.has(a.id) }"
            :aria-pressed="selected.has(a.id)"
            @click="toggle(a.id)"
          >
            <span>{{ a.label }}</span>
            <small class="mono">{{ sizeText(a) }}</small>
          </button>
        </div>

        <SubtitlePicker
          v-if="subtitleAssets.length && !isPlaylist"
          v-model:mode="subMode"
          :assets="subtitleAssets"
          :selected="selected"
          :has-video="videoSelected"
          :preferred="preferredSubs"
          @toggle="toggle"
          @pick="pickSubs"
        />
        <ClipPicker v-if="canClip" v-model:clip="clip" v-model:split="splitOn" :duration-ms="result.durationMs" :chapters="result.chapters ?? []" />

        <template v-if="isPlaylist">
          <PlaylistPicker v-model:selected="entrySel" :entries="entries" :downloaded="downloadedEntries" />
          <small class="mute">每个条目会按设置里的默认清晰度下载（{{ app.settings?.subtitleMode === 'off' ? '不含字幕' : '同时处理偏好语言的字幕' }}），保存到以列表名命名的文件夹。</small>
          <div class="row">
            <el-button type="primary" :loading="enqueuing" :disabled="!!batchInfo && !batchInfo.finished" @click="downloadEntries">
              {{ batchInfo && !batchInfo.finished ? `解析中 ${batchInfo.done}/${batchInfo.total}` : `下载所选 ${entrySel.size} 条` }}
            </el-button>
          </div>
        </template>

        <div v-if="!isPlaylist && result.kind !== 'images'" class="kv-line">
          <el-checkbox v-model="audioOnly">只保留音频（下载后转为 {{ (app.settings?.audioFormat ?? 'mp3').toUpperCase() }}）</el-checkbox>
        </div>

        <div v-if="!isPlaylist" class="row">
          <el-button type="primary" :loading="enqueuing" @click="download">下载所选</el-button>
          <el-button @click="copyLinks">复制直链</el-button>
          <el-button v-if="images.length > 1" @click="slideOpen = true">合成视频</el-button>
          <el-button @click="preview = true">预览</el-button>
        </div>
      </div>
    </section>

    <section v-if="!result && !parse.loading && parse.recent.length" class="recent">
      <div class="label">最近解析</div>
      <button v-for="r in parse.recent" :key="r.id" type="button" class="ritem" @click="openRecent(r)">
        <img v-if="r.cover" :src="r.cover" class="thumb" referrerpolicy="no-referrer" alt="" />
        <div v-else class="thumb" />
        <span class="ellipsis">{{ r.title }}</span>
        <small class="mono mute">{{ r.info.platformName }} · {{ recentKind(r) }}</small>
      </button>
    </section>

    <section v-if="!result && !parse.loading && !parse.recent.length && !parse.error" class="empty">
      <h3>复制分享链接，就能保存无水印作品</h3>
      <ol>
        <li>在 App 里点“分享 → 复制链接”。</li>
        <li>回到这里粘贴（开启剪贴板监听后会自动识别）。</li>
        <li>选择视频、图片或背景音乐，点击“下载所选”。</li>
      </ol>
    </section>

    <el-dialog v-model="slideOpen" title="图集合成视频" width="400px" append-to-body>
      <p class="mute">把选中的 {{ selectedImages }} 张图片合成一个视频（需要 ffmpeg），保存在下载目录。</p>
      <div class="slide-row"><span>每张显示</span><el-input-number v-model="slideSecs" :min="0.5" :max="30" :step="0.5" size="small" /><span>秒</span></div>
      <el-checkbox v-if="musicAsset" v-model="slideMusic">配上背景音乐</el-checkbox>
      <template #footer>
        <el-button @click="slideOpen = false">取消</el-button>
        <el-button type="primary" :loading="sliding" @click="makeSlideshow">生成</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="preview" :title="result?.title" width="720px" align-center destroy-on-close>
      <div v-if="result" class="preview">
        <video v-if="videoAsset" :src="videoAsset.url" controls autoplay referrerpolicy="no-referrer" />
        <el-carousel v-else-if="images.length" height="460px" indicator-position="outside" :autoplay="false">
          <el-carousel-item v-for="img in images" :key="img.id">
            <img :src="img.url" referrerpolicy="no-referrer" alt="" />
          </el-carousel-item>
        </el-carousel>
      </div>
    </el-dialog>
  </div>
</template>

<style scoped>
.slide-row {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}
.entries {
  max-height: 320px;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
}
.entry {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 10px;
  border-top: 1px dashed var(--cc-line);
  cursor: pointer;
  font-size: 12.5px;
}
.entry:first-child {
  border-top: 0;
}
.entry .idx {
  width: 28px;
  text-align: right;
}
.entry .grow {
  flex: 1;
  min-width: 0;
}
.kv-line {
  margin-top: 4px;
}
.page {
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 980px;
}
.paste {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}
.paste-input {
  flex: 1;
  min-width: 0;
}
.paste-input :deep(textarea) {
  font-family: var(--cc-mono);
  font-size: 12.5px;
  padding: 9px 12px;
  min-height: 40px !important;
}
.paste :deep(.el-button) {
  margin-left: 0;
}
.hint {
  font-size: 11.5px;
  color: var(--cc-mute);
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
  margin-top: -4px;
}
.hint .ok {
  color: var(--cc-ok);
}
.batch {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  flex-wrap: wrap;
}
.batch .el-button {
  margin-left: auto;
}
.result {
  display: grid;
  grid-template-columns: 168px 1fr;
  gap: 18px;
  padding: 16px;
}
.skeleton {
  display: block;
}
.cover-wrap {
  position: relative;
  cursor: pointer;
  align-self: start;
}
.cover {
  width: 168px;
  aspect-ratio: 9 / 16;
  display: block;
}
.play {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  font-size: 28px;
  color: #fffd;
  text-shadow: 0 2px 8px #0008;
}
.dur {
  position: absolute;
  right: 6px;
  bottom: 6px;
  font-size: 10.5px;
  background: #000a;
  color: #fff;
  padding: 0 5px;
  border-radius: 3px;
}
.meta {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.chips {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.title {
  margin: 0;
  font-size: 15px;
  line-height: 1.45;
  font-weight: 600;
  overflow-wrap: anywhere;
}
.grid-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
  color: var(--cc-mute);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(92px, 1fr));
  gap: 8px;
  max-height: 320px;
  overflow: auto;
  padding: 2px;
}
.ph {
  all: unset;
  cursor: pointer;
  position: relative;
  aspect-ratio: 3 / 4;
  border-radius: 6px;
  overflow: hidden;
  border: 2px solid transparent;
  background: var(--cc-side);
}
.ph img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  opacity: 0.55;
}
.ph.sel {
  border-color: var(--cc-acc);
}
.ph.sel img {
  opacity: 1;
}
.ph.sel::after {
  content: '✓';
  position: absolute;
  top: 5px;
  right: 5px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--cc-acc);
  color: #1a1208;
  font-size: 11px;
  display: grid;
  place-items: center;
  font-weight: 700;
}
.ph .no {
  position: absolute;
  left: 5px;
  bottom: 4px;
  font-size: 10px;
  color: #fff;
  text-shadow: 0 1px 3px #000;
}
.ph:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.fmt-head {
  font-size: 11.5px;
  color: var(--cc-mute);
  margin-bottom: -4px;
}
.formats {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 8px;
}
.opts {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
}
.opt {
  all: unset;
  cursor: pointer;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  font-size: 12.5px;
}
.opt small {
  color: var(--cc-mute);
  font-size: 10.5px;
}
.opt.on {
  border-color: var(--cc-acc);
  background: var(--cc-acc-soft);
}
.opt:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.recent {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.recent .label {
  font-size: 11px;
  color: var(--cc-mute);
  letter-spacing: 0.08em;
}
.ritem {
  all: unset;
  cursor: pointer;
  display: grid;
  grid-template-columns: 30px 1fr auto;
  gap: 12px;
  align-items: center;
  padding: 7px 10px;
  border-radius: 7px;
  background: var(--cc-card);
  border: 1px solid var(--cc-line);
}
.ritem:hover {
  border-color: var(--cc-acc);
}
.ritem .thumb {
  width: 30px;
  height: 40px;
}
.empty {
  padding: 28px 4px;
  color: var(--cc-mute);
}
.empty h3 {
  color: var(--cc-fg);
  margin: 0 0 8px;
  font-size: 15px;
}
.empty ol {
  padding-left: 18px;
  margin: 0;
  line-height: 1.9;
}
.preview video,
.preview img {
  width: 100%;
  max-height: 460px;
  object-fit: contain;
  background: #000;
  display: block;
}
</style>
