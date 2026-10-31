// 视频摘要的纯逻辑：字幕解析、整理成句、章节、导图布局、导出、缓存。不碰网页和浏览器接口，可以单元测试。
// 内容脚本里挂在 globalThis.ccVsum 上。
;(() => {
  if (globalThis.ccVsum) return

  // ---------- 时间 ----------

  function fmtClock(ms) {
    const s = Math.max(0, Math.floor((Number(ms) || 0) / 1000))
    const h = Math.floor(s / 3600)
    const m = Math.floor(s / 60) % 60
    const ss = String(s % 60).padStart(2, '0')
    return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
  }

  /** `1:23` / `01:02:03` / `83` / `1:23.5` → 毫秒；不是时间返回 null。 */
  function parseClock(s) {
    const t = String(s ?? '').trim().replace(',', '.')
    if (!/^\d+(:\d{1,2}){0,2}(\.\d+)?$/.test(t)) return null
    const parts = t.split(':').map(Number)
    let sec = 0
    for (const p of parts) sec = sec * 60 + p
    return Math.round(sec * 1000)
  }

  // ---------- 字幕解析 ----------

  /** YouTube 的 json3：{events:[{tStartMs,dDurationMs,segs:[{utf8}]}]} */
  function parseJson3(data) {
    const out = []
    for (const e of data?.events || []) {
      if (!Array.isArray(e.segs)) continue
      const text = e.segs.map((s) => s.utf8 || '').join('').replace(/\s*\n\s*/g, ' ').trim()
      if (text) out.push({ t: Math.round(e.tStartMs || 0), end: Math.round((e.tStartMs || 0) + (e.dDurationMs || 0)), text })
    }
    return out
  }

  /** YouTube 的 timedtext XML：<text start="1.2" dur="3">…</text>，或 srv3 的 <p t="1200" d="3000">…</p>。 */
  function parseTimedTextXml(xml) {
    const out = []
    const clean = (s) => decodeEntities(String(s).replace(/<[^>]*>/g, '')).replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(Number(n))).replace(/\s+/g, ' ').trim()
    for (const m of String(xml).matchAll(/<text\b([^>]*)>([\s\S]*?)<\/text>/g)) {
      const start = m[1].match(/start="([\d.]+)"/)
      const dur = m[1].match(/dur="([\d.]+)"/)
      const text = clean(m[2])
      if (start && text) out.push({ t: Math.round(Number(start[1]) * 1000), end: Math.round((Number(start[1]) + Number(dur?.[1] || 0)) * 1000), text })
    }
    if (out.length) return out
    for (const m of String(xml).matchAll(/<p\b([^>]*)>([\s\S]*?)<\/p>/g)) {
      const t = m[1].match(/\bt="(\d+)"/)
      const d = m[1].match(/\bd="(\d+)"/)
      const text = clean(m[2])
      if (t && text) out.push({ t: Number(t[1]), end: Number(t[1]) + Number(d?.[1] || 0), text })
    }
    return out
  }

  /** 哔哩哔哩的字幕 JSON：{body:[{from,to,content}]}（秒） */
  function parseBilibili(data) {
    const out = []
    for (const c of data?.body || []) {
      const text = String(c.content ?? '').replace(/\s*\n\s*/g, ' ').trim()
      if (text) out.push({ t: Math.round((c.from || 0) * 1000), end: Math.round((c.to || 0) * 1000), text })
    }
    return out
  }

  const decodeEntities = (s) =>
    s.replace(/&(amp|lt|gt|quot|nbsp|#39);/g, (_, n) => ({ amp: '&', lt: '<', gt: '>', quot: '"', nbsp: ' ', '#39': "'" })[n])

  /** SRT / WebVTT 文本。 */
  function parseSubtitleText(text) {
    const out = []
    const blocks = String(text || '').replace(/^﻿/, '').replace(/\r/g, '').split(/\n{2,}/)
    const TS = /((?:\d+:)?\d{1,2}:\d{2}(?:[.,]\d{1,3})?)\s*-->\s*((?:\d+:)?\d{1,2}:\d{2}(?:[.,]\d{1,3})?)/
    for (const b of blocks) {
      const lines = b.split('\n')
      const i = lines.findIndex((l) => TS.test(l))
      if (i < 0) continue
      const m = lines[i].match(TS)
      const body = lines
        .slice(i + 1)
        .join(' ')
        .replace(/<[^>]*>/g, '')
        .replace(/\{\\[^}]*\}/g, '')
        .replace(/\s+/g, ' ')
        .trim()
      const t = parseClock(m[1])
      if (body && t != null) out.push({ t, end: parseClock(m[2]) ?? t, text: decodeEntities(body) })
    }
    return out
  }

  /** 播放器里读出来的 TextTrackCue 列表 → 同样的格式。 */
  function fromTextTrackCues(cues) {
    return [...(cues || [])]
      .map((c) => ({ t: Math.round(c.startTime * 1000), end: Math.round(c.endTime * 1000), text: String(c.text || '').replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim() }))
      .filter((c) => c.text)
  }

  const SENT_END = /[.!?。！？…]["')”’）]?$/

  /**
   * 把零碎的字幕整理成一句一句：去掉自动字幕的滚动重复，把过短的相邻条目合并。
   * 返回 [{t, text}]，t 是这一句开始的毫秒。
   */
  function normalizeLines(cues, opt = {}) {
    const maxLen = opt.maxLen ?? 90
    const maxSpan = opt.maxSpanMs ?? 20000
    const sorted = [...(cues || [])].filter((c) => c && c.text).sort((a, b) => a.t - b.t)
    // 去重：与上一条相同，或上一条只是这一条的前缀（滚动字幕）
    const deduped = []
    for (const c of sorted) {
      const text = c.text.trim()
      const prev = deduped[deduped.length - 1]
      if (prev && (prev.text === text || text.startsWith(prev.text + ' ') && c.t - prev.t < 6000 && prev.text.length < text.length && prev.rolling !== false)) {
        if (prev.text !== text) prev.text = text
        continue
      }
      deduped.push({ t: c.t, end: c.end ?? c.t, text })
    }
    const out = []
    let cur = null
    for (const c of deduped) {
      if (!cur) {
        cur = { t: c.t, text: c.text }
        continue
      }
      const joined = /[぀-ヿ一-鿿]$/.test(cur.text) || /^[぀-ヿ一-鿿]/.test(c.text) ? cur.text + c.text : cur.text + ' ' + c.text
      if (SENT_END.test(cur.text) || cur.text.length >= maxLen || joined.length > maxLen * 1.6 || c.t - cur.t > maxSpan) {
        out.push(cur)
        cur = { t: c.t, text: c.text }
      } else {
        cur.text = joined
      }
    }
    if (cur) out.push(cur)
    return out
  }

  /** 视频简介里的章节：每行 `0:00 标题`（至少 3 行，从 0:00 开始，时间递增）。 */
  function parseDescriptionChapters(desc) {
    const found = []
    for (const raw of String(desc || '').split(/\r?\n/)) {
      const line = raw.trim()
      const m = line.match(/^[\s\-•·▪●*]*\(?\[?((?:\d{1,2}:)?\d{1,2}:\d{2})\]?\)?\s*[-–—:|.)]?\s*(.+)$/) || line.match(/^(.+?)\s*[-–—:|(]?\s*\(?((?:\d{1,2}:)?\d{1,2}:\d{2})\)?$/)
      if (!m) continue
      const [time, title] = /^\d/.test(m[1]) && /:/.test(m[1]) ? [m[1], m[2]] : [m[2], m[1]]
      const ms = parseClock(time)
      if (ms == null || !title.trim()) continue
      found.push({ title: title.trim().replace(/\s+/g, ' '), startMs: ms })
    }
    if (found.length < 3 || found[0].startMs > 1000) return []
    for (let i = 1; i < found.length; i++) if (found[i].startMs <= found[i - 1].startMs) return []
    return found
  }

  // ---------- 网址 ----------

  /** 识别视频网址：{site, id, part}；不认识返回 null。 */
  function videoIdOf(href) {
    let u
    try {
      u = new URL(href)
    } catch {
      return null
    }
    const host = u.hostname.replace(/^www\.|^m\./, '')
    if (host === 'youtu.be') return u.pathname.length > 1 ? { site: 'youtube', id: u.pathname.slice(1).split('/')[0] } : null
    if (host === 'youtube.com' || host === 'music.youtube.com') {
      const v = u.searchParams.get('v')
      if (v) return { site: 'youtube', id: v }
      const m = u.pathname.match(/^\/(?:shorts|embed|live|v)\/([\w-]{6,})/)
      return m ? { site: 'youtube', id: m[1] } : null
    }
    if (host === 'bilibili.com') {
      const m = u.pathname.match(/\/video\/(BV\w+|av\d+)/i)
      if (m) return { site: 'bilibili', id: m[1], part: Number(u.searchParams.get('p')) || 1 }
    }
    return null
  }

  /** 用来记住“这个视频”的键：YouTube / B 站按视频编号（B 站带分 P），其他按去掉锚点的网址。 */
  function videoKey(href) {
    const v = videoIdOf(href)
    if (v) return `${v.site}:${v.id}${v.part > 1 ? `:p${v.part}` : ''}`
    try {
      const u = new URL(href)
      u.hash = ''
      return `url:${u.href}`
    } catch {
      return `url:${href}`
    }
  }

  /** 带时间的链接：点一下能从那一刻开始看。 */
  function timeLink(href, ms) {
    const sec = Math.floor((ms || 0) / 1000)
    const v = videoIdOf(href)
    try {
      const u = new URL(href)
      u.hash = ''
      if (v?.site === 'youtube') {
        const clean = new URL('https://www.youtube.com/watch')
        clean.searchParams.set('v', v.id)
        if (sec) clean.searchParams.set('t', `${sec}s`)
        return clean.href
      }
      if (v?.site === 'bilibili') {
        u.searchParams.set('t', String(sec))
        return u.href
      }
      return u.href
    } catch {
      return href
    }
  }

  // ---------- YouTube 播放页里的数据 ----------

  /** 从 `start` 开始截出一段平衡的 JSON（数组或对象），不认识就返回 null。 */
  function sliceJson(s, start) {
    const open = s[start]
    const close = open === '[' ? ']' : open === '{' ? '}' : null
    if (!close) return null
    let depth = 0
    let inStr = false
    for (let i = start; i < s.length; i++) {
      const c = s[i]
      if (inStr) {
        if (c === '\\') i++
        else if (c === '"') inStr = false
      } else if (c === '"') inStr = true
      else if (c === open) depth++
      else if (c === close && --depth === 0) return s.slice(start, i + 1)
    }
    return null
  }

  /** 从 YouTube 播放页的 HTML 里取出字幕轨、标题、简介、时长。 */
  function parseYouTubePage(html) {
    const text = String(html || '')
    const out = { tracks: [], title: '', description: '', lengthSec: 0 }
    const ci = text.indexOf('"captionTracks":')
    if (ci >= 0) {
      const raw = sliceJson(text, text.indexOf('[', ci))
      try {
        out.tracks = (JSON.parse(raw) || [])
          .filter((t) => t && t.baseUrl)
          .map((t) => ({ url: String(t.baseUrl).replace(/\\u0026/g, '&'), lang: t.languageCode || '', name: t.name?.simpleText || t.name?.runs?.map((r) => r.text).join('') || t.languageCode || '', auto: t.kind === 'asr' }))
      } catch {
        out.tracks = []
      }
    }
    const str = (key) => {
      const i = text.indexOf(`"${key}":"`)
      if (i < 0) return ''
      let j = i + key.length + 4
      let esc = false
      for (; j < text.length; j++) {
        if (esc) esc = false
        else if (text[j] === '\\') esc = true
        else if (text[j] === '"') break
      }
      try {
        return JSON.parse(text.slice(i + key.length + 3, j + 1))
      } catch {
        return ''
      }
    }
    out.title = str('title')
    out.description = str('shortDescription')
    out.lengthSec = Number(str('lengthSeconds')) || 0
    return out
  }

  /** 选字幕轨：优先 想要的语言（人工 > 自动），其次任何人工字幕，再其次自动字幕，最后第一条。 */
  function pickTrack(tracks, prefer = []) {
    const list = tracks || []
    if (!list.length) return null
    const langs = [...prefer, navigator?.language, 'zh', 'en'].filter(Boolean).map((l) => String(l).toLowerCase())
    const match = (t, l) => t.lang.toLowerCase() === l || t.lang.toLowerCase().startsWith(l.split('-')[0])
    for (const l of langs) {
      const m = list.find((t) => !t.auto && match(t, l)) || list.find((t) => match(t, l))
      if (m) return m
    }
    return list.find((t) => !t.auto) || list[0]
  }

  // ---------- 导图 ----------

  /** Markdown 大纲 → 树。第一行 `# 标题` 是根；缺了就用传进来的标题。 */
  function outlineToTree(md, fallbackTitle = '') {
    const root = { text: fallbackTitle || '视频', children: [] }
    const stack = [{ depth: -1, node: root }]
    for (const raw of String(md || '').split('\n')) {
      const line = raw.replace(/\t/g, '  ').replace(/\s+$/, '')
      const head = line.match(/^#\s+(.+)$/)
      if (head && root.children.length === 0 && stack.length === 1) {
        root.text = head[1].trim()
        continue
      }
      const m = line.match(/^(\s*)[-*]\s+(.+)$/)
      if (!m) continue
      const depth = Math.floor(m[1].length / 2)
      const node = { text: m[2].trim(), children: [] }
      while (stack.length > 1 && stack[stack.length - 1].depth >= depth) stack.pop()
      stack[stack.length - 1].node.children.push(node)
      stack.push({ depth, node })
    }
    return root
  }

  const textWidth = (s) => [...String(s)].reduce((w, c) => w + (/[⺀-鿿＀-￯]/.test(c) ? 14 : 7.2), 0)

  /** 把长文字折成多行，每行不超过 maxW。 */
  function wrapText(s, maxW = 190) {
    const lines = []
    let cur = ''
    let w = 0
    for (const ch of String(s)) {
      const cw = textWidth(ch)
      if (w + cw > maxW && cur) {
        lines.push(cur)
        cur = ''
        w = 0
      }
      cur += ch
      w += cw
    }
    if (cur) lines.push(cur)
    return lines.length ? lines : ['']
  }

  /** 向右展开的树形布局：返回每个节点的位置和连线，宽高都算好。 */
  function layoutMindmap(tree, opt = {}) {
    const gapX = opt.gapX ?? 36
    const gapY = opt.gapY ?? 10
    const pad = opt.pad ?? 16
    const maxW = opt.maxW ?? 190
    const nodes = []
    const links = []
    const measure = (n, depth) => {
      const lines = wrapText(n.text, depth === 0 ? maxW + 40 : maxW)
      const w = Math.min(Math.max(...lines.map(textWidth)), maxW + 40) + 20
      const h = lines.length * 18 + 12
      const kids = n.children.map((c) => measure(c, depth + 1))
      const span = kids.length ? Math.max(h, kids.reduce((a, k) => a + k.span, 0) + gapY * (kids.length - 1)) : h
      return { n, depth, lines, w, h, kids, span }
    }
    const root = measure(tree, 0)
    const colW = []
    const widths = (m) => {
      colW[m.depth] = Math.max(colW[m.depth] || 0, m.w)
      m.kids.forEach(widths)
    }
    widths(root)
    const colX = []
    colW.forEach((w, i) => (colX[i] = i === 0 ? pad : colX[i - 1] + colW[i - 1] + gapX))
    const place = (m, top, parent) => {
      const cy = top + m.span / 2
      const node = { x: colX[m.depth], y: cy - m.h / 2, w: m.w, h: m.h, lines: m.lines, depth: m.depth, text: m.n.text }
      nodes.push(node)
      if (parent) links.push({ x1: parent.x + parent.w, y1: parent.y + parent.h / 2, x2: node.x, y2: cy })
      let y = top + (m.span - (m.kids.reduce((a, k) => a + k.span, 0) + gapY * Math.max(0, m.kids.length - 1))) / 2
      for (const k of m.kids) {
        place(k, y, node)
        y += k.span + gapY
      }
    }
    place(root, pad, null)
    return { nodes, links, width: colX[colX.length - 1] + colW[colW.length - 1] + pad, height: root.span + pad * 2 }
  }

  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])
  const BRANCH = ['#b4561a', '#2a7f62', '#3b6ea8', '#8e4ba0', '#b08a1a', '#b23a48']

  /** 布局 → 独立的 SVG 字符串（可以直接下载，也能画成 PNG）。 */
  function mindmapSvg(layout, dark = false) {
    const bg = dark ? '#1d2420' : '#ffffff'
    const fg = dark ? '#eef1ee' : '#1d2420'
    const branchOf = new Map()
    layout.nodes.forEach((n) => {
      if (n.depth === 1) branchOf.set(n, branchOf.size)
    })
    // 每个节点属于哪条一级分支：按 y 范围归属
    const tops = layout.nodes.filter((n) => n.depth === 1)
    const colorOf = (n) => {
      if (n.depth === 0) return '#1d2420'
      let best = 0
      tops.forEach((t, i) => {
        if (t.y <= n.y + n.h / 2 + 0.5) best = i
      })
      return BRANCH[best % BRANCH.length]
    }
    const parts = [`<svg xmlns="http://www.w3.org/2000/svg" width="${Math.ceil(layout.width)}" height="${Math.ceil(layout.height)}" viewBox="0 0 ${Math.ceil(layout.width)} ${Math.ceil(layout.height)}" font-family="-apple-system,'PingFang SC','Microsoft YaHei',sans-serif" font-size="13">`, `<rect width="100%" height="100%" fill="${bg}"/>`]
    for (const l of layout.links) {
      const mx = (l.x1 + l.x2) / 2
      parts.push(`<path d="M${l.x1.toFixed(1)} ${l.y1.toFixed(1)} C${mx.toFixed(1)} ${l.y1.toFixed(1)} ${mx.toFixed(1)} ${l.y2.toFixed(1)} ${l.x2.toFixed(1)} ${l.y2.toFixed(1)}" fill="none" stroke="#9aa5a0" stroke-width="1.5"/>`)
    }
    for (const n of layout.nodes) {
      const c = colorOf(n)
      const filled = n.depth <= 1
      parts.push(`<rect x="${n.x.toFixed(1)}" y="${n.y.toFixed(1)}" width="${n.w.toFixed(1)}" height="${n.h.toFixed(1)}" rx="8" fill="${filled ? c : bg}" stroke="${c}" stroke-width="1.5"/>`)
      n.lines.forEach((ln, i) => parts.push(`<text x="${(n.x + 10).toFixed(1)}" y="${(n.y + 6 + 14 + i * 18).toFixed(1)}" fill="${filled ? '#fff' : fg}">${esc(ln)}</text>`))
    }
    parts.push('</svg>')
    return parts.join('')
  }

  // ---------- 搜索与定位 ----------

  /** 所有词都出现的行，返回行号。 */
  function searchLines(lines, query) {
    const terms = String(query || '').toLowerCase().split(/\s+/).filter(Boolean)
    if (!terms.length) return []
    const out = []
    ;(lines || []).forEach((l, i) => {
      const t = l.text.toLowerCase()
      if (terms.every((x) => t.includes(x))) out.push(i)
    })
    return out
  }

  /** 播放到 ms 时正在说的是哪一行（最后一个 t <= ms 的）；还没开始返回 -1。 */
  function lineAt(lines, ms) {
    let lo = 0
    let hi = (lines || []).length - 1
    let ans = -1
    while (lo <= hi) {
      const mid = (lo + hi) >> 1
      if (lines[mid].t <= ms) {
        ans = mid
        lo = mid + 1
      } else hi = mid - 1
    }
    return ans
  }

  /** 章节里，ms 落在第几章；没有章节返回 -1。 */
  function chapterAt(chapters, ms) {
    let ans = -1
    ;(chapters || []).forEach((c, i) => {
      if ((c.startMs ?? 0) <= ms) ans = i
    })
    return ans
  }

  // ---------- 导出 ----------

  /** 完整的笔记（Markdown）：标题、链接、摘要、要点、章节（带跳转链接）、金句、卡片、文稿。 */
  function toMarkdown(d) {
    const url = d.url || ''
    const link = (ms) => (url ? timeLink(url, ms) : '')
    const r = d.result || {}
    const out = [`# ${d.title || '视频笔记'}`, '']
    if (url) out.push(`来源：${url}`, '')
    if (r.keywords?.length) out.push(r.keywords.map((k) => `#${String(k).replace(/\s+/g, '_')}`).join(' '), '')
    if (r.summary) out.push('## 摘要', '', r.summary, '')
    if (r.points?.length) out.push('## 要点', '', ...r.points.map((p) => `- ${p}`), '')
    if (r.chapters?.length) {
      out.push('## 章节', '')
      for (const c of r.chapters) out.push(url ? `- [${fmtClock(c.startMs)}](${link(c.startMs)}) ${c.title}` : `- ${fmtClock(c.startMs)} ${c.title}`)
      out.push('')
    }
    if (d.quotes?.length) {
      out.push('## 金句', '')
      for (const q of d.quotes) out.push(`> ${q.text}${q.t != null ? (url ? ` — [${fmtClock(q.t)}](${link(q.t)})` : ` — ${fmtClock(q.t)}`) : ''}`, '')
    }
    if (d.cards?.length) {
      out.push('## 复习卡片', '')
      d.cards.forEach((c, i) => out.push(`${i + 1}. **${c.q}**`, `   ${c.a}`))
      out.push('')
    }
    if (d.outline) out.push('## 导图大纲', '', d.outline.replace(/^# .*\n?/, '').trim(), '')
    if (d.lines?.length && d.includeTranscript) {
      out.push('## 文稿', '')
      for (const l of d.lines) out.push(url ? `[${fmtClock(l.t)}](${link(l.t)}) ${l.text}` : `[${fmtClock(l.t)}] ${l.text}`, '')
    }
    return out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n'
  }


  // ---------- 笔记 ----------

  const MAX_NOTES = 2000

  /** 一条笔记：{ id, key, url, title, t, text, kind: 'quote'|'note', created }。 */
  function makeNote(o) {
    const text = String(o.text || '').trim()
    if (!text) return null
    return {
      id: o.id || `n${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
      key: o.key || '',
      url: o.url || '',
      title: String(o.title || '').slice(0, 200),
      t: Number.isFinite(o.t) ? Math.max(0, Math.round(o.t)) : -1,
      text: text.slice(0, 6000),
      kind: o.kind === 'note' ? 'note' : 'quote',
      created: o.created || Date.now(),
    }
  }

  /** 加一条；同一视频同一时刻同样的文字不重复记。超过上限丢最早的。 */
  function addNote(list, note) {
    if (!note) return list
    if (list.some((n) => n.key === note.key && n.t === note.t && n.text === note.text)) return list
    return [...list, note].slice(-MAX_NOTES)
  }

  /** 同一视频的按时间排，所有视频的按视频分组、组内按时间。 */
  function sortNotes(list) {
    return [...list].sort((a, b) => (a.key === b.key ? a.t - b.t || a.created - b.created : a.created - b.created))
  }

  function searchNotes(list, q) {
    const s = String(q || '').trim().toLowerCase()
    if (!s) return list
    return list.filter((n) => `${n.text} ${n.title}`.toLowerCase().includes(s))
  }

  /** 笔记导出成 Markdown：每个视频一节，时间戳带跳转链接。 */
  function notesToMarkdown(list, opt = {}) {
    const groups = new Map()
    for (const n of sortNotes(list)) {
      if (!groups.has(n.key)) groups.set(n.key, [])
      groups.get(n.key).push(n)
    }
    const out = [`# ${opt.title || '视频笔记'}`, '']
    for (const [, ns] of groups) {
      const first = ns[0]
      out.push(`## ${first.title || '未命名视频'}`, '')
      if (first.url) out.push(`来源：${first.url}`, '')
      for (const n of ns) {
        const stamp = n.t >= 0 ? (n.url ? `[${fmtClock(n.t)}](${timeLink(n.url, n.t)}) ` : `${fmtClock(n.t)} `) : ''
        const body = n.text.replace(/\n+/g, ' ')
        out.push(n.kind === 'note' ? `- ${stamp}${body}` : `- ${stamp}> ${body}`)
      }
      out.push('')
    }
    return out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n'
  }

  /** 学习稿：YAML 头 + 摘要要点 + 章节 + 金句 + 我的笔记 (+ 文稿)。 */
  function studyDoc(d, notes) {
    const head = ['---', `title: "${String(d.title || '').replace(/"/g, "'")}"`, `source: ${d.url || ''}`, `date: ${new Date(d.date || Date.now()).toISOString().slice(0, 10)}`, 'tags: [video-notes]', '---', '']
    const body = toMarkdown(d).replace(/^# .*\n\n?/, '# ' + (d.title || '视频笔记') + '\n\n')
    const mine = (notes || []).filter((n) => n.t >= 0 || n.text)
    const sect = mine.length ? ['', '## 我的笔记', '', ...sortNotes(mine).map((n) => `- ${n.t >= 0 ? (d.url ? `[${fmtClock(n.t)}](${timeLink(d.url, n.t)}) ` : `${fmtClock(n.t)} `) : ''}${n.kind === 'note' ? '' : '> '}${n.text.replace(/\n+/g, ' ')}`), ''] : []
    return head.join('\n') + body.trimEnd() + '\n' + sect.join('\n')
  }

  /** 把 ms 附近一行及前后各 n 行拼成给大模型的上下文。 */
  function contextAround(lines, ms, n = 3) {
    const i = lineAt(lines, ms)
    if (i < 0) return { before: '', target: '', after: '', full: '' }
    const f = (a, b) => lines.slice(Math.max(0, a), b).map((l) => `[${fmtClock(l.t)}] ${l.text}`).join('\n')
    return { before: f(i - n, i), target: lines[i].text, after: f(i + 1, i + 1 + n), full: f(i - n, i + 1 + n) }
  }

  /** 译文视图：原文 / 译文 / 双语 → 每行要显示的 [主, 副]。 */
  function viewLine(text, tr, mode) {
    if (!tr) return [text, '']
    if (mode === 'tr') return [tr, '']
    if (mode === 'both') return [text, tr]
    return [text, '']
  }

  const srtTime = (ms) => {
    const t = Math.max(0, Math.round(ms))
    const p = (n, w = 2) => String(n).padStart(w, '0')
    return `${p(Math.floor(t / 3600000))}:${p(Math.floor(t / 60000) % 60)}:${p(Math.floor(t / 1000) % 60)},${p(t % 1000, 3)}`
  }

  function toSrt(lines) {
    return (lines || [])
      .map((l, i) => {
        const end = lines[i + 1] ? Math.min(lines[i + 1].t, l.t + 8000) : l.t + 4000
        return `${i + 1}\n${srtTime(l.t)} --> ${srtTime(Math.max(end, l.t + 500))}\n${l.text}\n`
      })
      .join('\n')
  }

  function toTxt(lines) {
    return (lines || []).map((l) => `[${fmtClock(l.t)}] ${l.text}`).join('\n') + '\n'
  }

  /** Anki 能直接导入的 CSV（制表符分隔，正面 / 背面 / 时间）。 */
  function cardsToCsv(cards) {
    const cell = (s) => String(s ?? '').replace(/[\t\r\n]+/g, ' ').trim()
    return (cards || []).map((c) => [cell(c.q), cell(c.a), c.t != null ? fmtClock(c.t) : ''].join('\t')).join('\n') + '\n'
  }

  /** 安全的文件名。 */
  function fileName(title, ext) {
    const base = String(title || '视频笔记').replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 80) || '视频笔记'
    return `${base}.${ext}`
  }

  // ---------- 缓存 ----------

  function hashText(s) {
    let h = 0x811c9dc5
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i)
      h = Math.imul(h, 0x01000193) >>> 0
    }
    return h.toString(36)
  }

  /** 一个结果的键：文稿内容 + 类型 + 风格 + 语言（这些不变就不用再问模型）。 */
  function cacheKey(lines, kind, style = '', lang = '', extra = '') {
    const body = (lines || []).map((l) => `${l.t}|${l.text}`).join('\n')
    return [kind, style, lang, extra, (lines || []).length, hashText(body)].join(':')
  }

  /** 缓存太多或太大时，丢掉最旧的。entries：{key: {ts, ...}}。 */
  function pruneCache(entries, maxCount = 40, maxBytes = 2_000_000) {
    const list = Object.entries(entries || {}).sort((a, b) => (b[1]?.ts || 0) - (a[1]?.ts || 0))
    const kept = {}
    let bytes = 0
    let n = 0
    for (const [k, v] of list) {
      const size = JSON.stringify(v).length
      if (n >= maxCount || bytes + size > maxBytes) continue
      kept[k] = v
      bytes += size
      n++
    }
    return kept
  }

  const STYLES = [
    ['standard', '标准'],
    ['brief', '简短'],
    ['detailed', '详细'],
    ['plain', '通俗'],
    ['study', '学习笔记'],
  ]
  const REWRITES = [
    ['xhs', '小红书文案'],
    ['thread', '推文串'],
    ['blog', '博客文章'],
    ['email', '分享邮件'],
    ['notes', '会议纪要式笔记'],
  ]
  const SUGGESTIONS = ['这个视频的核心观点是什么？', '有哪些可以直接照做的建议？', '作者提到了哪些数据或例子？', '有哪些我可能不同意的地方？']

  globalThis.ccVsum = {
    fmtClock, parseClock, parseJson3, parseBilibili, parseSubtitleText, fromTextTrackCues, normalizeLines, parseDescriptionChapters,
    videoIdOf, videoKey, parseTimedTextXml, timeLink, sliceJson, parseYouTubePage, pickTrack, outlineToTree, wrapText, layoutMindmap, mindmapSvg,
    searchLines, lineAt, chapterAt, makeNote, addNote, sortNotes, searchNotes, notesToMarkdown, studyDoc, contextAround, viewLine, toMarkdown, toSrt, toTxt, cardsToCsv, fileName, hashText, cacheKey, pruneCache, STYLES, REWRITES, SUGGESTIONS,
  }
})()
