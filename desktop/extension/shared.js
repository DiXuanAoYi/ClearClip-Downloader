// 与清影桌面端通信：桌面端在“设置 → 手机与浏览器扩展”开启后，在本机端口提供接口。
const api = globalThis.browser ?? globalThis.chrome

async function getConfig() {
  const c = await api.storage.local.get(['port', 'token', 'device', 'autoSync', 'syncSites', 'lastSync'])
  c.syncSites = c.syncSites || []
  c.lastSync = c.lastSync || {}
  if (!c.device) {
    c.device = 'ext-' + Math.random().toString(36).slice(2, 12)
    await api.storage.local.set({ device: c.device })
  }
  return c
}

async function call(path, body) {
  const c = await getConfig()
  if (!c.port || !c.token) throw new Error('请先在扩展设置里填写清影的端口和访问令牌')
  let r
  try {
    r = await fetch(`http://127.0.0.1:${c.port}${path}`, {
      method: body ? 'POST' : 'GET',
      headers: { 'Content-Type': 'application/json', 'X-Token': c.token, 'X-Device': c.device },
      body: body ? JSON.stringify(body) : undefined,
    })
  } catch {
    throw new Error('连接不到清影。请确认清影正在运行，并已开启“手机与浏览器扩展”。')
  }
  const j = await r.json().catch(() => ({}))
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`)
  return j
}

async function sendUrl(url) {
  const c = await getConfig()
  return call('/api/send', { text: url, device: c.device, name: browserName() })
}

function browserName() {
  const ua = navigator.userAgent
  if (/Edg\//.test(ua)) return 'Edge 浏览器扩展'
  if (/Firefox\//.test(ua)) return 'Firefox 浏览器扩展'
  return 'Chrome 浏览器扩展'
}

function registrable(host) {
  if (/^[\d.]+$/.test(host) || host.includes(':')) return host
  const parts = host.split('.').filter(Boolean)
  if (parts.length <= 2) return parts.join('.')
  const n = parts.length
  const two = parts[n - 1].length === 2 && ['com', 'net', 'org', 'gov', 'edu', 'co', 'ac'].includes(parts[n - 2])
  return parts.slice(two ? -3 : -2).join('.')
}

async function syncCookies(pageUrl, label, auto = false) {
  const u = new URL(pageUrl)
  const site = registrable(u.hostname)
  const cookies = await api.cookies.getAll({ domain: site })
  if (!cookies.length) throw new Error('这个网站没有 Cookie，请先在浏览器里登录')
  const r = await call('/api/cookies', { url: pageUrl, label: label || '', auto, cookies: cookies.map((c) => ({ name: c.name, value: c.value, domain: c.domain, path: c.path, expirationDate: c.expirationDate ?? null, secure: c.secure, httpOnly: c.httpOnly, hostOnly: c.hostOnly })) })
  const { lastSync = {} } = await api.storage.local.get('lastSync')
  lastSync[site] = Date.now()
  await api.storage.local.set({ lastSync })
  return r
}

/** 检查清影是否在运行、设置是否正确。返回 { ok, message } */
async function checkConnection() {
  const c = await getConfig()
  if (!c.port || !c.token) return { ok: false, message: '还没有设置端口和访问令牌' }
  try {
    const r = await call('/api/ping')
    return { ok: true, message: `已连接清影 ${r.version}` }
  } catch (e) {
    return { ok: false, message: e.message }
  }
}

function timeAgo(ms) {
  const d = Math.floor((Date.now() - ms) / 1000)
  if (d < 60) return '刚刚'
  if (d < 3600) return `${Math.floor(d / 60)} 分钟前`
  if (d < 86400) return `${Math.floor(d / 3600)} 小时前`
  return `${Math.floor(d / 86400)} 天前`
}

// ---------- 视频嗅探：记录网页加载过程中出现的视频流 / 视频文件，在弹窗里一键发送 ----------

/** 嗅探记录放在 storage.session（浏览器重启后清空），没有时退回 local。 */
const sniffStore = api.storage.session || api.storage.local
const sniffKey = (tabId) => `sniff:${tabId}`
const SEGMENT_RE = /\.(ts|m4s|aac|cmfv|cmfa|vtt|srt|jpg|png|webp|gif)(\?|#|$)/i
const MIN_FILE = 300 * 1024

/** 判断一个响应是不是值得记录的媒体。返回 { kind, size } 或 null。 */
function classifyMedia(url, contentType, contentLength, contentRange) {
  const ct = (contentType || '').toLowerCase()
  if (/^video\/mp2t/.test(ct) || SEGMENT_RE.test(url)) return null
  let size = Number(contentLength) || 0
  const m = /\/(\d+)\s*$/.exec(contentRange || '')
  if (m) size = Number(m[1])
  if (/mpegurl/.test(ct) || /\.m3u8(\?|#|$)/i.test(url)) return { kind: 'hls', size: 0 }
  const video = /^video\//.test(ct) || /\.(mp4|webm|flv|mov|m4v)(\?|#|$)/i.test(url)
  const audio = /^audio\//.test(ct) || /\.(m4a|mp3|ogg|opus|flac|wav)(\?|#|$)/i.test(url)
  if (!video && !audio) return null
  // 太小的多半是广告、预览或探测请求
  if (size && size < MIN_FILE) return null
  return { kind: video ? 'video' : 'audio', size }
}

function sniffId(url) {
  try {
    const u = new URL(url)
    return u.origin + u.pathname
  } catch {
    return url
  }
}

async function getSniffed(tabId) {
  const r = await sniffStore.get(sniffKey(tabId))
  return r[sniffKey(tabId)] || []
}

async function addSniffed(tabId, item) {
  const list = (await getSniffed(tabId)).filter((x) => sniffId(x.url) !== sniffId(item.url))
  list.unshift(item)
  list.length = Math.min(list.length, 30)
  await sniffStore.set({ [sniffKey(tabId)]: list })
  return list.length
}

/** 发送嗅探到的地址：带上所在网页（作为 Referer）和标题。 */
function sniffedPayload(item, pageUrl, title) {
  const enc = encodeURIComponent
  const sep = item.url.includes('#') ? '&' : '#'
  return `${item.url}${sep}cc-referer=${enc(pageUrl || '')}&cc-title=${enc((title || '').slice(0, 120))}`
}

const KIND_NAME = { hls: '视频流 (m3u8)', video: '视频文件', audio: '音频文件' }
