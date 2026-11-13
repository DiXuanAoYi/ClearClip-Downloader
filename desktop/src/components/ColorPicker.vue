<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import type { RGB3 } from '../types'
import {
  BASIC_COLORS, COLORBLIND_NAMES, HARMONY_SCHEMES, MOODS, SAFE_DEFAULT, adjustForContrast, clampSafe, colorName, contrastRatio, deltaE, extractPalette,
  gradientCss, hexToRgb, hslToRgb, hsvToRgb, parseRgbText, pixelAverage, ratioLabel, rgbCss, rgbToHex, rgbToHsl, rgbToHsv, rgbToLab, rgbToYcbcr709,
  sampleGradient, simulateColorblind, wcag, wheelPalette, type ColorBlind, type Extracted, type Mood, type PixelImage,
} from '../utils/color'

/**
 * 取色器：取色（色域 + 色相条 + 输入 + 基本 / 自定义 / 历史）、原页（吸管、提取一组颜色、取渐变）、配色（色轮）、
 * 检查（对比度、色盲）。BT.709 安全色域放在“高级”里，默认关。
 * 取到的颜色先放在这里，点“应用”才交给外面；渐变用“应用渐变”。
 */
const props = defineProps<{
  modelValue: boolean
  title: string
  color: RGB3
  /** 原页图（data URL）；给了才有“原页”页 */
  image?: string
  /** 对比度要对着看的颜色（文字色对背景、背景色对文字） */
  against?: RGB3
  againstLabel?: string
  /** 选中块在原页里的范围（0–1：左、上、右、下），“选中块周围”用 */
  focus?: [number, number, number, number] | null
  /** 能不能设渐变（背景色才有） */
  gradient?: boolean
  /** 现有的色标，改渐变时作起点 */
  stops?: RGB3[]
}>()
const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  (e: 'apply', c: RGB3): void
  (e: 'apply-gradient', stops: RGB3[], dir: 'hgrad' | 'vgrad'): void
}>()

const open = computed({ get: () => props.modelValue, set: (v: boolean) => emit('update:modelValue', v) })
const tab = ref<'pick' | 'page' | 'wheel' | 'check'>('pick')
const cur = ref<RGB3>([0, 0, 0])
const orig = ref<RGB3>([0, 0, 0])
const hsv = ref<[number, number, number]>([0, 0, 0])

// ---------- 本机记住的：自定义、历史、高级开关 ----------
const LS_CUSTOM = 'cc-color-custom'
const LS_HIST = 'cc-color-history'
const LS_SAFE = 'cc-color-safe'
const rd = (k: string): string[] => {
  try {
    const v = JSON.parse(localStorage.getItem(k) || '[]')
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string' && hexToRgb(x)) : []
  } catch {
    return []
  }
}
const wr = (k: string, v: unknown) => {
  try {
    localStorage.setItem(k, JSON.stringify(v))
  } catch {
    /* 记不住也不影响 */
  }
}
const custom = ref<string[]>(rd(LS_CUSTOM).slice(0, 16))
const history = ref<string[]>(rd(LS_HIST).slice(0, 12))
const safeOn = ref(false)
try {
  safeOn.value = localStorage.getItem(LS_SAFE) === '1'
} catch {
  /* 默认关 */
}
watch(safeOn, (v) => {
  try {
    localStorage.setItem(LS_SAFE, v ? '1' : '0')
  } catch {
    /* 记不住也不影响 */
  }
})
const advanced = ref(false)

// ---------- 当前色与各种写法 ----------
function setColor(c: readonly number[], keepHue = false) {
  const rgb: RGB3 = [Math.round(c[0]), Math.round(c[1]), Math.round(c[2])]
  cur.value = rgb
  const h = rgbToHsv(rgb)
  // 灰色没有色相，别把拖着的色相丢了
  hsv.value = keepHue || h[1] === 0 || h[2] === 0 ? [hsv.value[0], h[1], h[2]] : h
}
function setHsv(h: number, s: number, v: number) {
  hsv.value = [h, s, v]
  cur.value = hsvToRgb(h, s, v)
}
const hex = computed(() => rgbToHex(cur.value))
const hexIn = ref('')
const rgbIn = ref('')
watch(
  cur,
  (c) => {
    hexIn.value = rgbToHex(c)
    rgbIn.value = `RGB(${c[0]}, ${c[1]}, ${c[2]})`
  },
  { immediate: true },
)
function commitHex() {
  const c = hexToRgb(hexIn.value)
  if (c) setColor(c)
  else hexIn.value = hex.value
}
function commitRgbText() {
  const c = parseRgbText(rgbIn.value)
  if (c) setColor(c)
  else rgbIn.value = `RGB(${cur.value.join(', ')})`
}
const hsl = computed(() => rgbToHsl(cur.value).map((v) => Math.round(v)) as [number, number, number])
const lab = computed(() => rgbToLab(cur.value).map((v) => Math.round(v * 10) / 10) as [number, number, number])
const name = computed(() => colorName(cur.value))
const ycc = computed(() => rgbToYcbcr709(cur.value).map((v) => Math.round(v)) as [number, number, number])
const safe = computed(() => clampSafe(cur.value, SAFE_DEFAULT))
function numIn(i: number, v: number | undefined) {
  if (v === undefined || Number.isNaN(v)) return
  const c = [...cur.value] as RGB3
  c[i] = Math.min(255, Math.max(0, Math.round(v)))
  setColor(c)
}
function hsvIn(i: number, v: number | undefined) {
  if (v === undefined || Number.isNaN(v)) return
  const h = [...hsv.value] as [number, number, number]
  h[i] = Math.min(i === 0 ? 360 : 100, Math.max(0, v))
  setHsv(...h)
}
function hslIn(i: number, v: number | undefined) {
  if (v === undefined || Number.isNaN(v)) return
  const h = [...hsl.value] as [number, number, number]
  h[i] = Math.min(i === 0 ? 360 : 100, Math.max(0, v))
  setColor(hslToRgb(...h), true)
}

