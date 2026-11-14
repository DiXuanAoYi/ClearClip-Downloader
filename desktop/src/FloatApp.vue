<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText } from './api'
import type { ScreenStatus } from './types'
import { useLanguage } from './i18n/useLanguage'
import { useAppStore, useQueueStore } from './stores/app'
import { formatSpeed } from './utils/format'
import { useTauriEvents } from './composables/useTauriEvent'
const ev = useTauriEvents()

/**
 * 清影悬浮窗（桌面上只有这一个）：把浏览器里的链接（或一段带链接的文字）拖到这里就开始下载；点一下再按 Ctrl+V 也行。
 * 在 Windows 上还有一个“译”：点开是屏幕翻译的控制（开始 / 停止、暂停、模式、重新框选、存译图、显示 / 隐藏译文）。
 * 以前屏幕翻译的控制是屏幕边上另一个单独的悬浮钮。
 */
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

/** 空闲时在主文字下面补一行用法 */
const hint = computed(() => (!msg.value && !busy.value && !over.value && !active.value ? '或点击后 Ctrl+V' : ''))

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
    if (links.length >= 2) {
      // 多条链接：整批加入队列（按批次间隔逐条解析，队列里按批次显示）
      const r = await api.batchQuick(links.slice(0, 500), '悬浮窗')
      say(`已建立批次：${r.queued + r.pending} 条`)
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

/** 关掉悬浮窗：两处开关（设置 → 悬浮窗、屏幕翻译 → 悬浮窗里放屏幕翻译控制）一起关。 */
async function hide() {
  try {
    const s = await api.getSettings()
    await api.saveSettings({ ...s, floatBall: false, live: { ...s.live, floatButton: false } }, s)
  } catch (e) {
    say(errorText(e), true)
  }
}

// ---------- 屏幕翻译的控制 ----------
const sc = ref<ScreenStatus | null>(null)
const scOpen = ref(false)
const fps = ref(3)
const MODES: [string, string][] = [
  ['standard', '标准'],
  ['subtitle', '字幕'],
  ['manga', '漫画'],
]
const hk = computed(() => ({ hotkey: app.settings?.live.hotkey ?? '', hotkeyPick: app.settings?.live.hotkeyPick ?? '', hotkeyHide: app.settings?.live.hotkeyHide ?? '' }))
const scShown = computed(() => !!sc.value?.supported && (app.settings?.live.floatButton ?? false))
function setScOpen(v: boolean) {
  scOpen.value = v
}
/** 窗口宽度跟着内容走：量出一整行（胶囊 + 展开的控制条）的宽度，加上四周给阴影的留白 */
const rowEl = ref<HTMLElement | null>(null)
let ro: ResizeObserver | undefined
let lastFit = 0
function fit() {
  const el = rowEl.value
  if (!el) return
  const w = Math.ceil(el.getBoundingClientRect().width) + 20
  if (w === lastFit) return
  lastFit = w
  void api.floatFit(w).catch(() => {}) // 忽略：悬浮条尺寸调整失败无碍
}
watch([scOpen, scShown], () => void nextTick(fit))
async function act(kind: string, arg = '') {
  try {
    // 开始 / 停止和主程序“屏幕翻译”页的按钮是同一套：停止就是停止，开始用上次的区域（没有就去框选）
    if (kind === 'toggle') {
      if (sc.value?.running) sc.value = await api.scStop()
      else
        sc.value = await api.scStart().catch(async () => {
          await api.scPick()
          return sc.value as ScreenStatus
        })
    }
    else if (kind === 'mode') await api.scSetMode(arg)
    else if (kind === 'pause') await api.scPause()
    else if (kind === 'pick') await api.scPick()
    else if (kind === 'shot') await api.scShot()
    else await api.scHide()
  } catch {
    /* 错误由后端状态文字呈现 */
  }
}
function calcFps() {
  const l = app.settings?.live
  fps.value = l ? (sc.value?.mode === 'subtitle' ? l.subFps : l.fps) : 3
}
let offs: UnlistenFn[] = []

onMounted(async () => {
  // 窗口是透明的：页面背景也要透明，胶囊才有自己的圆角和阴影
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'
  if (rowEl.value && typeof ResizeObserver !== 'undefined') {
    ro = new ResizeObserver(fit)
    ro.observe(rowEl.value)
  }
  fit()
  await app.load()
  await queue.start()
  window.addEventListener('paste', onPaste)
  sc.value = await api.scStatus().catch(() => null) // 忽略：可选数据，取不到就按空处理
  calcFps()
  offs = [await ev.on(api.onScreenState((s) => ((sc.value = s), calcFps())))]
})
onBeforeUnmount(() => {
  ro?.disconnect()
  window.removeEventListener('paste', onPaste)
  offs.forEach((f) => f())
})
</script>

<template>
  <div class="wrap">
    <div ref="rowEl" class="row">
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
      <span class="ic" data-tauri-drag-region><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v11M7 11l5 5 5-5M5 20h14" /></svg></span>
      <span class="tx" data-tauri-drag-region :title="text">
        <b data-tauri-drag-region>{{ text }}</b>
        <small v-if="hint" data-tauri-drag-region>{{ hint }}</small>
      </span>
      <button v-if="scShown" class="tr" :class="{ on: sc?.running, open: scOpen }" type="button" :title="sc?.running ? '屏幕翻译进行中：点开控制' : '屏幕翻译'" @click.stop="setScOpen(!scOpen)">译</button>
      <button class="x" type="button" title="关闭悬浮窗" aria-label="关闭悬浮窗" @click.stop="hide"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 7 10 10M17 7 7 17" /></svg></button>
    </div>
    <div v-if="scShown && scOpen" class="panel">
      <button :title="`开始 / 停止（${hk.hotkey}）`" @click="act('toggle')">{{ sc?.running ? '停止' : '开始' }}</button>
      <button v-if="sc?.running" @click="act('pause')">{{ sc?.paused ? '▶ 继续' : '⏸ 暂停' }}</button>
      <button v-for="m in MODES" :key="m[0]" :class="{ sel: sc?.mode === m[0] }" @click="act('mode', m[0])">{{ m[1] }}</button>
      <button :title="`重新框选（${hk.hotkeyPick}）`" @click="act('pick')">▭ 重选</button>
      <button v-if="sc?.running" title="把译文叠在画面上存成图片（也会复制到剪贴板）" @click="act('shot')">📷 存译图</button>
      <button :title="`显示 / 隐藏译文（${hk.hotkeyHide}）`" @click="act('hide')">{{ sc?.hidden ? '显示译文' : '隐藏译文' }}</button>
      <span v-if="sc?.running" class="live">{{ sc.paused ? '已暂停' : `${fps} 帧/秒` }}</span>
      <button title="收起" @click="setScOpen(false)">‹</button>
    </div>
    </div>
  </div>
</template>

<style>
html, body, #app { background: transparent !important; overflow: hidden; }
</style>

<style scoped>
/* 胶囊不跟随主题：始终是深蓝黑底 + 细亮边 + 阴影，在浅色和深色桌面上都看得清圆角 */
.wrap {
  position: fixed;
  inset: 0;
  box-sizing: border-box;
  padding: 10px;
}
.row {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  /* 不能限制成窗口宽度：窗口宽度是按这一行量出来的，限制了就再也长不宽（“译”和关闭钮被截掉） */
  width: max-content;
}
.ball,
.panel {
  height: 48px;
  box-sizing: border-box;
  border-radius: 24px;
  background: #0b1424;
  border: 1px solid rgba(232, 240, 253, 0.22);
  box-shadow: 0 2px 8px rgba(2, 6, 14, 0.45);
  color: #e8f0fd;
}
.ball {
  position: relative;
  flex: 0 0 auto;
  width: max-content;
  max-width: 300px;
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 0 8px 0 9px;
  font-size: 12px;
  cursor: grab;
  user-select: none;
  outline: none;
  transition: border-color 0.12s, background 0.12s, box-shadow 0.12s;
}
.ball:focus-visible {
  border-color: #4be3d0;
}
.ball.over {
  border-color: #ffb547;
  background: #2a2016;
  box-shadow: 0 0 0 3px rgba(255, 181, 71, 0.28), 0 2px 8px rgba(2, 6, 14, 0.45);
}
.ball.bad {
  border-color: #ff7b8a;
}
.ball.busy .ic svg {
  animation: spin 1s linear infinite;
}
.ic {
  flex: none;
  width: 30px;
  height: 30px;
  border-radius: 50%;
  background: rgba(255, 181, 71, 0.16);
  color: #ffb547;
  display: flex;
  align-items: center;
  justify-content: center;
}
.ic svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2.2;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.tx {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1px;
}
.tx b {
  font-size: 12.5px;
  font-weight: 700;
  line-height: 1.25;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tx small {
  font-size: 10.5px;
  color: #9aabc6;
  line-height: 1.2;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.x {
  all: unset;
  flex: none;
  cursor: pointer;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  color: #9aabc6;
  display: flex;
  align-items: center;
  justify-content: center;
}
.x svg {
  width: 13px;
  height: 13px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2.2;
  stroke-linecap: round;
}
.x:hover {
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
}
.x:focus-visible,
.tr:focus-visible,
.panel button:focus-visible {
  outline: 2px solid #4be3d0;
  outline-offset: 1px;
}
.tr {
  all: unset;
  cursor: pointer;
  flex: none;
  width: 26px;
  height: 26px;
  border-radius: 50%;
  background: #1c2b43;
  color: #e8f0fd;
  font-size: 12px;
  font-weight: 700;
  text-align: center;
  line-height: 26px;
}
.tr.on {
  background: #007a70;
}
.tr.open {
  box-shadow: 0 0 0 2px #ffb547;
}
.panel {
  flex: 0 0 auto;
  display: flex;
  gap: 3px;
  align-items: center;
  padding: 0 10px;
  overflow: hidden;
}
.panel button {
  border: 0;
  border-radius: 14px;
  padding: 0 10px;
  height: 30px;
  background: transparent;
  color: #d3deec;
  font: 600 12px/1 inherit;
  cursor: pointer;
  white-space: nowrap;
}
.panel button:hover {
  background: rgba(255, 255, 255, 0.12);
}
.panel button.sel {
  background: #ffb547;
  color: #1a1100;
}
.live {
  color: #ffb547;
  padding: 0 6px;
  font-size: 12px;
  white-space: nowrap;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
