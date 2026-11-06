// 写作浮窗：网页里的输入框获得焦点时，在右下角显示小浮标；点开后选择动作和修饰，交给清影生成，
// 先看差异再采用。文字处理都在清影桌面应用里完成（本地模型优先），这里只负责读、写、显示。
;(() => {
  if (globalThis.__ccWriteLoaded) return
  globalThis.__ccWriteLoaded = true
  const { api, mount, h, send } = globalThis.ccUI
  const W = globalThis.ccWrite

  const CSS = `
    :host { --bg:#f4f5f4; --card:#fff; --fg:#1d2420; --mut:#5d6862; --line:#d9dedb; --acc:#d9762b; --accd:#b4561a; --acc2:#fbeadb; --del-bg:#fbe3e0; --del-fg:#c2453b; --add-bg:#dff1e6; --add-fg:#2f8a57; --warn:#b4701a; --err:#c2453b }
    @media (prefers-color-scheme: dark) { :host { --bg:#14181a; --card:#22292c; --fg:#e7ecea; --mut:#9aa6a2; --line:#2f383c; --acc:#f08a3c; --accd:#c8641f; --acc2:#3a2a1d; --del-bg:#4a2724; --del-fg:#ec7a70; --add-bg:#1f3a2c; --add-fg:#5cc28a; --warn:#e0a24a; --err:#ec7a70 } }
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

  let cfg = W.normalizeCfg(null)
  let ui = null
  let badge = null
  let panel = null
  let target = null // 当前输入框
  let composing = false
  let st = null // 面板状态
  let raf = 0

  const load = async () => {
    try {
      const { writeCfg } = await api.storage.local.get('writeCfg')
      cfg = W.normalizeCfg(writeCfg)
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
  let pref = { geom: null, w: 0, badge: null, extras: [] }
  api.storage.local.get('writeUi').then((r) => {
    const p = r?.writeUi
    if (p && typeof p === 'object') pref = { geom: p.geom || null, w: Number(p.w) || 0, badge: p.badge || null, extras: Array.isArray(p.extras) ? p.extras.slice(0, 8) : [] }
  }).catch(() => {})
  const savePref = () => api.storage.local.set({ writeUi: pref }).catch(() => {})
  const saveCfg = () => api.storage.local.set({ writeCfg: cfg }).catch(() => {})

  const usable = () => !W.hostOffNow(cfg, location.hostname)

  // ---------- 输入框 ----------

  function deepActive() {
    let a = document.activeElement
    while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement
    return a
  }

  function editableRoot(el) {
    if (!el || el.nodeType !== 1) return null
    if (el.matches?.('textarea, input')) return el
    if (el.isContentEditable) {
      let r = el
      while (r.parentElement && r.parentElement.isContentEditable) r = r.parentElement
      return r
    }
    return null
  }

  function infoOf(el) {
    const r = el.getBoundingClientRect()
    const cs = getComputedStyle(el)
    return {
      tag: el.tagName,
      type: el.type,
      editable: el.isContentEditable,
      disabled: el.disabled,
      readOnly: el.readOnly || el.getAttribute('aria-readonly') === 'true',
      autocomplete: el.getAttribute('autocomplete') || '',
      name: el.getAttribute('name') || '',
      id: el.id || '',
      placeholder: el.getAttribute('placeholder') || '',
      ariaLabel: el.getAttribute('aria-label') || '',
      width: r.width,
      height: r.height,
      hidden: cs.visibility === 'hidden' || cs.display === 'none' || r.width === 0,
    }
  }

  const isField = (el) => el.matches?.('textarea, input')
  const readAll = (el) => (isField(el) ? el.value : el.innerText || '')

  function snapshot(el) {
    if (!el) return null
    const full = readAll(el)
    if (isField(el)) {
      let s = 0
      let e = 0
      try {
        s = el.selectionStart ?? 0
        e = el.selectionEnd ?? 0
      } catch {
        // 有些输入类型不支持读取选区
      }
      return { el, full, start: s, end: e, sel: full.slice(s, e) }
    }
    const sel = getSelection()
    let range = null
    let selText = ''
    if (sel && sel.rangeCount && el.contains(sel.anchorNode)) {
      range = sel.getRangeAt(0).cloneRange()
      selText = sel.toString()
    }
    return { el, full, range, sel: selText }
  }

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
    const n = target ? W.countText(readAll(target)) : 0
    return n > 0 ? `${n} 字` : '清影写作'
  }

  function showBadge() {
    if (cfg.trigger !== 'badge' || !target || composing || !usable()) return hideBadge()
    if (!cfg.showEmpty && !readAll(target).trim()) return hideBadge()
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
    const o0 = pref.badge || { dx: 0, dy: 0 }
    const el = e.currentTarget
    el.setPointerCapture?.(e.pointerId)
    badgeMoved = false
    const move = (ev) => {
      const dx = ev.clientX - x0
      const dy = ev.clientY - y0
      if (!badgeMoved && Math.hypot(dx, dy) < 5) return
      badgeMoved = true
      pref.badge = { dx: Math.round(o0.dx + dx), dy: Math.round(o0.dy + dy) }
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
    if (!badge || !target) return
    const r = target.getBoundingClientRect()
    if (r.bottom < 24 || r.top > innerHeight - 24 || r.right < 24 || r.left > innerWidth - 24) {
      badge.style.display = 'none'
      return
    }
    badge.style.display = ''
    const w = badge.offsetWidth || 90
    const o = pref.badge || { dx: 0, dy: 0 }
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
    if (!W.fieldVerdict(infoOf(root), cfg).ok) return false
    target = root
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
        target = null
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
    if (target && e.composedPath?.()[0] === target && !composing) showBadge()
  }, true)
  document.addEventListener('compositionstart', () => {
    composing = true
    hideBadge()
  }, true)
  document.addEventListener('compositionend', () => {
    composing = false
    if (target) setTimeout(showBadge, 0)
  }, true)
  addEventListener('scroll', schedulePlace, { capture: true, passive: true })
  addEventListener('resize', schedulePlace)
  document.addEventListener('keydown', (e) => {
    if (!panel || !st) return
    if (e.key === 'Escape') closePanel()
    else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.isComposing && st.view === 'options') {
      e.preventDefault()
      run()
    } else if (e.altKey && ['1', '2', '3'].includes(e.key) && st.view === 'result' && st.res.cands.length >= Number(e.key)) {
      e.preventDefault()
      st.res.idx = Number(e.key) - 1
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
    ui.place(target?.closest?.('dialog[open], [aria-modal="true"], [role="dialog"]'))
    lastField = null
    userMoved = false
    let snap = null
    if (opts.readonly) {
      const s = getSelection()?.toString() || ''
      snap = { el: null, full: s, sel: s, readonly: true }
    } else {
      snap = snapshot(target)
    }
    if (!snap) return
    if (panel) panel.remove()
    st = {
      snap,
      scope: snap.sel.trim() ? 'sel' : 'all',
      sel: { ...cfg.last, extra: '' },
      view: 'options',
      note: '',
      info: null,
      job: 0,
      res: null,
      facts: null,
      history: null,
    }
    dragPos = pref.geom ? { left: pref.geom.left, top: pref.geom.top } : null
    panel = h('div', { class: 'layer panel' })
    if (pref.w) panel.style.width = `${Math.max(360, Math.min(pref.w, innerWidth - 16))}px`
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
          if (w >= 360 && w !== pref.w) {
            pref.w = w
            savePref()
          }
        }, 400)
      }).observe(panel)
    } catch {
      // 不支持就不记宽度
    }
    send({ type: 'cc-ai-status' }).then((s) => {
      if (st) {
        st.info = s
        if (st.view === 'options') render()
      }
    })
    render()
    placePanel()
  }

  function closePanel() {
    if (st?.job) send({ type: 'cc-ai-cancel', id: st.job })
    panel?.remove()
    panel = null
    st = null
    const el = target
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
        pref.geom = { ...dragPos }
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
    if (!panel || !target) {
      if (panel) {
        panel.style.left = `${Math.max(8, (innerWidth - 520) / 2)}px`
        panel.style.top = '48px'
      }
      return
    }
    const r = target.getBoundingClientRect()
    const w = Math.min(520, innerWidth - 16)
    const left = Math.max(8, Math.min(r.right - w, innerWidth - w - 8))
    const hh = panel.offsetHeight || 420
    const below = r.bottom + 8
    const top = below + hh <= innerHeight - 8 ? below : Math.max(8, r.top - hh - 8)
    panel.style.left = `${left}px`
    panel.style.top = `${Math.max(8, Math.min(top, innerHeight - hh - 8))}px`
  }

  const scopeText = () => (st.scope === 'sel' ? st.snap.sel : st.snap.full)
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
    const i = st.info
    if (!i || !i.ok) return ''
    if (cfg.route === 'cloud_only') return '云端模型'
    if (cfg.route === 'local_only') return '本地模型'
    return i.localModel ? '本地模型优先' : '云端模型'
  }

  function render() {
    if (!panel || !st) return
    const keep = panel.scrollTop
    panel.replaceChildren(...(VIEWS[st.view] || VIEWS.options)())
    panel.scrollTop = keep
    placePanel()
  }

  // ---- 选项视图
  function viewOptions() {
    const s = st.sel
    const a = W.actionOf(s.action)
    const kids = []
    const scopeChips = h('div', { class: 'acts' }, h('span', { class: 'mut', text: '处理范围：' }))
    if (st.snap.sel.trim()) scopeChips.append(chip(`选中 ${W.countText(st.snap.sel)} 字`, st.scope === 'sel', () => ((st.scope = 'sel'), render())))
    scopeChips.append(chip(`全文 ${W.countText(st.snap.full)} 字`, st.scope === 'all', () => ((st.scope = 'all'), render())))
    kids.push(header('清影写作', scopeChips))
    const bd = h('div', { class: 'bd' })
    if (st.info && !st.info.ok) bd.append(h('div', { class: 'warn', text: `连不上清影桌面应用：${st.info.error || '请确认清影正在运行，并在扩展设置里填好访问令牌。'}` }))
    else if (st.info && !st.info.ready) bd.append(h('div', { class: 'warn', text: '清影里还没有配置 AI 模型：请在清影的“设置 → AI”里填写，或安装本地模型。' }))
    if (st.note) {
      bd.append(h('div', { class: 'warn', text: st.note }))
      if (st.noteErr) bd.append(h('div', { class: 'acts' }, h('button', { class: 'sec', text: '重试', onclick: () => run() })))
    }

    const pickAction = (key) => {
      s.action = key
      if (cfg.runOnClick) run()
      else render()
    }
    bd.append(
      h('div', {}, h('div', { class: 'sec-t', text: '基础' }), h('div', { class: 'chips' }, ...W.BASIC.map((x) => chip(x.name, s.action === x.key, () => pickAction(x.key))))),
      h('div', {}, h('div', { class: 'sec-t', text: '改写风格' }), h('div', { class: 'chips' }, ...W.STYLES.map((x) => chip(x.name, s.action === x.key, () => pickAction(x.key))))),
    )

    const sep = h('div', { class: 'sep' })
    for (const key of ['length', 'formality']) sep.append(modRow(key))
    sep.append(modRow('format'), modRow('tone'))
    if (s.action === 'translate') {
      const sel = h('select', { class: 'grow', onchange: (e) => (s.target = e.target.value) })
      for (const [v, n] of W.TARGETS) sel.append(h('option', { value: v, text: n, selected: s.target === v }))
      sep.append(h('div', { class: 'row' }, h('span', { class: 'lbl', style: 'padding-top:8px', text: '翻译成' }), sel))
    }
    bd.append(sep)

    const extra = h('input', { type: 'text', class: 'grow', placeholder: '还有特别要求？例如“对方是客户，别太生硬”', value: s.extra || '', maxlength: '200', oninput: (e) => (s.extra = e.target.value) })
    extra.addEventListener('keydown', (e) => {
      // 输入法选字时按回车是“确认拼音”，不能当成运行
      if (e.key === 'Enter' && !e.isComposing && e.keyCode !== 229) run()
      e.stopPropagation()
    })
    const recent = pref.extras.length
      ? h('div', { class: 'chips' }, ...pref.extras.slice(0, 5).map((t) => chip(t.length > 12 ? t.slice(0, 12) + '…' : t, false, () => ((s.extra = t), (extra.value = t)), 'mut')))
      : null
    const cand = h('select', { title: '一次生成几个候选', onchange: (e) => ((cfg.candidates = Number(e.target.value)), saveCfg()) })
    for (const n of [1, 2, 3]) cand.append(h('option', { value: String(n), text: `${n} 个候选`, selected: cfg.candidates === n }))
    // Element.append(null) 会把 null 当成文字塞进页面（面板底下那行“null”），所以先滤掉空的
    bd.append(
      ...[
      h(
        'div',
        { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' },
        extra,
        a?.kind === 'grammar' || a?.kind === 'stats' ? null : cand,
        h('button', { text: a?.kind === 'stats' ? '查找数据' : a?.kind === 'grammar' ? '检查' : '生成', onclick: run, disabled: !scopeText().trim() }),
      ),
      recent,
      h('div', { class: 'acts mut' }, h('span', { text: 'Ctrl+Enter 生成 · Esc 关闭' }), h('a', { href: '#', text: '历史记录', onclick: (e) => (e.preventDefault(), showHistory()) })),
      ].filter(Boolean),
    )
    kids.push(bd)
    return kids
  }

  function modRow(key) {
    const m = W.MODS[key]
    const s = st.sel
    const cur = s[key]
    const chips = h('div', { class: 'chips' })
    for (const [v, n] of m.items) chips.append(chip(n, cur === v, () => ((s[key] = v), render())))
    if (m.custom) {
      const saved = key === 'format' ? cfg.customFormats : cfg.customTones
      const field = key === 'format' ? 'customFormat' : 'customTone'
      for (const name of saved) chips.append(chip(name, cur === 'custom' && s[field] === name, () => ((s[key] = 'custom'), (s[field] = name), render())))
      chips.append(
        chip(key === 'format' ? '+ 添加格式' : '+ 添加语气', false, () => {
          const input = h('input', { type: 'text', placeholder: key === 'format' ? '如：小红书风' : '如：像老朋友', maxlength: '40', style: 'height:26px;width:140px' })
          const ok = () => {
            const v = input.value.trim()
            if (!v) return render()
            const list = key === 'format' ? cfg.customFormats : cfg.customTones
            if (!list.includes(v)) list.push(v)
            s[key] = 'custom'
            s[field] = v
            saveCfg()
            render()
          }
          input.addEventListener('keydown', (e) => {
            e.stopPropagation()
            if (e.isComposing || e.keyCode === 229) return
            if (e.key === 'Enter') ok()
            if (e.key === 'Escape') render()
          })
          const holder = h('span', { class: 'acts' }, input, h('button', { class: 'chip x', text: '添加', onclick: ok }))
          chips.replaceChildren(holder)
          setTimeout(() => input.focus(), 0)
        }, 'dash'),
      )
    }
    return h('div', { class: 'row' }, h('span', { class: 'lbl', text: m.name }), chips)
  }

  // ---- 运行
  async function job(req) {
    let id = 0
    try {
      return await globalThis.ccUI.aiRun(req, { interval: 600, onSubmit: (x) => ((id = x), st && (st.job = x)), stop: () => !st || st.job !== id })
    } finally {
      if (st && st.job === id) st.job = 0
    }
  }

  async function run(extraOpts = {}) {
    if (!st) return
    const text = scopeText()
    if (!text.trim()) return
    const s = st.sel
    Object.assign(cfg.last, { action: s.action, length: s.length, formality: s.formality, format: s.format, tone: s.tone, target: s.target, customFormat: s.customFormat || '', customTone: s.customTone || '' })
    saveCfg()
    const ex = (s.extra || '').trim()
    if (ex) {
      pref.extras = [ex, ...pref.extras.filter((x) => x !== ex)].slice(0, 8)
      savePref()
    }
    const req = W.buildRequest(s, text, { candidates: cfg.candidates, route: cfg.route, page: `${document.title} ${location.hostname}`, ...extraOpts })
    st.view = 'running'
    st.reqKind = req.kind
    render()
    try {
      const out = await job(req)
      if (!st) return
      if (req.kind === 'grammar') {
        st.res = { issues: asArray(out.issues), skipped: new Set() }
        st.view = 'issues'
      } else if (req.kind === 'stats') {
        st.facts = { list: asArray(out.facts), picked: new Set(asArray(out.facts).map((_, i) => i)), note: out.note || '' }
        st.view = 'facts'
      } else {
        const cands = asArray(out.candidates)
        if (!cands.length) throw new Error('模型没有返回内容')
        st.res = { src: text, cands, idx: 0, mode: ['translate', 'summarize', 'continue'].includes(s.action) ? 'new' : 'diff', rejected: new Set(), parts: null }
        buildParts()
        st.view = 'result'
      }
    } catch (e) {
      if (!st) return
      st.view = 'options'
      st.note = e.message === '已取消' ? '' : e.message
      st.noteErr = !!st.note
    }
    render()
  }

  function viewRunning() {
    const label = st.reqKind === 'stats' ? '正在开放数据库里查找数据…' : st.reqKind === 'grammar' ? '正在检查…' : '正在生成…'
    return [
      header('清影写作'),
      h(
        'div',
        { class: 'bd' },
        h('div', { class: 'acts' }, h('span', { class: 'spin' }), h('span', { text: label })),
        h('div', { class: 'acts' }, h('button', { class: 'sec', text: '取消', onclick: () => {
          if (st.job) send({ type: 'cc-ai-cancel', id: st.job })
          st.job = 0
          st.view = 'options'
          render()
        } })),
      ),
    ]
  }

  // ---- 结果：差异预览
  function buildParts() {
    const r = st.res
    r.parts = W.hunks(W.diffOps(r.src, r.cands[r.idx].text))
    r.rejected = new Set()
  }

  const finalText = () => {
    const r = st.res
    return r.mode === 'new' || st.sel.action === 'continue' ? r.cands[r.idx].text : W.compose(r.parts, r.rejected)
  }

  function viewResult() {
    const r = st.res
    const c = r.cands[r.idx]
    const isCont = st.sel.action === 'continue'
    const a = W.actionOf(st.sel.action)
    const head = h('div', { class: 'acts' }, h('span', { class: 'mut', text: `${W.countText(r.src)} 字 → ${W.countText(c.text)} 字` }))
    const kids = [header(a?.name || '结果', head)]
    const bd = h('div', { class: 'bd' })
    const bar = h('div', { class: 'acts' })
    if (r.cands.length > 1) {
      bar.append(h('span', { class: 'mut', text: '候选' }))
      r.cands.forEach((_, i) => bar.append(chip(`方案 ${i + 1}`, r.idx === i, () => ((r.idx = i), buildParts(), render()))))
    }
    if (!isCont) {
      const modes = h('span', { class: 'acts', style: 'margin-left:auto' })
      modes.append(chip('差异', r.mode === 'diff', () => ((r.mode = 'diff'), render())), chip('只看新文', r.mode === 'new', () => ((r.mode = 'new'), render())), chip('对比原文', r.mode === 'cmp', () => ((r.mode = 'cmp'), render())))
      bar.append(modes)
    }
    bd.append(bar)
    if (c.guard?.warn) {
      const bits = []
      if (c.guard.missing?.length) bits.push(`原文里的 ${c.guard.missing.slice(0, 6).join('、')} 在结果里不见了，请核对。`)
      const free = ['continue', 'reply'].includes(st.sel.action) // 续写、回复的长短和原文无关
      const shrinks = ['summarize', 'shorten', 'concise', 'translate'].includes(st.sel.action)
      if (!free && (c.guard.ratio > 4 || (!shrinks && c.guard.ratio < 0.25))) bits.push(c.guard.ratio > 1 ? '结果比原文长了很多，可能多出内容。' : '结果比原文短了很多，可能漏掉内容。')
      if (bits.length) bd.append(h('div', { class: 'warn', text: '⚠ ' + bits.join(' ') }))
    }
    const box = h('div', { class: 'box' })
    if (isCont) {
      box.append(h('span', { class: 'add', text: c.text }))
    } else if (r.mode === 'new') {
      box.textContent = c.text
    } else if (r.mode === 'cmp') {
      box.append(h('div', { class: 'mut', text: '原文' }), h('div', { text: r.src, style: 'opacity:.75;white-space:pre-wrap;margin-bottom:8px' }), h('div', { class: 'mut', text: '结果' }), h('div', { text: c.text, style: 'white-space:pre-wrap' }))
    } else {
      for (const p of r.parts) {
        if (p.eq != null) box.append(p.eq)
        else {
          const off = r.rejected.has(p.id)
          const span = h('span', { class: `hk${off ? ' off' : ''}`, title: off ? '点击改为采用' : '点击放弃这处改动', onclick: () => ((off ? r.rejected.delete(p.id) : r.rejected.add(p.id)), render()) })
          if (p.del) span.append(h('span', { class: 'del', text: p.del }))
          if (p.add) span.append(h('span', { class: 'add', text: p.add }))
          box.append(span)
        }
      }
    }
    bd.append(box)
    if (!isCont && r.mode === 'diff') {
      const n = r.parts.filter((p) => p.id != null).length
      bd.append(h('div', { class: 'mut', text: n ? `共 ${n} 处改动，点击某一处可单独放弃或重新采用。` : '没有改动。' }))
    }
    const readonly = st.snap.readonly
    const acts = h('div', { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' })
    if (!readonly) {
      if (isCont) acts.append(h('button', { text: '插入到光标处', onclick: () => apply('insert') }))
      else acts.append(h('button', { text: st.scope === 'sel' ? '替换选中' : '替换原文', onclick: () => apply('replace') }), h('button', { class: 'sec', text: '插入到光标处', onclick: () => apply('insert') }))
    }
    acts.append(h('button', { class: 'sec', text: '复制', onclick: (e) => copy(finalText(), e.target) }), h('button', { class: 'sec', style: 'margin-left:auto', text: '再来一个', onclick: () => run() }), h('button', { class: 'sec', text: '返回', onclick: () => ((st.view = 'options'), render()) }))
    bd.append(acts)
    bd.append(h('div', { class: 'mut', text: readonly ? '这里不是输入框，只能复制结果。' : '替换后可用 Ctrl+Z 撤销。结果自动保存到“历史记录”。' }))
    kids.push(bd)
    return kids
  }

  async function copy(text, btn) {
    try {
      await navigator.clipboard.writeText(text)
      btn.textContent = '已复制'
    } catch {
      btn.textContent = '复制失败，请手动选择'
    }
    remember(text)
  }

  async function remember(out) {
    // 面板可能马上关掉，所以先把要记的东西取出来再异步保存
    const rec = { src: String(st?.res?.src || '').slice(0, 2000), out: String(out).slice(0, 4000), action: st?.sel?.action || '', host: location.hostname, ts: Date.now(), fav: false }
    try {
      const { writeHistory } = await api.storage.local.get('writeHistory')
      await api.storage.local.set({ writeHistory: W.pushHistory(writeHistory, rec) })
    } catch {
      // 忽略
    }
  }

  // ---- 写回
  function apply(mode) {
    const r = st.res
    const text = finalText()
    const where = mode === 'insert' || st.sel.action === 'continue' ? 'insert' : st.scope === 'sel' ? 'sel' : 'all'
    if (!writeBack(st.snap, text, where)) {
      st.note = '写不进这个输入框，已为你复制到剪贴板，请手动粘贴。'
      navigator.clipboard?.writeText(text).catch(() => {})
      st.view = 'options'
      render()
      return
    }
    remember(text)
    closePanel()
    if (target) showBadge()
    void r
  }

  /** 写进输入框。用 execCommand('insertText')，这样网页自己的撤销栈和输入事件都正常；不行再直接改值并补发事件。 */
  function writeBack(snap, text, where) {
    const el = snap.el
    if (!el || !el.isConnected) return false
    try {
      el.focus({ preventScroll: true })
      if (isField(el)) {
        const len = el.value.length
        let s = snap.start
        let e = snap.end
        if (where === 'all') {
          s = 0
          e = len
        } else if (where === 'insert') {
          s = e
        } else if (s === e) {
          return false
        }
        el.setSelectionRange(s, e)
        let ok = false
        try {
          ok = document.execCommand('insertText', false, text)
        } catch {
          ok = false
        }
        if (!ok || el.value.length !== len - (e - s) + text.length) {
          const proto = Object.getPrototypeOf(el)
          const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set
          const nv = el.value.slice(0, s) + text + el.value.slice(e)
          if (setter) setter.call(el, nv)
          else el.value = nv
          el.setSelectionRange(s + text.length, s + text.length)
          el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }))
        }
        return true
      }
      const sel = getSelection()
      if (!sel) return false
      if (where === 'all') {
        sel.selectAllChildren(el)
      } else if (snap.range) {
        sel.removeAllRanges()
        const rg = snap.range.cloneRange()
        if (where === 'insert') rg.collapse(false)
        sel.addRange(rg)
      } else if (where === 'sel') {
        return false
      } else {
        sel.selectAllChildren(el)
        sel.collapseToEnd()
      }
      let ok = false
      try {
        ok = document.execCommand('insertText', false, text)
      } catch {
        ok = false
      }
      if (!ok) {
        const rg = sel.getRangeAt(0)
        rg.deleteContents()
        rg.insertNode(document.createTextNode(text))
        rg.collapse(false)
        el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }))
      }
      return true
    } catch {
      return false
    }
  }

  // ---- 语法检查
  function viewIssues() {
    const r = st.res
    const text = scopeText()
    const kids = [header('语法检查', h('span', { class: 'mut', text: r.issues.length ? `发现 ${r.issues.length} 处` : '没有发现问题' }))]
    const bd = h('div', { class: 'bd' })
    if (!r.issues.length) {
      bd.append(h('div', { text: '没有发现需要修改的地方。' }), h('div', { class: 'acts' }, h('button', { class: 'sec', text: '返回', onclick: () => ((st.view = 'options'), render()) })))
      kids.push(bd)
      return kids
    }
    const chars = Array.from(text)
    const box = h('div', { class: 'box' })
    let pos = 0
    r.issues.forEach((it, i) => {
      box.append(chars.slice(pos, it.start).join(''))
      const cls = it.type === 'awkward' ? 'e2' : it.type === 'style' ? 'e3' : 'e1'
      box.append(h('span', { class: r.skipped.has(i) ? '' : cls, text: chars.slice(it.start, it.end).join(''), title: it.reason }))
      pos = it.end
    })
    box.append(chars.slice(pos).join(''))
    bd.append(box)
    const legend = h('div', { class: 'acts mut' })
    legend.append(h('span', { class: 'e1', text: '错别字/语法' }), h('span', { class: 'e2', text: '不通顺' }), h('span', { class: 'e3', text: '风格建议' }))
    bd.append(legend)
    r.issues.forEach((it, i) => {
      const skip = r.skipped.has(i)
      bd.append(
        h(
          'div',
          { class: `card${skip ? ' skip' : ''}` },
          h('div', { class: 'acts' },
            h('span', { class: 'grow' }, h('span', { class: 'del', text: it.from }), ' → ', h('b', { text: it.to })),
            chip(skip ? '忽略' : '采用', !skip, () => ((skip ? r.skipped.delete(i) : r.skipped.add(i)), render()), 'x'),
          ),
          h('div', { class: 'mut', text: it.reason || '' }),
        ),
      )
    })
    const readonly = st.snap.readonly
    const acts = h('div', { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' })
    const fixed = () => W.applyIssues(text, r.issues, r.skipped)
    if (!readonly) acts.append(h('button', { text: '应用所选建议', onclick: () => {
      st.res = { src: text, cands: [{ text: fixed() }], idx: 0, mode: 'new', rejected: new Set(), parts: [] }
      apply('replace')
    } }))
    acts.append(h('button', { class: 'sec', text: '复制修改后的文字', onclick: (e) => copy(fixed(), e.target) }), h('button', { class: 'sec', style: 'margin-left:auto', text: '返回', onclick: () => ((st.view = 'options'), render()) }))
    bd.append(acts)
    kids.push(bd)
    return kids
  }

  // ---- 统计数据
  function viewFacts() {
    const f = st.facts
    const kids = [header('加入统计数据', h('span', { class: 'mut', text: '来自开放数据库，不凭空编造' }))]
    const bd = h('div', { class: 'bd' })
    if (!f.list.length) {
      bd.append(h('div', { class: 'warn', text: f.note || '没有找到可用的数据。' }))
    } else {
      bd.append(h('div', { class: 'mut', text: '勾选要写进文中的数据。数字和出处原样使用，不会被改动。' }))
      f.list.forEach((x, i) => {
        const cb = h('input', { type: 'checkbox', checked: f.picked.has(i), onchange: (e) => (e.target.checked ? f.picked.add(i) : f.picked.delete(i)) })
        bd.append(
          h('label', { class: 'card', style: 'cursor:pointer' },
            h('div', { class: 'acts' }, cb, h('span', { class: 'grow', text: x.text })),
            h('div', { class: 'mut' }, `来源：${x.source} · 指标代码 ${x.code} · `, h('a', { href: x.url, target: '_blank', rel: 'noopener noreferrer', text: '查看原始数据' })),
          ),
        )
      })
      if (f.note) bd.append(h('div', { class: 'warn', text: f.note }))
    }
    const acts = h('div', { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' })
    if (f.list.length) acts.append(h('button', { text: '写入文中', onclick: () => {
      const facts = [...f.picked].sort((a, b) => a - b).map((i) => f.list[i])
      if (!facts.length) return
      run({ facts })
    } }))
    acts.append(h('button', { class: 'sec', style: 'margin-left:auto', text: '返回', onclick: () => ((st.view = 'options'), render()) }))
    bd.append(acts)
    kids.push(bd)
    return kids
  }

  // ---- 历史
  async function showHistory() {
    try {
      const { writeHistory } = await api.storage.local.get('writeHistory')
      st.history = asArray(writeHistory)
    } catch {
      st.history = []
    }
    st.view = 'history'
    render()
  }

  function viewHistory() {
    const kids = [header('历史记录', h('span', { class: 'mut', text: '只保存在这台电脑的浏览器里' }))]
    const bd = h('div', { class: 'bd' })
    if (!st.history.length) bd.append(h('div', { class: 'mut', text: '还没有记录。采用或复制结果后会出现在这里。' }))
    st.history.slice(0, 20).forEach((x, i) => {
      bd.append(
        h('div', { class: 'card' },
          h('div', { class: 'mut', text: `${new Date(x.ts).toLocaleString()} · ${x.host || ''}${x.fav ? ' · ★' : ''}` }),
          h('div', { text: String(x.out).slice(0, 160) }),
          h('div', { class: 'acts' },
            h('button', { class: 'sec chip x', text: '复制', onclick: (e) => copy(x.out, e.target) }),
            h('button', { class: 'sec chip x', text: x.fav ? '取消收藏' : '收藏', onclick: async () => {
              x.fav = !x.fav
              await api.storage.local.set({ writeHistory: st.history }).catch(() => {})
              render()
            } }),
            h('button', { class: 'sec chip x', text: '删除', onclick: async () => {
              st.history.splice(i, 1)
              await api.storage.local.set({ writeHistory: st.history }).catch(() => {})
              render()
            } }),
          ),
        ),
      )
    })
    bd.append(h('div', { class: 'acts' },
      h('button', { class: 'sec', text: '清空（保留收藏）', onclick: async () => {
        st.history = st.history.filter((x) => x.fav)
        await api.storage.local.set({ writeHistory: st.history }).catch(() => {})
        render()
      } }),
      h('button', { class: 'sec', style: 'margin-left:auto', text: '返回', onclick: () => ((st.view = 'options'), render()) }),
    ))
    kids.push(bd)
    return kids
  }

  const VIEWS = { options: viewOptions, running: viewRunning, result: viewResult, issues: viewIssues, facts: viewFacts, history: viewHistory }
})()
