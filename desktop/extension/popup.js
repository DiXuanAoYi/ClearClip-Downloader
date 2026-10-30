const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }

function show(screen) {
  // screen：'main' | 'settings' | 'takeover'（兼容旧调用：true = settings，false = main）
  const name = screen === true ? 'settings' : screen === false ? 'main' : screen
  $('main').classList.toggle('hidden', name !== 'main')
  $('settings').classList.toggle('hidden', name !== 'settings')
  $('takeover').classList.toggle('hidden', name !== 'takeover')
  // 设置页在独立标签页里打开时放宽，弹窗里保持窄
  document.body.classList.toggle('wide', name !== 'main' && window.innerWidth > 420)
  if (name !== 'main') stopTaskPolling()
  else if (currentTabName === 'downloads') startTaskPolling()
}

let currentTabName = 'discover'

function showTab(name) {
  currentTabName = name
  for (const b of document.querySelectorAll('#tabs button')) b.classList.toggle('on', b.dataset.tab === name)
  for (const v of document.querySelectorAll('.view')) v.classList.toggle('hidden', v.id !== `v-${name}`)
  if (name === 'downloads') startTaskPolling()
  else stopTaskPolling()
  if (name === 'tools') renderTools()
  if (name === 'later' && typeof renderLater === 'function') renderLater()
}

async function currentTab() {
  const [tab] = await api.tabs.query({ active: true, currentWindow: true })
  return tab
}

async function refreshConn() {
  const r = await checkConnection()
  $('conn').textContent = r.ok ? r.message : `未连接：${r.message}`
  $('conn').className = r.ok ? 'mute ok' : 'mute err'
  return r.ok
}

async function tabSite() {
  const tab = await currentTab()
  return tab?.url?.startsWith('http') ? registrable(new URL(tab.url).hostname) : null
}

async function renderSync() {
  const c = await getConfig()
  const site = await tabSite()
  $('syncInfo').textContent = site && c.lastSync[site] ? `${site} 上次同步：${timeAgo(c.lastSync[site])}` : ''
  $('autoSite').checked = !!site && c.autoSync && c.syncSites.includes(site)
  $('autoSite').disabled = !site
  $('autoSync').checked = !!c.autoSync
  $('sites').innerHTML = ''
  if (!c.syncSites.length) $('sites').textContent = '还没有自动同步的网站。在网站上打开扩展，勾选“登录状态变化时自动同步此网站”即可添加。'
  for (const s of c.syncSites) {
    const row = document.createElement('div')
    const name = document.createElement('span')
    name.textContent = c.lastSync[s] ? `${s}（${timeAgo(c.lastSync[s])}同步）` : s
    const del = document.createElement('a')
    del.textContent = '移除'
    del.onclick = async () => {
      await api.storage.local.set({ syncSites: c.syncSites.filter((x) => x !== s) })
      renderSync()
    }
    row.append(name, del)
    $('sites').append(row)
  }
}

const picked = new Set()

async function sendItems(items, tab) {
  const c = await getConfig()
  let ok = 0
  let pendingPair = false
  for (const item of items) {
    try {
      const r = item.kind === 'image' ? await call('/api/grab', grabPayload({ url: item.url, referrer: tab.url }, cookieHeader(await api.cookies.getAll({ url: item.url }).catch(() => [])), navigator.userAgent, browserName())) : await call('/api/send', { text: sniffedPayload(item, tab.url, tab.title), device: c.device, name: browserName() })
      if (r.state === 'pending_pair') pendingPair = true
      else ok++
    } catch (e) {
      msg(e.message, 'err')
      return
    }
  }
  msg(pendingPair ? '第一次使用：请在电脑上点“允许”完成配对' : `已发送 ${ok} 项，清影会开始下载`, 'ok')
}

