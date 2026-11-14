<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText } from './api'
import type { ScreenBlock, ScreenPayload } from './types'
import { ORIG_STYLE, blockStyle, overlaySvg, padOf, startFont, trStyle, viewOf } from './utils/overlayShot'
import { useTauriEvents } from './composables/useTauriEvent'
const ev = useTauriEvents()

/** 实时屏幕翻译的贴字层：透明、点击穿透。坐标是物理像素，按窗口实际大小换算成 CSS 像素（不依赖系统报的缩放比例，
 *  多个显示器缩放不同也对得上）；字号按框自动缩到放得下。竖排原文 → 竖排译文，横排 → 横排，原文居中 / 靠右的译文也跟着排。
 *  “保存译图”：把同样的排版画到一张干净的画面上存起来（贴字层本身不进截图）。 */
const payload = ref<ScreenPayload | null>(null)
const hidden = ref(false)
const paused = ref(false)
const note = ref('')
const els = ref<Record<number, HTMLElement | null>>({})
const fonts = ref<number[]>([])
const dpr = () => window.devicePixelRatio || 1
const view = () => viewOf([window.innerWidth, window.innerHeight], payload.value?.rect, dpr())
let offs: UnlistenFn[] = []
let noteTimer: ReturnType<typeof setTimeout> | undefined

function setEl(i: number, el: unknown) {
  els.value[i] = (el as HTMLElement | null) ?? null
}

function style(b: ScreenBlock, p: ScreenPayload, i: number) {
  return blockStyle(b, p, view(), fonts.value[i] ?? startFont(b, p, view()))
}

/** 字号从起始值往下缩，直到框没有被撑大（框设的是最小高度，放不下会长高）；不低于最小字号。返回缩好的字号。 */
function fit(el: HTMLElement, b: ScreenBlock, p: ScreenPayload): number {
  const v = view()
  const pad = padOf(b, p, v)
  let size = startFont(b, p, v)
  el.style.fontSize = `${size}px`
  const over = () => (b.vertical ? el.offsetWidth > b.w * v.sx + pad * 2 + 1 : el.offsetHeight > b.h * v.sy + pad * 2 + 1)
  let guard = 40
  while (over() && size > p.fontMin && guard-- > 0) {
    size = Math.max(p.fontMin, size - Math.max(0.5, size * 0.06))
    el.style.fontSize = `${size}px`
  }
  return size
}

async function layout() {
  const p = payload.value
  if (!p) return
  await nextTick()
  fonts.value = p.blocks.map((b, i) => {
    const el = els.value[i]
    return el ? fit(el, b, p) : startFont(b, p, view())
  })
}

async function apply(p: ScreenPayload) {
  payload.value = p
  await layout()
}

function say(t: string) {
  note.value = t
  clearTimeout(noteTimer)
  noteTimer = setTimeout(() => (note.value = ''), 4000)
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((ok, no) => {
    const img = new Image()
    img.onload = () => ok(img)
    img.onerror = () => no(new Error('图片加载失败'))
    img.src = src
  })
}

/** 取一帧干净的画面，把译文按当前排版叠上去，存成 PNG（同时放进剪贴板）。 */
async function saveShot() {
  try {
    const frame = await api.scShotFrame()
    const base = await loadImage(frame.png)
    const canvas = document.createElement('canvas')
    canvas.width = frame.rect[2]
    canvas.height = frame.rect[3]
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('画布不可用')
    ctx.drawImage(base, 0, 0)
    const p = payload.value
    if (p && !hidden.value && p.blocks.length) {
      const svg = overlaySvg(p, view(), fonts.value, window.innerWidth, window.innerHeight)
      const layer = await loadImage(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`)
      ctx.drawImage(layer, 0, 0, canvas.width, canvas.height)
    }
    const saved = await api.scShotSave(canvas.toDataURL('image/png'))
    say(`已保存译图${saved.copied ? '，也复制到了剪贴板' : ''}`)
  } catch (e) {
    say(`保存译图失败：${errorText(e)}`)
  }
}

const onResize = () => void layout()

onMounted(async () => {
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  window.addEventListener('resize', onResize)
  offs = await Promise.all([
    ev.on(api.onScreenBlocks((p) => void apply(p))),
    ev.on(api.onScreenHidden((h) => (hidden.value = h))),
    ev.on(api.onScreenState((s) => (paused.value = !!s.paused))),
    ev.on(api.onScreenShot(() => void saveShot())),
  ])
  const stt = await api.scStatus().catch(() => null) // 忽略：可选数据，取不到就按空处理
  if (stt) paused.value = !!stt.paused
  const cur = await api.scOverlayState().catch(() => null) // 忽略：可选数据，取不到就按空处理
  if (cur && !payload.value) void apply(cur)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  clearTimeout(noteTimer)
  offs.forEach((f) => f())
})
</script>

<template>
  <div v-if="!hidden" class="edge" :class="{ paused }"><span>{{ paused ? '已暂停' : '翻译中' }}</span></div>
  <div v-if="note" class="note">{{ note }}</div>
  <div v-if="payload && !hidden" class="ov">
    <div v-for="(b, i) in payload.blocks" :key="i + ':' + b.orig" :ref="(el) => setEl(i, el)" :style="style(b, payload, i)">
      <span v-if="payload.bilingual && b.orig" :style="ORIG_STYLE">{{ b.orig }}</span>
      <span :style="trStyle(b, payload)">{{ b.text }}</span>
    </div>
  </div>
</template>

<style>
html, body { margin: 0; background: transparent !important; overflow: hidden; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.edge { position: fixed; inset: 0; border: 2px solid #ffb547; border-radius: 4px; pointer-events: none; box-sizing: border-box; }
.edge.paused { border-style: dashed; opacity: 0.8; }
.edge span { position: absolute; left: 0; top: 0; background: #ffb547; color: #060a12; font: 600 11px/18px -apple-system, 'Microsoft YaHei', sans-serif; padding: 0 8px; border-radius: 0 0 4px 0; }
.note { position: fixed; right: 8px; top: 8px; max-width: 70%; background: rgba(30, 36, 48, 0.94); color: #fff; font: 600 12px/1.5 -apple-system, 'Microsoft YaHei', sans-serif; padding: 4px 10px; border-radius: 6px; pointer-events: none; }
.ov { position: fixed; inset: 0; pointer-events: none; }
</style>
