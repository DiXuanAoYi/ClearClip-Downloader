<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api } from './api'
import type { ScreenStatus } from './types'

/** 屏幕边上的小悬浮钮：平时是一个圆钮，点开是控制条（开始/停止、模式、重新框选、显示/隐藏）。 */
const st = ref<ScreenStatus | null>(null)
const open = ref(false)
const fps = ref(3)
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
    else if (kind === 'pause') await api.scPause()
    else if (kind === 'pick') await api.scPick()
    else if (kind === 'shot') await api.scShot()
    else await api.scHide()
  } catch {
    /* 错误由后端状态文字呈现 */
  }
}

onMounted(async () => {
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  st.value = await api.scStatus().catch(() => null)
  const cur = await api.getSettings().catch(() => null)
  const calc = () => {
    const l = cur?.live
    fps.value = l ? (st.value?.mode === 'subtitle' ? l.subFps : l.fps) : 3
  }
  calc()
  offs = [await api.onScreenState((s) => ((st.value = s), calc()))]
})
onBeforeUnmount(() => offs.forEach((f) => f()))
</script>

<template>
  <div class="wrap" @mousedown="down">
    <button class="dot" :class="{ on: st?.running }" title="屏幕翻译" @mousedown.stop @click="toggleOpen">译</button>
    <div v-if="open" class="panel" @mousedown.stop>
      <button :title="st?.running ? '停止' : '开始'" @click="act('toggle')">{{ st?.running ? '停止' : '开始' }}</button>
      <button v-if="st?.running" :title="st?.paused ? '继续' : '暂停'" @click="act('pause')">{{ st?.paused ? '▶ 继续' : '⏸ 暂停' }}</button>
      <button v-for="m in MODES" :key="m[0]" :class="{ sel: st?.mode === m[0] }" @click="act('mode', m[0])">{{ m[1] }}</button>
      <button title="重新框选" @click="act('pick')">▭ 重选区域</button>
      <button v-if="st?.running" title="把译文叠在画面上存成图片（也会复制到剪贴板）" @click="act('shot')">📷 存译图</button>
      <button :title="st?.hidden ? '显示译文' : '隐藏译文'" @click="act('hide')">{{ st?.hidden ? '👁 显示译文' : '👁 隐藏译文' }}</button>
      <span v-if="st?.running" class="live">{{ st.paused ? '已暂停' : `实时翻译中 · ${fps} 帧/秒` }}</span>
      <button title="收起成圆点" @click="toggleOpen">×</button>
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
.live { color: #f0a064; padding: 0 8px; white-space: nowrap; }
.panel button.sel { background: #2f6fed; color: #fff; }
</style>
