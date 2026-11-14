// 视频摘要纯逻辑的拆分模块（由 vsum-logic.js 汇总成对外的 ccVsum 对象）：不碰网页和浏览器接口；后面的模块直接取前面模块放进 C 的函数。
// 时间、字幕解析（JSON3 / XML / B 站 / 文本）、整理成句、章节、网址与视频标识。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsumLogic ||= {})
  P.parse = (C) => {
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
        const text = cleanCueText(e.segs.map((s) => s.utf8 || '').join(''))
        if (text) out.push({ t: Math.round(e.tStartMs || 0), end: Math.round((e.tStartMs || 0) + (e.dDurationMs || 0)), text })
      }
      return out
    }

    /** YouTube 的 timedtext XML：<text start="1.2" dur="3">…</text>，或 srv3 的 <p t="1200" d="3000">…</p>。 */
    function parseTimedTextXml(xml) {
      const out = []
      const clean = (s) => cleanCueText(s)
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
        const text = cleanCueText(c.content)
        if (text) out.push({ t: Math.round((c.from || 0) * 1000), end: Math.round((c.to || 0) * 1000), text })
      }
      return out
    }

    const decodeEntities = (s) =>
      s.replace(/&(amp|lt|gt|quot|nbsp|#39);/g, (_, n) => ({ amp: '&', lt: '<', gt: '>', quot: '"', nbsp: ' ', '#39': "'" })[n])

    /**
     * 一条字幕的文字清理：去标记、解实体、去零宽字符、合并空白。
     * 有些字幕的标记是转义过的（`&lt;b&gt;&lt;font color=&#39;#e27717&#39;&gt;…`）：只先去标记再解实体的话，解出来就是一串字面的 `<b><font …>`。
     * 所以：先去一遍标记 → 解实体（最多两轮，认命名和数字写法）→ 每轮解完再去一遍常见标记。
     */
    const KNOWN_TAG = /<\/?(?:b|i|u|s|em|strong|font|c|v|ruby|rt|rp|lang|span|br|p|div)\b[^>]*>|<\d{1,2}:\d{2}(?::\d{2})?[.,]\d{1,3}>/gi
    /** 像标记的：<b> </font> <c.colorE5E5E5> <v Bob> <00:01.500>；`a < b and c > d` 里的小于号后面是空格，不算 */
    const TAGLIKE = /<\/?[A-Za-z][^<>]*>|<\d{1,2}:\d{2}(?::\d{2})?[.,]\d{1,3}>/g
    const NAMED = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' }
    const decodeAll = (s) =>
      s.replace(/&(?:#(\d{1,7})|#x([0-9a-f]{1,6})|(amp|lt|gt|quot|apos|nbsp));/gi, (m, d, h, n) => {
        if (n) return NAMED[n.toLowerCase()]
        const cp = d ? Number(d) : parseInt(h, 16)
        return cp > 0 && cp <= 0x10ffff ? String.fromCodePoint(cp) : ''
      })
    function cleanCueText(raw) {
      let t = String(raw ?? '').replace(TAGLIKE, '').replace(/\{\\[^}]*\}/g, '')
      for (let i = 0; i < 2; i++) {
        const d = decodeAll(t)
        if (d === t) break
        t = d.replace(KNOWN_TAG, '')
      }
      return t
        .replace(/[\u200b-\u200d\u2060\ufeff\u00ad]/g, '')
        .replace(/[\u00a0\u2000-\u200a\u202f\u3000]/g, ' ')
        .replace(/\s+/g, ' ')
        .trim()
    }

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
        const body = cleanCueText(lines.slice(i + 1).join(' '))
        const t = parseClock(m[1])
        if (body && t != null) out.push({ t, end: parseClock(m[2]) ?? t, text: body })
      }
      return out
    }

    /** 播放器里读出来的 TextTrackCue 列表 → 同样的格式。 */
    function fromTextTrackCues(cues) {
      return [...(cues || [])]
        .map((c) => ({ t: Math.round(c.startTime * 1000), end: Math.round(c.endTime * 1000), text: cleanCueText(String(c.text || '').replace(/<[^>]*>/g, ' ')) }))
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
      const sorted = [...(cues || [])].map((c) => (c && c.text ? { ...c, text: cleanCueText(c.text) } : c)).filter((c) => c && c.text).sort((a, b) => a.t - b.t)
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


    Object.assign(C, { parseClock, fmtClock, timeLink, videoIdOf, parseJson3, parseBilibili, parseSubtitleText, cleanCueText, fromTextTrackCues, normalizeLines, parseDescriptionChapters, videoKey, parseTimedTextXml })
  }
})()
