// 页面字幕层：在任意网页的视频上叠一层字幕——可以双语、调字号和位置、对齐延迟、导出 SRT。
// 字幕来自 YouTube / B 站 / 播放器自带字幕轨，或让清影把音频转写出来；译文由清影用你设置的大模型翻译（走翻译缓存）。
// 再注入一次 = 关闭。
;(() => {
  const prev = globalThis.ccSubLayerRun
  if (prev) {
    prev.destroy()
    return
  }
  const { api, mount, h, send, toast } = globalThis.ccUI
  const L = globalThis.ccSubLayer
  const V = globalThis.ccVsum
  const VS = globalThis.ccVSession
  const WC = globalThis.ccWordCard

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

  const CSS = `
    .cap { position: fixed; pointer-events: none; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 2px }
    .cap span { font-size: inherit; display: inline-block; max-width: 92%; background: #000a; color: #fff; padding: 2px 10px; border-radius: 4px; white-space: pre-wrap; text-shadow: 0 1px 2px #000; line-height: 1.35 }
    .cap span.second { color: #ffe9a8 }
    .pill { transition: opacity .25s; position: fixed; display: flex; align-items: center; gap: 7px; height: 30px; padding: 0 4px 0 9px; border-radius: 15px; background: #1d2420d9; color: #fff; font-size: 13px; font-weight: 600; box-shadow: 0 2px 8px #0005; user-select: none }
    .pill .mk { width: 16px; height: 16px; border-radius: 5px; background: #f08a3c; color: #14181a; font-size: 10px; font-weight: 800; display: flex; align-items: center; justify-content: center; flex: none }
    .pill .lb { cursor: pointer; max-width: 9em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .pill .sw { width: 30px; height: 18px; border-radius: 9px; background: #ffffff59; position: relative; cursor: pointer; padding: 0; border: 0; flex: none }
    .pill .sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; transition: left .15s }
    .pill .sw.on { background: #f08a3c }
    .pill .sw.on::after { left: 14px }
    .pill .vd { width: 1px; height: 16px; background: #ffffff4d; flex: none }
    .pill .dots { width: 26px; height: 26px; border-radius: 13px; padding: 0; border: 0; background: transparent; display: flex; align-items: center; justify-content: center; cursor: pointer }
    .pill .dots:hover, .pill .dots.on { background: #ffffff38 }
    .pill .dots svg { width: 16px; height: 16px; fill: #fff }
    .menu { position: fixed; display: none; flex-direction: column; gap: 12px; width: 288px; box-sizing: border-box; padding: 14px; border-radius: 12px; border: 1px solid #2f383c; background: #22292c; color: #e7ecea; font-size: 13px; box-shadow: 0 10px 30px #0006 }
    .menu.open { display: flex }
    .menu .row { display: flex; flex-direction: column; gap: 6px }
    .menu .lab { display: flex; justify-content: space-between; color: #9aa6a2; font-size: 12px }
    .menu .lab b { color: #e7ecea; font-weight: 600 }
    .menu select { height: 30px; border: 1px solid #2f383c; border-radius: 8px; background: #14181a; color: #e7ecea; padding: 0 8px; font-size: 13px }
    .menu .seg { display: flex; background: #14181a; border: 1px solid #2f383c; border-radius: 8px; padding: 2px; gap: 2px }
    .menu .seg button { flex: 1; padding: 5px 0; border-radius: 6px; font-size: 12px; font-weight: 400; background: transparent; color: #9aa6a2 }
    .menu .seg button.on { background: #f08a3c; color: #14181a; font-weight: 600 }
    .menu input[type=range] { width: 100%; margin: 4px 0; accent-color: #f08a3c }
    .menu .sep { height: 1px; background: #2f383c }
    .menu .foot { display: flex; gap: 8px }
    .menu .foot button { flex: 1; height: 30px; border-radius: 8px; border: 1px solid #2f383c; background: #14181a; color: #e7ecea; font-weight: 400 }
    .menu .foot button.p { background: #f08a3c; border-color: #f08a3c; color: #14181a; font-weight: 600 }
    .menu .tail { display: flex; justify-content: space-between; color: #9aa6a2; font-size: 12px }
    .menu .tail button { background: none; border: 0; padding: 0; color: #9aa6a2; font-weight: 400; cursor: pointer; text-decoration: underline }
    .menu .st { color: #9aa6a2; font-size: 12px; line-height: 1.4; word-break: break-all }
    .cap .w { font: inherit; font-style: normal; pointer-events: auto; cursor: pointer; border-radius: 3px; background: transparent; padding: 0; display: inline }
    .cap .w:hover { background: #ffffff40 }
    .cap .w.unf { border-bottom: 2px dashed #f08a3c }
    .cap .w.sv { border-bottom: 2px solid #6fcf97 }
    .bub { position: fixed; display: none; width: 400px; max-width: calc(100vw - 16px); box-sizing: border-box; padding: 14px 16px; border-radius: 12px; border: 1px solid #2f383c; background: #22292c; color: #e7ecea; box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 6px }
    .bub.open { display: flex }
    .bub.wcard { --wc-fg: #e7ecea; --wc-head: #fff; --wc-mute: #9aa6a2; --wc-soft: #14181a; --wc-acc: #f0a064; --wc-tag: #3a2c20; --wc-line: #4a5a60; --wc-ok: #6cc08a }
    .bub .row2 button { white-space: nowrap; flex: none }
    .bub .row2 button, .ls button, .dr button.b { height: 28px; padding: 0 10px; border-radius: 8px; border: 1px solid #2f383c; background: #14181a; color: #e7ecea; font-weight: 400; font-size: 12.5px }
    .bub .row2 button.p, .ls button.p { background: #f08a3c; border-color: #f08a3c; color: #14181a; font-weight: 600 }
    .ls { position: fixed; display: none; width: 420px; max-width: calc(100vw - 16px); box-sizing: border-box; padding: 12px 14px; border-radius: 12px; border: 1px solid #2f383c; background: #22292c; color: #e7ecea; box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 8px }
    .ls.open { display: flex }
    .ls .r { display: flex; align-items: center; gap: 6px; flex-wrap: wrap }
    .ls .r .sp { flex: 1 }
    .ls select { height: 28px; border: 1px solid #2f383c; border-radius: 8px; background: #14181a; color: #e7ecea; padding: 0 6px; font-size: 12.5px }
    .ls label { display: flex; align-items: center; gap: 4px; color: #9aa6a2; font-size: 12px }
    .ls textarea { width: 100%; height: 56px; resize: none; border: 1px solid #2f383c; border-radius: 8px; background: #14181a; color: #e7ecea; padding: 6px 8px; font-size: 13px }
    .ls .res { line-height: 1.7; font-size: 14px }
    .ls .res .ok { color: #6fcf97 }
    .ls .res .no { color: #ff7b72; text-decoration: underline wavy }
    .ls .sc { color: #9aa6a2; font-size: 12px }
    .ls .pill2 { font-size: 11.5px; padding: 1px 8px; border-radius: 9px; background: #3a2c20; color: #f0a064 }
    .ls .keys { color: #9aa6a2; font-size: 11.5px; line-height: 1.6 }
    .ls .keys kbd { border: 1px solid #2f383c; border-bottom-width: 2px; border-radius: 4px; padding: 0 5px; margin: 0 2px; background: #14181a; font: inherit; font-size: 11px }
    .ls .chips { display: flex; gap: 6px; flex-wrap: wrap }
    .ls .chip { display: inline-flex; align-items: center; gap: 6px; padding: 2px 8px; border-radius: 9px; border: 1px solid #2f383c; font-size: 12.5px }
    .ls .chip a { color: #f0a064; cursor: pointer }
    .ls .prog { display: flex; flex-direction: column; gap: 2px; max-height: 120px; overflow-y: auto; border-top: 1px solid #2f383c; padding-top: 6px }
    .ls .prog .pr { display: flex; gap: 8px; font-size: 12px; color: #9aa6a2; cursor: pointer; padding: 1px 0 }
    .ls .prog .pr.cur { color: #f0a064 }
    .ls .prog .pr span:nth-child(2) { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .dr { position: fixed; display: none; width: 340px; max-height: 70vh; box-sizing: border-box; padding: 12px 14px; border-radius: 12px; border: 1px solid #2f383c; background: #22292c; color: #e7ecea; box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 8px }
    .dr.open { display: flex }
    .dr input { height: 28px; border: 1px solid #2f383c; border-radius: 8px; background: #14181a; color: #e7ecea; padding: 0 8px }
    .dr .lst { overflow-y: auto; display: flex; flex-direction: column; gap: 6px; min-height: 60px }
    .dr .it2 { border: 1px solid #2f383c; border-radius: 8px; padding: 6px 8px; display: flex; flex-direction: column; gap: 2px }
    .dr .it2 b { color: #fff }
    .dr .it2 small { color: #9aa6a2; font-size: 11.5px; line-height: 1.4 }
    .dr .it2 .ac { display: flex; gap: 10px; font-size: 12px }
    .dr .it2 .ac a { color: #f08a3c; cursor: pointer; text-decoration: underline }
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
  const svgNs = 'http://www.w3.org/2000/svg'
  const dotsIcon = document.createElementNS(svgNs, 'svg')
  dotsIcon.setAttribute('viewBox', '0 0 16 16')
  for (const y of [3, 8, 13]) {
    const c = document.createElementNS(svgNs, 'circle')
    c.setAttribute('cx', '8')
    c.setAttribute('cy', String(y))
    c.setAttribute('r', '1.5')
    dotsIcon.append(c)
  }
  const lbEl = h('span', { class: 'lb', title: '开/关双语字幕', onclick: () => toggleOn() })
  const swEl = h('button', { class: 'sw', title: '开/关双语字幕', 'aria-label': '开/关双语字幕', onclick: () => toggleOn() })
  const dotsBtn = h('button', { class: 'dots', title: '字幕设置', 'aria-label': '字幕设置', onclick: () => togglePop() }, dotsIcon)
  const pill = h('div', { class: 'pill' }, h('span', { class: 'mk', text: '清' }), lbEl, swEl, h('span', { class: 'vd' }), dotsBtn)
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
    dotsBtn.classList.toggle('on', on)
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
  const bub = h('div', { class: 'bub wcard' })
  const lsEl = h('div', { class: 'ls' })
  const drEl = h('div', { class: 'dr' })
  ui.add(cap)
  ui.add(pill)
  ui.add(pop)
  ui.add(bub)
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
      pill.style.display = 'none'
      pop.style.display = 'none'
      return
    }
    pill.style.display = ''
    // 胶囊放在视频右下角、控制栏上方；菜单在胶囊上方
    const pw = pill.offsetWidth || 150
    const ph = pill.offsetHeight || 30
    const pl = Math.min(window.innerWidth - pw - 4, Math.max(4, r.right - pw - 12))
    const pt = Math.min(window.innerHeight - ph - 4, Math.max(4, r.bottom - ph - 62))
    // 鼠标移出视频（含胶囊、菜单）1.5 秒后自动隐藏；菜单打开时一直显示
    const m = globalThis.__ccPtr || { x: -1, y: -1 }
    const inBox = (b) => m.x >= b.left && m.x <= b.right && m.y >= b.top && m.y <= b.bottom
    if (inBox(r) || pop.classList.contains('open') || inBox(pill.getBoundingClientRect())) S.seen = Date.now()
    const show = Date.now() - S.seen < 1500
    pill.style.opacity = show ? '1' : '0'
    pill.style.pointerEvents = show ? '' : 'none'
    pill.style.left = `${pl}px`
    pill.style.top = `${pt}px`
    pop.style.display = ''
    pop.style.left = `${Math.max(4, Math.min(window.innerWidth - 292, pl + pw - 288))}px`
    pop.style.top = 'auto'
    pop.style.bottom = `${window.innerHeight - pt + 8}px`
    pop.style.maxHeight = `${Math.max(120, pt - 16)}px`
    pop.style.overflowY = 'auto'
    const fs = Math.max(12, Math.round((r.height / 22) * (S.prefs.size / 100)))
    cap.style.cssText = `left:${r.left}px;width:${r.width}px;bottom:${Math.max(0, window.innerHeight - r.bottom) + (r.height * S.prefs.bottom) / 100}px;font-size:${fs}px`
  }

  function paint() {
    const v = S.video
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

  // ---------- 字幕与译文 ----------

  async function loadLines() {
    setStatus('正在读取字幕…')
    // 视频会话：视频摘要已经读到的文稿直接用，不再读一遍
    const d = await VS.load({ source: 'sublayer' })
    S.data = d
    S.lines = (d.lines || []).map((l) => ({ t: l.t, text: l.text }))
    S.trans = []
    if (!S.lines.length) {
      setStatus('这个视频没有读到字幕。可以点“用清影转写”。')
      return false
    }
    setStatus(`已读取 ${S.lines.length} 句字幕${d.track ? `（${d.track}）` : ''}`)
    return true
  }

  /** 译文来自视频会话：和摘要面板的“译文”是同一份，谁先翻好另一个直接用。 */
  async function translateAll() {
    if (S.busy || !S.lines.length) return
    S.busy = true
    const lines = S.lines
    try {
      const have = VS.translations(S.target)
      if (have && have.length === lines.length) {
        S.trans = have
        setStatus(`已翻译成${S.target}，共 ${lines.length} 句`)
        return
      }
      S.trans = new Array(lines.length).fill('')
      const arr = await VS.translate(S.target, {
        stop: () => S.closed || S.lines !== lines,
        onProgress: (done, total, part) => {
          if (S.lines !== lines) return
          S.trans = part.slice()
          setStatus(`正在翻译 ${done}/${total}…`)
          cap.dataset.k = ''
          paint()
        },
      })
      if (S.lines !== lines) return
      S.trans = arr
      setStatus(`已翻译成${S.target}，共 ${lines.length} 句`)
    } catch (e) {
      if (e.message === '已取消') return
      setStatus(e.message)
      toast(ui, e.message, true, 6000)
    } finally {
      S.busy = false
      cap.dataset.k = ''
      paint()
    }
  }

  async function transcribe() {
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    S.busy = true
    try {
      const st = await send({ type: 'cc-ai-status' })
      if (!st.ok) throw new Error(st.error)
      if (st.transcribe === false) throw new Error('清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp，或在模型池里勾选云端语音转文字')
      const res = await globalThis.ccUI.aiRun(
        { kind: 'transcribe', url: location.href, title: document.title },
        { interval: 800, stop: () => S.closed, onProgress: (j) => setStatus(`${j.note || '清影正在转写…'}${j.total ? ` ${j.done}%` : ''}`) },
      )
      const got = (res.lines || []).map((l) => ({ t: l.t, text: l.text }))
      if (!got.length) throw new Error('没有识别出任何语音')
      // 放进视频会话：摘要面板开着的话也会用上这份转写
      VS.setLines(got, '清影转写', 'sublayer')
      S.lines = got
      S.trans = []
    } catch (e) {
      setStatus(e.message)
      toast(ui, e.message, true, 6000)
      return
    } finally {
      S.busy = false
    }
    setStatus(`转写完成，共 ${S.lines.length} 句`)
    translateAll()
  }

  function exportSrt() {
    if (!S.lines.length) return toast(ui, '还没有字幕', true)
    const text = L.toSrt(S.lines, S.trans, S.prefs.mode, S.prefs.delay)
    const name = `${(document.title || 'subtitle').replace(/[\\/:*?"<>|]/g, '_').slice(0, 80)}.srt`
    const a = h('a', { href: URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' })), download: name })
    document.body.append(a)
    a.click()
    a.remove()
    toast(ui, `已导出 ${name}`)
  }

  // ---------- 字幕学习：查词 / 生词 ----------

  function wordNodes(text, lineIdx) {
    return LN.segment(text).map((p) => {
      if (!p.word) return document.createTextNode(p.text)
      const w = LN.norm(p.text)
      const cls = S.saved.has(w) ? 'w sv' : S.unf.has(w) ? 'w unf' : 'w'
      return h('i', { class: cls, 'data-w': p.text, 'data-i': String(lineIdx), style: 'font-style:normal', text: p.text })
    })
  }
  const wordText = (el) => el.dataset.w

  async function setLearn(v) {
    S.learn = v === 'off' ? { on: false, level: 0 } : { on: true, level: Number(v) }
    await store.set('learnPrefs', S.learn)
    S.ver++
    hideBubble(true)
    if (S.learn.on) refreshMarks()
  }

  async function refreshMarks() {
    if (!S.learn.on || !S.lines.length) return
    const saved = await send({ type: 'cc-vocab', op: 'words' })
    if (saved.ok) S.saved = new Set(saved.words || [])
    S.unf = new Set()
    const rank = LN.levelRank(S.learn.level)
    if (rank) {
      for (const g of LN.chunk(LN.uniqueWords(S.lines), 300)) {
        const r = await send({ type: 'cc-vocab', op: 'unfamiliar', words: g, minRank: rank })
        if (r.ok) (r.words || []).forEach((w) => S.unf.add(w))
        if (S.closed) return
      }
    }
    S.ver++
  }

  const lookup = (word, sentence) => WC.lookup(word, sentence, { lang: S.target, stop: () => S.closed })

  const speak = (t) => {
    try {
      const u = new SpeechSynthesisUtterance(t)
      u.lang = 'en-US'
      speechSynthesis.cancel()
      speechSynthesis.speak(u)
    } catch {
      // 浏览器不支持朗读
    }
  }

  let bubTimer = 0
  let bubFor = ''
  // 悬停 / 点词时暂停视频，词卡关闭后接着播（只恢复我们自己暂停的）
  let pausedByWord = false
  function pauseForWord() {
    const v = S.video
    if (v && !v.paused && !v.ended) {
      v.pause()
      pausedByWord = true
    }
  }
  function resumeAfterWord() {
    if (!pausedByWord) return
    pausedByWord = false
    try {
      void S.video?.play()
    } catch {
      /* 页面不让播就算了 */
    }
  }
  function hideBubble(now = false) {
    clearTimeout(bubTimer)
    const go = () => {
      bub.classList.remove('open')
      bubFor = ''
      resumeAfterWord()
    }
    if (now) go()
    else bubTimer = setTimeout(go, 280)
  }

  function placeBubble(r) {
    const bw = Math.min(400, window.innerWidth - 16)
    bub.style.left = `${Math.max(6, Math.min(window.innerWidth - bw - 6, r.left + r.width / 2 - bw / 2))}px`
    bub.style.maxHeight = `${Math.max(200, window.innerHeight - 24)}px`
    bub.style.overflowY = 'auto'
    const above = r.top > 200
    bub.style.top = above ? 'auto' : `${r.bottom + 8}px`
    bub.style.bottom = above ? `${window.innerHeight - r.top + 8}px` : 'auto'
  }

  async function showBubble(el) {
    const word = wordText(el)
    const li = Number(el.dataset.i)
    const sentence = String(S.lines[li]?.text || '')
    const id = `${word}|${li}`
    if (bubFor === id && bub.classList.contains('open')) return
    bubFor = id
    const r = el.getBoundingClientRect()
    bub.replaceChildren(h('div', { class: 'hd' }, h('b', { text: word })), h('div', { class: 'tip', text: '查询中…' }))
    bub.classList.add('open')
    placeBubble(r)
    let res
    try {
      res = await lookup(word, sentence)
    } catch (e) {
      if (bubFor !== id) return
      bub.replaceChildren(h('div', { class: 'hd' }, h('b', { text: word })), h('div', { class: 'tip', text: e.message }))
      return
    }
    if (bubFor !== id) return
    const guess = res.lemma && LN.norm(res.lemma) !== LN.norm(word) ? res.lemma : word
    const have = S.saved.has(LN.norm(guess)) || S.saved.has(LN.norm(word))
    // 例句先放本视频里别的句子（点时间跳过去），模型给的例句由词卡接在后面
    const lw = String(guess).toLowerCase()
    const hit = (t) => String(t || '').toLowerCase().includes(lw) || String(t || '').toLowerCase().includes(String(word).toLowerCase())
    const others = []
    for (let d = 1; d < S.lines.length && others.length < 2; d++) {
      for (const j of [li + d, li - d]) {
        if (others.length < 2 && j >= 0 && j < S.lines.length && hit(S.lines[j].text)) others.push(j)
      }
    }
    const extraExamples = others.map((j) => {
      const txt = String(S.lines[j].text || '')
      return h('div', { class: 'ex' }, h('span', { class: 'tag', text: '本视频' }), ...WC.markIn(h, txt, txt.toLowerCase().includes(lw) ? guess : word), document.createTextNode(' '), h('a', { text: LN.fmtTime(S.lines[j].t || 0), title: '跳到这句', onclick: () => seekLine(j, true) }))
    })
    const t0 = S.lines[li]?.t || 0
    const actions = [
      h('button', { class: 'p', text: `跳到这句 ${LN.fmtTime(t0)}`, onclick: () => seekLine(li, true) }),
      h('button', { text: '播放原声', onclick: () => playClip(li) }),
      h('button', { text: '朗读', onclick: () => speak(guess) }),
      h('button', { text: '复制', onclick: () => navigator.clipboard?.writeText(guess).catch(() => {}) }),
      h('button', { text: clipKeyHas(guess) ? '♪ 已录原声' : '录原声（Anki）', title: '录下这一句的原声，导出 Anki 时带上', onclick: (e) => recordClip(guess, li, e.target) }),
    ]
    const { nodes: parts } = WC.render({ h, word, sentence, res, saved: have, onSave: (lemma) => saveWord(lemma, res, li), extraExamples, actions })
    bub.replaceChildren(...parts)
    placeBubble(r)
  }

  // ---------- 跳到这句 / 播放原声 / 录原声 ----------

  function seekLine(li, play) {
    const sp = LN.span(S.lines, li, S.prefs.delay)
    if (!sp || !S.video) return
    S.video.currentTime = Math.max(0, sp.start / 1000)
    if (play) S.video.play().catch(() => {})
  }
  function playClip(li) {
    const sp = LN.span(S.lines, li, S.prefs.delay)
    if (!sp || !S.video) return
    S.clipEnd = sp.end / 1000
    S.video.currentTime = Math.max(0, sp.start / 1000)
    S.video.play().catch(() => {})
  }
  // 录下的原声存在清影桌面端（浏览器本地存储只有 10 MB，存不了几段）；这里只记“哪些词录过”
  const clipCache = {}
  const clipKey = (w) => String(w || '').trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '_').replace(/^_+|_+$/g, '').slice(0, 60)
  const clipKeyHas = (w) => !!clipCache[clipKey(w)]
  async function loadClips() {
    // 以前存在浏览器里的原声：搬到清影，搬完删掉
    const old = await store.get('vocabClips', {})
    const left = {}
    for (const [w, data] of Object.entries(old || {})) {
      const r = await send({ type: 'cc-vocab', op: 'clip', word: w, data })
      if (!r.ok) left[w] = data
    }
    if (Object.keys(old || {}).length) await store.set('vocabClips', left)
    const r = await send({ type: 'cc-vocab', op: 'clips' })
    if (r.ok) for (const k of r.words || []) clipCache[k] = true
  }
  /** 把这一句的原声录下来（播放一遍，用 captureStream 录；跨域或受保护的视频录不了）。 */
  async function recordClip(word, li, btn) {
    const v = S.video
    const sp = LN.span(S.lines, li, S.prefs.delay)
    if (!v || !sp) return
    if (typeof MediaRecorder === 'undefined' || !(v.captureStream || v.mozCaptureStream)) return toast(ui, '这个浏览器不支持录音', true)
    try {
      const stream = (v.captureStream || v.mozCaptureStream).call(v)
      const tracks = stream.getAudioTracks()
      if (!tracks.length) throw new Error('这个视频录不到声音（可能跨域或受保护）')
      const rec = new MediaRecorder(new MediaStream(tracks))
      const chunks = []
      rec.ondataavailable = (e) => e.data.size && chunks.push(e.data)
      const stopped = new Promise((ok) => (rec.onstop = ok))
      btn.textContent = '录制中…'
      const prevRate = v.playbackRate
      v.playbackRate = 1
      v.currentTime = Math.max(0, sp.start / 1000)
      await v.play()
      rec.start()
      await new Promise((ok) => {
        const tm = setInterval(() => {
          if (v.currentTime * 1000 >= sp.end || v.paused || v.ended) (clearInterval(tm), ok())
        }, 80)
      })
      rec.stop()
      v.pause()
      v.playbackRate = prevRate
      await stopped
      const blob = new Blob(chunks, { type: rec.mimeType || 'audio/webm' })
      if (blob.size > 900000) throw new Error('这一句太长了，没有保存')
      const data = await new Promise((ok, bad) => {
        const fr = new FileReader()
        fr.onload = () => ok(String(fr.result))
        fr.onerror = () => bad(new Error('读取录音失败'))
        fr.readAsDataURL(blob)
      })
      const saved = await send({ type: 'cc-vocab', op: 'clip', word, data })
      if (!saved.ok) throw new Error(`原声没有保存：${saved.error || '连不上清影'}（原声存在清影里，先把应用开着）`)
      clipCache[clipKey(word)] = true
      btn.textContent = '♪ 已录原声'
    } catch (e) {
      btn.textContent = '录下原声（Anki 用）'
      toast(ui, e.message || '录音失败', true, 3000)
    }
  }

  async function saveWord(lemma, res, li) {
    const line = S.lines[li] || { t: 0, text: '' }
    const r = await WC.save(lemma, res, { sentence: line.text, timeSec: (line.t || 0) / 1000 })
    if (!r.ok) {
      toast(ui, r.error || '收藏失败', true)
      return false
    }
    S.saved.add(LN.norm(lemma))
    S.ver++
    toast(ui, `已收藏“${lemma}”，可以在清影的“生词本”里复习`)
    syncCache(false)
    return true
  }

  /** 把这个视频的摘要会话和双语字幕同步到清影，生词本里就能看到、也能在别的浏览器恢复。 */
  let cacheSynced = 0
  async function syncCache(force) {
    try {
      if (!force && Date.now() - cacheSynced < 60000) return
      const i = await send({ type: 'cc-videocache', op: 'info', url: location.href })
      if (!i.ok || (!i.auto && !force)) return
      const vk = V.videoKey(location.href)
      const sess = Object.values(await store.get('vsumSessions', {})).find((e) => e && e.vk === vk)
      const trans = S.trans?.some(Boolean) ? S.trans : null
      const payload = {
        url: location.href,
        title: document.title,
        site: V.videoIdOf(location.href)?.site || '',
        summary: sess ? { key: sess.key, ts: sess.ts, vk: sess.vk, url: sess.url, data: sess.data } : null,
        subs: trans ? { key: L.cacheKey(V.videoKey(location.href), S.target, S.pv), target: S.target, lines: S.lines.map((l) => ({ t: l.t, text: l.text })), trans } : null,
        srcLang: '',
        dstLang: S.target || '',
      }
      if (!payload.summary && !payload.subs) return
      const r = await send({ type: 'cc-videocache', op: 'put', payload })
      if (r.ok) cacheSynced = Date.now()
    } catch {
      // 清影没开或没配置：下次收藏再试
    }
  }

  cap.addEventListener('mouseover', (e) => {
    const el = e.target?.closest?.('.w')
    if (!el) return
    clearTimeout(bubTimer)
    clearTimeout(cap._hv)
    pauseForWord()
    cap._hv = setTimeout(() => showBubble(el), 300)
  })
  cap.addEventListener('mouseout', (e) => {
    if (!e.target?.closest?.('.w')) return
    clearTimeout(cap._hv)
    if (!bub.classList.contains('open')) resumeAfterWord()
    hideBubble()
  })
  cap.addEventListener('click', (e) => {
    const el = e.target?.closest?.('.w')
    if (!el) return
    // 点词：暂停视频，立刻显示
    pauseForWord()
    clearTimeout(cap._hv)
    showBubble(el)
  })
  bub.addEventListener('mouseenter', () => clearTimeout(bubTimer))
  bub.addEventListener('mouseleave', () => hideBubble())

  // ---------- 生词本抽屉 ----------

  let drOpen = false
  async function toggleDrawer(open) {
    drOpen = open ?? !drOpen
    drEl.classList.toggle('open', drOpen)
    if (!drOpen) return
    togglePop(false)
    const m = S.video?.getBoundingClientRect()
    drEl.style.left = `${Math.max(8, Math.min(window.innerWidth - 348, (m ? m.right : window.innerWidth) - 348))}px`
    drEl.style.top = `${Math.max(8, (m ? m.top : 0) + 8)}px`
    await renderDrawer('')
  }
  async function renderDrawer(q) {
    const search = h('input', { placeholder: '搜索单词 / 释义 / 原句', value: q, oninput: (e) => (clearTimeout(search._t), (search._t = setTimeout(() => renderDrawer(e.target.value), 300))) })
    const lst = h('div', { class: 'lst' }, h('div', { class: 'tip', text: '读取中…' }))
    const exp = (f, n) => h('button', { class: 'b', text: n, onclick: () => exportVocab(f) })
    drEl.replaceChildren(
      h('div', { class: 'r', style: 'display:flex;justify-content:space-between;align-items:center' }, h('b', { text: '生词本' }), h('button', { class: 'b', text: '关闭', onclick: () => toggleDrawer(false) })),
      search,
      lst,
      h('div', { class: 'ft' }, exp('anki', '导出 Anki'), exp('csv', 'CSV'), exp('md', 'Markdown'), exp('json', 'JSON')),
    )
    const r = await send({ type: 'cc-vocab', op: 'list', q })
    if (!r.ok) return lst.replaceChildren(h('div', { class: 'tip', text: r.error }))
    const st = r.stats || {}
    lst.replaceChildren(
      h('div', { class: 'tip', text: `共 ${st.total ?? 0} 个词，今天该复习 ${st.due ?? 0} 个。复习在清影应用的“生词本”里。` }),
      ...(r.words || []).slice(0, 80).map((w) => {
        const here = w.sourceUrl === location.href
        return h(
          'div',
          { class: 'it2' },
          h('b', { text: `${w.word} ${w.phonetic || ''}` }),
          h('small', { text: `${w.pos || ''} ${w.meaning || ''}${w.ctxMeaning ? `（本句：${w.ctxMeaning}）` : ''}` }),
          w.sentence ? h('small', { text: w.sentence }) : null,
          h(
            'div',
            { class: 'ac' },
            here && S.video ? h('a', { text: `跳到 ${LN.fmtTime(w.timeSec * 1000)}`, onclick: () => ((S.video.currentTime = w.timeSec), S.video.play()) }) : h('small', { text: w.sourceTitle ? `${w.sourceTitle.slice(0, 20)} ${LN.fmtTime(w.timeSec * 1000)}` : '' }),
            h('a', { text: '删除', onclick: async () => ((await send({ type: 'cc-vocab', op: 'remove', id: w.id })), S.saved.delete(LN.norm(w.word)), S.ver++, renderDrawer(q)) }),
          ),
        )
      }),
    )
  }
  const dataToBytes = (d) => Uint8Array.from(atob(d.split(',')[1] || ''), (c) => c.charCodeAt(0))
  function download(blob, name) {
    const a = h('a', { href: URL.createObjectURL(blob), download: name })
    document.body.append(a)
    a.click()
    a.remove()
  }
  async function exportVocab(format) {
    const r = await send({ type: 'cc-vocab', op: 'export', format })
    if (!r.ok) return toast(ui, r.error || '导出失败', true)
    if (format === 'anki' && r.zip) {
      // 清影把卡片和录过的原声打成了一个 zip
      download(new Blob([dataToBytes(`data:application/zip;base64,${r.zip}`)], { type: 'application/zip' }), '生词本-anki.zip')
      return toast(ui, `已导出 Anki 包：卡片 + ${r.clips || 0} 段原声。把音频复制到 Anki 的 collection.media，再导入 .txt`, false, 6000)
    }
    const name = `生词本.${r.ext}`
    download(new Blob([r.text], { type: 'text/plain;charset=utf-8' }), name)
    toast(ui, `已导出 ${name}`)
  }


  // ---------- 精听 ----------

  function toggleListen(on) {
    const want = on ?? !S.ls
    if (!want) return stopListen()
    if (!S.lines.length) return toast(ui, '还没有字幕，没法精听', true)
    togglePop(false)
    const v = S.video
    const i = Math.max(0, L.indexAt(S.lines, v.currentTime * 1000, S.prefs.delay))
    S.ls = { i, done: 0, loops: 3, rate: 0.75, hideO: true, hideT: true, revealed: false, waiting: false, typed: '', result: null, prevRate: v.playbackRate, pauseEnd: true, skip: new Set(), prog: {} }
    store.get(progKey(), {}).then((p) => S.ls && (S.ls.prog = p || {}) && renderListen())
    document.addEventListener('keydown', lsKeys, true)
    hideBubble(true)
    cap.dataset.k = ''
    playLine()
    renderListen()
  }
  function lsKeys(e) {
    const ls = S.ls
    if (!ls) return
    const t = e.composedPath()[0]
    if (t && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)) return
    if (e.ctrlKey || e.metaKey || e.altKey) return
    const k = e.key
    const hit = k.toLowerCase() === 'r' ? () => (Object.assign(ls, { done: 0 }), playLine()) : k === ' ' ? () => (S.video.paused ? S.video.play() : S.video.pause()) : k === 'ArrowLeft' ? () => gotoLine(-1) : k === 'ArrowRight' ? () => gotoLine(1) : k === 'Tab' ? () => ((ls.revealed = true), (cap.dataset.k = ''), renderListen()) : null
    if (!hit) return
    e.preventDefault()
    e.stopImmediatePropagation()
    hit()
  }
  function stopListen() {
    const ls = S.ls
    if (!ls) return
    document.removeEventListener('keydown', lsKeys, true)
    if (S.video) S.video.playbackRate = ls.prevRate || 1
    S.ls = null
    lsEl.classList.remove('open')
    cap.dataset.k = ''
    paint()
  }
  function playLine() {
    const ls = S.ls
    const sp = LN.span(S.lines, ls.i, S.prefs.delay)
    if (!sp || !S.video) return
    S.video.playbackRate = ls.rate
    S.video.currentTime = Math.max(0, sp.start / 1000)
    ls.waiting = false
    S.video.play().catch(() => {})
  }
  function gotoLine(d) {
    const ls = S.ls
    ls.i = Math.min(S.lines.length - 1, Math.max(0, ls.i + d))
    Object.assign(ls, { done: 0, revealed: false, typed: '', result: null })
    cap.dataset.k = ''
    playLine()
    renderListen()
  }
  function submitDictation(box) {
    const ls = S.ls
    ls.typed = box.value
    ls.result = LN.dictationDiff(S.lines[ls.i].text, ls.typed)
    saveProg(ls.result)
    ls.revealed = true
    S.video?.pause()
    cap.dataset.k = ''
    renderListen()
  }
  async function addMissedWord(w) {
    const ls = S.ls
    const line = S.lines[ls.i]
    const r = await send({ type: 'cc-vocab', op: 'add', word: { word: LN.norm(w), sentence: line.text, sourceTitle: document.title, sourceUrl: location.href, timeSec: line.t / 1000 } })
    if (r.ok) (S.saved.add(LN.norm(w)), S.ver++)
    return !!r.ok
  }
  const progKey = () => `listenProg:${location.origin}${location.pathname}${location.search}`.slice(0, 200)
  function saveProg(res) {
    const ls = S.ls
    if (!ls || !res) return
    ls.prog[ls.i] = { right: res.right, total: res.total }
    store.set(progKey(), ls.prog)
  }
  function renderListen() {
    const ls = S.ls
    if (!ls) return
    const v = S.video?.getBoundingClientRect()
    lsEl.classList.add('open')
    lsEl.style.left = `${Math.max(8, Math.min(window.innerWidth - 428, (v ? v.left + v.width / 2 : window.innerWidth / 2) - 210))}px`
    lsEl.style.top = `${Math.max(8, Math.min(window.innerHeight - 420, v ? v.bottom + 8 : 100))}px`
    const box = h('textarea', { placeholder: '把听到的写下来，Enter 提交对照原句' })
    box.value = ls.typed
    box.addEventListener('keydown', (e) => {
      e.stopPropagation()
      if (e.isComposing) return
      if (e.key === 'Enter' && !e.shiftKey) (e.preventDefault(), submitDictation(box))
      else if (e.key === 'Tab') (e.preventDefault(), (ls.revealed = true), (cap.dataset.k = ''), renderListen())
      else if (e.altKey && e.key.toLowerCase() === 'r') (e.preventDefault(), Object.assign(ls, { done: 0 }), playLine())
      else if (e.altKey && e.key === 'ArrowLeft') (e.preventDefault(), gotoLine(-1))
      else if (e.altKey && e.key === 'ArrowRight') (e.preventDefault(), gotoLine(1))
    })
    box.addEventListener('keyup', (e) => e.stopPropagation())
    const sel = (opts, cur, fn) => h('select', { onchange: (e) => fn(e.target.value) }, ...opts.map(([v, t]) => h('option', { value: v, text: t, selected: String(v) === String(cur) ? '' : null })))
    const chk = (t, on, fn) => h('label', {}, h('input', { type: 'checkbox', checked: on ? '' : null, onchange: (e) => fn(e.target.checked) }), document.createTextNode(t))
    const res = ls.result
    const loopTxt = ls.loops ? `单句循环 ${Math.min(ls.done + 1, ls.loops)}/${ls.loops}` : '单句循环 ∞'
    const missed = (res?.missed || []).filter((w) => !ls.skip.has(LN.norm(w)))
    const lo = Math.max(0, ls.i - 2)
    const rows = []
    for (let k = lo; k < Math.min(S.lines.length, lo + 8); k++) {
      const pg = ls.prog[k]
      rows.push(h('div', { class: `pr${k === ls.i ? ' cur' : ''}`, onclick: () => ((ls.i = k), Object.assign(ls, { done: 0, revealed: false, typed: '', result: null }), (cap.dataset.k = ''), playLine(), renderListen()) }, h('span', { text: LN.fmtTime(S.lines[k].t) }), h('span', { text: S.lines[k].text }), h('span', { text: pg ? `${pg.right}/${pg.total}` : k === ls.i ? '▶' : '' })))
    }
    lsEl.replaceChildren(
      h('div', { class: 'r' }, h('b', { text: `精听 · 第 ${ls.i + 1} 句` }), h('span', { class: 'sc', text: `共 ${S.lines.length} 句` }), h('span', { class: 'pill2', text: loopTxt }), h('span', { class: 'sp' }), h('button', { text: '退出', onclick: () => stopListen() })),
      h('div', { class: 'keys' }, h('kbd', { text: 'R' }), '重播 ', h('kbd', { text: 'Space' }), '暂停 ', h('kbd', { text: '←' }), h('kbd', { text: '→' }), '上/下句 ', h('kbd', { text: 'Tab' }), '揭晓 ', h('kbd', { text: 'Enter' }), '提交', h('div', { text: ls.hideO ? '原文已遮住，只听声音。输入框里用 Alt+R / Alt+←→。' : '' })),
      h(
        'div',
        { class: 'r' },
        h('button', { text: '上一句', onclick: () => gotoLine(-1) }),
        h('button', { class: 'p', text: '重播本句', onclick: () => (Object.assign(ls, { done: 0 }), playLine()) }),
        h('button', { text: '下一句', onclick: () => gotoLine(1) }),
        h('span', { class: 'sp' }),
        sel([[0.5, '0.5×'], [0.75, '0.75×'], [1, '1×'], [1.25, '1.25×']], ls.rate, (v) => ((ls.rate = Number(v)), S.video && (S.video.playbackRate = ls.rate))),
        sel([[1, '播 1 次'], [3, '循环 3 次'], [0, '一直循环']], ls.loops, (v) => (ls.loops = Number(v))),
      ),
      h('div', { class: 'r' }, chk('显示原文', !ls.hideO, (v) => ((ls.hideO = !v), (cap.dataset.k = ''))), chk('显示译文', !ls.hideT, (v) => ((ls.hideT = !v), (cap.dataset.k = ''))), chk('句末暂停', ls.pauseEnd, (v) => (ls.pauseEnd = v))),
      box,
      h('div', { class: 'r' }, h('button', { class: 'p', text: '提交', onclick: () => submitDictation(box) }), h('button', { text: '揭晓原文', onclick: () => ((ls.revealed = true), (cap.dataset.k = ''), renderListen()) }), h('span', { class: 'sc', text: ls.waiting ? '本句播完了，写下来，或点下一句' : '' })),
      res
        ? h(
            'div',
            {},
            h('div', { class: 'res' }, ...res.parts.map((p) => (/\p{L}/u.test(p.text) ? h('span', { class: p.ok ? 'ok' : 'no', text: p.text }) : document.createTextNode(p.text)))),
            h('div', { class: 'r' }, h('span', { class: 'sc', text: `${res.right}/${res.total}　听对 ${res.right} 个词${res.total - res.right ? `，错 ${res.total - res.right} 个` : '，全对'}` })),
            missed.length
              ? h('div', { class: 'chips' }, ...missed.map((w) => h('span', { class: 'chip' }, h('b', { text: w }), h('a', { text: S.saved.has(LN.norm(w)) ? '已加入' : '加入生词本', onclick: async (e) => ((await addMissedWord(w)) && (e.target.textContent = '已加入')) }), h('a', { text: '已掌握', onclick: () => (ls.skip.add(LN.norm(w)), renderListen()) }))))
              : null,
            h('div', { class: 'r' }, h('button', { text: '再听一遍', onclick: () => (Object.assign(ls, { done: 0, revealed: false, typed: '', result: null }), (cap.dataset.k = ''), playLine(), renderListen()) }), h('button', { class: 'p', text: '下一句', onclick: () => gotoLine(1) })),
          )
        : null,
      h('div', { class: 'prog' }, h('div', { class: 'sc', text: `本视频进度 · 已练 ${Object.keys(ls.prog).length} / ${S.lines.length} 句` }), ...rows),
    )
  }
  function listenTick() {
    const ls = S.ls
    const v = S.video
    if (!ls || !v || v.paused || ls.waiting || ls.revealed) return
    const sp = LN.span(S.lines, ls.i, S.prefs.delay)
    const act = LN.listenStep(v.currentTime * 1000, sp, ls.done, ls.loops)
    if (act === 'again') {
      ls.done++
      v.currentTime = sp.start / 1000
    } else if (act === 'stop') {
      if (!ls.pauseEnd && ls.i < S.lines.length - 1) return gotoLine(1)
      v.pause()
      ls.waiting = true
      renderListen()
    }
  }

  // ---------- 生命周期 ----------

  function api_destroy() {
    S.closed = true
    if (S.ls) stopListen()
    clearInterval(S.timer)
    window.removeEventListener('resize', layout)
    ui.remove()
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
  const unsub = VS.subscribe((what, snap) => {
    if (S.closed) return
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
  globalThis.ccSubLayerRun = { destroy: api_destroy, setLines }

  ;(async () => {
    loadClips()
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
    if (!S.video) {
      toast(ui, '这个页面没有找到视频', true, 4000)
      return api_destroy()
    }
    renderBar()
    layout()
    window.addEventListener('resize', layout)
    document.addEventListener('mousemove', (e) => (globalThis.__ccPtr = { x: e.clientX, y: e.clientY }), { capture: true, passive: true })
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