async function renderSniffed() {
  const tab = await currentTab()
  const list = tab?.id != null ? arrangeSniffed(await getSniffed(tab.id)) : []
  $('sniffBox').classList.toggle('hidden', !list.length)
  $('sniffCount').textContent = list.length
  $('sniffList').innerHTML = ''
  for (const item of list) {
    const row = document.createElement('div')
    row.className = 'it'
    const box = document.createElement('input')
    box.type = 'checkbox'
    box.checked = picked.has(item.url)
    box.onchange = () => {
      box.checked ? picked.add(item.url) : picked.delete(item.url)
      $('sniffSend').textContent = picked.size ? `发送所选（${picked.size}）` : '发送所选'
    }
    const name = document.createElement('span')
    let path = item.url
    try {
      path = new URL(item.url).pathname.split('/').filter(Boolean).pop() || new URL(item.url).hostname
    } catch {
      // keep the raw url
    }
    const label = document.createElement('span')
    label.textContent = sniffLabel(item)
    const file = document.createElement('span')
    file.className = 'sub'
    file.textContent = path
    name.append(label, file)
    name.title = item.url
    const btn = document.createElement('button')
    btn.textContent = '发送'
    btn.onclick = async () => {
      btn.disabled = true
      try {
        await sendItems([item], tab)
      } finally {
        btn.disabled = false
      }
    }
    row.append(box, name, btn)
    $('sniffList').append(row)
  }
  $('sniffAll').onclick = () => {
    const all = list.every((x) => picked.has(x.url))
    for (const x of list) all ? picked.delete(x.url) : picked.add(x.url)
    renderSniffed()
  }
  $('sniffSend').textContent = picked.size ? `发送所选（${picked.size}）` : '发送所选'
  $('sniffSend').onclick = async () => {
    const items = list.filter((x) => picked.has(x.url))
    if (!items.length) return msg('先勾选要发送的项目', 'err')
    $('sniffSend').disabled = true
    try {
      await sendItems(items, tab)
    } finally {
      $('sniffSend').disabled = false
    }
  }
}

// ---------- 接管浏览器下载 ----------

async function renderTakeover() {
  const cfg = await getTakeover()
  const { pausedUntil = 0 } = await api.storage.local.get('pausedUntil')
  $('tkOn').checked = cfg.enabled
  const info = $('tkInfo')
  info.textContent = ''
  if (cfg.enabled) {
    const paused = Date.now() < pausedUntil
    info.append(paused ? `已暂停到 ${new Date(pausedUntil).toLocaleTimeString()}。` : '已开启。按住 Alt 点击可临时用浏览器下载。')
    const a = document.createElement('a')
    a.className = 'lnk'
    a.textContent = paused ? ' 恢复接管' : ' 暂停 30 分钟'
    a.onclick = async () => {
      await api.storage.local.set({ pausedUntil: paused ? 0 : Date.now() + 30 * 60000 })
      renderTakeover()
    }
    info.append(a)
  }
  const { recentGrabs = [] } = await api.storage.local.get('recentGrabs')
  $('recentBox').classList.toggle('hidden', !recentGrabs.length)
  $('recent').innerHTML = ''
  for (const g of recentGrabs.slice(0, 5)) {
    const row = document.createElement('div')
    row.className = 'it'
    const name = document.createElement('span')
    name.textContent = `${g.ok ? '' : '（失败）'}${g.name || shortUrl(g.url)}`
    name.title = g.error || shortUrl(g.url)
    const btn = document.createElement('button')
    btn.className = 'sec'
    btn.textContent = '用浏览器下载'
    btn.onclick = () => api.downloads.download({ url: g.url, conflictAction: 'uniquify' }).then(() => msg('已交给浏览器下载', 'ok'), (e) => msg(e.message, 'err'))
    row.append(name, btn)
    $('recent').append(row)
  }
}

async function loadTakeoverForm() {
  const cfg = await getTakeover()
  $('tkEnabled').checked = cfg.enabled
  $('tkMode').value = cfg.mode
  $('tkTypes').value = cfg.types.join(', ')
  $('tkMin').value = cfg.minSizeMB
  $('tkAllow').value = cfg.allowSites.join('\n')
  $('tkDeny').value = cfg.denySites.join('\n')
  $('tkNotify').checked = cfg.notify
}

// ---------- 下载标签 ----------

let taskTimer = null
let taskBusy = false
let taskTick = 0
let loginSites = []
const taskRows = new Map()

function stopTaskPolling() {
  clearInterval(taskTimer)
  taskTimer = null
}

