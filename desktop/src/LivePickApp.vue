<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { api } from './api'
import type { ScreenPick, ScreenWindow } from './types'

/** 选择要翻译的地方：整屏冻结，拖一个框，或者点一下某个窗口；Esc 取消。
 *  坐标换算：CSS 像素 → 截图像素（naturalWidth / innerWidth）→ 加上截图在屏幕上的起点。 */
type Rect = { x: number; y: number; w: number; h: number }
const info = ref<ScreenPick | null>(null)
const sel = ref<Rect | null>(null)
const hover = ref<ScreenWindow | null>(null)
let start: { x: number; y: number } | null = null
let nw = 1
let nh = 1

onMounted(async () => {
  window.addEventListener('keydown', onKey, true)
  const i = await api.scPickTake().catch(() => null)
  if (!i) return void api.scPickCancel()
  info.value = i
  const im = new Image()
  im.onload = () => {
    nw = im.naturalWidth || i.w
    nh = im.naturalHeight || i.h
  }
  im.src = i.dataUrl
})
onBeforeUnmount(() => window.removeEventListener('keydown', onKey, true))

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    void api.scPickCancel()
  }
}

const kx = () => nw / innerWidth
const ky = () => nh / innerHeight

/** 屏幕物理像素窗口 → 当前页面 CSS 像素 */
function cssOf(w: ScreenWindow): Rect {
  const i = info.value!
  return { x: (w.x - i.x) / kx(), y: (w.y - i.y) / ky(), w: w.w / kx(), h: w.h / ky() }
}

function windowAt(cx: number, cy: number): ScreenWindow | null {
  const i = info.value
  if (!i) return null
  const sx = i.x + cx * kx()
  const sy = i.y + cy * ky()
  // 列表按前后顺序给出，第一个包含该点的就是最上层
  return i.windows.find((w) => sx >= w.x && sx < w.x + w.w && sy >= w.y && sy < w.y + w.h) ?? null
}

const rectOf = (e: MouseEvent): Rect => {
  const s = start ?? { x: e.clientX, y: e.clientY }
  return { x: Math.min(s.x, e.clientX), y: Math.min(s.y, e.clientY), w: Math.abs(e.clientX - s.x), h: Math.abs(e.clientY - s.y) }
}
function down(e: MouseEvent) {
  if (e.button === 2) return void api.scPickCancel()
  if (e.button !== 0) return
  start = { x: e.clientX, y: e.clientY }
  sel.value = { x: e.clientX, y: e.clientY, w: 0, h: 0 }
}
function move(e: MouseEvent) {
  if (start) {
    sel.value = rectOf(e)
    hover.value = null
  } else {
    hover.value = windowAt(e.clientX, e.clientY)
  }
}
async function up(e: MouseEvent) {
  if (!start || !info.value) return
  const r = rectOf(e)
  start = null
  const i = info.value
  if (r.w < 12 || r.h < 12) {
    // 没拖动：点选窗口
    sel.value = null
    const w = windowAt(e.clientX, e.clientY)
    if (w) await api.scPickDone([w.x, w.y, w.w, w.h], w.hwnd).catch(() => {})
    return
  }
  const rect: [number, number, number, number] = [Math.round(i.x + r.x * kx()), Math.round(i.y + r.y * ky()), Math.max(1, Math.round(r.w * kx())), Math.max(1, Math.round(r.h * ky()))]
  await api.scPickDone(rect, 0).catch(() => {})
}

const hoverStyle = computed(() => {
  if (!hover.value) return null
  const c = cssOf(hover.value)
  return { left: c.x + 'px', top: c.y + 'px', width: c.w + 'px', height: c.h + 'px' }
})
const selStyle = computed(() => (sel.value ? { left: sel.value.x + 'px', top: sel.value.y + 'px', width: sel.value.w + 'px', height: sel.value.h + 'px' } : null))
</script>

<template>
  <div class="pick" @mousedown="down" @mousemove="move" @mouseup="up" @contextmenu.prevent>
    <img v-if="info" class="shot" :src="info.dataUrl" draggable="false" />
    <div class="dim" />
    <div v-if="hoverStyle" class="hov" :style="hoverStyle"><span>{{ hover?.title || hover?.exe }}</span></div>
    <div v-if="selStyle" class="sel" :style="selStyle" />
    <div class="tip">拖动框选要翻译的区域，或点一下某个窗口 · Esc 取消</div>
  </div>
</template>

<style>
html, body { margin: 0; overflow: hidden; background: #000; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.pick { position: fixed; inset: 0; cursor: crosshair; user-select: none; }
.shot { position: absolute; inset: 0; width: 100%; height: 100%; }
.dim { position: absolute; inset: 0; background: rgba(0, 0, 0, 0.35); pointer-events: none; }
.hov { position: absolute; border: 2px solid #2f6fed; background: rgba(47, 111, 237, 0.18); pointer-events: none; }
.hov span { position: absolute; left: 0; top: -22px; background: #2f6fed; color: #fff; font: 12px/20px -apple-system, 'Microsoft YaHei', sans-serif; padding: 0 8px; border-radius: 3px; white-space: nowrap; max-width: 60vw; overflow: hidden; text-overflow: ellipsis; }
.sel { position: absolute; border: 2px solid #1f9d55; background: rgba(31, 157, 85, 0.16); pointer-events: none; box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.25); }
.tip { position: absolute; left: 50%; top: 14px; transform: translateX(-50%); background: rgba(20, 24, 32, 0.9); color: #fff; padding: 6px 14px; border-radius: 16px; font: 13px/1.2 -apple-system, 'Microsoft YaHei', sans-serif; pointer-events: none; }
</style>
