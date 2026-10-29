<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { open, save } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { LutAdjust, LutLook, LutRecipe, LutSaved } from '../../types'
import { formatBytes } from '../../utils/format'
import { formatClock, parseClock } from '../../utils/time'
import FileInput from './FileInput.vue'

const props = defineProps<{ preset?: string[] }>()
const app = useAppStore()

const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff']
const VIDEO_EXTS = ['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'ts', 'm4v']
const VIDEO_RE = new RegExp(`\\.(${VIDEO_EXTS.join('|')})$`, 'i')

const DEFAULTS: LutAdjust = {
  exposure: 0,
  black: 0,
  white: 1,
  gamma: 0,
  contrast: 0,
  shadows: 0,
  highlights: 0,
  temperature: 0,
  tint: 0,
  saturation: 0,
  vibrance: 0,
  hue: 0,
  shadowTone: { hue: 220, amount: 0 },
  highlightTone: { hue: 40, amount: 0 },
  strength: 1,
}
const clone = <T,>(x: T): T => JSON.parse(JSON.stringify(x)) as T

// ---------- 状态 ----------

const samplePath = ref(props.preset?.[0] ?? '')
const sampleClock = ref('0:00')
const sampleMs = ref(0)
const isVideo = computed(() => VIDEO_RE.test(samplePath.value))
const sampleFiles = computed({
  get: () => (samplePath.value ? [samplePath.value] : []),
  set: (v: string[]) => {
    samplePath.value = v[0] ?? ''
    sampleClock.value = '0:00'
    sampleMs.value = 0
  },
})

interface Layer {
  path: string
  strength: number
  title: string
  size: number
  error: string
  checking: boolean
}
const layers = ref<Layer[]>([])
const adjust = reactive<LutAdjust>(clone(DEFAULTS))
const refPath = ref('')
const refTone = ref(0.8)
const refColor = ref(0.8)
const refFiles = computed({
  get: () => (refPath.value ? [refPath.value] : []),
  set: (v: string[]) => (refPath.value = v[0] ?? ''),
})
const name = ref('我的调色')
const size = ref<17 | 33 | 65>(33)
const looks = ref<LutLook[]>([])
const lookId = ref('')

const origCanvas = ref<HTMLCanvasElement | null>(null)
const afterCanvas = ref<HTMLCanvasElement | null>(null)
const hasSample = ref(false)
const loadingSample = ref(false)
const previewing = ref(false)
const sampleError = ref('')
const previewError = ref('')
const saving = ref(false)
const exporting = ref(false)
const saved = ref<(LutSaved & { key: string }) | null>(null)

// ---------- 调整滑块 ----------

type NumKey = Exclude<keyof LutAdjust, 'shadowTone' | 'highlightTone'>
interface Ctl {
  key: NumKey
  label: string
  hint: string
  min: number
  max: number
  step: number
  fmt: (v: number) => string
}
const signed = (v: number, digits = 0) => {
  const n = Number(v.toFixed(digits))
  return n > 0 ? `+${n.toFixed(digits)}` : (n === 0 ? 0 : n).toFixed(digits)
}
const pct = (v: number) => signed(v * 100)
const GROUPS: { title: string; items: Ctl[] }[] = [
  {
    title: '光线',
    items: [
      { key: 'exposure', label: '曝光', hint: '整体变亮或变暗，每档亮一倍（在线性光里调，不会发灰）', min: -3, max: 3, step: 0.05, fmt: (v) => `${signed(v, 2)} 档` },
      { key: 'black', label: '黑场', hint: '往右把更多暗部压成纯黑；往左抬高黑位，有褪色胶片的感觉', min: -0.2, max: 0.4, step: 0.01, fmt: (v) => signed(v * 100) },
      { key: 'white', label: '白场', hint: '往左把更多亮部推成纯白；往右压低最亮处', min: 0.6, max: 1.2, step: 0.01, fmt: (v) => String(Math.round(v * 100)) },
      { key: 'gamma', label: '中间调', hint: '只提亮或压暗中间调，黑和白不动', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'contrast', label: '对比度', hint: '拉开或收拢明暗差距，中灰不动', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'shadows', label: '阴影', hint: '正数提亮暗部、保留细节', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'highlights', label: '高光', hint: '负数压低亮部、找回细节', min: -1, max: 1, step: 0.02, fmt: pct },
    ],
  },
  {
    title: '色彩',
    items: [
      { key: 'temperature', label: '色温', hint: '往左偏蓝（冷），往右偏黄（暖）', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'tint', label: '色调', hint: '往左偏绿，往右偏品红', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'saturation', label: '饱和度', hint: '-100 是黑白，+100 约两倍', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'vibrance', label: '自然饱和度', hint: '优先加强不够鲜艳的颜色，肤色和已经很艳的颜色少动', min: -1, max: 1, step: 0.02, fmt: pct },
      { key: 'hue', label: '色相偏移', hint: '整体转动色轮，一般只用来做特殊效果', min: -180, max: 180, step: 1, fmt: (v) => `${signed(v)}°` },
    ],
  },
]
const isDefault = (k: NumKey) => Math.abs(adjust[k] - DEFAULTS[k]) < 1e-9
const reset = (k: NumKey) => (adjust[k] = DEFAULTS[k])
const swatch = (hue: number) => `hsl(${hue}, 75%, 52%)`

function setAdjust(a: LutAdjust) {
  Object.assign(adjust, clone(a))
}
function resetAll() {
  setAdjust(DEFAULTS)
  lookId.value = ''
}
function applyLook(l: LutLook) {
  lookId.value = l.id
  setAdjust(l.adjust)
}
const adjusted = computed(() => JSON.stringify(adjust) !== JSON.stringify(DEFAULTS))

// ---------- 基底 LUT ----------

async function addLayers() {
  const r = await open({ multiple: true, filters: [{ name: '3D LUT', extensions: ['cube'] }] })
  const list = (Array.isArray(r) ? r : r ? [r] : []) as string[]
  for (const path of list) {
    if (layers.value.some((l) => l.path === path)) continue
    const layer = reactive<Layer>({ path, strength: 1, title: '', size: 0, error: '', checking: true })
    layers.value.push(layer)
    api
      .lutInspect(path)
      .then((i) => {
        layer.title = i.title
        layer.size = i.size
      })
      .catch((e) => (layer.error = errorText(e)))
      .finally(() => (layer.checking = false))
  }
}
function moveLayer(i: number, d: number) {
  const j = i + d
  if (j < 0 || j >= layers.value.length) return
  const next = [...layers.value]
  ;[next[i], next[j]] = [next[j], next[i]]
  layers.value = next
}
const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p

// ---------- 配方 ----------

function recipe(): LutRecipe {
  return {
    name: name.value.trim() || 'ClearClip LUT',
    size: size.value,
    base: layers.value.filter((l) => !l.error && !l.checking).map((l) => ({ path: l.path, strength: l.strength })),
    adjust: clone(adjust),
    reference: refPath.value ? { path: refPath.value, tone: refTone.value, color: refColor.value } : null,
    sample: samplePath.value ? { path: samplePath.value, atMs: sampleMs.value } : null,
  }
}
/** 影响预览的内容：不含名字和格点数。 */
const previewKey = computed(() => {
  const r = recipe()
  return JSON.stringify({ base: r.base, adjust: r.adjust, reference: r.reference, sample: r.sample })
})
/** 影响生成的 LUT 的内容：没用参考图时示例图片不影响结果。 */
const lutKey = computed(() => {
  const r = recipe()
  return JSON.stringify({ ...r, sample: r.reference ? r.sample : null })
})

// ---------- 画面 ----------

/** 后端给的像素：8 字节头（宽、高，小端）+ RGBA。 */
function paint(canvas: HTMLCanvasElement | null, buf: ArrayBuffer): boolean {
  if (!canvas || buf.byteLength < 8) return false
  const dv = new DataView(buf)
  const w = dv.getUint32(0, true)
  const h = dv.getUint32(4, true)
  if (!w || !h || buf.byteLength !== 8 + w * h * 4) return false
  if (canvas.width !== w) canvas.width = w
  if (canvas.height !== h) canvas.height = h
  canvas.getContext('2d')?.putImageData(new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h), 0, 0)
  return true
}

