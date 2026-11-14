// 写作浮窗：网页里的输入框获得焦点时，在右下角显示小浮标；点开后选择动作和修饰，交给清影生成，
// 先看差异再采用。文字处理都在清影桌面应用里完成（本地模型优先），这里只负责读、写、显示。
;(() => {
  if (globalThis.__ccWriteLoaded) return
  globalThis.__ccWriteLoaded = true
  const { api, mount, h, send } = globalThis.ccUI
  const W = globalThis.ccWrite

  const CSS = `
    :host { --bg:#eaf0f7; --card:#fff; --fg:#0b1424; --mut:#4d5f7b; --line:#d9dedb; --acc:#b45f00; --accd:#b45f00; --acc2:#fbeadb; --del-bg:#fbe3e0; --del-fg:#c4283d; --add-bg:#dff1e6; --add-fg:#007a70; --warn:#b4701a; --err:#c4283d }
    @media (prefers-color-scheme: dark) { :host { --bg:#060a12; --card:#0e1625; --fg:#e8f0fd; --mut:#9aabc6; --line:#1c2b43; --acc:#ffb547; --accd:#c8641f; --acc2:#3a2a1d; --del-bg:#4a2724; --del-fg:#ff7b8a; --add-bg:#1f3a2c; --add-fg:#5cc28a; --warn:#e0a24a; --err:#ff7b8a } }
    button { background: var(--accd); color: #fff; font-weight: 600; border-radius: 8px; }
    button:focus-visible, input:focus-visible, select:focus-visible { outline: 2px solid var(--acc); outline-offset: 1px }
    button.sec { background: transparent; color: var(--fg); border: 1px solid var(--line); font-weight: 400; }
    button:disabled { opacity: .5; cursor: default; }
    .badge { display: flex; align-items: center; gap: 0; height: 28px; padding: 0 8px; border-radius: 14px; box-shadow: 0 2px 8px rgba(180,86,26,.45), 0 0 0 3px color-mix(in srgb, var(--card) 85%, transparent); font-size: 12px; user-select: none; overflow: hidden }
    .badge span { max-width: 0; opacity: 0; white-space: nowrap; transition: max-width .18s, opacity .18s, margin .18s }
    .badge:hover, .badge:focus-visible { background: var(--acc) }
    .badge:hover span, .badge:focus-visible span { max-width: 90px; opacity: 1; margin-left: 5px }
    @media (prefers-reduced-motion: reduce) { .badge span { transition: none } }
    .badge svg { width: 16px; height: 16px; flex: none }
    .panel { resize: horizontal; min-width: 360px; width: 480px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow: auto; background: var(--card); color: var(--fg); border: 1px solid var(--line); border-radius: 12px; box-shadow: 0 10px 30px rgba(20,30,25,.28), 0 1px 3px rgba(20,30,25,.16); font-size: 13px; }
    .hd { display: flex; align-items: center; gap: 8px; padding: 9px 10px 9px 12px; border-bottom: 1px solid var(--line); position: sticky; top: 0; background: var(--bg); z-index: 1; cursor: grab; user-select: none; touch-action: none }
    .hd.drag { cursor: grabbing }
    .logo { width: 20px; height: 20px; border-radius: 6px; background: var(--acc); color: #fff; display: grid; place-items: center; font: 700 11px/1 inherit; flex: none }
    .grip { display: grid; grid-template-columns: repeat(3, 3px); gap: 3px; opacity: .55 }
    .grip i { width: 3px; height: 3px; border-radius: 50%; background: var(--mut) }
    .hd b { font-size: 13px }
    .mut { color: var(--mut); font-size: 11px }
    .bd { padding: 12px 14px; display: flex; flex-direction: column; gap: 10px }
    .sec-t { font-size: 11px; color: var(--mut); letter-spacing: .08em; margin-bottom: 4px }
    .chips { display: flex; flex-wrap: wrap; gap: 6px; flex: 1; min-width: 0 }
    .chip { display: inline-flex; align-items: center; height: 26px; padding: 0 10px; border-radius: 8px; border: 1px solid var(--line); background: transparent; color: var(--fg); font-weight: 400; font-size: 12px; white-space: nowrap }
    .chip.on { background: var(--acc2); border-color: var(--acc); color: var(--accd); font-weight: 600 }
    @media (prefers-color-scheme: dark) { .chip.on { color: var(--acc) } }
    .chip.dash { border-style: dashed }
    .chip.x { height: 22px; padding: 0 8px }
    .row { display: flex; gap: 8px; align-items: flex-start }
    .lbl { width: 52px; flex: none; font-size: 11px; color: var(--mut); padding-top: 5px }
    .sep { border-top: 1px solid var(--line); padding-top: 10px; display: flex; flex-direction: column; gap: 8px }
    input[type=text], select { border: 1px solid var(--line); border-radius: 8px; padding: 0 10px; height: 32px; background: var(--card); color: var(--fg); min-width: 0 }
    .grow { flex: 1; min-width: 0 }
    .box { border: 1px solid var(--line); border-radius: 10px; padding: 12px 14px; line-height: 1.9; font-size: 14px; white-space: pre-wrap; word-break: break-word; background: var(--bg) }
    .del { background: var(--del-bg); color: var(--del-fg); text-decoration: line-through; border-radius: 3px; padding: 0 1px }
    .add { background: var(--add-bg); color: var(--add-fg); border-radius: 3px; padding: 0 1px }
    .hk { cursor: pointer }
    .hk.off .del { text-decoration: none; background: transparent; color: inherit }
    .hk.off .add { display: none }
    .e1 { border-bottom: 2px solid var(--err); background: var(--del-bg) }
    .e2 { border-bottom: 2px solid var(--warn); background: color-mix(in srgb, var(--warn) 18%, var(--bg)) }
    .e3 { border-bottom: 2px solid #1d5fa8; background: color-mix(in srgb, #1d5fa8 16%, var(--bg)) }
    .card { border: 1px solid var(--line); border-radius: 8px; padding: 8px 10px; display: flex; flex-direction: column; gap: 4px }
    .card.skip { opacity: .55 }
    .warn { border: 1px solid var(--warn); color: var(--warn); border-radius: 8px; padding: 8px 10px; font-size: 12px; line-height: 1.6 }
    .err { color: var(--err) }
    .acts { display: flex; gap: 8px; flex-wrap: wrap; align-items: center }
    a { color: var(--accd) }
    .spin { width: 12px; height: 12px; border-radius: 50%; border: 2px solid currentColor; border-top-color: transparent; display: inline-block; animation: sp .8s linear infinite }
    @keyframes sp { to { transform: rotate(360deg) } }
  `

  // 拆出去的模块（writing-field / options / result .js）和主文件共享的状态与工具：配置、当前输入框、面板状态、位置偏好
  const C = { W, h, send, api }
  C.cfg = W.normalizeCfg(null)
  let ui = null
  let badge = null
  let panel = null
  C.target = null // 当前输入框
  let composing = false
  C.st = null // 面板状态
  let raf = 0

  const load = async () => {
    try {
      const { writeCfg } = await api.storage.local.get('writeCfg')
      C.cfg = W.normalizeCfg(writeCfg)
    } catch {
      // 扩展刚更新
    }
    if (!usable()) hideBadge()
  }
  load()
  try {
    api.storage.onChanged.addListener((ch, area) => {
      if (area === 'local' && ch.writeCfg) load()
    })
  } catch {
    // 扩展已失效
  }
  // 界面偏好：面板位置/宽度、浮标位置、最近的额外要求
  C.pref = { geom: null, w: 0, badge: null, extras: [] }
  api.storage.local.get('writeUi').then((r) => {
    const p = r?.writeUi
    if (p && typeof p === 'object') C.pref = { geom: p.geom || null, w: Number(p.w) || 0, badge: p.badge || null, extras: Array.isArray(p.extras) ? p.extras.slice(0, 8) : [] }
  }).catch(() => {})
  const savePref = () => api.storage.local.set({ writeUi: C.pref }).catch(() => {})
  const saveCfg = () => api.storage.local.set({ writeCfg: C.cfg }).catch(() => {})

  const usable = () => !W.hostOffNow(C.cfg, location.hostname)

  // ---------- 输入框 ----------

  // ---------- 浮标 ----------

  let lastField = null
  let userMoved = false
  let reclaims = []

  /** 网页抢走焦点时还回去。2 秒内最多还 4 次，网页和我们抢个没完就不再争（让用户自己点）。 */
  function reclaimFocus(e) {
    if (!panel || !lastField || !lastField.isConnected || userMoved || (ui && e.target === ui.host)) return false
    const now = Date.now()
    reclaims = reclaims.filter((t) => now - t < 2000)
    if (reclaims.length >= 4) return false
    reclaims.push(now)
    const el = lastField
    setTimeout(() => panel && el.isConnected && el.focus({ preventScroll: true }), 0)
    return true
  }

  function ensureUi() {
    if (ui) return
    ui = mount(CSS)
    // 点击浮标和面板时不要让输入框失去焦点（否则选区会丢）；真正的输入控件除外
    ui.root.addEventListener('mousedown', (e) => {
      const t = e.composedPath()[0]
      if (!(t instanceof HTMLElement) || !t.matches('input, select, textarea, a')) e.preventDefault()
    })
    // 记住面板里最后聚焦的输入框；网页没经过用户操作就把焦点抢走时（焦点陷阱、自动聚焦），把焦点还给它，否则输入框看起来“打不了字”
    ui.root.addEventListener('focusin', (e) => {
      const t = e.composedPath()[0]
      if (t instanceof HTMLElement && t.matches('input, textarea, select')) {
        lastField = t
        userMoved = false
      }
    })
    document.addEventListener('pointerdown', (e) => { if (!(ui && e.composedPath().includes(ui.host))) userMoved = true }, true)
    document.addEventListener('keydown', (e) => { if (e.key === 'Tab') userMoved = true }, true)
    // 面板里的输入框：键盘、输入法、点击事件都不要冒泡到网页（很多网站的快捷键、编辑器、点击空白处收起逻辑会抢焦点或吞字）
    for (const ev of ['keydown', 'keyup', 'keypress', 'beforeinput', 'input', 'compositionstart', 'compositionupdate', 'compositionend', 'paste', 'cut', 'copy', 'mousedown', 'mouseup', 'pointerdown', 'pointerup', 'click', 'dblclick', 'contextmenu', 'focusin', 'focusout']) {
      ui.root.addEventListener(ev, (e) => e.stopPropagation())
    }
  }

  function penIcon() {
    const ns = 'http://www.w3.org/2000/svg'
    const s = document.createElementNS(ns, 'svg')
    s.setAttribute('viewBox', '0 0 24 24')
    s.setAttribute('fill', 'none')
    s.setAttribute('stroke', 'currentColor')
    s.setAttribute('stroke-width', '2')
    s.setAttribute('stroke-linecap', 'round')
    s.setAttribute('stroke-linejoin', 'round')
    for (const d of ['M4 20l4-1 11-11-3-3L5 16z', 'M14 6l3 3']) {
      const p = document.createElementNS(ns, 'path')
      p.setAttribute('d', d)
      s.append(p)
    }
    return s
  }

  function badgeLabel() {
    const n = C.target ? W.countText(readAll(C.target)) : 0
    return n > 0 ? `${n} 字` : '清影写作'
  }

  function showBadge() {
    if (C.cfg.trigger !== 'badge' || !C.target || composing || !usable()) return hideBadge()
    if (!C.cfg.showEmpty && !readAll(C.target).trim()) return hideBadge()
    ensureUi()
    if (!badge) {
      badge = h('button', { class: 'layer badge', title: '清影写作（Alt+J）· 可拖动位置', onclick: () => (badgeMoved ? (badgeMoved = false) : openPanel()), onpointerdown: onBadgeDown })
      ui.add(badge)
    }
    badge.replaceChildren(penIcon(), h('span', { text: badgeLabel() }))
    badge.setAttribute('aria-label', '清影写作')
    placeBadge()
  }

  let badgeMoved = false
  function onBadgeDown(e) {
    if (e.button > 0) return
    const x0 = e.clientX
    const y0 = e.clientY
    const o0 = C.pref.badge || { dx: 0, dy: 0 }
    const el = e.currentTarget
    el.setPointerCapture?.(e.pointerId)
    badgeMoved = false
    const move = (ev) => {
      const dx = ev.clientX - x0
      const dy = ev.clientY - y0
      if (!badgeMoved && Math.hypot(dx, dy) < 5) return
      badgeMoved = true
      C.pref.badge = { dx: Math.round(o0.dx + dx), dy: Math.round(o0.dy + dy) }
      placeBadge()
    }
    const up = () => {
      el.removeEventListener('pointermove', move)
      el.removeEventListener('pointerup', up)
      el.removeEventListener('pointercancel', up)
      if (badgeMoved) savePref()
      setTimeout(() => (badgeMoved = false), 0)
    }
    el.addEventListener('pointermove', move)
    el.addEventListener('pointerup', up)
    el.addEventListener('pointercancel', up)
  }

  function placeBadge() {
    if (!badge || !C.target) return
    const r = C.target.getBoundingClientRect()
    if (r.bottom < 24 || r.top > innerHeight - 24 || r.right < 24 || r.left > innerWidth - 24) {
      badge.style.display = 'none'
      return
    }
    badge.style.display = ''
    const w = badge.offsetWidth || 90
    const o = C.pref.badge || { dx: 0, dy: 0 }
    const left = Math.max(4, Math.min(r.right - w - 8 + o.dx, innerWidth - w - 4))
    const top = Math.max(4, Math.min(r.bottom - 38 + o.dy, innerHeight - 38))
    badge.style.left = `${left}px`
    badge.style.top = `${top}px`
  }

  function hideBadge() {
    if (badge) badge.style.display = 'none'
  }

  const schedulePlace = () => {
    if (raf) return
    raf = requestAnimationFrame(() => {
      raf = 0
      placeBadge()
      placePanel()
    })
  }

  function setTarget(el) {
    const root = editableRoot(el)
    if (!root) return false
    if (!W.fieldVerdict(infoOf(root), C.cfg).ok) return false
    C.target = root
    return true
  }

  document.addEventListener(
    'focusin',
    (e) => {
      if (ui && e.target === ui.host) return
      if (reclaimFocus(e)) return
      const t = e.composedPath?.()[0] || e.target
      if (setTarget(t)) showBadge()
      else if (!panel) {
        C.target = null
        hideBadge()
      }
    },
    true,
  )
  document.addEventListener(
    'focusout',
    () => {
      setTimeout(() => {
        if (panel) return
        const a = deepActive()
        if (!editableRoot(a)) hideBadge()
      }, 250)
    },
    true,
  )
  document.addEventListener('input', (e) => {
    if (C.target && e.composedPath?.()[0] === C.target && !composing) showBadge()
  }, true)
  document.addEventListener('compositionstart', () => {
    composing = true
    hideBadge()
  }, true)
  document.addEventListener('compositionend', () => {
    composing = false
    if (C.target) setTimeout(showBadge, 0)
  }, true)
  addEventListener('scroll', schedulePlace, { capture: true, passive: true })
  addEventListener('resize', schedulePlace)
  document.addEventListener('keydown', (e) => {
    if (!panel || !C.st) return
    if (e.key === 'Escape') closePanel()
    else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.isComposing && C.st.view === 'options') {
      e.preventDefault()
      run()
    } else if (e.altKey && ['1', '2', '3'].includes(e.key) && C.st.view === 'result' && C.st.res.cands.length >= Number(e.key)) {
      e.preventDefault()
      C.st.res.idx = Number(e.key) - 1
      buildParts()
      render()
    }
  }, true)

  // 快捷键（后台转发）：对有焦点的那个框架生效
  try {
    api.runtime.onMessage.addListener((m) => {
      if (m?.type !== 'cc-write-open' || !document.hasFocus() || !usable()) return
      const a = deepActive()
      if (setTarget(a)) openPanel()
      else openPanel({ readonly: true })
    })
  } catch {
    // 扩展已失效
  }

  // ---------- 面板 ----------

  const asArray = (x) => (Array.isArray(x) ? x : [])

  function openPanel(opts = {}) {
    ensureUi()
    // 目标在对话框里时，面板也放进同一个对话框：模态对话框外面的东西是“惰性”的点不了、打不了字，焦点陷阱也只放行自己里面的元素
    ui.place(C.target?.closest?.('dialog[open], [aria-modal="true"], [role="dialog"]'))
    lastField = null
    userMoved = false
    let snap = null
    if (opts.readonly) {
      const s = getSelection()?.toString() || ''
      snap = { el: null, full: s, sel: s, readonly: true }
    } else {
      snap = snapshot(C.target)
    }
    if (!snap) return
    if (panel) panel.remove()
    C.st = {
      snap,
      scope: snap.sel.trim() ? 'sel' : 'all',
      sel: { ...C.cfg.last, extra: '' },
      view: 'options',
      note: '',
      info: null,
      job: 0,
      res: null,
      facts: null,
      history: null,
    }
    dragPos = C.pref.geom ? { left: C.pref.geom.left, top: C.pref.geom.top } : null
    panel = h('div', { class: 'layer panel' })
    if (C.pref.w) panel.style.width = `${Math.max(360, Math.min(C.pref.w, innerWidth - 16))}px`
    ui.add(panel)
    let rzT = 0
    let first = true
    try {
      new ResizeObserver(() => {
        if (first) return void (first = false)
        clearTimeout(rzT)
        rzT = setTimeout(() => {
          if (!panel) return
          const w = Math.round(panel.offsetWidth)
          if (w >= 360 && w !== C.pref.w) {
            C.pref.w = w
            savePref()
          }
        }, 400)
      }).observe(panel)
    } catch {
      // 不支持就不记宽度
    }
    send({ type: 'cc-ai-status' }).then((s) => {
      if (C.st) {
        C.st.info = s
        if (C.st.view === 'options') render()
      }
    })
    render()
    placePanel()
  }

  function closePanel() {
    if (C.st?.job) send({ type: 'cc-ai-cancel', id: C.st.job })
    panel?.remove()
    panel = null
    C.st = null
    const el = C.target
    if (el && el.isConnected) {
      try {
        el.focus({ preventScroll: true })
      } catch {
        // 忽略
      }
    }
  }

  let dragPos = null // 用户拖过的位置，重新打开时清掉
  function clampPos(left, top) {
    const w = panel.offsetWidth || 480
    const hh = panel.offsetHeight || 420
    return [Math.max(4 - w + 80, Math.min(left, innerWidth - 80)), Math.max(0, Math.min(top, innerHeight - 36))].map(Math.round)
  }
  function onHdDown(e) {
    if (!panel || e.button > 0 || e.target.closest?.('button, input, select, a')) return
    const hd = e.currentTarget
    const r = panel.getBoundingClientRect()
    const dx = e.clientX - r.left
    const dy = e.clientY - r.top
    hd.setPointerCapture?.(e.pointerId)
    hd.classList.add('drag')
    const move = (ev) => {
      if (!panel) return
      const [l, t] = clampPos(ev.clientX - dx, ev.clientY - dy)
      dragPos = { left: l, top: t }
      panel.style.left = `${l}px`
      panel.style.top = `${t}px`
    }
    const up = () => {
      if (dragPos) {
        C.pref.geom = { ...dragPos }
        savePref()
      }
      hd.classList.remove('drag')
      hd.removeEventListener('pointermove', move)
      hd.removeEventListener('pointerup', up)
      hd.removeEventListener('pointercancel', up)
    }
    hd.addEventListener('pointermove', move)
    hd.addEventListener('pointerup', up)
    hd.addEventListener('pointercancel', up)
    e.preventDefault()
  }

  function placePanel() {
    if (panel && dragPos) {
      const [l, t] = clampPos(dragPos.left, dragPos.top)
      panel.style.left = `${l}px`
      panel.style.top = `${t}px`
      return
    }
    if (!panel || !C.target) {
      if (panel) {
        panel.style.left = `${Math.max(8, (innerWidth - 520) / 2)}px`
        panel.style.top = '48px'
      }
      return
    }
    const r = C.target.getBoundingClientRect()
    const w = Math.min(520, innerWidth - 16)
    const left = Math.max(8, Math.min(r.right - w, innerWidth - w - 8))
    const hh = panel.offsetHeight || 420
    const below = r.bottom + 8
    const top = below + hh <= innerHeight - 8 ? below : Math.max(8, r.top - hh - 8)
    panel.style.left = `${left}px`
    panel.style.top = `${Math.max(8, Math.min(top, innerHeight - hh - 8))}px`
  }

  const scopeText = () => (C.st.scope === 'sel' ? C.st.snap.sel : C.st.snap.full)
  const chip = (text, on, fn, cls = '') => h('button', { class: `chip${on ? ' on' : ''} ${cls}`, text, onclick: fn })

  function header(title, extra) {
    return h(
      'div',
      { class: 'hd', onpointerdown: onHdDown },
      h('span', { class: 'logo', text: '写' }),
      h('b', { text: title }),
      extra || null,
      h('span', { class: 'mut', style: 'margin-left:auto', text: modelLabel() }),
      h('span', { class: 'grip' }, ...Array.from({ length: 6 }, () => h('i'))),
      h('button', { class: 'sec chip x', text: '×', title: '关闭 (Esc)', onclick: closePanel }),
    )
  }

  function modelLabel() {
    const i = C.st.info
    if (!i || !i.ok) return ''
    if (C.cfg.route === 'cloud_only') return '云端模型'
    if (C.cfg.route === 'local_only') return '本地模型'
    return i.localModel ? '本地模型优先' : '云端模型'
  }

  function render() {
    if (!panel || !C.st) return
    const keep = panel.scrollTop
    panel.replaceChildren(...(VIEWS[C.st.view] || VIEWS.options)())
    panel.scrollTop = keep
    placePanel()
  }

  // 模块里的函数：声明提升，调用时再去 C 里取
  function buildParts(...a) { return C.buildParts(...a) }
  function deepActive(...a) { return C.deepActive(...a) }
  function editableRoot(...a) { return C.editableRoot(...a) }
  function infoOf(...a) { return C.infoOf(...a) }
  function readAll(...a) { return C.readAll(...a) }
  function run(...a) { return C.run(...a) }
  function snapshot(...a) { return C.snapshot(...a) }
  function viewFacts(...a) { return C.viewFacts(...a) }
  function viewHistory(...a) { return C.viewHistory(...a) }
  function viewIssues(...a) { return C.viewIssues(...a) }
  function viewOptions(...a) { return C.viewOptions(...a) }
  function viewResult(...a) { return C.viewResult(...a) }
  function viewRunning(...a) { return C.viewRunning(...a) }

  Object.assign(C, { asArray, chip, closePanel, header, render, saveCfg, savePref, scopeText, showBadge })
  for (const m of ['field', 'options', 'result']) globalThis.ccParts.write[m](C)

  const VIEWS = { options: viewOptions, running: viewRunning, result: viewResult, issues: viewIssues, facts: viewFacts, history: viewHistory }
})()
