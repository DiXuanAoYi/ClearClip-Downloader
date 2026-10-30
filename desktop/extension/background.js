try {
  importScripts('shared.js', 'features.js', 'clean-link.js', 'progress.js', 'takeover.js', 'sniff.js', 'bg-link.js')
} catch {
  // Firefox 以 background.scripts 加载，脚本需在 manifest 中先加载
}

function notify(title, message) {
  api.notifications.create({ type: 'basic', iconUrl: 'icons/128.png', title, message })
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const sessionStore = api.storage.session || api.storage.local

// ---------- 右键菜单 ----------

api.runtime.onInstalled.addListener(async () => {
  await api.contextMenus.removeAll()
  api.contextMenus.create({ id: 'cc-page', title: '用清影下载此页面', contexts: ['page'] })
  api.contextMenus.create({ id: 'cc-link', title: '用清影下载此链接', contexts: ['link'] })
  api.contextMenus.create({ id: 'cc-media', title: '用清影下载此视频 / 音频 / 图片', contexts: ['video', 'image', 'audio'] })
})

/** 把浏览器里的一个文件地址交给清影下载（带 Referer、Cookie、文件名）。item：{ url, finalUrl?, referrer?, filename?, fileSize?, mime? } */
async function grabUrl(item, extra = {}) {
  let cookie = ''
  try {
    cookie = cookieHeader(await api.cookies.getAll({ url: item.finalUrl || item.url }))
  } catch {
    // 没有 Cookie 权限或地址无效：不带 Cookie
  }
  return call('/api/grab', { ...grabPayload(item, cookie, navigator.userAgent, browserName()), ...extra })
}

async function rememberGrab(entry) {
  const { recentGrabs = [] } = await api.storage.local.get('recentGrabs')
  recentGrabs.unshift({ at: Date.now(), ...entry })
  recentGrabs.length = Math.min(recentGrabs.length, 20)
  await api.storage.local.set({ recentGrabs })
}

/** 在扩展图标上短暂显示结果（✓ 已发送 / ! 失败）：系统通知可能被关掉或被专注模式拦住，图标上的标记总是看得到。 */
async function flashBadge(tabId, ok) {
  const action = api.action || api.browserAction
  if (tabId === undefined || tabId < 0) return
  try {
    await action.setBadgeText({ tabId, text: ok ? '✓' : '!' })
    await action.setBadgeBackgroundColor({ tabId, color: ok ? '#2e9e5b' : '#d64545' })
    setTimeout(() => paintBadge(tabId), 4000)
  } catch {
    // 标签页可能已关闭
  }
}

/** 右键菜单的结果：通知 + 图标标记 + 记到弹窗的“最近发送”里。 */
async function menuResult(tab, ok, shown, message, name) {
  notify(ok ? '已发送到清影' : '发送失败', message)
  await flashBadge(tab?.id, ok)
  if (shown) await rememberGrab({ url: shown, name, ok, error: ok ? undefined : message })
}

/** 打开扩展设置页（独立标签页，比弹窗宽松）；已经开着就切过去。 */
async function openSetup() {
  const url = api.runtime.getURL('popup.html#settings')
  try {
    const [open] = await api.tabs.query({ url: api.runtime.getURL('popup.html') + '*' })
    if (open) {
      await api.tabs.update(open.id, { active: true, url })
      if (open.windowId != null) await api.windows?.update(open.windowId, { focused: true })
    } else {
      await api.tabs.create({ url })
    }
  } catch {
    await api.runtime.openOptionsPage?.()
  }
}

api.contextMenus.onClicked.addListener(async (info, tab) => {
  const page = info.pageUrl || tab?.url || ''
  let shown = ''
  try {
    let r
    // reveal：这是用户明确点的菜单，电脑上的清影把主窗口带到前台
    const c = await getConfig()
    const send = (text) => call('/api/send', { text, device: c.device, name: browserName(), reveal: true })
    if (info.menuItemId === 'cc-link') {
      shown = info.linkUrl
      r = linkAction(info.linkUrl) === 'grab' ? await grabUrl({ url: info.linkUrl, referrer: page }, { reveal: true }) : await send(await tidyUrl(info.linkUrl))
    } else if (info.menuItemId === 'cc-media') {
      const src = info.srcUrl || ''
      if (!/^https?:/i.test(src)) {
        // blob: 等地址交给清影也下不了：改为发送所在网页，由清影解析
        shown = page
        r = await send(await tidyUrl(page))
      } else if (info.mediaType === 'image') {
        shown = src
        r = await grabUrl({ url: src, referrer: page }, { reveal: true })
      } else {
        // 视频 / 音频：作为媒体发送，带上所在网页作为 Referer
        shown = src
        r = await send(sniffedPayload({ url: src }, page, tab?.title))
      }
    } else {
      shown = page
      r = await send(await tidyUrl(page))
    }
    if (!shown) return
    const pairing = r.state === 'pending_pair'
    await menuResult(tab, !pairing, shown, pairing ? '第一次使用：请在电脑上点“允许”完成配对' : shortUrl(shown), r.fileNames?.[0])
  } catch (e) {
    // 还没填端口和令牌：直接打开设置页，不让用户对着没反应的菜单猜原因
    if (e.code === 'unconfigured') await openSetup()
    await menuResult(tab, false, shown, e.message)
  }
})

// ---------- 自动同步 Cookie：勾选的网站登录状态变化时（Cookie 变化后安静 10 秒）同步给清影。默认关闭。----------

const pending = new Map()
const MIN_INTERVAL = 5 * 60 * 1000

api.cookies.onChanged.addListener(async ({ cookie }) => {
  const c = await getConfig()
  if (!c.autoSync || !c.syncSites.length) return
  const site = registrable(cookie.domain.replace(/^\./, ''))
  if (!c.syncSites.includes(site)) return
  clearTimeout(pending.get(site))
  pending.set(
    site,
    setTimeout(async () => {
      pending.delete(site)
      const now = await getConfig()
      if (Date.now() - (now.lastSync[site] || 0) < MIN_INTERVAL) return
      try {
        await syncCookies(`https://${site}/`, '', true)
      } catch {
        // 清影没有运行或网站已退出登录：下次变化时再试
      }
    }, 10000),
  )
})

// ---------- 接管浏览器下载 ----------

/** 暂时不接管：弹窗里“暂停接管”，或刚刚按住 Alt 点击过。 */
async function takeoverBypassed() {
  const { pausedUntil = 0 } = await api.storage.local.get('pausedUntil')
  const { bypassUntil = 0 } = await sessionStore.get('bypassUntil')
  return Date.now() < Math.max(pausedUntil, bypassUntil)
}

// 内容脚本报告：按住 Alt 点击了页面，接下来几秒内的下载留给浏览器
api.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === 'cc-bypass') sessionStore.set({ bypassUntil: Date.now() + 6000 })
})