// ---------- 色域：拖动只改色块和指示器 ----------
const svEl = ref<HTMLElement | null>(null)
const hueEl = ref<HTMLElement | null>(null)
function dragOn(el: HTMLElement | null, e: PointerEvent, f: (x: number, y: number) => void) {
  if (!el) return
  el.setPointerCapture(e.pointerId)
  const go = (ev: PointerEvent) => {
    const r = el.getBoundingClientRect()
    f(Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (ev.clientY - r.top) / r.height)))
  }
  go(e)
  const mv = (ev: PointerEvent) => go(ev)
  const up = () => {
    el.removeEventListener('pointermove', mv)
    el.removeEventListener('pointerup', up)
  }
  el.addEventListener('pointermove', mv)
  el.addEventListener('pointerup', up)
}
const onSv = (e: PointerEvent) => dragOn(svEl.value, e, (x, y) => setHsv(hsv.value[0], x * 100, (1 - y) * 100))
const onHue = (e: PointerEvent) => dragOn(hueEl.value, e, (x) => setHsv(x * 360, hsv.value[1] || 60, hsv.value[2] || 80))
function nudge(e: KeyboardEvent) {
  const d = e.shiftKey ? 10 : 1
  const [h, s, v] = hsv.value
  if (e.key === 'ArrowLeft') setHsv(h, Math.max(0, s - d), v)
  else if (e.key === 'ArrowRight') setHsv(h, Math.min(100, s + d), v)
  else if (e.key === 'ArrowUp') setHsv(h, s, Math.min(100, v + d))
  else if (e.key === 'ArrowDown') setHsv(h, s, Math.max(0, v - d))
  else return
  e.preventDefault()
}
const hueColor = computed(() => rgbCss(hsvToRgb(hsv.value[0], 100, 100)))

// ---------- 吸管 ----------
const eyeOk = typeof window !== 'undefined' && 'EyeDropper' in window
async function screenPick() {
  try {
    const r = await new (window as unknown as { EyeDropper: new () => { open: () => Promise<{ sRGBHex: string }> } }).EyeDropper().open()
    const c = hexToRgb(r.sRGBHex)
    if (c) setColor(c)
  } catch {
    /* 取消 */
  }
}

// ---------- 原页：像素、点取、框选提取、取样线渐变 ----------
let px: PixelImage | null = null
const pxReady = ref(false)
function loadPixels() {
  px = null
  pxReady.value = false
  if (!props.image) return
  const im = new Image()
  im.onload = () => {
    const c = document.createElement('canvas')
    c.width = im.naturalWidth
    c.height = im.naturalHeight
    const g = c.getContext('2d', { willReadFrequently: true })
    if (!g) return
    g.drawImage(im, 0, 0)
    px = g.getImageData(0, 0, c.width, c.height)
    pxReady.value = true
    runExtract()
  }
  im.src = props.image
}
const imgEl = ref<HTMLElement | null>(null)
type Tool = 'pick' | 'box' | 'line'
const tool = ref<Tool>('pick')
const hover = ref<{ x: number; y: number; c: RGB3 } | null>(null)
const box = ref<[number, number, number, number] | null>(null) // 0–1
const line = ref<[number, number, number, number] | null>(null)
const scope = ref<'page' | 'focus' | 'box'>('page')
const mood = ref<Mood>('auto')
const count = ref(5)
const found = ref<Extracted[]>([])
const gradStops = ref<RGB3[]>([])
const gradDir = ref<'hgrad' | 'vgrad'>('hgrad')
const gradN = ref(4)

