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
  /** 作者 / UP 主 / 频道名：先认页面里标准的写法，再认各站常见的位置。 */
  function pageOwner() {
    const pick = (sel, attr) => {
      const el = document.querySelector(sel)
      return String((attr ? el?.getAttribute(attr) : el?.textContent) || '').replace(/\s+/g, ' ').trim()
    }
    return (
      pick('.up-name') || pick('.up-info--right .up-name') || pick('a.up-name') || pick('#owner #channel-name a') || pick('ytd-channel-name a') ||
      pick('[itemprop="author"] [itemprop="name"]', 'content') || pick('[itemprop="author"] [itemprop="name"]') || pick('link[itemprop="name"]', 'content') ||
      pick('meta[name="author"]', 'content') || ''
    ).slice(0, 80)
  }
  const base = (site) => ({ site, owner: pageOwner(), title: document.title.replace(/\s*[-_|–]\s*(YouTube|哔哩哔哩.*|bilibili)\s*$/i, '').trim(), url: location.href, lines: [], chapters: [], durationMs: 0, track: '', tracks: [], note: '' })

  // ---------- YouTube ----------

  async function loadYouTube(id, wanted) {
    const out = base('youtube')
    // 页面里的数据在站内跳转后会过期，所以重新取一次当前网址
    const page = V.parseYouTubePage(await getText(location.href))
    if (page.title) out.title = page.title
    out.durationMs = page.lengthSec * 1000
    out.chapters = V.parseDescriptionChapters(page.description)
    out.tracks = page.tracks
    out.spoken = page.spoken
    const pick = (wanted && page.tracks.find((t) => t.url === wanted)) || V.pickTrack(page.tracks, [], { spoken: page.spoken, title: out.title })
    if (!pick) {
      out.note = '这个视频没有字幕'
      return out
    }
    out.track = pick.name || pick.lang
    out.trackUrl = pick.url
    out.trackLang = pick.lang
    out.trackAuto = !!pick.auto
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

  /**
   * B 站字幕文件在另一个域名（hdslb.com）上，带 Cookie 的跨域请求会被浏览器拦成 “Failed to fetch”，
   * 而字幕地址本身已带签名，不需要 Cookie。所以先不带 Cookie 取；不行再带；最后交给后台代取。
   */
  async function getSubtitleFile(url) {
    const u = String(url).replace(/^\/\//, 'https://').replace(/^http:/, 'https:')
    let lastErr
    for (const credentials of ['omit', 'include']) {
      try {
        const t = (await getText(u, { credentials })).trim()
        if (t) return JSON.parse(t)
        lastErr = new Error('服务器返回了空内容')
      } catch (e) {
        lastErr = e
      }
    }
    const r = await globalThis.ccUI?.send?.({ type: 'cc-fetch-sub', url: u })
    if (r?.ok && String(r.text || '').trim()) return JSON.parse(r.text)
    throw lastErr || new Error(r?.error || '取不到字幕')
  }

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
    const pick = (wanted && out.tracks.find((t) => t.url === wanted)) || V.pickTrack(out.tracks, [], { title: out.title })
    if (!pick) {
      out.note = '没有取到字幕（B 站的 AI 字幕需要登录后才返回；也可能这个视频没有字幕）'
      return out
    }
    out.track = pick.name
    out.trackUrl = pick.url
    try {
      out.lines = V.normalizeLines(V.parseBilibili(await getSubtitleFile(pick.url)))
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
    const pick = (wanted && out.tracks.find((t) => t.url === wanted)) || V.pickTrack(out.tracks, [], { title: out.title })
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
    // 文件里有真正的时间戳（不是全 0）
    out.timed = out.lines.some((l) => l.t > 0)
    return out
  }

  // ---------- 评论 ----------

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  /** 翻页间隔：0.7~1.5 秒随机，每 8 页多歇 3 秒，像正常滚动评论区，不整点连发。 */
  const pace = (i) => sleep(700 + Math.random() * 800 + (i % 8 === 7 ? 3000 : 0))
  /** 被站点限流 / 风控：带 block 标记，界面据此进入冷却，不再重试。 */
  const blocked = (msg) => Object.assign(new Error(msg), { block: true })
  const pageCap = (count, per) => Math.min(60, Math.ceil(count / per) + 3)

  async function ytComments(opt, on, stop) {
    const html = await getText(location.href)
    const ver = (html.match(/"INNERTUBE_CONTEXT_CLIENT_VERSION":"([^"]+)"/) || [])[1] || '2.20250101.00.00'
    const key = (html.match(/"INNERTUBE_API_KEY":"([^"]+)"/) || [])[1] || ''
    const si = html.indexOf('"sectionIdentifier":"comment-item-section"')
    const ti = si > 0 ? html.lastIndexOf('"token":"', si) : -1
    if (ti < 0) throw new Error('读不到评论入口（评论可能已关闭，或页面还没加载完）')
    let token = html.slice(ti + 9, html.indexOf('"', ti + 9))
    try {
      token = JSON.parse(`"${token}"`)
    } catch {
      // 保持原样
    }
    const post = async (tok) => {
      const r = await fetch(`/youtubei/v1/next?prettyPrint=false${key ? `&key=${key}` : ''}`, {
        method: 'POST',
        credentials: 'include',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ context: { client: { clientName: 'WEB', clientVersion: ver, hl: 'zh-CN' } }, continuation: tok }),
      })
      if (r.status === 429 || r.status === 403) throw blocked(`YouTube 暂时限制了评论读取（HTTP ${r.status}）`)
      if (!r.ok) throw new Error(`HTTP ${r.status}`)
      return V.parseYtComments(await r.json())
    }
    let page = await post(token)
    if (opt.sort === 'new' && page.sorts[1]) page = await post(page.sorts[1].token)
    const all = []
    for (let i = 0, cap = pageCap(opt.count, 20); i < cap; i++) {
      all.push(...page.items)
      on(Math.min(all.length, opt.count))
      if (all.length >= opt.count || !page.next || stop()) break
      await pace(i)
      page = await post(page.next)
    }
    return all
  }

  async function biliComments(vid, opt, on, stop) {
    let aid = /^av(\d+)$/i.test(vid.id) ? vid.id.slice(2) : ''
    if (!aid) aid = String((await getJson(`https://api.bilibili.com/x/web-interface/view?bvid=${vid.id}`)).data?.aid || '')
    if (!aid) throw new Error('取不到视频编号')
    const all = []
    let next = 0
    for (let i = 0, cap = pageCap(opt.count, 20); i < cap; i++) {
      const j = await getJson(`https://api.bilibili.com/x/v2/reply/main?type=1&oid=${aid}&mode=${opt.sort === 'new' ? 2 : 3}&next=${next}&ps=20`)
      if ([-412, -352, -799, -509].includes(j.code)) throw blocked('哔哩哔哩暂时限制了评论读取（风控）')
      if (j.code && j.code !== 0) throw new Error(j.code === 12002 || j.code === 12061 ? '这个视频的评论区已关闭' : j.message || `B 站返回错误 ${j.code}`)
      const p = V.parseBiliComments(j)
      all.push(...p.items)
      on(Math.min(all.length, opt.count))
      if (all.length >= opt.count || p.end || stop()) break
      next = p.next
      await pace(i)
    }
    return all
  }

  /** 读取评论：opt = { count, sort: 'hot'|'new' }；on(n) 报告进度，stop() 返回 true 即停。 */
  async function loadComments(opt = {}, on = () => {}, stop = () => false) {
    const o = { count: Math.min(2000, Math.max(20, Number(opt.count) || 300)), sort: opt.sort === 'new' ? 'new' : 'hot' }
    const vid = V.videoIdOf(location.href)
    let list
    if (vid?.site === 'youtube') list = await ytComments(o, on, stop)
    else if (vid?.site === 'bilibili') list = await biliComments(vid, o, on, stop)
    else throw new Error('这个站点暂不支持直接读取评论（目前支持 YouTube 和哔哩哔哩）')
    const out = V.cleanComments(list).slice(0, o.count)
    if (!out.length) throw new Error('没有读到评论（可能还没有人评论，或评论区已关闭）')
    return out
  }

  globalThis.ccVsumSource = { load, fromText, loadArticle, loadComments }
})()
