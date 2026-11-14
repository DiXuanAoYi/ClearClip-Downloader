// 弹窗 / 设置页（popup.html）的拆分脚本：和 popup.js 共用同一个全局作用域（普通脚本，按 popup.html 里的顺序加载），
// 这里只放：设置页：加载 / 保存设置、网页翻译、写作、视频设置。
// ---------- 设置页 ----------

const lineList = (id) => $(id).value.split(/[\s,;]+/).map((x) => x.trim().toLowerCase()).filter(Boolean)

async function loadSettings() {
  const c = await getConfig()
  $('port').value = c.port || ''
  $('token').value = c.token || ''
  $('noteLimit').value = Number((await api.storage.local.get('noteLimit')).noteLimit) || 0
  $('setupHint').classList.toggle('hidden', !!(c.port && c.token))
  const features = await getFeatures()
  const box = $('featureList')
  box.innerHTML = ''
  let group = ''
  for (const f of FEATURES) {
    if (f.group !== group) {
      group = f.group
      const h = document.createElement('div')
      h.className = 'mute grp'
      h.textContent = FEATURE_GROUPS[group]
      box.append(h)
    }
    const row = document.createElement('label')
    row.className = 'sw'
    const text = document.createElement('div')
    const n = document.createElement('div')
    n.className = 'n'
    n.textContent = f.name
    text.append(n)
    if (f.desc) {
      const d = document.createElement('div')
      d.className = 'mute'
      d.textContent = f.desc
      text.append(d)
    }
    const tag = document.createElement('span')
    tag.className = f.tag.startsWith('常驻') ? 'tag hot' : 'tag'
    tag.textContent = f.tag
    const cb = document.createElement('input')
    cb.type = 'checkbox'
    cb.checked = features[f.id]
    cb.onchange = async () => {
      // 在网页里常驻的功能要“访问所有网页”权限：开启时申请（必须是第一步）；拒绝也照常保存开关，只是暂时不生效
      if (cb.checked && f.tag.startsWith('常驻') && !(await ccPerms.hasSites())) await grantSites()
      const cur = await getFeatures()
      cur[f.id] = cb.checked
      await api.storage.local.set({ features: cur })
      msg(cb.checked ? `已开启：${f.name}` : `已关闭：${f.name}`, 'ok')
    }
    row.append(text, tag, cb)
    box.append(row)
  }
  const { notifyDone = true, engines = '' } = await api.storage.local.get(['notifyDone', 'engines'])
  $('notifyDone').checked = notifyDone
  const rules = await getSiteRules()
  $('unlockAll').checked = rules.unlockAll
  $('ruleUnlock').value = rules.unlockOn.join('\n')
  $('ruleFloat').value = rules.off.floatButton.join('\n')
  $('ruleVideo').value = rules.off.video.join('\n')
  $('ruleZoom').value = rules.off.hoverZoom.join('\n')
  $('ruleZoomRules').value = rules.zoomRules
  $('ruleParams').value = rules.extraParams
  $('ruleEngines').value = engines || enginesToText(DEFAULT_ENGINES)
  renderSync()
  renderVideoCfg()
  loadTransCfg()
  loadWriteCfg()
}

// ---------- 网页翻译 ----------

let appLang = ''
function fillLangs(sel, value) {
  sel.innerHTML = ''
  for (const l of ccTrans.LANGS) {
    const o = document.createElement('option')
    o.value = l.code
    o.textContent = ccTrans.label(l.code, appLang)
    sel.append(o)
  }
  sel.value = value
}

async function saveTransCfg(patch) {
  const { transCfg } = await api.storage.local.get('transCfg')
  await api.storage.local.set({ transCfg: ccTrans.normalizeCfg({ ...ccTrans.normalizeCfg(transCfg), ...patch }) })
}

