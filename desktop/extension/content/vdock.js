// 视频胶囊：每个视频上只出现一个清影入口（右下角、控制栏上方），鼠标移开一起隐藏。
// 下载、播放增强（倍速、截图、画中画…）、实时音效、双语字幕层、视频摘要都把自己挂进来：
// - inline：胶囊上常驻的小控件（字幕开关、倍速、音效状态、下载）
// - menu：点 ⋮ 展开的菜单里的几项
// 各功能自己的设置面板仍由各自打开，用 anchor() 定位在胶囊上方。
// 以前这些是 4–5 个各自定位的小按钮，左上角、右下角两两叠在一起。
;(() => {
  if (globalThis.ccVDock) return
  const { mount, h, api, send } = globalThis.ccUI

  const MIN_W = 240
  const MIN_H = 135
  const CSS = `
    .dock { transition: opacity .2s; position: fixed; display: none; align-items: center; gap: 6px; height: 30px; padding: 0 3px 0 8px; border-radius: 15px; background: #1d2420d9; color: #fff; font-size: 12.5px; font-weight: 600; box-shadow: 0 2px 8px #0005; user-select: none; backdrop-filter: blur(6px) }
    .dock .mk { width: 16px; height: 16px; border-radius: 5px; background: #ffb547; color: #060a12; font-size: 10px; font-weight: 800; display: flex; align-items: center; justify-content: center; flex: none }
    .dock .grp { display: flex; align-items: center; gap: 4px }
    .dock .vd { width: 1px; height: 16px; background: #ffffff4d; flex: none }
    .dock .lb { cursor: pointer; max-width: 9em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .dock .tx { color: #dfe6e2; font-weight: 500; white-space: nowrap; font-variant-numeric: tabular-nums }
    .dock .b { height: 24px; min-width: 24px; padding: 0 6px; border-radius: 12px; border: 0; background: transparent; color: #fff; font-size: 12.5px; font-weight: 500; cursor: pointer; white-space: nowrap }
    .dock .b:hover, .dock .b.on { background: #ffffff38 }
    .dock .sw { width: 30px; height: 18px; border-radius: 9px; background: #ffffff59; position: relative; cursor: pointer; padding: 0; border: 0; flex: none }
    .dock .sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; transition: left .15s }
    .dock .sw.on { background: #ffb547 }
    .dock .sw.on::after { left: 14px }
    .dock .dot { width: 7px; height: 7px; border-radius: 50%; background: #52b57c; flex: none }
    .dock .dot.w { background: #d9a441 } .dock .dot.o { background: #8a948f }
    .dock .dots { width: 26px; height: 26px; border-radius: 13px; padding: 0; border: 0; background: transparent; display: flex; align-items: center; justify-content: center; cursor: pointer }
    .dock .dots:hover, .dock .dots.on { background: #ffffff38 }
    .dock .dots svg { width: 16px; height: 16px; fill: #fff }
    .dmenu { position: fixed; display: flex; flex-direction: column; min-width: 220px; max-width: 300px; padding: 6px; border-radius: 12px; border: 1px solid #1c2b43; background: #22292cf5; color: #e8f0fd; box-shadow: 0 10px 30px #0006; font-size: 13px }
    .dmenu .hd { color: #9aabc6; font-size: 11px; padding: 6px 8px 2px }
    .dmenu .it { display: flex; justify-content: space-between; gap: 12px; width: 100%; text-align: left; background: transparent; color: inherit; font-weight: 400; border-radius: 6px; padding: 6px 8px; border: 0; cursor: pointer }
    .dmenu .it:hover { background: #ffffff1f }
    .dmenu .it .k { color: #9aabc6; font-size: 11.5px }
    .dmenu .sep { height: 1px; background: #1c2b43; margin: 4px 2px }
  `

  const specs = new Map()
  let ui = null
  let cap = null
  let dotsBtn = null
  let menu = null
  let video = null
  let seen = 0
  let lastMove = 0
  let feats = {}

  const readFeats = () =>
    Promise.resolve(api.storage?.local?.get('features'))
      .then((r) => (feats = r?.features || {}))
      .catch(() => {})
  readFeats()
  try {
    api.storage.onChanged.addListener((ch, area) => area === 'local' && ch.features && readFeats())
  } catch {
    // 扩展已失效
  }

  function ensureUi() {
    if (ui) return
    ui = mount(CSS)
    const ns = 'http://www.w3.org/2000/svg'
    const icon = document.createElementNS(ns, 'svg')
    icon.setAttribute('viewBox', '0 0 16 16')
    for (const y of [3, 8, 13]) {
      const c = document.createElementNS(ns, 'circle')
      c.setAttribute('cx', '8')
      c.setAttribute('cy', String(y))
      c.setAttribute('r', '1.5')
      icon.append(c)
    }
    dotsBtn = h('button', { class: 'dots', title: '更多（下载、字幕、音效、播放工具、摘要）', 'aria-label': '更多', 'aria-haspopup': 'true', onclick: () => toggleMenu() }, icon)
    cap = h('div', { class: 'dock', role: 'toolbar', 'aria-label': '清影' })
    ui.add(cap)
  }

  const sorted = () => [...specs.values()].sort((a, b) => (a.order || 0) - (b.order || 0))
  const applies = (s, v) => {
    try {
      return !s.applies || !!s.applies(v)
    } catch {
      return false
    }
  }
  // passive：只往菜单里加东西、自己不让胶囊出现（比如音效没开“显示状态”时）
  const passive = (s) => {
    try {
      return typeof s.passive === 'function' ? !!s.passive() : !!s.passive
    } catch {
      return false
    }
  }
  const sticky = () => !!menu || sorted().some((s) => s.sticky?.())

  /** 胶囊里的内容：清 · 各功能的小控件 · ⋮ */
  function render() {
    if (!cap || !video) return
    const kids = [h('span', { class: 'mk', text: '清', title: '清影' })]
    for (const s of sorted()) {
      if (!applies(s, video) || !s.inline) continue
      let node = null
      try {
        node = s.inline(video)
      } catch {
        node = null
      }
      if (!node) continue
      kids.push(h('span', { class: 'vd' }), h('span', { class: 'grp' }, ...[].concat(node)))
    }
    kids.push(dotsBtn)
    cap.replaceChildren(...kids)
    dotsBtn.classList.toggle('on', !!menu)
    place()
  }

  /** 内置的两项：没打开的视频摘要、字幕层可以从这里打开。 */
  function builtins() {
    const out = []
    if (feats.vsum !== false && !globalThis.__ccVsum) out.push({ label: '视频摘要 / 总结', hint: 'Alt+Shift+S', onclick: () => send({ type: 'cc-vsum-open' }) })
    if (feats.subLayer !== false && !globalThis.ccSubLayerRun) out.push({ label: '双语字幕层', onclick: () => send({ type: 'cc-sublayer-open' }) })
    return out
  }

  async function toggleMenu(force) {
    const open = force ?? !menu
    menu?.remove()
    menu = null
    if (!open || !video) return render()
    const v = video
    const groups = []
    for (const s of sorted()) {
      if (!applies(s, v) || !s.menu) continue
      let items = []
      try {
        items = (await s.menu(v)) || []
      } catch {
        items = []
      }
      if (items.length) groups.push({ title: s.title || '', items })
    }
    const extra = builtins()
    if (extra.length) groups.push({ title: '打开', items: extra })
    if (video !== v) return
    const kids = []
    groups.forEach((g, i) => {
      if (i) kids.push(h('div', { class: 'sep' }))
      if (g.title) kids.push(h('div', { class: 'hd', text: g.title }))
      for (const it of g.items) {
        kids.push(
          h(
            'button',
            {
              class: 'it',
              title: it.title || '',
              onclick: (e) => {
                e.stopPropagation()
                toggleMenu(false)
                it.onclick?.(v)
              },
            },
            h('span', { text: it.label }),
            it.hint ? h('span', { class: 'k', text: it.hint }) : null,
          ),
        )
      }
    })
    if (!kids.length) kids.push(h('div', { class: 'hd', text: '这个视频上没有可用的功能' }))
    menu = h('div', { class: 'dmenu', role: 'menu' }, ...kids)
    ui.add(menu)
    render()
  }

  function place() {
    if (!cap) return
    const r = video?.isConnected ? video.getBoundingClientRect() : null
    if (!r || r.width < MIN_W || r.height < MIN_H || r.bottom < 40 || r.top > innerHeight - 40) {
      cap.style.display = 'none'
      if (menu) toggleMenu(false)
      return
    }
    cap.style.display = 'flex'
    const w = cap.offsetWidth || 120
    const ph = cap.offsetHeight || 30
    cap.style.left = `${Math.min(innerWidth - w - 4, Math.max(4, r.right - w - 12))}px`
    cap.style.top = `${Math.min(innerHeight - ph - 4, Math.max(4, r.bottom - ph - 62))}px`
    if (menu) {
      const a = cap.getBoundingClientRect()
      const mw = menu.offsetWidth || 240
      menu.style.left = `${Math.max(4, Math.min(innerWidth - mw - 4, a.right - mw))}px`
      menu.style.top = 'auto'
      menu.style.bottom = `${innerHeight - a.top + 6}px`
      menu.style.maxHeight = `${Math.max(120, a.top - 12)}px`
      menu.style.overflowY = 'auto'
    }
  }

  function setVisible(on) {
    if (!cap) return
    cap.style.opacity = on ? '1' : '0'
    cap.style.pointerEvents = on ? '' : 'none'
  }

  const big = (v) => {
    const r = v.getBoundingClientRect()
    return r.width >= MIN_W && r.height >= MIN_H
  }
  function videoAt(e) {
    for (const el of e.composedPath?.() || []) if (el.tagName === 'VIDEO') return el
    return document.elementsFromPoint(e.clientX, e.clientY).find((x) => x.tagName === 'VIDEO') || null
  }
  const inside = (b, x, y) => b && x >= b.left && x <= b.right && y >= b.top && y <= b.bottom

  /** 鼠标在哪个视频上：在就显示胶囊（有功能挂在这个视频上的话）；离开视频、胶囊、菜单 1.5 秒后隐藏。 */
  function show(v) {
    if (!v || !big(v) || !sorted().some((s) => applies(s, v) && !passive(s))) return false
    ensureUi()
    if (video !== v) {
      if (menu) toggleMenu(false)
      video = v
      for (const s of sorted()) if (applies(s, v)) s.onHover?.(v)
    }
    seen = Date.now()
    render()
    setVisible(true)
    return true
  }

  document.addEventListener(
    'mousemove',
    (e) => {
      globalThis.__ccPtr = { x: e.clientX, y: e.clientY, t: Date.now() }
      const now = performance.now()
      if (now - lastMove < 120) return
      lastMove = now
      const v = videoAt(e)
      if (v && (v !== video || cap?.style.opacity !== '1')) show(v)
      else if (v) seen = Date.now()
      else if (cap && (inside(cap.getBoundingClientRect(), e.clientX, e.clientY) || inside(menu?.getBoundingClientRect(), e.clientX, e.clientY))) seen = Date.now()
    },
    { capture: true, passive: true },
  )
  setInterval(() => {
    if (!cap || !video) return
    if (sticky()) seen = Date.now()
    if (Date.now() - seen > 1500) setVisible(false)
    place()
  }, 400)
  addEventListener('scroll', () => video && place(), { capture: true, passive: true })
  addEventListener('resize', () => video && place())
  document.addEventListener('keydown', (e) => e.key === 'Escape' && menu && toggleMenu(false), true)
  document.addEventListener('mousedown', (e) => menu && ui && !e.composedPath().includes(ui.host) && toggleMenu(false), true)

  // 给各功能用的小控件（样式在胶囊里）
  const parts = {
    label: (text, onclick, title) => h('span', { class: 'lb', text, title: title || '', onclick }),
    text: (text, title) => h('span', { class: 'tx', text, title: title || '' }),
    btn: (text, onclick, title) => h('button', { class: 'b', text, title: title || '', 'aria-label': title || text, onclick: (e) => (e.stopPropagation(), onclick(e)) }),
    sw: (on, onclick, title) => h('button', { class: `sw${on ? ' on' : ''}`, role: 'switch', 'aria-checked': String(!!on), title: title || '', 'aria-label': title || '', onclick: (e) => (e.stopPropagation(), onclick(e)) }),
    dot: (cls) => h('span', { class: `dot ${cls || ''}` }),
  }

  globalThis.ccVDock = {
    /**
     * 挂一项功能。spec：{ order, title, applies(video), onHover(video), inline(video) → 节点 / 节点数组,
     * menu(video) → [{ label, hint, title, onclick(video) }], sticky() → 面板开着时让胶囊别隐藏,
     * passive() → 真：只往菜单里加项，自己不让胶囊出现 }
     */
    register(id, spec) {
      specs.set(id, spec)
      if (video) render()
    },
    unregister(id) {
      specs.delete(id)
      if (!video) return
      if (!sorted().some((s) => applies(s, video) && !passive(s))) {
        if (cap) cap.style.display = 'none'
        video = null
      } else render()
    },
    /** 状态变了（倍速、开关）：重画胶囊上的小控件。 */
    refresh: () => render(),
    /** 胶囊现在在哪儿（各功能的面板放在它上方）；没显示时按视频右下角推算。 */
    anchor(v) {
      const vv = v || video
      if (cap && cap.style.display !== 'none' && (!v || v === video)) return cap.getBoundingClientRect()
      const r = vv?.getBoundingClientRect()
      if (!r) return { left: innerWidth - 160, right: innerWidth - 12, top: innerHeight - 100, bottom: innerHeight - 70, width: 148, height: 30 }
      const top = Math.min(innerHeight - 34, Math.max(4, r.bottom - 92))
      return { left: r.right - 160, right: r.right - 12, top, bottom: top + 30, width: 148, height: 30 }
    },
    /** 主动显示在某个视频上（快捷键打开面板时）。 */
    show: (v) => show(v),
    current: () => video,
    /** 事件是不是点在胶囊或它的菜单上（各功能“点外面关面板”时别把胶囊上的点击当成外面）。 */
    /** 截图时先把胶囊藏起来（不然会被截进去）。 */
    setHidden: (on) => ui && (ui.host.style.visibility = on ? 'hidden' : ''),
    owns: (e) => !!ui && e.composedPath().includes(ui.host),
    /** 面板放在胶囊上方、右对齐：返回 { left, bottom, maxHeight }。 */
    above(width, v) {
      const a = this.anchor(v)
      return { left: Math.max(4, Math.min(innerWidth - width - 4, a.right - width)), bottom: innerHeight - a.top + 6, maxHeight: Math.max(120, a.top - 12) }
    },
    parts,
  }
})()
