<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api } from './api'
import type { ScreenStatus } from './types'

/** 屏幕边上的小悬浮钮：平时是一个圆钮，点开是控制条（开始/停止、模式、重新框选、显示/隐藏）。 */
const st = ref<ScreenStatus | null>(null)
const open = ref(false)
let offs: UnlistenFn[] = []

const MODES: [string, string][] = [
  ['standard', '标准'],
  ['subtitle', '字幕'],
  ['manga', '漫画'],
]

async function setOpen(v: boolean) {
  open.value = v
  await api.scBarExpand(v).catch(() => {})
}
function down(e: MouseEvent) {
  if (e.button !== 0) return
  void getCurrentWindow().startDragging()
}
function toggleOpen() {
  void setOpen(!open.value)
}
async function act(kind: string, arg = '') {
  try {
    if (kind === 'toggle') await api.scToggle()
    else if (kind === 'mode') await api.scSetMode(arg)
    else if (kind === 'pick') await api.scPick()
    else await api.scHide()
  } catch {
    /* 错误由后端状态文字呈现 */
  }
}

onMounted(async () => {
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  st.value = await api.scStatus().catch(() => null)
  offs = [await api.onScreenState((s) => (st.value = s))]
})
onBeforeUnmount(() => offs.forEach((f) => f()))
</script>

<template>
  <div class="wrap" @mousedown="down">
    <button class="dot" :class="{ on: st?.running }" title="屏幕翻译" @mousedown.stop @click="toggleOpen">译</button>
    <div v-if="open" class="panel" @mousedown.stop>
      <button :title="st?.running ? '停止' : '开始'" @click="act('toggle')">{{ st?.running ? '停' : '开始' }}</button>
      <button v-for="m in MODES" :key="m[0]" :class="{ sel: st?.mode === m[0] }" @click="act('mode', m[0])">{{ m[1] }}</button>
      <button title="重新框选" @click="act('pick')">框选</button>
      <button :title="st?.hidden ? '显示译文' : '隐藏译文'" @click="act('hide')">{{ st?.hidden ? '显' : '隐' }}</button>
    </div>
  </div>
</template>

<style>
html, body { margin: 0; background: transparent !important; overflow: hidden; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.wrap { width: 100vw; height: 100vh; display: flex; align-items: center; gap: 6px; font: 600 12px/1 -apple-system, 'Microsoft YaHei', 'PingFang SC', sans-serif; }
.dot { flex: none; width: 42px; height: 42px; border: 0; border-radius: 50%; background: #3a4a63; color: #fff; font-size: 15px; font-weight: 700; cursor: pointer; box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.85), 0 2px 6px rgba(0, 0, 0, 0.45); padding: 0; }
.dot.on { background: #1f9d55; }
.panel { display: flex; gap: 3px; padding: 4px; border-radius: 21px; background: rgba(30, 36, 48, 0.92); box-shadow: 0 2px 8px rgba(0, 0, 0, 0.45); height: 42px; box-sizing: border-box; align-items: center; }
.panel button { border: 0; border-radius: 14px; padding: 0 9px; height: 30px; background: transparent; color: #e6ebf3; font: inherit; cursor: pointer; white-space: nowrap; }
.panel button:hover { background: rgba(255, 255, 255, 0.14); }
.panel button.sel { background: #2f6fed; color: #fff; }
</style>