/** 浏览器刚开始下载时，文件名往往还没确定：等一会儿，最多约 0.7 秒。 */
async function settledItem(item) {
  let cur = item
  for (let i = 0; i < 9 && !cur.filename; i++) {
    await sleep(80)
    const [n] = await api.downloads.search({ id: item.id })
    if (!n || n.state !== 'in_progress') return null
    cur = n
  }
  return cur
}

async function takeOver(item) {
  const cfg = await getTakeover()
  if (!cfg.enabled || (await takeoverBypassed())) return
  // 扩展发起的下载（包括我们自己交还给浏览器的）不接管
  if (item.byExtensionId) return
  const cur = await settledItem(item)
  if (!cur || cur.byExtensionId) return
  const info = { ...cur, fileSize: cur.totalBytes > 0 ? cur.totalBytes : cur.fileSize, byExtension: !!cur.byExtensionId }
  if (!shouldTake(cfg, info).take) return

  // 先停掉浏览器自己的下载，再交给清影；清影没接住就交还给浏览器
  try {
    await api.downloads.cancel(item.id)
  } catch {
    return // 已经完成或无法取消：留给浏览器
  }
  try {
    await api.downloads.erase({ id: item.id })
    await api.downloads.removeFile?.(item.id)
  } catch {
    // 部分浏览器取消后没有文件可删
  }
  const name = String(cur.filename || '').split(/[\\/]/).pop()
  try {
    const r = await grabUrl(info)
    await rememberGrab({ url: info.finalUrl || info.url, name, ok: true })
    if (cfg.notify) notify('已交给清影下载', r.fileNames?.[0] || name || shortUrl(info.url))
  } catch (e) {
    await rememberGrab({ url: info.finalUrl || info.url, name, ok: false })
    notify('清影没有接住这个下载，已交还给浏览器', e.message)
    await giveBack(info.url, name)
  }
}

async function giveBack(url, name) {
  try {
    await api.downloads.download({ url, filename: name || undefined, conflictAction: 'uniquify' })
  } catch {
    await api.downloads.download({ url }).catch(() => {})
  }
}

if (api.downloads) {
  api.downloads.onCreated.addListener((item) => {
    takeOver(item).catch((e) => console.warn('takeover failed', e))
  })
}

// ---------- 视频嗅探 ----------

const enriching = new Set()

async function fetchText(url) {
  const r = await fetch(url, { credentials: 'include' })
  if (!r.ok) throw new Error(`HTTP ${r.status}`)
  return (await r.text()).slice(0, 2_000_000)
}

/** 取回 m3u8 / mpd 清单，得到时长、清晰度、预估大小和是否加密。失败就算了，列表里只是少一些信息。 */
async function enrich(tabId, item) {
  if (item.kind !== 'hls' && item.kind !== 'dash') return
  const key = sniffId(item.url)
  if (enriching.has(key)) return
  enriching.add(key)
  try {
    const detail = await describeManifest(item.kind, item.url, fetchText)
    if (detail) await setSniffDetail(tabId, item.url, detail)
  } catch {
    // 清单需要特殊的请求头或已失效
  } finally {
    enriching.delete(key)
  }
}

api.webRequest.onResponseStarted.addListener(
  async (d) => {
    if (d.tabId < 0 || d.statusCode >= 400) return
    const h = {}
    for (const x of d.responseHeaders || []) h[x.name.toLowerCase()] = x.value || ''
    const media = classifyMedia(d.url, h['content-type'], h['content-length'], h['content-range'])
    if (!media) return
    const item = { url: d.url, kind: media.kind, size: media.size, at: Date.now() }
    await addSniffed(d.tabId, item)
    await paintBadge(d.tabId)
    enrich(d.tabId, item)
  },
  { urls: ['<all_urls>'] },
  ['responseHeaders'],
)

// 换了页面：清空这个标签页之前的记录
api.tabs.onUpdated.addListener(async (tabId, change) => {
  if (change.status === 'loading' && change.url) {
    await sniffStore.remove(sniffKey(tabId))
    await paintBadge(tabId)
  }
})
api.tabs.onRemoved.addListener((tabId) => sniffStore.remove(sniffKey(tabId)))

ensureAlarm()
pollOnce().catch(() => {})
