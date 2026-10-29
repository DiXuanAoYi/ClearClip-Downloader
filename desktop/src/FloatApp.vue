<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { api, errorText } from './api'
import { useLanguage } from './i18n/useLanguage'
import { useAppStore, useQueueStore } from './stores/app'
import { formatSpeed } from './utils/format'

/** 悬浮拖拽窗：把浏览器里的链接（或一段带链接的文字）拖到这里就开始下载；点一下再按 Ctrl+V 也行。 */
const app = useAppStore()
useLanguage()
const queue = useQueueStore()
const over = ref(false)
const busy = ref(false)
const msg = ref('')
const bad = ref(false)
let clearTimer: number | undefined

const active = computed(() => queue.counts.running + queue.counts.queued)
const speed = computed(() => queue.tasks.filter((t) => t.status === 'running').reduce((s, t) => s + t.speed, 0))
const text = computed(() => {
  if (msg.value) return msg.value
  if (busy.value) return '正在解析…'
  if (over.value) return '松开即下载'
  if (active.value) return `${active.value} 个任务 · ${formatSpeed(speed.value)}`
  return '拖链接到这里'
})

function say(m: string, isBad = false) {
  msg.value = m
  bad.value = isBad
  window.clearTimeout(clearTimer)
  clearTimer = window.setTimeout(() => {
    msg.value = ''
    bad.value = false
  }, 3500)
}

/** 从文字里找出链接，逐个解析并加入下载。 */
async function addText(raw: string) {
  const text = raw.trim()
  if (!text || busy.value) return
  busy.value = true
  try {
    // 任何 http(s) 链接都交给后端判断：专用解析器、yt-dlp、通用嗅探都会依次尝试
    const links = [...new Set((text.match(/https?:\/\/[^\s<>"'）)」』】]+/g) ?? []).map((u) => u.replace(/[,，。.;；]+$/, '')))]
    if (!links.length) {
      say('没有找到链接', true)
      return
    }
    let ok = 0
    let last = ''
    for (const url of links.slice(0, 20)) {
      try {
        last = await api.resolveAndEnqueue(url)
        ok++
      } catch (e) {
        last = errorText(e)
      }
    }
    if (ok === links.length) say(ok === 1 ? `已加入：${last}` : `已加入 ${ok} 个链接`)
    else say(ok ? `加入 ${ok}/${links.length} 个，其余失败` : last, !ok)
  } catch (e) {
    say(errorText(e), true)
  } finally {
    busy.value = false
  }
}

function dropText(e: DragEvent): string {
  const dt = e.dataTransfer
  if (!dt) return ''
  return dt.getData('text/uri-list') || dt.getData('text/plain') || dt.getData('text/html').replace(/<[^>]+>/g, ' ')
}

function onDrop(e: DragEvent) {
  over.value = false
  void addText(dropText(e))
}

function onPaste(e: ClipboardEvent) {
  void addText(e.clipboardData?.getData('text/plain') ?? '')
}

async function hide() {
  if (app.settings) await app.patch({ floatBall: false })
}

onMounted(async () => {
  await app.load()
  await queue.start()
  window.addEventListener('paste', onPaste)
})
onBeforeUnmount(() => window.removeEventListener('paste', onPaste))
</script>

<template>
  <div
    class="ball"
    :class="{ over, bad, busy }"
    data-tauri-drag-region
    tabindex="0"
    @dragenter.prevent="over = true"
    @dragover.prevent="over = true"
    @dragleave="over = false"
    @drop.prevent="onDrop"
    @dblclick="api.showMain()"
  >
    <span class="ic" data-tauri-drag-region>⬇</span>
    <span class="tx" data-tauri-drag-region :title="text">{{ text }}</span>
    <button class="x" type="button" title="关闭悬浮窗" @click.stop="hide">×</button>
  </div>
</template>

<style scoped>
.ball {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 12px;
  background: var(--cc-card);
  border: 1.5px dashed var(--cc-line);
  border-radius: 14px;
  color: var(--cc-fg);
  font-size: 12px;
  cursor: grab;
  user-select: none;
  outline: none;
}
.ball.over {
  border-color: var(--cc-acc);
  background: var(--cc-acc-soft);
}
.ball.bad {
  border-color: var(--cc-err);
}
.ball.busy .ic {
  animation: spin 1s linear infinite;
}
.ic {
  font-size: 18px;
  color: var(--cc-acc);
}
.tx {
  flex: 1;
  line-height: 1.3;
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}
.x {
  all: unset;
  cursor: pointer;
  color: var(--cc-mute);
  font-size: 15px;
  line-height: 1;
  padding: 2px 4px;
}
.x:hover {
  color: var(--cc-fg);
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
