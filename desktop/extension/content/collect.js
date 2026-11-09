// 页面媒体总览：按需注入，收集这个页面（和它的每个 frame）里的图片、视频、音频地址。最后一个表达式的值就是结果。
(() => {
  const out = []
  const seen = new Set()
  const abs = (v) => {
    try {
      const u = new URL(String(v).trim(), document.baseURI)
      return /^https?:$/.test(u.protocol) ? u.href : ''
    } catch {
      return ''
    }
  }
  const clip = (t, n) => String(t || '').replace(/\s+/g, ' ').trim().slice(0, n)
  /** 一个媒体元素的“说明”和“周围的字”：title / aria-label / figcaption 当说明；所在块里最近的标题和一小段文字当周围的字。给 AI 起名用。 */
  const contextOf = (el) => {
    if (!el || !el.closest) return {}
    const fig = el.closest('figure')
    const cap = clip(el.title || el.getAttribute('aria-label') || fig?.querySelector('figcaption')?.textContent || el.closest('a[title]')?.title, 80)
    let box = el.parentElement
    for (let i = 0; i < 4 && box && clip(box.textContent, 400).length < 20; i++) box = box.parentElement
    const head = box?.querySelector?.('h1,h2,h3,h4,h5,h6')
    const near = clip([head?.textContent, box && !head ? box.textContent : box?.querySelector?.('p')?.textContent].filter(Boolean).join(' '), 120)
    return { ...(cap ? { cap } : {}), ...(near ? { near } : {}) }
  }
  const add = (kind, raw, extra = {}) => {
    const url = abs(raw)
    if (!url) return
    const key = `${kind}|${url}`
    if (seen.has(key)) return
    seen.add(key)
    out.push({ kind, url, ...extra })
  }
  /** srcset 里最大的一张（按 w 或 x 描述符）。 */
  const best = (srcset) => {
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
  const MEDIA_EXT = /\.(mp4|m4v|webm|mkv|mov|flv|m3u8|mpd|mp3|m4a|aac|flac|wav|ogg|opus)(\?|#|$)/i
  const IMG_EXT = /\.(jpe?g|png|webp|avif|gif|bmp)(\?|#|$)/i

  for (const img of document.images) {
    const w = img.naturalWidth || img.width
    const h = img.naturalHeight || img.height
    const lazy = img.dataset.original || img.dataset.src || img.dataset.lazySrc || img.dataset.origin || ''
    const src = best(img.srcset) || lazy || img.currentSrc || img.src
    add('image', src, { w, h, alt: img.alt || '', thumb: img.currentSrc || img.src, ...contextOf(img) })
  }
  for (const s of document.querySelectorAll('picture source[srcset]')) add('image', best(s.srcset))
  for (const v of document.querySelectorAll('video')) {
    const src = v.currentSrc || v.src
    if (src) add('video', src, { w: v.videoWidth, h: v.videoHeight, dur: isFinite(v.duration) ? v.duration : 0, thumb: v.poster ? abs(v.poster) : '', ...contextOf(v) })
    for (const s of v.querySelectorAll('source[src]')) add('video', s.src, { w: v.videoWidth, h: v.videoHeight })
    if (v.poster) add('image', v.poster, { alt: '视频封面' })
  }
  for (const a of document.querySelectorAll('audio')) {
    if (a.currentSrc || a.src) add('audio', a.currentSrc || a.src)
    for (const s of a.querySelectorAll('source[src]')) add('audio', s.src)
  }
  for (const m of document.querySelectorAll('meta[property="og:image"],meta[name="twitter:image"],meta[property="og:video"],meta[property="og:video:url"],meta[property="og:audio"]')) {
    const p = m.getAttribute('property') || m.getAttribute('name') || ''
    add(p.includes('video') ? 'video' : p.includes('audio') ? 'audio' : 'image', m.content, { alt: '页面声明' })
  }
  for (const a of document.querySelectorAll('a[href]')) {
    const href = a.getAttribute('href') || ''
    if (MEDIA_EXT.test(href)) add(/\.(mp3|m4a|aac|flac|wav|ogg|opus)(\?|#|$)/i.test(href) ? 'audio' : 'video', href, { alt: a.textContent.trim().slice(0, 60) })
    else if (IMG_EXT.test(href)) add('image', href, { alt: a.textContent.trim().slice(0, 60) })
  }
  // 较大元素的 CSS 背景图
  let scanned = 0
  for (const el of document.querySelectorAll('div,section,a,li,span,figure,header')) {
    if (++scanned > 1500) break
    const r = el.getBoundingClientRect()
    if (r.width < 120 || r.height < 120) continue
    const m = /url\((['"]?)(.*?)\1\)/.exec(getComputedStyle(el).backgroundImage || '')
    if (m && !m[2].startsWith('data:')) add('image', m[2], { w: Math.round(r.width), h: Math.round(r.height), alt: '背景图' })
  }
  // 页面正文摘录（给 AI 理解页面主题用）：优先文章区域，最多 1500 字
  const main = document.querySelector('article,main,[role="main"]') || document.body
  const excerpt = clip(main?.innerText, 1500)
  return { frame: location.href, title: document.title, items: out, excerpt }
})()