let sampleJob: Promise<void> = Promise.resolve()
let sampleToken = 0

function loadSample() {
  const my = ++sampleToken
  const path = samplePath.value
  sampleError.value = ''
  previewError.value = ''
  if (!path) {
    hasSample.value = false
    sampleJob = Promise.resolve()
    return
  }
  loadingSample.value = true
  sampleJob = (async () => {
    try {
      const buf = await api.lutSample(path, sampleMs.value)
      if (my !== sampleToken) return
      hasSample.value = paint(origCanvas.value, buf)
      if (!hasSample.value) sampleError.value = '读取到的图片数据不对。'
    } catch (e) {
      if (my !== sampleToken) return
      hasSample.value = false
      sampleError.value = errorText(e)
    } finally {
      if (my === sampleToken) loadingSample.value = false
    }
  })()
}

watch([samplePath, sampleMs], loadSample)

function setClock() {
  const ms = parseClock(sampleClock.value)
  if (ms === null) {
    ElMessage.warning('时间点的写法不对，请用 5、0:05 或 1:02:03 这样的格式。')
    sampleClock.value = formatClock(sampleMs.value)
    return
  }
  sampleClock.value = formatClock(ms)
  sampleMs.value = ms
}

let timer: number | undefined
let running = false
let dirty = false

