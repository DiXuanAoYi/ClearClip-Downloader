// 视频摘要面板：按需注入的侧栏。再次注入 = 关闭。
// 字幕在页面里读取，交给本机的清影，由清影调用“设置 → AI”里的大模型；密钥不会进到扩展或网页里。
;(() => {
  if (globalThis.__ccVsum) return globalThis.__ccVsum.stop()
  const { mount, h, send, toast, api } = globalThis.ccUI
  const V = globalThis.ccVsum
  const Src = globalThis.ccVsumSource

  const CSS = `
    .panel { top: 0; right: 0; width: 400px; max-width: 100vw; height: 100vh; display: flex; flex-direction: column; background: #fbfaf8; color: #1d2420; border-left: 1px solid #cfd5d1; box-shadow: -8px 0 28px #0002; }
    .hd { display: flex; align-items: center; gap: 6px; padding: 8px 10px; border-bottom: 1px solid #e3e6e3; }
    .hd .t { flex: 1; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .hd button, .tools button { padding: 3px 8px; }
    .meta { padding: 6px 10px; font-size: 12px; color: #5d6862; border-bottom: 1px solid #e3e6e3; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
    .tabs { display: flex; border-bottom: 1px solid #e3e6e3; }
    .tabs button { flex: 1; background: transparent; color: #5d6862; border-radius: 0; border-bottom: 2px solid transparent; padding: 7px 0; font-weight: 400; }
    .tabs button.on { color: #b4561a; border-bottom-color: #b4561a; font-weight: 700; }
    .body { flex: 1; overflow: auto; padding: 10px; }
    .tools { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-bottom: 8px; }
    select, input[type=text], textarea { border: 1px solid #cfd5d1; border-radius: 6px; padding: 4px 6px; background: #fff; color: inherit; min-width: 0; }
    textarea { width: 100%; resize: vertical; }
    .card { background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; white-space: pre-wrap; word-break: break-word; }
    .chips { display: flex; gap: 5px; flex-wrap: wrap; margin: 6px 0; }
    .chip { background: #d9762b1f; color: #8a3f0c; border-radius: 10px; padding: 1px 8px; font-size: 12px; }
    .ln { display: flex; gap: 8px; padding: 3px 4px; border-radius: 4px; cursor: pointer; }
    .ln:hover { background: #d9762b14; }
    .ln.cur { background: #d9762b2e; }
    .ln .tm { color: #b4561a; font-variant-numeric: tabular-nums; flex: none; }
    .ln .tr { color: #5d6862; display: block; }
    .mute { color: #5d6862; font-size: 12px; }
    .warn { background: #fff4e5; border: 1px solid #f0c98e; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; }
    .bar { height: 4px; background: #e3e6e3; border-radius: 2px; overflow: hidden; margin: 6px 0; }
    .bar i { display: block; height: 100%; background: #d9762b; transition: width .3s; }
    .msg { margin-bottom: 8px; }
    .msg.u { text-align: right; }
    .msg .b { display: inline-block; text-align: left; max-width: 92%; padding: 6px 10px; border-radius: 10px; background: #fff; border: 1px solid #e3e6e3; white-space: pre-wrap; word-break: break-word; }
    .msg.u .b { background: #b4561a; color: #fff; border-color: #b4561a; }
    .ts { background: transparent; color: #b4561a; padding: 0 2px; font-weight: 700; }
    .ask { display: flex; gap: 6px; padding: 8px 10px; border-top: 1px solid #e3e6e3; }
    .ask input { flex: 1; }
    .svgbox { overflow: auto; border: 1px solid #e3e6e3; border-radius: 8px; background: #fff; max-height: 55vh; }
    h4 { margin: 10px 0 4px; font-size: 13px; }
    ul { margin: 4px 0; padding-left: 18px; }
    @media (prefers-color-scheme: dark) {
      .panel { background: #1d2420; color: #eef1ee; border-left-color: #3a443e; }
      .hd, .meta, .tabs, .ask { border-color: #3a443e; }
      .card, .msg .b, select, input[type=text], textarea { background: #262f2a; border-color: #3a443e; color: #eef1ee; }
      button.sec { background: #262f2a; color: #eef1ee; border-color: #3a443e; }
      .ln .tr, .meta, .mute { color: #a9b3ad; }
      .warn { background: #3a2f1e; border-color: #6b5530; }
    }
  `

  const ui = mount(CSS)
  const S = {
    data: null, // 当前文稿：{ title, url, lines, chapters, tracks, ... }
    tab: 'summary',
    status: null, // 清影的状态：{ ready, model, lang, transcribe, maxChars }
    prefs: { style: 'standard', lang: '', follow: true, tab: 'summary' },
    res: {}, // 本次的结果：summary / outline / cards / quotes / translated / rewrite
    chat: [],
    job: null, // { id, kind, done, total }
    busy: false,
    query: '',
    showTr: true,
    timer: 0,
  }

  const panel = h('div', { class: 'layer panel', role: 'complementary', 'aria-label': '视频摘要' })
  ui.add(panel)

  // ---------- 小工具 ----------

  const store = {
    async get(k, d) {
      try {
        return (await api.storage.local.get(k))[k] ?? d
      } catch {
        return d
      }
    },
    async set(k, v) {
      try {
        await api.storage.local.set({ [k]: v })
      } catch {
        // 存不了就算了
      }
    },
  }
  const savePrefs = () => store.set('vsumPrefs', { ...S.prefs, tab: S.tab })
  const outLang = () => S.prefs.lang.trim() || S.status?.lang || '中文'
  const video = () => [...document.querySelectorAll('video')].sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0] || null
  const isVideo = () => S.data && S.data.site !== 'article' && S.data.site !== 'manual'

  function seek(ms) {
    const v = video()
    if (v && isVideo()) {
      v.currentTime = ms / 1000
      v.play?.().catch(() => {})
    } else if (/^https?:/.test(S.data?.url || '') && S.data?.site !== 'article') window.open(V.timeLink(S.data.url, ms), '_blank', 'noopener')
  }

  /** 把文字里的 [m:ss] 变成能点的时间。 */
  function linkify(text) {
    const out = []
    let last = 0
    for (const m of String(text).matchAll(/\[(\d{1,2}(?::\d{2}){1,2})\]/g)) {
      out.push(text.slice(last, m.index))
      const ms = V.parseClock(m[1])
      out.push(h('button', { class: 'ts', text: `[${m[1]}]`, onclick: () => seek(ms) }))
      last = m.index + m[0].length
    }
    out.push(text.slice(last))
    return out
  }

  const tsBtn = (ms) => (isVideo() && ms >= 0 ? h('button', { class: 'ts', text: V.fmtClock(ms), onclick: () => seek(ms) }) : null)

  function copy(text, what = '内容') {
    navigator.clipboard.writeText(text).then(
      () => toast(ui, `已复制${what}`),
      () => toast(ui, '浏览器不让复制，请手动选中后复制', true),
    )
  }

  function saveFile(name, text, type = 'text/plain') {
    const a = document.createElement('a')
    a.href = URL.createObjectURL(new Blob([text], { type: `${type};charset=utf-8` }))
    a.download = name
    ui.add(a)
    a.click()
    setTimeout(() => (URL.revokeObjectURL(a.href), a.remove()), 1000)
  }

  const sel = (opts, value, onchange, title) => {
    const s = h('select', { title }, ...opts.map(([v, n]) => h('option', { value: v, text: n })))
    s.value = value
    s.onchange = () => onchange(s.value)
    return s
  }

  // ---------- 与清影通信 ----------

  async function ensureReady() {
    const r = await send({ type: 'cc-ai-status' })
    if (!r.ok) {
      S.status = null
      throw new Error(r.error)
    }
    S.status = r
    if (!r.ready) throw new Error('清影里还没有设置大模型。请打开清影 → 设置 → AI，填好接口地址、密钥和模型。')
    return r
  }

  /** 缓存键里用的行：只留时间和文字。 */
  function linesForReq() {
    const max = S.status?.maxChars || 600000
    const out = []
    let n = 0
    for (const l of S.data.lines) {
      n += l.text.length + 8
      if (n > max) break
      out.push({ t: l.t, text: l.text })
    }
    return out
  }

  async function cached(key) {
    const all = await store.get('vsumCache', {})
    return all[key]?.data
  }
  async function putCache(key, data) {
    const all = await store.get('vsumCache', {})
    all[key] = { ts: Date.now(), data }
    await store.set('vsumCache', V.pruneCache(all, 40, 3_000_000))
  }

  /** 提交 → 轮询。cache 为 false 时不读不写缓存（问答、改写）。 */
  async function run(kind, extra = {}, cache = true) {
    await ensureReady()
    const lang = outLang()
    const style = kind === 'summary' ? S.prefs.style : ''
    const key = cache ? V.cacheKey(S.data.lines, kind, style + (extra.format || ''), lang) : ''
    if (key) {
      const hit = await cached(key)
      if (hit) return hit
    }
    const req = { kind, title: S.data.title, lang, style, lines: linesForReq(), ...extra }
    const result = await submit(req, kind)
    if (key) putCache(key, result)
    return result
  }

  /** 提交一个任务并轮询到结束，返回结果；进度显示在面板顶部，可取消。 */
  async function submit(req, kind) {
    const sub = await send({ type: 'cc-ai-run', req })
    if (!sub.ok) throw new Error(sub.error)
    S.job = { id: sub.id, kind, done: 0, total: 0, note: '' }
    renderProgress()
    try {
      for (;;) {
        await new Promise((r) => setTimeout(r, 700))
        const j = await send({ type: 'cc-ai-job', id: S.job.id })
        if (!j.ok) throw new Error(j.error)
        S.job.done = j.done
        S.job.total = j.total
        S.job.note = j.note || ''
        renderProgress()
        if (j.state === 'done') return j.result
        if (j.state === 'error') throw new Error(j.error || '失败了')
        if (j.state === 'cancelled') throw new Error('已取消')
      }
    } finally {
      S.job = null
      renderProgress()
    }
  }

  /** 没有字幕：让清影取音频、用语音识别转成带时间的文稿，之后和有字幕的视频一样处理。 */
  async function transcribeNow() {
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    if (S.status?.transcribe === false) return toast(ui, '清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp 或填写接口', true, 6000)
    const url = S.data?.url || location.href
    const title = S.data?.title || document.title
    S.busy = true
    render()
    try {
      const key = `tr:${url}`
      let r = await cached(key)
      if (!r) {
        r = await submit({ kind: 'transcribe', url, title }, 'transcribe')
        if (r?.lines?.length) putCache(key, r)
      }
      if (!r?.lines?.length) throw new Error('没有识别出任何语音')
      setData({ ...S.data, lines: r.lines, durationMs: r.durationMs || S.data?.durationMs || 0, track: r.track, noSubtitle: false, note: '', trackUrl: '', tracks: S.data?.tracks || [] })
      toast(ui, `转写完成，共 ${r.lines.length} 句`)
    } catch (e) {
      toast(ui, e.message, true, 7000)
    } finally {
      S.busy = false
      render()
    }
  }

  async function task(kind, extra, apply, cache = true) {
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    if (!S.data?.lines.length) return toast(ui, '还没有文稿', true)
    S.busy = true
    render()
    try {
      apply(await run(kind, extra, cache))
    } catch (e) {
      toast(ui, e.message, true, 5200)
    } finally {
      S.busy = false
      render()
    }
  }

  // ---------- 各个标签页 ----------

  const noData = () => {
    const d = S.data
    return h(
      'div',
      { class: 'warn' },
      d?.note || '还没有文稿',
      h('div', { class: 'tools', style: 'margin-top:6px' },
        d?.noSubtitle && h('button', { text: '用清影转写（语音识别）', title: S.status?.transcribe === false ? '清影里还没有配置语音转文字' : '取视频的音频，在清影里转成文字后再总结', onclick: transcribeNow }),
        h('button', { class: 'sec', text: '导入字幕文件', onclick: importFile }),
        h('button', { class: 'sec', text: '粘贴文字', onclick: () => (S.tab = 'more') && render() }),
      ),
    )
  }

  async function sendForTranscribe() {
    const r = await send({ type: 'cc-send-page' })
    toast(ui, r.ok ? (r.pairing ? '请在电脑上允许配对' : '已发送。下载完成后在清影的媒体库里点“转写”，再回来用文字稿') : r.error, !r.ok, 5200)
  }

  function importFile() {
    const inp = h('input', { type: 'file', accept: '.srt,.vtt,.txt,.md,.json', style: 'display:none' })
    inp.onchange = async () => {
      const f = inp.files?.[0]
      if (!f) return
      setData({ ...Src.fromText(await f.text(), f.name), url: S.data?.url || location.href, title: S.data?.title || f.name })
    }
    ui.add(inp)
    inp.click()
  }

  function tabSummary() {
    const r = S.res.summary
    const box = h('div')
    box.append(
      h('div', { class: 'tools' },
        sel(V.STYLES, S.prefs.style, (v) => ((S.prefs.style = v), savePrefs()), '摘要风格'),
        h('input', { type: 'text', size: 8, placeholder: S.status?.lang || '输出语言', value: S.prefs.lang, title: '输出语言，留空用清影设置里的目标语言', onchange: (e) => ((S.prefs.lang = e.target.value), savePrefs()) }),
        h('button', { text: r ? '重新生成' : '生成摘要', disabled: S.busy, onclick: () => task('summary', {}, (x) => (S.res.summary = x)) }),
      ),
    )
    if (!r) {
      box.append(h('div', { class: 'mute', text: '读取字幕后，由清影用你设置的大模型生成：总结、要点、关键词和章节。长视频会自动分段处理。' }))
      return box
    }
    box.append(h('h4', { text: '总结' }), h('div', { class: 'card', text: r.summary }))
    if (r.keywords?.length) box.append(h('div', { class: 'chips' }, ...r.keywords.map((k) => h('span', { class: 'chip', text: k }))))
    if (r.points?.length) box.append(h('h4', { text: '要点' }), h('ul', {}, ...r.points.map((p) => h('li', {}, ...linkify(p)))))
    box.append(h('div', { class: 'tools' }, h('button', { class: 'sec', text: '复制 Markdown', onclick: () => copy(V.toMarkdown(exportData()), ' Markdown') })))
    return box
  }

  function chapterList() {
    const ai = S.res.summary?.chapters || []
    return ai.length ? { list: ai, from: 'AI 章节' } : S.data.chapters?.length ? { list: S.data.chapters, from: '视频简介里的章节' } : { list: [], from: '' }
  }

  function tabChapters() {
    const { list, from } = chapterList()
    if (!list.length) return h('div', { class: 'mute', text: '这个视频的简介里没有章节。先在“摘要”里生成，会得到 AI 章节。' })
    const now = video() && isVideo() ? V.chapterAt(list, video().currentTime * 1000) : -1
    return h('div', {},
      h('div', { class: 'mute', text: `来源：${from}` }),
      ...list.map((c, i) => h('div', { class: `ln${i === now ? ' cur' : ''}`, 'data-ch': i, onclick: () => seek(c.startMs) }, h('span', { class: 'tm', text: V.fmtClock(c.startMs) }), h('span', { text: c.title }))),
    )
  }

  function tabTranscript() {
    const d = S.data
    const hits = S.query.trim() ? new Set(V.searchLines(d.lines, S.query)) : null
    const tr = S.res.translated
    const body = h('div', { id: 'lines' })
    d.lines.forEach((l, i) => {
      if (hits && !hits.has(i)) return
      body.append(h('div', { class: 'ln', 'data-i': i, onclick: () => seek(l.t) },
        isVideo() && h('span', { class: 'tm', text: V.fmtClock(l.t) }),
        h('span', {}, l.text, tr && S.showTr && tr[i] ? h('span', { class: 'tr', text: tr[i] }) : null)))
    })
    return h('div', {},
      h('div', { class: 'tools' },
        h('input', { type: 'text', placeholder: '搜索文稿', value: S.query, style: 'flex:1', oninput: (e) => ((S.query = e.target.value), (S.keepFocus = true), render()) }),
        hits && h('span', { class: 'mute', text: `${hits.size} 处` }),
      ),
      h('div', { class: 'tools' },
        isVideo() && h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.follow, onchange: (e) => ((S.prefs.follow = e.target.checked), savePrefs()) }), ' 跟随播放'),
        h('button', { class: 'sec', text: tr ? (S.showTr ? '隐藏译文' : '显示译文') : `翻译成${outLang()}`, disabled: S.busy, onclick: () => (tr ? ((S.showTr = !S.showTr), render()) : task('translate', {}, (x) => (S.res.translated = d.lines.map((_, i) => x.lines?.[i]?.text || '')))) }),
        h('button', { class: 'sec', text: '复制', onclick: () => copy(V.toTxt(d.lines), '文稿') }),
        isVideo() && h('button', { class: 'sec', text: '.srt', onclick: () => saveFile(V.fileName(d.title, 'srt'), V.toSrt(d.lines)) }),
        h('button', { class: 'sec', text: '.txt', onclick: () => saveFile(V.fileName(d.title, 'txt'), V.toTxt(d.lines)) }),
      ),
      body,
    )
  }

  async function savePng(svg, name) {
    const w = Number(svg.match(/width="([\d.]+)"/)?.[1]) || 800
    const hh = Number(svg.match(/height="([\d.]+)"/)?.[1]) || 600
    const img = new Image()
    img.src = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }))
    await img.decode().catch(() => {})
    const c = document.createElement('canvas')
    c.width = w * 2
    c.height = hh * 2
    const g = c.getContext('2d')
    g.scale(2, 2)
    g.drawImage(img, 0, 0, w, hh)
    URL.revokeObjectURL(img.src)
    c.toBlob((b) => {
      if (!b) return toast(ui, '图片太大，导出失败，请改用 SVG', true)
      const a = document.createElement('a')
      a.href = URL.createObjectURL(b)
      a.download = name
      a.click()
      setTimeout(() => URL.revokeObjectURL(a.href), 1000)
    }, 'image/png')
  }

  function tabMindmap() {
    const o = S.res.outline
    const box = h('div', {}, h('div', { class: 'tools' }, h('button', { text: o ? '重新生成' : '生成思维导图', disabled: S.busy, onclick: () => task('mindmap', {}, (x) => (S.res.outline = x.outline)) })))
    if (!o) return (box.append(h('div', { class: 'mute', text: '生成层级大纲，并画成可导出的导图。' })), box)
    const svg = V.mindmapSvg(V.layoutMindmap(V.outlineToTree(o, S.data.title)), matchMedia('(prefers-color-scheme: dark)').matches)
    const view = h('div', { class: 'svgbox' })
    view.append(new DOMParser().parseFromString(svg, 'image/svg+xml').documentElement)
    box.append(view,
      h('div', { class: 'tools', style: 'margin-top:8px' },
        h('button', { class: 'sec', text: '保存 SVG', onclick: () => saveFile(V.fileName(S.data.title, 'svg'), V.mindmapSvg(V.layoutMindmap(V.outlineToTree(o, S.data.title))), 'image/svg+xml') }),
        h('button', { class: 'sec', text: '保存 PNG', onclick: () => savePng(V.mindmapSvg(V.layoutMindmap(V.outlineToTree(o, S.data.title))), V.fileName(S.data.title, 'png')) }),
        h('button', { class: 'sec', text: '复制大纲', onclick: () => copy(o, '大纲') }),
      ),
      h('pre', { class: 'card', text: o }))
    return box
  }

  function tabAsk() {
    const box = h('div')
    if (!S.chat.length) {
      box.append(h('div', { class: 'mute', text: '针对这个视频提问。回答里的 [m:ss] 可以点，直接跳到那一刻。' }),
        h('div', { class: 'chips' }, ...V.SUGGESTIONS.map((q) => h('button', { class: 'sec', text: q, onclick: () => ask(q) }))))
    }
    for (const m of S.chat) box.append(h('div', { class: `msg${m.role === 'user' ? ' u' : ''}` }, h('div', { class: 'b' }, ...linkify(m.text))))
    if (S.chat.length) box.append(h('div', { class: 'tools' }, h('button', { class: 'sec', text: '清空对话', onclick: () => ((S.chat = []), render()) }), h('button', { class: 'sec', text: '复制对话', onclick: () => copy(S.chat.map((m) => `${m.role === 'user' ? '问' : '答'}：${m.text}`).join('\n\n'), '对话') })))
    return box
  }

  async function ask(q) {
    q = String(q || '').trim()
    if (!q) return
    const history = S.chat.slice(-6).map((m) => ({ role: m.role, text: m.text }))
    S.chat.push({ role: 'user', text: q })
    await task('ask', { question: q, history }, (x) => S.chat.push({ role: 'assistant', text: x.answer }), false)
    if (S.chat.at(-1)?.role === 'user') S.chat.pop()
    render()
  }

  function tabMore() {
    const box = h('div')
    const cards = S.res.cards
    const quotes = S.res.quotes
    box.append(
      h('h4', { text: '学习卡片（可导入 Anki）' }),
      h('div', { class: 'tools' },
        h('button', { text: cards ? '重新生成' : '生成问答卡片', disabled: S.busy, onclick: () => task('cards', {}, (x) => (S.res.cards = x.cards)) }),
        cards && h('button', { class: 'sec', text: '导出 CSV', onclick: () => saveFile(V.fileName(S.data.title, 'tsv'), V.cardsToCsv(cards)) }),
      ),
      ...(cards || []).map((c) => h('div', { class: 'card' }, h('b', { text: c.q }), '\n', c.a, ' ', tsBtn(c.t))),
      h('h4', { text: '金句' }),
      h('div', { class: 'tools' }, h('button', { text: quotes ? '重新生成' : '提取金句', disabled: S.busy, onclick: () => task('quotes', {}, (x) => (S.res.quotes = x.quotes)) })),
      ...(quotes || []).map((q) => h('div', { class: 'card' }, `“${q.text}” `, tsBtn(q.t))),
      h('h4', { text: '改写成其他形式' }),
      h('div', { class: 'tools' }, ...V.REWRITES.map(([f, n]) => h('button', { class: 'sec', text: n, disabled: S.busy || !S.res.summary, title: S.res.summary ? '' : '先在“摘要”里生成', onclick: () => task('rewrite', { format: f, text: V.toMarkdown(exportData()) }, (x) => (S.res.rewrite = { name: n, text: x.text }), false) }))),
      S.res.rewrite && h('div', {}, h('div', { class: 'card', text: S.res.rewrite.text }), h('button', { class: 'sec', text: `复制${S.res.rewrite.name}`, onclick: () => copy(S.res.rewrite.text, S.res.rewrite.name) })),
      h('h4', { text: '导出' }),
      h('div', { class: 'tools' },
        h('button', { class: 'sec', text: '下载 Markdown', onclick: () => saveFile(V.fileName(S.data.title, 'md'), V.toMarkdown(exportData(true)), 'text/markdown') }),
        h('button', { class: 'sec', text: '复制到剪贴板（含文稿）', onclick: () => copy(V.toMarkdown(exportData(true)), ' Markdown') }),
        h('button', { class: 'sec', text: '存进清影的便签', onclick: saveNote }),
        h('button', { class: 'sec', text: '发到 Obsidian', onclick: toObsidian }),
      ),
      h('h4', { text: '没有字幕 / 想换文稿' }),
      h('div', { class: 'tools' },
        isVideo() && h('button', { class: 'sec', text: '用清影转写（语音识别）', onclick: transcribeNow }),
        isVideo() && h('button', { class: 'sec', text: '只把视频发给清影下载', onclick: sendForTranscribe }),
        h('button', { class: 'sec', text: '导入 .srt / .vtt / .txt', onclick: importFile }),
        S.data.tracks?.length > 1 && sel(S.data.tracks.map((t) => [t.url, `${t.name}${t.auto ? '（自动）' : ''}`]), S.data.trackUrl || S.data.tracks[0].url, reloadTrack, '字幕轨'),
        h('button', { class: 'sec', text: '总结选中的文字', onclick: () => loadArticleFromSelection() }),
      ),
      h('textarea', { rows: 4, placeholder: '或者把文字稿 / 字幕粘贴到这里，再点“使用”', id: 'paste' }),
      h('div', { class: 'tools' }, h('button', { class: 'sec', text: '使用粘贴的文字', onclick: () => {
        const t = ui.root.getElementById('paste')?.value
        if (!t?.trim()) return toast(ui, '先粘贴文字', true)
        setData({ ...Src.fromText(t), url: S.data.url, title: S.data.title })
      } })),
    )
    return box
  }

  function exportData(withLines = false) {
    const r = S.res.summary || {}
    return {
      title: S.data.title,
      url: isVideo() ? S.data.url : '',
      result: { ...r, chapters: r.chapters?.length ? r.chapters : S.data.chapters },
      quotes: S.res.quotes,
      cards: S.res.cards,
      outline: S.res.outline,
      lines: S.data.lines,
      includeTranscript: withLines,
    }
  }

  async function saveNote() {
    const r = await send({ type: 'cc-note', text: V.toMarkdown(exportData()) })
    toast(ui, r.ok ? (r.pairing ? '请在电脑上允许配对' : '已存进清影的“收到的文件”') : r.error, !r.ok, 4200)
  }

  /** Obsidian：obsidian://new?name=…&content=…（内容太长时改为复制 + 只建空笔记）。 */
  function toObsidian() {
    const md = V.toMarkdown(exportData())
    const name = V.fileName(S.data.title, '').replace(/\.$/, '')
    const url = `obsidian://new?name=${encodeURIComponent(name)}&content=${encodeURIComponent(md)}`
    if (url.length > 7000) {
      copy(md, ' Markdown')
      location.href = `obsidian://new?name=${encodeURIComponent(name)}&clipboard`
    } else location.href = url
  }

  async function reloadTrack(url) {
    toast(ui, '正在读取字幕…')
    setData(await Src.load(url))
  }

  function loadArticleFromSelection() {
    const a = Src.loadArticle()
    if (!a.lines.length) return toast(ui, '页面上没有可总结的文字', true)
    setData(a)
  }

  // ---------- 框架 ----------

  const TABS = [['summary', '摘要'], ['chapters', '章节'], ['transcript', '文稿'], ['mindmap', '导图'], ['ask', '问答'], ['more', '更多']]

  function renderProgress() {
    const el = ui.root.getElementById('prog')
    if (!el) return
    el.replaceChildren()
    if (!S.job) return
    const pct = S.job.total ? Math.round((S.job.done / S.job.total) * 100) : 5
    el.append(
      h('div', { class: 'mute' }, `${S.job.note || '清影正在处理…'}${S.job.total ? (S.job.kind === 'transcribe' ? ` ${S.job.done}%` : ` ${S.job.done}/${S.job.total}`) : ''} `, h('button', { class: 'sec', text: '取消', onclick: () => send({ type: 'cc-ai-cancel', id: S.job.id }) })),
      h('div', { class: 'bar' }, h('i', { style: `width:${pct}%` })),
    )
  }

  function render() {
    const keepScroll = ui.root.querySelector('.body')?.scrollTop || 0
    const d = S.data
    const children = [
      h('div', { class: 'hd' },
        h('span', { class: 't', title: d?.title || '', text: d ? `摘要 · ${d.title}` : '视频摘要' }),
        h('button', { class: 'sec', text: '重新读取', title: '重新读取当前页面的字幕（切换视频后用）', onclick: () => start() }),
        h('button', { class: 'sec', text: '✕', title: '关闭', 'aria-label': '关闭', onclick: stop }),
      ),
    ]
    if (d) {
      children.push(h('div', { class: 'meta' },
        d.lines.length ? `${d.lines.length} 段` : '无文稿',
        d.track && `· ${d.track}`,
        d.durationMs > 0 && `· ${V.fmtClock(d.durationMs)}`,
        S.status && !S.status.ready && h('span', { style: 'color:#a8362d', text: '· 清影里未设置大模型' })))
      children.push(h('div', { class: 'tabs', role: 'tablist' }, ...TABS.map(([id, n]) => h('button', { class: S.tab === id ? 'on' : '', role: 'tab', 'aria-selected': S.tab === id, text: n, onclick: () => ((S.tab = id), savePrefs(), render()) }))))
    }
    const body = h('div', { class: 'body' }, h('div', { id: 'prog' }))
    if (!d) body.append(h('div', { class: 'mute', text: '正在读取字幕…' }))
    else {
      if (!d.lines.length && S.tab !== 'more') body.append(noData())
      else {
        const fn = { summary: tabSummary, chapters: tabChapters, transcript: tabTranscript, mindmap: tabMindmap, ask: tabAsk, more: tabMore }[S.tab]
        body.append(fn())
      }
    }
    children.push(body)
    if (d && S.tab === 'ask' && d.lines.length) {
      const inp = h('input', { type: 'text', placeholder: '问点什么…', id: 'q', onkeydown: (e) => e.key === 'Enter' && !e.isComposing && go() })
      const go = () => {
        const q = inp.value
        inp.value = ''
        ask(q)
      }
      children.push(h('div', { class: 'ask' }, inp, h('button', { text: '发送', disabled: S.busy, onclick: go })))
    }
    panel.replaceChildren(...children)
    renderProgress()
    ui.root.querySelector('.body').scrollTop = keepScroll
    if (S.keepFocus) {
      S.keepFocus = false
      const i = ui.root.querySelector('.body input[type=text]')
      if (i) {
        i.focus()
        i.setSelectionRange(i.value.length, i.value.length)
      }
    }
  }

  /** 跟随播放：高亮当前行 / 章节。 */
  function tick() {
    const v = video()
    if (!v || !S.data || !isVideo()) return
    const ms = v.currentTime * 1000
    if (S.tab === 'transcript') {
      const i = V.lineAt(S.data.lines, ms)
      const prev = ui.root.querySelector('.ln.cur')
      const cur = ui.root.querySelector(`.ln[data-i="${i}"]`)
      if (prev !== cur) {
        prev?.classList.remove('cur')
        cur?.classList.add('cur')
        if (S.prefs.follow && cur && !S.query) cur.scrollIntoView({ block: 'center', behavior: 'smooth' })
      }
    } else if (S.tab === 'chapters') {
      const i = V.chapterAt(chapterList().list, ms)
      const cur = ui.root.querySelector(`.ln[data-ch="${i}"]`)
      if (cur && !cur.classList.contains('cur')) {
        ui.root.querySelector('.ln.cur')?.classList.remove('cur')
        cur.classList.add('cur')
      }
    }
  }

  function setData(d) {
    S.data = d
    S.res = {}
    S.chat = []
    S.query = ''
    render()
    // 同一份文稿以前算过的摘要直接带出来（不需要大模型）
    const key = V.cacheKey(d.lines, 'summary', S.prefs.style, outLang())
    if (d.lines.length) cached(key).then((hit) => hit && S.data === d && !S.res.summary && ((S.res.summary = hit), render()))
  }

  async function start() {
    S.data = null
    render()
    try {
      setData(await Src.load())
    } catch (e) {
      setData({ title: document.title, url: location.href, site: 'html5', lines: [], chapters: [], tracks: [], note: `读取失败：${e.message}` })
    }
    ensureReady().catch(() => {}).finally(() => S.data && render())
  }

  function stop() {
    clearInterval(S.timer)
    if (S.job) send({ type: 'cc-ai-cancel', id: S.job.id })
    ui.remove()
    delete globalThis.__ccVsum
  }

  globalThis.__ccVsum = { stop }
  S.timer = setInterval(tick, 500)
  store.get('vsumPrefs', null).then((p) => {
    if (p) {
      S.prefs = { ...S.prefs, ...p }
      S.tab = TABS.some(([id]) => id === p.tab) ? p.tab : 'summary'
    }
    return start()
  })
})()
