<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { CapsSummary, ColorFlaggedShot, ColorReport, NormPreset, NormSpec, SkipSpan, ToneAdjust, VideoFactsInfo, VideoPreview, VideoReport } from '../../types'
import { formatClock, parseClock } from '../../utils/time'
import FileInput from './FileInput.vue'
import ToneSliders from './ToneSliders.vue'

const props = defineProps<{ preset?: string[]; hint?: string; lut?: string }>()
const emit = defineEmits<{ started: [] }>()
const app = useAppStore()

const files = ref<string[]>(props.preset ? [...props.preset] : [])
const caps = ref<CapsSummary | null>(null)
const presets = ref<NormPreset[]>([])
const presetId = ref('compat')
const noTone = (): ToneAdjust => ({ brightness: 0, contrast: 0, saturation: 0, temperature: 0, tint: 0 })
const spec = reactive<NormSpec>({
  size: 'limit', width: 1920, height: 1080, shortSide: 1080, followOrientation: true, fps: 'auto', fpsValue: 30, hdr: true, fixColor: true, levels: 0, outRange: 'tv', denoise: 'off',
  matchColor: 0, matchTone: noTone(), matchShots: [], matchSkip: [], lut: null,
  lutStrength: 1, autocrop: false, audioRate: 0, audioChannels: 0, loudness: null, fixSync: true, codec: 'h264', quality: 'balanced',
})
const reports = ref<Record<string, VideoReport | { error: string }>>({})
const analyzing = ref(false)
const showAdvanced = ref(false)
const outDir = ref('')
const busy = ref(false)
const stab = ref<'light' | 'normal' | 'strong'>('normal')
const stabBusy = ref(false)
const previewAt = ref('0:02')
const preview = ref<VideoPreview | null>(null)
const previewing = ref(false)
const colorReport = ref<ColorReport | null>(null)
const colorBusy = ref(false)
let token = 0
let colorSeq = 0
const isReport = (r: VideoReport | { error: string } | undefined): r is VideoReport => !!r && 'facts' in r

// 排除的时间段 / 帧区间：按文件时间记，只对第一个文件有效（勾选“所有文件”时对每个文件都用）
interface SkipRow {
  id: number
  mode: 'time' | 'frame'
  a: string
  b: string
}
let skipSeq = 0
const skipRows = ref<SkipRow[]>([])
const skipAll = ref(false)

// 换了第一个文件：旧的色彩检测结果、单独调节的镜头和排除的时间都作废（统一调节不跟文件走，保留）
watch(
  () => files.value[0],
  () => {
    colorSeq++
    colorBusy.value = false
    colorReport.value = null
    spec.matchShots = []
    skipRows.value = []
  },
)

const firstFacts = computed(() => {
  const r = reports.value[files.value[0]]
  return isReport(r) ? r.facts : null
})

/** 把毫秒写成能被 parseClock 原样读回的样子（不丢掉毫秒）。 */
function clockExact(ms: number): string {
  const rest = ms % 1000
  const base = formatClock(ms - rest)
  return rest ? `${base}.${String(rest).padStart(3, '0').replace(/0+$/, '')}` : base
}

interface SkipInfo {
  span: SkipSpan | null
  /** 写错了：显示在这一行，这一行不参与 */
  error: string
  /** 换算后的结果，给用户核对 */
  text: string
}

/** 一行 → 毫秒区间。帧号从 1 开始、含首尾两帧；`fps` 是这个文件的帧率。 */
function skipOf(r: SkipRow, fps: number | null, durationMs: number | null): SkipInfo {
  const a = r.a.trim()
  const b = r.b.trim()
  const none = (error = '', text = ''): SkipInfo => ({ span: null, error, text })
  if (!a && !b) return none()
  let start: number
  let end: number | null
  if (r.mode === 'time') {
    const s = a ? parseClock(a) : 0
    const e = b ? parseClock(b) : null
    if (s === null || (b && e === null)) return none('时间的写法不对，请用 5、0:05 或 1:02:03.5 这样的格式。')
    start = s
    end = e
  } else {
    if (!fps) return none('读不出这个视频的帧率，帧区间用不了，请改用时间。')
    const fa = a ? Number(a) : 1
    const fb = b ? Number(b) : null
    if (!Number.isInteger(fa) || fa < 1 || (fb !== null && (!Number.isInteger(fb) || fb < 1))) return none('帧号要填整数，从 1 开始。')
    start = Math.round(((fa - 1) / fps) * 1000)
    end = fb === null ? null : Math.round((fb / fps) * 1000)
  }
  if (end !== null && end <= start) return none('结束要晚于开始。')
  if (durationMs !== null && start >= durationMs) return none(`开始已经超出视频长度（${formatClock(durationMs)}）。`)
  const text = r.mode === 'frame' ? `= ${clockExact(start)} – ${end === null ? '结尾' : clockExact(end)}` : ''
  return { span: { startMs: start, endMs: end }, error: '', text }
}