function schedule(delay = 150) {
  window.clearTimeout(timer)
  timer = window.setTimeout(() => void runPreview(), delay)
}

/** 同一时间只算一次预览；算的时候又有变化就在算完后再来一次，只画最新的。 */
async function runPreview() {
  if (running) {
    dirty = true
    return
  }
  running = true
  try {
    do {
      dirty = false
      if (!samplePath.value) break
      await sampleJob
      if (!hasSample.value) break
      previewing.value = true
      try {
        const buf = await api.lutPreview(recipe())
        if (!dirty) {
          if (paint(afterCanvas.value, buf)) previewError.value = ''
          else previewError.value = '预览数据不对。'
        }
      } catch (e) {
        if (!dirty) previewError.value = errorText(e)
      }
    } while (dirty)
  } finally {
    running = false
    previewing.value = false
  }
}

watch(previewKey, () => schedule())
// 示例图片读好后画调整后的版本
watch(hasSample, (v) => v && schedule(0))

// ---------- 输出 ----------

const DIR_KEY = 'clearclip.lut.dir'
function lastDir(): string {
  try {
    return localStorage.getItem(DIR_KEY) ?? ''
  } catch {
    return ''
  }
}
function rememberDir(file: string) {
  const i = Math.max(file.lastIndexOf('/'), file.lastIndexOf('\\'))
  if (i <= 0) return
  try {
    localStorage.setItem(DIR_KEY, file.slice(0, i))
  } catch {
    /* 存不了就算了，下次从默认位置开始 */
  }
}
function defaultPath(file: string): string | undefined {
  const dir = lastDir()
  if (!dir) return file
  return `${dir}${dir.includes('\\') ? '\\' : '/'}${file}`
}
const safeName = () => (name.value.trim() || 'ClearClip LUT').replace(/[\\/:*?"<>|]+/g, '_')

async function saveCube() {
  const dest = await save({ defaultPath: defaultPath(`${safeName()}.cube`), filters: [{ name: '3D LUT', extensions: ['cube'] }] })
  if (!dest) return
  saving.value = true
  try {
    const r = await api.lutSave(recipe(), dest)
    saved.value = { ...r, key: lutKey.value }
    rememberDir(r.path)
    ElMessage.success(`已保存：${baseName(r.path)}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    saving.value = false
  }
}

async function exportImage() {
  const stem = baseName(samplePath.value).replace(/\.[^.]+$/, '') || 'image'
  const dest = await save({ defaultPath: defaultPath(`${stem}-lut.png`), filters: [{ name: '图片', extensions: ['png', 'jpg', 'webp'] }] })
  if (!dest) return
  exporting.value = true
  try {
    const out = await api.lutExportImage(recipe(), dest)
    rememberDir(out)
    ElMessage.success(`已导出：${baseName(out)}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    exporting.value = false
  }
}

function reveal() {
  if (saved.value) api.revealFile(saved.value.path).catch((e) => ElMessage.error(errorText(e)))
}
const savedStale = computed(() => !!saved.value && saved.value.key !== lutKey.value)
function useInNormalize() {
  if (!saved.value) return
  app.goToolbox(isVideo.value ? [samplePath.value] : [], undefined, { tab: 'normalize', lut: saved.value.path })
}

onMounted(async () => {
  looks.value = await api.lutLooks().catch(() => [])
  if (samplePath.value) loadSample()
})
onBeforeUnmount(() => window.clearTimeout(timer))

watch(
  () => props.preset,
  (p) => {
    if (p?.length) sampleFiles.value = [p[0]]
  },
)
</script>

<template>
  <div class="lt">
    <div class="card box">
      <h3>LUT 工作室</h3>
      <p class="mute desc">
        用一张示例图片（或视频里的一帧）边看边调，调好后生成 .cube 文件，可以用在视频规整里，也可以导入剪映、DaVinci Resolve、Premiere、OBS 等支持 3D LUT 的软件。可以在别人的 LUT 上继续调整，也可以让示例图片的色调向一张参考图靠拢。
      </p>
      <FileInput v-model="sampleFiles" :kinds="['image', 'video']" :extensions="[...IMAGE_EXTS, ...VIDEO_EXTS]" label="示例图片或视频" />
      <div v-if="isVideo" class="pv">
        <span class="mute small">取视频里这个时间点的画面</span>
        <el-input v-model="sampleClock" size="small" class="t" @keyup.enter="setClock" @blur="setClock" />
      </div>
      <p v-if="sampleError" class="err small">{{ sampleError }}</p>
    </div>

    <div v-show="hasSample" class="card box cmpbox">
      <div class="cmp" v-loading="loadingSample">
        <figure>
          <canvas ref="origCanvas" />
          <figcaption>原图</figcaption>
        </figure>
        <figure>
          <canvas ref="afterCanvas" />
          <figcaption>
            调整后
            <span v-if="previewing" class="mute">· 计算中…</span>
          </figcaption>
        </figure>
      </div>
      <p v-if="previewError" class="err small">{{ previewError }}</p>
    </div>

    <div class="card box">
      <h4>基底 LUT（可选）</h4>
      <p class="mute small">
        先套用这些 LUT，再在它们的基础上调整。按列表顺序叠加，可以分别调强度。只支持 3D 的 .cube 文件。
      </p>
      <div class="bar">
        <el-button size="small" type="primary" @click="addLayers">添加 LUT…</el-button>
        <el-button v-if="layers.length" size="small" link @click="layers = []">全部移除</el-button>
      </div>
      <div v-for="(l, i) in layers" :key="l.path" class="layer">
        <div class="lhead">
          <span class="ellipsis selectable" :title="l.path">{{ baseName(l.path) }}</span>
          <span v-if="l.checking" class="mute small">检查中…</span>
          <span v-else-if="l.error" class="err small">{{ l.error }}</span>
          <span v-else class="mute small mono">{{ l.size }}³{{ l.title ? ` · ${l.title}` : '' }}</span>
          <span class="ops">
            <el-button v-if="layers.length > 1" link size="small" :disabled="i === 0" @click="moveLayer(i, -1)">上移</el-button>
            <el-button v-if="layers.length > 1" link size="small" :disabled="i === layers.length - 1" @click="moveLayer(i, 1)">下移</el-button>
            <el-button link size="small" @click="layers.splice(i, 1)">移除</el-button>
          </span>
        </div>
        <div v-if="!l.error" class="row">
          <span class="lab">强度</span>
          <el-slider v-model="l.strength" :min="0" :max="1" :step="0.05" :show-tooltip="false" size="small" />
          <span class="val mono">{{ Math.round(l.strength * 100) }}%</span>
          <span />
        </div>
      </div>
    </div>

    <div class="card box">
      <div class="gh">
        <h4>调整</h4>
        <el-button size="small" link :disabled="!adjusted" @click="resetAll">全部恢复默认</el-button>
      </div>
      <div v-if="looks.length" class="looks">
        <span class="mute small">一键风格</span>
        <button v-for="l in looks" :key="l.id" type="button" class="look" :class="{ on: lookId === l.id }" :title="l.desc" @click="applyLook(l)">{{ l.name }}</button>
        <span class="mute small">套用后会替换下面的滑块，之后还能接着微调。</span>
      </div>

      <template v-for="g in GROUPS" :key="g.title">
        <h5>{{ g.title }}</h5>
        <div v-for="c in g.items" :key="c.key" class="row" :class="{ changed: !isDefault(c.key) }">
          <span class="lab" :title="c.hint" @dblclick="reset(c.key)">{{ c.label }}</span>
          <el-slider v-model="adjust[c.key]" :min="c.min" :max="c.max" :step="c.step" :show-tooltip="false" size="small" @change="lookId = ''" />
          <span class="val mono">{{ c.fmt(adjust[c.key]) }}</span>
          <button type="button" class="rs" title="重置" aria-label="重置" :disabled="isDefault(c.key)" @click="reset(c.key)">↺</button>
        </div>
      </template>

      <h5>分色调</h5>
      <p class="mute small">给暗部和亮部各染一种颜色，常见的“青橙”电影感就是暗部偏青、亮部偏橙。只改颜色，不改明暗。</p>
      <div class="row">
        <span class="lab">暗部颜色</span>
        <el-slider v-model="adjust.shadowTone.hue" :min="0" :max="360" :step="1" :show-tooltip="false" size="small" />
        <span class="val"><i class="dot" :style="{ background: swatch(adjust.shadowTone.hue) }" />{{ Math.round(adjust.shadowTone.hue) }}°</span>
        <span />
      </div>
      <div class="row" :class="{ changed: adjust.shadowTone.amount > 0 }">
        <span class="lab">暗部强度</span>
        <el-slider v-model="adjust.shadowTone.amount" :min="0" :max="1" :step="0.02" :show-tooltip="false" size="small" @change="lookId = ''" />
        <span class="val mono">{{ Math.round(adjust.shadowTone.amount * 100) }}</span>
        <button type="button" class="rs" title="重置" aria-label="重置" :disabled="adjust.shadowTone.amount === 0" @click="adjust.shadowTone.amount = 0">↺</button>
      </div>
      <div class="row">
        <span class="lab">亮部颜色</span>
        <el-slider v-model="adjust.highlightTone.hue" :min="0" :max="360" :step="1" :show-tooltip="false" size="small" />
        <span class="val"><i class="dot" :style="{ background: swatch(adjust.highlightTone.hue) }" />{{ Math.round(adjust.highlightTone.hue) }}°</span>
        <span />
      </div>
      <div class="row" :class="{ changed: adjust.highlightTone.amount > 0 }">
        <span class="lab">亮部强度</span>
        <el-slider v-model="adjust.highlightTone.amount" :min="0" :max="1" :step="0.02" :show-tooltip="false" size="small" @change="lookId = ''" />
        <span class="val mono">{{ Math.round(adjust.highlightTone.amount * 100) }}</span>
        <button type="button" class="rs" title="重置" aria-label="重置" :disabled="adjust.highlightTone.amount === 0" @click="adjust.highlightTone.amount = 0">↺</button>
      </div>

      <h5>整体</h5>
      <div class="row" :class="{ changed: adjust.strength !== 1 }">
        <span class="lab" title="把上面所有调整和原画面混合，100 是完全使用调整后的效果">效果强度</span>
        <el-slider v-model="adjust.strength" :min="0" :max="1" :step="0.02" :show-tooltip="false" size="small" />
        <span class="val mono">{{ Math.round(adjust.strength * 100) }}%</span>
        <button type="button" class="rs" title="重置" aria-label="重置" :disabled="adjust.strength === 1" @click="adjust.strength = 1">↺</button>
      </div>
    </div>

    <div class="card box">
      <h4>参考图匹配（可选）</h4>
      <p class="mute small">
        选一张色调想要的图片，程序会算出怎样调整示例图片，让它的明暗分布和整体色彩更接近参考图，再把这个调整生成到 LUT 里。需要先选好上面的示例图片；匹配出来的是整体的色调倾向，不会把参考图的内容搬过来。
      </p>
      <FileInput v-model="refFiles" :kinds="['image']" :extensions="IMAGE_EXTS" label="参考图片" />
      <template v-if="refPath">
        <div class="row">
          <span class="lab" title="让亮部、暗部的分布向参考图靠拢">明暗匹配</span>
          <el-slider v-model="refTone" :min="0" :max="1" :step="0.05" :show-tooltip="false" size="small" />
          <span class="val mono">{{ Math.round(refTone * 100) }}%</span>
          <span />
        </div>
        <div class="row">
          <span class="lab" title="让整体偏色和饱和度向参考图靠拢">色彩匹配</span>
          <el-slider v-model="refColor" :min="0" :max="1" :step="0.05" :show-tooltip="false" size="small" />
          <span class="val mono">{{ Math.round(refColor * 100) }}%</span>
          <span />
        </div>
        <p v-if="!samplePath" class="warn small">还没有示例图片，匹配需要先选一张。</p>
      </template>
    </div>

    <div class="card box">
      <h4>生成</h4>
      <div class="form">
        <div class="frow">
          <span class="lab">LUT 名称</span>
          <el-input v-model="name" size="small" maxlength="60" class="w" />
        </div>
        <div class="frow">
          <span class="lab">精细度</span>
          <el-radio-group v-model="size" size="small">
            <el-radio-button :value="17">17（小，快）</el-radio-button>
            <el-radio-button :value="33">33（推荐）</el-radio-button>
            <el-radio-button :value="65">65（精细，文件大）</el-radio-button>
          </el-radio-group>
        </div>
      </div>
      <div class="go">
        <el-button type="primary" :loading="saving" @click="saveCube">保存为 .cube…</el-button>
        <el-button :loading="exporting" :disabled="!samplePath" @click="exportImage">导出调整后的图片…</el-button>
      </div>
      <div v-if="saved" class="savedbox">
        <span class="ellipsis selectable" :title="saved.path">{{ saved.path }}</span>
        <span class="mute small mono">{{ saved.size }}³ · {{ formatBytes(saved.bytes) }}</span>
        <span v-if="savedStale" class="warn small">之后改动了设置，需要重新保存</span>
        <span class="ops">
          <el-button link size="small" @click="reveal">在文件夹中显示</el-button>
          <el-button link size="small" type="primary" @click="useInNormalize">用于视频规整</el-button>
        </span>
      </div>
      <p class="mute small note">
        生成的 LUT 面向普通的 Rec.709 / sRGB 画面，也就是手机、相机直出和平台下载的常见视频与图片。S-Log、V-Log、C-Log 之类的 Log 素材和 HDR 素材，需要先转换色彩空间再用，直接套用效果不对。
      </p>
    </div>
  </div>
</template>

<style scoped>
.lt {
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
h4 {
  margin: 0;
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
.note {
  margin: 0;
}
p {
  margin: 0;
}
.err {
  color: var(--cc-err);
}
.warn {
  color: var(--cc-warn, #b26a00);
}
.pv {
  display: flex;
  gap: 8px;
  align-items: center;
}
.t {
  width: 90px;
}
.cmpbox {
  position: sticky;
  top: 0;
  z-index: 3;
  background: var(--cc-card);
  padding-top: 10px;
  padding-bottom: 8px;
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
  min-width: 0;
}
canvas {
  width: 100%;
  max-height: 200px;
  object-fit: contain;
  border-radius: 8px;
  border: 1px solid var(--cc-line);
  background: #000;
}
figcaption {
  font-size: 11.5px;
  color: var(--cc-mute);
  text-align: center;
}
.bar {
  display: flex;
  gap: 8px;
  align-items: center;
}
.layer {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 6px 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.lhead {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12.5px;
}
.lhead > .ellipsis {
  flex: 0 1 220px;
}
.ops {
  margin-left: auto;
  flex: none;
}
.gh {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.looks {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}
.look {
  all: unset;
  cursor: pointer;
  box-sizing: border-box;
  font-size: 12px;
  padding: 2px 12px;
  border: 1.5px solid var(--cc-line);
  border-radius: 14px;
}
.look:hover {
  border-color: var(--cc-acc);
}
.look.on {
  border-color: var(--cc-acc);
  background: var(--cc-acc-soft);
}
.look:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.row {
  display: grid;
  grid-template-columns: 96px minmax(120px, 360px) 64px 22px;
  gap: 12px;
  align-items: center;
}
.lab {
  color: var(--cc-mute);
  font-size: 12.5px;
  cursor: default;
}
.row.changed .lab {
  color: var(--cc-fg);
  font-weight: 600;
}
.val {
  font-size: 12px;
  text-align: right;
  display: inline-flex;
  gap: 5px;
  align-items: center;
  justify-content: flex-end;
}
.dot {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  border: 1px solid var(--cc-line);
}
.rs {
  all: unset;
  cursor: pointer;
  text-align: center;
  font-size: 14px;
  line-height: 1;
  color: var(--cc-acc);
  border-radius: 4px;
}
.rs:disabled {
  visibility: hidden;
}
.rs:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.frow {
  display: grid;
  grid-template-columns: 96px auto;
  gap: 12px;
  align-items: center;
  justify-content: start;
}
.w {
  width: 260px;
}
.go {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.savedbox {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 6px 10px;
  font-size: 12.5px;
}
.savedbox > .ellipsis {
  flex: 0 1 380px;
}
</style>
