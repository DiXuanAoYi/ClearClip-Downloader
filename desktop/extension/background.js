try {
  importScripts('shared.js')
} catch {
  // Firefox 以 background.scripts 加载，shared.js 需在 manifest 中先加载
}

function notify(title, message) {
  api.notifications.create({ type: 'basic', iconUrl: 'icons/128.png', title, message })
}

api.runtime.onInstalled.addListener(() => {
  api.contextMenus.create({ id: 'cc-page', title: '发送此页面到清影', contexts: ['page'] })
  api.contextMenus.create({ id: 'cc-link', title: '发送链接到清影', contexts: ['link'] })
  api.contextMenus.create({ id: 'cc-media', title: '发送视频 / 图片到清影', contexts: ['video', 'image', 'audio'] })
})

api.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.menuItemId === 'cc-link' ? info.linkUrl : info.menuItemId === 'cc-media' ? info.srcUrl || info.pageUrl : info.pageUrl || tab?.url
  if (!url) return
  try {
    const r = await sendUrl(url)
    notify('已发送到清影', r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : url)
  } catch (e) {
    notify('发送失败', e.message)
  }
})

// 自动同步：勾选的网站登录状态变化时（Cookie 变化后安静 10 秒）同步给清影。默认关闭。
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

// ---------- 视频嗅探 ----------

api.webRequest.onResponseStarted.addListener(
  async (d) => {
    if (d.tabId < 0 || d.statusCode >= 400) return
    const h = {}
    for (const x of d.responseHeaders || []) h[x.name.toLowerCase()] = x.value || ''
    const media = classifyMedia(d.url, h['content-type'], h['content-length'], h['content-range'])
    if (!media) return
    const n = await addSniffed(d.tabId, { url: d.url, kind: media.kind, size: media.size, at: Date.now() })
    try {
      await (api.action || api.browserAction).setBadgeText({ tabId: d.tabId, text: String(n) })
      await (api.action || api.browserAction).setBadgeBackgroundColor({ tabId: d.tabId, color: '#d9772f' })
    } catch {
      // 标签页可能已关闭
    }
  },
  { urls: ['<all_urls>'] },
  ['responseHeaders'],
)

// 换了页面：清空这个标签页之前的记录
api.tabs.onUpdated.addListener(async (tabId, change) => {
  if (change.status === 'loading' && change.url) {
    await sniffStore.remove(sniffKey(tabId))
    try {
      await (api.action || api.browserAction).setBadgeText({ tabId, text: '' })
    } catch {
      // ignore
    }
  }
})
api.tabs.onRemoved.addListener((tabId) => sniffStore.remove(sniffKey(tabId)))
