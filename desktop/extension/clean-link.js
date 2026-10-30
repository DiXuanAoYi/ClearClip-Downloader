// 链接清理：去掉追踪参数、解开跳转链接。纯函数，不依赖浏览器接口。

const TRACK_PREFIX = ['utm_', 'pk_', 'mtm_', 'hsa_']
const TRACK_EXACT = [
  'fbclid', 'gclid', 'gclsrc', 'dclid', 'gbraid', 'wbraid', 'msclkid', 'yclid', 'twclid', 'ttclid', 'li_fat_id', 'mc_eid', 'mc_cid', 'igshid', 'igsh',
  '_hsenc', '_hsmi', 'mkt_tok', 'vero_id', 'ref_src', 'ref_url', 'spm', 'scm', 'share_source', 'share_medium', 'share_plat', 'share_session_id',
  'share_tag', 'share_from', 'from_source',
]

/** 只对某个网站有效的参数（站点本身和它的子域名）。 */
const SITE_PARAMS = [
  { sites: ['bilibili.com', 'b23.tv'], params: ['spm_id_from', 'from_spmid', 'vd_source', 'timestamp', 'unique_k', 'bbid', 'is_story_h5', 'buvid', 'mid', 'plat_id', 'up_id', '-Arouter', 'share_source', 'share_medium', 'share_plat', 'share_session_id', 'share_tag'] },
  { sites: ['youtube.com', 'youtu.be', 'youtube-nocookie.com'], params: ['si', 'feature', 'pp', 'ab_channel'] },
  { sites: ['twitter.com', 'x.com'], params: ['s', 't'] },
  { sites: ['taobao.com', 'tmall.com', 'jd.com', '1688.com'], params: ['pvid', 'utparam', 'ali_refid', 'ali_trackid', 'bxsign', 'sourceType', 'suid', 'ut_sk', 'un', 'share_crt_v', 'sp_tk', 'cpp', 'shareurl', 'short_name', 'app', 'wxsign', 'tbSocialPopKey', 'sourceid', 'abbucket', 'abtest'] },
  { sites: ['xiaohongshu.com', 'xhslink.com'], params: ['xhsshare', 'appuid', 'apptime', 'share_id', 'shareRedId', 'share_from_user_hidden'] },
  { sites: ['amazon.com', 'amazon.co.uk', 'amazon.de', 'amazon.co.jp', 'amazon.cn', 'amazon.ca', 'amazon.fr'], params: ['ref', 'ref_', 'qid', 'sr', 'keywords', 'crid', 'sprefix', 'linkCode', 'tag', 'content-id', 'dib', 'dib_tag', '_encoding', 'psc', 'th'], prefix: ['pf_rd_', 'pd_rd_'] },
  { sites: ['zhihu.com'], params: ['utm_psn', 'hybrid', 'utm_oi'] },
  { sites: ['weibo.com', 'weibo.cn'], params: ['refer_flag', 'wm', 'luicode', 'lfid', 'featurecode', 'rl'] },
  { sites: ['facebook.com', 'fb.com', 'instagram.com'], params: ['mibextid', 'sfnsn', 'rdid', 'share_url', '__tn__', '__cft__', '__xts__'] },
]

/** 跳转中转页：参数里放着真实地址。 */
const REDIRECTORS = [
  { host: 'link.zhihu.com', params: ['target'] },
  { host: 'www.google.com', path: '/url', params: ['q', 'url'] },
  { host: 'l.facebook.com', path: '/l.php', params: ['u'] },
  { host: 'weibo.cn', path: '/sinaurl', params: ['u', 'url'] },
  { host: 'c.pc.qq.com', params: ['pfurl', 'url'] },
  { host: 'link.juejin.cn', params: ['target'] },
  { host: 'steamcommunity.com', path: '/linkfilter/', params: ['url', 'u'] },
  { host: 'www.youtube.com', path: '/redirect', params: ['q'] },
  { host: 'out.reddit.com', params: ['url'] },
  { host: 'www.jianshu.com', path: '/go-wild', params: ['url'] },
  { host: 't.me', path: '/iv', params: ['url'] },
]

function hostMatches(host, site) {
  return host === site || host.endsWith('.' + site)
}

function safeDecode(s) {
  try {
    return decodeURIComponent(s.replace(/\+/g, ' '))
  } catch {
    return s
  }
}

/** 如果是跳转中转页，返回真实地址，否则返回 null。 */
function unwrapRedirect(u) {
  const host = u.hostname.toLowerCase()
  for (const r of REDIRECTORS) {
    if (host !== r.host || (r.path && !u.pathname.startsWith(r.path))) continue
    for (const p of r.params) {
      const v = u.searchParams.get(p)
      if (v && /^https?:\/\//i.test(v)) return v
    }
  }
  return null
}

/**
 * 清理一个链接。返回 { url, removed, unwrapped }：removed 是去掉的参数名。
 * 保持剩下参数的原始写法和顺序，不重新编码。extra：用户自己加的参数名。
 */
function cleanUrl(input, extra = []) {
  const raw = String(input || '').trim()
  let u
  try {
    u = new URL(raw)
  } catch {
    return { url: raw, removed: [], unwrapped: false }
  }
  if (!/^https?:$/.test(u.protocol)) return { url: raw, removed: [], unwrapped: false }
  let unwrapped = false
  for (let i = 0; i < 3; i++) {
    const inner = unwrapRedirect(u)
    if (!inner) break
    try {
      u = new URL(inner)
      unwrapped = true
    } catch {
      break
    }
  }
  const host = u.hostname.toLowerCase()
  const names = new Set([...TRACK_EXACT, ...extra.map((x) => String(x).trim()).filter(Boolean)])
  const prefixes = [...TRACK_PREFIX]
  for (const s of SITE_PARAMS) {
    if (!s.sites.some((x) => hostMatches(host, x))) continue
    s.params.forEach((p) => names.add(p))
    prefixes.push(...(s.prefix || []))
  }
  const removed = []
  if (u.search.length > 1) {
    const kept = []
    for (const pair of u.search.slice(1).split('&')) {
      if (!pair) continue
      const key = safeDecode(pair.split('=')[0])
      if (names.has(key) || prefixes.some((p) => key.startsWith(p))) removed.push(key)
      else kept.push(pair)
    }
    u.search = kept.length ? '?' + kept.join('&') : ''
  }
  // 亚马逊路径里的 /ref=xxx
  if (/(^|\.)amazon\./.test(host)) {
    const p = u.pathname.replace(/\/ref=[^/]*/, '')
    if (p !== u.pathname) {
      removed.push('ref')
      u.pathname = p
    }
  }
  const url = u.href
  return { url, removed: [...new Set(removed)], unwrapped }
}
