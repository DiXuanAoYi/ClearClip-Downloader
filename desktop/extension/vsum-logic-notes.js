// 视频摘要纯逻辑的拆分模块（由 vsum-logic.js 汇总成对外的 ccVsum 对象）：不碰网页和浏览器接口；后面的模块直接取前面模块放进 C 的函数。
// 搜索定位、导出（Markdown / SRT / 文本 / 卡片）、笔记、缓存淘汰。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsumLogic ||= {})
  P.notes = (C) => {
    const { fmtClock, timeLink } = C

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
    /** 总结文字拆成“一句话”和后面的段落。新格式第一段就是一句话；旧结果取第一句。 */
    function splitSummary(text) {
      const paras = String(text || '').split(/\n\s*\n/).map((x) => x.trim()).filter(Boolean)
      if (!paras.length) return { lead: '', paras: [] }
      if (paras.length > 1) return { lead: paras[0], paras: paras.slice(1) }
      const m = paras[0].match(/^(.+?[。！？!?.])(\s+|(?=[^\s]))(.*)$/s)
      if (m && m[1].length >= 8 && m[3].trim()) return { lead: m[1].trim(), paras: [m[3].trim()] }
      return { lead: paras[0], paras: [] }
    }

    /** 要点：“**标题** 说明 [m:ss]” 拆成 { title, detail, time }；旧的整句要点只有 detail。 */
    function parsePoint(text) {
      let t = String(text || '').trim()
      const tm = t.match(/\[(\d{1,2}(?::\d{2}){1,2})\]/)
      const time = tm ? tm[1] : ''
      if (tm) t = (t.slice(0, tm.index) + t.slice(tm.index + tm[0].length)).replace(/\s{2,}/g, ' ').trim()
      const m = t.match(/^\*\*(.+?)\*\*[\s:：—-]*(.*)$/s)
      return m ? { title: m[1].trim(), detail: m[2].trim(), time } : { title: '', detail: t, time }
    }

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

    /** 视频键去掉分 P 后缀：同一个视频的所有分 P 共用；分 P 序号（没有就是 1）。 */
    const baseKey = (key) => String(key || '').replace(/:p\d+$/, '')
    const partOf = (key) => Number((String(key || '').match(/:p(\d+)$/) || [])[1]) || 1

    /** 一条笔记：{ id, key, url, title, owner, t, text, kind: 'quote'|'note', created }。 */
    function makeNote(o) {
      const text = String(o.text || '').trim()
      if (!text) return null
      return {
        id: o.id || `n${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
        key: o.key || '',
        url: o.url || '',
        title: String(o.title || '').slice(0, 200),
        owner: String(o.owner || '').slice(0, 80),
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
      return trimNotes([...list, note])
    }

    const MAX_NOTES_BYTES = 3_000_000

    /** 笔记太多或太大时丢最早的（保留最新的）：条数上限 + 总字数上限，免得把扩展的 10MB 存储空间占满。 */
    function trimNotes(list, maxCount = MAX_NOTES, maxBytes = MAX_NOTES_BYTES) {
      const src = Array.isArray(list) ? list : []
      const out = []
      let bytes = 0
      for (let i = src.length - 1; i >= 0 && out.length < maxCount; i--) {
        const n = src[i]
        bytes += JSON.stringify(n ?? '').length
        if (bytes > maxBytes && out.length) break
        out.push(n)
      }
      return out.reverse()
    }

    /**
     * 按“最近用过”的顺序淘汰：对象的键顺序就是新旧顺序（最新的在最后），超过条数或总字数就从最前面删。
     * 写入时要先 delete 再放回去，才算“用过”。返回新对象。
     */
    function lruTrim(all, keep, maxBytes = Infinity) {
      const keys = Object.keys(all || {})
      const out = {}
      let bytes = 0
      let n = 0
      for (let i = keys.length - 1; i >= 0; i--) {
        const size = JSON.stringify(all[keys[i]]).length
        if (n >= keep || (n > 0 && bytes + size > maxBytes)) continue
        out[keys[i]] = all[keys[i]]
        bytes += size
        n++
      }
      return Object.fromEntries(Object.entries(out).reverse())
    }

    /** 一个视频里手动改字幕的记录（{ 键: 改后文字 }）最多留这么多条，超出丢最早的。 */
    const MAX_EDITS_PER_VIDEO = 2000
    function trimEdits(m, max = MAX_EDITS_PER_VIDEO) {
      const keys = Object.keys(m || {})
      if (keys.length <= max) return m
      return Object.fromEntries(keys.slice(keys.length - max).map((k) => [k, m[k]]))
    }

    /** 同一视频的按时间排，所有视频的按视频分组、组内按时间。 */
    function sortNotes(list) {
      return [...list].sort((a, b) => (a.key === b.key ? a.t - b.t || a.created - b.created : a.created - b.created))
    }

    function searchNotes(list, q) {
      const s = String(q || '').trim().toLowerCase()
      if (!s) return list
      return list.filter((n) => `${n.text} ${n.title} ${n.owner || ''}`.toLowerCase().includes(s))
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



    Object.assign(C, { lineAt, searchLines, chapterAt, makeNote, addNote, trimNotes, lruTrim, trimEdits, sortNotes, searchNotes, baseKey, partOf, notesToMarkdown, studyDoc, contextAround, viewLine, toMarkdown, splitSummary, parsePoint, toSrt, toTxt, cardsToCsv, fileName, hashText, cacheKey, pruneCache })
  }
})()
