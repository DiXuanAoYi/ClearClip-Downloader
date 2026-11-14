// 视频摘要面板：按需注入的侧栏。再次注入（快捷键 Alt+Shift+S）= 展开 / 收起到右边；要关掉点 ✕。
// 字幕在页面里读取，交给本机的清影，由清影调用“设置 → AI”里的大模型；密钥不会进到扩展或网页里。
;(() => {
  if (globalThis.__ccVsum) return globalThis.__ccVsum.toggle()
  const { mount, h, send, toast, api } = globalThis.ccUI
  const V = globalThis.ccVsum
  const Src = globalThis.ccVsumSource
  const VS = globalThis.ccVSession

  const { CSS, DARK: DARK0 } = globalThis.ccVsumStyle
  const Dk = globalThis.ccVsumDock
  const DARK = DARK0 + Dk.DARK
  const ui = mount(CSS + Dk.CSS)
  let dock = null // 收到右边（vsum-dock.js）：面板准备好后创建
  // 外观：主题（跟随系统 / 浅色 / 深色）和字号，改了马上生效
  const theme = document.createElement('style')
  const fontStyle = document.createElement('style')
  ui.root.append(theme, fontStyle)
  const isDark = () => (S.prefs.theme === 'dark' ? true : S.prefs.theme === 'light' ? false : matchMedia('(prefers-color-scheme: dark)').matches)
  function applyAppearance() {
    theme.textContent = S.prefs.theme === 'dark' ? DARK : S.prefs.theme === 'light' ? '' : `@media (prefers-color-scheme: dark) {${DARK}}`
    const fs = Math.min(20, Math.max(11, Number(S.prefs.fs) || 13))
    fontStyle.textContent = `.panel { font-size: ${fs}px; }`
  }
  const S = {
    appLang: '', // 清影界面语言（zh / en / ja），语言下拉框按它显示
    data: null, // 当前文稿：{ title, url, lines, chapters, tracks, ... }
    tab: 'summary',
    status: null, // 清影的状态：{ ready, model, lang, transcribe, maxChars }
    prefs: { style: 'standard', lang: '', follow: true, tab: 'summary', autoOnline: true, autoTranscribe: true, srcV: 1, view: 'both', lead: 3, prompts: {}, theme: 'auto', fs: 13 },
    res: {}, // 本次的结果：summary / outline / flow / cards / quotes / translated / rewrite
    chat: [],
    hit: 0, // 当前搜索命中的第几处
    detached: false, // 用户手动滚走了，不再自动跟随
    job: null, // { id, kind, done, total }
    busy: false,
    query: '',
    showTr: true,
    notes: [], // 所有视频的笔记
    scope: 'video',
    noteQuery: '',
    sel: null,
    timer: 0,
    closed: false,
    restored: 0, // 内容是上次带出来的：保存时间
  }

  const panel = h('div', { class: 'layer panel', role: 'complementary', 'aria-label': '视频摘要' })
  ui.add(panel)
  // ---------- 框架 ----------

  const TABS = [['summary', '摘要'], ['chapters', '章节'], ['transcript', '文稿'], ['mindmap', '导图'], ['ask', '问答'], ['notes', '笔记'], ['vocab', '生词'], ['comments', '评论'], ['more', '更多']]

  function renderProgress() {
    dock?.update()
    const el = ui.root.getElementById('prog')
    if (!el) return
    el.replaceChildren()
    if (!S.job) return
    const pct = S.job.total ? Math.round((S.job.done / S.job.total) * 100) : 5
    el.append(
      h('div', { class: 'mute' }, `${S.job.note || '清影正在处理…'}${S.job.total ? (S.job.kind === 'transcribe' ? ` ${S.job.done}%` : ` ${S.job.done}/${S.job.total}`) : ''} `, h('button', { class: 'sec', text: '取消', onclick: () => (S.job.cancel ? S.job.cancel() : send({ type: 'cc-ai-cancel', id: S.job.id })) })),
      h('div', { class: 'bar' }, h('i', { style: `width:${pct}%` })),
    )
  }

  function render() {
    const keepScroll = ui.root.querySelector('.body')?.scrollTop || 0
    const d = S.data
    const children = [
      h('div', { class: 'hd' },
        h('span', { class: 't', title: d?.title || '', text: d ? d.title : '视频摘要' }),
        h('button', { class: 'sec', text: '重新读取', title: '重新读取当前页面的字幕（切换视频后用）', onclick: () => start() }),
        dock?.collapseButton(),
        h('button', { class: 'sec', text: '✕', title: '关闭', 'aria-label': '关闭', onclick: stop }),
      ),
    ]
    if (d) {
      children.push(h('div', { class: 'meta' },
        d.lines.length ? `${d.lines.length} 段` : '无文稿',
        d.track && `· ${d.track}`,
        d.durationMs > 0 && `· ${V.fmtClock(d.durationMs)}`,
        S.status && !S.status.ready && h('span', { style: 'color:#c4283d', text: '· 清影里未设置大模型' })))
      children.push(h('div', { class: 'tabs', role: 'tablist' }, ...TABS.map(([id, n]) => h('button', { class: S.tab === id ? 'on' : '', role: 'tab', 'aria-selected': S.tab === id, text: id === 'vocab' && VOCAB.list ? `${n} ${VOCAB.list.length}` : n, onclick: () => ((S.tab = id), savePrefs(), id === 'vocab' && void loadVocab(), render()) }))))
    }
    const body = h('div', { class: 'body' }, h('div', { id: 'prog' }))
    if (!d) {
      // 读取期间也先把各标签页画出来（灰着），不要整个面板只剩一行字
      children.push(h('div', { class: 'tabs', role: 'tablist' }, ...TABS.map(([, n]) => h('button', { disabled: true, role: 'tab', text: n }))))
      body.append(h('div', { class: 'mute', text: '正在读取字幕…（最多等 25 秒）' }))
    }
    else {
      if (!d.lines.length && S.tab !== 'more' && S.tab !== 'notes' && S.tab !== 'vocab' && S.tab !== 'comments') body.append(noData())
      else {
        const fn = { summary: tabSummary, chapters: tabChapters, transcript: tabTranscript, mindmap: tabMindmap, ask: tabAsk, notes: tabNotes, vocab: tabVocab, comments: tabComments, more: tabMore }[S.tab]
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
    dock?.update()
    ui.root.querySelector('.body').scrollTop = keepScroll
    if (S.jumpHit) {
      S.jumpHit = false
      ui.root.querySelector('.ln.now')?.scrollIntoView({ block: 'center' })
    }
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
  // 站内切换视频（YouTube / B 站不刷新页面）：网址对应的视频变了就自动重新读字幕，旧视频的结果先存起来、字幕层关掉
  function watchNav() {
    if (S.closed || S.navBusy) return
    const k = V.videoKey(location.href)
    if (!S.pageKey) S.pageKey = k
    if (k === S.pageKey) return
    S.pageKey = k
    S.navBusy = true
    saveSession().catch(() => {})
    globalThis.ccSubLayerRun?.destroy()
    // 等页面把标题、播放器换成新视频的再读
    setTimeout(() => start().finally(() => (S.navBusy = false)), 700)
  }

  function tick(force) {
    watchNav()
    dock?.update()
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
        if (S.prefs.follow && cur && !S.query && !S.detached) cur.scrollIntoView({ block: 'center', behavior: 'smooth' })
      } else if (force && cur) cur.scrollIntoView({ block: 'center', behavior: 'smooth' })
    } else if (S.tab === 'chapters') {
      const i = V.chapterAt(chapterList().list, ms)
      const cur = ui.root.querySelector(`.ln[data-ch="${i}"]`)
      if (cur && !cur.classList.contains('cur')) {
        ui.root.querySelector('.ln.cur')?.classList.remove('cur')
        cur.classList.add('cur')
      }
    }
  }

  document.addEventListener('mousemove', (e) => (globalThis.__ccPtr = { x: e.clientX, y: e.clientY, t: Date.now() }), { capture: true, passive: true })
  // 视频胶囊（vdock.js）的菜单里：记笔记、回到面板、关闭摘要（以前视频左上角单独有个“笔记 (n)”按钮，和播放增强的工具条叠在一起）
  const Dock = globalThis.ccVDock
  Dock.register('vsum', {
    order: 40,
    title: '视频摘要',
    applies: () => !S.closed && !!S.data && isVideo(),
    menu: () => {
      const cnt = S.notes.filter((n) => n.key === localKey()).length
      return [
        { label: `记下当前这句（已有 ${cnt} 条）`, hint: 'n', onclick: () => quickNote() || toast(ui, '页面上没有找到视频', true) },
        { label: dock?.state === 'open' ? '收起到右边' : '展开摘要面板', hint: 'Alt+Shift+S', onclick: () => dock?.toggle() },
        { label: '关闭视频摘要', onclick: () => stop() },
      ]
    },
  })

  /** 读到字幕后，同时把文稿作为字幕层叠在视频上（已经开着就换成新文稿）；可在文稿页关掉。 */
  let subOpenAt = 0
  function syncSubLayer(d, force = false) {
    if (!isVideo() || !d.lines.length) return
    // 文稿放进视频会话：字幕层开着的话订阅了会话，自己跟着换
    VS.setLines(d.lines, d.track || '', 'vsum')
    if (S.prefs.autoSub === false) return
    if (globalThis.ccSubLayerRun) {
      // 用户在文稿页勾选“显示在视频上”：字幕层开着但被关掉了（字幕层上的开关）也要显示出来
      if (force) globalThis.ccSubLayerRun.show?.()
      return
    }
    // 正在打开的这几秒里不再重复发（再注入一次会把字幕层关掉）；字幕层打开后从会话里取文稿。用户主动勾选时不受这个限制
    if (!force && Date.now() - subOpenAt < 4000) return
    subOpenAt = Date.now()
    // 字幕层上次是关着的（存了“关”）：打开它就是要看字幕，先把开关存成开
    try {
      globalThis.ccUI.api.storage.local.set({ subLayerOn: true })
    } catch {
      // 存不了就沿用上次的开关
    }
    send({ type: 'cc-sublayer-open' })
  }

  // 字幕层翻好的译文（同一份文稿、同一种语言）直接显示在文稿里
  const unsubSession = VS.subscribe((what, _snap, extra) => {
    // 视频上的字幕层里改了一句：文稿跟着换（其他句的译文保留）
    if (what === 'edit' && extra?.source !== 'vsum' && !S.closed && S.data?.lines[extra.i] && _snap.lines[extra.i]?.t === S.data.lines[extra.i].t) {
      const d = S.data
      d.lines = d.lines.map((l, j) => (j === extra.i ? { ...l, text: extra.text, src: extra.src } : l))
      if (S.res.translated) S.res.translated = S.res.translated.map((x, j) => (j === extra.i ? '' : x))
      if (S.editing) S.pendingRender = true
      else render()
      return
    }
    if (what !== 'trans' || S.closed || !S.data?.lines.length || S.res.translated) return
    if (VS.fingerprint(S.data.lines.map((l) => ({ t: l.t, text: l.text }))) !== VS.fingerprint(VS.current().lines)) return
    const have = VS.translations(outLang())
    if (have && have.length === S.data.lines.length) {
      S.res.translated = have
      render()
    }
  })

  /** 没有标点的自动字幕：大部分行不是以标点结尾 */
  const looksUnpunctuated = (lines) => lines.length >= 8 && lines.filter((l) => /[.!?。！？…，,、;；:：]["')”’）]?$/.test(l.text)).length < lines.length * 0.15

  /**
   * 预处理：读到文稿后先让清影清理一遍（去非语音说明、口水词；设置 → AI 里可调），文稿里显示清理后的，
   * 清理前的存在 d.orig，文稿栏上有“显示原始”开关；清影没开 / 没连上就原样显示。设置里开了“预处理后自动补标点”的话，
   * 没有标点的自动字幕接着自动补标点。送给模型的文字不管显示哪份，清影那边始终会再清理一遍。
   */
  async function autoClean(d) {
    if (d.cleanDone || d.streaming || !d.lines.length || S.closed) return
    d.cleanDone = true
    try {
      const st = await send({ type: 'cc-ai-status' })
      if (!st.ok) return
      const r = await submit({ kind: 'preclean', title: d.title, lines: d.lines.map((l) => ({ t: l.t, text: l.text })) }, 'preclean')
      if (S.data !== d || !Array.isArray(r?.lines)) return
      const rep = r.report || {}
      if ((rep.hearing || 0) + (rep.filler || 0) + (rep.emptied || 0) > 0 && r.lines.length) {
        d.orig = d.lines
        d.cleaned = rep
        d.lines = r.lines.map((l) => ({ t: l.t, text: l.text }))
        S.showOrig = false
        render()
        syncSubLayer(d)
      }
      if (r.punct && !d.raw && looksUnpunctuated(d.lines) && S.status?.ready) {
        S.pendingPunct = true
      }
    } catch {
      // 清影没开或不支持：保持原样
    }
  }

  function setData(d) {
    S.data = d
    S.res = {}
    S.chat = []
    S.query = ''
    S.preview = null
    S.restored = 0
    S.showOrig = false
    render()
    if (!d.lines.length) return
    syncSubLayer(d)
    // 先预处理，再带出以前生成的结果（缓存键按清理后的行算）
    // 同一份文稿以前生成的所有结果和对话直接带出来（不需要大模型）；会话里没有的摘要再去缓存里找
    autoClean(d)
      .then(() => (S.data === d ? applyEditsDoc(d) : null))
      .then(() => (S.data === d ? restoreSession(d) : null))
      .then(async () => {
        if (S.data !== d) return
        if (!S.res.summary) {
          const hit = await cached(V.cacheKey(d.lines, 'summary', S.prefs.style, outLang()))
          if (hit && S.data === d && !S.res.summary) {
            S.res.summary = hit
            S.restored ||= Date.now()
          }
        }
        // 有东西带出来才重画（没有就不动，免得打断正在做的选择）
        if (S.restored || Object.keys(S.res).length) render()
        resumeJobs(d)
        if (S.pendingPunct && S.data === d && !S.busy) {
          S.pendingPunct = false
          punctuateDoc()
        }
      })
      .catch(() => {})
  }

  const RESUME = {
    summary: (x) => (S.res.summary = x),
    mindmap: (x) => (S.res.outline = x.outline),
    cards: (x) => (S.res.cards = x.cards),
    quotes: (x) => (S.res.quotes = x.quotes),
    translate: (x) => (S.res.translated = S.data.lines.map((_, i) => x.lines?.[i]?.text || '')),
  }
  /** 上次关面板时还在生成的东西：清影还在跑或已经跑完，接上取回来。 */
  async function resumeJobs(d) {
    const page = d.url || location.href
    const recs = Object.values(await jobRecs()).filter((r) => r.page === page && RESUME[r.kind] && !S.res[{ summary: 'summary', mindmap: 'outline', cards: 'cards', quotes: 'quotes', translate: 'translated' }[r.kind]])
    for (const r of recs) {
      if (S.data !== d || S.busy) return
      toast(ui, `接上上次没做完的${r.kind === 'summary' ? '摘要' : '任务'}…`)
      await task(r.kind, {}, RESUME[r.kind])
    }
  }

  async function start() {
    S.pageKey = V.videoKey(location.href)
    S.data = null
    render()
    try {
      // 读字幕最多等 25 秒：网络慢或后台没响应时，不要让面板一直停在“正在读取”，直接给出可操作的界面
      const d = await Promise.race([
        Src.load(),
        new Promise((r) => setTimeout(() => r({ title: document.title, url: location.href, site: 'html5', lines: [], chapters: [], tracks: [], note: '读取字幕超时（网络慢或站点没响应）。可以点“重新读取”，或在“更多”里导入字幕 / 用清影转写。', fetchFailed: true }), 25000)),
      ])
      S.recovering = !d.lines.length // 先走“本地字幕 / 缓存 / 在线字幕 / 自动转写”，都没有结果再列字幕轨
      setData(d)
      try {
        if (!d.lines.length) await recover(d)
      } finally {
        if (S.recovering) {
          S.recovering = false
          render()
        }
      }
    } catch (e) {
      setData({ title: document.title, url: location.href, site: 'html5', lines: [], chapters: [], tracks: [], note: `读取失败：${e.message}` })
    }
    ensureReady().catch(() => {}).finally(() => S.data && render())
    refreshAppLang()
  }

  /** 语言下拉框按清影的界面语言显示：先用上次记下的，再问一下清影，变了就重画。 */
  async function refreshAppLang() {
    try {
      S.appLang = (await api.storage.local.get('appLang')).appLang || ''
    } catch {
      // 读不到就用浏览器语言
    }
    const r = await send({ type: 'cc-app-lang' })
    if (r?.ok && r.lang && r.lang !== S.appLang) {
      S.appLang = r.lang
      if (S.data) render()
    }
  }

  function stop() {
    clearInterval(S.timer)
    document.removeEventListener('keydown', onKey, true)
    // 正在生成的任务不取消：清影继续跑，重新打开后自动接上（要取消请点进度条上的“取消”）
    S.closed = true
    dock?.destroy()
    unsubSession()
    Dock.unregister('vsum')
    saveSession().catch(() => {})
    ui.remove()
    delete globalThis.__ccVsum
  }

  // ---------- 拆出去的模块（vsum-util / session / flow / local / tabs .js）：共享的状态和工具放在 C 里 ----------
  const C = { api, ui, S, V, h, toast, send, VS, Src, render, renderProgress, setData }
  for (const m of ['util', 'session', 'flow', 'local', 'tabs']) globalThis.ccParts.vsum[m](C)
  const {
    actsBar, addNote, ask, cached, chapterList, copy, ensureReady, forgetLocal, grp, importFile, isVideo, jobRecs,
    langSelect, loadArticleFromSelection, localKey, noData, onKey, onSelect, outLabel, outLang, quickNote, recover,
    reloadTrack, rememberLocal, restoreSession, run, saveFile, savePrefs, saveSession, seek, sel, sendForTranscribe,
    sourcePicker, store, submit, syncToApp, tabAsk, tabChapters, tabSummary, task, trackBar, transcribeNow,
    translateDoc, tsBtn, video
  } = C
  // ---------- 各标签页（vsum-*.js）：依赖从这里传进去 ----------

  const TABS_ = globalThis.ccVsumTabs
  const { getSel, tabTranscript, punctuate: punctuateDoc, applyEdits: applyEditsDoc, rememberEdit: rememberEditDoc } = TABS_.transcript({
    S, V, actsBar, addNote, copy, h, isVideo, langSelect, onSelect, outLabel, render, saveFile, savePrefs, seek,
    sel, send, setData, syncSubLayer, task, tick, toast, trackBar, translateDoc, ui, video
  })
  const { exportData, tabMore } = TABS_.more({
    S, Src, V, applyAppearance, copy, forgetLocal, grp, h, importFile, isVideo, loadArticleFromSelection,
    reloadTrack, rememberLocal, render, saveFile, savePrefs, sel, send, sendForTranscribe, setData, sourcePicker,
    store, syncToApp, task, tick, toast, transcribeNow, tsBtn, ui
  })
  const { tabNotes } = TABS_.notes({
    S, V, actsBar, copy, exportData, h, isVideo, localKey, quickNote, render, run, saveFile, seek, sel, store,
    toast, ui
  })
  const { tabComments } = TABS_.comments({
    S, V, actsBar, addNote, copy, ensureReady, h, localKey, outLang, render, saveFile, savePrefs, seek, sel, store,
    submit, toast, ui
  })
  const { VOCAB, loadVocab, tabVocab } = TABS_.vocab({
    S, V, actsBar, addNote, h, localKey, render, saveFile, seek, sel, send, toast, ui
  })
  const { tabMindmap } = TABS_.mindmap({
    S, V, actsBar, copy, h, isDark, isVideo, onKey, render, saveFile, seek, task, toast, ui
  })
  Object.assign(C, { exportData, getSel, rememberEditDoc })

  for (const ev of ['wheel', 'touchmove']) {
    ui.root.addEventListener(ev, () => {
      if (S.tab === 'transcript' && S.prefs.follow && isVideo() && !S.detached && !S.query) {
        S.detached = true
        render()
      }
    }, { passive: true })
  }

  dock = Dk.create({ ui, h, panel, V, store, S, render })
  applyAppearance()
  globalThis.__ccVsum = { stop, toggle: () => dock.toggle() }
  S.timer = setInterval(tick, 500)
  document.addEventListener('keydown', onKey, true)
  store.get('vsumNotes', []).then((n) => (S.notes = Array.isArray(n) ? n : []))
  store.get('vsumPrefs', null).then((p) => {
    if (p) {
      S.prefs = { ...S.prefs, ...p }
      // 0.10.3：没有字幕时默认自动取音频转写（以前默认关）。只改一次，之后用户怎么选就怎么算
      if (!p.srcV) {
        S.prefs.autoTranscribe = true
        S.prefs.srcV = 1
      }
      applyAppearance()
      S.tab = TABS.some(([id]) => id === p.tab) ? p.tab : 'summary'
    }
    // 设置了“打开时默认收起到右边”：直接是小标签
    if (S.prefs.collapseOnOpen) dock.set('collapse')
    return start()
  })
})()