const skipInfos = computed(() => skipRows.value.map((r) => skipOf(r, firstFacts.value?.video?.fps ?? null, firstFacts.value?.durationMs ?? null)))
const skipSpans = computed(() => skipInfos.value.flatMap((i) => (i.span ? [i.span] : [])))
const vfrFrames = computed(() => {
  const r = reports.value[files.value[0]]
  return isReport(r) && !!r.analysis.vfr?.variable && skipRows.value.some((x) => x.mode === 'frame')
})

/** 某个文件要排除的时间段：帧区间按这个文件自己的帧率换算。 */
function skipFor(file: string): SkipSpan[] {
  const r = reports.value[file]
  const facts = isReport(r) ? r.facts : null
  return skipRows.value.flatMap((x) => {
    const sp = skipOf(x, facts?.video?.fps ?? null, facts?.durationMs ?? null).span
    return sp ? [sp] : []
  })
}

function addSkip(mode: SkipRow['mode'] = 'time', a = '', b = '') {
  skipRows.value.push({ id: ++skipSeq, mode, a, b })
}
function removeSkip(id: number) {
  skipRows.value = skipRows.value.filter((x) => x.id !== id)
}
const skipBar = (a: number, b: number) => {
  const total = Math.max(1, colorReport.value?.durationMs ?? 1)
  return { left: `${(a / total) * 100}%`, width: `${Math.max(0.6, ((b - a) / total) * 100)}%` }
}

/** 把一个检测出来的镜头加进排除列表（不一致的镜头不想校正时用）。 */
function excludeShot(s: ColorFlaggedShot) {
  addSkip('time', clockExact(s.startMs), clockExact(s.endMs))
}

// 预览和处理用的排除时间跟着输入走；改了之后，已经检测过的结果自动重新检测
let redetect: ReturnType<typeof setTimeout> | undefined
watch(
  () => JSON.stringify(skipSpans.value),
  (json) => {
    spec.matchSkip = JSON.parse(json) as SkipSpan[]
    if (!colorReport.value) return
    clearTimeout(redetect)
    redetect = setTimeout(() => void checkColor(), 700)
  },
)

async function checkColor() {
  const f = files.value[0]
  if (!f) return
  const my = ++colorSeq
  colorBusy.value = true
  try {
    const rep = await api.videoColor(f, spec.matchSkip)
    if (my !== colorSeq) return // 排除的时间又改了，这次的结果已经过时
    colorReport.value = rep
    // 重新检测后，已经不在结果里的镜头不再单独调节
    const ids = new Set(rep.flagged.map((x) => x.id))
    spec.matchShots = spec.matchShots.filter((x) => ids.has(x.id))
  } catch (e) {
    if (my !== colorSeq) return
    colorReport.value = null
    ElMessage.error(errorText(e))
  } finally {
    if (my === colorSeq) colorBusy.value = false
  }
}

/** 打开 / 关闭某个镜头的单独调节：关闭后这个镜头跟着统一的参数走。 */
function toggleOwn(id: number, on: boolean) {
  const rest = spec.matchShots.filter((x) => x.id !== id)
  spec.matchShots = on ? [...rest, { id, strength: spec.matchColor, tone: noTone() }] : rest
}
const isOwn = (id: number) => spec.matchShots.some((x) => x.id === id)
const ownOf = (id: number) => spec.matchShots.filter((x) => x.id === id)
const pctText = (v: number) => `${Math.round(v * 100)}%`

/** 看某个镜头校正前后的对比：取镜头中间的一帧。 */
function previewShot(s: ColorFlaggedShot) {
  previewAt.value = formatClock(Math.round((s.startMs + s.endMs) / 2))
  void makePreview()
}

const segPercent = (a: number, b: number) => `${Math.max(0.4, ((b - a) / Math.max(1, colorReport.value?.durationMs ?? 1)) * 100)}%`

watch(
  () => props.preset,
  (p) => p && (files.value = [...p]),
)

// 从 LUT 工作室带过来的 LUT
watch(
  () => props.lut,
  (l) => {
    if (l) {
      spec.lut = l
      spec.lutStrength = 1
    }
  },
  { immediate: true },
)

function applyPreset(p: NormPreset) {
  presetId.value = p.id
  // 色阶、LUT、输出电平、降噪和手动调节是个人的选择，不跟着预设走
  Object.assign(spec, p.spec, {
    lut: spec.lut,
    lutStrength: spec.lutStrength,
    levels: spec.levels,
    outRange: spec.outRange,
    denoise: spec.denoise,
    matchTone: spec.matchTone,
    matchShots: spec.matchShots,
    matchSkip: spec.matchSkip,
  })
}

onMounted(async () => {
  presets.value = await api.videoPresets().catch(() => [])
  caps.value = await api.videoCaps().catch(() => null)
  const first = presets.value.find((p) => p.id === presetId.value)
  if (first) applyPreset(first)
})


