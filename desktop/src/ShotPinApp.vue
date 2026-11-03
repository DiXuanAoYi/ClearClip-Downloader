<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { api } from './api'

/** 钉在桌面的译图：置顶小窗，可拖动，滚轮调透明度，双击或点 × 关闭。 */
const url = ref('')
const opacity = ref(1)

onMounted(async () => {
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  url.value = (await api.shotPinTake().catch(() => null)) ?? ''
  if (!url.value) void api.shotPinClose()
})
function down(e: MouseEvent) {
  if (e.button === 0) void getCurrentWindow().startDragging()
}
function wheel(e: WheelEvent) {
  opacity.value = Math.min(1, Math.max(0.25, opacity.value + (e.deltaY < 0 ? 0.1 : -0.1)))
}
</script>

<template>
  <div class="pin" :style="{ opacity }" title="拖动移动 · 滚轮调透明度 · 双击关闭" @mousedown="down" @wheel.prevent="wheel" @dblclick="api.shotPinClose()">
    <img v-if="url" :src="url" draggable="false" />
    <button class="x" @mousedown.stop @click="api.shotPinClose()">×</button>
  </div>
</template>

<style>
html, body { margin: 0; overflow: hidden; background: transparent !important; }
#app { width: 100vw; height: 100vh; }
</style>
<style scoped>
.pin { position: fixed; inset: 0; border: 2px solid #f0a064; box-sizing: border-box; border-radius: 4px; overflow: hidden; background: #14181a; }
img { width: 100%; height: 100%; object-fit: fill; display: block; pointer-events: none; }
.x { position: absolute; right: 2px; top: 2px; width: 20px; height: 20px; border: 0; border-radius: 50%; background: #000a; color: #fff; cursor: pointer; display: none; line-height: 1; }
.pin:hover .x { display: block; }
</style>