function frac(e: PointerEvent): [number, number] {
  const r = imgEl.value!.getBoundingClientRect()
  return [Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height))]
}
function radiusOf(e: { shiftKey: boolean; altKey: boolean }) {
  return e.altKey ? 4 : e.shiftKey ? 2 : 0
}
function sampleAt(f: [number, number], r: number): RGB3 | null {
  if (!px) return null
  return pixelAverage(px, f[0] * (px.width - 1), f[1] * (px.height - 1), r)
}
function pxBox(b: [number, number, number, number]): [number, number, number, number] {
  return [Math.floor(b[0] * px!.width), Math.floor(b[1] * px!.height), Math.ceil(b[2] * px!.width), Math.ceil(b[3] * px!.height)]
}
function runExtract() {
  if (!px) return
  let b: [number, number, number, number] | undefined
  if (scope.value === 'box' && box.value) b = pxBox(box.value)
  else if (scope.value === 'focus' && props.focus) {
    const [l, t, r, bt] = props.focus
    const w = r - l
    const h = bt - t
    b = pxBox([Math.max(0, l - w), Math.max(0, t - h), Math.min(1, r + w), Math.min(1, bt + h)])
  }
  found.value = extractPalette(px, count.value, mood.value, b)
}
function runGrad() {
  if (!px || !line.value) return
  const [a, b, c, d] = line.value
  gradStops.value = sampleGradient(px, [a * (px.width - 1), b * (px.height - 1)], [c * (px.width - 1), d * (px.height - 1)], gradN.value)
  gradDir.value = Math.abs(c - a) >= Math.abs(d - b) ? 'hgrad' : 'vgrad'
}
let down: [number, number] | null = null
function imgDown(e: PointerEvent) {
  if (!px) return
  imgEl.value?.setPointerCapture(e.pointerId)
  down = frac(e)
  if (tool.value === 'box') box.value = [...down, ...down]
  else if (tool.value === 'line') line.value = [...down, ...down]
}
function imgMove(e: PointerEvent) {
  const f = frac(e)
  const c = sampleAt(f, radiusOf(e))
  hover.value = c ? { x: f[0], y: f[1], c } : null
  if (!down) return
  if (tool.value === 'box') box.value = [Math.min(down[0], f[0]), Math.min(down[1], f[1]), Math.max(down[0], f[0]), Math.max(down[1], f[1])]
  else if (tool.value === 'line') line.value = [down[0], down[1], f[0], f[1]]
}
function imgUp(e: PointerEvent) {
  if (!down) return
  const f = frac(e)
  if (tool.value === 'pick') {
    const c = sampleAt(f, radiusOf(e))
    if (c) setColor(c)
  } else if (tool.value === 'box') {
    if (box.value && (box.value[2] - box.value[0] < 0.01 || box.value[3] - box.value[1] < 0.01)) box.value = null
    else {
      scope.value = 'box'
      runExtract()
    }
  } else if (tool.value === 'line') {
    runGrad()
  }
  down = null
}
function imgKey(e: KeyboardEvent) {
  if (e.key === 'Escape') tool.value = 'pick'
}
watch([scope, mood, count], runExtract)
watch(gradN, runGrad)
const boxStyle = (b: [number, number, number, number]) => ({ left: `${b[0] * 100}%`, top: `${b[1] * 100}%`, width: `${(b[2] - b[0]) * 100}%`, height: `${(b[3] - b[1]) * 100}%` })
const gradCss = computed(() => gradientCss(gradStops.value, gradDir.value))
function dragStop(i: number, e: PointerEvent) {
  // 拖色标：沿取样线重新取这个位置的颜色
  const bar = (e.currentTarget as HTMLElement).parentElement!
  const el = e.currentTarget as HTMLElement
  el.setPointerCapture(e.pointerId)
  const mv = (ev: PointerEvent) => {
    if (!px || !line.value) return
    const r = bar.getBoundingClientRect()
    const t = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width))
    const [a, b, c, d] = line.value
    const s = pixelAverage(px, (a + (c - a) * t) * (px.width - 1), (b + (d - b) * t) * (px.height - 1), 1)
    gradStops.value = gradStops.value.map((v, k) => (k === i ? s : v))
  }
  const up = () => {
    el.removeEventListener('pointermove', mv)
    el.removeEventListener('pointerup', up)
  }
  el.addEventListener('pointermove', mv)
  el.addEventListener('pointerup', up)
}

// ---------- 配色 ----------
const SCHEMES = ['类似', '单色', '互补', '分裂互补', '三色', '四方']
const scheme = ref('类似')
const wheel = computed(() => wheelPalette(cur.value, scheme.value))
const oldPairs = computed(() => Object.entries(HARMONY_SCHEMES).map(([n, o]) => ({ n, list: [cur.value, ...o.map((d) => hsvToRgb(hsv.value[0] + d, hsv.value[1], hsv.value[2]))] })))
function wheelDot(c: RGB3) {
  const [h, s] = rgbToHsv(c)
  const a = ((h - 90) * Math.PI) / 180
  const r = (s / 100) * 46
  return { left: `${50 + Math.cos(a) * r}%`, top: `${50 + Math.sin(a) * r}%`, background: rgbCss(c) }
}

// ---------- 检查：对比度、色盲 ----------
const againstC = computed<RGB3 | null>(() => props.against ?? null)
const ratio = computed(() => (againstC.value ? contrastRatio(cur.value, againstC.value) : 0))
const against2 = ref<'against' | 'white' | 'black'>('against')
const baseAgainst = computed<RGB3>(() => (against2.value === 'white' ? [255, 255, 255] : against2.value === 'black' ? [0, 0, 0] : (againstC.value ?? [255, 255, 255])))
const ratioBase = computed(() => contrastRatio(cur.value, baseAgainst.value))
const cbRows = computed(() =>
  (Object.keys(COLORBLIND_NAMES) as ColorBlind[]).map((k) => {
    const a = simulateColorblind(cur.value, k)
    const b = simulateColorblind(baseAgainst.value, k)
    return { k, name: COLORBLIND_NAMES[k], a, b, ratio: contrastRatio(a, b), diff: deltaE(a, b) }
  }),
)
function fixContrast(target: number) {
  const r = adjustForContrast(cur.value, baseAgainst.value, target)
  if (r) setColor(r)
}
const canFix = computed(() => adjustForContrast(cur.value, baseAgainst.value, 4.5) !== null)