async function loadTransCfg() {
  const { transCfg } = await api.storage.local.get('transCfg')
  const c = ccTrans.normalizeCfg(transCfg)
  appLang = (await api.storage.local.get('appLang')).appLang || ''
  // 问一下清影现在的界面语言；变了就把下拉框重新排一遍
  api.runtime.sendMessage({ type: 'cc-app-lang' }).then((r) => {
    if (r?.ok && r.lang && r.lang !== appLang) {
      appLang = r.lang
      for (const id of ['trTarget', 'trTarget2']) fillLangs($(id), $(id).value)
      fillLangs($('trInputLang'), $('trInputLang').value)
    }
  }).catch(() => {})
  for (const id of ['trTarget', 'trTarget2']) fillLangs($(id), c.target)
  fillLangs($('trInputLang'), c.inputLang)
  $('trMode').value = c.mode === 'replace' ? 'replace' : c.blur ? 'blur' : 'bilingual'
  $('trEngine').value = c.engine
  $('trStyle').value = c.style
  $('trHover').value = c.hover
  $('trHold').value = String(c.holdMs)
  $('trCompare').checked = c.compare
  for (const eng of ccTrans.ENGINES) {
    const sv = c.services[eng] || {}
    $(`svcOn_${eng}`).checked = eng === 'custom' || eng === 'deeplx' ? sv.enabled === true : sv.enabled !== false
    const sel = $(`svcLang_${eng}`)
    fillLangs(sel, sv.target || c.target)
    const keep = document.createElement('option')
    keep.value = ''
    keep.textContent = '沿用上面的目标语言'
    sel.prepend(keep)
    sel.value = sv.target || ''
  }
  $('svcPrompt_clearclip').value = c.services.clearclip?.prompt || ''
  $('svcUrl_deeplx').value = c.services.deeplx?.url || ''
  $('svcKey_deeplx').value = c.services.deeplx?.key || ''
  $('svcUrl_custom').value = c.services.custom?.url || ''
  $('svcKey_custom').value = c.services.custom?.key || ''
  $('svcModel_custom').value = c.services.custom?.model || ''
  $('svcPrompt_custom').value = c.services.custom?.prompt || ''
  $('trExclude').value = (c.exclude || []).join('\n')
  $('trCss').value = c.customCss || ''
  $('trInput').checked = c.input
  $('trLazy').checked = c.lazy
  $('trDomain').checked = c.domain
  $('trSkip').checked = c.skipTarget
  $('trAuto').value = c.auto.join('\n')
  $('trNever').value = c.never.join('\n')
  $('trSites').value = ccTrans.formatSites(c.sites)
}

$('trTarget').onchange = async () => (await saveTransCfg({ target: $('trTarget').value }), ($('trTarget2').value = $('trTarget').value))
$('trMode').onchange = () => {
  const v = $('trMode').value
  return saveTransCfg({ mode: v === 'replace' ? 'replace' : 'bilingual', blur: v === 'blur' })
}
$('startTrans').onclick = () => runOnPage('webtrans')
$('openTransBox').onclick = async () => {
  await api.runtime.sendMessage({ type: 'cc-open-transbox' }).catch(() => {})
  window.close()
}
$('saveTrans').onclick = async () => {
  await saveTransCfg({
    target: $('trTarget2').value,
    engine: $('trEngine').value,
    style: $('trStyle').value,
    hover: $('trHover').value,
    inputLang: $('trInputLang').value,
    holdMs: Number($('trHold').value),
    compare: $('trCompare').checked,
    services: Object.fromEntries(
      ccTrans.ENGINES.map((eng) => [
        eng,
        {
          enabled: $(`svcOn_${eng}`).checked,
          target: $(`svcLang_${eng}`).value,
          prompt: eng === 'clearclip' ? $('svcPrompt_clearclip').value : eng === 'custom' ? $('svcPrompt_custom').value : '',
          ...(eng === 'deeplx' ? { url: $('svcUrl_deeplx').value, key: $('svcKey_deeplx').value } : {}),
          ...(eng === 'custom' ? { url: $('svcUrl_custom').value, key: $('svcKey_custom').value, model: $('svcModel_custom').value } : {}),
        },
      ]),
    ),
    input: $('trInput').checked,
    lazy: $('trLazy').checked,
    domain: $('trDomain').checked,
    skipTarget: $('trSkip').checked,
    exclude: $('trExclude').value,
    customCss: $('trCss').value,
    auto: ccTrans.hostList($('trAuto').value),
    never: ccTrans.hostList($('trNever').value),
    sites: ccTrans.parseSites($('trSites').value),
  })
  await loadTransCfg()
  msg('网页翻译设置已保存', 'ok')
}
$('clearTrCache').onclick = async (e) => {
  e.preventDefault()
  await api.runtime.sendMessage({ type: 'cc-tr-clear' }).catch(() => {})
  msg('已清除翻译缓存', 'ok')
}

