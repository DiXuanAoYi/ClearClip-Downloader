<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api } from './api'
import type { ScreenSub } from './types'
import { useTauriEvents } from './composables/useTauriEvent'
const ev = useTauriEvents()

/** 字幕悬浮条：屏幕字幕与系统声音实时字幕共用。按住空白处可拖动，边缘可缩放。 */
const sub = ref<ScreenSub | null>(null)
const hidden = ref(false)
let offs: UnlistenFn[] = []

function drag(e: MouseEvent) {
  if (e.button === 0) void getCurrentWindow().startDragging()
}

onMounted(async () => {
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  offs = await Promise.all([ev.on(api.onScreenSub((s) => (sub.value = s.text || (s.draft && s.orig) ? s : null))), ev.on(api.onScreenHidden((h) => (hidden.value = h)))])
})
onBeforeUnmount(() => offs.forEach((f) => f()))
</script>

<template>
  <div class="bar" @mousedown="drag">
    <div v-if="sub && !hidden" class="box" :style="{ fontSize: sub.font + 'px' }">
      <div v-if="(sub.bilingual || sub.draft) && sub.orig" class="orig" :class="{ draft: sub.draft }">{{ sub.orig }}</div>
      <div v-if="sub.text" class="tr">{{ sub.text }}</div>
    </div>
  </div>
</template>

<style>
html, body { margin: 0; background: transparent !important; overflow: hidden; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.bar { width: 100vw; height: 100vh; display: flex; align-items: flex-end; justify-content: center; cursor: move; user-select: none; }
.box { max-width: 100%; box-sizing: border-box; padding: 6px 16px 8px; border-radius: 8px; background: rgba(0, 0, 0, 0.72); color: #fff; text-align: center; font-family: -apple-system, 'Microsoft YaHei', 'PingFang SC', 'Noto Sans CJK SC', sans-serif; line-height: 1.35; text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8); }
.orig { font-size: 0.68em; opacity: 0.75; margin-bottom: 2px; }
.orig.draft { font-size: 1em; opacity: 0.6; font-style: italic; margin-bottom: 0 }
.tr { font-weight: 600; }
</style>
