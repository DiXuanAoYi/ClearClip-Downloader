// 页面功能的后台部分：常驻内容脚本的注册、按需注入、内容脚本和扩展页面发来的消息、右键菜单。

const CONTENT_MATCHES = ['http://*/*', 'https://*/*']
let residentLock = Promise.resolve()

/** 按功能开关注册 / 注销常驻内容脚本。串行执行，避免连续改开关时互相覆盖。 */
function syncResident(reset = false) {
  residentLock = residentLock.then(() => syncResidentNow(reset)).catch((e) => console.warn('register scripts failed', e))
  return residentLock
}

async function syncResidentNow(reset) {
  const sc = api.scripting
  if (!sc?.registerContentScripts) return
  const want = new Set(neededScripts(await getFeatures()))
  const have = (await sc.getRegisteredContentScripts()).filter((s) => s.id.startsWith('cc-'))
  const drop = have.filter((s) => reset || !want.has(s.id)).map((s) => s.id)
  if (drop.length) await sc.unregisterContentScripts({ ids: drop })
  const keep = new Set(have.map((s) => s.id).filter((id) => !drop.includes(id)))
  for (const s of RESIDENT_SCRIPTS) {
    if (!want.has(s.id) || keep.has(s.id)) continue
    const def = { id: s.id, js: s.js, matches: CONTENT_MATCHES, allFrames: true, runAt: s.runAt || 'document_idle', ...(s.world ? { world: s.world } : {}) }
    try {
      await sc.registerContentScripts([{ ...def, persistAcrossSessions: true }])
    } catch {
      // Firefox 不认 persistAcrossSessions
      await sc.registerContentScripts([def])
    }
  }
}

/** 在标签页里注入脚本文件。返回每个 frame 最后一个表达式的值。 */
async function injectFiles(tabId, files, allFrames = false) {
  const res = await api.scripting.executeScript({ target: { tabId, allFrames }, files })
  return res.map((r) => r.result).filter(Boolean)
}

function isWebPage(tab) {
  return !!tab?.url && /^https?:/i.test(tab.url)
}

/** 向清影发送一段文本（链接）。返回是否在等待配对。 */
async function sendText(text, extra = {}) {
  const c = await getConfig()
  const r = await call('/api/send', { text, device: c.device, name: browserName(), ...extra })
  return r.state === 'pending_pair'
}

/** 一批要发送的项目：image 走 /api/grab，video 作为媒体发送，link / page 发送地址让清影解析。 */
async function sendBatch(items, tab) {
  const page = tab?.url || ''
  let sent = 0
  let failed = 0
  let pairing = false
  let error = ''
  for (const it of items.slice(0, 200)) {
    try {
      let p = false
      if (it.kind === 'image') {
        const r = await grabUrl({ url: it.url, referrer: page })
        p = r.state === 'pending_pair'
      } else if (it.kind === 'video' && /^https?:/i.test(it.url) && it.url !== page) {
        p = await sendText(sniffedPayload({ url: it.url }, page, tab?.title))
      } else {
        p = await sendText(await tidyUrl(it.url || page))
      }
      if (p) pairing = true
      else sent++
    } catch (e) {
      failed++
      error = e.message
      if (e.code === 'unconfigured') break
    }
  }
  return { sent, failed, pairing, error }
}

/** 打开页面媒体总览：收集页面里的媒体，放进会话存储，再打开总览页。 */
async function openOverview(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用总览')
  const frames = await injectFiles(tab.id, ['content/collect.js'], true)
  const items = []
  const seen = new Set()
  for (const f of frames) {
    for (const it of f.items || []) {
      const key = `${it.kind}|${it.url}`
      if (seen.has(key)) continue
      seen.add(key)
      items.push(it)
    }
  }
  const sniffed = arrangeSniffed(await getSniffed(tab.id)).map((x) => ({ url: x.url, kind: x.kind, label: sniffLabel(x) }))
  const key = `overview:${Date.now()}`
  await sessionStore.set({ [key]: { page: tab.url, title: tab.title || '', tabId: tab.id, items, sniffed } })
  await api.tabs.create({ url: api.runtime.getURL(`overview.html?k=${encodeURIComponent(key)}`), index: tab.index + 1 })
}

async function startPicker(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能批量选取')
  await injectFiles(tab.id, ['content/ui.js', 'content/picker.js'])
}

// ---------- 以图搜图 ----------

async function searchImage(imageUrl, which) {
  if (!/^https?:/i.test(imageUrl || '')) throw new Error('这张图片不是公开的网址（可能是内嵌图片）。可以在右键菜单里选“上传本地图片…”')
  const engines = await getEngines()
  const list = which === 'all' ? engines : [engines[Number(which)]].filter(Boolean)
  for (const e of list) await api.tabs.create({ url: engineUrl(e, imageUrl), active: which !== 'all' })
}

// ---------- 来自内容脚本和扩展页面的消息 ----------

