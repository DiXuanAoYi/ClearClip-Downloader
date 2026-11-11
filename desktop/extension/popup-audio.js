// 弹窗“音效”页：响度标准化开关、预设和自定义、混响与均衡器、本页状态与响度表、本站关闭。
// 设置存在 storage.local.audioCfg，网页里的音效脚本监听它，改了立刻生效。
;(() => {
  const A = globalThis.ccAudio
  const $ = (id) => document.getElementById(id)
  const api = globalThis.browser ?? globalThis.chrome
  let cfg = A.normalizeCfg()
  let tab = null
  let host = ''
  let timer = 0
  let siteOff = false
  let built = false

  const pct = (v) => Math.max(0, Math.min(100, ((v + 40) / 32) * 100))

  async function save(next) {
    cfg = A.normalizeCfg(next)
    await api.storage.local.set({ audioCfg: cfg }).catch(() => {})
    render()
  }

  function build() {
    if (built) return
    built = true
    $('auSave').onclick = saveUser
    $('auName').onkeydown = (e) => {
      if (e.key === 'Enter') saveUser()
    }
    $('auDel').onclick = () => save(A.deleteUserPreset(cfg, cfg.preset))
    $('auRev').append(new Option('关', ''))
    for (const r of A.REVERBS) $('auRev').append(new Option(r.name, r.id))
    $('auEq').append(new Option('关', ''))
    for (const e of A.EQS) $('auEq').append(new Option(e.name, e.id))

    $('auOn').onchange = () => save({ ...cfg, on: $('auOn').checked })
    $('auPill').onchange = () => save({ ...cfg, pillShow: $('auPill').checked })
    $('auTarget').oninput = () => save(A.withCustom(cfg, { target: Number($('auTarget').value) }))
    $('auComp').oninput = () => save(A.withCustom(cfg, { comp: Number($('auComp').value) }))
    $('auMax').oninput = () => save(A.withCustom(cfg, { maxGain: Number($('auMax').value) }))
    $('auSpeed').onchange = () => save({ ...cfg, speed: $('auSpeed').value })
    $('auLimiter').onchange = () => save({ ...cfg, limiter: $('auLimiter').checked })
    $('auRev').onchange = () => save({ ...cfg, reverb: { ...cfg.reverb, on: !!$('auRev').value, id: $('auRev').value || cfg.reverb.id, mix: null } })
    $('auMix').oninput = () => save({ ...cfg, reverb: { ...cfg.reverb, mix: Number($('auMix').value) } })
    $('auEq').onchange = () => save({ ...cfg, eq: { on: !!$('auEq').value, id: $('auEq').value || cfg.eq.id } })
    $('auSiteOff').onclick = async (e) => {
      e.preventDefault()
      const { siteRules } = await api.storage.local.get('siteRules')
      const r = siteRules && typeof siteRules === 'object' ? siteRules : {}
      const list = new Set(Array.isArray(r.off?.audio) ? r.off.audio : [])
      if (siteOff) for (const s of [...list]) if (host === s || host.endsWith('.' + s)) list.delete(s)
      if (!siteOff) list.add(host)
      r.off = { ...(r.off || {}), audio: [...list] }
      await api.storage.local.set({ siteRules: r })
      siteOff = !siteOff
      render()
    }
    $('auTmp').onclick = async (e) => {
      e.preventDefault()
      const want = $('auTmp').dataset.off !== '1'
      await api.tabs.sendMessage(tab.id, { type: 'cc-audio', action: 'tmp', off: want }).catch(() => {})
      poll()
    }
  }

  /** 预设按用途分组：内置的几组，再加“我的预设”和一个还没保存的“自定义”。 */
  function renderPresets() {
    const box = $('auPresets')
    const groups = []
    for (const p of A.allPresets(cfg)) {
      let g = groups.find((x) => x.name === p.group)
      if (!g) groups.push((g = { name: p.group, items: [] }))
      g.items.push(p)
    }
    const mine = groups.find((x) => x.name === '我的预设')
    if (!mine) groups.push({ name: '我的预设', items: [] })
    const g2 = groups.find((x) => x.name === '我的预设')
    g2.items.push({ id: 'custom', name: '自定义', target: null, custom: true })
    box.replaceChildren()
    for (const g of groups) {
      const wrap = document.createElement('div')
      wrap.className = 'auGroup'
      const t = document.createElement('div')
      t.className = 'mute'
      t.textContent = g.name
      const chips = document.createElement('div')
      chips.className = 'chips'
      for (const p of g.items) {
        const b = document.createElement('button')
        b.dataset.id = p.id
        b.append(p.name)
        if (p.target != null) {
          const s = document.createElement('small')
          s.textContent = String(p.target)
          b.append(s)
        }
        b.setAttribute('aria-pressed', String(p.id === cfg.preset))
        b.onclick = () => save(p.custom ? { ...cfg, preset: 'custom' } : A.withPreset(cfg, p.id))
        chips.append(b)
      }
      wrap.append(t, chips)
      box.append(wrap)
    }
  }

  async function saveUser() {
    const name = $('auName').value.trim()
    const exists = cfg.userPresets.some((x) => x.name === name)
    if (!exists && cfg.userPresets.length >= A.MAX_USER_PRESETS) {
      $('auSaveMsg').textContent = `最多存 ${A.MAX_USER_PRESETS} 个，先删掉不用的`
      return
    }
    await save(A.saveUserPreset(cfg, name))
    $('auName').value = ''
    $('auSaveMsg').textContent = exists ? `已更新“${name}”` : '已存为我的预设，可以随时选'
  }

  function render() {
    const p = A.allPresets(cfg).find((x) => x.id === cfg.preset)
    $('auOn').checked = cfg.on
    renderPresets()
    $('auDel').classList.toggle('hidden', !p?.user)
    $('auPresetNote').textContent = p ? (p.user ? `我的预设：目标 ${p.target} LUFS · 压缩 ${p.comp}% · 最大增益 +${p.maxGain} dB` : p.note) : '改了任意一项就算自定义，可以在下面起个名字存成预设。'
    $('auTarget').value = cfg.target
    $('auComp').value = cfg.comp
    $('auMax').value = cfg.maxGain
    $('auTargetV').textContent = `${cfg.target} LUFS`
    $('auCompV').textContent = `${cfg.comp}%`
    $('auMaxV').textContent = `+${cfg.maxGain} dB`
    $('auPill').checked = cfg.pillShow
    $('auSpeed').value = cfg.speed
    $('auLimiter').checked = cfg.limiter
    $('auRev').value = cfg.reverb.on ? cfg.reverb.id : ''
    const spec = A.reverbSpec(cfg)
    $('auMix').value = spec.mix
    $('auMix').disabled = !cfg.reverb.on
    $('auMixV').textContent = `${spec.mix} dB`
    $('auEq').value = cfg.eq.on ? cfg.eq.id : ''
    $('auTg').style.left = `${pct(cfg.target)}%`
    $('auCustom').style.opacity = cfg.on ? 1 : 0.5
    $('auSite').textContent = host || '此页面'
    $('auSiteOff').textContent = siteOff ? '恢复本站' : '本站关闭'
    $('auSiteOff').classList.toggle('hidden', !host)
  }

  async function poll() {
    let r = null
    if (tab?.id != null) r = await api.tabs.sendMessage(tab.id, { type: 'cc-audio', action: 'stat' }).catch(() => null)
    const st = $('auState')
    const f = await getFeatures()
    st.className = 'card'
    if (!f.audioFx) {
      st.classList.add('off')
      st.textContent = '实时音效已在“设置 → 功能开关”里关闭'
    } else if (!r) {
      st.textContent = host ? A.statusText({ siteOff, count: 0 }) : '这个页面不能使用音效'
      st.classList.add(siteOff ? 'off' : 'warn')
      if (!siteOff) st.classList.remove('warn')
    } else {
      st.textContent = A.statusText({ off: !cfg.on && !cfg.reverb.on && !cfg.eq.on, siteOff: r.siteOff, blocked: r.blocked, active: r.active, count: r.count })
      if (r.siteOff || r.tmpOff) st.classList.add('off')
      else if (r.blocked && !r.active) st.classList.add('warn')
    }
    const lufs = r?.lufs ?? null
    const gain = r?.gain ?? null
    const out = lufs == null || gain == null ? null : lufs + gain
    $('auIn').style.width = lufs == null ? '0' : `${pct(lufs)}%`
    $('auOut').style.width = out == null ? '0' : `${pct(out)}%`
    $('auGain').style.width = gain == null ? '0' : `${Math.max(0, Math.min(100, ((gain + 18) / 42) * 100))}%`
    $('auInV').textContent = A.fmtLufs(lufs)
    $('auOutV').textContent = A.fmtLufs(out)
    $('auGainV').textContent = A.fmtDb(gain)
    $('auTmp').dataset.off = r?.tmpOff ? '1' : '0'
    $('auTmp').textContent = r?.tmpOff ? '恢复本页' : '临时关闭本页'
    $('auTmp').classList.toggle('hidden', !r)
    $('auTmpInfo').textContent = r?.tmpOff ? '本页已临时关闭，刷新后恢复' : ''
  }

  async function open() {
    build()
    try {
      const [t] = await api.tabs.query({ active: true, currentWindow: true })
      tab = t
      host = t?.url?.startsWith('http') ? new URL(t.url).hostname.toLowerCase() : ''
    } catch {
      tab = null
    }
    const r = await api.storage.local.get(['audioCfg', 'siteRules']).catch(() => ({}))
    cfg = A.normalizeCfg(r.audioCfg)
    siteOff = (r.siteRules?.off?.audio || []).some((s) => host === s || host.endsWith('.' + s))
    render()
    if (tab?.id != null) api.tabs.sendMessage(tab.id, { type: 'cc-audio', action: 'meter', on: true }).catch(() => {})
    poll()
    clearInterval(timer)
    timer = setInterval(poll, 600)
  }

  function close() {
    clearInterval(timer)
    if (tab?.id != null) api.tabs.sendMessage(tab.id, { type: 'cc-audio', action: 'meter', on: false }).catch(() => {})
  }

  globalThis.ccAudioPopup = { open, close }
  addEventListener('pagehide', close)
})()
