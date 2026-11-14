<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox, ElNotification } from 'element-plus'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api, errorText, events } from './api'
import { useAppStore, useParseStore, useQueueStore, type ViewName } from './stores/app'
import ParseView from './views/ParseView.vue'
import LockScreen from './components/LockScreen.vue'
import DisclaimerDialog from './components/DisclaimerDialog.vue'
import OnboardingDialog from './components/OnboardingDialog.vue'
import CommandPalette from './components/CommandPalette.vue'
import PlayerDialog from './components/PlayerDialog.vue'
import { elLocale } from './i18n/elLocale'
import { useLanguage } from './i18n/useLanguage'
import { dropFiles, dropLeave, dropOver } from './utils/dropHub'
import type { PairRequest } from './types'
import { useTauriEvents } from './composables/useTauriEvent'
import { toastErr } from './utils/toastErr'

// 除默认的解析页外，其余页面第一次切过去时才加载，减小首屏体积（切换行为不变：仍是 v-if 挂载）
const QueueView = defineAsyncComponent(() => import('./views/QueueView.vue'))
const LibraryView = defineAsyncComponent(() => import('./views/LibraryView.vue'))
const ToolboxView = defineAsyncComponent(() => import('./views/ToolboxView.vue'))
const SubsView = defineAsyncComponent(() => import('./views/SubsView.vue'))
const LiveView = defineAsyncComponent(() => import('./views/LiveView.vue'))
const ScreenView = defineAsyncComponent(() => import('./views/ScreenView.vue'))
const VocabView = defineAsyncComponent(() => import('./views/VocabView.vue'))
const FileTransView = defineAsyncComponent(() => import('./views/FileTransView.vue'))
const SettingsView = defineAsyncComponent(() => import('./views/SettingsView.vue'))

const ev = useTauriEvents()

const app = useAppStore()
const queue = useQueueStore()
useLanguage()
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
  api.setAfterAllDone('none').catch(toastErr('取消定时操作'))
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
// 侧栏收起：手动选择会记住；窗口矮于 560 时自动只显示图标
const NAV_KEY = 'clearclip.nav.collapsed'
const navManual = ref(false)
try {
  navManual.value = localStorage.getItem(NAV_KEY) === '1'
} catch {
  /* 读不到就按展开 */
}
const shortWin = ref(false)
let shortMq: MediaQueryList | null = null
const onShort = () => (shortWin.value = !!shortMq?.matches)
const navCollapsed = computed(() => navManual.value || shortWin.value)
function toggleNav() {
  navManual.value = !navManual.value
  try {
    localStorage.setItem(NAV_KEY, navManual.value ? '1' : '0')
  } catch {
    /* 存不了只是下次不记得 */
  }
}
function goNav(id: ViewName) {
  if (id === 'library' && app.inboxUnhandled > 0) app.goLibrary('inbox')
  else app.view = id
}
onMounted(() => {
  shortMq = window.matchMedia('(max-height: 559px)')
  onShort()
  shortMq.addEventListener('change', onShort)
})
onBeforeUnmount(() => shortMq?.removeEventListener('change', onShort))

/** 设置放在导航栏最下面，和上面的页面隔开 */
const mainNavs = computed(() => navs.filter((n) => n.id !== 'settings'))
const settingsNav = navs.find((n) => n.id === 'settings')!

