// 播放增强：倍速与快捷键、记住每个网站的倍速、快进快退、逐帧、截图、画中画、A–B 循环、跳过片头片尾、音量增益、画面调整。
// 只在页面里真的有 <video> / <audio> 时才初始化；每个功能按设置单独开关。
;(() => {
  if (globalThis.__ccVideo) return
  globalThis.__ccVideo = true
  const L = globalThis.ccVideo
  const { api, mount, h, send, onSettings, listed } = globalThis.ccUI
  const host = location.hostname.toLowerCase()

  const DEF_FEAT = { speed: true, speedMemory: true, seek: true, videoTools: false, videoAdvanced: false }
  let feat = { ...DEF_FEAT }
  let cfg = L.normalizeCfg()
  let rates = {}
  let off = false
  let ui = null
  let bar = null
  let panel = null
  let hovered = null
  let lastGesture = 0
  let lastMove = 0
  let hideTimer = 0
  const known = new Set()
  const st = new WeakMap()
  const S = (el) => {
    let s = st.get(el)
    if (!s) st.set(el, (s = { desired: null, resets: [], introDone: false, outroDone: false, A: null, B: null, fx: { mirror: false, rotate: 0, brightness: 100, contrast: 100, saturate: 100, fill: false }, saved: null, gain: null }))
    return s
  }

  const CSS = `
    .vbar { display: flex; gap: 2px; align-items: center; background: #14181add; color: #fff; border-radius: 16px; padding: 3px 6px; box-shadow: 0 2px 10px #0006; font-size: 12px; user-select: none; }
    .vbar button { background: transparent; color: #fff; padding: 2px 7px; border-radius: 10px; font-size: 12px; font-weight: 500; line-height: 18px; }
    .vbar button:hover { background: #ffffff33; }
    .vbar .rate { font-weight: 700; min-width: 42px; text-align: center; color: #ffb27a; }
    .vbar .sp { width: 1px; height: 14px; background: #ffffff44; margin: 0 3px; }
    .flash { background: #14181acc; color: #fff; border-radius: 8px; padding: 4px 12px; font-size: 16px; font-weight: 700; transform: translateX(-50%); pointer-events: none; }
    .pnl { background: #fff; color: #1d2420; border: 1px solid #cfd5d1; border-radius: 10px; box-shadow: 0 10px 28px #0004; padding: 10px 12px; width: 280px; font-size: 12px; }
    .pnl .r { display: flex; align-items: center; gap: 8px; padding: 4px 0; }
    .pnl .r > span { flex: 0 0 84px; color: #5d6862; }
    .pnl input[type=range] { flex: 1; min-width: 0; }
    .pnl input[type=number] { width: 64px; padding: 2px 4px; border: 1px solid #cfd5d1; border-radius: 4px; }
    .pnl .hd { font-weight: 700; margin-bottom: 4px; }
    .pnl button { font-size: 12px; padding: 3px 10px; }
  `

  // ---------- 设置 ----------

  async function load() {
    try {
      const r = await api.storage.local.get(['features', 'videoCfg', 'videoRates', 'siteRules'])
      const f = r.features && typeof r.features === 'object' ? r.features : {}
      feat = { ...DEF_FEAT }
      for (const k of Object.keys(DEF_FEAT)) if (typeof f[k] === 'boolean') feat[k] = f[k]
      cfg = L.normalizeCfg(r.videoCfg)
      rates = r.videoRates && typeof r.videoRates === 'object' ? r.videoRates : {}
      off = listed(r.siteRules?.off?.video, host)
    } catch {
      return
    }
    if (off) teardown()
    else {
      scan()
      for (const el of known) applyRemembered(el)
    }
  }
  onSettings(load)
  api.storage.onChanged.addListener((ch, area) => area === 'local' && ch.videoCfg && load())

  let saveTimer = 0
  function saveRates() {
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => api.storage.local.set({ videoRates: rates }).catch(() => {}), 600)
  }
  const saveCfg = () => api.storage.local.set({ videoCfg: cfg }).catch(() => {})

  // ---------- 找媒体元素 ----------

  function deepAll(sel, root = document, out = [], budget = { n: 6000 }) {
    out.push(...root.querySelectorAll(sel))
    for (const el of root.querySelectorAll('*')) {
      if (--budget.n < 0) break
      if (el.shadowRoot) deepAll(sel, el.shadowRoot, out, budget)
    }
    return out
  }

  function scan() {
    if (off) return
    for (const el of deepAll('video,audio')) adopt(el)
    for (const el of [...known]) if (!el.isConnected) known.delete(el)
  }

  function adopt(el) {
    if (known.has(el)) return
    known.add(el)
    const s = S(el)
    el.addEventListener('ratechange', () => onRateChange(el))
    el.addEventListener('loadedmetadata', () => {
      s.introDone = s.outroDone = false
      applyRemembered(el)
    })
    el.addEventListener('emptied', () => {
      s.introDone = s.outroDone = false
      s.A = s.B = null
    })
    el.addEventListener('play', () => (lastActive = el))
    el.addEventListener('timeupdate', () => onTime(el))
    applyRemembered(el)
  }

  let lastActive = null
  let moTimer = 0
  new MutationObserver(() => {
    clearTimeout(moTimer)
    moTimer = setTimeout(scan, 400)
  }).observe(document, { childList: true, subtree: true })
  scan()

  function activeMedia() {
    if (hovered?.isConnected) return hovered
    if (lastActive?.isConnected && !lastActive.paused) return lastActive
    let best = null
    let area = -1
    for (const el of known) {
      if (!el.isConnected || el.paused) continue
      const r = el.getBoundingClientRect()
      if (r.width * r.height > area) {
        area = r.width * r.height
        best = el
      }
    }
    if (best) return best
    for (const el of known) {
      const r = el.getBoundingClientRect()
      if (el.tagName === 'VIDEO' && r.width * r.height > area) {
        area = r.width * r.height
        best = el
      }
    }
    return best || (known.size ? [...known][0] : null)
  }

  // ---------- 倍速 ----------

  function setRate(el, r, announce = true) {
    const s = S(el)
    const v = L.clampRate(r)
    s.desired = v
    s.ours = performance.now()
    el.playbackRate = v
    if (feat.speedMemory) {
      if (v === 1) delete rates[host]
      else rates[host] = v
      saveRates()
    }
    if (announce) flash(el, L.fmtRate(v))
    refreshBar()
  }

  function applyRemembered(el) {
    if (off) return
    const s = S(el)
    const r = feat.speedMemory ? Number(rates[host]) : 0
    if (r && r !== 1 && s.desired == null) {
      s.desired = L.clampRate(r)
      s.ours = performance.now()
      el.playbackRate = s.desired
    } else if (s.desired != null && Math.abs(el.playbackRate - s.desired) > 0.001) {
      s.ours = performance.now()
      el.playbackRate = s.desired
    }
  }

  /** 网站把倍速改回去：不是用户刚操作过，就改回我们设置的值；用户自己在网站菜单里改的就跟随。 */
  function onRateChange(el) {
    const s = S(el)
    if (off || s.desired == null || Math.abs(el.playbackRate - s.desired) <= 0.001 || performance.now() - (s.ours || 0) < 60) return
    const now = performance.now()
    s.resets = s.resets.filter((t) => now - t < 3000)
    if (now - lastGesture < 700) {
      s.desired = el.playbackRate
      if (feat.speedMemory) {
        if (s.desired === 1) delete rates[host]
        else rates[host] = s.desired
        saveRates()
      }
      refreshBar()
      return
    }
    // 网站和我们来回争：不再改回去，也不把网站的值当成用户的选择（记住的倍速保持不变）
    if (s.resets.length >= 5) return
    s.resets.push(now)
    s.ours = now
    el.playbackRate = s.desired
  }

  // ---------- 跳过片头片尾、A–B 循环 ----------

  function onTime(el) {
    if (off) return
    const s = S(el)
    if (feat.videoTools && s.A != null && s.B != null && el.currentTime >= s.B) el.currentTime = s.A
    if (!feat.videoAdvanced) return
    if (!s.introDone && el.currentTime > 0) {
      s.introDone = true
      const t = L.skipIntroTarget(el.currentTime, cfg.skipIntro[host], el.duration)
      if (t) {
        el.currentTime = t
        flash(el, `已跳过片头 ${t} 秒`)
      }
    }
    if (!s.outroDone && L.inOutro(el.currentTime, cfg.skipOutro[host], el.duration)) {
      s.outroDone = true
      flash(el, '已跳过片尾')
      el.currentTime = el.duration
    }
  }

  function loopStep(el) {
    const s = S(el)
    if (s.A == null) {
      s.A = el.currentTime
      s.B = null
      flash(el, `A 点 ${L.fmtTime(s.A).replace(/-/g, ':')}`)
    } else if (s.B == null) {
      if (el.currentTime <= s.A + 0.5) return flash(el, 'B 点要在 A 点之后')
      s.B = el.currentTime
      flash(el, `循环 A–B`)
    } else {
      s.A = s.B = null
      flash(el, '已取消循环')
    }
    refreshBar()
  }

  // ---------- 画面调整、音量增益 ----------

  function applyFx(el) {
    const s = S(el)
    if (!s.saved) s.saved = { filter: el.style.filter, transform: el.style.transform, objectFit: el.style.objectFit }
    const fx = s.fx
    el.style.filter = L.filterCss(fx) || s.saved.filter
    const k = fx.rotate % 180 && el.clientWidth ? Math.min(1, el.clientHeight / el.clientWidth) : 1
    el.style.transform = (L.transformCss(fx) + (k < 1 ? ` scale(${k})` : '')).trim() || s.saved.transform
    el.style.objectFit = fx.fill ? 'cover' : s.saved.objectFit
  }

  function setGain(el, pct) {
    const s = S(el)
    if (!s.gain) {
      if (pct <= 100) return
      if (L.audioGraphUnsafe(el.currentSrc || el.src, location.origin, el.crossOrigin)) {
        flash(el, '这个视频来自其他网站，放大音量会让它没有声音，已取消')
        return false
      }
      try {
        const ac = new AudioContext()
        const src = ac.createMediaElementSource(el)
        const gain = ac.createGain()
        const an = ac.createAnalyser()
        src.connect(gain)
        gain.connect(ac.destination)
        gain.connect(an)
        s.gain = { ac, gain, an }
        setTimeout(() => {
          const buf = new Uint8Array(an.fftSize)
          an.getByteTimeDomainData(buf)
          if (!el.paused && !el.muted && el.volume > 0 && buf.every((v) => v === 128)) {
            gain.gain.value = 1
            flash(el, '放大音量后没有声音，可能是网站限制；刷新页面可以恢复')
          }
        }, 1800)
      } catch {
        flash(el, '这个视频不能放大音量')
        return false
      }
    }
    s.gain.ac.resume?.()
    s.gain.gain.gain.value = Math.min(4, Math.max(1, pct / 100))
    return true
  }

  // ---------- 截图 ----------

  async function shot(el, mode = 'save') {
    if (el.tagName !== 'VIDEO' || !el.videoWidth) return flash(el, '没有可截的画面')
    const type = (await api.storage.local.get('shotFormat').catch(() => ({}))).shotFormat === 'jpg' ? 'image/jpeg' : 'image/png'
    let blob = null
    try {
      const c = document.createElement('canvas')
      c.width = el.videoWidth
      c.height = el.videoHeight
      c.getContext('2d').drawImage(el, 0, 0)
      blob = await new Promise((ok, no) => c.toBlob((b) => (b ? ok(b) : no(new Error('empty'))), type, 0.92))
    } catch {
      blob = await shotViaTab(el, type)
    }
    if (!blob) return flash(el, '截图失败：这个视频受保护，不能读取画面')
    if (mode === 'copy') {
      try {
        await navigator.clipboard.write([new ClipboardItem({ [blob.type]: blob })])
        return flash(el, '截图已复制')
      } catch {
        return flash(el, '复制失败，请改用保存')
      }
    }
    const a = document.createElement('a')
    a.href = URL.createObjectURL(blob)
    a.download = L.shotName(document.title, el.currentTime, type === 'image/png' ? 'png' : 'jpg')
    document.body.append(a)
    a.click()
    a.remove()
    setTimeout(() => URL.revokeObjectURL(a.href), 4000)
    flash(el, '截图已保存')
  }

  /** 视频跨域、画布被污染：改用浏览器对可见区域的截图，再裁出视频所在的部分。 */
  async function shotViaTab(el, type) {
    bar && (bar.host.style.visibility = 'hidden')
    await new Promise((r) => setTimeout(r, 90))
    const res = await send({ type: 'cc-capture' })
    bar && (bar.host.style.visibility = '')
    if (!res.ok || !res.dataUrl) return null
    const img = await new Promise((ok) => {
      const i = new Image()
      i.onload = () => ok(i)
      i.onerror = () => ok(null)
      i.src = res.dataUrl
    })
    if (!img) return null
    const r = el.getBoundingClientRect()
    const k = img.naturalWidth / innerWidth
    const c = document.createElement('canvas')
    c.width = Math.round(r.width * k)
    c.height = Math.round(r.height * k)
    c.getContext('2d').drawImage(img, r.left * k, r.top * k, r.width * k, r.height * k, 0, 0, c.width, c.height)
    return new Promise((ok) => c.toBlob((b) => ok(b), type, 0.92))
  }

  // ---------- 动作 ----------

  const needs = {
    slower: 'speed', faster: 'speed', reset: 'speed', back: 'seek', forward: 'seek',
    frameBack: 'videoTools', frameForward: 'videoTools', shot: 'videoTools', pip: 'videoTools', loopA: 'videoTools', loopB: 'videoTools', loopClear: 'videoTools',
    mirror: 'videoAdvanced', rotate: 'videoAdvanced',
  }

  function run(id, el) {
    const s = S(el)
    switch (id) {
      case 'slower': return setRate(el, L.stepRate(el.playbackRate, -1, cfg.rateStep))
      case 'faster': return setRate(el, L.stepRate(el.playbackRate, 1, cfg.rateStep))
      case 'reset': return setRate(el, 1)
      case 'back': return seek(el, -cfg.seekStep)
      case 'forward': return seek(el, cfg.seekStep)
      case 'frameBack': return frame(el, -1)
      case 'frameForward': return frame(el, 1)
      case 'shot': return shot(el)
      case 'pip': return pip(el)
      case 'loopA': s.A = el.currentTime; s.B = null; flash(el, `A 点 ${L.fmtTime(s.A).replace(/-/g, ':')}`); return refreshBar()
      case 'loopB':
        if (s.A == null || el.currentTime <= s.A + 0.5) return flash(el, '先标记 A 点')
        s.B = el.currentTime
        flash(el, '循环 A–B')
        return refreshBar()
      case 'loopClear': s.A = s.B = null; flash(el, '已取消循环'); return refreshBar()
      case 'mirror': s.fx.mirror = !s.fx.mirror; return applyFx(el)
      case 'rotate': s.fx.rotate = (s.fx.rotate + 90) % 360; return applyFx(el)
    }
  }

  function seek(el, d) {
    if (!isFinite(el.duration)) return
    el.currentTime = Math.max(0, Math.min(el.duration, el.currentTime + d))
    flash(el, `${d > 0 ? '+' : '−'}${Math.abs(d)} 秒`)
  }

  function frame(el, d) {
    if (!el.paused) el.pause()
    el.currentTime = Math.max(0, el.currentTime + d / 30)
  }

  async function pip(el) {
    try {
      if (document.pictureInPictureElement === el) await document.exitPictureInPicture()
      else await el.requestPictureInPicture()
    } catch {
      flash(el, '这个视频不能使用画中画')
    }
  }

  const editable = (t) => t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))

  document.addEventListener('keydown', (e) => {
    lastGesture = performance.now()
    if (off || e.isComposing || editable(e.composedPath?.()[0] || e.target)) return
    const id = L.matchKey(e, cfg.keys)
    if (!id || !feat[needs[id]]) return
    const el = activeMedia()
    if (!el) return
    e.preventDefault()
    e.stopImmediatePropagation()
    run(id, el)
  }, true)
  document.addEventListener('pointerdown', () => (lastGesture = performance.now()), true)
  document.addEventListener('click', () => (lastGesture = performance.now()), true)

  // ---------- 控制条 ----------

  function ensureUi() {
    if (ui) return
    ui = mount(CSS)
  }

  function flash(el, text) {
    ensureUi()
    const r = el.getBoundingClientRect()
    const t = h('div', { class: 'layer flash', text, style: `left:${Math.max(60, r.left + r.width / 2)}px;top:${Math.max(r.top, 0) + 16}px` })
    ui.add(t)
    setTimeout(() => t.remove(), 1100)
  }

  const BTN = (text, fn, title) => h('button', { text, title, onclick: (e) => { e.stopPropagation(); fn() } })
  const SP = () => h('span', { class: 'sp' })

  function refreshBar() {
    if (!bar || !hovered) return
    const el = hovered
    const s = S(el)
    const wide = el.getBoundingClientRect().width >= 420
    const kids = []
    if (feat.speed) kids.push(BTN('‹', () => run('slower', el), '减速'), h('span', { class: 'rate', text: L.fmtRate(el.playbackRate) }), BTN('›', () => run('faster', el), '加速'), BTN('复位', () => run('reset', el)))
    if (feat.seek && wide) kids.push(SP(), BTN(`−${cfg.seekStep}s`, () => run('back', el)), BTN(`+${cfg.seekStep}s`, () => run('forward', el)))
    if (feat.videoTools && wide) {
      kids.push(SP(), BTN('截图', () => run('shot', el)))
      if (el.requestPictureInPicture) kids.push(BTN('画中画', () => run('pip', el)))
      kids.push(BTN(s.A == null ? '循环 A–B' : s.B == null ? '标记 B' : '取消循环', () => loopStep(el)))
    }
    kids.push(SP(), BTN('⋯', () => togglePanel(el), '更多'))
    bar.firstChild.replaceChildren(...kids)
  }

  function place() {
    if (!bar || !hovered) return
    const r = hovered.getBoundingClientRect()
    if (r.width < 240 || r.height < 135 || r.bottom < 40 || r.top > innerHeight - 40) return hideBar()
    const el = bar.firstChild
    el.style.left = `${Math.max(4, r.left + 8)}px`
    el.style.top = `${Math.max(r.top + 8, 8)}px`
    el.style.display = ''
    if (panel) {
      panel.style.left = el.style.left
      panel.style.top = `${parseFloat(el.style.top) + 32}px`
    }
  }

  function showBar(v) {
    if (off || !(feat.speed || feat.seek || feat.videoTools || feat.videoAdvanced || feat.speedMemory)) return
    ensureUi()
    if (!bar) {
      const wrap = h('div', { class: 'layer vbar', style: 'display:none' })
      wrap.addEventListener('mouseenter', () => clearTimeout(hideTimer))
      wrap.addEventListener('mouseleave', scheduleHide)
      ui.add(wrap)
      bar = { host: ui.host, firstChild: wrap }
    }
    clearTimeout(hideTimer)
    hovered = v
    adopt(v)
    refreshBar()
    place()
  }

  function hideBar() {
    clearTimeout(hideTimer)
    closePanel()
    if (bar) bar.firstChild.style.display = 'none'
    hovered = null
  }
  const scheduleHide = () => {
    clearTimeout(hideTimer)
    hideTimer = setTimeout(() => !panel && hideBar(), 1800)
  }

  document.addEventListener('mousemove', (e) => {
    const now = performance.now()
    if (off || now - lastMove < 120) return
    lastMove = now
    let v = null
    for (const el of e.composedPath?.() || []) if (el.tagName === 'VIDEO') { v = el; break }
    if (!v) v = document.elementsFromPoint(e.clientX, e.clientY).find((x) => x.tagName === 'VIDEO') || null
    if (v) showBar(v)
    else if (bar && bar.firstChild.style.display !== 'none') scheduleHide()
  }, { capture: true, passive: true })
  addEventListener('scroll', () => hovered && place(), { capture: true, passive: true })
  addEventListener('resize', () => hovered && place())

  // ---------- “⋯”面板 ----------

  function closePanel() {
    panel?.remove()
    panel = null
  }

  function row(label, ...ctrl) {
    return h('div', { class: 'r' }, h('span', { text: label }), ...ctrl)
  }

  function togglePanel(el) {
    if (panel) return closePanel()
    const s = S(el)
    const kids = [h('div', { class: 'hd', text: '播放增强' })]
    if (feat.speedMemory) kids.push(row('新视频默认倍速', h('span', { style: 'flex:1', text: rates[host] ? `记住 · ${L.fmtRate(Number(rates[host]))}` : '1×（未记住）' }), BTN('清除', () => { delete rates[host]; saveRates(); closePanel() })))
    if (feat.videoAdvanced) {
      const num = (map, key) => {
        const i = h('input', { type: 'number', min: '0', max: '3600', value: String(map[host] || 0) })
        i.addEventListener('change', () => {
          const v = Math.max(0, Math.min(3600, Number(i.value) || 0))
          if (v) map[host] = v
          else delete map[host]
          saveCfg()
        })
        return i
      }
      kids.push(row('跳过片头（秒）', num(cfg.skipIntro), h('span', { text: '0 = 不跳' })), row('跳过片尾（秒）', num(cfg.skipOutro)))
      const g = h('input', { type: 'range', min: '100', max: '400', step: '10', value: String(Math.round((s.gain?.gain.gain.value || 1) * 100)) })
      const gl = h('span', { style: 'flex:0 0 40px', text: `${g.value}%` })
      g.addEventListener('input', () => {
        if (setGain(el, Number(g.value)) === false) g.value = '100'
        gl.textContent = `${g.value}%`
      })
      kids.push(row('音量增益', g, gl))
      const slider = (key, label) => {
        const i = h('input', { type: 'range', min: '50', max: '150', value: String(s.fx[key]) })
        i.addEventListener('input', () => {
          s.fx[key] = Number(i.value)
          applyFx(el)
        })
        return row(label, i)
      }
      kids.push(slider('brightness', '亮度'), slider('contrast', '对比度'), slider('saturate', '饱和度'))
      const fill = h('input', { type: 'checkbox' })
      fill.checked = s.fx.fill
      fill.addEventListener('change', () => { s.fx.fill = fill.checked; applyFx(el) })
      kids.push(row('画面', BTN('镜像', () => run('mirror', el)), BTN('旋转', () => run('rotate', el)), h('label', {}, fill, '填充')))
      kids.push(row('', BTN('还原画面', () => { s.fx = { mirror: false, rotate: 0, brightness: 100, contrast: 100, saturate: 100, fill: false }; applyFx(el); closePanel() })))
    }
    if (feat.videoTools) {
      const loop = h('input', { type: 'checkbox' })
      loop.checked = el.loop
      loop.addEventListener('change', () => (el.loop = loop.checked))
      kids.push(row('整段循环', loop), row('截图', BTN('复制到剪贴板', () => shot(el, 'copy'))))
    }
    kids.push(row('此网站', BTN('不再显示播放增强', async () => {
      const r = (await api.storage.local.get('siteRules')).siteRules || {}
      r.off = { floatButton: [], video: [], hoverZoom: [], ...(r.off || {}) }
      r.off.video = [...new Set([...(r.off.video || []), host])]
      await api.storage.local.set({ siteRules: r })
    })))
    panel = h('div', { class: 'layer pnl' }, ...kids)
    ui.add(panel)
    place()
  }

  document.addEventListener('keydown', (e) => e.key === 'Escape' && panel && closePanel(), true)
  document.addEventListener('mousedown', (e) => panel && !e.composedPath().includes(ui.host) && closePanel(), true)

  function teardown() {
    hideBar()
    ui?.remove()
    ui = bar = null
    for (const el of known) {
      const s = S(el)
      if (s.saved) {
        el.style.filter = s.saved.filter
        el.style.transform = s.saved.transform
        el.style.objectFit = s.saved.objectFit
      }
    }
  }

  load()
})()
