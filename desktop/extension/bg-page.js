// 页面功能的后台部分：常驻内容脚本的注册、按需注入、内容脚本和扩展页面发来的消息、右键菜单。

const CONTENT_MATCHES = ['http://*/*', 'https://*/*']
let residentLock = Promise.resolve()
const STUB_SCRIPT = { id: 'cc-stub', get js() { return ccFiles.GROUPS.stub } }

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
  const ctx = { takeover: await getTakeover() }
  // 编号 → 要匹配的网址；匹配范围为空的（比如解除限制的名单是空的）不注册
  const want = new Map()
  let lazyNeeded = false
  for (const s of RESIDENT_SCRIPTS) {
    if (!s.need(normalizeFeatures(features), ctx)) continue
    if (s.lazy) {
      lazyNeeded = true // 视频类：不直接注册，由下面的探测 stub 按需加载
      continue
    }
    const matches = s.matches ? s.matches(rules, trans) : CONTENT_MATCHES
    // 网页访问是可选权限：没授权的范围不注册（否则注册会报错），授权后 permissions.onAdded 会再同步一次
    if (matches.length && (await ccPerms.has({ origins: matches }))) want.set(s.id, matches)
  }
  if (lazyNeeded && (await ccPerms.has({ origins: CONTENT_MATCHES }))) want.set('cc-stub', CONTENT_MATCHES)
  const have = (await sc.getRegisteredContentScripts()).filter((s) => s.id.startsWith('cc-'))
  const same = (a, b) => a.length === b.length && a.every((x) => b.includes(x))
  const drop = have.filter((s) => reset || !want.has(s.id) || !same(s.matches || [], want.get(s.id))).map((s) => s.id)
  if (drop.length) await sc.unregisterContentScripts({ ids: drop })
  const keep = new Set(have.map((s) => s.id).filter((id) => !drop.includes(id)))
  for (const s of [...RESIDENT_SCRIPTS, STUB_SCRIPT]) {
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