// 下载进度已经在“下载队列”的角标和页头里，页脚只留版本（短，不截断）
const footer = computed(() => {
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
  if (Date.now() - lastActive > mins * 60_000) api.lockNow().catch(() => {}) // 忽略：空闲自动锁定：失败不打扰
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
  await ev.on(events.onLock(() => app.refreshLock()))
  // 取景窗口、截图翻译等后台功能失败时，原来只写日志，界面上什么都看不到
  await ev.on(events.onToastError((msg) => ElMessage.error({ message: msg, duration: 8000, showClose: true })))
  await ev.on(events.onWebviewBroken(askRestart))
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
  await ev.on(events.onClipboardLink(async (p) => {
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
  }))
  await ev.on(events.onBatchOpen((p) => app.openBatch(p.urls, p.source, p.truncated)))
  await ev.on(events.onAutoResult((r) => {
    if (r.ok) ElMessage.success(r.message)
    else ElMessage.error(r.message)
  }))
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
      if (e === 'cancel' || e === 'close') await api.phonePairRespond(p.deviceId, false).catch(toastErr('回复配对请求'))
      else ElMessage.error(errorText(e))
    } finally {
      asking.delete(p.deviceId)
    }
  }
  await ev.on(events.onPairRequest(askPair))
  // 上次关闭前还没处理的配对请求
  api
    .phoneInfo()
    .then((info) => info.pending.forEach(askPair))
    .catch(() => {}) // 忽略：启动时的可选同步，取不到下次事件会补上
  // 收到的链接：侧栏角标
  await ev.on(events.onInbox((c) => (app.inboxUnhandled = c.unhandled)))
  api
    .inboxCounts()
    .then((c) => (app.inboxUnhandled = c.unhandled))
    .catch(() => {}) // 忽略：角标是可选的，取不到就不显示
  await ev.on(events.onPhoneReceived((text) => {
    const url = text.match(/https?:\/\/\S+/)?.[0] ?? text
    ElNotification({ title: '收到发送到清影的链接', message: `${url.slice(0, 80)}（可在“媒体库 → 收到的链接”查看）`, type: 'info', duration: 3000, onClick: () => app.goLibrary('inbox') })
  }))
  await ev.on(events.onTransferReceived((r) => {
    ElNotification({ title: '收到手机发来的内容', message: r.name.slice(0, 80), type: 'success', duration: 3000, onClick: () => app.goLibrary('transfer') })
  }))
  // 全部完成后睡眠 / 关机的倒计时
  await ev.on(events.onPowerCountdown((p) => {
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
  }))
  // 拖入包含链接的文本文件
  await ev.on(getCurrentWebview().onDragDropEvent(async (e) => {
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
  }))

  // 全局快捷键 / 托盘“解析剪贴板”
  await ev.on(events.onParseRequest((text) => {
    app.view = 'parse'
    if (text.trim()) parse.parse(text)
  }))

  if (app.settings?.checkUpdate) {
    api
      .checkUpdate()
      .then((u) => {
        if (u.hasUpdate) {
          ElMessage({ type: 'info', message: `发现新版本 v${u.latest}，可在“设置 → 关于”中下载。`, duration: 6000 })
        }
      })
      .catch(() => {}) // 忽略：尽力而为，失败不影响当前操作
  }
})
</script>