/** 分析选中的文件（逐个，避免同时跑太多 ffmpeg）；按多数文件的推荐自动选预设。 */
watch(
  files,
  async (list) => {
    const my = ++token
    preview.value = null
    const next: Record<string, VideoReport | { error: string }> = {}
    for (const f of list) if (reports.value[f]) next[f] = reports.value[f]
    reports.value = next
    analyzing.value = true
    for (const f of list) {
      if (reports.value[f]) continue
      try {
        reports.value[f] = await api.videoAnalyze(f, props.hint)
      } catch (e) {
        reports.value[f] = { error: errorText(e) }
      }
      if (my !== token) return
    }
    analyzing.value = false
    const recs = list.map((f) => reports.value[f]).filter(isReport).map((r) => r.recommended)
    if (recs.length) {
      const top = [...new Set(recs)].sort((a, b) => recs.filter((x) => x === b).length - recs.filter((x) => x === a).length)[0]
      const p = presets.value.find((x) => x.id === top)
      if (p) applyPreset(p)
    }
  },
  { immediate: true },
)

const recommended = computed(() => {
  const recs = files.value.map((f) => reports.value[f]).filter(isReport).map((r) => r.recommended)
  return recs.length ? [...new Set(recs)].sort((a, b) => recs.filter((x) => x === b).length - recs.filter((x) => x === a).length)[0] : ''
})

const name = (p: string) => p.split(/[\\/]/).pop() ?? p
const CH: Record<number, string> = { 1: '单声道', 2: '立体声', 6: '5.1 声道', 8: '7.1 声道' }

function summary(f: VideoFactsInfo): string {
  const v = f.video
  if (!v) return ''
  const swap = v.rotation === 90 || v.rotation === 270
  const parts = [`${swap ? v.height : v.width}×${swap ? v.width : v.height}`, v.codec.toUpperCase()]
  if (v.fps) parts.push(`${+v.fps.toFixed(2)} 帧`)
  if (v.hdr !== 'none') parts.push(v.hdr === 'hlg' ? 'HLG' : 'HDR10')
  else if (v.bitDepth > 8) parts.push(`${v.bitDepth} 位`)
  if (f.audio) parts.push(`${+(f.audio.sampleRate / 1000).toFixed(1)} kHz ${CH[f.audio.channels] ?? `${f.audio.channels} 声道`}`)
  else parts.push('没有音轨')
  return parts.join(' · ')
}

const presetName = (id: string) => presets.value.find((p) => p.id === id)?.name ?? id
const curPreset = computed(() => presets.value.find((p) => p.id === presetId.value))

const DENOISE_NOTES: Record<NormSpec['denoise'], string> = {
  off: '',
  light: '去掉轻微的噪点：白天拍摄的素材、轻度压缩产生的颗粒。',
  medium: '夜景、室内暗光、高 ISO 的素材常用这一档。',
  strong: '噪点很重时用。画面会更柔和，毛发、纹理这些细节可能被抹掉一些。',
  best: '非局部均值算法，画质最好，但处理速度只有其他档位的四分之一左右，适合需要认真处理的短素材。',
}

const loudnessOn = computed({
  get: () => spec.loudness !== null,
  set: (v: boolean) => (spec.loudness = v ? -16 : null),
})

const BOXES = [
  { v: '1920x1080', t: '1920×1080（横屏 1080p）' },
  { v: '1280x720', t: '1280×720（横屏 720p）' },
  { v: '3840x2160', t: '3840×2160（横屏 4K）' },
  { v: '1080x1920', t: '1080×1920（竖屏 1080p）' },
  { v: '720x1280', t: '720×1280（竖屏 720p）' },
]
const box = computed({
  get: () => `${spec.width}x${spec.height}`,
  set: (v: string) => {
    const [w, h] = v.split('x').map(Number)
    spec.width = w
    spec.height = h
  },
})
const FPS_VALUES = [23.976, 24, 25, 29.97, 30, 50, 59.94, 60]

async function pickLut() {
  const r = await open({ multiple: false, filters: [{ name: '3D LUT', extensions: ['cube'] }] })
  if (typeof r === 'string') spec.lut = r
}

async function pickDir() {
  const d = await open({ directory: true, multiple: false })
  if (typeof d === 'string') outDir.value = d
}

async function makePreview() {
  const f = files.value[0]
  if (!f) return
  const ms = parseClock(previewAt.value)
  if (ms === null) return ElMessage.warning('时间点的写法不对，请用 5、0:05 或 1:02:03 这样的格式。')
  previewing.value = true
  try {
    preview.value = await api.videoPreview(f, { ...spec }, ms)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    previewing.value = false
  }
}

