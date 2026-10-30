// 取原图：从缩略图地址推出更大的图片地址。三层线索：页面自带的（srcset、data-original、外层链接）→ 通用后缀 / 参数规则 → 少量站点规则 + 用户自己的规则。
// 只产生“候选地址”，由悬停放大逐个试着加载，第一个真的更大的就用它；找不到就显示图片本身。
;(() => {
  const EXT = '(?:jpe?g|png|gif|webp|avif|bmp)'
  const rule = (re, to) => ({ re, to })

  // 站点规则放前面，通用规则在后。to 可以是字符串（$1 等）、函数，或多个候选的数组。
  const BUILTIN = [
    rule(/^(https?:\/\/[^/]*(?:googleusercontent|ggpht)\.com\/.+?)=[swh]\d[^/?#]*$/i, '$1=s0'),
    rule(/^(https?:\/\/i\.ytimg\.com\/vi(?:_webp)?\/[^/]+\/)(?:default|mqdefault|hqdefault|sddefault)(\.\w+)(?:\?.*)?$/i, '$1maxresdefault$2'),
    rule(/^(https?:\/\/pbs\.twimg\.com\/media\/[^?#]+\?[^#]*?\bname=)[a-z0-9_]+/i, '$1orig'),
    rule(/^(https?:\/\/pbs\.twimg\.com\/media\/[^?#:]+):(?:thumb|small|medium|large)$/i, '$1:orig'),
    rule(/^(https?:\/\/i\.pinimg\.com)\/(?:\d+x\d*|\d+x)\//i, '$1/originals/'),
    rule(/^(https?:\/\/[^/]*sinaimg\.cn)\/(?:thumb\d+|orj\d+|mw\d+|bmiddle|small|square|thumbnail|wap\d+|crop\.[^/]+)\//i, '$1/large/'),
    rule(/^(https?:\/\/upload\.wikimedia\.org\/wikipedia\/[^/]+)\/thumb\/(\w\/\w\w\/[^/]+)\/[^/]+$/i, '$1/$2'),
    rule(/^(https?:\/\/i\.imgur\.com\/\w{5,8})[sbtmlh](\.\w+)(\?.*)?$/i, '$1$2'),
    rule(/(doubanio\.com\/view\/photo\/)(?:s_ratio_poster|m_ratio_poster|sqxs|s|m)\//i, '$1l/'),
    rule(/^(https?:\/\/pic\d*\.zhimg\.com\/[^_?]+?)(?:_[a-z0-9]+)?(\.(?:jpg|png|webp|gif))(\?.*)?$/i, '$1_r$2'),
    rule(/^https?:\/\/i\.pximg\.net\/(?:c\/[^/]+\/)?(?:img-master|custom-thumb)\/(.+?)_(?:master|square|custom)\d+\.jpg$/i, (m, p) => [`https://i.pximg.net/img-original/${p}.jpg`, `https://i.pximg.net/img-original/${p}.png`]),
    rule(/^(https?:\/\/\d+\.media\.tumblr\.com\/.*tumblr_\w+_)\d{2,4}(\.\w+)$/i, '$11280$2'),
    rule(/^(https?:\/\/res\.cloudinary\.com\/[^/]+\/image\/upload\/)(?:[a-z]{1,3}_[^/]+\/)+/i, '$1'),
    rule(/^(https?:\/\/(?:images-na\.ssl-images-amazon\.com|m\.media-amazon\.com)\/images\/I\/[^._/]+)\._[^/]*_\.(\w+)$/i, '$1.$2'),
    rule(/^https?:\/\/preview\.redd\.it\/([^?#]+)\?.*$/i, 'https://i.redd.it/$1'),
    rule(/^(https?:\/\/imgsrc\.baidu\.com)\/forum\/(?:w%3D\d+|wh%3D[^/]+|pic\/item)\/(?:sign=[^/]+\/)?([\w.]+)$/i, '$1/forum/pic/item/$2'),
  ]

  const GENERIC = [
    // 阿里云 OSS / 七牛 / 腾讯云 COS 的处理参数、B 站 @ 后缀、又拍云 ! 后缀
    rule(/([?&])x-oss-process=[^&#]*&?/i, '$1'),
    rule(/\?(?:imageView2|imageMogr2|imageslim|watermark)\b[^#]*$/i, ''),
    rule(new RegExp(`(\\.${EXT})@[^/?#]*$`, 'i'), '$1'),
    rule(new RegExp(`(\\.${EXT})![\\w.-]+$`, 'i'), '$1'),
    // 淘宝系：xxx.jpg_300x300.jpg、xxx.jpg_.webp、xxx.jpg_Q90.jpg
    rule(new RegExp(`(\\.${EXT})(?:_(?:\\d+x\\d+|[qQ]\\d+)[a-zA-Z0-9]*)*(?:_?\\.(?:jpe?g|png|webp|avif))$`, 'i'), '$1'),
    // WordPress / Shopify：name-300x200.jpg、name_300x.jpg
    rule(new RegExp(`[-_]\\d{2,4}x\\d{0,4}(\\.${EXT})(?=$|[?#])`, 'i'), '$1'),
    // name_thumb.jpg、name-small.jpg
    rule(new RegExp(`[-_](?:thumb|thumbnail|small|medium|mini|tiny|preview|sm|xs)(\\.${EXT})(?=$|[?#])`, 'i'), '$1'),
    // name.jpg.thumb.jpg、name.jpg.200x200.jpg
    rule(new RegExp(`(\\.${EXT})\\.(?:thumb|small|preview|\\d+x\\d+)\\.${EXT}(?=$|[?#])`, 'i'), '$1'),
  ]

  const SIZE_PARAMS = ['w', 'h', 'width', 'height', 'resize', 'fit', 'crop', 'dpr', 'maxwidth', 'maxheight']

  /** 去掉地址里表示尺寸的查询参数。没有就原样返回。 */
  function dropSizeParams(url) {
    let u
    try {
      u = new URL(url)
    } catch {
      return url
    }
    let hit = false
    for (const k of SIZE_PARAMS) {
      if (u.searchParams.has(k)) {
        u.searchParams.delete(k)
        hit = true
      }
    }
    return hit ? u.href : url
  }

  /** 用户规则：每行 “匹配正则 => 替换”，# 开头是注释。解析不了的行忽略。 */
  function parseUserRules(text) {
    const out = []
    for (const raw of String(text || '').split(/\r?\n/)) {
      const line = raw.trim()
      if (!line || line.startsWith('#')) continue
      const i = line.indexOf('=>')
      if (i < 0) continue
      try {
        out.push(rule(new RegExp(line.slice(0, i).trim(), 'i'), line.slice(i + 2).trim()))
      } catch {
        // 正则写错了：忽略这一行
      }
    }
    return out
  }

  const apply = (r, url) => {
    const m = r.re.exec(url)
    if (!m) return []
    if (typeof r.to === 'function') return [].concat(r.to(...m)).filter((x) => x && x !== url)
    const res = url.replace(r.re, r.to)
    return res !== url ? [res] : []
  }

  /** 一个地址的候选原图地址（不含它自己），按可能性从高到低、去重。 */
  function candidates(url, userRules = '') {
    const src = String(url || '')
    if (!/^https?:\/\//i.test(src)) return []
    const out = []
    const push = (x) => {
      if (x && x !== src && !out.includes(x)) out.push(x)
    }
    const rules = [...parseUserRules(userRules), ...BUILTIN, ...GENERIC]
    for (const r of rules) for (const x of apply(r, src)) push(x)
    // 全部规则连着套一遍：缩略图常常同时带有几种后缀
    let chain = src
    for (const r of rules) {
      const next = apply(r, chain)[0]
      if (next) chain = next
    }
    chain = dropSizeParams(chain)
    push(chain)
    push(dropSizeParams(src))
    // 连着套出来的最彻底，放最前面
    const i = out.indexOf(chain)
    if (i > 0) out.unshift(...out.splice(i, 1))
    return out
  }

  const LAZY_ATTRS = ['data-original', 'data-orig-file', 'data-large-file', 'data-full', 'data-full-url', 'data-large', 'data-big', 'data-hires', 'data-zoom-src', 'data-origin', 'data-src', 'data-lazy-src', 'data-lazy']

  function bestOfSrcset(srcset) {
    let top = ''
    let score = -1
    for (const part of String(srcset || '').split(/,\s+(?=\S)/)) {
      const [u, d] = part.trim().split(/\s+/)
      if (!u) continue
      const n = d ? parseFloat(d) || 1 : 1
      if (n > score) {
        score = n
        top = u
      }
    }
    return top
  }

  const absUrl = (v, base = document.baseURI) => {
    try {
      const u = new URL(String(v).trim(), base)
      return /^https?:$/.test(u.protocol) ? u.href : ''
    } catch {
      return ''
    }
  }

  const IMG_RE = new RegExp(`\\.${EXT}(?:$|[?#])`, 'i')

  /** 一个图片元素的候选原图地址，页面自带线索在前。第一项以后依次尝试。 */
  function forElement(el, userRules = '') {
    const out = []
    const push = (x) => {
      const u = absUrl(x)
      if (u && !out.includes(u)) out.push(u)
    }
    const a = el.closest?.('a[href]')
    if (a && IMG_RE.test(a.getAttribute('href') || '')) push(a.getAttribute('href'))
    for (const k of LAZY_ATTRS) if (el.getAttribute?.(k)) push(el.getAttribute(k))
    push(bestOfSrcset(el.getAttribute?.('srcset')))
    const pic = el.closest?.('picture')
    if (pic) for (const s of pic.querySelectorAll('source[srcset]')) push(bestOfSrcset(s.getAttribute('srcset')))
    const shown = el.currentSrc || el.src || ''
    for (const x of candidates(shown, userRules)) push(x)
    return out
  }

  globalThis.ccOriginal = { candidates, forElement, parseUserRules, dropSizeParams, bestOfSrcset }
})()
