// 接管浏览器下载：判断哪些下载交给清影。这里只放不依赖浏览器接口的纯函数，方便单元测试。

const TAKEOVER_TYPES = [
  // 压缩包
  'zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz', 'zst',
  // 安装包、镜像
  'exe', 'msi', 'dmg', 'pkg', 'deb', 'rpm', 'appimage', 'apk', 'ipa', 'iso', 'img',
  // 音视频
  'mp4', 'mkv', 'avi', 'mov', 'webm', 'flv', 'wmv', 'm4v', 'ts', 'mp3', 'flac', 'm4a', 'wav', 'aac', 'ogg', 'opus',
]

const DEFAULT_TAKEOVER = {
  enabled: false,
  // types：只接管列表里的类型；all：所有下载（仍受大小和站点限制）
  mode: 'types',
  types: TAKEOVER_TYPES,
  minSizeMB: 1,
  allowSites: [],
  denySites: [],
  notify: true,
}

/** 把存储里的原始值整理成完整、合法的设置。 */
function normalizeTakeover(raw) {
  const r = raw && typeof raw === 'object' ? raw : {}
  const list = (v, f) => [...new Set((Array.isArray(v) ? v : []).map(f).filter(Boolean))]
  const ext = (x) => String(x).trim().toLowerCase().replace(/^[*.\s]+/, '').replace(/[^a-z0-9]/g, '')
  const types = list(r.types, ext)
  const size = Number(r.minSizeMB)
  return {
    enabled: r.enabled === true,
    mode: r.mode === 'all' ? 'all' : 'types',
    types: types.length ? types : TAKEOVER_TYPES.slice(),
    minSizeMB: Number.isFinite(size) && size >= 0 ? Math.min(size, 100000) : DEFAULT_TAKEOVER.minSizeMB,
    allowSites: list(r.allowSites, normalizeSite),
    denySites: list(r.denySites, normalizeSite),
    notify: r.notify !== false,
  }
}

/** 站点写法统一成小写主机名：去掉协议、路径、端口和开头的 `*.`。 */
function normalizeSite(s) {
  let v = String(s || '').trim().toLowerCase()
  if (!v) return ''
  v = v.replace(/^[a-z][a-z0-9+.-]*:\/\//, '').replace(/^\*\./, '').replace(/^\./, '')
  v = v.split(/[/?#]/)[0].replace(/:\d+$/, '')
  return /^[a-z0-9.-]+$|^\[[0-9a-f:]+\]$/.test(v) ? v : ''
}

/** 主机名是否属于这个站点（站点本身或它的子域名）。 */
function hostInSite(host, site) {
  const h = String(host || '').toLowerCase()
  return !!site && (h === site || h.endsWith('.' + site))
}

function hostOf(url) {
  try {
    return new URL(url).hostname
  } catch {
    return ''
  }
}

/** 文件扩展名（小写，不含点）。优先用文件名，其次用地址路径。 */
function fileExt(filename, url) {
  const fromName = (n) => {
    const base = String(n || '').split(/[\\/]/).pop()
    const i = base.lastIndexOf('.')
    return i > 0 && i < base.length - 1 ? base.slice(i + 1).toLowerCase() : ''
  }
  const e = fromName(filename)
  if (e) return e
  try {
    return fromName(decodeURIComponent(new URL(url).pathname))
  } catch {
    return ''
  }
}

const GENERIC_MIME = /^(application\/(octet-stream|zip|x-zip-compressed|x-rar-compressed|vnd\.rar|x-7z-compressed|gzip|x-gzip|x-tar|x-bzip2|x-xz|x-msdownload|x-msi|x-apple-diskimage|x-iso9660-image|vnd\.android\.package-archive|x-debian-package|x-rpm)|binary\/octet-stream)$/

/**
 * 这个下载要不要交给清影。item：{ url, finalUrl, filename, mime, fileSize, referrer, byExtension }
 * 返回 { take, reason }，reason 只用于调试和界面提示。
 */
function shouldTake(cfg, item) {
  if (!cfg || !cfg.enabled) return { take: false, reason: 'disabled' }
  if (item.byExtension) return { take: false, reason: 'other-extension' }
  const url = item.finalUrl || item.url || ''
  if (!/^https?:\/\//i.test(url)) return { take: false, reason: 'scheme' }
  const hosts = [hostOf(item.url), hostOf(url), hostOf(item.referrer)].filter(Boolean)
  if (cfg.denySites.some((s) => hosts.some((h) => hostInSite(h, s)))) return { take: false, reason: 'denied-site' }
  if (cfg.allowSites.length && !cfg.allowSites.some((s) => hosts.some((h) => hostInSite(h, s)))) return { take: false, reason: 'not-allowed-site' }
  const size = Number(item.fileSize) > 0 ? Number(item.fileSize) : 0
  if (size && size < cfg.minSizeMB * 1048576) return { take: false, reason: 'too-small' }
  if (cfg.mode === 'all') return { take: true, reason: 'all' }
  const ext = fileExt(item.filename, url)
  if (ext) return cfg.types.includes(ext) ? { take: true, reason: 'type' } : { take: false, reason: 'type' }
  // 没有扩展名：只在内容类型是通用二进制 / 压缩包，且大小已知并达到下限时接管
  const mime = String(item.mime || '').split(';')[0].trim().toLowerCase()
  if (GENERIC_MIME.test(mime) && size) return { take: true, reason: 'mime' }
  return { take: false, reason: 'type' }
}

/** 发给清影 /api/grab 的内容。 */
function grabPayload(item, cookie, userAgent, deviceName) {
  return {
    url: item.finalUrl || item.url,
    referer: item.referrer || '',
    cookie: cookie || '',
    filename: String(item.filename || '').split(/[\\/]/).pop(),
    size: Number(item.fileSize) > 0 ? Number(item.fileSize) : null,
    mime: item.mime || '',
    userAgent: userAgent || '',
    deviceName: deviceName || '',
  }
}

/** Cookie 列表拼成请求头；只带适用于这个地址的（浏览器已按地址筛过）。 */
function cookieHeader(cookies) {
  return (cookies || []).map((c) => `${c.name}=${c.value}`).join('; ')
}

/** 地址里去掉查询串和片段，用于记录和提示（查询串里常有一次性令牌）。 */
function shortUrl(url) {
  try {
    const u = new URL(url)
    if (u.protocol === 'blob:') return String(url).slice(0, 200).split(/[?#]/)[0]
    return u.origin + u.pathname
  } catch {
    return String(url || '').slice(0, 200)
  }
}

const MEDIA_EXTS = ['mp4', 'mkv', 'avi', 'mov', 'webm', 'flv', 'wmv', 'm4v', 'ts', 'mp3', 'flac', 'm4a', 'wav', 'aac', 'ogg', 'opus', 'm3u8']
const DOC_EXTS = ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'epub', 'csv', 'txt']

/** 右键菜单里的链接：指向压缩包 / 安装包 / 文档这类普通文件的直接下载；其余（网页、音视频）交给清影解析。 */
function linkAction(url) {
  const ext = fileExt('', url)
  if (!ext || MEDIA_EXTS.includes(ext)) return 'send'
  return TAKEOVER_TYPES.includes(ext) || DOC_EXTS.includes(ext) ? 'grab' : 'send'
}