function startTaskPolling() {
  stopTaskPolling()
  taskTick = 0
  loadTasks()
  taskTimer = setInterval(loadTasks, 1000)
}

async function loadTasks() {
  if (taskBusy) return
  taskBusy = true
  try {
    const tasks = (await call('/api/v1/tasks')).tasks || []
    if (taskTick++ % 5 === 0) loginSites = await call('/api/v1/login_needed').then((r) => r.sites || [], () => [])
    renderTasks(tasks)
  } catch (e) {
    $('dlSummary').textContent = ''
    $('dlList').innerHTML = ''
    taskRows.clear()
    $('dlEmpty').textContent = e.message
    $('dlEmpty').classList.remove('hidden')
    $('dlToggleAll').classList.add('hidden')
  } finally {
    taskBusy = false
  }
}

const ACTION_LABEL = { pause: '暂停', resume: '继续', reveal: '打开文件夹', login: '同步登录' }

async function taskAction(task, action) {
  try {
    if (action === 'login') {
      const entry = loginSites.find((s) => s.site === task.site) || loginSites[0]
      if (!entry) return msg('找不到这个任务的网站地址，请在网站上打开扩展再同步', 'err')
      const r = await syncCookies(entry.url)
      msg(`已同步 ${r.count} 个 Cookie（${r.site}）${r.retried ? `，已重新开始 ${r.retried} 个任务` : ''}`, 'ok')
      loginSites = loginSites.filter((s) => s !== entry)
    } else {
      await call(`/api/v1/tasks/${task.id}/${action}`, {})
    }
    loadTasks()
  } catch (e) {
    msg(e.message, 'err')
  }
}

function makeTaskRow(id) {
  const el = document.createElement('div')
  el.className = 'task'
  const nm = document.createElement('div')
  nm.className = 'nm'
  const bar = document.createElement('div')
  bar.className = 'bar'
  bar.append(document.createElement('i'))
  const act = document.createElement('div')
  act.className = 'act'
  act.append(document.createElement('span'))
  el.append(nm, bar, act)
  return { id, el, nm, bar, act, line: act.firstChild, sig: '' }
}

function renderTasks(tasks) {
  const sum = summarizeTasks(tasks)
  const list = arrangeTasks(tasks)
  $('dlSummary').textContent = list.length ? `进行中 ${sum.running + sum.queued} · 暂停 ${sum.paused} · 失败 ${sum.failed}` : ''
  $('dlFoot').textContent = sum.speed ? `↓ ${fmtBytes(sum.speed)}/s` : ''
  const toggle = $('dlToggleAll')
  const canPause = sum.running + sum.queued > 0
  toggle.classList.toggle('hidden', !canPause && !sum.paused)
  toggle.textContent = canPause ? '全部暂停' : '全部继续'
  toggle.dataset.act = canPause ? 'pause_all' : 'resume_all'
  $('dlEmpty').textContent = '清影里没有下载任务。'
  $('dlEmpty').classList.toggle('hidden', list.length > 0)

  const seen = new Set()
  let prev = null
  for (const t of list) {
    const d = describeTask(t)
    seen.add(t.id)
    let row = taskRows.get(t.id)
    if (!row) {
      row = makeTaskRow(t.id)
      taskRows.set(t.id, row)
    }
    row.el.className = `task ${d.state}`
    row.nm.textContent = d.name
    row.nm.title = d.name
    row.bar.classList.toggle('hidden', d.pct == null || d.state === 'failed')
    row.bar.firstChild.style.width = `${d.pct ?? 0}%`
    row.line.textContent = d.line
    row.line.className = d.state === 'failed' ? 'bad' : ''
    const sig = d.actions.join(',')
    if (sig !== row.sig) {
      row.sig = sig
      row.act.querySelectorAll('button').forEach((b) => b.remove())
      for (const a of d.actions) {
        const b = document.createElement('button')
        b.className = a === 'login' ? 'small' : 'sec small'
        b.textContent = ACTION_LABEL[a]
        b.onclick = () => taskAction(t, a)
        row.act.append(b)
      }
    }
    // 保持顺序：只在位置不对时才移动，避免打断点击
    const at = prev ? prev.el.nextSibling : $('dlList').firstChild
    if (at !== row.el) $('dlList').insertBefore(row.el, at)
    prev = row
  }
  for (const [id, row] of taskRows) {
    if (!seen.has(id)) {
      row.el.remove()
      taskRows.delete(id)
    }
  }
}

