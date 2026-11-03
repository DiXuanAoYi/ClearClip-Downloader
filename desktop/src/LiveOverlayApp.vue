<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api } from './api'
import type { ScreenBlock, ScreenPayload } from './types'

/** 实时屏幕翻译的贴字层：透明、点击穿透。坐标是物理像素，这里换算成 CSS 像素；字号按框自动缩到放得下。
 *  竖排原文 → 竖排译文（writing-mode: vertical-rl），横排原文 → 横排译文。 */
const payload = ref<ScreenPayload | null>(null)
const hidden = ref(false)
const paused = ref(false)
const els = ref<Record<number, HTMLElement | null>>({})
const dpr = window.devicePixelRatio || 1
let offs: UnlistenFn[] = []

const px = (v: number) => `${v / dpr}px`

function setEl(i: number, el: unknown) {
  els.value[i] = (el as HTMLElement | null) ?? null
}

function style(b: ScreenBlock, p: ScreenPayload) {
  return {
    left: px(b.x),
    top: px(b.y),
    width: px(b.w),
    height: px(b.h),
    color: b.fg,
    background: b.bg,
    opacity: String(Math.max(0.3, p.opacity / 100)),
    fontSize: `${Math.max(p.fontMin, b.font / dpr)}px`,
    ...(p.fontFamily ? { fontFamily: p.fontFamily } : {}),
  } as Record<string, string>
}

/** 字号从起始值往下缩，直到内容不溢出框；不低于最小字号。 */
function fit(el: HTMLElement, b: ScreenBlock, p: ScreenPayload) {
  let size = Math.max(p.fontMin, b.font / dpr)
  el.style.fontSize = `${size}px`
  const over = () => (b.vertical ? el.scrollWidth > el.clientWidth + 1 : el.scrollHeight > el.clientHeight + 1)
  let guard = 40
  while (over() && size > p.fontMin && guard-- > 0) {
    size = Math.max(p.fontMin, size - Math.max(0.5, size * 0.06))
    el.style.fontSize = `${size}px`
  }
}

async function apply(p: ScreenPayload) {
  payload.value = p
  await nextTick()
  p.blocks.forEach((b, i) => {
    const el = els.value[i]
    if (el) fit(el, b, p)
  })
}

onMounted(async () => {
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  offs = await Promise.all([api.onScreenBlocks((p) => void apply(p)), api.onScreenHidden((h) => (hidden.value = h)), api.onScreenState((s) => (paused.value = !!s.paused))])
  const stt = await api.scStatus().catch(() => null)
  if (stt) paused.value = !!stt.paused
  const cur = await api.scOverlayState().catch(() => null)
  if (cur && !payload.value) void apply(cur)
})
onBeforeUnmount(() => offs.forEach((f) => f()))
</script>

<template>
  <div v-if="!hidden" class="edge" :class="{ paused }"><span>{{ paused ? '已暂停' : '翻译中' }}</span></div>
  <div v-if="payload && !hidden" class="ov">
    <div
      v-for="(b, i) in payload.blocks"
      :key="payload.seq + ':' + i"
      :ref="(el) => setEl(i, el)"
      class="blk"
      :class="{ v: b.vertical, cjk: payload.cjk, c: b.center }"
      :style="style(b, payload)"
    >
      <span v-if="payload.bilingual && b.orig" class="orig">{{ b.orig }}</span>
      <span class="tr">{{ b.text }}</span>
    </div>
  </div>
</template>

<style>
html, body { margin: 0; background: transparent !important; overflow: hidden; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.edge { position: fixed; inset: 0; border: 2px solid #f0a064; border-radius: 4px; pointer-events: none; box-sizing: border-box; }
.edge.paused { border-style: dashed; opacity: 0.8; }
.edge span { position: absolute; left: 0; top: 0; background: #f0a064; color: #14181a; font: 600 11px/18px -apple-system, 'Microsoft YaHei', sans-serif; padding: 0 8px; border-radius: 0 0 4px 0; }
.ov { position: fixed; inset: 0; pointer-events: none; }
.blk { position: absolute; box-sizing: border-box; overflow: hidden; padding: 1px 3px; border-radius: 3px; font-family: -apple-system, 'Microsoft YaHei', 'PingFang SC', 'Noto Sans CJK SC', sans-serif; line-height: 1.22; font-weight: 600; word-break: break-word; overflow-wrap: anywhere; display: flex; flex-direction: column; justify-content: flex-start; }
.blk.c { justify-content: center; align-items: center; text-align: center; }
.blk.v { writing-mode: vertical-rl; flex-direction: row-reverse; line-height: 1.15; padding: 3px 1px; }
.blk.v.cjk .tr { text-orientation: upright; }
.blk.v:not(.cjk) .tr { text-orientation: mixed; }
.orig { font-size: 0.62em; font-weight: 400; opacity: 0.72; }
</style>