const HANDLERS = {
  'cc-sniffed': async (_m, sender) => {
    const list = arrangeSniffed(await getSniffed(sender.tab.id)).filter((x) => x.kind !== 'image')
    return { items: list.map((x) => ({ url: x.url, kind: x.kind, label: sniffLabel(x) })) }
  },
  'cc-send-page': async (_m, sender) => ({ pairing: await sendText(await tidyUrl(sender.tab.url)) }),
  'cc-send-media': async (m, sender) => ({ pairing: await sendText(sniffedPayload({ url: String(m.url) }, sender.tab.url, sender.tab.title)) }),
  'cc-send-batch': async (m, sender) => {
    const r = await sendBatch(Array.isArray(m.items) ? m.items : [], sender.tab)
    if (!r.sent && r.failed) throw new Error(r.error || '发送失败')
    return r
  },
  // 扩展页面（弹窗、总览）发来的：带着标签页编号
  'cc-run': async (m) => {
    const tab = await api.tabs.get(Number(m.tabId))
    if (m.action === 'overview') await openOverview(tab)
    else if (m.action === 'picker') await startPicker(tab)
    else throw new Error('未知操作')
    return {}
  },
  'cc-send-items': async (m) => ({ ...(await sendBatch(Array.isArray(m.items) ? m.items : [], { url: m.page || '', title: m.title || '' })) }),
  'cc-engines': async () => ({ engines: await getEngines() }),
  'cc-save': async (m) => {
    if (!/^https?:/i.test(String(m.url))) throw new Error('这张图片不能保存')
    await api.downloads.download({ url: String(m.url), conflictAction: 'uniquify' })
    return {}
  },
  'cc-search': async (m) => {
    await searchImage(m.url, m.engine)
    return {}
  },
  // 视频画布被跨域污染时，用浏览器对可见区域的截图代替（只在用户点了截图之后）
  'cc-capture': async (_m, sender) => ({ dataUrl: await api.tabs.captureVisibleTab(sender.tab.windowId, { format: 'png' }) }),
}

api.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  const handler = msg && HANDLERS[msg.type]
  if (!handler) return false
  // 内容脚本只能从有标签页的发送者来；扩展页面的消息 sender.tab 可能为空
  if (/^cc-(sniffed|send-page|send-media|send-batch|engines|save|capture)$/.test(msg.type) && !sender.tab) return false
  handler(msg, sender)
    .then((r) => sendResponse({ ok: true, ...r }))
    .catch(async (e) => {
      if (e.code === 'unconfigured') await openSetup()
      sendResponse({ ok: false, error: e.message })
    })
  return true
})

// ---------- 右键菜单 ----------

async function buildMenus() {
  const f = await getFeatures()
  await api.contextMenus.removeAll()
  const add = (o) => api.contextMenus.create(o)
  add({ id: 'cc-page', title: '发送此页面到清影', contexts: ['page'] })
  if (f.onDemand) add({ id: 'cc-overview', title: '打开页面媒体总览', contexts: ['page'] })
  add({ id: 'cc-link', title: '发送此链接到清影', contexts: ['link'] })
  add({ id: 'cc-media', title: '发送此视频 / 音频 / 图片到清影', contexts: ['video', 'image', 'audio'] })
  if (f.onDemand) {
    add({ id: 'cc-search', title: '以图搜图', contexts: ['image'] })
    for (const [i, e] of (await getEngines()).entries()) add({ id: `cc-search:${i}`, parentId: 'cc-search', title: e.name, contexts: ['image'] })
    add({ id: 'cc-search:all', parentId: 'cc-search', title: '全部引擎各开一个标签', contexts: ['image'] })
    add({ id: 'cc-search:upload', parentId: 'cc-search', title: '上传本地图片…', contexts: ['image'] })
  }
}

let menuTimer = 0
function rebuildMenusSoon() {
  clearTimeout(menuTimer)
  menuTimer = setTimeout(() => buildMenus().catch((e) => console.warn('menus failed', e)), 300)
}

api.contextMenus.onClicked.addListener(async (info, tab) => {
  const id = String(info.menuItemId)
  try {
    if (id === 'cc-overview') await openOverview(tab)
    else if (id === 'cc-search:upload') await api.tabs.create({ url: api.runtime.getURL('search.html') })
    else if (id.startsWith('cc-search:')) await searchImage(info.srcUrl, id.slice('cc-search:'.length))
  } catch (e) {
    notify('操作没有完成', e.message)
  }
})

api.runtime.onInstalled.addListener(() => {
  buildMenus().catch((e) => console.warn('menus failed', e))
  syncResident(true)
})
api.runtime.onStartup?.addListener(() => syncResident())
api.storage.onChanged.addListener((ch, area) => {
  if (area !== 'local') return
  if (ch.features) {
    syncResident()
    rebuildMenusSoon()
  }
  if (ch.engines) rebuildMenusSoon()
})
syncResident()
