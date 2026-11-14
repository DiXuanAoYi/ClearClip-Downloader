// 页面字幕层：在任意网页的视频上叠一层字幕——可以双语、调字号和位置、对齐延迟、导出 SRT。
// 字幕来自 YouTube / B 站 / 播放器自带字幕轨，或让清影把音频转写出来；译文由清影用你设置的大模型翻译（走翻译缓存）。
// 再注入一次 = 关闭。
;(() => {
  const prev = globalThis.ccSubLayerRun
  if (prev) {
    prev.destroy()
    return
  }
  const { api, life, mount, h, send, toast } = globalThis.ccUI
  const L = globalThis.ccSubLayer
  const V = globalThis.ccVsum
  const VS = globalThis.ccVSession
  const WC = globalThis.ccWordCard

  const store = {
    async get(k, d) {
      try {
        return (await api.storage.local.get(k))[k] ?? d
      } catch (e) {
        globalThis.ccUI.log(`store.get ${k}`, e)
        return d
      }
    },
    // 写失败（多半是存储配额满了）：先清可再生成的缓存重试，还不行就提示用户，不再静默
    async set(k, v) {
      return globalThis.ccUI.storeSet(ui, { [k]: v })
    },
  }

  const CSS = `
    .cap { position: fixed; pointer-events: none; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 2px }
    .cap span { pointer-events: auto; font-size: inherit; display: inline-block; max-width: 92%; background: #000a; color: #fff; padding: 2px 10px; border-radius: 4px; white-space: pre-wrap; text-shadow: 0 1px 2px #000; line-height: 1.35 }
    .cap span.second { color: #ffe9a8 }
    .cap textarea.ledit { pointer-events: auto; width: min(92%, 42em); box-sizing: border-box; resize: none; overflow: hidden; font: inherit; line-height: 1.35; text-align: center; padding: 2px 10px; border: 2px solid #ffb547; border-radius: 4px; background: #000e; color: #fff }
    .menu { position: fixed; display: none; flex-direction: column; gap: 12px; width: 288px; box-sizing: border-box; padding: 14px; border-radius: 12px; border: 1px solid var(--line); background: var(--bg); color: var(--fg); font-size: 13px; box-shadow: 0 10px 30px #0006 }
    .menu.open { display: flex }
    .menu .row { display: flex; flex-direction: column; gap: 6px }
    .menu .lab { display: flex; justify-content: space-between; color: var(--mute); font-size: 12px }
    .menu .lab b { color: var(--fg); font-weight: 600 }
    .menu select { height: 30px; border: 1px solid var(--line); border-radius: 8px; background: var(--inp); color: var(--fg); padding: 0 8px; font-size: 13px }
    .menu .seg { display: flex; background: var(--inp); border: 1px solid var(--line); border-radius: 8px; padding: 2px; gap: 2px }
    .menu .seg button { flex: 1; padding: 5px 0; border-radius: 6px; font-size: 12px; font-weight: 400; background: transparent; color: var(--mute) }
    .menu .seg button.on { background: var(--acc); color: var(--onacc); font-weight: 600 }
    .menu input[type=range] { width: 100%; margin: 4px 0; accent-color: var(--acc) }
    .menu .sep { height: 1px; background: var(--line) }
    .menu .foot { display: flex; gap: 8px }
    .menu .foot button { flex: 1; height: 30px; border-radius: 8px; border: 1px solid var(--line); background: var(--inp); color: var(--fg); font-weight: 400 }
    .menu .foot button.p { background: var(--acc); border-color: var(--acc); color: var(--onacc); font-weight: 600 }
    .menu .tail { display: flex; justify-content: space-between; color: var(--mute); font-size: 12px }
    .menu .tail button { background: none; border: 0; padding: 0; color: var(--mute); font-weight: 400; cursor: pointer; text-decoration: underline }
    .menu .st { color: var(--mute); font-size: 12px; line-height: 1.4; word-break: break-all }
    .cap .w { font: inherit; font-style: normal; pointer-events: auto; cursor: pointer; border-radius: 3px; background: transparent; padding: 0; display: inline }
    .cap .w:hover { background: #ffffff40 }
    .cap .w.unf { border-bottom: 2px dashed #ffb547 }
    .cap .w.sv { border-bottom: 2px solid #4be3d0 }
    .ls button, .dr button.b { height: 28px; padding: 0 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--inp); color: var(--fg); font-weight: 400; font-size: 12.5px }
    .ls button.p { background: var(--acc); border-color: var(--acc); color: var(--onacc); font-weight: 600 }
    .ls { position: fixed; display: none; width: 420px; max-width: calc(100vw - 16px); box-sizing: border-box; padding: 12px 14px; border-radius: 12px; border: 1px solid var(--line); background: var(--bg); color: var(--fg); box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 8px }
    .ls.open { display: flex }
    .ls .r { display: flex; align-items: center; gap: 6px; flex-wrap: wrap }
    .ls .r .sp { flex: 1 }
    .ls select { height: 28px; border: 1px solid var(--line); border-radius: 8px; background: var(--inp); color: var(--fg); padding: 0 6px; font-size: 12.5px }
    .ls label { display: flex; align-items: center; gap: 4px; color: var(--mute); font-size: 12px }
    .ls textarea { width: 100%; height: 56px; resize: none; border: 1px solid var(--line); border-radius: 8px; background: var(--inp); color: var(--fg); padding: 6px 8px; font-size: 13px }
    .ls .res { line-height: 1.7; font-size: 14px }
    .ls .res .ok { color: var(--ok) }
    .ls .res .no { color: var(--bad); text-decoration: underline wavy }
    .ls .sc { color: var(--mute); font-size: 12px }
    .ls .pill2 { font-size: 11.5px; padding: 1px 8px; border-radius: 9px; background: var(--accsoft); color: var(--acc) }
    .ls .keys { color: var(--mute); font-size: 11.5px; line-height: 1.6 }
    .ls .keys kbd { border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 4px; padding: 0 5px; margin: 0 2px; background: var(--inp); font: inherit; font-size: 11px }
    .ls .chips { display: flex; gap: 6px; flex-wrap: wrap }
    .ls .chip { display: inline-flex; align-items: center; gap: 6px; padding: 2px 8px; border-radius: 9px; border: 1px solid var(--line); font-size: 12.5px }
    .ls .chip a { color: var(--acc); cursor: pointer }
    .ls .prog { display: flex; flex-direction: column; gap: 2px; max-height: 120px; overflow-y: auto; border-top: 1px solid var(--line); padding-top: 6px }
    .ls .prog .pr { display: flex; gap: 8px; font-size: 12px; color: var(--mute); cursor: pointer; padding: 1px 0 }
    .ls .prog .pr.cur { color: var(--acc) }
    .ls .prog .pr span:nth-child(2) { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .dr { position: fixed; display: none; width: 340px; max-height: 70vh; box-sizing: border-box; padding: 12px 14px; border-radius: 12px; border: 1px solid var(--line); background: var(--bg); color: var(--fg); box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 8px }
    .dr.open { display: flex }
    .dr input { height: 28px; border: 1px solid var(--line); border-radius: 8px; background: var(--inp); color: var(--fg); padding: 0 8px }
    .dr .lst { overflow-y: auto; display: flex; flex-direction: column; gap: 6px; min-height: 60px }
    .dr .it2 { border: 1px solid var(--line); border-radius: 8px; padding: 6px 8px; display: flex; flex-direction: column; gap: 2px }
    .dr .it2 b { color: var(--fg) }
    .dr .it2 small { color: var(--mute); font-size: 11.5px; line-height: 1.4 }
    .dr .it2 .ac { display: flex; gap: 10px; font-size: 12px }
    .dr .it2 .ac a { color: var(--acc); cursor: pointer; text-decoration: underline }
    .dr .ft { display: flex; gap: 6px; flex-wrap: wrap }
  ` + globalThis.ccWordCard.CSS
  const ui = mount(CSS)
  const S = {
    prefs: L.clampPrefs(null),
    lines: [],
    trans: [],
    video: null,
    data: null,
    status: '',
    busy: false,
    closed: false,
    target: '',
    pv: '',
    timer: 0,
    drag: false,
    on: true,
    seen: Date.now() + 1000,
    // 字幕学习
    learn: { on: true, level: 1 },
    unf: new Set(),
    saved: new Set(),
    lk: new Map(),
    ver: 0,
    ls: null,
  }
  const LN = globalThis.ccLearn

  // ---------- 找视频 ----------

  const pickVideo = () => {
    const list = [...document.querySelectorAll('video')].filter((v) => v.clientWidth > 120 && v.clientHeight > 80)
    return list.sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0] || null
  }

  // ---------- 界面 ----------

  const cap = h('div', { class: 'cap' })
  const stEl = h('div', { class: 'st' })
  const sizeEl = h('b')
  const delayEl = h('b')
  const posEl = h('b')
  const modeBtns = L.MODES.map((m) =>
    h('button', { text: { 'orig-top': '原文在上', 'trans-top': '译文在上', orig: '仅原文', trans: '仅译文' }[m], onclick: () => update({ mode: m }) }),
  )
  const langSel = h(
    'select',
    { onchange: (e) => update({ target: e.target.value }, true) },
    ...['简体中文', '繁體中文', 'English', '日本語', '한국어', 'Français', 'Deutsch', 'Español', 'Русский'].map((l) => h('option', { value: l, text: l })),
  )
  const sizeIn = h('input', { type: 'range', min: '50', max: '300', step: '10', oninput: (e) => update({ size: Number(e.target.value) }) })
  const posIn = h('input', { type: 'range', min: '0', max: '60', step: '1', oninput: (e) => update({ bottom: Number(e.target.value) }) })
  const delayIn = h('input', { type: 'range', min: '-10000', max: '10000', step: '100', oninput: (e) => update({ delay: Number(e.target.value) }) })
  const LEARN_OPTS = [
    ['off', '关闭查词'],
    ['0', '查词（不标记）'],
    ['1', '查词 + 标生僻词'],
    ['2', '查词 + 标进阶以上'],
    ['3', '查词 + 标常用以外'],
  ]
  const learnSel = h(
    'select',
    { onchange: (e) => setLearn(e.target.value) },
    ...LEARN_OPTS.map(([v, t]) => h('option', { value: v, text: t })),
  )
  const lbEl = h('span', { class: 'lb', title: '开/关双语字幕', onclick: () => toggleOn() })
  const swEl = h('button', { class: 'sw', title: '开/关双语字幕', 'aria-label': '开/关双语字幕', onclick: () => toggleOn() })
  const pop = h(
    'div',
    { class: 'menu' },
    h('div', { class: 'row' }, h('div', { class: 'lab' }, h('span', { text: '翻译成' })), langSel),
    h('div', { class: 'row' }, h('div', { class: 'lab' }, h('span', { text: '显示' })), h('div', { class: 'seg' }, ...modeBtns)),
    h('div', { class: 'row' }, h('div', { class: 'lab' }, h('span', { text: '学习（悬停字幕单词查词）' })), learnSel),
    h(
      'div',
      { class: 'foot' },
      h('button', { text: '精听模式', title: '遮住字幕、句末暂停、单句循环、听写对照', onclick: () => toggleListen() }),
      h('button', { text: '生词本', title: '看收藏的词，导出 Anki / CSV', onclick: () => toggleDrawer() }),
    ),
    h('div', { class: 'sep' }),
    h('div', { class: 'row' }, h('div', { class: 'lab' }, h('span', { text: '字号' }), sizeEl), sizeIn),
    h('div', { class: 'row' }, h('div', { class: 'lab' }, h('span', { text: '位置（距底）' }), posEl), posIn),
    h('div', { class: 'row' }, h('div', { class: 'lab' }, h('span', { text: '字幕延迟' }), delayEl), delayIn),
    h('div', { class: 'sep' }),
    h(
      'div',
      { class: 'foot' },
      h('button', { text: '导出 SRT', onclick: exportSrt }),
      h('button', { class: 'p', text: '用清影转写', title: '这个视频没有字幕时，让清影把音频转成文字', onclick: transcribe }),
    ),
    h(
      'div',
      { class: 'tail' },
      h('button', { text: '恢复默认', onclick: () => update({ size: L.DEFAULTS.size, bottom: L.DEFAULTS.bottom, delay: L.DEFAULTS.delay }) }),
      h('button', { text: '关闭字幕层', onclick: () => api_destroy() }),
    ),
    stEl,
  )
  function togglePop(open) {
    const on = open ?? !pop.classList.contains('open')
    pop.classList.toggle('open', on)
    layout()
  }
  async function toggleOn() {
    S.on = !S.on
    await store.set('subLayerOn', S.on)
    cap.dataset.k = ''
    renderBar()
    layout()
    paint()
  }
  // 挂进视频胶囊：胶囊上是“双语”和开关；⋮ 菜单里是字幕设置、精听、生词本、关闭
  const Dock = globalThis.ccVDock
  Dock.register('sublayer', {
    order: 5,
    title: '字幕层',
    applies: (v) => !S.closed && v === S.video,
    inline: () => [lbEl, swEl],
    menu: () => [
      { label: '字幕设置…', title: '翻译成、显示方式、查词、字号位置、延迟、导出 SRT、转写', onclick: () => togglePop(true) },
      { label: S.ls ? '退出精听模式' : '精听模式', onclick: () => toggleListen() },
      { label: '生词本', onclick: () => toggleDrawer() },
      { label: '关闭字幕层', onclick: () => api_destroy() },
    ],
    sticky: () => pop.classList.contains('open'),
  })
  document.addEventListener('mousedown', (e) => pop.classList.contains('open') && !e.composedPath().includes(ui.host) && !Dock.owns(e) && togglePop(false), true)
  // 查词浮窗：和视频摘要文稿里悬停单词弹出的是同一个（wordcard.js）
  const wpop = WC.popover(ui, { onHide: () => resumeAfterWord() })
  const lsEl = h('div', { class: 'ls' })
  const drEl = h('div', { class: 'dr' })
  ui.add(cap)
  ui.add(pop)
  ui.add(lsEl)
  ui.add(drEl)

  const setStatus = (t) => {
    S.status = t
    stEl.textContent = t
    stEl.title = t
  }

  function layout() {
    const v = S.video
    const r = v && v.isConnected ? v.getBoundingClientRect() : null
    if (!r || r.width < 10) {
      cap.style.display = 'none'
      pop.style.display = 'none'
      return
    }
    // 设置菜单放在视频胶囊（vdock.js）上方
    pop.style.display = ''
    const at = Dock.above(288, v)
    pop.style.left = `${at.left}px`
    pop.style.top = 'auto'
    pop.style.bottom = `${at.bottom}px`
    pop.style.maxHeight = `${at.maxHeight}px`
    pop.style.overflowY = 'auto'
    const fs = Math.max(12, Math.round((r.height / 22) * (S.prefs.size / 100)))
    cap.style.cssText = `left:${r.left}px;width:${r.width}px;bottom:${Math.max(0, window.innerHeight - r.bottom) + (r.height * S.prefs.bottom) / 100}px;font-size:${fs}px`
  }

  function paint() {
    const v = S.video
    if (S.editing) return
    if (!v || !S.lines.length || !S.on) {
      cap.dataset.k = ''
      return cap.replaceChildren()
    }
    const i = L.indexAt(S.lines, v.currentTime * 1000, S.prefs.delay)
    const rows = i < 0 ? [] : L.compose(S.lines[i].text, S.trans[i], S.prefs.mode)
    const ls = S.ls
    const hideO = !!(ls && ls.hideO && !ls.revealed)
    const hideT = !!(ls && ls.hideT && !ls.revealed)
    const key = `${i}|${S.prefs.mode}|${S.trans[i] || ''}|${S.learn.on}|${S.ver}|${hideO}|${hideT}`
    if (cap.dataset.k === key) return
    cap.dataset.k = key
    const orig = i < 0 ? '' : String(S.lines[i].text || '').trim()
    const kids = []
    rows.forEach((t, k) => {
      const isOrig = t === orig
      if (isOrig ? hideO : hideT) return
      const el = h('span', { class: k ? 'second' : '' })
      if (isOrig && S.learn.on && !ls) el.append(...wordNodes(t, i))
      else el.textContent = t
      kids.push(el)
    })
    cap.replaceChildren(...kids)
  }

  /** 双击视频上的字幕：就地改当前这一句（回车保存、Esc 取消）；改动同步到摘要文稿，也记在本地 */
  function editCurrent() {
    const v = S.video
    if (!v || !S.lines.length || !S.on || S.editing || S.ls) return
    const i = L.indexAt(S.lines, v.currentTime * 1000, S.prefs.delay)
    if (i < 0) return
    hideBubble(true)
    try {
      ;(ui.root.getSelection?.() || getSelection())?.removeAllRanges()
    } catch {
      /* 选区清不掉不影响 */
    }
    const wasPlaying = !v.paused
    if (wasPlaying) v.pause()
    S.editing = true
    const ta = h('textarea', { class: 'ledit', rows: 1 })
    ta.value = S.lines[i].text
    const fit = () => {
      ta.style.height = 'auto'
      ta.style.height = `${Math.min(ta.scrollHeight, 160)}px`
    }
    let done = false
    const finish = (save) => {
      if (done) return
      done = true
      const text = ta.value.replace(/\s*\n\s*/g, ' ').trim()
      if (save && text && text !== S.lines[i].text) {
        VS.editLine(i, text, 'sublayer')
        S.lines[i] = { t: S.lines[i].t, text }
        S.trans[i] = ''
        setStatus('已修改这一句')
      }
      S.editing = false
      cap.dataset.k = ''
      paint()
      if (wasPlaying) Promise.resolve(v.play()).catch(() => {})
    }
    ta.addEventListener('input', fit)
    ta.addEventListener('keydown', (ev) => {
      ev.stopPropagation()
      if (ev.isComposing) return
      if (ev.key === 'Enter' && !ev.shiftKey) (ev.preventDefault(), finish(true))
      else if (ev.key === 'Escape') (ev.preventDefault(), finish(false))
    })
    ta.addEventListener('keyup', (ev) => ev.stopPropagation())
    ta.addEventListener('blur', () => finish(true))
    for (const t of ['click', 'dblclick', 'mousedown', 'mouseup']) ta.addEventListener(t, (ev) => ev.stopPropagation())
    cap.replaceChildren(ta)
    fit()
    ta.focus()
    ta.setSelectionRange(ta.value.length, ta.value.length)
  }
  cap.addEventListener('dblclick', editCurrent)

  function renderBar() {
    swEl.classList.toggle('on', S.on)
    L.MODES.forEach((m, i) => modeBtns[i].classList.toggle('on', m === S.prefs.mode))
    sizeEl.textContent = `${S.prefs.size}%`
    sizeIn.value = String(S.prefs.size)
    posEl.textContent = `${S.prefs.bottom}%`
    posIn.value = String(S.prefs.bottom)
    delayEl.textContent = `${S.prefs.delay >= 0 ? '+' : ''}${(S.prefs.delay / 1000).toFixed(1)}s`
    delayIn.value = String(S.prefs.delay)
    langSel.value = S.target
  }

  async function update(p, retranslate = false) {
    S.prefs = L.clampPrefs({ ...S.prefs, ...p })
    if (p.target !== undefined) S.target = S.prefs.target
    await store.set('subLayerPrefs', S.prefs)
    renderBar()
    layout()
    cap.dataset.k = ''
    paint()
    if (retranslate) {
      S.trans = []
      translateAll()
    }
  }
  // ---------- 拆出去的模块（sublayer-data / learn / listen .js）：共享的状态和工具放在 C 里 ----------
  const C = { S, V, VS, L, WC, LN, h, send, store, toast, ui, cap, wpop, drEl, lsEl, setStatus, paint, togglePop }
  for (const m of ['data', 'learn', 'listen']) globalThis.ccParts.sub[m](C)
  // 模块里的函数：声明提升，页面搭起来时（上面的按钮等）就能引用，调用时再去 C 里取
  function exportSrt(...a) { return C.exportSrt(...a) }
  function hideBubble(...a) { return C.hideBubble(...a) }
  function listenTick(...a) { return C.listenTick(...a) }
  function loadLines(...a) { return C.loadLines(...a) }
  function refreshMarks(...a) { return C.refreshMarks(...a) }
  function resumeAfterWord(...a) { return C.resumeAfterWord(...a) }
  function setLearn(...a) { return C.setLearn(...a) }
  function stopListen(...a) { return C.stopListen(...a) }
  function toggleDrawer(...a) { return C.toggleDrawer(...a) }
  function toggleListen(...a) { return C.toggleListen(...a) }
  function transcribe(...a) { return C.transcribe(...a) }
  function translateAll(...a) { return C.translateAll(...a) }
  function wordNodes(...a) { return C.wordNodes(...a) }

  // ---------- 生命周期 ----------

  function api_destroy() {
    S.closed = true
    if (S.ls) stopListen()
    clearInterval(S.timer)
    window.removeEventListener('resize', layout)
    if (S.onPtr) document.removeEventListener('mousemove', S.onPtr, { capture: true })
    S.onPtr = null
    S.unlife?.()
    ui.remove()
    Dock.unregister('sublayer')
    unsub()
    delete globalThis.ccSubLayerRun
  }
  /** 视频会话里的文稿换了（摘要里选了别的字幕轨、转写完成、单页应用切换了视频）：字幕层跟着换，译文重新来（有缓存的直接用）。 */
  function setLines(lines, track) {
    S.data = { ...(S.data || {}), lines, track }
    S.lines = lines.map((l) => ({ t: l.t, text: l.text }))
    S.trans = []
    S.busy = false
    cap.dataset.k = ''
    setStatus(`已载入文稿 ${S.lines.length} 句${track ? `（${track}）` : ''}`)
    translateAll()
    refreshMarks()
    paint()
  }
  const unsub = VS.subscribe((what, snap, extra) => {
    if (S.closed) return
    // 摘要文稿里改了一句：这里跟着换（其他句的译文保留）
    if (what === 'edit' && extra?.source !== 'sublayer' && S.lines[extra.i]) {
      S.lines[extra.i] = { t: S.lines[extra.i].t, text: extra.text }
      S.trans[extra.i] = ''
      cap.dataset.k = ''
      if (!S.editing) paint()
      return
    }
    if (what === 'lines' && snap.lines.length && snap.source !== 'sublayer') setLines(snap.lines, snap.track)
    if (what === 'trans' && !S.trans.some(Boolean)) {
      const have = VS.translations(S.target)
      if (have && have.length === S.lines.length) {
        S.trans = have
        cap.dataset.k = ''
        paint()
      }
    }
  })
  /** 摘要文稿页勾选“显示在视频上”：字幕层开着但字幕被关掉了，就打开 */
  function show() {
    if (S.closed) return
    if (!S.on) {
      S.on = true
      store.set('subLayerOn', true)
    }
    renderBar()
    cap.dataset.k = ''
    layout()
    paint()
  }
  globalThis.ccSubLayerRun = { destroy: api_destroy, setLines, show }

  ;(async () => {
    WC.loadClips()
    S.prefs = L.clampPrefs(await store.get('subLayerPrefs', null))
    S.on = (await store.get('subLayerOn', true)) !== false
    S.learn = { on: true, level: 1, ...(await store.get('learnPrefs', {})) }
    learnSel.value = S.learn.on ? String(S.learn.level) : 'off'
    const st = await send({ type: 'cc-ai-status' })
    S.pv = st.ok ? String(st.promptVersion || '') : ''
    VS.setPromptVersion(S.pv)
    S.target = S.prefs.target || (st.ok && st.lang) || '简体中文'
    if (![...langSel.options].some((o) => o.value === S.target)) langSel.append(h('option', { value: S.target, text: S.target }))
    S.video = pickVideo()
    // 刚打开时让胶囊露一下，知道入口在哪儿
    if (S.video) Dock.show(S.video)
    if (!S.video) {
      toast(ui, '这个页面没有找到视频', true, 4000)
      return api_destroy()
    }
    renderBar()
    layout()
    window.addEventListener('resize', layout)
    S.unlife = life.add(api_destroy) // 扩展更新 / 卸载后本页统一清理时一并关闭字幕层
    S.onPtr = (e) => (globalThis.__ccPtr = { x: e.clientX, y: e.clientY })
    document.addEventListener('mousemove', S.onPtr, { capture: true, passive: true })
    S.timer = setInterval(() => {
      if (!S.video?.isConnected) S.video = pickVideo()
      layout()
      paint()
      listenTick()
      if (S.clipEnd && S.video && (S.video.currentTime >= S.clipEnd || S.video.paused)) {
        if (S.video.currentTime >= S.clipEnd) S.video.pause()
        S.clipEnd = 0
      }
      const lb = S.busy && S.status ? S.status.replace(/^正在/, '').slice(0, 10) : '双语'
      if (lbEl.textContent !== lb) lbEl.textContent = lb
    }, 150)
    if (await loadLines().catch((e) => (setStatus(e.message), false))) {
      translateAll()
      refreshMarks()
    }
  })()
})()