// ---------- 工具标签 ----------

let toolsUrl = ''

async function renderTools() {
  const tab = await currentTab()
  const ok = !!tab?.url?.startsWith('http')
  const f = await getFeatures()
  const rules = await getSiteRules()
  const extra = rules.extraParams.split(/[\s,]+/).filter(Boolean)
  const r = ok ? cleanUrl(tab.url, extra) : { url: '', removed: [] }
  toolsUrl = r.url
  const before = $('cleanBefore')
  before.textContent = ''
  if (!ok) {
    before.textContent = '当前页面没有可用的网址'
  } else if (!r.removed.length && !r.unwrapped) {
    before.textContent = r.url
  } else {
    // 原链接里被去掉的参数划线显示
    const u = new URL(tab.url)
    const removed = new Set(r.removed)
    const parts = u.search.length > 1 ? u.search.slice(1).split('&') : []
    before.append(`${u.origin}${u.pathname}`)
    parts.forEach((pair, i) => {
      before.append(i === 0 ? '?' : '&')
      let key = pair.split('=')[0]
      try { key = decodeURIComponent(key) } catch { /* 保持原样 */ }
      if (removed.has(key)) {
        const del = document.createElement('del')
        del.textContent = pair
        before.append(del)
      } else {
        before.append(pair)
      }
    })
  }
  $('cleanInfo').textContent = !ok ? '' : r.unwrapped ? '已还原跳转链接' : r.removed.length ? `去掉了 ${r.removed.length} 个追踪参数` : '没有需要去掉的参数'
  $('copyClean').disabled = !ok
  $('toPhone').disabled = !ok
  const svg = ok ? qrSvg(toolsUrl) : null
  $('qrBox').innerHTML = ''
  if (svg) {
    const img = document.createElement('img')
    img.alt = '二维码'
    img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
    $('qrBox').append(img)
  } else if (ok) {
    $('qrBox').textContent = '链接太长，放不进二维码'
  }
  await renderSiteToggles(tab)
  const pageSave = $('pageSave')
  if (pageSave && typeof wirePageSave === 'function') pageSave.classList.toggle('hidden', !(await getFeatures()).onDemand)
}

const SITE_TOGGLES = [
  { id: 'floatButton', name: '不显示视频悬浮下载按钮', get: (r, h) => hostListed(r.off.floatButton, h), set: (r, h, on) => (r.off.floatButton = toggleHost(r.off.floatButton, h, on)) },
  { id: 'video', name: '不启用播放增强', get: (r, h) => hostListed(r.off.video, h), set: (r, h, on) => (r.off.video = toggleHost(r.off.video, h, on)) },
  { id: 'hoverZoom', name: '不启用图片悬停放大', get: (r, h) => hostListed(r.off.hoverZoom, h), set: (r, h, on) => (r.off.hoverZoom = toggleHost(r.off.hoverZoom, h, on)) },
  { id: 'unlock', name: '解除复制 / 右键限制', get: (r, h) => r.unlockAll || hostListed(r.unlockOn, h), set: (r, h, on) => (r.unlockOn = toggleHost(r.unlockOn, h, on)) },
]

function toggleHost(list, host, on) {
  const rest = list.filter((x) => x !== host && !host.endsWith('.' + x))
  return on ? [...rest, host] : rest
}

