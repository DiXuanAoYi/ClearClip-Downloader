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
    .cap .w { pointer-events: auto; cursor: pointer; border-radius: 3px; background: transparent; padding: 0; display: inline }
    .cap .w:hover { background: #ffffff40 }
    .cap .w.unf { border-bottom: 2px dashed #f08a3c }
    .cap .w.sv { border-bottom: 2px solid #6fcf97 }
    .bub { position: fixed; display: none; width: 290px; box-sizing: border-box; padding: 12px 14px; border-radius: 12px; border: 1px solid #2f383c; background: #22292c; color: #e7ecea; box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 6px }
    .bub.open { display: flex }
    .bub .hd { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap }
    .bub .hd b { font-size: 17px; color: #fff }
    .bub .ph { color: #9aa6a2; font-size: 12px }
    .bub .lv { margin-left: auto; font-size: 11px; padding: 0 7px; border-radius: 9px; border: 1px solid #f08a3c; color: #f08a3c }
    .bub .ms { line-height: 1.55 }
    .bub .ms i { font-style: normal; color: #9aa6a2; margin-right: 6px }
    .bub .cx { padding: 6px 8px; border-radius: 8px; background: #14181a; color: #e7ecea; font-size: 12.5px; line-height: 1.5 }
    .bub .tip { color: #9aa6a2; font-size: 12px; line-height: 1.5 }
    .bub .row2 { display: flex; gap: 6px; margin-top: 2px }
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
  `
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
  const bub = h('div', { class: 'bub' })
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
    const d = await globalThis.ccVsumSource.load()
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

  async function transKey() {
    const vk = V.videoKey(location.href)
    return L.cacheKey(vk, S.target, S.pv)
  }

  async function translateAll() {
    if (S.busy || !S.lines.length) return
    S.busy = true
    try {
      const key = await transKey()
      const saved = (await store.get('subLayerTrans', {}))[key]
      if (saved && saved.length === S.lines.length) {
        S.trans = saved
        setStatus('已载入保存的译文')
        return
      }
      S.trans = new Array(S.lines.length).fill('')
      const idx = L.missing(S.lines, S.trans)
      const groups = L.chunk(idx, 40)
      let done = 0
      for (const g of groups) {
        if (S.closed) return
        const r = await send({ type: 'cc-tr', texts: g.map((i) => S.lines[i].text), target: S.target, engine: 'clearclip' })
        if (!r.ok) throw new Error(r.error || '翻译失败')
        g.forEach((i, k) => (S.trans[i] = String(r.texts?.[k] || '')))
        done += g.length
        setStatus(`正在翻译 ${done}/${idx.length}…`)
        cap.dataset.k = ''
        paint()
      }
      const all = await store.get('subLayerTrans', {})
      all[key] = S.trans
      // 只留最近的 12 份，避免占满存储
      const keys = Object.keys(all)
      for (const k of keys.slice(0, Math.max(0, keys.length - 12))) delete all[k]
      await store.set('subLayerTrans', all)
      setStatus(`已翻译成${S.target}，共 ${S.lines.length} 句`)
    } catch (e) {
      setStatus(e.message)
      toast(ui, e.message, true, 6000)
    } finally {
      S.busy = false
    }
  }

  async function transcribe() {
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    S.busy = true
    try {
      const st = await send({ type: 'cc-ai-status' })
      if (!st.ok) throw new Error(st.error)
      if (st.transcribe === false) throw new Error('清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp，或在模型池里勾选云端语音转文字')
      const sub = await send({ type: 'cc-ai-run', req: { kind: 'transcribe', url: location.href, title: document.title } })
      if (!sub.ok) throw new Error(sub.error)
      for (;;) {
        await new Promise((r) => setTimeout(r, 800))
        if (S.closed) return
        const j = await send({ type: 'cc-ai-job', id: sub.id })
        if (!j.ok) throw new Error(j.error)
        setStatus(`${j.note || '清影正在转写…'}${j.total ? ` ${j.done}%` : ''}`)
        if (j.state === 'error') throw new Error(j.error || '转写失败')
        if (j.state === 'cancelled') throw new Error('已取消')
        if (j.state === 'done') {
          S.lines = (j.result?.lines || []).map((l) => ({ t: l.t, text: l.text }))
          if (!S.lines.length) throw new Error('没有识别出任何语音')
          S.trans = []
          break
        }
      }
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

  async function lookup(word, sentence) {
    const key = `${LN.norm(word)}|${sentence.slice(0, 80)}`
    if (S.lk.has(key)) return S.lk.get(key)
    const sub = await send({ type: 'cc-ai-run', req: { kind: 'lookup', text: word, context: sentence, title: document.title, lang: S.target } })
    if (!sub.ok) throw new Error(sub.error || '查询失败')
    for (let n = 0; n < 80; n++) {
      await new Promise((r) => setTimeout(r, 350))
      if (S.closed) throw new Error('已关闭')
      const j = await send({ type: 'cc-ai-job', id: sub.id })
      if (!j.ok) throw new Error(j.error)
      if (j.state === 'error') throw new Error(j.error || '查询失败')
      if (j.state === 'done') {
        S.lk.set(key, j.result || {})
        if (S.lk.size > 200) S.lk.delete(S.lk.keys().next().value)
        return j.result || {}
      }
    }
    throw new Error('查询超时')
  }

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
    bub.style.left = `${Math.max(6, Math.min(window.innerWidth - 296, r.left + r.width / 2 - 145))}px`
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
    const lemma = res.lemma && LN.norm(res.lemma) !== LN.norm(word) ? res.lemma : word
    const have = S.saved.has(LN.norm(lemma)) || S.saved.has(LN.norm(word))
    const parts = [
      h('div', { class: 'hd' }, h('b', { text: lemma }), res.phonetic ? h('span', { class: 'ph', text: res.phonetic }) : null, res.level ? h('span', { class: 'lv', text: res.level }) : null),
    ]
    if (res.offline) {
      parts.push(h('div', { class: 'tip', text: '清影里没有可用的 AI 模型，只能显示词汇等级。到“设置 → AI”配置后就能看到释义和本句意思。' }))
    } else {
      if (res.meaning) parts.push(h('div', { class: 'ms' }, res.pos ? h('i', { text: res.pos }) : null, document.createTextNode(res.meaning)))
      if (res.context_meaning) parts.push(h('div', { class: 'cx', text: `本句：${res.context_meaning}` }))
      if (res.note) parts.push(h('div', { class: 'tip', text: res.note }))
    }
    const star = h('button', { class: have ? '' : 'p', text: have ? '已收藏' : '收藏到生词本', onclick: () => saveWord(lemma, res, li, star) })
    parts.push(h('div', { class: 'row2' }, star, h('button', { text: '朗读', onclick: () => speak(lemma) }), h('button', { text: '复制', onclick: () => navigator.clipboard?.writeText(lemma).catch(() => {}) })))
    bub.replaceChildren(...parts)
    placeBubble(r)
  }

  async function saveWord(lemma, res, li, btn) {
    const line = S.lines[li] || { t: 0, text: '' }
    const r = await send({
      type: 'cc-vocab',
      op: 'add',
      word: { word: lemma, phonetic: res.phonetic || '', pos: res.pos || '', meaning: res.meaning || '', ctxMeaning: res.context_meaning || '', sentence: line.text, sourceTitle: document.title, sourceUrl: location.href, timeSec: (line.t || 0) / 1000 },
    })
    if (!r.ok) return toast(ui, r.error || '收藏失败', true)
    S.saved.add(LN.norm(lemma))
    S.ver++
    btn.textContent = '已收藏'
    btn.classList.remove('p')
    toast(ui, `已收藏“${lemma}”，可以在清影的“生词本”里复习`)
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
  async function exportVocab(format) {
    const r = await send({ type: 'cc-vocab', op: 'export', format })
    if (!r.ok) return toast(ui, r.error || '导出失败', true)
    const name = `生词本.${r.ext}`
    const a = h('a', { href: URL.createObjectURL(new Blob([r.text], { type: 'text/plain;charset=utf-8' })), download: name })
    document.body.append(a)
    a.click()
    a.remove()
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
    S.ls = { i, done: 0, loops: 3, rate: 0.75, hideO: true, hideT: true, revealed: false, waiting: false, typed: '', result: null, prevRate: v.playbackRate }
    hideBubble(true)
    cap.dataset.k = ''
    playLine()
    renderListen()
  }
  function stopListen() {
    const ls = S.ls
    if (!ls) return
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
    ls.revealed = true
    S.video?.pause()
    cap.dataset.k = ''
    renderListen()
  }
  async function addMissed() {
    const ls = S.ls
    const line = S.lines[ls.i]
    let n = 0
    for (const w of ls.result?.missed || []) {
      const r = await send({ type: 'cc-vocab', op: 'add', word: { word: LN.norm(w), sentence: line.text, sourceTitle: document.title, sourceUrl: location.href, timeSec: line.t / 1000 } })
      if (r.ok) (S.saved.add(LN.norm(w)), n++)
    }
    S.ver++
    toast(ui, n ? `已把 ${n} 个没听出来的词加入生词本` : '没有可加入的词')
  }
  function renderListen() {
    const ls = S.ls
    if (!ls) return
    const v = S.video?.getBoundingClientRect()
    lsEl.classList.add('open')
    lsEl.style.left = `${Math.max(8, Math.min(window.innerWidth - 428, (v ? v.left + v.width / 2 : window.innerWidth / 2) - 210))}px`
    lsEl.style.top = `${Math.max(8, Math.min(window.innerHeight - 330, v ? v.bottom + 8 : 100))}px`
    const box = h('textarea', { placeholder: '把听到的写下来，写完点“提交”对照原句' })
    box.value = ls.typed
    box.addEventListener('keydown', (e) => {
      e.stopPropagation()
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) (e.preventDefault(), submitDictation(box))
    })
    box.addEventListener('keyup', (e) => e.stopPropagation())
    const sel = (opts, cur, fn) => h('select', { onchange: (e) => fn(e.target.value) }, ...opts.map(([v, t]) => h('option', { value: v, text: t, selected: String(v) === String(cur) ? '' : null })))
    const chk = (t, on, fn) => h('label', {}, h('input', { type: 'checkbox', checked: on ? '' : null, onchange: (e) => fn(e.target.checked) }), document.createTextNode(t))
    const res = ls.result
    lsEl.replaceChildren(
      h('div', { class: 'r' }, h('b', { text: `精听 第 ${ls.i + 1} / ${S.lines.length} 句` }), h('span', { class: 'sp' }), h('button', { text: '退出', onclick: () => stopListen() })),
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
      h('div', { class: 'r' }, chk('遮住原文', ls.hideO, (v) => ((ls.hideO = v), (cap.dataset.k = ''))), chk('遮住译文', ls.hideT, (v) => ((ls.hideT = v), (cap.dataset.k = '')))),
      box,
      h('div', { class: 'r' }, h('button', { class: 'p', text: '提交', onclick: () => submitDictation(box) }), h('button', { text: '揭晓原文', onclick: () => ((ls.revealed = true), (cap.dataset.k = ''), renderListen()) }), h('span', { class: 'sc', text: ls.waiting ? '本句播完了，写下来，或点下一句' : '' })),
      res
        ? h(
            'div',
            {},
            h('div', { class: 'res' }, ...res.parts.map((p) => (/\p{L}/u.test(p.text) ? h('span', { class: p.ok ? 'ok' : 'no', text: p.text }) : document.createTextNode(p.text)))),
            h('div', { class: 'r' }, h('span', { class: 'sc', text: `写对了 ${res.score}%${res.missed.length ? `，漏掉 ${res.missed.length} 个词` : '，全对'}` }), h('span', { class: 'sp' }), res.missed.length ? h('button', { text: '错词加入生词本', onclick: addMissed }) : null),
          )
        : null,
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
    delete globalThis.ccSubLayerRun
  }
  globalThis.ccSubLayerRun = { destroy: api_destroy }

  ;(async () => {
    S.prefs = L.clampPrefs(await store.get('subLayerPrefs', null))
    S.on = (await store.get('subLayerOn', true)) !== false
    S.learn = { on: true, level: 1, ...(await store.get('learnPrefs', {})) }
    learnSel.value = S.learn.on ? String(S.learn.level) : 'off'
    const st = await send({ type: 'cc-ai-status' })
    S.pv = st.ok ? String(st.promptVersion || '') : ''
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
      const lb = S.busy && S.status ? S.status.replace(/^正在/, '').slice(0, 10) : '双语'
      if (lbEl.textContent !== lb) lbEl.textContent = lb
    }, 150)
    if (await loadLines().catch((e) => (setStatus(e.message), false))) {
      translateAll()
      refreshMarks()
    }
  })()
})()