async function start() {
  if (!files.value.length) return ElMessage.warning('请先选择文件。')
  busy.value = true
  try {
    for (const f of files.value) {
      // 单独调节的镜头和排除的时间是按第一个文件记的，别的文件用不上（排除的时间可以选择对所有文件生效）；统一调节对每个文件都有效
      const first = f === files.value[0]
      const own = {
        ...spec,
        matchShots: first ? spec.matchShots.map((x) => ({ ...x, tone: { ...x.tone } })) : [],
        matchSkip: first || skipAll.value ? skipFor(f) : [],
      }
      await api.mediaJobStart({ op: 'normalize', spec: own, preset: presetId.value, inputs: [f], outputDir: outDir.value || null })
    }
    ElMessage.success(files.value.length > 1 ? `已加入 ${files.value.length} 个任务，可以在下方查看进度。` : '已开始处理，可以在下方查看进度。')
    emit('started')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

async function stabilize() {
  if (!files.value.length) return ElMessage.warning('请先选择文件。')
  stabBusy.value = true
  try {
    for (const f of files.value) await api.mediaJobStart({ op: 'stabilize', strength: stab.value, inputs: [f], outputDir: outDir.value || null })
    ElMessage.success('已开始防抖，可以在下方查看进度。')
    emit('started')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    stabBusy.value = false
  }
}

const stabNote = computed(() => {
  switch (caps.value?.stabilize) {
    case 'vidstab':
      return '使用 vidstab 两遍防抖：先分析抖动，再平滑并适当放大裁掉边缘，效果好，耗时约为视频长度的 1–2 倍。'
    case 'deshake':
      return caps.value.edition === 'full' ? '当前的 ffmpeg 没有 vidstab，会改用 deshake，效果较弱。' : '当前的 ffmpeg 没有 vidstab，会改用 deshake，效果较弱。在“设置 → 组件”里安装完整版 ffmpeg 可以使用 vidstab。'
    case 'none':
      return caps.value.edition === 'full' ? '当前的 ffmpeg 没有防抖滤镜（vidstab、deshake）。' : '当前的 ffmpeg 没有防抖滤镜。在“设置 → 组件”里安装完整版 ffmpeg 后可以使用。'
    default:
      return ''
  }
})

const imgSrc = (p: string) => convertFileSrc(p)
</script>

<template>
  <div class="nt">
    <div class="card box">
      <h3>视频规整</h3>
      <p class="mute desc">把手机录屏、直播录制、平台下载的成片统一成同一种规格：画面尺寸、固定帧率、色彩（HDR 转 SDR、LUT、自动色阶）、黑边、声音响度。不会修改原文件，处理结果会生成新文件。</p>
      <FileInput v-model="files" multiple :kinds="['video']" :extensions="['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'ts', 'm4v']" label="视频" />

      <div v-if="files.length" class="found">
        <div v-for="f in files" :key="f" class="file">
          <div class="fn ellipsis selectable" :title="f">{{ name(f) }}</div>
          <div v-if="!reports[f]" class="mute small">检测中…</div>
          <div v-else-if="!isReport(reports[f])" class="err small">{{ (reports[f] as { error: string }).error }}</div>
          <template v-else>
            <div class="mute small mono">{{ summary((reports[f] as VideoReport).facts) }}</div>
            <div class="chips">
              <el-tooltip v-for="i in (reports[f] as VideoReport).issues" :key="i.id" :content="i.detail || i.label" placement="top" :disabled="!i.detail">
                <span class="chip" :class="i.level">{{ i.label }}</span>
              </el-tooltip>
              <span v-if="!(reports[f] as VideoReport).issues.length" class="chip ok">没发现问题</span>
              <span class="rec">推荐：{{ presetName((reports[f] as VideoReport).recommended) }}</span>
            </div>
          </template>
        </div>
      </div>

      <h4>预设</h4>
      <div class="presets">
        <button v-for="p in presets" :key="p.id" type="button" class="preset" :class="{ on: presetId === p.id }" @click="applyPreset(p)">
          <span class="pn">{{ p.name }}<span v-if="recommended === p.id" class="badge">推荐</span></span>
          <span class="pd">{{ p.desc }}</span>
        </button>
      </div>

      <el-alert v-if="caps && caps.notes.length" type="info" :closable="false" show-icon class="caps">
        <template #title>当前 ffmpeg（{{ caps.edition === 'full' ? '完整版' : '精简版' }} {{ caps.version }}）有这些限制</template>
        <ul>
          <li v-for="n in caps.notes" :key="n">{{ n }}</li>
        </ul>
        <el-button v-if="caps.edition !== 'full'" size="small" link type="primary" @click="app.goSettings('components')">去“设置 → 组件”安装完整版 ffmpeg</el-button>
      </el-alert>

      <el-button link type="primary" class="adv" @click="showAdvanced = !showAdvanced">{{ showAdvanced ? '收起详细设置' : '详细设置（可以改预设里的每一项）' }}</el-button>
      <div v-show="showAdvanced" class="form">
        <h5>画面</h5>
        <label>
          <span>尺寸</span>
          <el-select v-model="spec.size" size="small" class="w">
            <el-option value="keep" label="保持原样" />
            <el-option value="limit" label="缩小到不超过…（不放大）" />
            <el-option value="fit" label="统一到目标尺寸，补黑边" />
            <el-option value="blur" label="统一到目标尺寸，模糊背景补边" />
            <el-option value="fill" label="统一到目标尺寸，铺满并裁掉多余" />
          </el-select>
        </label>
        <label v-if="spec.size === 'limit'">
          <span>短边上限</span>
          <el-select v-model="spec.shortSide" size="small" class="w">
            <el-option :value="2160" label="2160（4K）" />
            <el-option :value="1440" label="1440" />
            <el-option :value="1080" label="1080" />
            <el-option :value="720" label="720" />
            <el-option :value="480" label="480" />
          </el-select>
        </label>
        <template v-if="['fit', 'blur', 'fill'].includes(spec.size)">
          <label>
            <span>目标尺寸</span>
            <el-select v-model="box" size="small" class="w"><el-option v-for="b in BOXES" :key="b.v" :value="b.v" :label="b.t" /></el-select>
          </label>
          <label>
            <span>跟随素材方向</span>
            <el-switch v-model="spec.followOrientation" />
            <small class="mute">打开后，竖屏素材自动用竖屏的目标尺寸（例如 1080×1920），横屏素材用横屏的。</small>
          </label>
        </template>
        <label>
          <span>去黑边</span>
          <el-switch v-model="spec.autocrop" />
          <small class="mute">在视频里取几处画面检测，上下左右至少有 8 像素黑边才会裁；画面本身很暗时不会误裁。</small>
          <small v-if="caps && !caps.cropDetect" class="warn">
            当前的 ffmpeg 没有 cropdetect 滤镜，检测不了黑边，这一项不会生效。<template v-if="caps.edition !== 'full'">在“设置 → 组件”里安装完整版 ffmpeg 后可以使用。</template>
          </small>
        </label>
        <label>
          <span>降噪</span>
          <el-select v-model="spec.denoise" size="small" class="w">
            <el-option value="off" label="关闭" />
            <el-option value="light" label="弱" />
            <el-option value="medium" label="中" />
            <el-option value="strong" label="强" />
            <el-option value="best" label="高质量（最慢）" />
          </el-select>
          <small class="mute">
            {{ DENOISE_NOTES[spec.denoise] || '去掉暗光、高 ISO、压缩过度画面里的噪点颗粒。先到下面的“预览对比”里看效果：降噪越强，细节被抹掉得越多。' }}
          </small>
        </label>

        <h5>帧率</h5>
        <label>
          <span>帧率</span>
          <el-select v-model="spec.fps" size="small" class="w">
            <el-option value="keep" label="保持原样" />
            <el-option value="auto" label="可变帧率转固定（本来就固定的不动）" />
            <el-option value="fixed" label="统一为…" />
          </el-select>
        </label>
        <label v-if="spec.fps === 'fixed'">
          <span>统一帧率</span>
          <el-select v-model="spec.fpsValue" size="small" class="w"><el-option v-for="v in FPS_VALUES" :key="v" :value="v" :label="`${v} 帧`" /></el-select>
        </label>

        <h5>色彩</h5>
        <label>
          <span>HDR 转 SDR</span>
          <el-switch v-model="spec.hdr" />
          <small class="mute">检测到 HDR10 / HLG 时转成普通的 SDR，避免在普通播放器和剪辑软件里发灰。</small>
        </label>
        <label>
          <span>补全色彩信息</span>
          <el-switch v-model="spec.fixColor" />
          <small class="mute">补写缺失的色彩标记；高清素材的 BT.601 转 BT.709。</small>
        </label>
        <div class="frow">
          <span>输出电平</span>
          <el-radio-group v-model="spec.outRange" size="small">
            <el-radio-button value="tv">16–235</el-radio-button>
            <el-radio-button value="pc">0–255</el-radio-button>
            <el-radio-button value="keep">沿用素材</el-radio-button>
          </el-radio-group>
          <small class="mute">
            16–235 是视频的标准电平（电视范围），游戏、影视美术、投稿平台等基本都要求用它，所以是默认值；0–255 是全范围，黑和白更满，但有些播放器和剪辑软件对全范围视频的处理不一致，除非明确要求，不建议用。
          </small>
        </div>
        <label>
          <span>自动色阶</span>
          <el-slider v-model="spec.levels" :min="0" :max="1" :step="0.05" :format-tooltip="(v: number) => `${Math.round(v * 100)}%`" size="small" class="sl" />
          <small class="mute">0 为关闭。把画面最暗和最亮的位置拉到黑和白，适合发灰、偏暗的素材。</small>
        </label>
        <label>
          <span>分段色彩匹配</span>
          <el-slider v-model="spec.matchColor" :min="0" :max="1" :step="0.05" :format-tooltip="(v: number) => `${Math.round(v * 100)}%`" size="small" class="sl" />
          <small class="mute">
            0 为关闭。视频里有几段来源不同、偏色（偏黄、偏蓝……）、亮度或饱和度和整体不一致时，把不一致的镜头自动校正到和整体一致，一致的镜头不动。这个滑块是统一的自动校正强度。
          </small>
        </label>
        <div v-if="spec.matchColor > 0" class="cm">
          <div class="cmtitle">统一调节</div>
          <div class="mute small">没有单独调节的镜头都用这组参数，包括检测不出差异的镜头；全 0 就是不额外调节。</div>
          <ToneSliders v-model="spec.matchTone" />
          <div class="cmtitle">排除的时间段</div>
          <div class="mute small">
            这些时间不参与匹配：不计入“整体”的基准，也不会被校正，保持原样。适合片头片尾的 Logo、黑场、故意调过色的段落。可以填时间（如 0:03 – 0:06），也可以填帧号（第几帧到第几帧，从 1 开始、含首尾两帧）；结束留空 = 一直到结尾。
          </div>
          <div v-for="(r, i) in skipRows" :key="r.id" class="skiprow">
            <el-select v-model="r.mode" size="small" class="skmode">
              <el-option value="time" label="时间" />
              <el-option value="frame" label="帧号" />
            </el-select>
            <el-input v-model="r.a" size="small" class="skin" :placeholder="r.mode === 'time' ? '开始，如 0:03' : '起始帧，如 1'" clearable />
            <span class="mute">–</span>
            <el-input v-model="r.b" size="small" class="skin" :placeholder="r.mode === 'time' ? '结束，留空 = 到结尾' : '结束帧，留空 = 到结尾'" clearable />
            <span v-if="skipInfos[i]?.error" class="skerr">{{ skipInfos[i].error }}</span>
            <span v-else-if="skipInfos[i]?.text" class="mute small mono">{{ skipInfos[i].text }}</span>
            <el-button size="small" link type="danger" @click="removeSkip(r.id)">删除</el-button>
          </div>
          <div class="cmhead">
            <el-button size="small" @click="addSkip()">添加一段</el-button>
            <el-checkbox v-if="files.length > 1 && skipRows.length" v-model="skipAll">对所有文件都排除这些时间</el-checkbox>
            <small v-if="vfrFrames" class="mute">这个视频是可变帧率，帧号换算成时间只是近似值，要求精确时请直接填时间。</small>
          </div>
          <div class="cmtitle">按镜头单独调节</div>
          <div class="cmhead">
            <el-button size="small" :loading="colorBusy" :disabled="!files.length" @click="checkColor">检测第一个文件的色彩差异</el-button>
            <small class="mute">解码一遍关键帧，几秒到几十秒。检测出和整体不一致的镜头后，可以选其中的某几个单独调节，没选的镜头用上面的统一参数。</small>
          </div>
          <template v-if="colorReport">
            <div class="strip" :title="`共 ${colorReport.shotsTotal} 个镜头`">
              <span
                v-for="(seg, i) in colorReport.timeline"
                :key="i"
                :class="{ off: seg.off }"
                :style="{ width: segPercent(seg.startMs, seg.endMs) }"
                :title="`${formatClock(seg.startMs)} – ${formatClock(seg.endMs)}${seg.off ? '（和整体不一致）' : ''}`"
              />
              <i
                v-for="(k, i) in colorReport.skipped"
                :key="`k${i}`"
                class="skipbar"
                :style="skipBar(k.startMs, k.endMs)"
                :title="`排除：${formatClock(k.startMs)} – ${formatClock(k.endMs)}`"
              />
            </div>
            <div v-if="colorReport.note" class="mute small">{{ colorReport.note }}</div>
            <div v-else class="mute small">共 {{ colorReport.shotsTotal }} 个镜头，{{ colorReport.flagged.length }} 个和整体不一致（橙色）。{{ spec.matchShots.length }} 个单独调节。</div>
            <div v-if="colorReport.skipped.length" class="mute small">斜线是排除的时间段，其中 {{ colorReport.shotsExcluded }} 个镜头没有参与统计。</div>
            <div v-for="f in colorReport.flagged" :key="f.id" class="shotbox">
              <div class="shot">
                <span class="mono when">{{ formatClock(f.startMs) }} – {{ formatClock(f.endMs) }}</span>
                <span class="chips">
                  <span v-for="d in f.defects" :key="d" class="chip warn">{{ d }}</span>
                </span>
                <el-button size="small" link type="primary" :loading="previewing" @click="previewShot(f)">看对比</el-button>
                <el-button size="small" link @click="excludeShot(f)">排除这个镜头</el-button>
                <el-checkbox :model-value="isOwn(f.id)" @change="(v: string | number | boolean) => toggleOwn(f.id, !!v)">单独调节</el-checkbox>
              </div>
              <div v-for="o in ownOf(f.id)" :key="o.id" class="own">
                <div class="orow">
                  <span class="name">自动校正</span>
                  <el-slider v-model="o.strength" :min="0" :max="1" :step="0.05" :show-tooltip="false" size="small" class="osl" />
                  <span class="val mono">{{ pctText(o.strength ?? 0) }}</span>
                  <small class="mute">0 = 这个镜头不做自动校正</small>
                </div>
                <ToneSliders v-model="o.tone" />
              </div>
            </div>
          </template>
        </div>
        <label>
          <span>LUT</span>
          <span class="dirbox">
            <el-input :model-value="spec.lut ?? ''" size="small" readonly placeholder="未选择（.cube 文件）" class="w" />
            <el-button size="small" @click="pickLut">选择…</el-button>
            <el-button v-if="spec.lut" size="small" link @click="spec.lut = null">清除</el-button>
          </span>
        </label>
        <label v-if="spec.lut">
          <span>LUT 强度</span>
          <el-slider v-model="spec.lutStrength" :min="0" :max="1" :step="0.05" :format-tooltip="(v: number) => `${Math.round(v * 100)}%`" size="small" class="sl" />
        </label>

        <h5>声音</h5>
        <label>
          <span>采样率</span>
          <el-select v-model="spec.audioRate" size="small" class="w">
            <el-option :value="0" label="保持原样" />
            <el-option :value="44100" label="44.1 kHz" />
            <el-option :value="48000" label="48 kHz" />
          </el-select>
        </label>
        <label>
          <span>声道</span>
          <el-select v-model="spec.audioChannels" size="small" class="w">
            <el-option :value="0" label="保持原样" />
            <el-option :value="1" label="单声道" />
            <el-option :value="2" label="立体声" />
          </el-select>
        </label>
        <label>
          <span>响度标准化</span>
          <el-switch v-model="loudnessOn" />
          <span v-if="spec.loudness !== null" class="dirbox">
            <el-select v-model="spec.loudness" size="small" class="w">
              <el-option :value="-14" label="-14 LUFS（短视频、流媒体）" />
              <el-option :value="-16" label="-16 LUFS（通用）" />
              <el-option :value="-23" label="-23 LUFS（广播）" />
            </el-select>
          </span>
          <small class="mute">先测量整段音频再调整（两遍），保持原有的动态，不会忽大忽小。</small>
        </label>
        <label>
          <span>修正音画不同步</span>
          <el-switch v-model="spec.fixSync" />
          <small class="mute">直播录制（FLV / TS）和可变帧率的视频常有时间戳跳变，打开后会修正。</small>
        </label>

        <h5>输出</h5>
        <label>
          <span>视频编码</span>
          <el-select v-model="spec.codec" size="small" class="w">
            <el-option value="keep" label="能不重新编码就不动" />
            <el-option value="h264" label="H.264（兼容性最好）" />
            <el-option value="hevc" label="HEVC（体积更小）" :disabled="!!caps && !caps.hevc" />
          </el-select>
          <small v-if="caps && !caps.hevc" class="mute">当前的 ffmpeg 没有 HEVC 编码器，选 HEVC 时会改用 H.264。</small>
        </label>
        <label>
          <span>画质</span>
          <el-radio-group v-model="spec.quality" size="small">
            <el-radio-button value="small">体积小</el-radio-button>
            <el-radio-button value="balanced">均衡</el-radio-button>
            <el-radio-button value="high">画质优先</el-radio-button>
          </el-radio-group>
          <small v-if="caps" class="mute">视频编码器：{{ caps.h264 ?? '没有 H.264 编码器' }}{{ caps.hardware.length ? `；可用硬件编码：${caps.hardware.join('、')}` : '' }}</small>
        </label>
      </div>

      <h4>预览对比</h4>
      <div class="pv">
        <el-input v-model="previewAt" size="small" class="t mono" placeholder="0:02" />
        <el-button size="small" :loading="previewing" :disabled="!files.length" @click="makePreview">生成第一个文件的对比图</el-button>
        <small class="mute">只含画面处理（尺寸、HDR、色阶、分段色彩匹配、LUT、去黑边），不含帧率和声音。</small>
      </div>
      <div v-if="preview" class="cmp">
        <figure>
          <img :src="imgSrc(preview.before)" alt="处理前" />
          <figcaption>处理前（普通播放器里的样子）</figcaption>
        </figure>
        <figure>
          <img :src="imgSrc(preview.after)" alt="处理后" />
          <figcaption>{{ preview.changed ? '处理后' : '处理后（画面不需要处理）' }}</figcaption>
        </figure>
        <div v-if="preview.notes.length || preview.warnings.length" class="pnotes mute small">
          <div v-for="n in preview.notes" :key="n">· {{ n }}</div>
          <div v-for="w in preview.warnings" :key="w" class="warn">注意：{{ w }}</div>
        </div>
      </div>

      <label class="dir">
        <span>保存到</span>
        <span class="dirbox"><el-input v-model="outDir" size="small" clearable placeholder="留空放在原文件旁边" class="w" /><el-button size="small" @click="pickDir">选择…</el-button></span>
      </label>
      <div class="go">
        <el-button type="primary" :loading="busy" :disabled="!files.length || analyzing" @click="start">
          开始规整{{ files.length > 1 ? `（${files.length} 个文件）` : '' }}{{ curPreset ? ` · ${curPreset.name}` : '' }}
        </el-button>
        <small class="mute">已经符合规格的文件不会重新编码；没有需要改的会直接告诉你。</small>
      </div>
    </div>

    <div class="card box">
      <h3>防抖</h3>
      <p class="mute desc">稳定手持拍摄的抖动画面，会略微放大裁掉边缘。声音不动。</p>
      <div class="pv">
        <el-radio-group v-model="stab" size="small">
          <el-radio-button value="light">轻微</el-radio-button>
          <el-radio-button value="normal">标准</el-radio-button>
          <el-radio-button value="strong">强力</el-radio-button>
        </el-radio-group>
        <el-button size="small" type="primary" :loading="stabBusy" :disabled="!files.length || caps?.stabilize === 'none'" @click="stabilize">对上面选中的文件防抖</el-button>
      </div>
      <small class="mute">{{ stabNote }}</small>
    </div>
  </div>
</template>

<style scoped>
.nt {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.box {
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
.cm {
  margin: -4px 0 4px 0;
  padding: 8px 10px;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.cmhead {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.strip {
  position: relative;
  display: flex;
  height: 10px;
  border-radius: 5px;
  overflow: hidden;
  background: var(--cc-line);
}
.strip span {
  display: block;
  height: 100%;
  background: color-mix(in srgb, var(--cc-mute) 30%, transparent);
}
.strip span.off {
  background: var(--cc-acc);
}
.skipbar {
  position: absolute;
  top: 0;
  bottom: 0;
  background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--cc-fg) 55%, transparent) 0 2px, transparent 2px 5px);
  border-left: 1px solid var(--cc-fg);
  border-right: 1px solid var(--cc-fg);
}
.skiprow {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.skiprow .skmode {
  width: 76px;
}
.skiprow .skin {
  width: 150px;
}
.skerr {
  color: var(--cc-warn, #b26a00);
  font-size: 11.5px;
}
.shot {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.shot .when {
  min-width: 96px;
  font-size: 12.5px;
}
/* 复选框本身是个 label，会吃到表单里 label 的两列网格，这里还原 */
.shot .chips {
  min-width: 180px;
}
.cm :deep(.el-checkbox) {
  display: inline-flex;
  grid-template-columns: none;
  height: auto;
}
.cm :deep(.el-checkbox__label) {
  font-size: 12.5px;
}
.cmtitle {
  font-size: 12.5px;
  font-weight: 600;
}
.shotbox {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.own {
  margin-left: 24px;
  padding: 6px 10px;
  border-left: 2px solid var(--cc-acc);
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.orow {
  display: grid;
  grid-template-columns: 64px 280px 52px auto;
  gap: 10px;
  align-items: center;
}
.orow .name {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.orow .val {
  font-size: 11.5px;
  text-align: right;
}
h4 {
  margin: 6px 0 0;
  font-size: 13px;
}
h5 {
  margin: 8px 0 0;
  font-size: 12px;
  color: var(--cc-mute);
  font-weight: 600;
}
.desc {
  margin: 0;
  font-size: 12.5px;
}
.small {
  font-size: 11.5px;
}
.found {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.file {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.fn {
  font-size: 13px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}
.chip {
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 10px;
  background: var(--cc-acc-soft);
  color: var(--cc-fg);
}
.chip.warn {
  background: var(--cc-warn-soft, #fdf0d8);
  color: var(--cc-warn, #b26a00);
}
.chip.ok {
  background: var(--cc-ok-soft, #e1f4e8);
  color: var(--cc-ok, #1b7f46);
}
.rec {
  font-size: 11.5px;
  color: var(--cc-acc);
  margin-left: auto;
}
.err {
  color: var(--cc-err);
}
.presets {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 8px;
}
.preset {
  all: unset;
  cursor: pointer;
  box-sizing: border-box;
  border: 1.5px solid var(--cc-line);
  border-radius: 9px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.preset:hover {
  border-color: var(--cc-acc);
}
.preset.on {
  border-color: var(--cc-acc);
  background: var(--cc-acc-soft);
}
.preset:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.pn {
  font-size: 13px;
  font-weight: 600;
}
.badge {
  margin-left: 6px;
  font-size: 10.5px;
  font-weight: 500;
  padding: 0 6px;
  border-radius: 8px;
  background: var(--cc-acc);
  color: #fff;
}
.pd {
  font-size: 11.5px;
  color: var(--cc-mute);
  line-height: 1.5;
}
.caps ul {
  margin: 4px 0;
  padding-left: 18px;
  font-size: 12px;
}
.adv {
  align-self: flex-start;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.form label,
.form .frow,
.dir {
  display: grid;
  grid-template-columns: 120px auto;
  gap: 10px;
  align-items: center;
  justify-content: start;
}
.form label > span:first-child,
.form .frow > span:first-child,
.dir > span:first-child {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.form label small,
.form .frow small {
  grid-column: 2;
  font-size: 11.5px;
  max-width: 560px;
}
.w {
  width: 260px;
}
.sl {
  width: 340px;
}
.dirbox {
  display: flex;
  gap: 8px;
  align-items: center;
}
.pv {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.t {
  width: 90px;
}
.cmp {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}
figure {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
figure img {
  width: 100%;
  border-radius: 8px;
  border: 1px solid var(--cc-line);
  background: #000;
}
figcaption {
  font-size: 11.5px;
  color: var(--cc-mute);
  text-align: center;
}
.pnotes {
  grid-column: 1 / -1;
}
.warn {
  color: var(--cc-warn, #b26a00);
}
.go {
  display: flex;
  gap: 12px;
  align-items: center;
  margin-top: 4px;
}
</style>
