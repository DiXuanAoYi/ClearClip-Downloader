import { defineStore } from 'pinia'
import { computed, ref, shallowRef, watch } from 'vue'
import { api, errorKind, errorText, events } from '../api'
import { clone } from '../utils/merge3'
import { createProgressBatcher, mergeProgress } from '../utils/progressBatch'
import type { AppInfo, BatchRequest, ErrorKind, HistoryItem, LockStatus, MediaInfo, Settings, TaskSnapshot } from '../types'

export type ViewName = 'parse' | 'queue' | 'subs' | 'live' | 'screen' | 'filetr' | 'library' | 'vocab' | 'tools' | 'settings'
export type LibraryTab = 'files' | 'inbox' | 'transfer' | 'history'
export type ToolboxTab = 'video' | 'normalize' | 'edit' | 'lut' | 'subtitle' | 'tags' | 'ai'
export type SettingsTab = 'download' | 'parse' | 'p2p' | 'network' | 'accounts' | 'phone' | 'ai' | 'writing' | 'automation' | 'security' | 'components' | 'diagnostics' | 'general'

function readNavOpen(): boolean {
  try {
    return localStorage.getItem('clearclip.toolbox.nav') === '1'
  } catch {
    return false
  }
}

/** 设置、主题与当前页面。 */
export const useAppStore = defineStore('app', () => {
  const settings = ref<Settings | null>(null)
  const info = ref<AppInfo | null>(null)
  const view = ref<ViewName>('parse')
  const settingsTab = ref<SettingsTab>('download')
  const shortcutError = ref<string | null>(null)
  const libraryTab = ref<LibraryTab>('files')
  /** 需要处理的收到的链接数量（侧栏角标） */
  const inboxUnhandled = ref(0)
  /** 请求账号页打开某个网站的登录（内置平台 ID 或域名）；处理后清空 */
  const loginRequest = ref<string | null>(null)
  /** 应用锁状态；locked 时界面被锁屏遮住 */
  const lock = ref<LockStatus>({ enabled: false, locked: false, lockedOutSecs: 0 })
  /** 内置播放器：要播放的文件和起始位置；为 null 时关闭 */
  const player = ref<{ path: string; startMs?: number; taskId?: number } | null>(null)
  /** 命令面板请求媒体库按关键词搜索；处理后清空 */
  const libraryQuery = ref<string | null>(null)
  /** 命令面板请求解析页解析这段文字；处理后清空 */
  const pendingParse = ref<string | null>(null)
  /** 请求工具箱打开并预选文件（媒体库里的“用工具箱处理”）；处理后清空 */
  const toolboxRequest = ref<{ paths: string[]; tool?: string; tab?: ToolboxTab; aiMode?: 'transcribe' | 'summarize' | 'correct'; hint?: string; lut?: string } | null>(null)

  /** 工具箱的文件篮：所有工具共用，换工具文件不丢 */
  const toolFiles = ref<string[]>([])
  /** 进工具箱之前所在的页面（工具箱的返回按钮回到这里） */
  const toolsFrom = ref<ViewName>('parse')
  watch(view, (n, o) => {
    if (n === 'tools' && o !== 'tools') toolsFrom.value = o
  })
  /** 文件翻译正在版面里调整：主导航和左栏都收起来，给页面让地方 */
  const layoutWork = ref(false)
  /** 工具箱里主导航是否展开（默认收成图标条），记住选择 */
  const navOpen = ref(readNavOpen())
  function setNavOpen(v: boolean) {
    navOpen.value = v
    try {
      localStorage.setItem('clearclip.toolbox.nav', v ? '1' : '0')
    } catch {
      /* 存不了就只在这次有效 */
    }
  }

  /** 请求文件翻译页翻译这些文件（媒体库里字幕的“翻译字幕”）；处理后清空 */
  const filetrRequest = ref<string[] | null>(null)
  function goFiletr(paths: string[]) {
    filetrRequest.value = paths
    view.value = 'filetr'
  }

  /** 请求打开“批量添加”面板（链接已提取好）；面板打开后清空 */
  const batchRequest = ref<BatchRequest | null>(null)
  /** 主页的模式：单个解析 / 批量下载（批量不再是单独的页面） */
  const homeMode = ref<'single' | 'batch'>('single')
  /** 下载队列只看某个批次（批量下载页点“查看任务”）；null = 全部 */
  const queueBatch = ref<string | null>(null)
  function goQueueBatch(id: string | null) {
    queueBatch.value = id
    view.value = 'queue'
  }
  function openBatch(urls: string[], source: string, truncated = false) {
    batchRequest.value = { urls, source, truncated }
    homeMode.value = 'batch'
    view.value = 'parse'
  }

  function goSettings(tab: SettingsTab) {
    settingsTab.value = tab
    view.value = 'settings'
  }

  function goToolbox(paths: string[], tool?: string, extra?: { tab?: ToolboxTab; aiMode?: 'transcribe' | 'summarize' | 'correct'; hint?: string; lut?: string }) {
    toolboxRequest.value = { paths, tool, ...extra }
    view.value = 'tools'
  }

  function openPlayer(path: string, startMs?: number) {
    player.value = { path, startMs }
  }

  /** 边下边播：播放一个还在下载的任务。 */
  function previewTask(taskId: number) {
    player.value = { path: '', taskId }
  }

  function goLibraryQuery(q: string) {
    libraryQuery.value = q
    libraryTab.value = 'files'
    view.value = 'library'
  }

  function goLibrary(tab: LibraryTab) {
    libraryTab.value = tab
    view.value = 'library'
  }

  /** 打开账号页并直接开始登录这个网站。 */
  function goLogin(site: string | null | undefined) {
    loginRequest.value = site || null
    goSettings('accounts')
  }

  async function refreshLock() {
    lock.value = await api.lockStatus().catch(() => lock.value)
  }

  // 最近一次和后端同步的设置：保存时作为三方合并的基准
  let base: Settings | null = null
  /** 别处改了设置时加一，编辑设置的页面据此把改动合进自己的表单 */
  const settingsRev = ref(0)
  let listening = false

  async function load() {
    const [s, i] = await Promise.all([api.getSettings(), api.appInfo()])
    settings.value = s
    base = clone(s)
    if (!listening) {
      listening = true
      void events.onSettingsChanged((n) => {
        settings.value = n
        base = clone(n)
        settingsRev.value++
        applyTheme()
      })
    }
    info.value = i
    shortcutError.value = i.shortcutError
    applyTheme()
  }

  /** 保存设置。editorBase：表单打开时的那份（不给就用最近同步的），后端只采用表单真的改过的字段。 */
  async function save(next: Settings, editorBase?: Settings | null) {
    const r = await api.saveSettings(next, editorBase ?? base)
    settings.value = r.settings
    base = clone(r.settings)
    shortcutError.value = r.shortcutError
    applyTheme()
  }

  async function patch(p: Partial<Settings>) {
    if (!settings.value) return
    await save({ ...settings.value, ...p })
  }

  const media = window.matchMedia('(prefers-color-scheme: dark)')
  function applyTheme() {
    const t = settings.value?.theme ?? 'system'
    const dark = t === 'dark' || (t === 'system' && media.matches)
    document.documentElement.classList.toggle('dark', dark)
  }
  media.addEventListener('change', applyTheme)

  return { homeMode, filetrRequest, goFiletr, settings, info, view, settingsTab, shortcutError, libraryTab, inboxUnhandled, loginRequest, lock, libraryQuery, pendingParse, batchRequest, openBatch, queueBatch, goQueueBatch, player, openPlayer, previewTask, goLibraryQuery, toolboxRequest, toolFiles, toolsFrom, navOpen, layoutWork, setNavOpen, refreshLock, goSettings, goLibrary, goToolbox, goLogin, load, save, patch, applyTheme, settingsRev }
})