// ---------- 自定义 / 历史 ----------
function addCustom() {
  const h = hex.value
  custom.value = [h, ...custom.value.filter((x) => x !== h)].slice(0, 16)
  wr(LS_CUSTOM, custom.value)
}
function addAll(list: readonly (readonly number[])[]) {
  const add = list.map((c) => rgbToHex(c))
  custom.value = [...add, ...custom.value.filter((x) => !add.includes(x))].slice(0, 16)
  wr(LS_CUSTOM, custom.value)
}
function removeCustom(h: string) {
  custom.value = custom.value.filter((x) => x !== h)
  wr(LS_CUSTOM, custom.value)
}
const copied = ref('')
async function copyList(list: readonly (readonly number[])[]) {
  const t = list.map((c) => rgbToHex(c)).join(' ')
  try {
    await navigator.clipboard.writeText(t)
    copied.value = '已复制'
  } catch {
    copied.value = t
  }
  setTimeout(() => (copied.value = ''), 2500)
}
const pickHex = (h: string) => {
  const c = hexToRgb(h)
  if (c) setColor(c)
}

function applyColor() {
  let c = cur.value
  if (safeOn.value && safe.value.clamped) c = safe.value.rgb
  const h = rgbToHex(c)
  history.value = [h, ...history.value.filter((x) => x !== h)].slice(0, 12)
  wr(LS_HIST, history.value)
  emit('apply', c)
  open.value = false
}
function applyGradient() {
  if (gradStops.value.length < 2) return
  emit('apply-gradient', gradStops.value, gradDir.value)
  open.value = false
}

watch(
  () => [props.modelValue, props.color] as const,
  ([o, c]) => {
    if (!o) return
    orig.value = [...c] as RGB3
    setColor(c)
    tab.value = 'pick'
    tool.value = 'pick'
    box.value = null
    line.value = null
    scope.value = props.focus ? 'focus' : 'page'
    gradStops.value = props.stops?.length ? [...props.stops] : []
    advanced.value = false
    loadPixels()
    void nextTick()
  },
  { immediate: true },
)
watch(() => props.image, loadPixels)
onBeforeUnmount(() => {
  px = null
})
const safeNote = computed(() => (safeOn.value ? (safe.value.clamped ? `超出广播安全范围（Y ${ycc.value[0]}，Cb ${ycc.value[1]}，Cr ${ycc.value[2]}），应用时会拉回 ${rgbToHex(safe.value.rgb)}` : `在安全范围内（Y ${ycc.value[0]}，Cb ${ycc.value[1]}，Cr ${ycc.value[2]}）`) : ''))
</script>

