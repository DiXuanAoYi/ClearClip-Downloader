// 视频嗅探：记录网页加载过程中出现的视频流 / 视频文件 / 音频 / 图片，在弹窗里排序、批量发送给清影。
// 解析函数（classifyMedia、parseM3u8、parseMpd 等）不依赖浏览器接口，可以单元测试。

/** 嗅探记录放在 storage.session（浏览器重启后清空），没有时退回 local。 */
const sniffStore = api.storage.session || api.storage.local
const sniffKey = (tabId) => `sniff:${tabId}`
const SEGMENT_RE = /\.(ts|m4s|aac|cmfv|cmfa|vtt|srt|jpg|jpeg|png|webp|gif|avif)(\?|#|$)/i
const IMAGE_RE = /\.(jpe?g|png|webp|avif|gif)(\?|#|$)/i
const MIN_FILE = 300 * 1024
const MIN_IMAGE = 60 * 1024
const MAX_MEDIA = 30
const MAX_IMAGES = 60

/** 判断一个响应是不是值得记录的媒体。返回 { kind, size } 或 null。kind：hls / dash / video / audio / image */
function classifyMedia(url, contentType, contentLength, contentRange) {
  const ct = (contentType || '').toLowerCase()
  let size = Number(contentLength) || 0
  const m = /\/(\d+)\s*$/.exec(contentRange || '')
  if (m) size = Number(m[1])
  if (/mpegurl/.test(ct) || /\.m3u8(\?|#|$)/i.test(url)) return { kind: 'hls', size: 0 }
  if (/dash\+xml/.test(ct) || /\.mpd(\?|#|$)/i.test(url)) return { kind: 'dash', size: 0 }
  // 图片：单独一类，太小的（图标、表情、缩略图）不记录，没有大小的也不记录
  if (/^image\//.test(ct) || (!ct && IMAGE_RE.test(url))) {
    if (/svg|x-icon|vnd\.microsoft\.icon/.test(ct)) return null
    return size >= MIN_IMAGE ? { kind: 'image', size } : null
  }
  if (/^video\/mp2t/.test(ct) || SEGMENT_RE.test(url)) return null
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

/** 加入一条记录（同一地址去重，新的覆盖旧的但保留已解析出的详情）。媒体和图片各自限制数量。返回媒体条数（徽标上显示）。 */
async function addSniffed(tabId, item) {
  const all = await getSniffed(tabId)
  const old = all.find((x) => sniffId(x.url) === sniffId(item.url))
  const merged = old ? { ...old, ...item, detail: old.detail } : item
  const rest = all.filter((x) => sniffId(x.url) !== sniffId(item.url))
  const isImg = (x) => x.kind === 'image'
  const media = [merged, ...rest].filter((x) => !isImg(x)).slice(0, MAX_MEDIA)
  const imgs = [merged, ...rest].filter(isImg).slice(0, MAX_IMAGES)
  await sniffStore.set({ [sniffKey(tabId)]: [...media, ...imgs].sort((a, b) => b.at - a.at) })
  return media.length
}

async function setSniffDetail(tabId, url, detail) {
  const all = await getSniffed(tabId)
  const it = all.find((x) => sniffId(x.url) === sniffId(url))
  if (!it) return
  it.detail = detail
  await sniffStore.set({ [sniffKey(tabId)]: all })
}

/** 发送嗅探到的地址：带上所在网页（作为 Referer）和标题。 */
function sniffedPayload(item, pageUrl, title) {
  const enc = encodeURIComponent
  const sep = item.url.includes('#') ? '&' : '#'
  return `${item.url}${sep}cc-referer=${enc(pageUrl || '')}&cc-title=${enc((title || '').slice(0, 120))}`
}

const KIND_NAME = { hls: '视频流 (m3u8)', dash: '视频流 (mpd)', video: '视频文件', audio: '音频文件', image: '图片' }

// ---------- 清单解析：时长、清晰度、预估大小、是否加密 ----------

function absUrl(ref, base) {
  try {
    return new URL(ref, base).href
  } catch {
    return ref
  }
}

function attrOf(line, name) {
  const m = new RegExp(`(?:^|[:,])${name}=("([^"]*)"|[^,]*)`, 'i').exec(line)
  return m ? (m[2] !== undefined ? m[2] : m[1]) : ''
}

/**
 * 解析 m3u8。主清单返回 variants（按带宽从高到低）；媒体清单返回时长、是否直播、是否加密（DRM）。
 * AES-128 加密清影能解，只有 SAMPLE-AES 和各家 DRM 的密钥格式算 DRM。
 */
function parseM3u8(text, base) {
  const lines = String(text || '').split(/\r?\n/).map((l) => l.trim())
  if (!lines.some((l) => l.startsWith('#EXTM3U'))) return null
  const out = { master: false, variants: [], durationMs: 0, live: false, drm: false }
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i]
    if (l.startsWith('#EXT-X-STREAM-INF')) {
      const uri = lines.slice(i + 1).find((x) => x && !x.startsWith('#'))
      if (!uri) continue
      const res = /^(\d+)x(\d+)$/.exec(attrOf(l, 'RESOLUTION'))
      out.variants.push({
        url: absUrl(uri, base),
        bandwidth: Number(attrOf(l, 'BANDWIDTH')) || Number(attrOf(l, 'AVERAGE-BANDWIDTH')) || 0,
        width: res ? Number(res[1]) : 0,
        height: res ? Number(res[2]) : 0,
        codecs: attrOf(l, 'CODECS'),
      })
      out.master = true
    } else if (l.startsWith('#EXTINF:')) {
      out.durationMs += Math.round((parseFloat(l.slice(8)) || 0) * 1000)
    } else if (l.startsWith('#EXT-X-KEY')) {
      const method = attrOf(l, 'METHOD').toUpperCase()
      const fmt = attrOf(l, 'KEYFORMAT').toLowerCase()
      if (method === 'SAMPLE-AES' || method === 'SAMPLE-AES-CTR' || /streamingkeydelivery|edef8ba9|playready|widevine/.test(fmt)) out.drm = true
    }
  }
  out.live = !out.master && !lines.includes('#EXT-X-ENDLIST')
  out.variants.sort((a, b) => b.height - a.height || b.bandwidth - a.bandwidth)
  return out
}

/** ISO 8601 时长（PT1H2M3.5S）转毫秒。 */
function isoDurationMs(s) {
  const m = /^P(?:(\d+)D)?T?(?:(\d+(?:\.\d+)?)H)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)S)?$/i.exec(String(s || '').trim())
  if (!m) return 0
  const [, d, h, mi, se] = m
  return Math.round(((Number(d || 0) * 24 + Number(h || 0)) * 3600 + Number(mi || 0) * 60 + Number(se || 0)) * 1000)
}

/** 解析 mpd（用正则，扩展的后台脚本里没有 DOMParser）。 */
function parseMpd(text) {
  const t = String(text || '')
  if (!/<MPD[\s>]/.test(t)) return null
  const dur = /mediaPresentationDuration="([^"]+)"/.exec(t)
  const live = /<MPD[^>]*\btype="dynamic"/.test(t)
  const drm = /<ContentProtection\b/.test(t)
  const sets = t.match(/<AdaptationSet[\s\S]*?<\/AdaptationSet>/g) || []
  let bandwidth = 0
  let width = 0
  let height = 0
  for (const s of sets) {
    const bws = [...s.matchAll(/\bbandwidth="(\d+)"/g)].map((m) => Number(m[1]))
    bandwidth += bws.length ? Math.max(...bws) : 0
    for (const m of s.matchAll(/\bheight="(\d+)"/g)) height = Math.max(height, Number(m[1]))
    for (const m of s.matchAll(/\bwidth="(\d+)"/g)) width = Math.max(width, Number(m[1]))
  }
  return { durationMs: dur ? isoDurationMs(dur[1]) : 0, live, drm, bandwidth, width, height }
}

/** 预估大小（字节）：码率（比特/秒）× 时长。 */
function estimateSize(bandwidth, durationMs) {
  return bandwidth > 0 && durationMs > 0 ? Math.round((bandwidth * durationMs) / 8000) : 0
}

/**
 * 把清单整理成列表里显示的详情：{ durationMs, width, height, size, live, drm, variantUrls }
 * fetchText(url) 取回文本；主清单会再取最清晰的一路来得到时长。取不到的部分留空。
 */
async function describeManifest(kind, url, fetchText) {
  const text = await fetchText(url)
  if (kind === 'dash') {
    const m = parseMpd(text)
    if (!m) return null
    return { durationMs: m.durationMs, width: m.width, height: m.height, size: m.live ? 0 : estimateSize(m.bandwidth, m.durationMs), live: m.live, drm: m.drm, variantUrls: [] }
  }
  const m = parseM3u8(text, url)
  if (!m) return null
  if (!m.master) return { durationMs: m.live ? 0 : m.durationMs, width: 0, height: 0, size: 0, live: m.live, drm: m.drm, variantUrls: [] }
  const best = m.variants[0]
  const detail = { durationMs: 0, width: best.width, height: best.height, size: 0, live: false, drm: false, variantUrls: m.variants.map((v) => v.url) }
  try {
    const sub = parseM3u8(await fetchText(best.url), best.url)
    if (sub) {
      detail.live = sub.live
      detail.drm = sub.drm
      detail.durationMs = sub.live ? 0 : sub.durationMs
      detail.size = sub.live ? 0 : estimateSize(best.bandwidth, sub.durationMs)
    }
  } catch {
    // 子清单取不到：只显示清晰度
  }
  return detail
}

// ---------- 列表整理 ----------

const KIND_ORDER = { hls: 0, dash: 0, video: 0, audio: 1, image: 2 }

/** 去重并排序：主清单里列出的子清单不再单独显示；视频按清晰度、大小从高到低，音频其次，图片最后。 */
function arrangeSniffed(list) {
  const hidden = new Set()
  for (const x of list) for (const v of x.detail?.variantUrls || []) hidden.add(sniffId(v))
  const seen = new Set()
  const rows = []
  for (const x of list) {
    const id = sniffId(x.url)
    if (hidden.has(id) || seen.has(id)) continue
    seen.add(id)
    rows.push(x)
  }
  const h = (x) => x.detail?.height || 0
  const sz = (x) => x.detail?.size || x.size || 0
  return rows.sort((a, b) => (KIND_ORDER[a.kind] ?? 9) - (KIND_ORDER[b.kind] ?? 9) || h(b) - h(a) || sz(b) - sz(a) || b.at - a.at)
}

function fmtSize(n) {
  if (!n) return ''
  if (n >= 1073741824) return `${(n / 1073741824).toFixed(2)} GB`
  return n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`
}

function fmtDuration(ms) {
  if (!ms) return ''
  const s = Math.round(ms / 1000)
  const p = (n) => String(n).padStart(2, '0')
  return s >= 3600 ? `${Math.floor(s / 3600)}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}` : `${Math.floor(s / 60)}:${p(s % 60)}`
}

/** 列表里一行的说明：类型 · 清晰度 · 时长 · 大小（带 ≈ 的是预估）。 */
function sniffLabel(item) {
  const d = item.detail || {}
  const parts = [KIND_NAME[item.kind] || item.kind]
  if (d.height) parts.push(`${d.height}p`)
  if (d.live) parts.push('直播')
  else if (d.durationMs) parts.push(fmtDuration(d.durationMs))
  const estimated = !item.size && d.size
  const size = fmtSize(item.size || d.size)
  if (size) parts.push(estimated ? `≈ ${size}` : size)
  if (d.drm) parts.push('DRM 加密')
  return parts.join(' · ')
}
