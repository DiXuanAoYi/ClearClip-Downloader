// 划词工具条的纯逻辑：朗读语言、搜索链接、高亮的锚点（带前后文）与定位、导出 Markdown。
// 不碰网页和浏览器接口，可以单元测试。内容脚本里挂在 globalThis.ccSel。
;(() => {
  if (globalThis.ccSel) return

  const COLORS = [
    { id: 'y', name: '重点', bg: '#ffe58a' },
    { id: 'g', name: '术语', bg: '#bfe8d3' },
    { id: 'r', name: '存疑', bg: '#ffc9c2' },
    { id: 'b', name: '引用', bg: '#cfdcff' },
  ]
  const colorOf = (id) => COLORS.find((c) => c.id === id) || COLORS[0]

  /** 朗读用哪种语言的声音：看文字主要是什么文字。 */
  function speechLang(text) {
    const t = String(text || '')
    const n = (re) => (t.match(re) || []).length
    const kana = n(/[぀-ヿ]/g)
    const han = n(/[一-鿿]/g)
    const hangul = n(/[가-힯]/g)
    const cyr = n(/[Ѐ-ӿ]/g)
    const latin = n(/[A-Za-zÀ-ÿ]/g)
    if (kana > 0) return 'ja-JP'
    if (hangul > han && hangul > latin) return 'ko-KR'
    if (han >= latin / 2 && han > 0) return 'zh-CN'
    if (cyr > latin) return 'ru-RU'
    return 'en-US'
  }

  /** 是不是“一个词或短语”（查词典），而不是一句话（做解释）。 */
  function isWord(text) {
    const t = String(text || '').trim()
    if (!t || t.length > 40) return false
    if (/[。！？!?；;\n]/.test(t)) return false
    if (/[㐀-鿿]/.test(t)) return t.length <= 8
    return t.split(/\s+/).length <= 3
  }

  const searchUrl = (text) => `https://www.bing.com/search?q=${encodeURIComponent(String(text || '').trim().slice(0, 200))}`

  /** 页面键：去掉锚点，同一页的高亮放一起。 */
  function pageKey(href) {
    try {
      const u = new URL(href)
      u.hash = ''
      return u.href
    } catch {
      return String(href || '')
    }
  }

  const CTX = 24

  /** 一条高亮：text 是选中的字，before / after 是它前后各一小段，重新打开页面时用来找回原来的位置。 */
  function makeHl({ full, start, end, color = 'y', url = '', title = '' }) {
    const text = full.slice(start, end)
    return {
      id: `h${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
      color: COLORS.some((c) => c.id === color) ? color : 'y',
      text,
      before: full.slice(Math.max(0, start - CTX), start),
      after: full.slice(end, end + CTX),
      url: pageKey(url),
      title: String(title || '').slice(0, 200),
      created: Date.now(),
    }
  }

  /** 在整页文字里找回一条高亮的位置。返回 [start, end] 或 null。前后文都对得上最准，对不上就退一步。 */
  function locate(full, h) {
    const t = h.text
    if (!t) return null
    const tries = [`${h.before}${t}${h.after}`, `${h.before}${t}`, `${t}${h.after}`]
    for (const [i, probe] of tries.entries()) {
      const at = full.indexOf(probe)
      if (at >= 0) {
        const s = at + (i === 2 ? 0 : h.before.length)
        return [s, s + t.length]
      }
    }
    const at = full.indexOf(t)
    return at >= 0 ? [at, at + t.length] : null
  }

  /** 把一页的高亮导出成 Markdown。 */
  function toMarkdown(list, { title = '', url = '' } = {}) {
    const L = [`# ${title || '网页摘录'}`, '']
    if (url) L.push(`来源：${url}`, '')
    for (const h of list || []) L.push(`- ${colorOf(h.color).name}：${String(h.text).replace(/\s+/g, ' ').trim()}${h.note ? `\n  - 笔记：${h.note}` : ''}`)
    return L.join('\n') + '\n'
  }

  /** 按页面分组，最近的在前。 */
  function groupByPage(all) {
    return Object.entries(all || {})
      .map(([url, list]) => ({ url, title: list.find((x) => x.title)?.title || url, list, last: Math.max(0, ...list.map((x) => x.created || 0)) }))
      .filter((g) => g.list.length)
      .sort((a, b) => b.last - a.last)
  }

  /** 限制总量：最多 300 页，每页最多 200 条。 */
  function prune(all) {
    const g = groupByPage(all).slice(0, 300)
    return Object.fromEntries(g.map((x) => [x.url, x.list.slice(-200)]))
  }

  globalThis.ccSel = { COLORS, colorOf, speechLang, isWord, searchUrl, pageKey, makeHl, locate, toMarkdown, groupByPage, prune }
})()
