<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox, ElNotification } from 'element-plus'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api, errorText, events } from './api'
import { useAppStore, useParseStore, useQueueStore, type ViewName } from './stores/app'
import ParseView from './views/ParseView.vue'
import QueueView from './views/QueueView.vue'
import LibraryView from './views/LibraryView.vue'
import ToolboxView from './views/ToolboxView.vue'
import SubsView from './views/SubsView.vue'
import LiveView from './views/LiveView.vue'
import ScreenView from './views/ScreenView.vue'
import VocabView from './views/VocabView.vue'
import FileTransView from './views/FileTransView.vue'
import SettingsView from './views/SettingsView.vue'
import LockScreen from './components/LockScreen.vue'
import DisclaimerDialog from './components/DisclaimerDialog.vue'
import OnboardingDialog from './components/OnboardingDialog.vue'
import CommandPalette from './components/CommandPalette.vue'
import PlayerDialog from './components/PlayerDialog.vue'
import elEn from 'element-plus/es/locale/lang/en'
import elZh from 'element-plus/es/locale/lang/zh-cn'
import { useLanguage } from './i18n/useLanguage'
import { dropFiles, dropLeave, dropOver } from './utils/dropHub'
import type { PairRequest } from './types'

const app = useAppStore()
const queue = useQueueStore()
useLanguage()
const elLocale = computed(() => (app.settings?.language === 'en' ? elEn : elZh))
const parse = useParseStore()
const ready = ref(false)
const dragging = ref(false)
const countdown = ref<{ action: string; left: number } | null>(null)
let countdownTimer: number | undefined

async function importText(text: string) {
  if (!text.trim()) return
  const urls = await api.batchExtract(text).catch(() => [] as string[])
  if (urls.length > 1) {
    app.openBatch(urls, '导入文件')
    return
  }
  app.view = 'parse'
  parse.text = text
  if (urls.length === 1) parse.parse(text)
  else ElMessage.info('没有找到可识别的链接')
}

// 锁定时关闭播放器，免得声音继续、画面露出来
watch(
  () => app.lock.locked,
  (v) => {
    if (v) app.player = null
  },
)

watch(
  () => app.pendingParse,
  (t) => {
    if (t) {
      app.pendingParse = null
      importText(t)
    }
  },
)

function onHtmlDrop(e: DragEvent) {
  dragging.value = false
  const text = e.dataTransfer?.getData('text/uri-list') || e.dataTransfer?.getData('text/plain') || ''
  if (text) importText(text)
}

function cancelPower() {
  window.clearInterval(countdownTimer)
  countdown.value = null
  api.setAfterAllDone('none').catch(() => {})
  ElMessage.info('已取消')
}
const loadError = ref('')