<template>
  <el-config-provider :locale="elLocale">
  <div v-if="loadError" class="fatal">
    <h2>启动失败</h2>
    <p class="selectable">{{ loadError }}</p>
  </div>
  <div v-else-if="ready && app.settings" class="shell" :class="{ collapsed: navCollapsed }" :inert="app.lock.locked" @dragover.prevent="dragging = true" @dragleave.self="dragging = false" @drop.prevent="onHtmlDrop">
    <nav class="side" :class="{ collapsed: navCollapsed }" aria-label="主导航">
      <div class="logo" role="img" aria-label="清影">
        <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 3l3.5 6.1M20.2 8.2l-6.8 1M20.2 15.8 15 11M12 21l-3.5-6.1M3.8 15.8l6.8-1M3.8 8.2 9 13" /></svg>
      </div>
      <div class="navs">
        <button v-for="n in mainNavs" :key="n.id" :class="['nav', { on: app.view === n.id }]" :aria-current="app.view === n.id ? 'page' : undefined" :aria-label="n.label" :title="navCollapsed ? n.label : undefined" type="button" @click="goNav(n.id)">
          <svg class="ico" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="n.icon" /></svg>
          <span>{{ n.label }}</span>
          <em v-if="n.id === 'queue' && queue.active > 0" :aria-label="`${queue.active} 个任务未完成`">{{ queue.active }}</em>
          <em v-if="n.id === 'library' && app.inboxUnhandled > 0" class="warn" :title="`收到的链接里有 ${app.inboxUnhandled} 条需要处理`">{{ app.inboxUnhandled }}</em>
        </button>
      </div>
      <div class="navbot">
        <button class="nav" :class="{ on: app.view === settingsNav.id }" :aria-current="app.view === settingsNav.id ? 'page' : undefined" :aria-label="settingsNav.label" :title="navCollapsed ? settingsNav.label : undefined" type="button" @click="goNav(settingsNav.id)">
          <svg class="ico" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="settingsNav.icon" /></svg>
          <span>{{ settingsNav.label }}</span>
        </button>
        <button v-if="!shortWin" class="navtoggle" type="button" :aria-label="navManual ? '展开侧栏' : '收起侧栏'" :aria-expanded="!navManual" :title="navManual ? '展开侧栏' : '收起侧栏'" data-testid="nav-toggle" @click="toggleNav">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="navManual ? 'M9 5l7 7-7 7' : 'M15 5l-7 7 7 7'" /></svg>
        </button>
        <div class="foot">
          <button v-if="app.settings.security.privacyMode" class="priv" type="button" title="点击关闭隐私模式" @click="togglePrivacy">● 隐私</button>
          <div v-if="app.settings.watchClipboard" class="watch" title="剪贴板监听中" aria-label="剪贴板监听中">● <span class="wtx">监听</span></div>
          <div v-if="footer" class="ver" :title="footer">{{ footer }}</div>
        </div>
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
    <el-dialog append-to-body :model-value="!!countdown" :title="countdown?.action === 'shutdown' ? '即将关机' : '即将睡眠'" width="360px" :close-on-click-modal="false" :show-close="false">
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
.priv:focus-visible,
.navtoggle:focus-visible {
  outline: var(--cc-focus-w) solid var(--cc-focus);
  outline-offset: var(--cc-focus-off);
}
.shell {
  --nav-w: 88px;
  display: grid;
  grid-template-columns: var(--nav-w) minmax(0, 1fr);
  height: 100%;
  background: var(--cc-page-bg);
  transition: grid-template-columns var(--cc-dur-slow, 320ms) var(--cc-ease-out, ease);
}
.shell.collapsed {
  --nav-w: 56px;
}
.side {
  display: flex;
  flex-direction: column;
  align-items: center;
  min-height: 0;
  padding: var(--cc-sp-3) 0 var(--cc-sp-2);
  background: var(--cc-rail);
  border-right: 1px solid var(--cc-line);
  backdrop-filter: blur(12px);
  overflow: hidden;
}
/* 中间区可滚动；logo 在顶、设置 / 收起 / 监听钉在底，矮窗口也不会被推出视口 */
.navs {
  flex: 1 1 auto;
  min-height: 0;
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
  padding: 2px 0;
  mask-image: linear-gradient(to bottom, transparent 0, black 6px, black calc(100% - 14px), transparent 100%);
}
.navbot {
  flex: none;
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding-top: var(--cc-sp-1);
}
.logo {
  width: 44px;
  height: 44px;
  margin-bottom: var(--cc-sp-2);
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
  min-height: 52px;
  padding: 7px 0 6px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  font-size: var(--cc-fs-xs);
  line-height: 1.2;
  color: var(--cc-mute);
  border-radius: var(--cc-r-lg);
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
  outline: var(--cc-focus-w) solid var(--cc-focus);
  outline-offset: var(--cc-focus-off);
}
.nav em {
  position: absolute;
  top: 3px;
  right: 6px;
  font-style: normal;
  font-family: var(--cc-mono);
  font-size: 10px;
  line-height: 15px;
  min-width: 15px;
  text-align: center;
  background: var(--cc-amber);
  color: var(--cc-on-acc);
  border-radius: var(--cc-r-md);
  padding: 0 var(--cc-sp-1);
  font-weight: 700;
}
.nav em.warn {
  background: var(--cc-err);
  color: var(--cc-on-err);
}
.navtoggle {
  all: unset;
  box-sizing: border-box;
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border-radius: var(--cc-r-md);
  color: var(--cc-mute);
  cursor: pointer;
}
.navtoggle:hover {
  color: var(--cc-fg);
  background: var(--cc-inset);
}
.foot {
  flex: none;
  width: 100%;
  padding: var(--cc-sp-1) var(--cc-sp-1) 0;
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
/* 收起：只剩图标，名称靠 aria-label 和悬停提示 */
.collapsed .nav {
  width: 40px;
  min-height: 40px;
  padding: 0;
  justify-content: center;
}
.collapsed .nav span,
.collapsed .foot .ver,
.collapsed .foot .wtx {
  display: none;
}
.collapsed .nav em {
  top: 1px;
  right: 0;
}
.collapsed .logo {
  width: 36px;
  height: 36px;
}
.main {
  min-width: 0;
  overflow: auto;
}
.dropzone {
  position: fixed;
  inset: 12px;
  border: 2px dashed var(--cc-acc);
  border-radius: var(--cc-r-lg);
  background: color-mix(in srgb, var(--cc-bg) 70%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--cc-fs-lg);
  z-index: 3000;
  pointer-events: none;
}
.fatal {
  padding: 40px;
}
@media (prefers-reduced-motion: reduce) {
  .shell {
    transition: none;
  }
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
    overflow: visible;
  }
  .navs,
  .navbot {
    display: contents;
  }
  .navs {
    mask-image: none;
  }
  .logo {
    margin: 0 8px 0 0;
    width: 36px;
    height: 36px;
  }
  .foot,
  .navtoggle {
    display: none;
  }
}
</style>
