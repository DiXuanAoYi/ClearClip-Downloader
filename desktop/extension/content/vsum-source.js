// 取得当前页面的文稿：YouTube / B 站的字幕、网页播放器自带的字幕轨、手动导入的字幕文件、没有视频时的文章正文。
// 只在页面里读取，不会把任何内容发给第三方；之后由面板交给本机的清影处理。
;(() => {
  if (globalThis.ccVsumSource) return
  const V = globalThis.ccVsum

  const getText = async (url, opt = {}) => {
    const r = await fetch(url, { credentials: 'include', ...opt })
    if (!r.ok) throw new Error(`HTTP ${r.status}`)
    return r.text()
  }
  const getJson = async (url) => {
    const t = (await getText(url)).trim()
    if (!t) throw new Error('服务器返回了空内容')
    return JSON.parse(t)
  }

  /** 结果统一成：{ site, title, url, lines, chapters, durationMs, track, tracks, note } */
  const base = (site) => ({ site, title: document.title.replace(/\s*[-_|–]\s*(YouTube|哔哩哔哩.*|bilibili)\s*$/i, '').trim(), url: location.href, lines: [], chapters: [], durationMs: 0, track: '', tracks: [], note: '' })

  // ---------- YouTube ----------

  async function loadYouTube(id, wanted) {
    const out = base('youtube')
    // 页面里的数据在站内跳转后会过期，所以重新取一次当前网址
    const page = V.parseYouTubePage(await getText(location.href))
    if (page.title) out.title = page.title
    out.durationMs = page.lengthSec * 1000
    out.chapters = V.parseDescriptionChapters(page.description)
    out.tracks = page.tracks
    const pick = (wanted && page.tracks.find((t) => t.url === wanted)) || V.pickTrack(page.tracks)
    if (!pick) {
      out.note = '这个视频没有字幕'
      return out
    }
    out.track = pick.name || pick.lang
    out.trackUrl = pick.url
    out.lines = V.normalizeLines(await fetchYouTubeTrack(pick.url))
    if (!out.lines.length) {
      out.note = 'YouTube 没有返回这条字幕的内容（常见于需要验证令牌的视频）'
      out.fetchFailed = true
    }
    return out
  }

  /** 字幕地址有时返回 200 却是空内容；依次试 json3 → vtt → 原始格式，都读不到就返回空数组。 */
  async function fetchYouTubeTrack(trackUrl) {
    for (const fmt of ['json3', 'vtt', '']) {
      try {
        const url = new URL(trackUrl)
        if (fmt) url.searchParams.set('fmt', fmt)
        else url.searchParams.delete('fmt')
        const text = (await getText(url.href)).trim()
        if (!text) continue
        let cues = []
        if (text.startsWith('{')) cues = V.parseJson3(JSON.parse(text))
        else if (text.startsWith('<')) cues = V.parseTimedTextXml(text)
        else cues = V.parseSubtitleText(text)
        if (cues.length) return cues
      } catch {
        // 换下一种格式
      }
    }
    return []
  }

  // ---------- 哔哩哔哩 ----------

  async function loadBilibili(vid, wanted) {
    const out = base('bilibili')
    const info = (await getJson(`https://api.bilibili.com/x/web-interface/view?bvid=${vid.id}`)).data
    if (!info) throw new Error('取不到视频信息')
    const part = info.pages?.[(vid.part || 1) - 1] || info.pages?.[0]
    out.title = info.title + (info.pages?.length > 1 && part?.part ? ` · P${vid.part || 1} ${part.part}` : '')
    out.durationMs = (part?.duration || info.duration || 0) * 1000
    const cid = part?.cid || info.cid
    let subs = []
    for (const path of ['x/player/wbi/v2', 'x/player/v2']) {
      try {
        const d = (await getJson(`https://api.bilibili.com/${path}?bvid=${vid.id}&cid=${cid}`)).data
        subs = d?.subtitle?.subtitles || []
        if (subs.length) break
      } catch {
        // 换下一个接口
      }
    }
    out.tracks = subs.map((s) => ({ url: s.subtitle_url.replace(/^\/\//, 'https://'), lang: s.lan, name: s.lan_doc, auto: !!s.ai_type }))
    const pick = (wanted && out.tracks.find((t) => t.url === wanted)) || V.pickTrack(out.tracks)
    if (!pick) {
      out.note = '没有取到字幕（B 站的 AI 字幕需要登录后才返回；也可能这个视频没有字幕）'
      return out
    }
    out.track = pick.name
    out.trackUrl = pick.url
    try {
      out.lines = V.normalizeLines(V.parseBilibili(await getJson(pick.url)))
    } catch (e) {
      out.note = `这条字幕读不出来（${e.message}）`
      out.fetchFailed = true
    }
    return out
  }

  // ---------- 网页播放器自带的字幕轨 ----------

  async function loadHtmlTracks(wanted) {
    const out = base('html5')
    const video = [...document.querySelectorAll('video')].sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0]
    if (!video) return out
    out.durationMs = Number.isFinite(video.duration) ? Math.round(video.duration * 1000) : 0
    const list = [...video.textTracks].filter((t) => t.kind === 'subtitles' || t.kind === 'captions')
    out.tracks = list.map((t, i) => ({ url: `texttrack:${i}`, lang: t.language || '', name: t.label || t.language || `字幕 ${i + 1}`, auto: false }))
    const pick = (wanted && out.tracks.find((t) => t.url === wanted)) || V.pickTrack(out.tracks)
    if (!pick) return out
    const tt = list[Number(pick.url.split(':')[1])]
    const prev = tt.mode
    if (tt.mode === 'disabled') tt.mode = 'hidden'
    // 刚打开的字幕轨要等一下浏览器把字幕读进来
    for (let i = 0; i < 20 && !(tt.cues && tt.cues.length); i++) await new Promise((r) => setTimeout(r, 150))
    const cues = tt.cues ? V.fromTextTrackCues([...tt.cues]) : []
    if (prev === 'disabled') tt.mode = 'disabled'
    out.track = pick.name
    out.trackUrl = pick.url
    out.lines = V.normalizeLines(cues)
    return out
  }

  // ---------- 文章 / 选中的文字 ----------

  /** 选中了文字就用选中的，否则取 <article> / <main> / 最长的一块正文。 */
  function pageText() {
    const sel = String(getSelection?.() || '').trim()
    if (sel.length > 80) return { text: sel, picked: 'selection' }
    const cands = [...document.querySelectorAll('article, main, [role=main], .post, .article, .content, #content')]
    let best = ''
    for (const el of cands) {
      const t = (el.innerText || '').trim()
      if (t.length > best.length) best = t
    }
    if (best.length < 300) best = (document.body?.innerText || '').trim()
    return { text: best.replace(/\n{3,}/g, '\n\n'), picked: 'page' }
  }

  function loadArticle() {
    const out = base('article')
    const { text, picked } = pageText()
    out.text = text
    out.note = picked === 'selection' ? '使用选中的文字' : ''
    // 文章没有时间轴：按段落切成行，时间都是 0
    out.lines = text.split(/\n+/).map((s) => s.trim()).filter(Boolean).map((s) => ({ t: 0, text: s }))
    return out
  }

  // ---------- 入口 ----------

  /** 自动识别：先按网站，再看播放器字幕轨，最后当作文章。wanted 是想用的字幕轨网址。 */
  async function load(wanted = '') {
    const vid = V.videoIdOf(location.href)
    let first = null
    try {
      if (vid?.site === 'youtube') first = await loadYouTube(vid.id, wanted)
      else if (vid?.site === 'bilibili') first = await loadBilibili(vid, wanted)
    } catch (e) {
      first = { ...base(vid?.site || 'html5'), note: `读取字幕失败：${e.message}`, fetchFailed: true }
    }
    if (first?.lines.length) return first
    let html = null
    try {
      html = await loadHtmlTracks(wanted)
    } catch {
      html = null
    }
    if (html?.lines.length) return html
    const hasVideo = !!(first || html?.durationMs || document.querySelector('video'))
    if (hasVideo) {
      const r = first || html || base('html5')
      r.noSubtitle = true
      r.fetchFailed ||= !!first?.fetchFailed
      r.note ||= '没有找到字幕'
      return r
    }
    return loadArticle()
  }

  /** 手动导入的字幕文件 / 粘贴的文字：.srt .vtt 或带 [m:ss] 的文本；都不是就当纯文本按段落。 */
  function fromText(text, name = '') {
    const out = base('manual')
    if (name) out.title = name.replace(/\.[a-z0-9]+$/i, '')
    let cues = V.parseSubtitleText(text)
    if (!cues.length) {
      cues = String(text || '')
        .split(/\n+/)
        .map((s) => s.trim())
        .filter(Boolean)
        .map((s) => {
          const m = s.match(/^\[?(\d{1,2}(?::\d{2}){1,2})\]?\s+(.+)$/)
          return { t: m ? V.parseClock(m[1]) : 0, text: m ? m[2] : s }
        })
    }
    out.lines = V.normalizeLines(cues, { maxLen: 160 })
    if (!out.lines.length) out.lines = cues.map((c) => ({ t: c.t, text: c.text }))
    return out
  }

  globalThis.ccVsumSource = { load, fromText, loadArticle }
})()