// ---------- 写作浮窗 ----------
async function loadWriteCfg() {
  const { writeCfg } = await api.storage.local.get('writeCfg')
  const c = ccWrite.normalizeCfg(writeCfg)
  $('wrTrigger').value = c.trigger
  $('wrRoute').value = c.route
  $('wrCands').value = String(c.candidates)
  $('wrEmpty').checked = c.showEmpty
  $('wrClick').checked = c.runOnClick
  $('wrOff').value = c.offHosts.join('\n')
}
$('saveWrite').onclick = async () => {
  const { writeCfg } = await api.storage.local.get('writeCfg')
  const cur = ccWrite.normalizeCfg(writeCfg)
  const next = ccWrite.normalizeCfg({
    ...cur,
    trigger: $('wrTrigger').value,
    route: $('wrRoute').value,
    candidates: Number($('wrCands').value),
    showEmpty: $('wrEmpty').checked,
    runOnClick: $('wrClick').checked,
    offHosts: $('wrOff').value.split(/\s+/),
  })
  await api.storage.local.set({ writeCfg: next })
  await loadWriteCfg()
  msg('写作浮窗设置已保存', 'ok')
}
$('clearWrHist').onclick = async (e) => {
  e.preventDefault()
  await api.storage.local.remove('writeHistory')
  msg('已清除写作历史', 'ok')
}

let videoCfgDraft = ccVideo.normalizeCfg()
async function renderVideoCfg() {
  const { videoCfg, shotFormat } = await api.storage.local.get(['videoCfg', 'shotFormat'])
  videoCfgDraft = ccVideo.normalizeCfg(videoCfg)
  $('vRateStep').value = videoCfgDraft.rateStep
  $('vSeekStep').value = videoCfgDraft.seekStep
  $('vShotFmt').value = shotFormat === 'jpg' ? 'jpg' : 'png'
  const box = $('videoKeys')
  box.innerHTML = ''
  for (const a of ccVideo.ACTIONS) {
    const row = document.createElement('div')
    row.className = 'row spread'
    const n = document.createElement('span')
    n.textContent = a.name
    const inp = document.createElement('input')
    inp.readOnly = true
    inp.style.width = '80px'
    inp.value = videoCfgDraft.keys[a.id]
    inp.dataset.id = a.id
    inp.onkeydown = (e) => {
      if (e.key === 'Tab' || e.key === 'Shift') return
      e.preventDefault()
      if (e.ctrlKey || e.metaKey || e.altKey) return
      videoCfgDraft.keys[a.id] = e.key.length === 1 ? e.key.toLowerCase() : e.key
      inp.value = videoCfgDraft.keys[a.id]
      checkVideoKeys()
    }
    row.append(n, inp)
    box.append(row)
  }
  checkVideoKeys()
}
function checkVideoKeys() {
  const c = ccVideo.findConflicts(videoCfgDraft.keys)
  const names = Object.fromEntries(ccVideo.ACTIONS.map((a) => [a.id, a.name]))
  const txt = Object.entries(c).map(([k, ids]) => `“${k}”被 ${ids.map((i) => names[i]).join('、')} 同时使用`).join('；')
  $('videoKeyWarn').textContent = txt
  $('videoKeyWarn').classList.toggle('hidden', !txt)
  for (const i of $('videoKeys').querySelectorAll('input')) i.style.borderColor = c[i.value] ? 'var(--err)' : ''
  return !txt
}
$('saveVideo').onclick = async () => {
  if (!checkVideoKeys()) return msg('有快捷键重复，请先改掉', 'err')
  const old = (await api.storage.local.get('videoCfg')).videoCfg
  const keep = ccVideo.normalizeCfg(old)
  await api.storage.local.set({
    videoCfg: ccVideo.normalizeCfg({ ...videoCfgDraft, rateStep: $('vRateStep').value, seekStep: $('vSeekStep').value, skipIntro: keep.skipIntro, skipOutro: keep.skipOutro }),
    shotFormat: $('vShotFmt').value,
  })
  msg('已保存播放设置', 'ok')
  renderVideoCfg()
}
$('resetVideo').onclick = async (e) => {
  e.preventDefault()
  videoCfgDraft.keys = { ...ccVideo.DEFAULT_KEYS }
  for (const i of $('videoKeys').querySelectorAll('input')) i.value = videoCfgDraft.keys[i.dataset.id]
  checkVideoKeys()
}
