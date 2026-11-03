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
    .meta { padding: 0 10px 6px; font-size: 12px; color: #5d6862; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
    .tabs { display: flex; border-bottom: 1px solid #e3e6e3; }
    .tabs button { flex: 1; background: transparent; color: #5d6862; border-radius: 0; border-bottom: 2px solid transparent; padding: 7px 0; font-weight: 400; }
    .tabs button.on { color: #b4561a; border-bottom-color: #b4561a; font-weight: 700; }
    .body { flex: 1; overflow: auto; padding: 10px; display: flex; flex-direction: column; }
    .body > :not(.selbar):not(#prog) { flex: 1 0 auto; display: flex; flex-direction: column; }
    .body > .sm { gap: 12px; }
    .body > #prog { flex: 0 0 auto; display: block }
    .body > #prog:empty { display: none }
    .tools { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-bottom: 8px; }
    select, input[type=text], textarea { border: 1px solid #cfd5d1; border-radius: 6px; padding: 4px 6px; background: #fff; color: inherit; min-width: 0; }
    textarea { width: 100%; resize: vertical; }
    .card { background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; white-space: pre-wrap; word-break: break-word; }
    .vcard .vh { display: flex; gap: 8px; align-items: baseline; flex-wrap: wrap; }
    .vcard .vf { margin-top: 4px; }
    .chips { display: flex; gap: 5px; flex-wrap: wrap; margin: 6px 0; }
    .chip { background: #d9762b1f; color: #8a3f0c; border-radius: 10px; padding: 1px 8px; font-size: 12px; }
    .ln { position: relative; display: grid; grid-template-columns: 48px 1fr; gap: 8px; padding: 5px 6px; border-radius: 8px; cursor: pointer; }
    .ln:hover { background: #d9762b14; }
    .ln.cur { background: #d9762b1f; }
    .ln.cur .tm { background: #b4561a; color: #fff; border-radius: 6px; padding: 0 6px; }
    .ln.hit { background: #f3d06a30; }
    .ln.hit.now { outline: 2px solid #d9762b; }
    mark { background: #ffe066; color: #1b1f1d; border-radius: 2px; padding: 0 1px; }
    .src { border: 1px solid #e3e6e3; background: #fbfaf8; border-radius: 8px; margin: 4px 0 0; padding: 4px 8px; font-size: 12px; color: #5d6862; cursor: pointer; }
    .warn { color: #a4561a; font-size: 12px; margin-top: 4px; }
    .ln .tm { color: #b4561a; font-variant-numeric: tabular-nums; font-weight: 700; font-size: 12px; line-height: 20px; align-self: start; justify-self: start; }
    .ln .tr { color: #5d6862; display: block; }
    .mute { color: #5d6862; font-size: 12px; }
    .cmbar { display: flex; height: 10px; border-radius: 5px; overflow: hidden; gap: 2px; margin: 6px 0; }
    .cmbar i { display: block; }
    .cmbar .p { background: #2f7d57; } .cmbar .u { background: #8a948f; } .cmbar .n { background: #b8472f; }
    .cmleg { display: flex; gap: 12px; font-size: 12px; color: #5d6862; flex-wrap: wrap; }
    .cmth { padding: 8px 0; border-top: 1px solid #e3e6e3; } .cmth:first-of-type { border-top: 0; padding-top: 2px; }
    .cmb { height: 6px; border-radius: 3px; background: #e3e6e3; margin: 4px 0; } .cmb i { display: block; height: 6px; border-radius: 3px; background: #d9762b; }
    .cq { background: #f4f5f4; border-radius: 6px; padding: 6px 8px; font-size: 12px; margin-top: 4px; white-space: pre-wrap; word-break: break-word; }
    .cq .m { display: flex; justify-content: space-between; align-items: center; color: #5d6862; margin-top: 2px; font-size: 11.5px; }
    .cq .m button { padding: 0 6px; font-size: 11.5px; }
    .tg { font-size: 11px; padding: 0 6px; border-radius: 4px; background: #d9762b1f; color: #8a3f0c; margin-right: 6px; white-space: nowrap; }
    .tg.ok { background: #2f7d5722; color: #1f6a45; } .tg.no { background: #b8472f22; color: #9a3320; }
    .cmq { display: flex; align-items: flex-start; gap: 4px; margin: 4px 0; }
    .warn { background: #fff4e5; border: 1px solid #f0c98e; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; }
    .bar { height: 4px; background: #e3e6e3; border-radius: 2px; overflow: hidden; margin: 6px 0; }
    .bar i { display: block; height: 100%; background: #d9762b; transition: width .3s; }
    .msg { margin-bottom: 8px; }
    .msg.u { text-align: right; }
    .msg .b { display: inline-block; text-align: left; max-width: 92%; padding: 6px 10px; border-radius: 10px; background: #fff; border: 1px solid #e3e6e3; white-space: pre-wrap; word-break: break-word; }
    .msg.u .b { background: #b4561a; color: #fff; border-color: #b4561a; }
    .ts { background: #fff; border: 1px solid #e3e6e3; color: #b4561a; padding: 0 6px; font-size: 12px; line-height: 20px; border-radius: 6px; font-weight: 700; font-variant-numeric: tabular-nums; }
    .ask { display: flex; gap: 6px; padding: 8px 10px; border-top: 1px solid #e3e6e3; }
    .ask input { flex: 1; }
    .svgbox { overflow: auto; border: 1px solid #e3e6e3; border-radius: 8px; background: #fff; max-height: 55vh; }
    .svgbox svg { display: block; width: 100%; height: auto; cursor: zoom-in; }
    .mapview { inset: 0; display: flex; flex-direction: column; background: #fbfaf8; color: #1d2420; }
    .mapview .bar2 { display: flex; gap: 6px; align-items: center; padding: 8px 10px; border-bottom: 1px solid #e3e6e3; flex-wrap: wrap; }
    .mapview .bar2 b { flex: 1; min-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .mapview .stage { flex: 1; overflow: hidden; position: relative; cursor: grab; touch-action: none; background: #fff; }
    .mapview .stage.drag { cursor: grabbing; }
    .mapview .stage > svg { position: absolute; left: 0; top: 0; transform-origin: 0 0; max-width: none; }
    .mapview .hint { font-size: 12px; color: #5d6862; padding: 4px 10px; }
    .ln .rec { visibility: hidden; position: absolute; right: 4px; top: 3px; background: #fff; border: 1px solid #e3e6e3; color: #b4561a; padding: 0 7px; font-size: 12px; }
    .ln:hover .rec { visibility: visible; }
    .selbar { position: sticky; top: 0; z-index: 2; background: #fff; border: 1px solid #d9762b; border-radius: 8px; padding: 6px 8px; margin-bottom: 8px; box-shadow: 0 2px 8px #0002; }
    .note { background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; word-break: break-word; }
    .note .q { border-left: 3px solid #d9762b; padding-left: 8px; white-space: pre-wrap; }
    .note .x { display: flex; gap: 4px; flex-wrap: wrap; margin-top: 6px; align-items: center; }
    .note .x button { padding: 1px 7px; font-size: 12px; }
    .pill { transition: opacity .25s; position: fixed; z-index: 2147483000; padding: 4px 10px; border-radius: 14px; background: #b4561acc; color: #fff; font-size: 12px; box-shadow: 0 2px 8px #0004; }
    .sm { display: flex; flex-direction: column; gap: 12px; }
    .sm .vid { display: grid; gap: 2px; }
    .sm .vid b { font-size: 14px; line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .sm .vmeta { display: flex; flex-wrap: wrap; gap: 2px 10px; color: #5d6862; font-size: 12px; }
    .sm .saved { color: #2f7a4d; }
    .sm .ctrl { display: grid; gap: 8px; }
    .sm .ctrl.stuck { position: sticky; top: -10px; z-index: 3; background: #fbfaf8; margin: -10px -10px 0; padding: 10px; border-bottom: 1px solid #e3e6e3; }
    .sm .seg { display: flex; gap: 2px; padding: 2px; background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; overflow-x: auto; scrollbar-width: none; }
    .sm .seg button { flex: none; background: transparent; color: #5d6862; font-weight: 400; padding: 3px 10px; border-radius: 6px; white-space: nowrap; }
    .sm .seg button.on { background: #b4561a; color: #fff; font-weight: 700; }
    .sm .crow { display: flex; align-items: center; gap: 8px; }
    .sm .crow select { min-width: 0; max-width: 150px; }
    .sm .grow { flex: 1; }
    .sm .lead { background: #d9762b1a; border-radius: 10px; padding: 10px 12px; }
    .sm .lead .k { font-size: 11px; letter-spacing: .1em; color: #b4561a; font-weight: 700; margin-bottom: 2px; }
    .sm .lead p { margin: 0; font-size: 1.15em; line-height: 1.55; font-weight: 600; }
    .sm .sh { display: flex; align-items: baseline; gap: 4px; margin: 0 0 6px; font-size: 12px; font-weight: 700; letter-spacing: .08em; color: #5d6862; }
    .sm .sh em { font-style: normal; font-weight: 400; letter-spacing: 0; }
    .sm .sh .lnk { margin-left: auto; background: transparent; color: #b4561a; padding: 0; font-weight: 400; }
    .sm .prose p { margin: 0 0 .65em; line-height: 1.8; }
    .sm .prose p:last-child { margin: 0; }
    .sm .kw { display: flex; flex-wrap: wrap; gap: 6px; }
    .sm .kw span { border: 1px solid #e3e6e3; background: #fff; border-radius: 6px; padding: 0 8px; font-size: 12px; line-height: 22px; }
    .sm .kw span::before { content: "#"; color: #b4561a; margin-right: 2px; }
    .sm .pts { list-style: none; margin: 0; padding: 0; position: relative; display: grid; gap: 2px; }
    .sm .pts::before { content: ""; position: absolute; left: 25px; top: 10px; bottom: 10px; border-left: 1px dashed #cfd5d1; }
    .sm .pts li { position: relative; display: grid; grid-template-columns: 52px 1fr; gap: 8px; padding: 6px 4px; border-radius: 8px; }
    .sm .pts li.go { cursor: pointer; }
    .sm .pts li.go:hover { background: #d9762b14; }
    .sm .pts .tm { align-self: start; justify-self: start; position: relative; background: #fff; border: 1px solid #e3e6e3; color: #b4561a; font-variant-numeric: tabular-nums; font-weight: 700; font-size: 12px; line-height: 20px; padding: 0 6px; border-radius: 6px; }
    .sm .pts b { display: block; line-height: 1.5; }
    .sm .pts .d { color: #5d6862; }
    .acts { position: sticky; bottom: -10px; z-index: 2; margin-top: auto; flex: none; display: flex; gap: 6px; padding: 8px 0 2px; background: #fbfaf8; }
    .acts button:first-child { flex: 1; }
    .stick { position: sticky; top: -10px; z-index: 2; display: grid; gap: 8px; background: #fbfaf8; margin: -10px -10px 8px; padding: 10px; border-bottom: 1px solid #e3e6e3; }
    .stick .tools { margin: 0; }
    .chs { display: grid; gap: 2px; position: relative; }
    .chs::before { content: ""; position: absolute; left: 27px; top: 12px; bottom: 12px; border-left: 1px dashed #cfd5d1; }
    .chs .ln { grid-template-columns: 52px 1fr; }
    .chs .ln .tm { background: #fff; border: 1px solid #e3e6e3; border-radius: 6px; padding: 0 6px; position: relative; }
    .chs .ln.cur .tm { background: #b4561a; color: #fff; border-color: #b4561a; }
    .chs .gist { display: block; color: #5d6862; }
    .quote { border-left: 3px solid #d9762b; padding: 2px 0 2px 10px; margin: 4px 0 0; color: #5d6862; font-size: 12px; }
    .sugg { display: grid; gap: 6px; }
    .sugg button { text-align: left; display: flex; justify-content: space-between; padding: 8px 12px; border-radius: 10px; font-weight: 400; }
    .sugg button::after { content: "›"; color: #b4561a; }
    .svgbox { position: relative; }
    .svgbox .zoomhint { position: absolute; left: 10px; bottom: 6px; font-size: 11px; color: #5d6862; pointer-events: none; }
    details.acc { border: 1px solid #e3e6e3; border-radius: 10px; background: #fff; }
    details.acc summary { padding: 8px 10px; cursor: pointer; font-weight: 700; }
    details.acc pre { margin: 0; padding: 0 10px 10px; font: 12px/1.7 ui-monospace, Menlo, Consolas, monospace; color: #5d6862; white-space: pre-wrap; }
    .grp { background: #fff; border: 1px solid #e3e6e3; border-radius: 10px; overflow: hidden; margin-bottom: 10px; }
    .grp .gh { padding: 6px 12px; font-size: 12px; font-weight: 700; letter-spacing: .08em; color: #5d6862; background: #fbfaf8; border-bottom: 1px solid #e3e6e3; }
    .grp .gb { padding: 8px 12px; display: flex; flex-direction: column; gap: 8px; }
    .grp .gb .card { background: #fbfaf8; margin: 0; }
    .note .x { opacity: .95; }
    .sm .empty { display: grid; gap: 6px; justify-items: center; text-align: center; padding: 16px 8px; }
    .sm .empty p { margin: 0; max-width: 30ch; }
    .sm .warn { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 0; }
    .sm .warn span { flex: 1; min-width: 160px; }
    h4 { margin: 10px 0 4px; font-size: 13px; }
    ul { margin: 4px 0; padding-left: 18px; }
  `

  const DARK = `
      .panel, .mapview { background: #1d2420; color: #eef1ee; border-left-color: #3a443e; }
      .mapview .stage { background: #1d2420; }
      .mapview .bar2 { border-color: #3a443e; }
      .hd, .meta, .tabs, .ask { border-color: #3a443e; }
      .selbar, .note, .grp, details.acc, .ts, .ln .rec, .chs .ln .tm { background: #262f2a; border-color: #3a443e; }
      .stick, .grp .gh, .src { background: #1d2420; border-color: #3a443e; }
      .grp .gb .card { background: #1d2420; }
      .ln .tm, .ts, .ln.cur .tm, .sugg button::after { color: #f0a064; }
      .ln.cur .tm { background: #f0a064; color: #1d2420; }
      .ln.cur { background: #e28a4524; }
      .quote, .chs .gist, details.acc pre, .grp .gh { color: #a9b3ad; }
      .chs::before { border-color: #3a443e; }
      .chs .ln.cur .tm { background: #f0a064; color: #1d2420; border-color: #f0a064; }
      .card, .msg .b, select, input[type=text], textarea { background: #262f2a; border-color: #3a443e; color: #eef1ee; }
      button.sec { background: #262f2a; color: #eef1ee; border-color: #3a443e; }
      .ln .tr, .meta, .mute, .src { color: #a9b3ad; }
      .warn { background: #3a2f1e; border-color: #6b5530; }
      .cq { background: #1d2420; } .chip { color: #f0a064; background: #e28a4529; } .cmb, .cmth { background: #3a443e; border-color: #3a443e; } .cmth { background: none; } .cmleg, .cq .m { color: #a9b3ad; }
      .tg { color: #f0a064; } .tg.ok { color: #6cc08a; } .tg.no { color: #ee917b; }
      .sm .seg, .sm .kw span, .sm .pts .tm { background: #262f2a; border-color: #3a443e; }
      .sm .seg button, .sm .vmeta, .sm .sh, .sm .pts .d { color: #a9b3ad; }
      .sm .seg button.on { background: #f0a064; color: #1d2420; }
      .sm .lead .k, .sm .pts .tm, .sm .sh .lnk { color: #f0a064; }
      .sm .saved { color: #6cc08a; }
      .acts, .sm .ctrl.stuck { background: #1d2420; border-color: #3a443e; }
      .sm .pts::before { border-color: #3a443e; }
  `
  const ui = mount(CSS)
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
    prefs: { style: 'standard', lang: '', follow: true, tab: 'summary', autoOnline: true, autoTranscribe: true, srcV: 1, view: 'both', lead: 3, pill: true, prompts: {}, theme: 'auto', fs: 13 },
    res: {}, // 本次的结果：summary / outline / cards / quotes / translated / rewrite
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
  const TL = globalThis.ccTrans
  /** 写进请求的输出语言（提示词里的名字）；留空 = 清影设置里的目标语言（中文视频的翻译默认译成英文）。 */
  const outLang = () => (S.prefs.lang.trim() ? TL.promptName(S.prefs.lang) : S.status?.lang || '中文')
  const outLabel = () => {
    if (!S.prefs.lang.trim()) return ''
    const code = TL.LANGS.find((l) => l.code === S.prefs.lang.trim() || l.en === TL.promptName(S.prefs.lang))?.code
    return code ? TL.label(code, S.appLang).split(' · ')[0] : S.prefs.lang.trim()
  }
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

  /** 底部操作栏：第一个按钮占满宽度，滚到哪里都点得到。 */
  const actsBar = (...kids) => h('div', { class: 'acts' }, ...kids)
  /** “更多”页的分组卡片。 */
  const grp = (title, ...kids) => h('div', { class: 'grp' }, h('div', { class: 'gh', text: title }), h('div', { class: 'gb' }, ...kids.filter(Boolean)))

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

  /** 输出 / 翻译语言下拉：自动 + 常用的几十种语言；以前手填的别的写法也保留成一项。 */
  function langSelect(title, after) {
    const known = TL.LANGS.some((l) => l.code === S.prefs.lang)
    const opts = [['', '跟随视频'], ...TL.LANGS.map((l) => [l.code, TL.label(l.code, S.appLang)])]
    if (S.prefs.lang && !known) opts.push([S.prefs.lang, S.prefs.lang])
    return sel(opts, S.prefs.lang, (v) => ((S.prefs.lang = v), savePrefs(), after?.()), title)
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

  // ---------- 会话缓存：同一份文稿的所有结果、对话，关掉再开直接带出来 ----------
  const SESS_KEEP = 40
  const sessKey = () => V.cacheKey(S.data.lines, 'session')
  let saveTimer = 0
  function persist() {
    clearTimeout(saveTimer)
    saveTimer = setTimeout(saveSession, 300)
  }
  async function saveSession() {
    if (!S.data?.lines?.length) return
    const res = { ...S.res }
    delete res.rewrite
    const empty = !Object.values(res).some(Boolean) && !S.chat.length
    const all = await store.get('vsumSessions', {})
    const key = sessKey()
    if (empty) delete all[key]
    else all[key] = { ts: Date.now(), data: { res, chat: S.chat.slice(-40), style: S.prefs.style, lang: outLang(), title: S.data.title } }
    await store.set('vsumSessions', V.pruneCache(all, SESS_KEEP, 2_500_000))
  }
  /** 取回上次的会话；摘要风格或输出语言变了，对应的结果不带。 */
  async function restoreSession(d) {
    const all = await store.get('vsumSessions', {})
    const hit = all[V.cacheKey(d.lines, 'session')]
    if (!hit?.data || S.data !== d) return false
    const { res = {}, chat = [], style, lang } = hit.data
    const out = { ...res }
    if (style !== S.prefs.style) delete out.summary
    if (lang !== outLang()) {
      delete out.summary
      delete out.translated
    }
    if (out.translated && out.translated.length !== d.lines.length) delete out.translated
    if (!Object.values(out).some(Boolean) && !chat.length) return false
    S.res = { ...S.res, ...out }
    if (chat.length) S.chat = chat
    S.restored = hit.ts
    return true
  }
  const ago = (ts) => {
    const m = Math.max(0, Math.round((Date.now() - ts) / 60000))
    if (m < 1) return '刚刚'
    if (m < 60) return `${m} 分钟前`
    if (m < 1440) return `${Math.round(m / 60)} 小时前`
    return `${Math.round(m / 1440)} 天前`
  }

  // 正在进行的任务：关掉面板不取消，重开后接着等（清影那边会继续跑）
  const JOB_FRESH = 25 * 60 * 1000
  async function jobRecs() {
    const all = await store.get('vsumJobs', {})
    const now = Date.now()
    return Object.fromEntries(Object.entries(all).filter(([, v]) => now - v.ts < JOB_FRESH))
  }
  async function setJobRec(tag, rec) {
    const all = await jobRecs()
    if (rec) all[tag] = rec
    else delete all[tag]
    await store.set('vsumJobs', all)
  }

  /** 提交 → 轮询。cache 为 false 时不读不写缓存（问答、改写）。 */
  async function run(kind, extra = {}, cache = true) {
    await ensureReady()
    const lang = outLang()
    const style = kind === 'summary' ? S.prefs.style : ''
    const key = cache && !S.prefs.prompts?.[kind] ? V.cacheKey(S.data.lines, kind, style + (extra.format || ''), lang) : ''
    if (key) {
      const hit = await cached(key)
      if (hit) return hit
    }
    const own = String(S.prefs.prompts?.[kind] || '').trim()
    const req = { kind, title: S.data.title, lang, style, lines: linesForReq(), ...(own ? { system: own } : {}), ...extra }
    const result = await submit(req, kind, key)
    if (key) putCache(key, result)
    return result
  }

  /** 提交一个任务并轮询到结束，返回结果；进度显示在面板顶部，可取消。 */
  async function submit(req, kind, tag = '') {
    const page = S.data?.url || location.href
    let id = 0
    if (tag) {
      // 上次关掉面板时这个任务还在跑：接上它，不再重新提交
      const old = (await jobRecs())[tag]
      if (old) {
        const j = await send({ type: 'cc-ai-job', id: old.id })
        if (j.ok && (j.state === 'running' || j.state === 'done')) id = old.id
      }
    }
    if (!id) {
      const sub = await send({ type: 'cc-ai-run', req })
      if (!sub.ok) throw new Error(sub.error)
      id = sub.id
    }
    if (tag) await setJobRec(tag, { id, kind, page, ts: Date.now() })
    const job = { id, kind, tag, done: 0, total: 0, note: '' }
    if (!S.job) S.job = job // 同时有多个后台任务时，进度条只显示最先开始的那个
    renderProgress()
    let finished = false
    try {
      for (;;) {
        await new Promise((r) => setTimeout(r, 700))
        if (S.closed) throw new Error('已取消')
        const j = await send({ type: 'cc-ai-job', id })
        if (!j.ok) throw new Error(j.error)
        job.done = j.done
        job.total = j.total
        job.note = j.note || ''
        renderProgress()
        if (j.state === 'done') {
          finished = true
          return j.result
        }
        if (j.state === 'error') {
          finished = true
          throw new Error(j.error || '失败了')
        }
        if (j.state === 'cancelled') {
          finished = true
          throw new Error('已取消')
        }
      }
    } catch (e) {
      finished = finished || !S.closed
      throw e
    } finally {
      if (tag && finished) setJobRec(tag, null)
      if (S.job === job) S.job = null
      if (!S.closed) renderProgress()
    }
  }

  // ---------- 文稿来源：用清影解析出的字幕轨 / 音频转写，单选，后台执行 ----------

  async function loadSources() {
    const url = S.data?.url
    if (S.srcLoading || S.src?.url === url) return
    S.srcLoading = true
    try {
      const st = await send({ type: 'cc-ai-status' })
      if (!st.ok) throw new Error(st.error)
      S.status = st
      const r = await submit({ kind: 'tracks', url, title: S.data.title }, 'tracks')
      const seen = new Set()
      const tracks = (r?.tracks || []).filter((t) => t.code && !seen.has(t.code) && seen.add(t.code))
      S.src = { url, tracks, note: r?.note || '' }
    } catch (e) {
      S.src = { url, tracks: [], note: e.message === '已取消' ? '' : e.message }
    } finally {
      S.srcLoading = false
      if (!S.closed) render()
    }
  }

  /** 单选一个来源：字幕轨（语言代码）或 audio（取音频转写）。同一个视频同一个来源只做一次。 */
  async function pickSource(v) {
    if (!v) return
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    const audio = v === 'audio'
    if (audio && S.status?.transcribe === false) return toast(ui, '清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp 或填写接口', true, 6000)
    const url = S.data.url
    const key = audio ? `stt:${url}` : `sub:${url}:${v}`
    S.busy = true
    render()
    try {
      let r = await cached(key)
      if (!r) {
        r = await submit(audio ? { kind: 'transcribe', url, title: S.data.title, track: 'audio' } : { kind: 'subtitles', url, title: S.data.title, track: v }, audio ? 'transcribe' : 'subtitles', key)
        if (r?.lines?.length) putCache(key, r)
      }
      if (!r?.lines?.length) throw new Error(audio ? '没有识别出任何语音' : '这条字幕是空的')
      setData({ ...S.data, lines: r.lines, durationMs: r.durationMs || S.data.durationMs || 0, track: r.track, noSubtitle: false, fetchFailed: false, note: '', trackUrl: '' })
      toast(ui, `${audio ? '转写' : '字幕'}完成，共 ${r.lines.length} 句`)
    } catch (e) {
      if (!S.closed) toast(ui, e.message, true, 7000)
    } finally {
      S.busy = false
      if (!S.closed) render()
    }
  }

  /** auto：没有文稿时自动去查；已有文稿时只在点按钮后才查（不为了好看多打一次网站的请求）。 */
  function sourcePicker(auto = true) {
    if (!isVideo() || !/^https?:/.test(S.data?.url || '')) return null
    if (S.src?.url !== S.data.url) {
      if (!auto && !S.srcLoading) return h('div', { class: 'tools' }, h('button', { class: 'sec', text: '换一个文稿来源（字幕轨 / 音频转写）', onclick: () => ((S.wantSrc = true), loadSources()) }))
      if (!S.recovering) loadSources()
      return h('div', { class: 'mute', text: '正在查看这个视频有哪些字幕…' })
    }
    const opts = [['', '选择文稿来源…'], ...S.src.tracks.map((t) => [t.code, `字幕 · ${t.name}`]), ['audio', '音频转写（取音频，用 Whisper 转成文字）']]
    return h('div', {},
      h('div', { class: 'tools' }, h('span', { class: 'mute', text: '文稿来源' }), sel(opts, '', pickSource, '单选：有字幕就直接取字幕，没有就取音频转写；在后台进行，结果会缓存')),
      !S.src.tracks.length && h('div', { class: 'mute', text: S.src.note ? `没有取到字幕列表：${S.src.note}` : '这个视频没有可用的在线字幕，可以选“音频转写”。' }),
    )
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
        r = await submit({ kind: 'transcribe', url, title }, 'transcribe', key)
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
      S.restored = 0
      persist()
    } catch (e) {
      if (!S.closed) toast(ui, e.message, true, 5200)
    } finally {
      S.busy = false
      if (!S.closed) render()
    }
  }

  // ---------- 各个标签页 ----------

  const noData = () => {
    const d = S.data
    return h(
      'div',
      { class: 'warn' },
      d?.note || '还没有文稿',
      d?.site === 'bilibili' && h('div', { class: 'mute', text: 'B 站的字幕（含 AI 字幕）需要登录后才能读到；登录后点上面的“重新读取”。' }),
      sourcePicker(),
      h('div', { class: 'tools', style: 'margin-top:6px' },
        (d?.noSubtitle || d?.fetchFailed) && h('button', { text: '用清影转写（语音识别）', title: S.status?.transcribe === false ? '清影里还没有配置语音转文字' : '取视频的音频，在清影里转成文字后再总结', onclick: transcribeNow }),
        h('button', { class: 'sec', text: '导入字幕文件', onclick: importFile }),
        h('button', { class: 'sec', text: '粘贴文字', onclick: () => (S.tab = 'more') && render() }),
      ),
    )
  }

  async function sendForTranscribe() {
    const r = await send({ type: 'cc-send-page' })
    toast(ui, r.ok ? (r.pairing ? '请在电脑上允许配对' : '已发送。下载完成后在清影的媒体库里点“转写”，再回来用文字稿') : r.error, !r.ok, 5200)
  }

  // ---------- 记住本地导入的字幕：在线字幕读不到时再用，在线的永远优先 ----------

  const localKey = () => V.videoKey(S.data?.url || location.href)
  async function savedLocal() {
    return (await store.get('vsumLocal', {}))[localKey()]?.data || null
  }
  async function rememberLocal(name, d) {
    if (!isVideo() && d.site !== 'manual') return
    if (!/^https?:/.test(S.data?.url || location.href) || !d.lines.length) return
    const all = await store.get('vsumLocal', {})
    all[localKey()] = { ts: Date.now(), data: { name, lines: d.lines.map((l) => ({ t: l.t, text: l.text })) } }
    await store.set('vsumLocal', V.pruneCache(all, 40, 3_000_000))
    S.hasLocal = true
    toast(ui, '已记住这份字幕：下次这个视频读不到在线字幕时会自动使用', false, 4200)
    render()
  }
  async function forgetLocal() {
    const all = await store.get('vsumLocal', {})
    delete all[localKey()]
    await store.set('vsumLocal', all)
    S.hasLocal = false
    toast(ui, '已忘记这个视频的本地字幕')
    render()
  }

  /** 在线字幕读不到时的补救：已记住的本地字幕 → 让清影用 yt-dlp 取在线字幕 → （可选）自动转写。 */
  async function recover(d) {
    S.hasLocal = !!(await savedLocal())
    const saved = await savedLocal()
    if (S.data !== d) return
    if (saved?.lines?.length) {
      setData({ ...d, lines: saved.lines, track: `${saved.name}（本地字幕）`, noSubtitle: false, note: '', fromLocal: true })
      S.hasLocal = true
      render()
      return
    }
    if (!d.url || !/^https?:/.test(d.url) || d.site === 'article' || !(d.noSubtitle || d.fetchFailed)) return
    // 以前转写 / 取到过的文稿直接用，不再请求清影
    for (const [key, name] of [[`tr:${d.url}`, '清影转写'], [`sub:${d.url}`, null]]) {
      const old = await cached(key)
      if (S.data !== d) return
      if (old?.lines?.length) {
        setData({ ...d, lines: old.lines, durationMs: old.durationMs || d.durationMs, track: name ? old.track || name : old.track, noSubtitle: false, note: '', trackUrl: '' })
        return
      }
    }
    const st = await send({ type: 'cc-ai-status' })
    if (S.data !== d) return
    if (st.ok) S.status = st
    if (S.prefs.autoOnline && st.ok && st.subtitles !== false && !S.busy) {
      S.busy = true
      render()
      try {
        const skey = `sub:${d.url}`
        let r = await cached(skey)
        if (!r) {
          r = await submit({ kind: 'subtitles', url: d.url, title: d.title }, 'subtitles', skey)
          if (r?.lines?.length) putCache(skey, r)
        }
        if (S.data !== d) return
        if (r?.lines?.length) {
          S.busy = false
          setData({ ...d, lines: r.lines, durationMs: r.durationMs || d.durationMs, track: r.track, noSubtitle: false, note: '', trackUrl: '' })
          return
        }
        d.note = `${d.note || '没有找到字幕'}；清影也没有从网站取到在线字幕`
      } catch (e) {
        if (e.message !== '已取消') d.note = `${d.note || '没有找到字幕'}；清影取在线字幕失败：${e.message}`
      } finally {
        S.busy = false
        render()
      }
    }
    if (S.data === d && S.prefs.autoTranscribe && st.ok && st.transcribe) await transcribeNow()
  }

  function importFile() {
    const inp = h('input', { type: 'file', accept: '.srt,.vtt,.txt,.md,.json', style: 'display:none' })
    inp.onchange = async () => {
      const f = inp.files?.[0]
      if (!f) return
      const d = { ...Src.fromText(await f.text(), f.name), url: S.data?.url || location.href, title: S.data?.title || f.name }
      setData(d)
      rememberLocal(f.name, d)
    }
    ui.add(inp)
    inp.click()
  }

  /** 只把失败的那几段重新让模型总结，再并进已有的摘要（不重做成功的部分）。 */
  async function retryFailed() {
    const r = S.res.summary
    const ranges = r?.failedRanges || []
    if (!ranges.length) return
    const inAny = (t) => ranges.some(([a, b]) => t >= a && t < b)
    const lines = S.data.lines.filter((l) => inAny(l.t)).map((l) => ({ t: l.t, text: l.text }))
    if (!lines.length) return
    await task('summary', { lines }, (x) => {
      const chapters = [...(r.chapters || []), ...(x.chapters || [])].sort((a, b) => (a.startMs ?? a.start_ms ?? 0) - (b.startMs ?? b.start_ms ?? 0))
      S.res.summary = {
        ...r,
        summary: [r.summary, x.summary].filter(Boolean).join('\n'),
        points: [...(r.points || []), ...(x.points || [])],
        keywords: [...new Set([...(r.keywords || []), ...(x.keywords || [])])].slice(0, 10),
        chapters,
        failed: x.failed || [],
        failedRanges: x.failedRanges || [],
      }
    }, false)
  }

  function tabSummary() {
    const r = S.res.summary
    const box = h('div', { class: 'sm' })
    const d = S.data
    const sty = V.STYLES.find(([v]) => v === S.prefs.style)?.[1] || ''
    box.append(
      h('div', { class: 'vid' },
        h('b', { text: d?.title || '' }),
        h('div', { class: 'vmeta' },
          d?.durationMs > 0 && h('span', { text: V.fmtClock(d.durationMs) }),
          h('span', { text: `${sty} · ${outLabel() || '跟随视频'}` }),
          r && S.restored ? h('span', { class: 'saved', text: `✓ ${ago(S.restored)}生成，已保存` }) : null)),
      h('div', { class: 'ctrl stuck' },
        h('div', { class: 'seg', role: 'group', 'aria-label': '摘要风格' },
          ...V.STYLES.map(([v, n]) => h('button', { class: v === S.prefs.style ? 'on' : '', 'aria-pressed': v === S.prefs.style, text: n, disabled: S.busy, onclick: () => ((S.prefs.style = v), savePrefs(), render()) }))),
        h('div', { class: 'crow' },
          h('span', { class: 'mute', text: '输出语言' }),
          langSelect(`输出语言：摘要、导图、卡片、问答都用这个语言写。“跟随视频”＝用${S.status?.lang || '中文'}，中文视频译成英文`, () => render()),
          h('span', { class: 'grow' }),
          h('button', { class: r ? 'sec' : '', text: r ? '重新生成' : '生成摘要', disabled: S.busy, onclick: () => task('summary', {}, (x) => (S.res.summary = x)) }))),
    )
    if (!r) {
      box.append(h('div', { class: 'empty' }, h('b', { text: '还没有摘要' }), h('p', { class: 'mute', text: '读取字幕后，由清影用你设置的大模型生成：总结、要点、关键词和章节。长视频会自动分段处理。' })))
      return box
    }
    if (r.failed?.length) {
      box.append(h('div', { class: 'warn' }, h('span', { text: `有 ${r.failed.length} 段没生成成功（从 ${r.failed.filter(Boolean).join('、') || '部分位置'} 起），摘要不完整。` }),
        r.failedRanges?.length ? h('button', { text: '只补这几段', disabled: S.busy, onclick: () => retryFailed() }) : null))
    }
    const { lead, paras } = V.splitSummary(r.summary)
    if (lead) box.append(h('div', { class: 'lead' }, h('div', { class: 'k', text: '一句话看完' }), h('p', { text: lead })))
    if (paras.length) box.append(h('div', {}, h('div', { class: 'sh', text: '总结' }), h('div', { class: 'prose' }, ...paras.map((p) => h('p', { text: p })))))
    if (r.keywords?.length) box.append(h('div', { class: 'kw' }, ...r.keywords.map((k) => h('span', { text: k }))))
    if (r.points?.length) {
      box.append(h('div', {},
        h('div', { class: 'sh' }, '要点', h('em', { text: ` ${r.points.length} 条${isVideo() ? '，点时间跳转' : ''}` }),
          r.chapters?.length ? h('button', { class: 'lnk', text: '看章节 ›', onclick: () => ((S.tab = 'chapters'), render()) }) : null),
        h('ul', { class: 'pts' }, ...r.points.map((p) => {
          const x = V.parsePoint(p)
          const ms = x.time ? V.parseClock(x.time) : -1
          return h('li', { class: ms >= 0 && isVideo() ? 'go' : '', onclick: () => ms >= 0 && seek(ms) },
            h('span', { class: 'tm', text: x.time || '·' }),
            h('div', {}, x.title && h('b', { text: x.title }), x.detail && h('span', { class: x.title ? 'd' : '', text: x.detail })))
        }))))
    }
    box.append(h('div', { class: 'acts' },
      h('button', { text: '复制 Markdown', onclick: () => copy(V.toMarkdown(exportData()), ' Markdown') }),
      h('button', { class: 'sec', text: '下载 .md', onclick: () => saveFile(V.fileName(d.title + ' 摘要', 'md'), V.toMarkdown(exportData()), 'text/markdown') })))
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
    const box = h('div', {},
      h('div', { class: 'mute', style: 'margin-bottom:6px', text: `来源：${from}，共 ${list.length} 章` }),
      h('div', { class: 'chs' }, ...list.map((c, i) => {
        const end = list[i + 1]?.startMs ?? Infinity
        const qs = (S.res.quotes || []).filter((q) => q.t >= (c.startMs ?? 0) && q.t < end)
        return h('div', { class: `ln${i === now ? ' cur' : ''}`, 'data-ch': i, onclick: () => seek(c.startMs) },
          h('span', { class: 'tm', text: V.fmtClock(c.startMs) }),
          h('div', {}, h('b', { text: c.title }), ...qs.map((q) => h('div', { class: 'quote' }, `“${q.text}” `, tsBtn(q.t)))))
      })),
      !S.res.quotes && h('div', { class: 'mute', style: 'margin-top:6px', text: '在“更多”里提取金句后，会按章节显示在这里。' }),
      actsBar(
        h('button', { text: '复制章节列表', onclick: () => copy(list.map((c) => `${V.fmtClock(c.startMs)} ${c.title}`).join('\n'), '章节') }),
        h('button', { class: 'sec', text: '去“摘要”重新生成', onclick: () => ((S.tab = 'summary'), render()) })),
    )
    return box
  }

  const getSel = () => String((ui.root.getSelection && ui.root.getSelection()) || getSelection() || '').trim()

  function tabTranscript() {
    const d = S.data
    const found = S.query.trim() ? V.searchLines(d.lines, S.query) : []
    const hits = found.length ? new Set(found) : null
    if (S.hit >= found.length) S.hit = 0
    const nowIdx = found[S.hit]
    const tr = S.res.translated
    const mode = S.prefs.view
    const body = h('div', { id: 'lines', onmouseup: onSelect })
    d.lines.forEach((l, i) => {
      const [main, sub] = V.viewLine(l.text, tr?.[i], mode)
      body.append(h('div', { class: `ln${hits?.has(i) ? ' hit' : ''}${i === nowIdx ? ' now' : ''}`, 'data-i': i, onclick: () => !getSel() && seek(l.t) },
        isVideo() && h('span', { class: 'tm', text: V.fmtClock(l.t) }),
        h('span', { style: 'flex:1' }, ...marks(main), sub ? h('span', { class: 'tr' }, ...marks(sub)) : null),
        h('button', { class: 'rec', title: '把这句记成笔记', text: '＋', onclick: (e) => (e.stopPropagation(), addNote({ t: l.t, text: l.text })) })))
    })
    return h('div', {},
      h('div', { class: 'stick' }, h('div', { class: 'tools' },
        h('input', { type: 'text', id: 'tsearch', placeholder: '搜索文稿（/）', value: S.query, style: 'flex:1', oninput: (e) => ((S.query = e.target.value), (S.hit = 0), (S.keepFocus = true), (S.jumpHit = true), render()), onkeydown: (e) => e.key === 'Enter' && !e.isComposing && (e.preventDefault(), stepHit(e.shiftKey ? -1 : 1)) }),
        found.length > 0 && h('span', { class: 'mute', text: `${S.hit + 1}/${found.length}` }),
        S.query.trim() && !found.length && h('span', { class: 'mute', text: '没有' }),
        found.length > 1 && h('button', { class: 'sec', text: '↑', title: '上一处（Shift+Enter）', onclick: () => stepHit(-1) }),
        found.length > 1 && h('button', { class: 'sec', text: '↓', title: '下一处（Enter）', onclick: () => stepHit(1) }),
      ),
      h('div', { class: 'tools' },
        S.detached && S.prefs.follow && isVideo() && h('button', { text: '回到当前句', onclick: () => ((S.detached = false), tick(true)) }),
        isVideo() && h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.follow, onchange: (e) => ((S.prefs.follow = e.target.checked), savePrefs()) }), ' 跟随播放'),
        langSelect('翻译成哪种语言（任意语言互译，自动识别原文）', () => ((S.res.translated = null), render())),
        h('button', { class: 'sec', text: tr ? '重新翻译' : outLabel() ? `翻译成${outLabel()}` : '翻译（自动方向）', disabled: S.busy, onclick: () => task('translate', {}, (x) => (S.res.translated = d.lines.map((_, i) => x.lines?.[i]?.text || ''))) }),
        tr && sel([['both', '双语'], ['orig', '只看原文'], ['tr', '只看译文']], mode, (v) => ((S.prefs.view = v), savePrefs(), render()), '显示'),
        h('button', { class: 'sec', text: d.raw ? '还原顺句前' : '顺句', title: d.raw ? '恢复识别出来的原样' : '给没有标点的字幕（自动识别）补标点、改同音错字，行数不变', disabled: S.busy, onclick: d.raw ? unpunctuate : punctuate }),
      )),
      h('div', { class: 'mute', text: '选中一段文字可以解释、存为笔记；鼠标移到一行上点 ＋ 记笔记。' }),
      body,
      actsBar(
        h('button', { text: '复制文稿', onclick: () => copy(V.toTxt(d.lines), '文稿') }),
        isVideo() && h('button', { class: 'sec', text: '.srt', onclick: () => saveFile(V.fileName(d.title, 'srt'), V.toSrt(d.lines)) }),
        h('button', { class: 'sec', text: '.txt', onclick: () => saveFile(V.fileName(d.title, 'txt'), V.toTxt(d.lines)) })),
    )
  }

  /** 把文字里的搜索词用 <mark> 标出来。 */
  function marks(text) {
    return V.splitMarks(text, S.query).map((p) => (p.hit ? h('mark', { text: p.s }) : p.s))
  }

  function stepHit(d) {
    const n = V.searchLines(S.data.lines, S.query).length
    if (!n) return
    S.hit = (S.hit + d + n) % n
    S.jumpHit = true
    S.keepFocus = ui.root.activeElement?.id === 'tsearch'
    render()
  }


  async function punctuate() {
    const before = S.data.lines
    await task('punctuate', {}, (x) => {
      const out = (x.lines || []).map((l) => ({ t: l.t, text: l.text })).filter((l) => l.text)
      if (!out.length) throw new Error('没有得到结果')
      S.data.raw = before
      S.data.lines = out
      S.res.translated = null
    }, false)
  }

  function unpunctuate() {
    S.data.lines = S.data.raw
    S.data.raw = null
    S.res.translated = null
    render()
  }

  // ---------- 划词：解释 / 存为笔记 ----------

  function onSelect(e) {
    const text = getSel()
    ui.root.getElementById('selbar')?.remove()
    if (!text || text.length > 800) return
    const row = e.target.closest?.('.ln')
    const i = row ? Number(row.dataset.i) : -1
    const t = i >= 0 ? S.data.lines[i].t : -1
    const bar = h('div', { class: 'selbar', id: 'selbar' },
      h('div', { class: 'mute', text: `“${text.length > 80 ? text.slice(0, 80) + '…' : text}”` }),
      h('div', { class: 'tools', style: 'margin:4px 0 0' },
        h('button', { text: '解释', onclick: () => explain(text, t, bar) }),
        h('button', { class: 'sec', text: '存为笔记', onclick: () => (addNote({ t, text }), bar.remove()) }),
        h('button', { class: 'sec', text: '✕', 'aria-label': '关闭', onclick: () => bar.remove() })),
      h('div', { id: 'selans' }))
    ui.root.querySelector('.body')?.prepend(bar)
  }

  async function explain(text, t, bar) {
    const out = bar.querySelector('#selans')
    out.replaceChildren(h('div', { class: 'mute', text: '解释中…' }))
    try {
      const c = V.contextAround(S.data.lines, t >= 0 ? t : 0, 3)
      const r = await run('explain', { text, context: c.full }, false)
      out.replaceChildren(h('div', { class: 'card', text: r.text }),
        h('button', { class: 'sec', text: '把解释记进笔记', onclick: () => addNote({ t, text: `${text}\n→ ${r.text}`, kind: 'note' }) }))
    } catch (e) {
      out.replaceChildren(h('div', { class: 'warn', text: e.message }))
    }
  }

  // ---------- 笔记 ----------

  async function addNote({ t = -1, text, kind = 'quote' }) {
    const n = V.makeNote({ key: localKey(), url: S.data.url, title: S.data.title, owner: S.data.owner, t, text, kind })
    if (!n) return false
    const before = S.notes.length
    S.notes = V.addNote(S.notes, n)
    await store.set('vsumNotes', S.notes)
    toast(ui, S.notes.length > before ? '已记下' : '这条已经记过了')
    if (S.tab === 'notes') render()
    return true
  }

  /** n 键 / 悬浮“笔记”：记下当前播放位置（往前提一点）那一句。 */
  function quickNote() {
    const v = video()
    if (!v || !S.data) return false
    const ms = Math.max(0, v.currentTime * 1000 - (Number(S.prefs.lead) || 0) * 1000)
    const i = V.lineAt(S.data.lines, ms)
    addNote({ t: ms, text: i >= 0 ? S.data.lines[i].text : '（此处没有字幕）' })
    return true
  }

  function onKey(e) {
    if (e.key === '/' && S.tab === 'transcript' && S.data && !e.ctrlKey && !e.metaKey && !e.altKey && !e.isComposing) {
      const t = e.composedPath?.()[0] || e.target
      if (t && (/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable)) return
      const box = ui.root.getElementById?.('tsearch') || ui.root.querySelector('#tsearch')
      if (box) (e.preventDefault(), e.stopPropagation(), box.focus())
      return
    }
    if (e.key !== 'n' || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.isComposing) return
    const el = e.composedPath?.()[0] || e.target
    if (el && (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName) || el.isContentEditable)) return
    if (isVideo() && quickNote()) e.preventDefault()
  }

  const scopedNotes = () => (S.scope === 'video' ? S.notes.filter((n) => n.key === localKey()) : S.scope === 'series' ? S.notes.filter((n) => V.baseKey(n.key) === V.baseKey(localKey())) : S.notes)
  const shownNotes = () => V.searchNotes(scopedNotes(), S.noteQuery)

  async function updateNote(id, patch) {
    S.notes = S.notes.map((n) => (n.id === id ? { ...n, ...patch } : n))
    await store.set('vsumNotes', S.notes)
  }

  async function polish(n) {
    const c = V.contextAround(S.data.lines, Math.max(n.t, 0), 3)
    const refine = n.kind === 'note'
    try {
      const r = await run(refine ? 'refine' : 'note', { text: refine ? n.text : c.target || n.text, context: c.full }, false)
      S.preview = { id: n.id, quote: r.quote }
    } catch (e) {
      toast(ui, e.message, true, 5200)
    }
    render()
  }

  function noteCard(n) {
    const same = n.key === localKey()
    const stamp = n.t >= 0 ? (same && isVideo() ? h('button', { class: 'ts', text: V.fmtClock(n.t), onclick: () => seek(n.t) }) : h('a', { class: 'ts', href: V.timeLink(n.url, n.t), target: '_blank', rel: 'noopener', text: V.fmtClock(n.t) })) : null
    const pv = S.preview?.id === n.id ? S.preview : null
    const editing = S.editing === n.id
    const ta = editing ? h('textarea', { rows: 3, id: `ed-${n.id}`, text: n.text }) : null
    if (ta) ta.value = n.text
    return h('div', { class: 'note' },
      S.scope !== 'video' && h('div', { class: 'mute', text: [n.title, V.partOf(n.key) > 1 ? `P${V.partOf(n.key)}` : '', n.owner].filter(Boolean).join(' · ') }),
      h('div', {}, stamp, ' ', n.kind === 'note' ? h('span', { text: ta ? '' : n.text, style: 'white-space:pre-wrap' }) : h('span', { class: 'q', text: ta ? '' : n.text })),
      ta,
      pv && h('div', { class: 'card', style: 'margin-top:6px' }, h('div', { class: 'mute', text: 'AI 整理后：' }), pv.quote,
        h('div', { class: 'x' },
          h('button', { text: '替换', onclick: async () => (await updateNote(n.id, { text: pv.quote }), (S.preview = null), render()) }),
          h('button', { class: 'sec', text: '追加在后面', onclick: async () => (await updateNote(n.id, { text: `${n.text}\n${pv.quote}`, kind: 'note' }), (S.preview = null), render()) }),
          h('button', { class: 'sec', text: '不要', onclick: () => ((S.preview = null), render()) }))),
      h('div', { class: 'x' },
        ...(editing
          ? [h('button', { text: '保存', onclick: async () => (await updateNote(n.id, { text: ui.root.getElementById(`ed-${n.id}`).value.trim() || n.text, kind: 'note' }), (S.editing = null), render()) }), h('button', { class: 'sec', text: '取消', onclick: () => ((S.editing = null), render()) })]
          : [h('button', { class: 'sec', text: '编辑', onclick: () => ((S.editing = n.id), render()) }),
            same && S.data.lines.length && h('button', { class: 'sec', text: 'AI 整理', disabled: S.busy, title: '整理成完整通顺的句子，不加原话没有的内容；先预览再决定', onclick: () => polish(n) })]),
        h('button', { class: 'sec', text: '复制', onclick: () => copy(n.text, '笔记') }),
        n.t >= 0 && n.url && h('button', { class: 'sec', text: '复制带时间的链接', onclick: () => copy(V.timeLink(n.url, n.t), '链接') }),
        h('button', { class: 'sec', text: '删除', onclick: async () => ((S.notes = S.notes.filter((x) => x.id !== n.id)), await store.set('vsumNotes', S.notes), render()) })))
  }


  // ---------- 评论分析 ----------

  const CM_KEEP = 6

  async function cmLoadSaved() {
    const k = localKey()
    if (S.cmLoading) return
    S.cmLoading = true
    try {
      const all = await store.get('vsumComments', {})
      const old = S.cm
      S.cm = { key: k, list: all[k]?.list || [], res: all[k]?.res || null, sort: old?.sort || 'hot', count: old?.count || 300, loading: false, n: 0, err: '', ask: [], stop: false, asking: false }
    } finally {
      S.cmLoading = false
    }
    render()
  }

  async function cmSave() {
    const c = S.cm
    const all = await store.get('vsumComments', {})
    delete all[c.key]
    all[c.key] = { list: c.list, res: c.res, ts: Date.now() }
    const keys = Object.keys(all)
    for (const k of keys.slice(0, Math.max(0, keys.length - CM_KEEP))) delete all[k]
    await store.set('vsumComments', all)
  }

  const CM_GAP = 90 * 1000 // 同站点两次读取至少隔 90 秒
  const CM_COOL = 15 * 60 * 1000 // 被限流后冷却 15 分钟
  const cmSite = () => V.videoIdOf(S.data?.url || location.href)?.site || ''

  /** 返回还需等待的毫秒数（0 = 可以读）。 */
  async function cmWait() {
    const m = (await store.get('vsumCmGate', {}))[cmSite()] || {}
    return Math.max(0, (m.until || 0) - Date.now(), (m.last || 0) + CM_GAP - Date.now())
  }

  async function cmGate(patch) {
    const all = await store.get('vsumCmGate', {})
    all[cmSite()] = { ...(all[cmSite()] || {}), ...patch }
    await store.set('vsumCmGate', all)
  }

  async function cmRead() {
    const c = S.cm
    const wait = await cmWait()
    if (wait > 0) {
      c.err = `读取太频繁，为避免被站点限制，请 ${Math.ceil(wait / 1000) >= 120 ? `${Math.ceil(wait / 60000)} 分钟` : `${Math.ceil(wait / 1000)} 秒`}后再试。已读取的评论可以直接分析。`
      render()
      return
    }
    await cmGate({ last: Date.now() })
    c.loading = true
    c.err = ''
    c.n = 0
    c.stop = false
    render()
    try {
      c.list = await globalThis.ccVsumSource.loadComments({ count: c.count, sort: c.sort }, (n) => ((c.n = n), !S.closed && S.tab === 'comments' && cmProgress()), () => c.stop || S.closed)
      c.res = null
      c.ask = []
      await cmSave()
    } catch (e) {
      c.err = e.block ? `${e.message}。已暂停 15 分钟，已读取的评论仍可分析。` : e.message
      if (e.block) await cmGate({ until: Date.now() + CM_COOL })
    } finally {
      c.loading = false
      if (!S.closed) render()
    }
  }

  function cmProgress() {
    const el = ui.root.getElementById('cmprog')
    if (el) el.textContent = `已读 ${S.cm.n} / ${S.cm.count}`
  }

  async function cmAnalyze() {
    const c = S.cm
    if (S.busy || !c.list.length) return
    S.busy = true
    render()
    try {
      await ensureReady()
      const comments = c.list.map((x) => ({ author: x.author, text: x.text, likes: x.likes, t: x.t, replies: x.replies }))
      c.res = await submit({ kind: 'comments', title: S.data?.title || document.title, lang: outLang(), comments }, 'comments')
      await cmSave()
    } catch (e) {
      if (!S.closed) toast(ui, e.message, true, 5200)
    } finally {
      S.busy = false
      if (!S.closed) render()
    }
  }

  async function cmAsk(q) {
    const c = S.cm
    q = String(q || '').trim()
    if (!q || S.busy || !c.list.length) return
    S.busy = true
    c.ask.push({ q, a: '' })
    render()
    try {
      await ensureReady()
      const comments = c.list.map((x) => ({ author: x.author, text: x.text, likes: x.likes, t: x.t, replies: x.replies }))
      const r = await submit({ kind: 'askcomments', title: S.data?.title || document.title, lang: outLang(), question: q, comments }, '')
      c.ask[c.ask.length - 1].a = r.answer
    } catch (e) {
      c.ask.pop()
      if (!S.closed) toast(ui, e.message, true, 5200)
    } finally {
      S.busy = false
      if (!S.closed) render()
    }
  }

  const cmQuote = (q) =>
    q &&
    h('div', { class: 'cq' }, q.text,
      h('div', { class: 'm' },
        h('span', { text: [q.author && `@${q.author}`, q.likes ? `赞 ${q.likes}` : ''].filter(Boolean).join(' · ') }),
        q.t >= 0 && h('button', { class: 'sec', text: V.fmtClock(q.t), title: '跳到视频里的这个时间', onclick: () => seek(q.t) })))

  function tabComments() {
    if (S.cm?.key !== localKey()) {
      cmLoadSaved()
      return h('div', { class: 'mute', text: '…' })
    }
    const c = S.cm
    const r = c.res
    const supported = ['youtube', 'bilibili'].includes(V.videoIdOf(S.data?.url || location.href)?.site)
    const head = h('div', { class: 'card' },
      h('div', { class: 'tools' },
        h('b', { text: c.list.length ? `已读取 ${c.list.length} 条评论` : '读取评论' }),
        h('span', { class: 'mute', style: 'flex:1' }),
        sel([['100', '100 条'], ['300', '300 条'], ['500', '500 条'], ['1000', '1000 条']], String(c.count), (v) => ((c.count = Number(v)), savePrefs()), '读取多少条'),
        sel([['hot', '热门'], ['new', '最新']], c.sort, (v) => (c.sort = v), '排序')),
      !c.list.length && h('div', { class: 'mute', text: supported ? '读取这个视频的评论后，可以分析整体情绪、主要观点、观众提问和纠错补充。评论只在本机的清影里处理，不会上传到别处。' : '这个站点暂不支持直接读取评论（目前支持 YouTube 和哔哩哔哩）。' }),
      c.err && h('div', { class: 'warn', text: c.err }),
      c.loading
        ? h('div', { class: 'tools' }, h('span', { class: 'mute', id: 'cmprog', text: `已读 ${c.n} / ${c.count}` }), h('button', { class: 'sec', text: '停止', onclick: () => (c.stop = true) }))
        : h('div', { class: 'tools' },
            ...(!c.list.length
              ? [h('button', { text: '读取并分析', disabled: !supported || S.busy, onclick: async () => (await cmRead(), S.cm.list.length && cmAnalyze()) }), h('button', { class: 'sec', text: '只读取', disabled: !supported || S.busy, onclick: cmRead })]
              : [h('button', { text: r ? '重新分析' : '开始分析', disabled: S.busy, onclick: cmAnalyze }), h('button', { class: 'sec', text: '重新读取', disabled: !supported || S.busy, onclick: cmRead })])))
    const out = [head]
    if (r) {
      const sm = r.sentiment
      out.push(
        h('div', { class: 'card' },
          h('div', { class: 'tools' }, h('b', { text: '整体情绪' }), h('span', { class: 'mute', text: `基于 ${r.count} 条评论` })),
          h('div', { class: 'cmbar' }, h('i', { class: 'p', style: `width:${sm.pos}%` }), h('i', { class: 'u', style: `width:${sm.neu}%` }), h('i', { class: 'n', style: `width:${sm.neg}%` })),
          h('div', { class: 'cmleg' }, h('span', { text: `正面 ${sm.pos}%` }), h('span', { text: `中性 ${sm.neu}%` }), h('span', { text: `负面 ${sm.neg}%` })),
          r.verdict && h('div', { style: 'margin-top:6px', text: r.verdict })))
      if (r.themes?.length) {
        const max = Math.max(1, ...r.themes.map((t) => t.count))
        out.push(h('div', { class: 'card' }, h('b', { text: '主要观点' }),
          ...r.themes.map((t) => h('div', { class: 'cmth' },
            h('div', { class: 'tools' }, h('b', { text: t.name, style: 'flex:1' }), h('span', { class: 'mute', text: `${t.count} 人` })),
            h('div', { class: 'cmb' }, h('i', { style: `width:${Math.round((t.count / max) * 100)}%` })),
            ...(t.quotes || []).map(cmQuote)))))
      }
      if (r.disputes?.length) {
        out.push(h('div', { class: 'card' }, h('b', { text: '争议点' }),
          ...r.disputes.map((d) => h('div', { class: 'cmth' }, h('div', {}, h('span', { class: 'tg no', text: '分歧' }), d.topic, h('span', { class: 'mute', text: `  支持 ${d.pro} · 质疑 ${d.con}` })), ...(d.quotes || []).map(cmQuote)))))
      }
      if (r.questions?.length) {
        const open = r.questions.filter((x) => !x.answered).length
        out.push(h('div', { class: 'card' }, h('div', { class: 'tools' }, h('b', { text: '观众提问' }), h('span', { class: 'mute', text: `${r.questions.length} 个 · ${open} 个未解答` })),
          ...r.questions.map((x) => h('div', { class: 'cmq' }, h('span', { class: `tg ${x.answered ? 'ok' : 'no'}`, text: x.answered ? '已回答' : '未解答' }), h('span', { text: x.q })))))
      }
      if (r.corrections?.length) {
        out.push(h('div', { class: 'card' }, h('b', { text: '纠错与补充' }),
          ...r.corrections.map((x) => h('div', { class: 'cmth' }, h('div', { text: x.text }), cmQuote(x.quote)))))
      }
      if (r.keywords?.length) out.push(h('div', { class: 'card' }, h('b', { text: '热词' }), h('div', { class: 'chips' }, ...r.keywords.map((k) => h('span', { class: 'chip', text: k })))))
    }
    if (c.list.length) {
      const inp = h('input', { type: 'text', placeholder: '问评论：例如“大家最希望作者下期讲什么？”', style: 'flex:1', onkeydown: (e) => e.key === 'Enter' && !e.isComposing && (cmAsk(inp.value), (inp.value = '')) })
      out.push(h('div', { class: 'card' }, h('b', { text: '问评论' }),
        ...c.ask.map((m) => h('div', { class: 'cmth' }, h('div', { class: 'mute', text: m.q }), h('div', { text: m.a || '正在想…' }))),
        h('div', { class: 'tools', style: 'margin-top:6px' }, inp, h('button', { text: '发送', disabled: S.busy, onclick: () => (cmAsk(inp.value), (inp.value = '')) }))))
    }
    if (r) {
      out.push(actsBar(
        h('button', { class: 'sec', text: '导出 Markdown', onclick: () => saveFile(V.fileName(`${S.data.title} 评论分析`, 'md'), V.commentsToMarkdown(S.data.title, r), 'text/markdown') }),
        h('button', { class: 'sec', text: '存为笔记', onclick: () => addNote({ t: -1, text: `评论分析：${r.verdict || ''}\n${(r.themes || []).map((t) => `- ${t.name}（${t.count} 人）`).join('\n')}`.trim(), kind: 'note' }) }),
        h('button', { class: 'sec', text: '复制', onclick: () => copy(V.commentsToMarkdown(S.data.title, r), '评论分析') })))
    }
    return h('div', {}, ...out)
  }

  // ---------- 生词页：字幕学习里收藏的词，按本视频 / 全部查看 ----------
  const VOCAB = { list: null, err: '', scope: 'video', hard: true, loading: false }
  async function loadVocab() {
    VOCAB.loading = true
    const r = await send({ type: 'cc-vocab', op: 'list', q: '' })
    VOCAB.loading = false
    if (!r?.ok) VOCAB.err = r?.error || '连不上清影应用'
    else {
      VOCAB.err = ''
      VOCAB.list = r.words || []
      VOCAB.stats = r.stats || {}
    }
    if (S.tab === 'vocab') render()
  }
  const sameVideo = (w) => {
    try {
      return V.videoKey(w.sourceUrl) === localKey()
    } catch {
      return w.sourceUrl === (S.data?.url || location.href)
    }
  }
  const shownVocab = () => {
    let l = VOCAB.list || []
    if (VOCAB.scope === 'video') l = l.filter(sameVideo)
    if (VOCAB.hard) l = l.filter((w) => w.level && w.level !== '基础')
    return l
  }
  const fmtClock = (sec) => {
    const t = Math.max(0, Math.floor(sec || 0))
    return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`
  }
  function vocabCard(w) {
    const here = sameVideo(w)
    return h('div', { class: 'card vcard' },
      h('div', { class: 'vh' },
        h('b', { text: w.word }),
        w.phonetic ? h('span', { class: 'mute', text: w.phonetic }) : null,
        w.level ? h('span', { class: 'chip', text: w.level }) : null),
      h('div', { text: `${w.pos ? w.pos + ' ' : ''}${w.meaning || ''}${w.ctxMeaning ? `（本句：${w.ctxMeaning}）` : ''}` }),
      w.sentence ? h('div', { class: 'mute', text: w.sentence }) : null,
      h('div', { class: 'vf' },
        here ? h('button', { class: 'ts', text: `跳到 ${fmtClock(w.timeSec)}`, onclick: () => seek((w.timeSec || 0) * 1000) }) : h('span', { class: 'mute', text: `${(w.sourceTitle || '').slice(0, 24)} ${fmtClock(w.timeSec)}` })))
  }
  function tabVocab() {
    if (!VOCAB.list && !VOCAB.loading && !VOCAB.err) void loadVocab()
    const list = shownVocab()
    const exportAnki = async () => {
      const r = await send({ type: 'cc-vocab', op: 'export', format: 'anki' })
      if (!r?.ok) return toast(ui, r?.error || '导出失败', true)
      saveFile(`生词本.${r.ext}`, r.text)
    }
    return h('div', {},
      h('div', { class: 'stick' }, h('div', { class: 'tools' },
        sel([['video', '本视频'], ['all', '全部视频']], VOCAB.scope, (v) => ((VOCAB.scope = v), render()), '范围'),
        h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: VOCAB.hard ? true : null, onchange: (e) => ((VOCAB.hard = e.target.checked), render()) }), ' 只看难词'),
        h('span', { class: 'mute', text: VOCAB.loading ? '读取中…' : `${list.length} 个` }))),
      VOCAB.err ? h('div', { class: 'mute', text: `${VOCAB.err}。生词保存在清影应用里，先把应用开着。` }) : null,
      !VOCAB.err && VOCAB.list && !list.length ? h('div', { class: 'mute', text: '还没有收藏的词。看视频时在字幕上悬停生词，点“收藏到生词本”。' }) : null,
      ...list.map(vocabCard),
      actsBar(
        h('button', { text: '导出 Anki', disabled: !list.length, onclick: exportAnki }),
        h('button', { class: 'sec', text: '存为笔记', disabled: !list.length, onclick: () => addNote({ t: -1, text: list.map((w) => `- ${w.word} ${w.phonetic || ''} ${w.meaning || ''}`.trim()).join('\n'), kind: 'note' }) }),
        h('button', { class: 'sec', text: '刷新', onclick: () => ((VOCAB.list = null), loadVocab()) }),
        h('button', { class: 'sec', text: '去应用里复习', title: '复习在清影应用的“生词本”页', onclick: () => toast(ui, '打开清影应用 → 生词本，就能复习今天到期的词') })),
    )
  }

  function tabNotes() {
    const list = V.sortNotes(shownNotes())
    const mine = S.notes.filter((n) => n.key === localKey())
    return h('div', {},
      h('div', { class: 'stick' }, h('div', { class: 'tools' },
        sel([['video', '本视频'], ['series', '本视频的所有分P'], ['all', '全部视频']], S.scope, (v) => ((S.scope = v), render()), '范围'),
        h('input', { type: 'text', placeholder: '搜索笔记（正文、标题、作者）', value: S.noteQuery, style: 'flex:1', oninput: (e) => ((S.noteQuery = e.target.value), (S.keepFocus = true), render()) }),
        h('span', { class: 'mute', text: `${list.length} 条` }))),
      !list.length && h('div', { class: 'mute', text: `还没有笔记。播放时按 n 键记下当前这句；在文稿里选中文字也能存。` }),
      ...list.map(noteCard),
      actsBar(
        isVideo() && h('button', { text: '记下当前这句', title: '快捷键 n', onclick: () => quickNote() || toast(ui, '页面上没有找到视频', true) }),
        h('button', { class: 'sec', text: '导出笔记 (.md)', disabled: !list.length, onclick: () => saveFile(V.fileName(S.scope !== 'all' ? S.data.title + ' 笔记' : '视频笔记', 'md'), V.notesToMarkdown(list, { title: S.scope === 'video' ? S.data.title : '视频笔记' }), 'text/markdown') }),
        h('button', { class: 'sec', text: '导出学习稿', disabled: !S.data.lines.length && !mine.length, title: '摘要 + 章节 + 金句 + 我的笔记，带 YAML 头，适合放进 Obsidian', onclick: () => saveFile(V.fileName(S.data.title + ' 学习稿', 'md'), V.studyDoc(exportData(false), mine), 'text/markdown') })),
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

  /** 全屏查看导图：滚轮缩放（以鼠标为中心）、拖动平移、双击适应窗口、+ / − / 0 键。 */
  function openMapViewer(svgText) {
    const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml').documentElement
    const W = Number(doc.getAttribute('width')) || 800
    const H = Number(doc.getAttribute('height')) || 600
    doc.removeAttribute('style')
    const stage = h('div', { class: 'stage' })
    stage.append(doc)
    doc.setAttribute('width', W)
    doc.setAttribute('height', H)
    doc.style.width = `${W}px`
    doc.style.height = `${H}px`
    let k = 1
    let x = 0
    let y = 0
    const apply = () => (doc.style.transform = `translate(${x}px, ${y}px) scale(${k})`)
    const fit = () => {
      const r = stage.getBoundingClientRect()
      k = Math.min(r.width / W, r.height / H, 1.5) * 0.96
      x = (r.width - W * k) / 2
      y = (r.height - H * k) / 2
      apply()
    }
    const zoomAt = (f, cx, cy) => {
      const nk = Math.min(6, Math.max(0.1, k * f))
      x = cx - ((cx - x) / k) * nk
      y = cy - ((cy - y) / k) * nk
      k = nk
      apply()
    }
    const center = (f) => {
      const r = stage.getBoundingClientRect()
      zoomAt(f, r.width / 2, r.height / 2)
    }
    stage.addEventListener('wheel', (e) => {
      e.preventDefault()
      const r = stage.getBoundingClientRect()
      zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - r.left, e.clientY - r.top)
    }, { passive: false })
    const pts = new Map()
    let pinch = 0
    stage.addEventListener('pointerdown', (e) => {
      stage.setPointerCapture(e.pointerId)
      pts.set(e.pointerId, { x: e.clientX, y: e.clientY })
      stage.classList.add('drag')
      pinch = 0
    })
    stage.addEventListener('pointermove', (e) => {
      const p = pts.get(e.pointerId)
      if (!p) return
      const dx = e.clientX - p.x
      const dy = e.clientY - p.y
      p.x = e.clientX
      p.y = e.clientY
      if (pts.size === 2) {
        const [a, b] = [...pts.values()]
        const d = Math.hypot(a.x - b.x, a.y - b.y)
        if (pinch) {
          const r = stage.getBoundingClientRect()
          zoomAt(d / pinch, (a.x + b.x) / 2 - r.left, (a.y + b.y) / 2 - r.top)
        }
        pinch = d
      } else {
        x += dx
        y += dy
        apply()
      }
    })
    const up = (e) => {
      pts.delete(e.pointerId)
      pinch = 0
      if (!pts.size) stage.classList.remove('drag')
    }
    stage.addEventListener('pointerup', up)
    stage.addEventListener('pointercancel', up)
    stage.addEventListener('dblclick', fit)
    const view = h('div', { class: 'layer mapview', role: 'dialog', 'aria-label': '思维导图' },
      h('div', { class: 'bar2' },
        h('b', { text: S.data?.title || '思维导图' }),
        h('button', { class: 'sec', text: '−', title: '缩小', onclick: () => center(1 / 1.25) }),
        h('button', { class: 'sec', text: '+', title: '放大', onclick: () => center(1.25) }),
        h('button', { class: 'sec', text: '适应窗口', onclick: fit }),
        h('button', { class: 'sec', text: '100%', onclick: () => { k = 1; x = 8; y = 8; apply() } }),
        h('button', { class: 'sec', text: '保存 SVG', onclick: () => saveFile(V.fileName(S.data.title, 'svg'), svgText, 'image/svg+xml') }),
        h('button', { class: 'sec', text: '保存 PNG', onclick: () => savePng(svgText, V.fileName(S.data.title, 'png')) }),
        h('button', { class: 'sec', text: '✕', title: '关闭（Esc）', 'aria-label': '关闭', onclick: close }),
      ),
      stage,
      h('div', { class: 'hint', text: '滚轮缩放 · 拖动平移 · 双击适应窗口 · 键盘 + − 0 · Esc 关闭' }),
    )
    function onKey(e) {
      if (e.key === 'Escape') close()
      else if (e.key === '+' || e.key === '=') center(1.25)
      else if (e.key === '-') center(1 / 1.25)
      else if (e.key === '0') fit()
      else return
      e.stopPropagation()
    }
    function close() {
      document.removeEventListener('keydown', onKey, true)
      view.remove()
    }
    document.addEventListener('keydown', onKey, true)
    ui.add(view)
    requestAnimationFrame(fit)
  }

  function tabMindmap() {
    const o = S.res.outline
    const gen = h('button', { class: o ? 'sec' : '', text: o ? '重新生成' : '生成思维导图', disabled: S.busy, onclick: () => task('mindmap', {}, (x) => (S.res.outline = x.outline)) })
    if (!o) return h('div', { class: 'sm' }, h('div', { class: 'empty' }, h('b', { text: '还没有思维导图' }), h('p', { class: 'mute', text: '生成层级大纲（最多 4 层，带具体细节），并画成可缩放、可导出的导图。' }), gen))
    const dark = isDark()
    const mk = (d) => V.mindmapSvg(V.layoutMindmap(V.outlineToTree(o, S.data.title)), d)
    const view = h('div', { class: 'svgbox', title: '点击全屏查看，可缩放、拖动', onclick: () => openMapViewer(mk(dark)) })
    view.append(new DOMParser().parseFromString(mk(dark), 'image/svg+xml').documentElement, h('div', { class: 'zoomhint', text: '点击缩略图全屏查看' }))
    return h('div', { class: 'sm' },
      h('div', { class: 'crow' }, h('span', { class: 'mute', text: '思维导图' }), h('span', { class: 'grow' }), gen),
      view,
      h('details', { class: 'acc', open: true }, h('summary', { text: '大纲文本' }), h('pre', { text: o })),
      actsBar(
        h('button', { text: '全屏查看 / 缩放', onclick: () => openMapViewer(mk(dark)) }),
        h('button', { class: 'sec', text: '保存 SVG', onclick: () => saveFile(V.fileName(S.data.title, 'svg'), mk(false), 'image/svg+xml') }),
        h('button', { class: 'sec', text: '保存 PNG', onclick: () => savePng(mk(false), V.fileName(S.data.title, 'png')) }),
        h('button', { class: 'sec', text: '复制大纲', onclick: () => copy(o, '大纲') })),
    )
  }

  function tabAsk() {
    const box = h('div')
    if (!S.chat.length) {
      box.append(h('div', { class: 'mute', text: '针对这个视频提问。回答里的 [m:ss] 可以点，直接跳到那一刻。' }),
        h('div', { class: 'sugg' }, ...V.SUGGESTIONS.map((q) => h('button', { class: 'sec', text: q, onclick: () => ask(q) }))))
    }
    for (const m of S.chat) {
      const cite = m.role === 'assistant' ? V.citations(m.text, S.data.lines) : null
      box.append(h('div', { class: `msg${m.role === 'user' ? ' u' : ''}` }, h('div', { class: 'b' }, ...linkify(m.text),
        ...(cite?.refs || []).map((r) => h('div', { class: 'src', title: '回答引用的原话，点一下跳到那里', onclick: () => seek(r.ms) }, `[${r.label}] ${r.text}`)),
        cite?.unsupported && h('div', { class: 'warn', text: '这条回答没有给出可核对的时间点，请当作参考，别全信。' }),
        cite?.bad && h('div', { class: 'warn', text: `有 ${cite.bad} 处时间点超出了文稿范围，已忽略。` }))))
    }
    if (S.chat.length) box.append(h('div', { class: 'tools' }, h('button', { class: 'sec', text: '清空对话', onclick: () => ((S.chat = []), persist(), render()) }), h('button', { class: 'sec', text: '复制对话', onclick: () => copy(S.chat.map((m) => `${m.role === 'user' ? '问' : '答'}：${m.text}`).join('\n\n'), '对话') })))
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
      grp('学习卡片（可导入 Anki）',
      h('div', { class: 'tools' },
        h('button', { text: cards ? '重新生成' : '生成问答卡片', disabled: S.busy, onclick: () => task('cards', {}, (x) => (S.res.cards = x.cards)) }),
        cards && h('button', { class: 'sec', text: '导出 CSV', onclick: () => saveFile(V.fileName(S.data.title, 'tsv'), V.cardsToCsv(cards)) }),
      ),
      ...(cards || []).map((c) => h('div', { class: 'card' }, h('b', { text: c.q }), '\n', c.a, ' ', tsBtn(c.t))),
      ),
      grp('金句',
      h('div', { class: 'tools' }, h('button', { text: quotes ? '重新生成' : '提取金句', disabled: S.busy, onclick: () => task('quotes', {}, (x) => (S.res.quotes = x.quotes)) })),
      ...(quotes || []).map((q) => h('div', { class: 'card' }, `“${q.text}” `, tsBtn(q.t))),
      ),
      grp('改写成其他形式',
      h('div', { class: 'tools' }, ...V.REWRITES.map(([f, n]) => h('button', { class: 'sec', text: n, disabled: S.busy || !S.res.summary, title: S.res.summary ? '' : '先在“摘要”里生成', onclick: () => task('rewrite', { format: f, text: V.toMarkdown(exportData()) }, (x) => (S.res.rewrite = { name: n, text: x.text }), false) }))),
      S.res.rewrite && h('div', {}, h('div', { class: 'card', text: S.res.rewrite.text }), h('button', { class: 'sec', text: `复制${S.res.rewrite.name}`, onclick: () => copy(S.res.rewrite.text, S.res.rewrite.name) })),
      ),
      grp('导出',
      h('div', { class: 'tools' },
        h('button', { class: 'sec', text: '下载 Markdown', onclick: () => saveFile(V.fileName(S.data.title, 'md'), V.toMarkdown(exportData(true)), 'text/markdown') }),
        h('button', { class: 'sec', text: '复制到剪贴板（含文稿）', onclick: () => copy(V.toMarkdown(exportData(true)), ' Markdown') }),
        h('button', { class: 'sec', text: '存进清影的便签', onclick: saveNote }),
        h('button', { class: 'sec', text: '发到 Obsidian', onclick: toObsidian }),
      ),
      ),
      grp('笔记与偏好',
      h('div', { class: 'tools' },
        sel([['auto', '跟随系统'], ['light', '浅色'], ['dark', '深色']], S.prefs.theme, (v) => ((S.prefs.theme = v), savePrefs(), applyAppearance(), render()), '主题'),
        sel([[12, '小'], [13, '标准'], [15, '大'], [17, '特大']], String(S.prefs.fs), (v) => ((S.prefs.fs = Number(v)), savePrefs(), applyAppearance()), '字号')),
      h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.pill, onchange: (e) => ((S.prefs.pill = e.target.checked), savePrefs(), tick()) }), ' 在视频上显示“笔记”按钮（也可以按 n 键）'),
      h('label', { class: 'mute' }, '记笔记时往前提 ', h('input', { type: 'number', min: 0, max: 15, value: S.prefs.lead, style: 'width:56px', onchange: (e) => ((S.prefs.lead = Math.min(15, Math.max(0, Number(e.target.value) || 0))), savePrefs()) }), ' 秒'),
      ),
      grp('提示词',
      promptEditor(),
      ),
      grp('数据',
      h('div', { class: 'tools' },
        h('button', { class: 'sec', text: '备份设置和笔记', title: '不包含任何密钥', onclick: backup }),
        h('button', { class: 'sec', text: '恢复备份', onclick: restore }),
        h('button', { class: 'sec', text: '清除摘要缓存（含各视频已生成的内容）', onclick: async () => (await store.set('vsumCache', {}), await store.set('vsumSessions', {}), await store.set('vsumJobs', {}), await store.set('vsumComments', {}), (S.cm = null), (S.restored = 0), toast(ui, '已清除')) }),
        h('button', { class: 'sec', text: '删除全部笔记', disabled: !S.notes.length, onclick: async () => confirm(`确定删除全部 ${S.notes.length} 条笔记？`) && ((S.notes = []), await store.set('vsumNotes', []), render()) })),
      ),
      grp('没有字幕 / 想换文稿',
      sourcePicker(!S.data.lines.length || !!S.wantSrc),
      h('div', { class: 'tools' },
        isVideo() && h('button', { class: 'sec', text: '用清影转写（语音识别）', onclick: transcribeNow }),
        isVideo() && h('button', { class: 'sec', text: '只把视频发给清影下载', onclick: sendForTranscribe }),
        h('button', { class: 'sec', text: '导入 .srt / .vtt / .txt', title: '导入后会按视频记住；在线字幕读得到时仍以在线字幕为准', onclick: importFile }),
        S.hasLocal && h('button', { class: 'sec', text: '忘记已存的本地字幕', onclick: forgetLocal }),
        S.data.tracks?.length > 1 && sel(S.data.tracks.map((t) => [t.url, `${t.name}${t.auto ? '（自动）' : ''}`]), S.data.trackUrl || S.data.tracks[0].url, reloadTrack, '字幕轨'),
        h('button', { class: 'sec', text: '总结选中的文字', onclick: () => loadArticleFromSelection() }),
      ),
      h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.autoOnline, onchange: (e) => ((S.prefs.autoOnline = e.target.checked), savePrefs()) }), ' 读不到字幕时，让清影从网站取在线字幕'),
      h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.autoTranscribe, onchange: (e) => ((S.prefs.autoTranscribe = e.target.checked), savePrefs()) }), ' 没有字幕时，自动取音频转写（后台进行；耗时，用本机 Whisper 不花钱，用接口会占额度）'),
      h('textarea', { rows: 4, placeholder: '或者把文字稿 / 字幕粘贴到这里，再点“使用”', id: 'paste' }),
      h('div', { class: 'tools' }, h('button', { class: 'sec', text: '使用粘贴的文字', onclick: () => {
        const t = ui.root.getElementById('paste')?.value
        if (!t?.trim()) return toast(ui, '先粘贴文字', true)
        const d = { ...Src.fromText(t), url: S.data.url, title: S.data.title }
        setData(d)
        rememberLocal('粘贴的文字', d)
      } })),
      ),
    )
    return box
  }

  const PROMPT_KINDS = [['summary', '摘要'], ['mindmap', '导图'], ['ask', '问答'], ['explain', '划词解释'], ['punctuate', '顺句'], ['note', '整理笔记'], ['refine', '优化笔记'], ['rewrite', '改写']]

  /** 自己写提示词：留空用内置的；可以用 {lang} 代表输出语言。 */
  function promptEditor() {
    const kind = S.promptKind || 'explain'
    const ta = h('textarea', { rows: 5, id: 'pt', maxlength: 20000, placeholder: '留空 = 用内置提示词。可用 {lang} 代表输出语言。' })
    ta.value = S.prefs.prompts?.[kind] || ''
    return h('div', {},
      h('div', { class: 'tools' },
        sel(PROMPT_KINDS, kind, (v) => ((S.promptKind = v), render()), '要改哪一类'),
        h('button', { class: 'sec', text: '保存', onclick: () => {
          const v = ui.root.getElementById('pt').value.trim()
          S.prefs.prompts = { ...S.prefs.prompts }
          if (v) S.prefs.prompts[kind] = v
          else delete S.prefs.prompts[kind]
          savePrefs()
          toast(ui, v ? '已保存，下次生成时使用' : '已改回内置提示词')
        } })),
      ta)
  }

  async function backup() {
    const { prompts, ...rest } = S.prefs
    saveFile('清影视频摘要备份.json', JSON.stringify({ app: 'clearclip-vsum', v: 1, prefs: { ...rest, prompts }, notes: S.notes }, null, 1), 'application/json')
  }

  function restore() {
    const i = h('input', { type: 'file', accept: '.json,application/json' })
    i.onchange = async () => {
      try {
        const j = JSON.parse(await i.files[0].text())
        if (j.app !== 'clearclip-vsum') throw new Error('不是清影的备份文件')
        if (j.prefs && typeof j.prefs === 'object') S.prefs = { ...S.prefs, ...j.prefs }
        if (Array.isArray(j.notes)) {
          for (const n of j.notes) S.notes = V.addNote(S.notes, V.makeNote(n))
          await store.set('vsumNotes', S.notes)
        }
        savePrefs()
        toast(ui, '已恢复')
        render()
      } catch (e) {
        toast(ui, `恢复失败：${e.message}`, true)
      }
    }
    i.click()
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

  const TABS = [['summary', '摘要'], ['chapters', '章节'], ['transcript', '文稿'], ['mindmap', '导图'], ['ask', '问答'], ['notes', '笔记'], ['vocab', '生词'], ['comments', '评论'], ['more', '更多']]

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
        h('span', { class: 't', title: d?.title || '', text: d ? d.title : '视频摘要' }),
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
      children.push(h('div', { class: 'tabs', role: 'tablist' }, ...TABS.map(([id, n]) => h('button', { class: S.tab === id ? 'on' : '', role: 'tab', 'aria-selected': S.tab === id, text: id === 'vocab' && VOCAB.list ? `${n} ${VOCAB.list.length}` : n, onclick: () => ((S.tab = id), savePrefs(), render()) }))))
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
  function tick(force) {
    const v = video()
    placePill(v)
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

  const pill = h('button', { class: 'pill', text: '笔记 (n)', title: '记下当前这句话', style: 'display:none', onclick: () => quickNote() })
  ui.add(pill)
  let pillSeen = 0
  document.addEventListener('mousemove', (e) => (globalThis.__ccPtr = { x: e.clientX, y: e.clientY, t: Date.now() }), { capture: true, passive: true })
  function placePill(v) {
    const r = v?.getBoundingClientRect()
    const ok = r && S.prefs.pill && S.data && isVideo() && r.width > 200 && r.height > 120 && r.bottom > 0 && r.top < innerHeight
    pill.style.display = ok ? '' : 'none'
    if (ok) {
      const cnt = S.notes.filter((n) => n.key === localKey()).length
      const lab = `笔记 (${cnt})`
      if (pill.textContent !== lab) pill.textContent = lab
      // 鼠标移出视频区域后淡出隐藏（离开 1.5 秒后）；刚出现时先显示 2.5 秒
      const m = globalThis.__ccPtr || { x: -1, y: -1, t: 0 }
      const inside = m.x >= r.left && m.x <= r.right && m.y >= r.top && m.y <= r.bottom
      if (inside) pillSeen = Date.now()
      if (!pillSeen) pillSeen = Date.now() + 1000
      const show = Date.now() - pillSeen < 1500
      pill.style.opacity = show ? '1' : '0'
      pill.style.pointerEvents = show ? '' : 'none'
      pill.style.left = `${Math.max(4, r.left + 8)}px`
      pill.style.top = `${Math.max(4, r.top + 8)}px`
    }
  }

  function setData(d) {
    S.data = d
    S.res = {}
    S.chat = []
    S.query = ''
    S.preview = null
    S.restored = 0
    render()
    if (!d.lines.length) return
    // 同一份文稿以前生成的所有结果和对话直接带出来（不需要大模型）；会话里没有的摘要再去缓存里找
    restoreSession(d)
      .then(async () => {
        if (S.data !== d) return
        if (!S.res.summary) {
          const hit = await cached(V.cacheKey(d.lines, 'summary', S.prefs.style, outLang()))
          if (hit && S.data === d && !S.res.summary) {
            S.res.summary = hit
            S.restored ||= Date.now()
          }
        }
        render()
        resumeJobs(d)
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
    saveSession().catch(() => {})
    ui.remove()
    delete globalThis.__ccVsum
  }

  for (const ev of ['wheel', 'touchmove']) {
    ui.root.addEventListener(ev, () => {
      if (S.tab === 'transcript' && S.prefs.follow && isVideo() && !S.detached && !S.query) {
        S.detached = true
        render()
      }
    }, { passive: true })
  }

  applyAppearance()
  globalThis.__ccVsum = { stop }
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
    return start()
  })
})()
