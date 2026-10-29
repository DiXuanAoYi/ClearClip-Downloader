import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, errorKind, errorText, events } from '../api'
import type { AppInfo, ErrorKind, HistoryItem, LockStatus, MediaInfo, Settings, TaskSnapshot } from '../types'

export type ViewName = 'parse' | 'queue' | 'subs' | 'live' | 'library' | 'tools' | 'safebox' | 'settings'
export type LibraryTab = 'files' | 'inbox' | 'history'
export type ToolboxTab = 'video' | 'normalize' | 'edit' | 'lut' | 'subtitle' | 'tags' | 'ai'
export type SettingsTab = 'download' | 'parse' | 'network' | 'accounts' | 'phone' | 'ai' | 'automation' | 'security' | 'components' | 'diagnostics' | 'general'

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
  const player = ref<{ path: string; startMs?: number } | null>(null)
  /** 命令面板请求媒体库按关键词搜索；处理后清空 */
  const libraryQuery = ref<string | null>(null)
  /** 命令面板请求解析页解析这段文字；处理后清空 */
  const pendingParse = ref<string | null>(null)
  /** 请求工具箱打开并预选文件（媒体库里的“用工具箱处理”）；处理后清空 */
  const toolboxRequest = ref<{ paths: string[]; tool?: string; tab?: ToolboxTab; aiMode?: 'transcribe' | 'translate' | 'summarize'; hint?: string; lut?: string } | null>(null)

  function goSettings(tab: SettingsTab) {
    settingsTab.value = tab
    view.value = 'settings'
  }

  function goToolbox(paths: string[], tool?: string, extra?: { tab?: ToolboxTab; aiMode?: 'transcribe' | 'translate' | 'summarize'; hint?: string; lut?: string }) {
    toolboxRequest.value = { paths, tool, ...extra }
    view.value = 'tools'
  }

  function openPlayer(path: string, startMs?: number) {
    player.value = { path, startMs }
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

  async function load() {
    const [s, i] = await Promise.all([api.getSettings(), api.appInfo()])
    settings.value = s
    info.value = i
    shortcutError.value = i.shortcutError
    applyTheme()
  }

  async function save(next: Settings) {
    const r = await api.saveSettings(next)
    settings.value = r.settings
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

  return { settings, info, view, settingsTab, shortcutError, libraryTab, inboxUnhandled, loginRequest, lock, libraryQuery, pendingParse, player, openPlayer, goLibraryQuery, toolboxRequest, refreshLock, goSettings, goLibrary, goToolbox, goLogin, load, save, patch, applyTheme }
})

/** 下载队列，监听后端事件保持同步。 */
export const useQueueStore = defineStore('queue', () => {
  const tasks = ref<TaskSnapshot[]>([])
  let started = false

  async function start() {
    if (started) return
    started = true
    tasks.value = await api.listTasks()
    await events.onTasks((list) => (tasks.value = list))
    await events.onProgress((t) => {
      const i = tasks.value.findIndex((x) => x.id === t.id)
      if (i >= 0) tasks.value[i] = t
    })
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
