// 实时音效：给页面里的 <video> / <audio> 接一条 Web Audio 处理链，播放时统一音量。
//   声音 → 均衡器 → 混响 → [响度测量] → 增益 → 压缩 → 限幅 → 音量放大 → 输出
// 默认只开“响度标准化 + 限幅”；均衡器和混响默认关。每一环都可以绕过，全部关掉就等于没接这条链。
// 接进 Web Audio 之后没法撤回，所以跨域直链（没有 CORS）和受版权保护的视频会先判断、直接跳过并提示，不改变它的声音。
;(() => {
  if (globalThis.__ccAudioFx) return
  globalThis.__ccAudioFx = true
  const A = globalThis.ccAudio
  const { api, mount, h, listed } = globalThis.ccUI
  const host = location.hostname.toLowerCase()

  let cfg = A.normalizeCfg()
  let featOn = true
  let siteOff = false
  let tmpOff = false // 本页临时关闭（刷新后恢复）
  let ctx = null
  let notified = false
  let meterWanted = 0 // 面板 / 弹窗打开时 >0：即使没开响度标准化也测量，给仪表用

  const chains = new Map() // 媒体元素 → 处理链
  const blocked = new Map() // 媒体元素 → { code, text }
  const pending = new Set()
  const boosts = new WeakMap() // 音量放大（倍速面板里的“音量增益”），1 = 原音量

  const anyFx = () => cfg.on || cfg.reverb.on || cfg.eq.on
  const wanted = () => featOn && !siteOff && anyFx()
  const bypassing = () => !wanted() || tmpOff

  // ---------- 设置 ----------

  async function load() {
    try {
      const r = await api.storage.local.get(['features', 'audioCfg', 'siteRules'])
      featOn = !(r.features && typeof r.features === 'object' && r.features.audioFx === false)
      cfg = A.normalizeCfg(r.audioCfg)
      siteOff = listed(r.siteRules?.off?.audio, host)
    } catch {
      return
    }
    applyAll()
  }
  try {
    api.storage.onChanged.addListener((ch, area) => area === 'local' && (ch.audioCfg || ch.features || ch.siteRules) && load())
  } catch {
    // 扩展已失效
  }

  async function saveCfg(next) {
    cfg = A.normalizeCfg(next)
    applyAll()
    try {
      await api.storage.local.set({ audioCfg: cfg })
    } catch {
      // 扩展刚更新，本页先用着
    }
  }

  async function setSiteOff(off) {
    try {
      const { siteRules } = await api.storage.local.get('siteRules')
      const r = siteRules && typeof siteRules === 'object' ? siteRules : {}
      const list = new Set(Array.isArray(r.off?.audio) ? r.off.audio : [])
      if (off) list.add(host)
      else for (const s of [...list]) if (host === s || host.endsWith('.' + s)) list.delete(s)
      r.off = { ...(r.off || {}), audio: [...list] }
      await api.storage.local.set({ siteRules: r })
    } catch {
      // 忽略
    }
  }

  // ---------- 处理链 ----------

  function getCtx() {
    if (!ctx || ctx.state === 'closed') {
      const AC = globalThis.AudioContext || globalThis.webkitAudioContext
      ctx = new AC({ latencyHint: 'playback' })
    }
    return ctx
  }

  const wire = (c) => c.wire({ cfg, bypass: bypassing(), measure: meterWanted > 0 })

  // ---------- 接入 ----------

  const audible = (el) => !el.paused && !el.ended && !el.muted && el.volume > 0

  async function attach(el, force = false) {
    if (chains.has(el)) return chains.get(el)
    if (blocked.has(el) || pending.has(el)) return null
    const v = A.unprocessable({ src: el.currentSrc || el.src, pageOrigin: location.origin, crossOrigin: el.crossOrigin, hasMediaKeys: !!el.mediaKeys })
    if (v) {
      blocked.set(el, v)
      announce(el, v)
      return null
    }
    pending.add(el)
    try {
      const ac = getCtx()
      // 没有用户操作过的页面里 AudioContext 起不来；这时接进去会变成没有声音，所以先确认在运行再接
      await Promise.race([ac.resume().catch(() => {}), new Promise((r) => setTimeout(r, 300))])
      if (ac.state !== 'running') {
        waitGesture()
        return null
      }
      const c = new A.Chain(ac, el, boosts.get(el) || 1)
      chains.set(el, c)
      wire(c)
      return c
    } catch {
      blocked.set(el, { code: 'error', text: '这个视频不能处理，已按原样播放。' })
      return null
    } finally {
      pending.delete(el)
      if (force) refreshPill()
    }
  }

  let gestureWait = false
  function waitGesture() {
    if (gestureWait) return
    gestureWait = true
    const go = () => {
      gestureWait = false
      for (const t of ['pointerdown', 'keydown', 'touchend']) removeEventListener(t, go, true)
      scan()
    }
    for (const t of ['pointerdown', 'keydown', 'touchend']) addEventListener(t, go, { capture: true, passive: true })
  }

  function consider(el) {
    if (!(el instanceof HTMLMediaElement) || !wanted()) return
    const c = chains.get(el)
    if (c) {
      if (c.ac.state === 'suspended') c.ac.resume().catch(() => {})
      return
    }
    if (audible(el)) attach(el)
  }

  function deepAll(sel, root = document, out = [], budget = { n: 4000 }) {
    out.push(...root.querySelectorAll(sel))
    for (const el of root.querySelectorAll('*')) {
      if (--budget.n < 0) break
      if (el.shadowRoot) deepAll(sel, el.shadowRoot, out, budget)
    }
    return out
  }
  const scan = () => deepAll('video,audio').forEach(consider)

  for (const ev of ['playing', 'volumechange']) document.addEventListener(ev, (e) => consider(e.target), true)
  for (const ev of ['emptied', 'loadstart']) document.addEventListener(ev, (e) => chains.get(e.target)?.reset(), true)
  document.addEventListener('playing', (e) => {
    const c = chains.get(e.target)
    if (c) c.last = performance.now()
  }, true)

  function applyAll() {
    for (const [el, c] of chains) {
      if (!el.isConnected) {
        chains.delete(el)
        continue
      }
      wire(c)
    }
    scan()
    refreshPill()
  }

  // ---------- 测量与增益控制 ----------

  function tick() {
    if (document.visibilityState === 'hidden') return
    const now = performance.now()
    for (const [el, c] of chains) {
      if (!el.isConnected) {
        chains.delete(el)
        continue
      }
      if (el.paused || el.ended) continue
      c.tick(now)
    }
  }
  setInterval(tick, 100)

  // ---------- 音量放大（倍速面板里的“音量增益”共用这条链） ----------

  const engine = {
    /** 把音量放大到 pct%（100–400）。返回 false = 这个视频不能处理。 */
    setBoost(el, pct) {
      const v = Math.min(4, Math.max(1, Number(pct) / 100 || 1))
      boosts.set(el, v)
      const c = chains.get(el)
      if (c) {
        c.setBoost(v)
        return true
      }
      if (v <= 1) return true
      if (blocked.has(el) || A.unprocessable({ src: el.currentSrc || el.src, pageOrigin: location.origin, crossOrigin: el.crossOrigin, hasMediaKeys: !!el.mediaKeys })) return false
      attach(el, true).then((nc) => nc?.setBoost(boosts.get(el) || 1))
      return true
    },
    getBoost: (el) => boosts.get(el) || 1,
  }
  globalThis.ccAudioEngine = engine

  // ---------- 弹窗来取状态 ----------

  function stat() {
    const media = deepAll('video,audio')
    if (!media.length) return null
    let active = 0
    let blockedN = 0
    let best = null
    for (const el of media) {
      if (blocked.has(el)) blockedN++
      const c = chains.get(el)
      if (c && !bypassing()) {
        active++
        if (!el.paused && (!best || c.lufs != null)) best = c
      }
    }
    return {
      count: media.length, active, blocked: blockedN, tmpOff, siteOff, host,
      reasons: [...new Set([...blocked.values()].map((v) => v.code))],
      lufs: best?.lufs ?? null, gain: best ? best.curDb : null,
    }
  }
  try {
    api.runtime.onMessage.addListener((m, _s, reply) => {
      if (!m || m.type !== 'cc-audio') return
      if (m.action === 'stat') {
        const s = stat()
        if (s) reply({ ok: true, ...s })
        return
      }
      if (m.action === 'meter') {
        meterWanted = m.on ? 1 : 0
        for (const c of chains.values()) wire(c)
        return
      }
      if (m.action === 'tmp') {
        tmpOff = !!m.off
        applyAll()
        reply({ ok: true, tmpOff })
      }
    })
  } catch {
    // 扩展已失效
  }

  // ---------- 视频胶囊里的状态和快捷面板 ----------

  const CSS = `
    .apnl { width: 232px; background: #10201af2; color: #fff; border: 1px solid #ffffff29; border-radius: 10px; padding: 10px; display: flex; flex-direction: column; gap: 8px; font-size: 12px; box-shadow: 0 8px 24px #0006; }
    .apnl .r { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
    .apnl .ch { display: flex; flex-wrap: wrap; gap: 5px; }
    .apnl .c { background: transparent; border: 1px solid #ffffff40; color: #fff; border-radius: 12px; padding: 1px 9px; font-size: 11px; font-weight: 400; }
    .apnl .c.on { background: #e8903f40; border-color: #e8903f; color: #ffc590; }
    .apnl .sw { width: 32px; height: 18px; border-radius: 9px; background: #ffffff40; padding: 0; position: relative; }
    .apnl .sw::after { content: ""; position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: #fff; transition: left .15s; }
    .apnl .sw.on { background: #e8903f; } .apnl .sw.on::after { left: 16px; }
    .apnl .lk { background: none; border: 0; color: #ffb27a; padding: 0; font-size: 11.5px; font-weight: 400; text-align: left; }
    .apnl .m { color: #c3cdc8; font-size: 11px; font-variant-numeric: tabular-nums; }
    .atoast { max-width: 300px; background: #10201af2; color: #fff; border: 1px solid #ffffff29; border-radius: 8px; padding: 8px 10px; font-size: 12px; display: flex; flex-direction: column; gap: 6px; box-shadow: 0 6px 20px #0005; }
  `
  let ui = null
  let panel = null
  let hovered = null
  let meterTimer = 0
  const Dock = globalThis.ccVDock

  function ensureUi() {
    if (ui) return
    ui = mount(CSS)
  }

  function pillState() {
    if (!featOn) return null
    const el = hovered
    if (!el) return null
    if (blocked.has(el)) return { cls: 'w', text: '音效 未处理' }
    if (siteOff || tmpOff || !anyFx()) return { cls: 'o', text: '音效 关' }
    const c = chains.get(el)
    return { cls: '', text: c && cfg.on ? `音效 ${cfg.target} LUFS` : '音效' }
  }

  // 挂进视频胶囊（vdock.js）：设置里开了“在视频上显示音效状态”时，胶囊上显示状态点和响度；菜单里一项“实时音效…”
  Dock.register('audio', {
    order: 30,
    title: '音效',
    applies: () => featOn,
    // 默认不在视频上显示音效状态：这时只在别的功能让胶囊出现时，菜单里多一项“实时音效…”
    passive: () => !cfg.pillShow && !panel,
    onHover: (v) => {
      if (hovered !== v) closePanel()
      hovered = v
    },
    inline: (v) => {
      hovered = v
      const st = cfg.pillShow ? pillState() : null
      if (!st) return null
      return [Dock.parts.dot(st.cls), Dock.parts.label(st.text, () => togglePanel(), '实时音效：点开设置')]
    },
    menu: () => [{ label: '实时音效…', hint: 'Alt+Shift+A', title: '响度标准化、混响、临时关闭', onclick: (v) => ((hovered = v), togglePanel()) }],
    sticky: () => !!panel,
  })
  const refreshPill = () => Dock.refresh()

  function place() {
    if (!panel) return
    const p = Dock.above(240, hovered)
    panel.style.left = `${p.left}px`
    panel.style.top = 'auto'
    panel.style.bottom = `${p.bottom}px`
    panel.style.maxHeight = `${p.maxHeight}px`
    panel.style.overflowY = 'auto'
  }

  // Alt+Shift+A：用快捷键打开同一个音效面板（胶囊上不显示音效状态时也能用）
  document.addEventListener('keydown', (e) => {
    if (!featOn || !e.altKey || !e.shiftKey || e.code !== 'KeyA' || e.ctrlKey || e.metaKey) return
    const vids = [...document.querySelectorAll('video')].filter((v) => v.getBoundingClientRect().width >= 240)
    const v = Dock.current() || hovered || vids.find((x) => !x.paused) || vids[0]
    if (!v) return
    e.preventDefault()
    hovered = v
    togglePanel()
    Dock.show(v)
    place()
  }, true)
  addEventListener('scroll', () => panel && place(), { capture: true, passive: true })
  addEventListener('resize', () => panel && place())
  document.addEventListener('keydown', (e) => e.key === 'Escape' && panel && closePanel(), true)
  document.addEventListener('mousedown', (e) => panel && ui && !e.composedPath().includes(ui.host) && !Dock.owns(e) && closePanel(), true)

  const REV_QUICK = [['', '关'], ['room', '房间'], ['hall', '大厅']]

  function closePanel() {
    panel?.remove()
    panel = null
    clearInterval(meterTimer)
    if (meterWanted === 2) {
      meterWanted = 0
      for (const c of chains.values()) wire(c)
    }
  }

  function togglePanel() {
    if (panel) return closePanel()
    ensureUi()
    if (!meterWanted) meterWanted = 2 // 面板开着就一直测量
    for (const c of chains.values()) wire(c)
    panel = h('div', { class: 'layer apnl' })
    ui.add(panel)
    renderPanel()
    meterTimer = setInterval(() => panel && updateMeter(), 500)
  }

  function updateMeter() {
    const m = panel?.querySelector('.m')
    if (!m) return
    const c = hovered && chains.get(hovered)
    if (!c || bypassing()) return void (m.textContent = hovered && blocked.has(hovered) ? blocked.get(hovered).text : '本视频未处理')
    const out = c.lufs == null ? null : c.lufs + c.curDb
    m.textContent = `输入 ${A.fmtLufs(c.lufs)} → 输出 ${A.fmtLufs(out)} LUFS · 增益 ${A.fmtDb(c.curDb)}`
  }

  const BTN = (text, cls, fn) => h('button', { class: cls, text, onclick: fn })

  function renderPanel() {
    if (!panel) return
    const sw = h('button', { class: `sw${cfg.on ? ' on' : ''}`, role: 'switch', 'aria-checked': String(cfg.on), 'aria-label': '响度标准化', onclick: () => saveCfg({ ...cfg, on: !cfg.on }).then(renderPanel) })
    const presets = h('div', { class: 'ch' }, ...A.LOUD_PRESETS.map((p) => BTN(p.name, `c${cfg.preset === p.id ? ' on' : ''}`, () => { saveCfg(A.withPreset(cfg, p.id)); renderPanel() })))
    const rv = h('div', { class: 'ch' }, ...REV_QUICK.map(([id, name]) => {
      const on = id ? cfg.reverb.on && cfg.reverb.id === id : !cfg.reverb.on
      return BTN(name, `c${on ? ' on' : ''}`, () => { saveCfg({ ...cfg, reverb: { ...cfg.reverb, on: !!id, id: id || cfg.reverb.id } }); renderPanel() })
    }))
    panel.replaceChildren(
      h('div', { class: 'r' }, h('b', { text: '响度标准化' }), sw),
      presets,
      h('div', { class: 'r' }, h('span', { text: '混响' }), rv),
      h('div', { class: 'm', text: '' }),
      h('div', { class: 'r' },
        BTN(tmpOff ? '恢复音效' : '临时关闭本页', 'lk', () => { tmpOff = !tmpOff; applyAll(); renderPanel() }),
        BTN(siteOff ? '本站已关闭 · 恢复' : '本站关闭', 'lk', () => setSiteOff(!siteOff)),
      ),
      BTN('更多音效设置…', 'lk', () => globalThis.ccUI.send({ type: 'cc-open-audio' })),
    )
    updateMeter()
    place()
  }

  // ---------- “无法处理”的提示 ----------

  function announce(el, v) {
    refreshPill()
    if (notified || cfg.notifyOff || !wanted()) return
    notified = true
    ensureUi()
    const r = el.getBoundingClientRect()
    const t = h('div', { class: 'layer atoast', style: `left:${Math.max(8, r.left + 12)}px;top:${Math.max(8, r.top + 12)}px` },
      h('div', {}, h('b', { text: '这个视频无法统一音量' }), h('br'), h('span', { text: v.text })),
      h('div', { style: 'display:flex;gap:12px' },
        BTN('知道了', 'lk', () => t.remove()),
        BTN('不再提示', 'lk', () => { saveCfg({ ...cfg, notifyOff: true }); t.remove() })))
    ui.add(t)
    setTimeout(() => t.remove(), 9000)
  }

  load()
  setTimeout(scan, 1500)
})()
