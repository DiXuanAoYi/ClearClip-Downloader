// 图片悬停放大与画廊：鼠标停在缩略图上约 0.2 秒，旁边出现原图；按住 Alt 锁定后可以发送、保存、以图搜图、旋转，或进入画廊。
;(() => {
  if (globalThis.__ccZoom) return
  globalThis.__ccZoom = true
  const { mount, h, send, hostOff, siteRules, onSettings, toast } = globalThis.ccUI
  const orig = globalThis.ccOriginal

  const DWELL = 220
  const CSS = `
    .zoom { background: #fff; border: 1px solid #cfd5d1; border-radius: 8px; box-shadow: 0 12px 36px #0006; overflow: hidden; pointer-events: none; }
    .zoom.locked { pointer-events: auto; }
    .zoom .stage { display: flex; align-items: center; justify-content: center; background: #111; overflow: hidden; }
    img.big { display: block; max-width: none; transition: transform .12s; }
    .info { background: #1d2420; color: #dfe5e2; font-size: 12px; padding: 3px 10px; display: flex; justify-content: space-between; gap: 10px; }
    .tools { display: none; gap: 6px; padding: 6px; background: #f4f5f4; flex-wrap: wrap; align-items: center; }
    .zoom.locked .tools, .gal .tools { display: flex; }
    .tools button { padding: 3px 10px; font-size: 12px; }
    .gal { inset: 0; background: #000d; display: flex; flex-direction: column; }
    .gal .stage { flex: 1; min-height: 0; background: transparent; position: relative; display: flex; align-items: center; justify-content: center; }
    .gal .tools { background: #1d2420; justify-content: center; }
    .gal .info { justify-content: center; gap: 24px; background: #1d2420; }
    .gal .nav { position: absolute; top: 50%; transform: translateY(-50%); font-size: 28px; padding: 8px 14px; background: #0008; }
    .gal .nav.l { left: 14px } .gal .nav.r { right: 14px }
    .strip { display: flex; gap: 6px; padding: 8px; overflow-x: auto; background: #1d2420; justify-content: center; }
    .strip img { height: 54px; width: auto; border: 2px solid transparent; border-radius: 4px; cursor: pointer; opacity: .7; }
    .strip img.on { border-color: #d9762b; opacity: 1; }
    .pop { background: #fff; color: #1d2420; border: 1px solid #cfd5d1; border-radius: 8px; box-shadow: 0 8px 24px #0004; padding: 4px; }
    .pop button { display: block; width: 100%; text-align: left; background: transparent; color: inherit; font-weight: 400; }
    .pop button:hover { background: #d9762b24; }
  `
  let off = false
  let userRules = ''
  let ui = null
  let view = null // 悬停放大：{ el, url, shown, w, h, rot, zoom, locked }
  let gal = null // 画廊：{ list, i }
  let timer = 0
  let token = 0
  let cx = 0
  let cy = 0
  const cache = new WeakMap()

  const refresh = async () => {
    off = await hostOff('hoverZoom')
    userRules = (await siteRules()).zoomRules || ''
    if (off) closeAll()
  }
  refresh()
  onSettings(refresh)

  // ---------- 找图片、取原图 ----------

  function targetFrom(path) {
    for (const el of path.slice(0, 6)) {
      if (el.tagName === 'IMG') return { el, shown: el.currentSrc || el.src }
      if (el.tagName === 'VIDEO' || el.tagName === 'CANVAS') return null
    }
    const el = path[0]
    if (el?.nodeType === 1) {
      const m = /url\((['"]?)(.*?)\1\)/.exec(getComputedStyle(el).backgroundImage || '')
      if (m && /^https?:/i.test(new URL(m[2], document.baseURI).href)) return { el, shown: new URL(m[2], document.baseURI).href, bg: true }
    }
    return null
  }

  const probe = (url) =>
    new Promise((ok) => {
      const img = new Image()
      const t = setTimeout(() => ok(null), 6000)
      img.onload = () => {
        clearTimeout(t)
        ok({ url, w: img.naturalWidth, h: img.naturalHeight })
      }
      img.onerror = () => {
        clearTimeout(t)
        ok(null)
      }
      img.src = url
    })

  /** 找到比显示出来的更大的那一张：依次试候选地址，最多 4 个。 */
  async function resolve(t, needLarger = true) {
    const r = t.el.getBoundingClientRect()
    const urls = (t.bg ? orig.candidates(t.shown, userRules) : orig.forElement(t.el, userRules)).filter((u) => u !== t.shown).slice(0, 4)
    for (const u of urls) {
      const p = await probe(u)
      if (p && (!needLarger || p.w > r.width * 1.15 || p.h > r.height * 1.15)) return { ...p, original: true }
    }
    const nw = t.el.naturalWidth || 0
    const nh = t.el.naturalHeight || 0
    if (!needLarger || nw > r.width * 1.15 || nh > r.height * 1.15) return nw ? { url: t.shown, w: nw, h: nh, original: false } : needLarger ? null : await probe(t.shown).then((p) => p && { ...p, original: false })
    return null
  }

  // ---------- 界面 ----------

  function ensureUi() {
    if (ui) return
    ui = mount(CSS)
  }

  const ext = (u) => (/\.(jpe?g|png|gif|webp|avif|bmp)(?:$|[?#])/i.exec(u)?.[1] || 'IMG').toUpperCase().replace('JPEG', 'JPG')

  /** 按当前图片生成一套浮层内容：图片、信息行、工具栏。 */
  function build(cur, boxSize) {
    const img = h('img', { class: 'big', src: cur.url, alt: '' })
    const fit = Math.min(1, boxSize.w / (cur.rot % 180 ? cur.h : cur.w), boxSize.h / (cur.rot % 180 ? cur.w : cur.h))
    const scale = fit * cur.zoom
    img.style.width = `${cur.w * scale}px`
    img.style.height = `${cur.h * scale}px`
    img.style.transform = `rotate(${cur.rot}deg)`
    const sw = (cur.rot % 180 ? cur.h : cur.w) * scale
    const sh = (cur.rot % 180 ? cur.w : cur.h) * scale
    const stage = h('div', { class: 'stage', style: gal ? '' : `width:${sw}px;height:${sh}px` }, img)
    const info = h('div', { class: 'info' }, h('span', { text: `${cur.w}×${cur.h} · ${ext(cur.url)} · ${cur.original ? '原图' : '页面图片'}` }), h('span', { text: gal ? `${gal.i + 1} / ${gal.list.length}` : '按住 Alt 锁定 · +/− 缩放 · R 旋转' }))
    return { stage, info, img, sw, sh }
  }

  function tools(cur) {
    const bar = h('div', { class: 'tools' })
    const act = async (fn) => {
      const r = await fn()
      if (r) toast(ui, r.msg, !r.ok)
    }
    bar.append(
      h('button', { text: '发送到清影', onclick: () => act(async () => {
        const r = await send({ type: 'cc-send-batch', items: [{ kind: 'image', url: cur.url }] })
        return { ok: r.ok, msg: r.ok ? (r.pairing ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送到清影') : r.error || '发送失败' }
      }) }),
      h('button', { class: 'sec', text: '保存', onclick: () => act(async () => {
        const r = await send({ type: 'cc-save', url: cur.url })
        return { ok: r.ok, msg: r.ok ? '已交给浏览器保存' : r.error || '保存失败' }
      }) }),
      h('button', { class: 'sec', text: '以图搜图 ▾', onclick: (e) => engineMenu(e.target, cur) }),
      h('button', { class: 'sec', text: '旋转', onclick: () => rotate() }),
      h('button', { class: 'sec', text: gal ? '关闭画廊' : '画廊', onclick: () => (gal ? closeGallery() : openGallery()) }),
    )
    return bar
  }

  async function engineMenu(anchor, cur) {
    const r = await send({ type: 'cc-engines' })
    const list = r.ok ? r.engines || [] : []
    const rect = anchor.getBoundingClientRect()
    const pop = h('div', { class: 'layer pop', style: `left:${rect.left}px;top:${rect.bottom + 4}px;z-index:2` })
    const close = () => pop.remove()
    for (const [i, e] of list.entries()) {
      pop.append(h('button', { text: e.name, onclick: async () => {
        close()
        const res = await send({ type: 'cc-search', url: cur.url, engine: i })
        if (!res.ok) toast(ui, res.error || '没有打开', true)
      } }))
    }
    pop.append(h('button', { text: '全部引擎各开一个标签', onclick: async () => {
      close()
      const res = await send({ type: 'cc-search', url: cur.url, engine: 'all' })
      if (!res.ok) toast(ui, res.error || '没有打开', true)
    } }))
    ui.add(pop)
    setTimeout(() => document.addEventListener('mousedown', close, { once: true, capture: true }), 0)
  }

  let layer = null

  function render() {
    ensureUi()
    const cur = gal ? gal.list[gal.i].cur : view
    if (!cur) return
    layer?.remove()
    if (gal) {
      const b = build(cur, { w: innerWidth - 160, h: innerHeight - 190 })
      const nav = (cls, text, d) => h('button', { class: `nav ${cls}`, text, onclick: () => step(d) })
      b.stage.append(nav('l', '‹', -1), nav('r', '›', 1))
      const strip = h('div', { class: 'strip' }, ...gal.list.map((it, i) => h('img', { class: i === gal.i ? 'on' : '', src: it.shown, onclick: () => goto(i) })))
      layer = h('div', { class: 'layer gal' }, tools(cur), b.stage, b.info, strip)
      layer.addEventListener('mousedown', (e) => e.target === layer && closeGallery())
      layer.querySelector('.strip img.on')?.scrollIntoView({ inline: 'center', block: 'nearest' })
    } else {
      const b = build(cur, { w: Math.min(innerWidth - 40, 1400), h: innerHeight - 40 })
      layer = h('div', { class: `layer zoom${cur.locked ? ' locked' : ''}` }, b.stage, b.info, tools(cur))
      ui.add(layer)
      position(b.sw, b.sh)
      return
    }
    ui.add(layer)
  }

  function position(sw, sh) {
    if (!layer || gal) return
    const w = sw + 2
    const total = sh + 26 + (view?.locked ? 40 : 0)
    let left = cx + 24
    if (left + w > innerWidth - 8) left = Math.max(8, cx - 24 - w)
    const top = Math.max(8, Math.min(cy - total / 2, innerHeight - total - 8))
    layer.style.left = `${left}px`
    layer.style.top = `${top}px`
  }

  function rotate() {
    const cur = gal ? gal.list[gal.i].cur : view
    if (!cur) return
    cur.rot = (cur.rot + 90) % 360
    render()
  }

  function zoomBy(f) {
    const cur = gal ? gal.list[gal.i].cur : view
    if (!cur) return
    cur.zoom = Math.max(0.2, Math.min(8, cur.zoom * f))
    render()
  }

  function closeAll() {
    clearTimeout(timer)
    token++
    view = null
    gal = null
    layer?.remove()
    layer = null
  }

  // ---------- 悬停放大 ----------

  async function start(t) {
    const my = ++token
    const r = t.el.getBoundingClientRect()
    if (r.width < 48 || r.height < 48) return
    const found = await resolve(t)
    if (my !== token || !found) return
    view = { el: t.el, shown: t.shown, ...found, rot: 0, zoom: 1, locked: false }
    cache.set(t.el, view)
    render()
  }

  document.addEventListener(
    'mouseover',
    (e) => {
      if (off || gal || view?.locked) return
      clearTimeout(timer)
      const t = targetFrom(e.composedPath())
      if (view && t && t.el === view.el) return
      if (view) {
        view = null
        layer?.remove()
        layer = null
      }
      token++
      if (t) timer = setTimeout(() => start(t), DWELL)
    },
    true,
  )
  document.addEventListener(
    'mousemove',
    (e) => {
      cx = e.clientX
      cy = e.clientY
      if (view && !view.locked && layer) {
        const st = layer.querySelector('.stage')
        position(parseFloat(st.style.width) || 200, parseFloat(st.style.height) || 200)
      }
    },
    { capture: true, passive: true },
  )
  document.addEventListener('mouseout', (e) => !e.relatedTarget && !gal && !view?.locked && closeAll(), true)
  addEventListener('scroll', () => !gal && !view?.locked && closeAll(), { capture: true, passive: true })
  document.addEventListener('mousedown', (e) => {
    if (view?.locked && !e.composedPath().includes(ui?.host)) closeAll()
  }, true)

  // ---------- 画廊 ----------

  function pageImages() {
    const out = []
    for (const el of document.images) {
      const r = el.getBoundingClientRect()
      if (r.width < 100 || r.height < 100 || !(el.currentSrc || el.src)) continue
      out.push({ el, shown: el.currentSrc || el.src })
    }
    return out
  }

  async function ensureCur(it) {
    if (it.cur) return it.cur
    const found = (await resolve({ el: it.el, shown: it.shown }, false)) || { url: it.shown, w: it.el.naturalWidth || 400, h: it.el.naturalHeight || 300, original: false }
    it.cur = { ...found, rot: 0, zoom: 1, locked: true }
    return it.cur
  }

  async function openGallery() {
    const list = pageImages()
    if (!list.length) return
    const start = view?.el ? Math.max(0, list.findIndex((x) => x.el === view.el)) : 0
    view = null
    gal = { list, i: start }
    token++
    await ensureCur(list[start])
    render()
    // 预取前后各一张
    for (const d of [1, -1]) if (list[start + d]) ensureCur(list[start + d])
  }

  async function goto(i) {
    if (!gal) return
    gal.i = (i + gal.list.length) % gal.list.length
    await ensureCur(gal.list[gal.i])
    render()
    for (const d of [1, -1]) if (gal.list[gal.i + d]) ensureCur(gal.list[gal.i + d])
  }

  const step = (d) => goto(gal.i + d)

  function closeGallery() {
    gal = null
    layer?.remove()
    layer = null
  }

  // ---------- 键盘 ----------

  const editable = (t) => t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))

  document.addEventListener(
    'keydown',
    (e) => {
      if (!view && !gal) return
      if (e.key === 'Alt' && view && !view.locked) {
        view.locked = true
        e.preventDefault()
        render()
        return
      }
      if (editable(e.target) || e.ctrlKey || e.metaKey) return
      const cur = gal ? gal.list[gal.i].cur : view
      let handled = true
      switch (e.key) {
        case 'Escape':
          gal ? closeGallery() : closeAll()
          break
        case '+':
        case '=':
          zoomBy(1.25)
          break
        case '-':
        case '_':
          zoomBy(0.8)
          break
        case 'r':
        case 'R':
          rotate()
          break
        case 's':
        case 'S':
          send({ type: 'cc-save', url: cur.url })
          toast(ui, '已交给浏览器保存')
          break
        case 'g':
        case 'G':
          gal ? closeGallery() : openGallery()
          break
        case 'ArrowLeft':
        case 'ArrowRight': {
          const d = e.key === 'ArrowLeft' ? -1 : 1
          if (gal) step(d)
          else {
            openGallery().then(() => gal && step(d))
          }
          break
        }
        default:
          handled = false
      }
      if (handled) {
        e.preventDefault()
        e.stopImmediatePropagation()
      }
    },
    true,
  )
  document.addEventListener('keyup', (e) => e.key === 'Alt' && view?.locked && e.preventDefault(), true)
})()
