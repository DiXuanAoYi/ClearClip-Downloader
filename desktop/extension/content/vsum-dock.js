// 视频摘要面板“收到右边”：收起时只剩右边缘一个小标签；点它展开，停在上面预览。
// 状态：open 展开（固定）/ hidden 收起 / peek 预览；切换规则在 vsum-logic.js 的 dockNext，这里只管界面和事件。
;(() => {
  const NS = 'http://www.w3.org/2000/svg'
  const HOVER_MS = 350 // 在标签上停多久才预览：路过右边缘不会弹
  const LEAVE_MS = 800 // 预览时鼠标移开多久才收回
  const TOAST_MS = 6000

  const svg = (d, w = 14, sw = 2.4) => {
    const s = document.createElementNS(NS, 'svg')
    s.setAttribute('width', String(w))
    s.setAttribute('height', String(w))
    s.setAttribute('viewBox', '0 0 24 24')
    s.setAttribute('fill', 'none')
    s.setAttribute('stroke', 'currentColor')
    s.setAttribute('stroke-width', String(sw))
    s.setAttribute('stroke-linecap', 'round')
    s.setAttribute('stroke-linejoin', 'round')
    s.setAttribute('aria-hidden', 'true')
    const p = document.createElementNS(NS, 'path')
    p.setAttribute('d', d)
    s.append(p)
    return s
  }
  /** 标题栏上的“收起”按钮图标：⇥ */
  const collapseIcon = () => svg('M8 6l6 6-6 6M19 5v14', 16, 2)

  const CSS = `
    .panel { transition: transform .18s ease; }
    .panel.hide { transform: translateX(105%); visibility: hidden; pointer-events: none; transition: transform .18s ease, visibility 0s .18s; }
    .panel.peek { border-left-color: #b45f00; box-shadow: -14px 0 36px #0003; padding-top: 34px; }
    .peekbar { top: 0; right: 0; width: 400px; max-width: 100vw; height: 34px; display: none; align-items: center; gap: 8px; padding: 0 10px 0 12px; background: var(--accsoft); border-left: 1px solid #b45f00; border-bottom: 1px solid #e3b475; color: var(--acc); font-size: 12px; z-index: 2147483647; }
    .peekbar.on { display: flex; }
    .peekbar i { width: 8px; height: 8px; border-radius: 50%; background: #b45f00; flex: none; }
    .peekbar span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .peekbar button { display: inline-flex; align-items: center; gap: 4px; height: 24px; padding: 0 10px; font-size: 12px; }
    .dockbtn { display: inline-flex; align-items: center; padding: 3px 7px; }
    .dtab { right: 0; width: 30px; height: 84px; padding: 0; border: 1px solid var(--line); border-right: 0; border-radius: 12px 0 0 12px; background: var(--bg); color: var(--mute); box-shadow: -3px 2px 10px #0002; opacity: .78; display: none; flex-direction: column; align-items: center; justify-content: center; gap: 7px; font-weight: 400; touch-action: none; cursor: pointer; transition: opacity .15s, width .15s, right .18s ease, border-color .15s; overflow: hidden; }
    .dtab.on { display: flex; }
    .dtab:hover { opacity: 1; filter: none; }
    .dtab:hover:not(.peek) { width: 136px; gap: 3px; border-color: #b45f00; }
    .dtab .rw { display: flex; flex-direction: column; align-items: center; gap: 7px; }
    .dtab:hover:not(.peek) .rw { flex-direction: row; gap: 7px; }
    .dtab .nm { display: none; color: var(--fg); font-weight: 600; white-space: nowrap; }
    .dtab:hover:not(.peek) .nm { display: block; }
    .dtab .kb { display: none; font-size: 11px; color: var(--mute); font-weight: 400; }
    .dtab:hover:not(.peek) .kb { display: block; }
    .dtab .lg { position: relative; width: 20px; height: 20px; border-radius: 6px; background: #b45f00; color: #fff; font-size: 12px; font-weight: 700; line-height: 20px; text-align: center; flex: none; }
    .dtab .lg .rg { position: absolute; left: -5px; top: -5px; display: none; animation: dspin 1s linear infinite; }
    .dtab.busy .lg .rg { display: block; }
    .dtab .lg .dt { position: absolute; right: -4px; top: -4px; width: 9px; height: 9px; border-radius: 50%; box-shadow: 0 0 0 2px var(--bg); display: none; }
    .dtab.fresh .lg .dt { display: block; background: #007a70; }
    .dtab.warn .lg .dt { display: block; background: #d99a1e; }
    .dtab .gp { display: flex; flex-direction: column; gap: 2px; }
    .dtab .gp i { display: block; width: 10px; height: 2px; border-radius: 1px; background: var(--line); }
    .dtab.peek { opacity: 1; border-color: #b45f00; background: var(--accsoft); color: var(--acc); }
    .dtab.peek .gp { display: none; }
    .dtab.drag { opacity: 1; cursor: grabbing; transition: none; }
    .dtoast { right: 38px; height: 30px; display: none; align-items: center; gap: 7px; padding: 0 12px; border-radius: 999px; background: var(--fg); color: var(--bg); font-size: 12px; white-space: nowrap; box-shadow: 0 4px 14px #0004; pointer-events: none; }
    .dtoast.on { display: flex; }
    .dtoast i { width: 8px; height: 8px; border-radius: 50%; background: #4fb87f; flex: none; }
    .dtoast.warn { background: var(--card); color: #7a4a05; border: 1px solid #e3c27a; }
    .dtoast.warn i { background: #d99a1e; }
    @keyframes dspin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .panel, .panel.hide, .dtab { transition: none; } .dtab .lg .rg { animation: none; } }
  `
  const DARK = `
      .panel.peek, .peekbar { border-left-color: #ffb547; }
      .peekbar { border-bottom-color: #5a4330; }
      .peekbar i { background: #ffb547; }
      .dtab .lg { background: #ffb547; color: #0b1424; }
      .dtab.fresh .lg .dt { background: #4be3d0; } .dtab.warn .lg .dt { background: #f0b73a; }
      .dtab:hover:not(.peek), .dtab.peek { border-color: #ffb547; }
      .dtoast.warn { background: #2f2a1c; color: #f2d58e; border-color: #6b5530; }
  `

  /**
   * 创建收起 / 预览的外壳。deps：{ ui, h, panel, V, store, S }。
   * 返回 { state, set(action), toggle(), update(), placeTab(), collapseButton(), destroy() }。
   */
  function create({ ui, h, panel, V, store, S }) {
    const dock = { state: 'open', fresh: false, warn: false }
    let hoverTimer = 0
    let leaveTimer = 0
    let toastTimer = 0
    let frac = 0.38 // 小标签中心在窗口高度的几分之几（所有网站共用，记在存储里）
    let wasBusy = false
    let lastKind = ''
    let warned = false

    const ring = (() => {
      const s = document.createElementNS(NS, 'svg')
      s.setAttribute('class', 'rg')
      s.setAttribute('width', '30')
      s.setAttribute('height', '30')
      s.setAttribute('viewBox', '0 0 30 30')
      s.setAttribute('fill', 'none')
      s.setAttribute('aria-hidden', 'true')
      const a = document.createElementNS(NS, 'circle')
      for (const [k, v] of [['cx', '15'], ['cy', '15'], ['r', '13'], ['stroke', '#e3b475'], ['stroke-width', '2']]) a.setAttribute(k, v)
      const b = document.createElementNS(NS, 'path')
      for (const [k, v] of [['d', 'M15 2a13 13 0 0 1 13 13'], ['stroke', '#b45f00'], ['stroke-width', '2.4'], ['stroke-linecap', 'round']]) b.setAttribute(k, v)
      s.append(a, b)
      return s
    })()
    const chevron = svg('M15 6l-6 6 6 6')
    const tab = h('button', { class: 'layer dtab', type: 'button', 'aria-label': '展开视频摘要', title: '视频摘要（Alt+Shift+S）。上下拖动可以换位置' },
      h('span', { class: 'rw' },
        h('span', { class: 'lg' }, '摘', ring, h('i', { class: 'dt' })),
        h('span', { class: 'nm', text: '视频摘要' }),
        chevron),
      h('span', { class: 'kb', text: 'Alt+Shift+S' }),
      h('span', { class: 'gp' }, h('i'), h('i')))
    const toast = h('div', { class: 'layer dtoast', role: 'status' }, h('i'), h('span'))
    const bar = h('div', { class: 'layer peekbar' }, h('i'), h('span', { text: '预览中 · 鼠标移开会自动收回去' }),
      h('button', { type: 'button', title: '固定在这里，不再自动收回', onclick: () => dock.set('pin') }, svg('M9 4h6l-1 6 3 3v2h-5v5l-1 1-1-1v-5H5v-2l3-3z', 12, 2), '固定'))
    ui.add(bar)
    ui.add(toast)
    ui.add(tab)

    const clear = () => {
      clearTimeout(hoverTimer)
      clearTimeout(leaveTimer)
    }

    /** 小标签的纵向位置：按记住的比例；和视频胶囊重叠时躲开。 */
    function placeTab() {
      const vh = innerHeight
      let top = V.dockTabTop(frac, vh)
      try {
        const a = globalThis.ccVDock?.anchor?.()
        if (a && a.right > innerWidth - 36 && a.bottom > top - 6 && a.top < top + 90) {
          const up = a.top - 90
          top = up >= 8 ? up : Math.min(vh - 92, a.bottom + 6)
        }
      } catch {
        /* 没有胶囊就不用躲 */
      }
      tab.style.top = `${top}px`
      toast.style.top = `${top + 27}px`
      tab.style.right = dock.state === 'peek' ? `min(400px, 100vw)` : '0'
      toast.style.right = dock.state === 'peek' ? 'calc(min(400px, 100vw) + 38px)' : '38px'
    }

    function sync() {
      const hidden = dock.state === 'hidden'
      const peek = dock.state === 'peek'
      panel.classList.toggle('hide', hidden)
      panel.classList.toggle('peek', peek)
      panel.inert = hidden
      panel.setAttribute('aria-hidden', hidden ? 'true' : 'false')
      bar.classList.toggle('on', peek)
      tab.classList.toggle('on', hidden || peek)
      tab.classList.toggle('peek', peek)
      tab.setAttribute('aria-label', peek ? '收起视频摘要' : '展开视频摘要')
      if (dock.state === 'open') {
        dock.fresh = false
        showToast('')
      }
      update()
      placeTab()
    }

    function showToast(text, warn = false) {
      clearTimeout(toastTimer)
      if (!text) return toast.classList.remove('on', 'warn')
      toast.querySelector('span').textContent = text
      toast.classList.toggle('warn', warn)
      toast.classList.add('on')
      placeTab()
      toastTimer = setTimeout(() => toast.classList.remove('on'), TOAST_MS)
    }

    /** 收起时标签上的状态：生成中 / 生成好了 / 要处理。 */
    function update() {
      const busy = !!S.job || !!S.busy
      if (S.job?.kind) lastKind = S.job.kind
      const away = dock.state !== 'open'
      if (wasBusy && !busy && away) {
        dock.fresh = true
        showToast(lastKind === 'transcribe' ? '转写好了 · 点开看' : lastKind === 'summary' ? '摘要生成好了 · 点开看' : '处理好了 · 点开看')
      }
      wasBusy = busy
      dock.warn = !!S.status && !S.status.ready
      if (away && dock.warn && !warned && !dock.fresh) {
        warned = true
        showToast('清影里还没设置大模型', true)
      }
      tab.classList.toggle('busy', busy && away)
      tab.classList.toggle('fresh', dock.fresh && away)
      tab.classList.toggle('warn', dock.warn && away && !dock.fresh)
    }

    dock.set = (action) => {
      const next = V.dockNext(dock.state, action)
      if (next === dock.state) return
      clear()
      dock.state = next
      sync()
    }
    dock.toggle = () => dock.set('toggle')

    // ---- 悬停预览 ----
    const wantPeek = () => S.prefs.peek !== false
    tab.addEventListener('pointerenter', () => {
      clearTimeout(leaveTimer)
      if (dock.state === 'hidden' && wantPeek() && !drag) {
        clearTimeout(hoverTimer)
        hoverTimer = setTimeout(() => dock.set('hover'), HOVER_MS)
      }
    })
    const leave = () => {
      clearTimeout(hoverTimer)
      if (dock.state === 'peek' && !drag) {
        clearTimeout(leaveTimer)
        leaveTimer = setTimeout(() => dock.set('leave'), LEAVE_MS)
      }
    }
    tab.addEventListener('pointerleave', leave)
    panel.addEventListener('pointerleave', leave)
    bar.addEventListener('pointerleave', leave)
    panel.addEventListener('pointerenter', () => clearTimeout(leaveTimer))
    bar.addEventListener('pointerenter', () => clearTimeout(leaveTimer))
    // 预览时在面板里点任何地方 = 固定
    panel.addEventListener('pointerdown', () => dock.state === 'peek' && dock.set('pin'), true)

    // ---- 点击 / 上下拖动 ----
    let drag = null
    tab.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return
      drag = { y0: e.clientY, moved: false, top0: tab.getBoundingClientRect().top }
      try {
        tab.setPointerCapture(e.pointerId)
      } catch {
        /* 个别浏览器不支持就算了，拖得快会丢 */
      }
    })
    tab.addEventListener('pointermove', (e) => {
      if (!drag) return
      if (!drag.moved && Math.abs(e.clientY - drag.y0) < 5) return
      drag.moved = true
      clear()
      tab.classList.add('drag')
      const top = Math.min(innerHeight - 92, Math.max(8, drag.top0 + e.clientY - drag.y0))
      frac = V.dockFrac(top + 42, innerHeight)
      placeTab()
    })
    const end = (e) => {
      if (!drag) return
      const moved = drag.moved
      drag = null
      tab.classList.remove('drag')
      try {
        tab.releasePointerCapture(e.pointerId)
      } catch {
        /* 没有捕获就算了 */
      }
      if (moved) {
        store.set('vsumTabFrac', frac)
        // 拖动结束后鼠标还在标签上：当作没碰过，不弹预览
        tab.dataset.dragged = '1'
        setTimeout(() => delete tab.dataset.dragged, 0)
      }
    }
    tab.addEventListener('pointerup', end)
    tab.addEventListener('pointercancel', end)
    tab.addEventListener('click', () => {
      if (tab.dataset.dragged) return
      dock.set('click')
    })
    addEventListener('resize', placeTab)

    store.get('vsumTabFrac', 0.38).then((f) => {
      if (typeof f === 'number' && f > 0 && f < 1) frac = f
      placeTab()
    })

    /** 标题栏上的“收起”按钮。 */
    dock.collapseButton = () => {
      const b = h('button', { class: 'sec dockbtn', type: 'button', 'aria-label': '收起到右边', title: '收起到右边（Alt+Shift+S）', onclick: () => dock.set('collapse') })
      b.append(collapseIcon())
      return b
    }
    dock.update = update
    dock.placeTab = placeTab
    dock.destroy = () => {
      clear()
      clearTimeout(toastTimer)
      removeEventListener('resize', placeTab)
    }
    sync()
    return dock
  }

  globalThis.ccVsumDock = { create, CSS, DARK }
})()