<template>
  <el-dialog v-model="open" :title="title" width="780px" class="ccp-dlg" append-to-body destroy-on-close align-center>
    <div class="ccp">
      <div class="top">
        <div class="cmp" :title="`左：原来 ${rgbToHex(orig)}　右：现在 ${hex}`">
          <i :style="{ background: rgbCss(orig) }" />
          <i :style="{ background: rgbCss(cur) }" />
        </div>
        <div class="tinfo">
          <b class="mono">{{ hex }}</b>
          <span class="mute">{{ name }}</span>
        </div>
        <span class="sp" />
        <el-button v-if="eyeOk" size="small" @click="screenPick">屏幕取色</el-button>
        <el-button size="small" @click="setColor(orig)">还原</el-button>
      </div>

      <div class="tabs" role="tablist">
        <button v-for="t in ([['pick', '取色'], ['page', '原页'], ['wheel', '配色'], ['check', '对比度与色盲']] as const)" :key="t[0]" role="tab" :aria-selected="tab === t[0]" :class="{ on: tab === t[0] }" :disabled="t[0] === 'page' && !image" :title="t[0] === 'page' && !image ? '没有原页图' : undefined" @click="tab = t[0]">{{ t[1] }}</button>
      </div>

      <!-- 取色 -->
      <div v-show="tab === 'pick'" class="pane pick">
        <div class="l">
          <div ref="svEl" class="sv" tabindex="0" role="slider" aria-label="饱和度与明度" :aria-valuetext="`饱和度 ${Math.round(hsv[1])}，明度 ${Math.round(hsv[2])}`" :style="{ background: hueColor }" @pointerdown.prevent="onSv" @keydown="nudge">
            <i class="w" /><i class="k" />
            <b class="dot" :style="{ left: hsv[1] + '%', top: 100 - hsv[2] + '%', background: rgbCss(cur) }" />
          </div>
          <div ref="hueEl" class="hue" role="slider" aria-label="色相" :aria-valuenow="Math.round(hsv[0])" aria-valuemin="0" aria-valuemax="360" @pointerdown.prevent="onHue"><b class="dot" :style="{ left: (hsv[0] / 360) * 100 + '%', background: hueColor }" /></div>
          <div class="bas" aria-label="基本颜色">
            <button v-for="c in BASIC_COLORS" :key="c" :style="{ background: c }" :title="c" :class="{ on: c === hex }" @click="pickHex(c)" />
          </div>
        </div>
        <div class="r">
          <div class="f2"><label>Hex</label><el-input v-model="hexIn" size="small" class="mono" @change="commitHex" @keydown.enter="commitHex" /></div>
          <div class="f2"><label>RGB</label><el-input v-model="rgbIn" size="small" class="mono" @change="commitRgbText" @keydown.enter="commitRgbText" /></div>
          <div class="nums">
            <label>R</label><el-input-number :model-value="cur[0]" :min="0" :max="255" size="small" :controls="false" @change="(v: number | undefined) => numIn(0, v)" />
            <label>G</label><el-input-number :model-value="cur[1]" :min="0" :max="255" size="small" :controls="false" @change="(v: number | undefined) => numIn(1, v)" />
            <label>B</label><el-input-number :model-value="cur[2]" :min="0" :max="255" size="small" :controls="false" @change="(v: number | undefined) => numIn(2, v)" />
            <label>H</label><el-input-number :model-value="Math.round(hsv[0])" :min="0" :max="360" size="small" :controls="false" @change="(v: number | undefined) => hsvIn(0, v)" />
            <label>S</label><el-input-number :model-value="Math.round(hsv[1])" :min="0" :max="100" size="small" :controls="false" @change="(v: number | undefined) => hsvIn(1, v)" />
            <label>V</label><el-input-number :model-value="Math.round(hsv[2])" :min="0" :max="100" size="small" :controls="false" @change="(v: number | undefined) => hsvIn(2, v)" />
            <label>HSL·L</label><el-input-number :model-value="hsl[2]" :min="0" :max="100" size="small" :controls="false" @change="(v: number | undefined) => hslIn(2, v)" />
            <label>Lab</label><span class="mono lab">{{ lab[0] }}, {{ lab[1] }}, {{ lab[2] }}</span>
          </div>
          <div class="sec"><b>自定义</b><el-button size="small" text @click="addCustom">＋ 添加当前色</el-button></div>
          <div class="cus" aria-label="自定义颜色">
            <button v-for="c in custom" :key="c" :style="{ background: c }" :title="`${c}（右键删除）`" :class="{ on: c === hex }" @click="pickHex(c)" @contextmenu.prevent="removeCustom(c)" />
            <span v-if="!custom.length" class="mute">还没有，点“添加当前色”</span>
          </div>
          <div class="sec"><b>最近用过</b></div>
          <div class="cus" aria-label="历史">
            <button v-for="c in history" :key="c" :style="{ background: c }" :title="c" :class="{ on: c === hex }" @click="pickHex(c)" />
            <span v-if="!history.length" class="mute">应用过的颜色会出现在这里</span>
          </div>
          <div class="adv" role="button" tabindex="0" @click="advanced = !advanced" @keydown.enter="advanced = !advanced">{{ advanced ? '▾' : '▸' }} 高级</div>
          <div v-if="advanced" class="advbox">
            <el-checkbox v-model="safeOn" size="small">限制在 BT.709 广播安全色域（Y 16–235，CbCr 16–240）</el-checkbox>
            <small class="mute">做视频才需要；做 PDF 用不到，默认关。</small>
            <small v-if="safeNote" :class="safe.clamped ? 'warn' : 'mute'">{{ safeNote }}</small>
          </div>
        </div>
      </div>

      <!-- 原页 -->
      <div v-if="image" v-show="tab === 'page'" class="pane pg">
        <div class="l">
          <div class="tools" role="radiogroup" aria-label="在原页上做什么">
            <button :class="{ on: tool === 'pick' }" role="radio" :aria-checked="tool === 'pick'" @click="tool = 'pick'">吸管</button>
            <button :class="{ on: tool === 'box' }" role="radio" :aria-checked="tool === 'box'" title="拖一个框，只从框里提取颜色" @click="tool = 'box'">框选范围</button>
            <button v-if="gradient" :class="{ on: tool === 'line' }" role="radio" :aria-checked="tool === 'line'" title="拖一条线，沿线取渐变" @click="tool = 'line'">取样线</button>
          </div>
          <div ref="imgEl" class="pimg" tabindex="0" :class="tool" @pointerdown.prevent="imgDown" @pointermove="imgMove" @pointerup="imgUp" @pointerleave="hover = null" @keydown="imgKey">
            <img :src="image" alt="原页" draggable="false" />
            <i v-if="box" class="box" :style="boxStyle(box)" />
            <i v-if="focus && scope === 'focus'" class="box foc" :style="boxStyle([focus[0], focus[1], focus[2], focus[3]])" />
            <svg v-if="line" class="ln" viewBox="0 0 1 1" preserveAspectRatio="none"><line :x1="line[0]" :y1="line[1]" :x2="line[2]" :y2="line[3]" vector-effect="non-scaling-stroke" /></svg>
            <span v-if="hover" class="tip" :style="{ left: hover.x * 100 + '%', top: hover.y * 100 + '%' }"><i :style="{ background: rgbCss(hover.c) }" />{{ rgbToHex(hover.c) }}</span>
          </div>
          <small class="mute">单击取色　Shift 取 5×5 平均　Alt 取 9×9 平均　Esc 回到吸管</small>
        </div>
        <div class="r">
          <div class="sec"><b>提取一组颜色</b></div>
          <div class="row">
            <span class="mute">范围</span>
            <div class="seg"><span :class="{ on: scope === 'page' }" @click="scope = 'page'">整页</span><span v-if="focus" :class="{ on: scope === 'focus' }" @click="scope = 'focus'">选中块周围</span><span :class="{ on: scope === 'box' }" :title="box ? '' : '先用“框选范围”在左边拖一个框'" @click="box ? (scope = 'box') : (tool = 'box')">框选区域</span></div>
          </div>
          <div class="row">
            <span class="mute">风格</span>
            <div class="seg"><span v-for="m in MOODS" :key="m.id" :class="{ on: mood === m.id }" @click="mood = m.id">{{ m.name }}</span></div>
          </div>
          <div class="row"><span class="mute">数量</span><el-slider v-model="count" :min="2" :max="8" :step="1" size="small" class="sl" /></div>
          <div class="sw5">
            <div v-for="(f, i) in found" :key="i" class="sw">
              <div class="chip" :style="{ background: rgbCss(f.rgb) }"><em>{{ i + 1 }}</em></div>
              <div class="mono">{{ rgbToHex(f.rgb) }}</div>
              <div class="mute sh">占 {{ Math.round(f.share * 100) }}%</div>
              <button @click="setColor(f.rgb)">选用</button>
            </div>
            <span v-if="!found.length" class="mute">{{ pxReady ? '这个范围里没有可取的颜色' : '正在读取页面…' }}</span>
          </div>
          <div v-if="found.length" class="row"><el-button size="small" @click="addAll(found.map((f) => f.rgb))">全部加入自定义</el-button><el-button size="small" @click="copyList(found.map((f) => f.rgb))">复制色值</el-button><small class="mute">{{ copied }}</small></div>

          <template v-if="gradient">
            <div class="sec"><b>取一条渐变（给“渐变背景”）</b></div>
            <small class="mute">切到“取样线”，在原页上拖一条线，沿线取色标。</small>
            <div v-if="gradStops.length >= 2" class="gbar" :style="{ background: gradCss }">
              <b v-for="(s, i) in gradStops" :key="i" :style="{ left: (i / (gradStops.length - 1)) * 100 + '%', background: rgbCss(s) }" :title="rgbToHex(s)" @pointerdown.prevent="dragStop(i, $event)" />
            </div>
            <div v-if="gradStops.length >= 2" class="row">
              <span class="mute">色标</span><el-input-number v-model="gradN" :min="2" :max="6" size="small" />
              <div class="seg"><span :class="{ on: gradDir === 'hgrad' }" @click="gradDir = 'hgrad'">左 → 右</span><span :class="{ on: gradDir === 'vgrad' }" @click="gradDir = 'vgrad'">上 → 下</span></div>
              <span class="sp" /><el-button size="small" type="primary" @click="applyGradient">应用渐变</el-button>
            </div>
          </template>
        </div>
      </div>

      <!-- 配色 -->
      <div v-show="tab === 'wheel'" class="pane wh">
        <div class="l">
          <div class="wheel" role="img" aria-label="色轮">
            <b v-for="(c, i) in wheel" :key="i" :style="wheelDot(c)" :class="{ cen: i === 2 }" />
          </div>
          <small class="mute">中间那个是当前色，其余按规则排开。</small>
        </div>
        <div class="r">
          <div class="row"><span class="mute">规则</span><div class="seg"><span v-for="s in SCHEMES" :key="s" :class="{ on: scheme === s }" @click="scheme = s">{{ s }}</span></div></div>
          <div class="sw5">
            <div v-for="(c, i) in wheel" :key="i" class="sw">
              <div class="chip" :style="{ background: rgbCss(c) }" />
              <div class="mono">{{ rgbToHex(c) }}</div>
              <div class="mute sh">{{ colorName(c) }}</div>
              <button @click="setColor(c)">选用</button>
            </div>
          </div>
          <div class="row"><el-button size="small" @click="addAll(wheel)">全部加入自定义</el-button><el-button size="small" @click="copyList(wheel)">复制色值</el-button><small class="mute">{{ copied }}</small></div>
          <div class="sec"><b>常用搭配</b></div>
          <div v-for="p in oldPairs" :key="p.n" class="pair"><span>{{ p.n }}</span><button v-for="(c, i) in p.list" :key="i" :style="{ background: rgbCss(c) }" :title="rgbToHex(c)" @click="setColor(c)" /></div>
        </div>
      </div>

      <!-- 对比度与色盲 -->
      <div v-show="tab === 'check'" class="pane ck">
        <div class="l">
          <div class="row"><span class="mute">对着</span>
            <div class="seg"><span v-if="against" :class="{ on: against2 === 'against' }" @click="against2 = 'against'">{{ againstLabel || '对方' }}</span><span :class="{ on: against2 === 'white' }" @click="against2 = 'white'">白</span><span :class="{ on: against2 === 'black' }" @click="against2 = 'black'">黑</span></div>
          </div>
          <div class="big" :class="{ ok: ratioBase >= 4.5, mid: ratioBase >= 3 && ratioBase < 4.5, bad: ratioBase < 3 }">
            <b>{{ ratioBase.toFixed(1) }}</b><span>: 1</span>
          </div>
          <div class="sample" :style="{ background: rgbCss(baseAgainst), color: rgbCss(cur) }"><b>译文示例 Aa 123</b><small>小字号的样子</small></div>
          <div class="lv">
            <span :class="{ y: wcag(ratioBase).aa }">普通文字 AA（4.5）</span><span :class="{ y: wcag(ratioBase).aaa }">AAA（7）</span>
            <span :class="{ y: wcag(ratioBase).aaLarge }">大字 AA（3）</span><span :class="{ y: wcag(ratioBase).aaaLarge }">大字 AAA（4.5）</span>
          </div>
          <div class="row"><el-button size="small" :disabled="!canFix || ratioBase >= 4.5" title="保持色相和饱和度，只调明度" @click="fixContrast(4.5)">调到 AA（保持色相）</el-button><el-button size="small" :disabled="ratioBase >= 7" @click="fixContrast(7)">调到 AAA</el-button></div>
          <small v-if="!canFix && ratioBase < 4.5" class="warn">这个底色上调不出够用的文字色，换一个底色试试。</small>
          <small v-if="againstC" class="mute">{{ ratioLabel(ratio) }}（对{{ againstLabel || '对方' }}）</small>
        </div>
        <div class="r">
          <div class="sec"><b>色盲下的样子</b></div>
          <div class="cb">
            <div v-for="r in cbRows" :key="r.k" class="cbr">
              <div class="s" :style="{ background: rgbCss(r.b), color: rgbCss(r.a) }">Aa</div>
              <div class="t"><b>{{ r.name }}</b><span class="mute">{{ ratioLabel(r.ratio) }}</span></div>
              <em v-if="r.ratio < 3" class="warn">几乎看不清</em>
              <em v-else-if="r.diff < 10" class="warn">两色很接近</em>
            </div>
          </div>
          <small class="mute">只靠颜色区分意思的地方（红 / 绿）要小心；文字和底色的明度差够大，色盲也能读。</small>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="foot">
        <span v-if="safeOn && safe.clamped" class="warn">将拉回安全色域：{{ rgbToHex(safe.rgb) }}</span>
        <span class="sp" />
        <el-button @click="open = false">取消</el-button>
        <el-button type="primary" @click="applyColor">应用 {{ hex }}</el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.ccp { display: flex; flex-direction: column; gap: 10px; min-height: 420px }
