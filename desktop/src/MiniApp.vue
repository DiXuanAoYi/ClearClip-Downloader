<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api, errorText, events } from './api'
import { useLanguage } from './i18n/useLanguage'
import { useAppStore, useQueueStore } from './stores/app'
import type { AutoResult, ClipboardLink } from './types'
import { formatSpeed } from './utils/format'

const app = useAppStore()
useLanguage()
const queue = useQueueStore()
const link = ref<ClipboardLink | null>(null)
const busy = ref(false)
const message = ref('')
const failed = ref(false)
let hideTimer: number | undefined

const first = computed(() => link.value?.links[0])
const runningPercent = computed(() => {
  const running = queue.tasks.filter((t) => t.status === 'running' && t.total)
  if (!running.length) return 0
  const r = running.reduce((s, t) => s + t.received, 0)
  const total = running.reduce((s, t) => s + (t.total ?? 0), 0)
  return total ? Math.floor((r / total) * 100) : 0
})

function cancelHide() {
  window.clearTimeout(hideTimer)
}

function scheduleHide(ms = 10000) {
  window.clearTimeout(hideTimer)
  hideTimer = window.setTimeout(() => api.hideMini(), ms)
}

function showLink(p: ClipboardLink) {
  link.value = p
  message.value = app.settings?.autoDownload ? '正在解析并加入下载…' : ''
  failed.value = false
  scheduleHide()
}

function showResult(r: AutoResult) {
  message.value = r.message
  failed.value = !r.ok
  scheduleHide(6000)
}

onMounted(async () => {
  await app.load()
  await queue.start()
  await events.onClipboardLink(showLink)
  await events.onAutoResult(showResult)
  // 迷你窗是用到时才创建的：页面加载好之前发出的链接和自动下载结果在这里补上
  try {
    const s = await api.miniReady()
    if (s.link) showLink(s.link)
    if (s.result) showResult(s.result)
  } catch {
    // 取不到就等下一次事件
  }
})

async function downloadNow() {
  if (!first.value) return
  busy.value = true
  failed.value = false
  window.clearTimeout(hideTimer)
  try {
    const title = await api.resolveAndEnqueue(link.value!.text)
    message.value = `已加入下载：${title}`
    scheduleHide(5000)
  } catch (e) {
    message.value = errorText(e)
    failed.value = true
    scheduleHide(8000)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="mini" @mouseenter="cancelHide" @mouseleave="scheduleHide(5000)">
    <div class="bar" data-tauri-drag-region>
      <span class="brand" data-tauri-drag-region>清<i>影</i></span>
      <button class="x" type="button" aria-label="关闭" @click="api.hideMini()">×</button>
    </div>
    <div class="body">
      <div v-if="first" class="toast">
        <div class="tag">{{ first.platformName }}</div>
        <div class="txt">
          <b>检测到{{ first.platformName }}链接<template v-if="link!.links.length > 1">（共 {{ link!.links.length }} 条）</template></b>
          <small class="ellipsis">{{ first.url }}</small>
        </div>
      </div>
      <div v-else class="mute">复制支持平台的分享链接后会出现在这里。</div>
      <div v-if="message" class="msg ellipsis" :class="{ err: failed }" :title="message">{{ message }}</div>
      <div class="row">
        <el-button v-if="first && !app.settings?.autoDownload" type="primary" size="small" :loading="busy" @click="downloadNow">立即下载</el-button>
        <el-button size="small" @click="api.showMain()">打开主窗口</el-button>
      </div>
      <div v-if="queue.active > 0" class="q">
        <div class="pbar"><i :style="{ width: runningPercent + '%' }" /></div>
        <small class="mono mute">队列：{{ queue.counts.running }} 个进行中 · {{ formatSpeed(queue.totalSpeed) }}</small>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mini {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--cc-bg);
  border: 1px solid var(--cc-line);
  overflow: hidden;
}
.bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 30px;
  padding: 0 6px 0 12px;
  background: var(--cc-side);
  border-bottom: 1px solid var(--cc-line);
}
.brand {
  font-weight: 900;
  letter-spacing: 0.06em;
}
.brand i {
  font-style: normal;
  color: var(--cc-acc);
}
.x {
  all: unset;
  cursor: pointer;
  width: 22px;
  height: 22px;
  text-align: center;
  border-radius: 4px;
  color: var(--cc-mute);
  font-size: 16px;
  line-height: 22px;
}
.x:hover {
  background: var(--cc-line);
}
.body {
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.toast {
  display: flex;
  gap: 10px;
  align-items: center;
  background: var(--cc-card);
  border: 1px solid var(--cc-acc);
  border-radius: 8px;
  padding: 8px 10px;
}
.tag {
  flex: none;
  font-size: 11px;
  font-weight: 700;
  color: #1a1208;
  background: var(--cc-acc);
  border-radius: 4px;
  padding: 2px 6px;
}
.txt {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.txt small {
  color: var(--cc-mute);
  font-size: 11px;
}
.msg {
  font-size: 12px;
  color: var(--cc-ok);
}
.msg.err {
  color: var(--cc-err);
}
.row {
  display: flex;
  gap: 8px;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.pbar {
  height: 4px;
  background: var(--cc-line);
  border-radius: 2px;
  overflow: hidden;
  margin-bottom: 3px;
}
.pbar i {
  display: block;
  height: 100%;
  background: var(--cc-acc);
}
.q small {
  font-size: 10.5px;
}
</style>