/**
 * 下载队列，监听后端事件保持同步。
 *
 * 列表用 shallowRef：进度事件很密（任务多时每秒几百次），不用把每个任务深度变成响应式对象；
 * 同一帧内同一任务的多条进度合并成最后一条，一帧只换一次数组（没变的任务对象引用不变）。
 */
export const useQueueStore = defineStore('queue', () => {
  const tasks = shallowRef<TaskSnapshot[]>([])
  const batcher = createProgressBatcher<TaskSnapshot>((updates) => {
    const next = mergeProgress(tasks.value, updates)
    if (next !== tasks.value) tasks.value = next as TaskSnapshot[]
  })
  let started = false

  async function start() {
    if (started) return
    started = true
    tasks.value = await api.listTasks()
    await events.onTasks((list) => {
      // 完整列表比还没写入的进度新，旧进度不能再盖回去
      batcher.clear()
      tasks.value = list
    })
    await events.onProgress((t) => batcher.push(t))
  }

  const counts = computed(() => {
    const c = { running: 0, queued: 0, done: 0, failed: 0, paused: 0 }
    for (const t of tasks.value) {
      if (t.status === 'running') c.running++
      else if (t.status === 'queued') c.queued++
      else if (t.status === 'done') c.done++
      else if (t.status === 'failed') c.failed++
      else if (t.status === 'paused') c.paused++
    }
    return c
  })
  const active = computed(() => counts.value.running + counts.value.queued)
  const totalSpeed = computed(() => tasks.value.reduce((sum, t) => sum + (t.status === 'running' ? t.speed : 0), 0))

  return { tasks, start, counts, active, totalSpeed }
})

/** 解析页的状态（App 级的剪贴板 / 快捷键事件也会写入这里）。 */
export const useParseStore = defineStore('parse', () => {
  const text = ref('')
  const loading = ref(false)
  const error = ref('')
  const errorKindRef = ref<ErrorKind>('other')
  /** 需要登录时对应的网站 */
  const errorSite = ref<string | null>(null)
  const result = ref<MediaInfo | null>(null)
  const recent = ref<HistoryItem[]>([])

  async function parse(input?: string) {
    if (input !== undefined) text.value = input
    const t = text.value.trim()
    if (!t) {
      error.value = '请先粘贴作品的分享链接。'
      return
    }
    loading.value = true
    error.value = ''
    try {
      result.value = await api.resolve(t)
      await loadRecent()
    } catch (e) {
      result.value = null
      error.value = errorText(e)
      errorKindRef.value = errorKind(e)
      errorSite.value = null
      if (errorKindRef.value === 'need_login' || errorKindRef.value === 'rate_limited') errorSite.value = await api.loginSite(t).catch(() => null)
    } finally {
      loading.value = false
    }
  }

  async function loadRecent() {
    recent.value = (await api.listHistory('')).slice(0, 5)
  }

  function clear() {
    text.value = ''
    error.value = ''
    result.value = null
  }

  return { text, loading, error, errorKind: errorKindRef, errorSite, result, recent, parse, loadRecent, clear }
})