async function renderSiteToggles(tab) {
  const box = $('siteToggles')
  box.innerHTML = ''
  let host = ''
  try {
    host = tab?.url?.startsWith('http') ? new URL(tab.url).hostname.toLowerCase() : ''
  } catch { /* 没有主机名 */ }
  $('siteName').textContent = host || '（当前页面不可用）'
  if (!host) return
  const rules = await getSiteRules()
  for (const t of SITE_TOGGLES) {
    const row = document.createElement('label')
    row.className = 'sw'
    const name = document.createElement('div')
    name.className = 'n'
    name.textContent = t.name
    const box2 = document.createElement('input')
    box2.type = 'checkbox'
    box2.checked = t.get(rules, host)
    box2.disabled = t.id === 'unlock' && rules.unlockAll
    box2.onchange = async () => {
      const cur = await getSiteRules()
      t.set(cur, host, box2.checked)
      await api.storage.local.set({ siteRules: cur })
      if (t.id === 'unlock' && box2.checked) {
        // 给这个网站明确打开了：功能总开关一并打开，并在当前页面立刻生效
        const f = await getFeatures()
        if (!f.unlock) await api.storage.local.set({ features: { ...f, unlock: true } })
        const r = await api.runtime.sendMessage({ type: 'cc-unlock-now', tabId: tab.id }).catch(() => null)
        return msg(r?.ok ? '已在此网站解除复制 / 右键限制，当前页面已生效' : '已记住，刷新页面后生效', 'ok')
      }
      msg(box2.checked ? '已记住，刷新页面后生效' : '已取消，刷新页面后生效', 'ok')
    }
    row.append(name, box2)
    box.append(row)
  }
}

// ---------- 设置页 ----------

const lineList = (id) => $(id).value.split(/[\s,;]+/).map((x) => x.trim().toLowerCase()).filter(Boolean)