// icon：只在工具箱里（主导航收成图标条时）显示
const navs: { id: ViewName; label: string; icon: string }[] = [
  { id: 'parse', label: '解析', icon: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1' },
  { id: 'queue', label: '下载队列', icon: 'M12 4v11M7 11l5 5 5-5M5 20h14' },
  { id: 'subs', label: '订阅', icon: 'M6 16v-5a6 6 0 1 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0' },
  { id: 'live', label: '直播', icon: 'M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0M7.5 7.5a6 6 0 0 0 0 9M16.5 7.5a6 6 0 0 1 0 9' },
  { id: 'screen', label: '屏幕翻译', icon: 'M4 5h16v11H4zM8 20h8M9 9h6M9 12h4' },
  { id: 'library', label: '媒体库', icon: 'M4 5h16v14H4zM4 15l4-4 4 4 3-3 5 5' },
  { id: 'filetr', label: '文件翻译', icon: 'M7 3h7l5 5v13H7zM14 3v5h5M10 13h6M10 17h4' },
  { id: 'vocab', label: '生词与摘录', icon: 'M7 4h10v17l-5-4-5 4z' },
  { id: 'tools', label: '工具箱', icon: 'M4 8h16v11H4zM9 8V5h6v3M4 13h16' },
  { id: 'settings', label: '设置', icon: 'M5 7h14M5 12h14M5 17h14M9 5v4M15 10v4M8 15v4' },
]
/** 设置放在导航栏最下面，和上面的页面隔开 */
const mainNavs = computed(() => navs.filter((n) => n.id !== 'settings'))
const settingsNav = navs.find((n) => n.id === 'settings')!

const footer = computed(() => {
  if (queue.active > 0) return `下载中 ${queue.counts.running} · 等待 ${queue.counts.queued}`
  if (!app.info) return ''
  // 调试版带上提交号，和正式版、其他调试包一眼就能分开
  return app.info.debug ? `v${app.info.version} · 调试版 ${app.info.commit}` : `v${app.info.version}`
})

// 空闲自动锁定：记录最近一次键盘 / 鼠标操作
let lastActive = Date.now()
const touch = () => (lastActive = Date.now())
const ACTIVITY = ['pointerdown', 'pointermove', 'keydown', 'wheel'] as const
let idleTimer: number | undefined

function checkIdle() {
  const mins = app.settings?.security.autoLockMinutes ?? 0
  if (!app.lock.enabled || app.lock.locked || mins <= 0) return
  if (Date.now() - lastActive > mins * 60_000) api.lockNow().catch(() => {})
}

async function togglePrivacy() {
  if (!app.settings) return
  await app.patch({ security: { ...app.settings.security, privacyMode: false } })
  ElMessage.success('隐私模式已关闭')
}

onBeforeUnmount(() => {
  window.clearInterval(idleTimer)
  ACTIVITY.forEach((t) => window.removeEventListener(t, touch))
})

let restartAsked = false
/** 窗口组件（WebView2）断开：屏幕翻译取景、写作浮标这类要新开窗口的功能都会失效，只有重启清影才恢复。 */
async function askRestart(msg: string) {
  if (restartAsked) return
  restartAsked = true
  try {
    await ElMessageBox.confirm(`${msg}\n\n现在重启清影吗？进行中的下载会暂停，重启后可以继续。`, '窗口组件断开了', {
      type: 'warning',
      confirmButtonText: '现在重启',
      cancelButtonText: '稍后',
      distinguishCancelAndClose: true,
    })
    await api.restartApp()
  } catch {
    /* 选了“稍后”：之后再失败时还会再提醒 */
  } finally {
    // 十分钟内不重复弹（后端也限频），免得一直打断
    window.setTimeout(() => (restartAsked = false), 600_000)
  }
}

onMounted(async () => {
  // 先确定有没有锁住，再显示任何内容
  await app.refreshLock()
  await events.onLock(() => app.refreshLock())
  // 取景窗口、截图翻译等后台功能失败时，原来只写日志，界面上什么都看不到
  await events.onToastError((msg) => ElMessage.error({ message: msg, duration: 8000, showClose: true }))
  await events.onWebviewBroken(askRestart)
  ACTIVITY.forEach((t) => window.addEventListener(t, touch, { passive: true }))
  idleTimer = window.setInterval(checkIdle, 10_000)
  try {
    await app.load()
    await queue.start()
    await parse.loadRecent()
  } catch (e) {
    loadError.value = errorText(e)
  }
  ready.value = true

  // 主窗口在前台时复制到链接：直接填入并解析
  await events.onClipboardLink(async (p) => {
    if (app.settings?.autoDownload) return
    if (p.links.length >= 2) {
      try {
        await ElMessageBox.confirm(`剪贴板里有 ${p.links.length} 条链接。要批量添加吗？`, '检测到多条链接', {
          confirmButtonText: '批量添加…',
          cancelButtonText: '只解析第一条',
          distinguishCancelAndClose: true,
          type: 'info',
        })
        app.openBatch(p.links.map((l) => l.url), '剪贴板')
        return
      } catch (e) {
        if (e === 'close') return
      }
    }
    app.view = 'parse'
    parse.parse(p.text)
  })
  await events.onBatchOpen((p) => app.openBatch(p.urls, p.source, p.truncated))
  await events.onAutoResult((r) => {
    if (r.ok) ElMessage.success(r.message)
    else ElMessage.error(r.message)
  })
  // 手机发送：新设备请求配对
  const asking = new Set<string>()
  async function askPair(p: PairRequest) {
    if (asking.has(p.deviceId)) return
    asking.add(p.deviceId)
    const from = p.ip ? `“${p.name}”（${p.ip}）` : `“${p.name}”`
    try {
      await ElMessageBox.confirm(`${from}想要向清影发送链接。${p.code ? `请核对手机上显示的验证码：${p.code}。` : ''}允许后这台设备以后可以直接发送，配对前发来的链接也会继续处理。可在“设置 → 手机与浏览器扩展”中撤销。`, '新设备请求配对', {
        confirmButtonText: '允许',
        cancelButtonText: '拒绝',
        type: 'info',
      })
      await api.phonePairRespond(p.deviceId, true)
      ElMessage.success('已配对')
    } catch (e) {
      if (e === 'cancel' || e === 'close') await api.phonePairRespond(p.deviceId, false).catch(() => {})
      else ElMessage.error(errorText(e))
    } finally {
      asking.delete(p.deviceId)
    }
  }
  await events.onPairRequest(askPair)
  // 上次关闭前还没处理的配对请求
  api
    .phoneInfo()
    .then((info) => info.pending.forEach(askPair))
    .catch(() => {})
  // 收到的链接：侧栏角标
  await events.onInbox((c) => (app.inboxUnhandled = c.unhandled))
  api
    .inboxCounts()
    .then((c) => (app.inboxUnhandled = c.unhandled))
    .catch(() => {})
  await events.onPhoneReceived((text) => {
    const url = text.match(/https?:\/\/\S+/)?.[0] ?? text
    ElNotification({ title: '收到发送到清影的链接', message: `${url.slice(0, 80)}（可在“媒体库 → 收到的链接”查看）`, type: 'info', duration: 3000, onClick: () => app.goLibrary('inbox') })
  })
  await events.onTransferReceived((r) => {
    ElNotification({ title: '收到手机发来的内容', message: r.name.slice(0, 80), type: 'success', duration: 3000, onClick: () => app.goLibrary('transfer') })
  })
  // 全部完成后睡眠 / 关机的倒计时
  await events.onPowerCountdown((p) => {
    window.clearInterval(countdownTimer)
    countdown.value = { action: p.action, left: p.seconds }
    countdownTimer = window.setInterval(() => {
      if (!countdown.value) return
      countdown.value.left--
      if (countdown.value.left <= 0) {
        window.clearInterval(countdownTimer)
        countdown.value = null
      }
    }, 1000)
  })
  // 拖入包含链接的文本文件
  await getCurrentWebview().onDragDropEvent(async (e) => {
    // 位置是物理像素，换成页面里的 CSS 像素；剪辑页等会自己接收拖进来的素材文件
    const at = (p: { x: number; y: number }) => ({ x: p.x / (window.devicePixelRatio || 1), y: p.y / (window.devicePixelRatio || 1) })
    if (e.payload.type === 'over' || e.payload.type === 'enter') {
      const { x, y } = at(e.payload.position)
      dragging.value = !dropOver(x, y)
    } else if (e.payload.type === 'leave') {
      dragging.value = false
      dropLeave()
    } else if (e.payload.type === 'drop') {
      dragging.value = false
      const { x, y } = at(e.payload.position)
      if (dropFiles(e.payload.paths, x, y)) return
      const texts: string[] = []
      for (const path of e.payload.paths.slice(0, 20)) {
        try {
          texts.push(await api.readLinksFile(path))
        } catch (err) {
          ElMessage.warning(`${path.split(/[\\/]/).pop()}：${errorText(err)}`)
        }
      }
      if (texts.length) importText(texts.join('\n'))
    }
  })

  // 全局快捷键 / 托盘“解析剪贴板”
  await events.onParseRequest((text) => {
    app.view = 'parse'
    if (text.trim()) parse.parse(text)
  })

  if (app.settings?.checkUpdate) {
    api
      .checkUpdate()
      .then((u) => {
        if (u.hasUpdate) {
          ElMessage({ type: 'info', message: `发现新版本 v${u.latest}，可在“设置 → 关于”中下载。`, duration: 6000 })
        }
      })
      .catch(() => {})
  }
})
</script>

<template>
  <el-config-provider :locale="elLocale">
  <div v-if="loadError" class="fatal">
    <h2>启动失败</h2>
    <p class="selectable">{{ loadError }}</p>
  </div>
  <div v-else-if="ready && app.settings" class="shell" :inert="app.lock.locked" @dragover.prevent="dragging = true" @dragleave.self="dragging = false" @drop.prevent="onHtmlDrop">
    <nav class="side" aria-label="主导航">
      <div class="logo" role="img" aria-label="清影">
        <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 3l3.5 6.1M20.2 8.2l-6.8 1M20.2 15.8 15 11M12 21l-3.5-6.1M3.8 15.8l6.8-1M3.8 8.2 9 13" /></svg>
      </div>
      <button
        v-for="n in [...mainNavs, null, settingsNav]"
        :key="n ? n.id : 'gap'"
        :class="n ? ['nav', { on: app.view === n.id }] : 'nav-gap'"
        :disabled="!n"
        :aria-current="n && app.view === n.id ? 'page' : undefined"
        :aria-label="n?.label"
        type="button"
        @click="n && (n.id === 'library' && app.inboxUnhandled > 0 ? app.goLibrary('inbox') : (app.view = n.id))"
      >
        <template v-if="n">
          <svg class="ico" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="n.icon" /></svg>
          <span>{{ n.label }}</span>
          <em v-if="n.id === 'queue' && queue.active > 0">{{ queue.active }}</em>
          <em v-if="n.id === 'library' && app.inboxUnhandled > 0" class="warn" :title="`收到的链接里有 ${app.inboxUnhandled} 条需要处理`">{{ app.inboxUnhandled }}</em>
        </template>
      </button>
      <div class="foot">
        <button v-if="app.settings.security.privacyMode" class="priv" type="button" title="点击关闭隐私模式" @click="togglePrivacy">● 隐私</button>
        <div v-if="app.settings.watchClipboard" class="watch" title="剪贴板监听中">● 监听</div>
        <div class="ver" :title="footer">{{ footer }}</div>
      </div>
    </nav>
    <main class="main">
      <ParseView v-show="app.view === 'parse'" />
      <QueueView v-if="app.view === 'queue'" />
      <SubsView v-if="app.view === 'subs'" />
      <LiveView v-if="app.view === 'live'" />
      <ScreenView v-if="app.view === 'screen'" />
      <LibraryView v-if="app.view === 'library'" />
      <FileTransView v-if="app.view === 'filetr'" />
      <VocabView v-if="app.view === 'vocab'" />
      <ToolboxView v-if="app.view === 'tools'" />
      <SettingsView v-if="app.view === 'settings'" />
    </main>
    <DisclaimerDialog />
    <OnboardingDialog />
    <CommandPalette />
    <PlayerDialog />
    <div v-if="dragging" class="dropzone">松开以导入链接（支持链接文本和 .txt 文件）</div>
    <el-dialog :model-value="!!countdown" :title="countdown?.action === 'shutdown' ? '即将关机' : '即将睡眠'" width="360px" :close-on-click-modal="false" :show-close="false">
      <p>全部下载已完成，将在 <b class="mono">{{ countdown?.left }}</b> 秒后{{ countdown?.action === 'shutdown' ? '关机' : '睡眠' }}。</p>
      <template #footer>
        <el-button type="primary" @click="cancelPower">取消</el-button>
      </template>
    </el-dialog>
  </div>
  <LockScreen />
  </el-config-provider>
</template>

<style scoped>
.priv {
  all: unset;
  cursor: pointer;
  color: var(--cc-acc);
}
.shell {
  display: grid;
  grid-template-columns: 88px minmax(0, 1fr);
  height: 100%;
}
.side {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 18px 0 12px;
  background: var(--cc-rail);
  border-right: 1px solid var(--cc-line);
  backdrop-filter: blur(12px);
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: none;
}
.side::-webkit-scrollbar {
  display: none;
}
.logo {
  width: 44px;
  height: 44px;
  margin-bottom: 14px;
  display: grid;
  place-items: center;
  color: var(--cc-amber);
  flex: none;
}
.nav {
  all: unset;
  box-sizing: border-box;
  position: relative;
  cursor: pointer;
  width: 64px;
  padding: 10px 0 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  line-height: 1.2;
  color: var(--cc-mute);
  border-radius: 12px;
  flex: none;
}
.nav:hover {
  color: var(--cc-fg);
  background: var(--cc-inset);
}
.nav.on {
  color: var(--cc-amber);
  background: var(--cc-acc-soft);
  box-shadow:
    inset 0 0 0 1px var(--cc-amber-line),
    0 0 22px -8px var(--cc-amber-glow);
}
.nav .ico {
  width: 22px;
  height: 22px;
  flex: none;
}
.nav:focus-visible {
  outline: 2px solid var(--cc-cyan);
  outline-offset: 2px;
}
.nav em {
  position: absolute;
  top: 4px;
  right: 6px;
  font-style: normal;
  font-family: var(--cc-mono);
  font-size: 10px;
  line-height: 15px;
  min-width: 15px;
  text-align: center;
  background: var(--cc-amber);
  color: var(--cc-on-acc);
  border-radius: 8px;
  padding: 0 4px;
  font-weight: 700;
}
.nav em.warn {
  background: var(--cc-err);
  color: #fff;
}
.nav-gap {
  all: unset;
  flex: 1;
  min-height: 8px;
}
.foot {
  flex: none;
  width: 100%;
  padding: 8px 4px 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  font-size: 10px;
  color: var(--cc-faint);
  text-align: center;
}
.foot .ver {
  font-family: var(--cc-mono);
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.watch {
  color: var(--cc-ok);
}
.main {
  min-width: 0;
  overflow: auto;
}
.dropzone {
  position: fixed;
  inset: 12px;
  border: 2px dashed var(--cc-acc);
  border-radius: 12px;
  background: color-mix(in srgb, var(--cc-bg, #000) 70%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 16px;
  z-index: 3000;
  pointer-events: none;
}
.fatal {
  padding: 40px;
}
@media (max-width: 860px) {
  .shell {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
  }
  .side {
    flex-direction: row;
    flex-wrap: wrap;
    justify-content: center;
    border-right: 0;
    border-bottom: 1px solid var(--cc-line);
    padding: 10px;
  }
  .logo {
    margin: 0 8px 0 0;
    width: 36px;
    height: 36px;
  }
  .nav-gap,
  .foot {
    display: none;
  }
}
</style>
