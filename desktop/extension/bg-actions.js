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
  const features = await getFeatures()
  const rules = await getSiteRules()
  const trans = await getTransCfg()
  // 编号 → 要匹配的网址；匹配范围为空的（比如解除限制的名单是空的）不注册
  const want = new Map()
  for (const s of RESIDENT_SCRIPTS) {
    if (!s.need(normalizeFeatures(features))) continue
    const matches = s.matches ? s.matches(rules, trans) : CONTENT_MATCHES
    if (matches.length) want.set(s.id, matches)
  }
  const have = (await sc.getRegisteredContentScripts()).filter((s) => s.id.startsWith('cc-'))
  const same = (a, b) => a.length === b.length && a.every((x) => b.includes(x))
  const drop = have.filter((s) => reset || !want.has(s.id) || !same(s.matches || [], want.get(s.id))).map((s) => s.id)
  if (drop.length) await sc.unregisterContentScripts({ ids: drop })
  const keep = new Set(have.map((s) => s.id).filter((id) => !drop.includes(id)))
  for (const s of RESIDENT_SCRIPTS) {
    if (!want.has(s.id) || keep.has(s.id)) continue
    const def = { id: s.id, js: s.js, matches: want.get(s.id), allFrames: true, runAt: s.runAt || 'document_idle', ...(s.world ? { world: s.world } : {}) }
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

/** 把图片缩到最长边 1400 像素以内、压成 JPEG，返回 data URL（给清影识字用，体积要小）。读不到 / 解不了返回空串。 */
async function shrinkImage(url, referrer = '') {
  const ctl = new AbortController()
  const timer = setTimeout(() => ctl.abort(), 15000)
  try {
    const r = await fetch(url, { signal: ctl.signal, credentials: 'include', referrer: referrer || undefined, referrerPolicy: 'no-referrer-when-downgrade' })
    if (!r.ok) return ''
    const blob = await r.blob()
    if (blob.size > 12 * 1024 * 1024) return ''
    const bmp = await createImageBitmap(blob)
    const k = Math.min(1, 1400 / Math.max(bmp.width, bmp.height))
    const w = Math.max(1, Math.round(bmp.width * k))
    const h = Math.max(1, Math.round(bmp.height * k))
    const cv = new OffscreenCanvas(w, h)
    const g = cv.getContext('2d')
    g.fillStyle = '#fff'
    g.fillRect(0, 0, w, h)
    g.drawImage(bmp, 0, 0, w, h)
    let q = 0.85
    let out = await cv.convertToBlob({ type: 'image/jpeg', quality: q })
    while (out.size > 1_100_000 && q > 0.4) {
      q -= 0.15
      out = await cv.convertToBlob({ type: 'image/jpeg', quality: q })
    }
    if (out.size > 1_100_000) return ''
    const bytes = new Uint8Array(await out.arrayBuffer())
    let bin = ''
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    return `data:image/jpeg;base64,${btoa(bin)}`
  } catch {
    return ''
  } finally {
    clearTimeout(timer)
  }
}

/** 读出一张图片里的文字（清影本机识字），最多等 60 秒；失败返回空串。 */
async function ocrOfImage(url, referrer) {
  const image = await shrinkImage(url, referrer)
  if (!image) return ''
  const sub = await call('/api/v1/ai/run', { kind: 'ocrtext', image })
  const deadline = Date.now() + 60_000
  for (;;) {
    await sleep(400)
    const j = await call(`/api/v1/ai/job?id=${encodeURIComponent(String(sub.id))}`)
    if (j.state === 'done') return String(j.result?.text || '')
    if (j.state === 'error' || j.state === 'cancelled' || Date.now() > deadline) return ''
  }
}

/** 一批要发送的项目：image 走 /api/grab，video 作为媒体发送，link / page 发送地址让清影解析。 */
/** 读一个媒体地址开头的几百字节，认出真实格式和大小。读不到不报错，返回空结果。 */
async function probeMedia(url, referrer = '') {
  const out = { ok: false, ext: '', mime: '', size: 0 }
  if (!/^https?:/i.test(String(url))) return out
  const ctl = new AbortController()
  const timer = setTimeout(() => ctl.abort(), 8000)
  try {
    const r = await fetch(url, { headers: { Range: 'bytes=0-599' }, signal: ctl.signal, credentials: 'include', referrer: referrer || undefined, referrerPolicy: 'no-referrer-when-downgrade' })
    out.mime = (r.headers.get('content-type') || '').split(';')[0].trim()
    const range = r.headers.get('content-range')
    out.size = Number((range && range.split('/')[1]) || r.headers.get('content-length')) || 0
    const reader = r.body.getReader()
    const chunks = []
    let got = 0
    while (got < 600) {
      const { value, done } = await reader.read()
      if (done) break
      chunks.push(value)
      got += value.length
    }
    reader.cancel().catch(() => {})
    const head = new Uint8Array(got)
    let off = 0
    for (const c of chunks) (head.set(c, off), (off += c.length))
    out.ext = ccSniff.sniffExt(head.slice(0, 600)) || ccSniff.extOfMime(out.mime)
    out.ok = r.ok
  } catch {
    // 网络或跨域限制：没有结果
  } finally {
    clearTimeout(timer)
  }
  return out
}

/**
 * 选集：打开这一集的页面（后台标签页、静音），等它播放起来、扩展嗅探到视频流后取下流地址并关掉标签页。
 * 最多等 25 秒；没识别到返回 null。
 */
async function streamOfPage(url, timeoutMs = 25000) {
  const tab = await api.tabs.create({ url, active: false })
  try {
    const t0 = Date.now()
    while (Date.now() - t0 < timeoutMs) {
      await sleep(1000)
      try {
        // 后台标签页不一定自动播放：静音后主动播一下（静音的播放浏览器一般允许）
        await api.scripting.executeScript({
          target: { tabId: tab.id, allFrames: true },
          func: () => document.querySelectorAll('video').forEach((v) => ((v.muted = true), v.play?.().catch(() => {}))),
        })
      } catch {
        // 页面还没加载好 / 不允许注入：下一轮再试
      }
      const item = pickStream(await getSniffed(tab.id))
      if (item) return { item, title: (await api.tabs.get(tab.id).catch(() => tab))?.title || '' }
    }
    return null
  } finally {
    api.tabs.remove(tab.id).catch(() => {})
  }
}

async function sendBatch(items, tab) {
  const page = tab?.url || ''
  const { streamSites } = await api.storage.local.get('streamSites')
  const sites = streamFirstSites(streamSites)
  let sent = 0
  let failed = 0
  let pairing = false
  let error = ''
  for (const it of items.slice(0, 200)) {
    try {
      let p = false
      if (it.kind === 'image') {
        const real = it.ext || (await probeMedia(it.url, page)).ext
        const r = await grabUrl({ url: it.url, referrer: page, mime: it.mime || (real && ccSniff.MIME[real]) || '', filename: it.name ? ccNaming.withExt(ccNaming.cleanName(it.name), real) : real ? ccSniff.fileNameFor(it.url, real) : '' })
        p = r.state === 'pending_pair'
      } else if (it.kind !== 'video' && isStreamFirst(it.url, sites)) {
        // 选集：这一集的页面里没有视频地址，开页面取到真实的视频流再发
        const r = await streamOfPage(it.url)
        if (!r) throw new Error('没有识别到这一集的视频流（页面没有播放起来）：' + shortUrl(it.url))
        p = await sendText(sniffedPayload(r.item, it.url, it.name || r.title || tab?.title))
      } else if (it.kind === 'video' && /^https?:/i.test(it.url) && it.url !== page) {
        p = await sendText(sniffedPayload({ url: it.url }, page, it.name || tab?.title))
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

/** 收集页面里看起来是视频页面的链接（去重，最多 500 条）。函数会被注入到页面里执行，必须自成一体。 */
function pageVideoLinks() {
  const PATTERNS = [
    /youtube\.com\/(watch\?|shorts\/|live\/)/i, /youtu\.be\//i, /bilibili\.com\/(video\/|bangumi\/play\/|medialist\/)/i, /b23\.tv\//i,
    /douyin\.com\/(video|note)\//i, /v\.douyin\.com\//i, /tiktok\.com\/@[^/]+\/video\//i, /ixigua\.com\/\d+/i, /kuaishou\.com\/(short-video|f)\//i,
    /xiaohongshu\.com\/(explore|discovery\/item)\//i, /weibo\.(com|cn)\/(tv|\d+)\//i, /vimeo\.com\/\d+/i, /twitter\.com\/[^/]+\/status\//i, /x\.com\/[^/]+\/status\//i,
    /\.(mp4|m3u8|webm|mkv|mov|flv)(\?|$)/i, /\/(video|videos|watch|play|v)\/[\w-]{4,}/i,
  ]
  const seen = new Set()
  const out = []
  for (const a of document.querySelectorAll('a[href]')) {
    let u
    try {
      u = new URL(a.href, location.href)
    } catch {
      continue
    }
    if (!/^https?:$/.test(u.protocol) || !PATTERNS.some((r) => r.test(u.href))) continue
    u.hash = ''
    const k = u.href
    if (seen.has(k)) continue
    seen.add(k)
    out.push(k)
    if (out.length >= 500) break
  }
  return out
}

/** 本页所有视频链接：收集后交给清影，在电脑上打开批量添加面板。 */
async function sendPageVideoLinks(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用这个功能')
  const { streamSites } = await api.storage.local.get('streamSites')
  if (isStreamFirst(tab.url, streamFirstSites(streamSites))) throw new Error('这个网站要一集一集取视频流：请用“批量选取”点选要下载的集数。')
  const [res] = await api.scripting.executeScript({ target: { tabId: tab.id }, func: pageVideoLinks })
  const urls = (res && res.result) || []
  if (!urls.length) throw new Error('这个页面上没有找到视频链接')
  const c = await getConfig()
  const r = await call('/api/send', { text: urls.join('\n'), device: c.device, name: browserName(), batch: true })
  if (r.state === 'pending_pair') throw new Error('第一次使用：请在电脑上点“允许”完成配对，然后再试一次')
  return { count: r.count || urls.length }
}

/** 打开页面媒体总览：收集页面里的媒体，放进会话存储，再打开总览页。 */
async function openOverview(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用总览')
  const frames = await injectFiles(tab.id, ccFiles.GROUPS.collect, true)
  const items = []
  const seen = new Set()
  const excerpt = (frames.find((f) => f.excerpt && f.frame === tab.url) || frames.find((f) => f.excerpt))?.excerpt || ''
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
  await sessionStore.set({ [key]: { page: tab.url, title: tab.title || '', tabId: tab.id, items, sniffed, excerpt } })
  await api.tabs.create({ url: api.runtime.getURL(`overview.html?k=${encodeURIComponent(key)}`), index: tab.index + 1 })
}

async function startPicker(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能批量选取')
  await injectFiles(tab.id, ccFiles.GROUPS.picker)
}

/** 打开视频摘要面板（再次注入 = 关闭）。 */
async function startVsum(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用视频摘要')
  await injectFiles(tab.id, ccFiles.GROUPS.vsum)
}

/** 在当前页面的视频上叠字幕层（再次注入 = 关闭）。 */
async function startShot(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用截图翻译')
  await injectFiles(tab.id, ccFiles.GROUPS.shot)
}

async function startSubLayer(tab) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用字幕层')
  await injectFiles(tab.id, ccFiles.GROUPS.subLayer)
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
  'cc-send-page': async (_m, sender) => {
    await firstTimeSync(sender.tab.url)
    return { pairing: await sendText(await tidyUrl(sender.tab.url)) }
  },
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
    else if (m.action === 'vsum') await startVsum(tab)
    else if (m.action === 'sublayer') await startSubLayer(tab)
    else if (m.action === 'shot') await startShot(tab)
    else if (m.action === 'batch-links') return await sendPageVideoLinks(tab)
    else if (m.action === 'webtrans') await webTransDo(tab, String(m.mode || 'toggle'))
    else if (m.action === 'excludepick') await injectFiles(tab.id, ccFiles.GROUPS.excludePick)
    else throw new Error('未知操作')
    return {}
  },
  'cc-send-items': async (m) => ({ ...(await sendBatch(Array.isArray(m.items) ? m.items : [], { url: m.page || '', title: m.title || '' })) }),
  // 视频上的音效面板点“更多音效设置”：在新标签页打开弹窗的音效页
  'cc-open-audio': async () => (await api.tabs.create({ url: api.runtime.getURL('popup.html#audio') }), {}),
  'cc-engines': async () => ({ engines: await getEngines() }),
  'cc-save': async (m) => {
    if (!/^https?:/i.test(String(m.url))) throw new Error('这张图片不能保存')
    // 地址没有后缀或后缀不对时，按真实格式命名（不再保存成 .bin）
    const real = (await probeMedia(String(m.url), String(m.referrer || ''))).ext
    const cur = ccSniff.extOfUrl(String(m.url))
    const filename = real && real !== cur ? ccSniff.fileNameFor(String(m.url), real) : undefined
    await api.downloads.download({ url: String(m.url), filename, conflictAction: 'uniquify' })
    return {}
  },
  // 总览：探测图片的真实格式和大小
  'cc-probe-media': async (m) => ({ ...(await probeMedia(String(m.url), String(m.referrer || ''))) }),
  'cc-search': async (m) => {
    await searchImage(m.url, m.engine)
    return {}
  },
  // 页面保存：弹窗发来（带标签页编号）；选区截图由页面里的选框发来
  'cc-page-save': async (m) => ({ ...(await pageSave(String(m.action), await api.tabs.get(Number(m.tabId)), '')) }),
  // 截图翻译：裁出选区，连同“翻译成什么语言”一起交给页面
  'cc-shot': async (m, sender) => {
    const cfg = await getTransCfg()
    const r = m.rect || {}
    const rect = { x: Number(r.x) || 0, y: Number(r.y) || 0, w: Number(r.w) || 0, h: Number(r.h) || 0 }
    if (rect.w < 8 || rect.h < 8) throw new Error('选区太小')
    return { ...(await cropForTranslate(sender.tab, rect, Number(m.vw) || 0)), lang: ccTrans.promptName(cfg.target) }
  },
  'cc-area': async (m, sender) => {
    await captureArea(sender.tab, m.rect, m.vw)
    return {}
  },
  // 在当前页面立刻启用解除限制（不用刷新）：弹窗里给这个网站打开开关之后调用
  'cc-unlock-now': async (m) => {
    const tab = await api.tabs.get(Number(m.tabId))
    if (!isWebPage(tab)) throw new Error('当前页面不能使用这个功能')
    await api.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, files: ccFiles.GROUPS.unlock, world: 'MAIN' })
    return {}
  },
  // 视频摘要：面板把请求交给清影（本机），由清影调用设置里的大模型；长任务提交后轮询
  'cc-ai-status': async () => ({ ...(await call('/api/v1/ai/status')) }),
  // 页面里直接取字幕文件被跨域规则拦住时，由后台代取（只允许 B 站的字幕域名，只读，不带 Cookie）
  'cc-fetch-sub': async (m) => {
    const u = new URL(String(m.url || '').replace(/^\/\//, 'https://').replace(/^http:/, 'https:'))
    if (!/(^|\.)(hdslb\.com|bilibili\.com|biliapi\.net)$/.test(u.hostname)) throw new Error('不支持的地址')
    const r = await fetch(u.href, { credentials: 'omit' })
    if (!r.ok) throw new Error(`HTTP ${r.status}`)
    return { text: await r.text() }
  },
  // 清影界面语言：问一次清影并记下来；清影没开就用上次记下的
  'cc-app-lang': async () => {
    try {
      const r = await call('/api/ping')
      if (r.lang) await api.storage.local.set({ appLang: r.lang })
    } catch {
      // 清影没开：用上次的
    }
    return { lang: (await api.storage.local.get('appLang')).appLang || '' }
  },
  // 字幕学习：生词本（清影桌面端保存）
  'cc-open-highlights': async () => (await api.tabs.create({ url: api.runtime.getURL('highlights.html') }), {}),
  // 视频缓存：同步到清影 / 查询是否允许自动同步
  'cc-videocache': async (m) => {
    if (m.op === 'put') return { ...(await vcache.put(m.payload && typeof m.payload === 'object' ? m.payload : {})) }
    if (m.op === 'info') return { ...(await vcache.info(String(m.url))) }
    throw new Error('不支持的操作')
  },
  'cc-vocab': async (m) => {
    const enc = encodeURIComponent
    switch (m.op) {
      case 'words':
        return { ...(await call('/api/v1/vocab/words')) }
      case 'list':
        return { ...(await call(`/api/v1/vocab/list?q=${enc(String(m.q || '').slice(0, 80))}&state=${enc(String(m.state || 'all'))}`)) }
      case 'add':
        return { ...(await call('/api/v1/vocab/add', m.word && typeof m.word === 'object' ? m.word : {})) }
      case 'remove':
        return { ...(await call('/api/v1/vocab/remove', { id: Number(m.id) || 0 })) }
      case 'unfamiliar':
        return { ...(await call('/api/v1/vocab/unfamiliar', { words: Array.isArray(m.words) ? m.words.slice(0, 400) : [], minRank: Number(m.minRank) || 0 })) }
      case 'export':
        return { ...(await call(`/api/v1/vocab/export?format=${enc(String(m.format || 'csv'))}`)) }
      case 'clip':
        return { ...(await call('/api/v1/vocab/clip', { word: String(m.word || '').slice(0, 80), data: String(m.data || '') })) }
      case 'clips':
        return { ...(await call('/api/v1/vocab/clips')) }
      default:
        throw new Error('不支持的操作')
    }
  },
  // 总览：让清影里的大模型起名字。依据：网页标题、页面正文摘录（理解主题）、每项的图片说明 / 周围文字，以及图片里识出的文字（OCR，本机识字）。
  'cc-ai-name': async (m) => {
    const items = (Array.isArray(m.items) ? m.items : []).slice(0, 60)
    if (!items.length) return { names: [] }
    // 先给图片读字（最多 12 张，3 张并行；读不出来就不带，不影响起名）
    const ocr = new Array(items.length).fill('')
    if (m.ocr !== false) {
      const todo = items.map((x, i) => i).filter((i) => items[i].kind === 'image' && /^https?:/i.test(items[i].url || '')).slice(0, 12)
      let next = 0
      const worker = async () => {
        while (next < todo.length) {
          const i = todo[next++]
          try {
            ocr[i] = await ocrOfImage(items[i].url, String(m.page || ''))
          } catch {
            /* 这张读不了：只用其他信息 */
          }
        }
      }
      await Promise.all([worker(), worker(), worker()])
    }
    const lines = items.map((x, t) => ({
      t,
      text: [x.kind, x.alt && `alt: ${x.alt}`, x.cap && `caption: ${x.cap}`, x.text && x.text !== x.alt && `link: ${x.text}`, x.orig && `file: ${x.orig}`, x.near && `near: ${x.near}`, ocr[t] && `OCR: ${ocr[t]}`].filter(Boolean).join(' | ').slice(0, 420),
    }))
    const sub = await call('/api/v1/ai/run', { kind: 'namer', title: String(m.title || '').slice(0, 200), text: String(m.excerpt || '').slice(0, 1500), lang: String(m.lang || ''), lines })
    const deadline = Date.now() + 2 * 60_000
    for (;;) {
      await sleep(450)
      const j = await call(`/api/v1/ai/job?id=${encodeURIComponent(String(sub.id))}`)
      if (j.state === 'done') return { names: j.result?.names || [], ocr: ocr.map((t) => !!t) }
      if (j.state === 'error') throw new Error(j.error || 'AI 命名失败了')
      if (j.state === 'cancelled') throw new Error('已取消')
      if (Date.now() > deadline) throw new Error('AI 命名超时')
    }
  },
  'cc-ai-run': async (m) => ({ ...(await call('/api/v1/ai/run', m.req && typeof m.req === 'object' ? m.req : {})) }),
  'cc-ai-job': async (m) => ({ ...(await call(`/api/v1/ai/job?id=${encodeURIComponent(String(m.id))}`)) }),
  'cc-ai-cancel': async (m) => ({ ...(await call(`/api/v1/ai/cancel?id=${encodeURIComponent(String(m.id))}`, {})) }),
  // 网页翻译：内容脚本把一批文字发来，返回译文
  'cc-tr': async (m) => {
    const cfg = await getTransCfg()
    const texts = (Array.isArray(m.texts) ? m.texts : []).map((t) => String(t)).slice(0, 60)
    if (!texts.length) return { texts: [], cached: 0 }
    const engine = ccTrans.ENGINES.includes(m.engine) ? m.engine : ccTrans.pickEngine(cfg)
    const p = m.page && typeof m.page === 'object' ? m.page : null
    const page = p ? { host: String(p.host || '').slice(0, 100), title: String(p.title || ''), sample: String(p.sample || '') } : null
    return translateTexts(texts, { target: String(m.target || ccTrans.engineTarget(cfg, engine)), engine, page, domain: cfg.domain, extra: ccTrans.enginePrompt(cfg, engine) })
  },
  // 独立翻译窗：同一段文字用几个翻译服务各翻一遍，并排给出结果（任何一个失败不影响别的）
  'cc-trbox': async (m) => {
    const cfg = await getTransCfg()
    const text = String(m.text || '').slice(0, 5000)
    if (!text.trim()) return { results: [] }
    const target = String(m.target || cfg.target)
    const wanted = Array.isArray(m.engines) ? m.engines.filter((e) => ccTrans.ENGINES.includes(e)) : []
    const engines = wanted.length ? wanted : ccTrans.activeEngines(cfg)
    const results = await Promise.all(
      engines.map(async (engine) => {
        const name = ccTrans.ENGINE_NAMES[engine] || engine
        try {
          const r = await translateTexts([text], { target, engine, page: null, domain: false, extra: ccTrans.enginePrompt(cfg, engine) })
          return { engine, name, text: r.texts[0] }
        } catch (e) {
          return { engine, name, error: String(e?.message || e) }
        }
      }),
    )
    return { results, engines: ccTrans.activeEngines(cfg), names: ccTrans.ENGINE_NAMES }
  },
  // 划词工具条的“翻译”：在选区所在的那个页面（框架）里打开网页翻译的划词卡片；那里还没有翻译脚本就先注入
  'cc-wt-sel': async (m, sender) => {
    const tab = sender?.tab
    if (!tab) throw new Error('找不到页面')
    const frameId = sender.frameId ?? 0
    const r0 = m.rect && typeof m.rect === 'object' ? m.rect : null
    const rect = r0 ? { left: Number(r0.left) || 0, right: Number(r0.right) || 0, top: Number(r0.top) || 0, bottom: Number(r0.bottom) || 0 } : null
    const msg = { type: 'cc-wt', action: 'selection', text: String(m.text || '').slice(0, 5000), rect }
    const ready = (r) => r && typeof r === 'object' && 'on' in r
    try {
      if (ready(await api.tabs.sendMessage(tab.id, msg, { frameId }))) return {}
    } catch {
      // 还没注入
    }
    await api.scripting.executeScript({ target: { tabId: tab.id, frameIds: [frameId] }, files: ccFiles.GROUPS.trans })
    await sleep(60)
    if (!ready(await api.tabs.sendMessage(tab.id, msg, { frameId }))) throw new Error('划词翻译没有启动（这个页面可能不允许扩展运行脚本）')
    return {}
  },
  'cc-open-transbox': async (m, sender) => (await openTransBox(sender?.tab || null, String(m.text || '').slice(0, 5000)), {}),
  // 视频摘要读到字幕后，把文稿作为字幕层叠在视频上
  'cc-sublayer-open': async (_m, sender) => (sender?.tab && (await startSubLayer(sender.tab)), {}),
  // 视频胶囊菜单里的“视频摘要”（再注入一次会关掉，所以胶囊只在没打开时显示这一项）
  'cc-vsum-open': async (_m, sender) => (sender?.tab && (await startVsum(sender.tab)), {}),
  'cc-tr-clear': async () => (await clearTransCache(), {}),
  // 把笔记存进清影的“摘录”；字数上限在设置里（0 = 不限制，默认）
  'cc-note': async (m) => {
    const { noteLimit } = await api.storage.local.get('noteLimit')
    const max = Math.max(0, Math.floor(Number(noteLimit) || 0))
    const text = String(m.text || '')
    return { pairing: await sendText(max ? text.slice(0, max) : text, { note: true }) }
  },
  // 视频画布被跨域污染时，用浏览器对可见区域的截图代替（只在用户点了截图之后）
  'cc-capture': async (_m, sender) => ({ dataUrl: await api.tabs.captureVisibleTab(sender.tab.windowId, { format: 'png' }) }),
}

api.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  const handler = msg && typeof msg.type === 'string' && Object.prototype.hasOwnProperty.call(HANDLERS, msg.type) ? HANDLERS[msg.type] : null
  if (!handler) return false
  // 入口检查（bg-guard.js）：发送者是本扩展、需要标签页的消息来自内容脚本、字段按白名单整理并限长；不合规返回统一的错误
  const g = ccGuard.check(msg, sender, api.runtime.id)
  if (!g.ok) {
    sendResponse({ ok: false, error: g.error, code: g.code })
    return false
  }
  handler(g.msg, sender)
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
  if (f.vsum) {
    add({ id: 'cc-vsum', title: '视频摘要 / 总结…', contexts: ['page', 'video'] })
    add({ id: 'cc-vsum-sel', title: '总结选中的文字…', contexts: ['selection'] })
  }
  if (f.subLayer) add({ id: 'cc-sublayer', title: '给视频加字幕层（双语 / 转写）…', contexts: ['page', 'video'] })
  if (f.webtrans) {
    add({ id: 'cc-wt', title: '翻译此页面 / 还原（Alt+A）', contexts: ['page'] })
    add({ id: 'cc-wt-sel', title: '翻译选中的文字', contexts: ['selection'] })
    add({ id: 'cc-tbox', title: '在翻译窗里翻译（多个服务对比，Alt+D）', contexts: ['selection'] })
  }
  if (f.onDemand) add({ id: 'cc-overview', title: '打开页面媒体总览', contexts: ['page'] })
  add({ id: 'cc-link', title: '发送此链接到清影', contexts: ['link'] })
  add({ id: 'cc-media', title: '发送此视频 / 音频 / 图片到清影', contexts: ['video', 'image', 'audio'] })
  if (f.readLater) {
    add({ id: 'cc-later', title: '稍后看（存进清影的待看列表）', contexts: ['page'] })
    add({ id: 'cc-later-link', title: '把此链接加入稍后看', contexts: ['link'] })
  }
  if (f.cleanLink) {
    add({ id: 'cc-copy-clean', title: '复制干净的链接', contexts: ['page'] })
    add({ id: 'cc-copy-clean-link', title: '复制干净的链接', contexts: ['link'] })
  }
  add({ id: 'cc-qr', title: '用二维码在手机上打开此页…', contexts: ['page'] })
  if (f.onDemand) {
    add({ id: 'cc-clip', title: '摘录选中文字到清影', contexts: ['selection'] })
    add({ id: 'cc-save', title: '页面保存', contexts: ['page'] })
    add({ id: 'cc-save:shotVisible', parentId: 'cc-save', title: '截图：可见区域', contexts: ['page'] })
    add({ id: 'cc-save:shotArea', parentId: 'cc-save', title: '截图：选取区域', contexts: ['page'] })
    add({ id: 'cc-save:shotFull', parentId: 'cc-save', title: '截图：整页', contexts: ['page'] })
    add({ id: 'cc-save:pdf', parentId: 'cc-save', title: '另存为 PDF', contexts: ['page'] })
    add({ id: 'cc-save:md', parentId: 'cc-save', title: '保存为 Markdown', contexts: ['page'] })
  }
  if (f.onDemand) {
    add({ id: 'cc-search', title: '以图搜图', contexts: ['image'] })
    for (const [i, e] of (await getEngines()).entries()) add({ id: `cc-search:${i}`, parentId: 'cc-search', title: e.name, contexts: ['image'] })
    add({ id: 'cc-search:all', parentId: 'cc-search', title: '全部引擎各开一个标签', contexts: ['image'] })
    add({ id: 'cc-search:upload', parentId: 'cc-search', title: '上传本地图片…', contexts: ['image'] })
  }
}

// 设置连着改几次只重建一次；待办存进存储 + alarm 兜底，后台在这 300 毫秒里被回收也会补做
function rebuildMenusSoon() {
  ccDefer.schedule('menus', 300).catch((e) => console.warn('schedule menus failed', e))
}
ccDefer.on('menus', () => buildMenus().catch((e) => console.warn('menus failed', e)))

api.contextMenus.onClicked.addListener(async (info, tab) => {
  const id = String(info.menuItemId)
  try {
    if (id === 'cc-overview') await openOverview(tab)
    else if (id === 'cc-vsum' || id === 'cc-vsum-sel') await startVsum(tab)
    else if (id === 'cc-sublayer') await startSubLayer(tab)
    else if (id === 'cc-wt') await webTransDo(tab, 'toggle')
    else if (id === 'cc-wt-sel') await webTransDo(tab, 'selection', { text: info.selectionText || '' })
    else if (id === 'cc-tbox') await openTransBox(tab, info.selectionText || '')
    else if (id === 'cc-later' || id === 'cc-later-link') {
      const url = id === 'cc-later' ? tab.url : info.linkUrl
      const r = await addLater({ url, title: id === 'cc-later' ? tab.title : info.selectionText || '', icon: id === 'cc-later' ? tab.favIconUrl : '' })
      if (r.invalid) throw new Error('这个地址不能加入稍后看')
      notify(r.added ? '已加入稍后看' : '稍后看里已经有了', '在扩展弹窗的“稍后看”标签里查看')
      await flashBadge(tab?.id, true)
    } else if (id === 'cc-copy-clean' || id === 'cc-copy-clean-link') {
      const r = await copyCleanLink(tab, id === 'cc-copy-clean' ? tab.url : info.linkUrl)
      if (r.via === 'ok') await flashBadge(tab?.id, true)
    } else if (id === 'cc-qr') await openQr(tab, tab.url)
    else if (id === 'cc-clip') {
      const r = await pageSave('clip', tab, info.selectionText)
      notify(r.pairing ? '请在电脑上允许配对' : '已摘录到清影', r.pairing ? '第一次使用需要在清影里点“允许”' : '在清影的“生词与摘录 → 摘录”里查看')
      await flashBadge(tab?.id, !r.pairing)
    } else if (id.startsWith('cc-save:')) {
      const r = await pageSave(id.slice('cc-save:'.length), tab, '')
      if (r.note) notify('页面保存', r.note)
    }
    else if (id === 'cc-search:upload') await api.tabs.create({ url: api.runtime.getURL('search.html') })
    else if (id.startsWith('cc-search:')) await searchImage(info.srcUrl, id.slice('cc-search:'.length))
  } catch (e) {
    notify('操作没有完成', e.message)
  }
})

/** 打开翻译窗：优先在当前网页上弹出可拖动的悬浮窗（选中的文字带进去）；页面不让注入脚本时才开独立页面。 */
async function openTransBox(tab, text = '') {
  const t = tab || (await api.tabs.query({ active: true, currentWindow: true }))[0]
  try {
    await webTransDo(t, 'box', { text })
    return
  } catch {
    // 内置页、PDF 查看器等：退回独立页面
  }
  await api.tabs.create({ url: api.runtime.getURL('transbox.html') })
}

api.commands?.onCommand.addListener(async (cmd) => {
  if (cmd === 'toggle-writing') {
    try {
      if (!(await getFeatures()).writing) return
      const [tab] = await api.tabs.query({ active: true, currentWindow: true })
      // 常驻脚本在每个框架里都有一份，只有有焦点的那个会响应
      if (tab?.id != null) await api.tabs.sendMessage(tab.id, { type: 'cc-write-open' })
    } catch (e) {
      notify('写作浮窗没有打开', '这个页面不支持（浏览器内置页面或刚装好的扩展需要刷新页面）')
    }
    return
  }
  if (cmd === 'toggle-webtrans') {
    try {
      if (!(await getFeatures()).webtrans) return
      const [tab] = await api.tabs.query({ active: true, currentWindow: true })
      await webTransDo(tab, 'toggle')
    } catch (e) {
      notify('网页翻译没有打开', e.message)
    }
    return
  }
  if (cmd === 'open-transbox') {
    try {
      await openTransBox(null)
    } catch (e) {
      notify('翻译窗没有打开', e.message)
    }
    return
  }
  if (cmd !== 'toggle-vsum') return
  try {
    if (!(await getFeatures()).vsum) return
    const [tab] = await api.tabs.query({ active: true, currentWindow: true })
    await startVsum(tab)
  } catch (e) {
    notify('视频摘要没有打开', e.message)
  }
})

// ---------- 扩展的用户数据同步到清影（笔记、高亮、本地导入的字幕）：进清影的备份；新装扩展时自动取回 ----------

const EXT_KEYS = ['vsumNotes', 'hlData', 'vsumLocal']
function pushExtDataSoon() {
  ccDefer.schedule('ext-push', 15000).catch((e) => console.warn('schedule ext push failed', e))
}
ccDefer.on('ext-push', async () => {
  try {
    const data = await api.storage.local.get(EXT_KEYS)
    await call('/api/v1/extdata', { data })
  } catch {
    // 清影没开：下次改动时再同步
  }
})
async function pullExtData() {
  try {
    const r = await call('/api/v1/extdata')
    const have = await api.storage.local.get(EXT_KEYS)
    const fill = {}
    for (const k of EXT_KEYS) {
      const v = r?.data?.[k]
      const empty = have[k] == null || (Array.isArray(have[k]) ? !have[k].length : typeof have[k] === 'object' && !Object.keys(have[k]).length)
      if (v != null && empty) fill[k] = v
    }
    if (Object.keys(fill).length) await api.storage.local.set(fill)
  } catch {
    // 清影没开或没配对：不取回
  }
}

api.runtime.onInstalled.addListener(async (d) => {
  if (d?.reason === 'install') pullExtData()
  // 升级上来的用户保持原来的样子：视频上的音效按钮继续显示（新安装默认不显示）
  if (d?.reason === 'update') {
    try {
      // 播放增强新增了总开关（默认关）。升级上来、自己存过功能开关的用户保持原来的样子：原来用着播放增强的继续开着
      const { features } = await api.storage.local.get('features')
      if (features && typeof features === 'object' && typeof features.videoMaster !== 'boolean') {
        const was = features.speed !== false || features.speedMemory !== false || features.seek !== false || features.videoTools === true || features.videoAdvanced === true
        await api.storage.local.set({ features: { ...features, videoMaster: was } })
      }
      // 1.18 起“解除复制 / 右键限制”默认只对开启过的网站生效。以前默认是所有网站，升级时改掉一次并提示（还能在设置里改回来）
      const { siteRules, unlockMigrated } = await api.storage.local.get(['siteRules', 'unlockMigrated'])
      if (!unlockMigrated) {
        if (!siteRules || siteRules.unlockAll !== false) {
          await api.storage.local.set({ siteRules: { ...(siteRules || {}), unlockAll: false } })
          notify('解除复制限制改为按网站开启', '以前对所有网站生效，会影响在线文档、设计工具的右键和拖放。现在只对你开启过的网站生效：在弹窗“工具”里对当前网站打开，或在设置里改回“所有网站”。')
        }
        await api.storage.local.set({ unlockMigrated: true })
      }
      const { audioCfg } = await api.storage.local.get('audioCfg')
      if (!audioCfg || typeof audioCfg.pillShow !== 'boolean') await api.storage.local.set({ audioCfg: { ...(audioCfg || {}), pillShow: true } })
    } catch {
      // 读不到存储：按默认
    }
  }
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
  if (ch.siteRules || ch.transCfg) syncResident()
  if (EXT_KEYS.some((k) => ch[k])) pushExtDataSoon()
  // 刚连上清影（填了令牌 / 配对成功）：本地还没有的笔记、高亮从清影取回
  if (ch.token?.newValue && !ch.token.oldValue) pullExtData()
})
syncResident()