async function loadSettings() {
  const c = await getConfig()
  $('port').value = c.port || ''
  $('token').value = c.token || ''
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

// ---------- 稍后看 ----------

async function renderLater() {
  const list = await getLater()
  const box = $('laterList')
  box.innerHTML = ''
  $('laterEmpty').classList.toggle('hidden', list.length > 0)
  const unseen = laterUnseen(list)
  $('laterBadge').textContent = unseen
  $('laterBadge').classList.toggle('hidden', !unseen)
  for (const it of list) {
    const row = document.createElement('div')
    row.className = 'it'
    if (it.seen) row.style.opacity = '.6'
    const text = document.createElement('span')
    const title = document.createElement('b')
    title.textContent = it.title || it.url
    title.style.fontWeight = it.seen ? '400' : '700'
    const sub = document.createElement('span')
    sub.className = 'sub'
    let host = it.url
    try {
      host = new URL(it.url).hostname
    } catch {
      // keep the raw url
    }
    sub.textContent = `${host} · ${timeAgo(it.at)}`
    text.title = it.url
    text.append(title, sub)
    const dl = document.createElement('button')
    dl.textContent = '下载'
    dl.onclick = async () => {
      try {
        const r = await sendUrl(it.url)
        await saveLater(laterSeen(await getLater(), it.url))
        msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送，清影会开始解析', 'ok')
        renderLater()
      } catch (e) {
        msg(e.message, 'err')
      }
    }
    const seen = document.createElement('button')
    seen.className = 'sec'
    seen.textContent = it.seen ? '未看' : '已看'
    seen.onclick = async () => {
      await saveLater(laterSeen(await getLater(), it.url, !it.seen))
      renderLater()
    }
    const open = document.createElement('button')
    open.className = 'sec'
    open.textContent = '打开'
    open.onclick = () => api.tabs.create({ url: it.url })
    const del = document.createElement('button')
    del.className = 'sec'
    del.textContent = '删除'
    del.onclick = async () => {
      await saveLater(laterRemove(await getLater(), it.url))
      renderLater()
    }
    row.append(text, dl, open, seen, del)
    box.append(row)
  }
}

async function refreshLaterBadge() {
  const unseen = laterUnseen(await getLater())
  $('laterBadge').textContent = unseen
  $('laterBadge').classList.toggle('hidden', !unseen)
}

$('laterSave').onclick = async () => {
  const tab = await currentTab()
  try {
    const r = await addLater({ url: tab?.url, title: tab?.title, icon: tab?.favIconUrl })
    if (r.invalid) return msg('当前页面不能加入稍后看', 'err')
    msg(r.added ? '已加入稍后看' : '稍后看里已经有这一页了，已提到最前面', 'ok')
    renderLater()
  } catch (e) {
    msg(e.message, 'err')
  }
}

// ---------- 页面保存 ----------

async function savePageAction(action) {
  const tab = await currentTab()
  if (!tab?.url?.startsWith('http')) return msg('当前页面不能使用这个功能', 'err')
  msg(action === 'shotFull' ? '正在滚动截图，请不要操作页面……' : '处理中……')
  const r = await api.runtime.sendMessage({ type: 'cc-page-save', action, tabId: tab.id }).catch((e) => ({ ok: false, error: e.message }))
  if (!r?.ok) return msg(r?.error || '没有完成', 'err')
  if (r.pairing) return msg('第一次使用：请在电脑上点“允许”完成配对', 'ok')
  const done = { shotVisible: '截图已保存到下载文件夹', shotArea: '', shotFull: '整页截图已保存到下载文件夹', pdf: '', md: '', clip: '已摘录到清影，在“收到的文件”里查看' }[action]
  msg(r.note || done, 'ok')
  // 选区截图、Markdown 预览都需要用户回到页面 / 新标签页：关掉弹窗
  if (action === 'shotArea' || action === 'md' || action === 'pdf') window.close()
}

function wirePageSave() {
  for (const [id, action] of Object.entries({ shotVisible: 'shotVisible', shotArea: 'shotArea', shotFull: 'shotFull', savePdf: 'pdf', saveMd: 'md', clipText: 'clip' })) {
    $(id).onclick = () => savePageAction(action)
  }
}
wirePageSave()

async function saveRules() {
  const rules = normalizeSiteRules({
    off: { floatButton: lineList('ruleFloat'), video: lineList('ruleVideo'), hoverZoom: lineList('ruleZoom') },
    unlockOn: lineList('ruleUnlock'),
    unlockAll: $('unlockAll').checked,
    zoomRules: $('ruleZoomRules').value,
    extraParams: $('ruleParams').value,
  })
  const text = $('ruleEngines').value.trim()
  const lines = text.split(/\r?\n/).filter((l) => l.trim())
  const parsed = text ? parseEngines(text) : []
  const bad = text ? lines.length - lines.filter((l) => /\|/.test(l)).length : 0
  const same = text === enginesToText(DEFAULT_ENGINES)
  await api.storage.local.set({ siteRules: rules })
  if (!text || same) await api.storage.local.remove('engines')
  else await api.storage.local.set({ engines: enginesToText(parsed) })
  msg(bad ? `已保存。有 ${bad} 行引擎格式不对（要有“名称 | 地址”，地址里要有 {url}），已忽略。` : '已保存', bad ? 'err' : 'ok')
  loadSettings()
}

async function init() {
  const c = await getConfig()
  const tab = await currentTab()
  $('page').textContent = tab?.title || tab?.url || ''
  const f = await getFeatures()
  $('pageTools').classList.toggle('hidden', !f.onDemand)
  $('tabLater').classList.toggle('hidden', !(f.readLater && typeof renderLater === 'function'))
  await loadSettings()
  refreshLaterBadge()
  show(location.hash === '#settings' || !c.port || !c.token ? 'settings' : 'main')
  renderSniffed()
  renderTakeover()
  refreshConn()
}

for (const b of document.querySelectorAll('#tabs button')) b.onclick = () => showTab(b.dataset.tab)

$('dlToggleAll').onclick = async () => {
  try {
    await call(`/api/v1/tasks/${$('dlToggleAll').dataset.act}`, {})
    loadTasks()
  } catch (e) {
    msg(e.message, 'err')
  }
}
$('openApp').onclick = async (e) => {
  e.preventDefault()
  try {
    await call('/api/v1/show', {})
  } catch (err) {
    msg(err.message, 'err')
  }
}

$('copyClean').onclick = async () => {
  try {
    await navigator.clipboard.writeText(toolsUrl)
    msg('已复制干净链接', 'ok')
  } catch {
    msg('复制失败，请手动选中上面的链接', 'err')
  }
}
$('toPhone').onclick = async () => {
  try {
    const c = await getConfig()
    const r = await call('/api/to_phone', { text: toolsUrl, device: c.device })
    msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发到手机盒子，手机上打开清影网页即可看到', 'ok')
  } catch (e) {
    msg(e.message, 'err')
  }
}
async function runOnPage(action) {
  const tab = await currentTab()
  if (!tab?.url?.startsWith('http')) return msg('当前页面不能使用这个功能', 'err')
  const r = await api.runtime.sendMessage({ type: 'cc-run', action, tabId: tab.id }).catch((e) => ({ ok: false, error: e.message }))
  if (!r?.ok) return msg(r?.error || '没有完成', 'err')
  window.close()
}
$('openOverview').onclick = () => runOnPage('overview')
$('startPick').onclick = () => runOnPage('picker')
$('openSettings').onclick = () => show('settings')
$('saveRules').onclick = saveRules
$('notifyDone').onchange = () => api.storage.local.set({ notifyDone: $('notifyDone').checked })

$('autoSite').onchange = async () => {
  const site = await tabSite()
  if (!site) return
  const c = await getConfig()
  const sites = c.syncSites.filter((s) => s !== site)
  if ($('autoSite').checked) {
    sites.push(site)
    await api.storage.local.set({ syncSites: sites, autoSync: true })
    // 勾选时先同步一次
    try {
      const tab = await currentTab()
      const r = await syncCookies(tab.url)
      msg(`已开启自动同步，并同步了 ${r.count} 个 Cookie（${r.site}）`, 'ok')
    } catch (e) {
      msg(`已开启自动同步。本次同步失败：${e.message}`, 'err')
    }
  } else {
    await api.storage.local.set({ syncSites: sites })
    msg('已关闭此网站的自动同步')
  }
  renderSync()
}

$('autoSync').onchange = async () => {
  await api.storage.local.set({ autoSync: $('autoSync').checked })
  renderSync()
}

$('send').onclick = async () => {
  const tab = await currentTab()
  if (!tab?.url?.startsWith('http')) return msg('当前页面不能发送', 'err')
  msg('发送中…')
  try {
    const r = await sendUrl(await tidyUrl(tab.url))
    msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送，清影会开始解析下载', 'ok')
  } catch (e) {
    msg(e.message, 'err')
  }
}

$('cookies').onclick = async () => {
  const tab = await currentTab()
  if (!tab?.url?.startsWith('http')) return msg('当前页面不能同步', 'err')
  msg('同步中…')
  try {
    const r = await syncCookies(tab.url)
    msg(`已同步 ${r.count} 个 Cookie 到清影（${r.site}）。之前因需要登录而失败的链接会自动重试。`, 'ok')
    renderSync()
  } catch (e) {
    msg(e.message, 'err')
  }
}

$('back').onclick = () => show('main')
$('save').onclick = async () => {
  await api.storage.local.set({ port: Number($('port').value) || null, token: $('token').value.trim() })
  if (await refreshConn()) {
    msg('设置已保存', 'ok')
    show('main')
  } else {
    msg('设置已保存，但连接不到清影。请确认清影正在运行、已开启“手机与浏览器扩展”，端口和令牌正确。', 'err')
  }
}

$('tkOn').onchange = async () => {
  const cfg = await getTakeover()
  await api.storage.local.set({ takeover: { ...cfg, enabled: $('tkOn').checked } })
  renderTakeover()
}
$('openTakeover').onclick = async () => {
  await loadTakeoverForm()
  show('takeover')
}
$('tkBack').onclick = () => {
  show('main')
  renderTakeover()
}
$('tkDefaultTypes').onclick = () => {
  $('tkTypes').value = TAKEOVER_TYPES.join(', ')
}
$('tkSave').onclick = async () => {
  const lines = (id) => $(id).value.split(/[\s,;]+/)
  await api.storage.local.set({
    takeover: normalizeTakeover({
      enabled: $('tkEnabled').checked,
      mode: $('tkMode').value,
      types: lines('tkTypes'),
      minSizeMB: $('tkMin').value === '' ? 1 : Number($('tkMin').value),
      allowSites: lines('tkAllow'),
      denySites: lines('tkDeny'),
      notify: $('tkNotify').checked,
    }),
  })
  msg('已保存', 'ok')
  show('main')
  renderTakeover()
}

init()
// 设置页已经开着时，右键菜单再次要求配置：切到设置界面
window.addEventListener('hashchange', () => {
  if (location.hash === '#settings') init()
})