.mono { font-family: ui-monospace, Menlo, Consolas, monospace }
.mute { color: var(--cc-mute); font-size: 12.5px }
.warn { color: var(--cc-amber); font-size: 12.5px }
.sp { flex: 1 }
.top { display: flex; align-items: center; gap: 10px }
.cmp { display: flex; width: 74px; height: 38px; border-radius: 8px; overflow: hidden; border: 1px solid var(--cc-line) }
.cmp i { flex: 1 }
.tinfo { display: flex; flex-direction: column; line-height: 1.3 }
.tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--cc-line) }
.tabs button { border: 0; background: none; padding: 7px 14px; cursor: pointer; font: inherit; color: var(--cc-mute); border-bottom: 2px solid transparent }
.tabs button.on { color: var(--cc-amber); border-bottom-color: var(--cc-amber); font-weight: 700 }
.tabs button:disabled { opacity: .4; cursor: not-allowed }
.pane { display: flex; gap: 18px; min-height: 0 }
.pane .l { width: 300px; flex: none; display: flex; flex-direction: column; gap: 10px }
.pane .r { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px }
.sv { position: relative; height: 170px; border-radius: 8px; cursor: crosshair; touch-action: none; outline-offset: 2px }
.sv .w, .sv .k { position: absolute; inset: 0; border-radius: 8px }
.sv .w { background: linear-gradient(to right, #fff, rgba(255, 255, 255, 0)) }
.sv .k { background: linear-gradient(to top, #000, rgba(0, 0, 0, 0)) }
.dot { position: absolute; width: 14px; height: 14px; margin: -7px 0 0 -7px; border-radius: 50%; border: 2px solid #fff; box-shadow: 0 0 0 1px #1d2433; box-sizing: border-box; pointer-events: none }
.hue { position: relative; height: 14px; border-radius: 7px; cursor: pointer; touch-action: none; background: linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00) }
.hue .dot { top: 50% }
.bas { display: grid; grid-template-columns: repeat(8, 1fr); gap: 3px }
.bas button, .cus button { height: 22px; border: 1px solid var(--cc-line); border-radius: 4px; cursor: pointer; padding: 0 }
.bas button.on, .cus button.on { outline: 2px solid var(--cc-amber); outline-offset: 1px }
.cus { display: flex; flex-wrap: wrap; gap: 4px }
.cus button { width: 24px }
.f2 { display: flex; align-items: center; gap: 8px }
.f2 label { width: 34px; color: var(--cc-mute); font-size: 12.5px }
.nums { display: grid; grid-template-columns: 44px 1fr 44px 1fr 44px 1fr; gap: 6px 6px; align-items: center }
.nums label { color: var(--cc-mute); font-size: 12px }
.nums :deep(.el-input-number) { width: 100% }
.nums .lab { grid-column: 2 / 7; font-size: 12.5px }
.sec { display: flex; align-items: center; justify-content: space-between; margin-top: 4px }
.adv { color: var(--cc-amber); cursor: pointer; font-size: 12.5px; margin-top: 6px; user-select: none }
.advbox { display: flex; flex-direction: column; gap: 4px; padding: 8px 10px; border: 1px solid var(--cc-line); border-radius: 8px; background: var(--cc-fill) }
.row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap }
.seg { display: flex; background: var(--cc-inset); border: 1px solid var(--cc-line); border-radius: 8px; padding: 2px; gap: 2px; flex-wrap: wrap }
.seg span { padding: 2px 10px; border-radius: 6px; cursor: pointer; font-size: 12.5px; color: var(--cc-mute) }
.seg .on { background: var(--cc-amber); color: var(--cc-on-acc); font-weight: 700 }
.sl { flex: 1; min-width: 120px }
.tools { display: flex; gap: 4px }
.tools button { border: 1px solid var(--cc-line); background: var(--cc-card); border-radius: 6px; padding: 3px 10px; cursor: pointer; font: inherit; font-size: 12.5px; color: var(--cc-fg) }
.tools button.on { background: var(--cc-amber); color: var(--cc-on-acc); border-color: var(--cc-amber) }
.pimg { position: relative; border: 1px solid var(--cc-line); line-height: 0; touch-action: none; user-select: none; max-height: 380px; overflow: hidden }
.pimg.pick { cursor: crosshair }
.pimg.box, .pimg.line { cursor: copy }
.pimg img { width: 100%; height: auto; display: block }
.pimg .box { position: absolute; border: 2px dashed #3b82f6; background: rgba(59, 130, 246, .12); box-sizing: border-box; pointer-events: none }
.pimg .box.foc { border-color: #d9762b; background: none }
.pimg .ln { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none }
.pimg .ln line { stroke: #fff; stroke-width: 3; filter: drop-shadow(0 0 1px #000) }
.pimg .tip { position: absolute; transform: translate(10px, 10px); background: #1d2433; color: #fff; font: 11px ui-monospace, Menlo, Consolas, monospace; padding: 2px 6px; border-radius: 6px; display: flex; gap: 5px; align-items: center; line-height: 1.3; pointer-events: none; white-space: nowrap }
.pimg .tip i { width: 12px; height: 12px; border: 1px solid #fff; border-radius: 3px }
.sw5 { display: flex; gap: 8px; flex-wrap: wrap }
.sw { width: 78px; display: flex; flex-direction: column; gap: 3px; text-align: center; font-size: 12px }
.sw .chip { position: relative; height: 54px; border-radius: 8px; border: 1px solid var(--cc-line) }
.sw .chip em { position: absolute; left: 4px; top: 4px; width: 16px; height: 16px; border-radius: 8px; background: #1d2433; color: #fff; font-size: 10px; font-style: normal; display: flex; align-items: center; justify-content: center }
.sw .sh { font-size: 11px }
.sw button { height: 22px; border: 1px solid var(--cc-line); border-radius: 6px; background: var(--cc-card); cursor: pointer; font: inherit; font-size: 12px; color: var(--cc-fg) }
.sw button:hover { border-color: var(--cc-amber); color: var(--cc-amber) }
.gbar { position: relative; height: 40px; border-radius: 8px; border: 1px solid var(--cc-line); margin-bottom: 8px }
.gbar b { position: absolute; bottom: -8px; width: 14px; height: 14px; margin-left: -7px; border-radius: 50%; border: 2px solid #fff; box-shadow: 0 0 0 1px #1d2433; box-sizing: border-box; cursor: ew-resize; touch-action: none }
.wheel { position: relative; width: 240px; height: 240px; margin: 0 auto; border-radius: 50%; background: radial-gradient(circle, #fff 0, rgba(255, 255, 255, 0) 70%), conic-gradient(from 0deg, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00); border: 1px solid var(--cc-line) }
.wheel b { position: absolute; width: 22px; height: 22px; margin: -11px 0 0 -11px; border-radius: 50%; border: 3px solid #fff; box-shadow: 0 0 0 1px #1d2433, 0 2px 6px rgba(0, 0, 0, .4); box-sizing: border-box }
.wheel b.cen { width: 28px; height: 28px; margin: -14px 0 0 -14px }
.pair { display: flex; align-items: center; gap: 4px }
.pair span { width: 64px; font-size: 12.5px; color: var(--cc-mute) }
.pair button { width: 28px; height: 20px; border: 1px solid var(--cc-line); border-radius: 4px; cursor: pointer; padding: 0 }
.big { display: flex; align-items: baseline; gap: 4px; padding: 4px 0 }
.big b { font-size: 44px; line-height: 1; font-variant-numeric: tabular-nums }
.big span { color: var(--cc-mute) }
.big.ok b { color: var(--cc-cyan) }
.big.mid b { color: var(--cc-amber) }
.big.bad b { color: var(--cc-err) }
.sample { border: 1px solid var(--cc-line); border-radius: 8px; padding: 12px 14px; display: flex; flex-direction: column; gap: 2px }
.sample small { font-size: 11px }
.lv { display: flex; flex-wrap: wrap; gap: 6px }
.lv span { font-size: 12px; padding: 2px 8px; border-radius: 10px; border: 1px solid var(--cc-line); color: var(--cc-mute) }
.lv span.y { background: var(--cc-cyan-soft); border-color: var(--cc-cyan-line); color: var(--cc-cyan) }
.cb { display: flex; flex-direction: column; gap: 8px }
.cbr { display: flex; align-items: center; gap: 10px }
.cbr .s { width: 64px; height: 38px; border-radius: 8px; border: 1px solid var(--cc-line); display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 18px }
.cbr .t { display: flex; flex-direction: column; flex: 1 }
.cbr em { font-style: normal; font-size: 12px }
.foot { display: flex; align-items: center; gap: 8px }
</style>
