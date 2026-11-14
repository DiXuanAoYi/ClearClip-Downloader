// 视频摘要纯逻辑的拆分模块（由 vsum-logic.js 汇总成对外的 ccVsum 对象）：不碰网页和浏览器接口；后面的模块直接取前面模块放进 C 的函数。
// YouTube 播放页里的数据（字幕轨、标题、章节）与重新断句。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsumLogic ||= {})
  P.page = (C) => {
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
      // 视频自己说的语言：只有自动字幕（asr）一定是按语音识别出来的。
      // 页面里的 defaultCaptionTrackIndex 是“按观看者的界面语言挑的默认字幕”，不是视频说的语言（中文用户看英文视频会指到中文轨），不能拿来当依据；
      // 没有自动字幕时交给 pickTrack 按标题文字、再按偏好判断
      const asr = out.tracks.find((t) => t.auto)
      out.spoken = asr?.lang || ''
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
    /** 文字主要是哪种文字：zh / ja / ko / latin，看不出来返回空。 */
    function scriptOf(text) {
      const t = String(text || '')
      const n = (re) => (t.match(re) || []).length
      const kana = n(/[\u3040-\u30ff]/g)
      const han = n(/[\u4e00-\u9fff]/g)
      const hangul = n(/[\uac00-\ud7af]/g)
      const latin = n(/[A-Za-z]/g)
      if (kana > 0 && kana + han >= latin) return 'ja'
      if (hangul > 0 && hangul >= latin) return 'ko'
      if (han > 0 && han >= latin / 2) return 'zh'
      return latin > 0 ? 'latin' : ''
    }
    const sameLang = (a, b) => !!a && !!b && String(a).toLowerCase().split(/[-_]/)[0] === String(b).toLowerCase().split(/[-_]/)[0]

    /**
     * 选字幕轨。hint：{ spoken: 视频语音的语言码, title: 标题 }。
     * 知道视频说的是什么语言时，只在这种语言里选（人工字幕 > 自动字幕），不会把别的语言的轨道当成“原文”；
     * 不知道时：标题是英文/拉丁字母就偏英文，是汉字就偏中文，再不行才看浏览器语言。
     */
    function pickTrack(tracks, prefer = [], hint = {}) {
      const list = tracks || []
      if (!list.length) return null
      const titleScript = scriptOf(hint.title)
      // 自动字幕（语音识别）的语言就是视频说的语言：比标题文字更可靠（中文标题的英语教学视频不会被当成中文视频）
      const asr = prefer.length ? null : list.find((t) => t.auto && t.lang)
      const spoken = hint.spoken || (asr ? asr.lang : '') || (titleScript === 'zh' ? 'zh' : titleScript === 'ja' ? 'ja' : titleScript === 'ko' ? 'ko' : '')
      if (spoken) {
        const m = list.find((t) => !t.auto && sameLang(t.lang, spoken)) || list.find((t) => sameLang(t.lang, spoken))
        if (m) return m
      } else if (titleScript === 'latin') {
        const m = list.find((t) => !t.auto && sameLang(t.lang, 'en')) || list.find((t) => sameLang(t.lang, 'en'))
        if (m) return m
      }
      const nav = hint.title ? [] : [navigator?.language]
      const langs = [...prefer, ...nav, 'zh', 'en'].filter(Boolean).map((l) => String(l).toLowerCase())
      const match = (t, l) => t.lang.toLowerCase() === l || t.lang.toLowerCase().startsWith(l.split('-')[0])
      for (const l of langs) {
        const m = list.find((t) => !t.auto && match(t, l)) || list.find((t) => match(t, l))
        if (m) return m
      }
      return list.find((t) => !t.auto) || list[0]
    }

    // ---------- 重新断句 ----------

    const CJK = /[\u3040-\u30ff\u3400-\u9fff\uac00-\ud7af]/
    const SEG = { short: [16, 36], standard: [22, 48], long: [32, 72] }

    /** 把过长的一行按标点 / 空格拆成不超过上限的几段。 */
    function splitLong(text, maxCjk, maxLatin) {
      const t = String(text || '').replace(/\s+/g, ' ').trim()
      const cjk = CJK.test(t)
      const limit = cjk ? maxCjk : maxLatin
      if (t.length <= limit) return [t]
      // 先按句末标点拆，再按逗号 / 空格拆，再贪心合并到上限内
      const sentences = t.split(/(?<=[。！？!?；;…])|(?<=[.])\s+(?=[A-Z"'(])/).map((x) => x.trim()).filter(Boolean)
      const atoms = []
      for (const s of sentences) {
        if (s.length <= limit) atoms.push(s)
        else {
          const parts = s.split(/(?<=[，,、：:])\s*|(?<=\s)(?=(?:and|but|so|because|which|that|when|while|then|or)\s)/i).map((x) => x.trim()).filter(Boolean)
          for (const p of parts) {
            if (p.length <= limit) atoms.push(p)
            else if (cjk) for (let i = 0; i < p.length; i += limit) atoms.push(p.slice(i, i + limit))
            else {
              let cur = ''
              for (const w of p.split(' ')) {
                if (cur && (cur + ' ' + w).length > limit) (atoms.push(cur), (cur = w))
                else cur = cur ? cur + ' ' + w : w
              }
              if (cur) atoms.push(cur)
            }
          }
        }
      }
      const out = []
      let cur = ''
      for (const a of atoms) {
        const joined = cur ? (cjk ? cur + a : cur + ' ' + a) : a
        if (cur && joined.length > limit) (out.push(cur), (cur = a))
        else cur = joined
      }
      if (cur) out.push(cur)
      return out
    }

    /** 重新断句：[{t, text}] → [{t, text}]，一行拆成几行时开始时间按字数比例分配；已经够短的原样保留。 */
    function resegmentLines(lines, durationMs = 0, level = 'standard') {
      const [mc, ml] = SEG[level] || SEG.standard
      const list = lines || []
      const out = []
      list.forEach((l, i) => {
        const pieces = splitLong(l.text, mc, ml)
        if (pieces.length <= 1) return out.push({ ...l, text: pieces[0] ?? l.text })
        const next = list[i + 1]?.t ?? (durationMs > l.t ? durationMs : l.t + pieces.join('').length * 220)
        const span = Math.max(next - l.t, pieces.length * 800)
        const total = pieces.reduce((n, p) => n + p.length, 0) || 1
        let acc = 0
        for (const p of pieces) {
          out.push({ ...l, t: Math.round(l.t + (span * acc) / total), text: p })
          acc += p.length
        }
      })
      return out
    }


    Object.assign(C, { sliceJson, parseYouTubePage, pickTrack, scriptOf, splitLong, resegmentLines })
  }
})()
