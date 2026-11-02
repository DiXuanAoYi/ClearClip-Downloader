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
    .pill { position: fixed; display: flex; align-items: center; gap: 7px; height: 30px; padding: 0 4px 0 9px; border-radius: 15px; background: #1d2420d9; color: #fff; font-size: 13px; font-weight: 600; box-shadow: 0 2px 8px #0005; user-select: none }
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
  }

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
  ui.add(cap)
  ui.add(pill)
  ui.add(pop)

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
    const key = `${i}|${S.prefs.mode}|${S.trans[i] || ''}`
    if (cap.dataset.k === key) return
    cap.dataset.k = key
    cap.replaceChildren(...rows.map((t, k) => h('span', { class: k ? 'second' : '', text: t })))
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

  // ---------- 生命周期 ----------

  function api_destroy() {
    S.closed = true
    clearInterval(S.timer)
    window.removeEventListener('resize', layout)
    ui.remove()
    delete globalThis.ccSubLayerRun
  }
  globalThis.ccSubLayerRun = { destroy: api_destroy }

  ;(async () => {
    S.prefs = L.clampPrefs(await store.get('subLayerPrefs', null))
    S.on = (await store.get('subLayerOn', true)) !== false
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
    S.timer = setInterval(() => {
      if (!S.video?.isConnected) S.video = pickVideo()
      layout()
      paint()
      const lb = S.busy && S.status ? S.status.replace(/^正在/, '').slice(0, 10) : '双语'
      if (lbEl.textContent !== lb) lbEl.textContent = lb
    }, 150)
    if (await loadLines().catch((e) => (setStatus(e.message), false))) translateAll()
  })()
})()
