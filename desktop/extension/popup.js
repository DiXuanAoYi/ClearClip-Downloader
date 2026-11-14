const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }

function show(screen) {
  // screen：'main' | 'settings' | 'takeover'（兼容旧调用：true = settings，false = main）
  const name = screen === true ? 'settings' : screen === false ? 'main' : screen
  $('main').classList.toggle('hidden', name !== 'main')
  $('settings').classList.toggle('hidden', name !== 'settings')
  $('takeover').classList.toggle('hidden', name !== 'takeover')
  if (name === 'settings') buildSettingsNav()
  // 设置页在独立标签页里打开时放宽，弹窗里保持窄
  document.body.classList.toggle('wide', name !== 'main' && window.innerWidth > 420)
  if (name !== 'main') stopTaskPolling()
  else if (currentTabName === 'downloads') startTaskPolling()
}

/** 设置页太长：按 <h2> 分成几个分区，顶部一排横向标签，一次只显示一个 */
let setNavBuilt = false
function buildSettingsNav() {
  if (setNavBuilt) return
  setNavBuilt = true
  const root = $('settings')
  const kids = [...root.children]
  const secs = []
  let cur = null
  for (const el of kids) {
    if (el.tagName === 'H2') {
      cur = document.createElement('div')
      cur.className = 'setsec'
      cur.dataset.name = el.textContent.trim()
      root.insertBefore(cur, el)
      secs.push(cur)
    }
    if (cur) cur.appendChild(el)
  }
  const bar = document.createElement('div')
  bar.id = 'setBar'
  const mk = (cls, label, text, fn) => {
    const x = document.createElement('button')
    x.type = 'button'
    x.className = cls
    x.setAttribute('aria-label', label)
    x.title = label
    x.textContent = text
    x.onclick = fn
    return x
  }
  const back = mk('setBack', '返回', '◀', () => show('main'))
  const nav = document.createElement('nav')
  nav.id = 'setNav'
  nav.setAttribute('role', 'tablist')
  nav.setAttribute('aria-label', '设置分区')
  const prev = mk('setArrow', '向左', '‹', () => nav.scrollBy({ left: -nav.clientWidth * 0.7, behavior: 'smooth' }))
  const next = mk('setArrow', '向右', '›', () => nav.scrollBy({ left: nav.clientWidth * 0.7, behavior: 'smooth' }))
  const fit = () => {
    const over = nav.scrollWidth > nav.clientWidth + 2
    prev.hidden = next.hidden = !over
    prev.disabled = nav.scrollLeft <= 1
    next.disabled = nav.scrollLeft + nav.clientWidth >= nav.scrollWidth - 1
  }
  nav.addEventListener('scroll', fit, { passive: true })
  // 滚轮上下滚也能横向移动导航
  nav.addEventListener('wheel', (e) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      nav.scrollLeft += e.deltaY
      e.preventDefault()
    }
  }, { passive: false })
  window.addEventListener('resize', fit)
  const pick = (name) => {
    for (const b of nav.children) {
      const on = b.dataset.name === name
      b.setAttribute('aria-selected', String(on))
      if (on) b.scrollIntoView({ block: 'nearest', inline: 'center' })
    }
    for (const s of secs) s.classList.toggle('on', s.dataset.name === name)
    try { sessionStorage.setItem('cc-set-sec', name) } catch {}
    fit()
  }
  for (const s of secs) {
    const b = document.createElement('button')
    b.type = 'button'
    b.setAttribute('role', 'tab')
    b.dataset.name = s.dataset.name
    b.textContent = s.dataset.name
    b.onclick = () => pick(s.dataset.name)
    nav.appendChild(b)
  }
  // 键盘：左右方向键在分区之间切换
  nav.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return
    const names = secs.map((s) => s.dataset.name)
    const cur = names.indexOf(sessionStorage.getItem('cc-set-sec') || names[0])
    const n = names[(cur + (e.key === 'ArrowRight' ? 1 : names.length - 1)) % names.length]
    pick(n)
    nav.querySelector('[aria-selected="true"]')?.focus()
  })
  bar.append(back, prev, nav, next)
  root.insertBefore(bar, secs[0])
  let saved = ''
  try { saved = sessionStorage.getItem('cc-set-sec') || '' } catch {}
  pick(secs.some((s) => s.dataset.name === saved) ? saved : secs[0]?.dataset.name)
  requestAnimationFrame(fit)
}

let currentTabName = 'discover'

function showTab(name) {
  if (currentTabName === 'audio' && name !== 'audio') globalThis.ccAudioPopup?.close()
  currentTabName = name
  for (const b of document.querySelectorAll('#tabs button')) b.classList.toggle('on', b.dataset.tab === name)
  for (const v of document.querySelectorAll('.view')) v.classList.toggle('hidden', v.id !== `v-${name}`)
  if (name === 'downloads') startTaskPolling()
  else stopTaskPolling()
  if (name === 'tools') renderTools()
  if (name === 'audio') globalThis.ccAudioPopup?.open()
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

// 权限申请必须在用户点击的处理函数里、而且前面不能先 await 别的：打开弹窗时先把当前网址记下来
let curUrl = ''
const siteOfUrl = (url) => (url?.startsWith('http') ? registrable(new URL(url).hostname) : null)

/** 同步 Cookie 前先申请“Cookie + 这个网站”的权限（已授权时不会弹框）。被拒绝时给出说明。 */
async function askCookiePerm(url) {
  const site = siteOfUrl(url)
  if (!site) return false
  if (await ccPerms.request(ccPerms.cookieRequest(site))) return true
  msg(`没有授权，无法同步 ${site} 的登录 Cookie。${ccPerms.TEXT.cookies}`, 'err')
  return false
}

/** 所有网页的访问权限：未授权时在“发现”页和设置页提示，一个按钮申请。 */
async function renderPerms() {
  const ok = await ccPerms.hasSites()
  $('permBox').classList.toggle('hidden', ok)
  $('permInfo').textContent = ok ? '已授权访问所有网页。' : ccPerms.TEXT.sites
  $('permSetBtn').classList.toggle('hidden', ok)
}
async function grantSites() {
  const ok = await ccPerms.request(ccPerms.sitesRequest())
  msg(ok ? '已授权访问网页，常驻功能马上生效' : '没有授权：常驻功能仍不可用。需要时再点“授权访问网页”', ok ? 'ok' : 'err')
  renderPerms()
  return ok
}
$('permBtn').onclick = grantSites
$('permSetBtn').onclick = grantSites

async function renderSync() {
  const c = await getConfig()
  const site = await tabSite()
  $('syncInfo').textContent = site && c.lastSync[site] ? `${site} 上次同步：${timeAgo(c.lastSync[site])}` : ''
  $('autoSite').checked = !!site && c.autoSync && c.syncSites.includes(site)
  $('autoSite').disabled = !site
  $('autoSync').checked = !!c.autoSync
  $('sites').innerHTML = ''
  if (!c.syncSites.length) $('sites').textContent = `自动同步${c.autoSync ? '已开启' : '未开启'}，但名单还是空的，所以暂时不会同步任何网站。到要同步的网站页面打开扩展，勾选“登录状态变化时自动同步此网站”，它就会出现在这里。`
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

// 检测到的视频流只显示个数；勾选发送在“页面媒体总览”里（以前这里还有一份同样的列表）
async function renderSniffed() {
  const tab = await currentTab()
  const list = tab?.id != null ? arrangeSniffed(await getSniffed(tab.id)) : []
  $('sniffBox').classList.toggle('hidden', !list.length)
  $('sniffCount').textContent = list.length
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
  $('streamSites').value = streamFirstSites((await api.storage.local.get('streamSites')).streamSites).join('\n')
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
      if (!(await askCookiePerm(entry.url))) return
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
  { id: 'floatButton', name: '视频胶囊里不显示“下载”', get: (r, h) => hostListed(r.off.floatButton, h), set: (r, h, on) => (r.off.floatButton = toggleHost(r.off.floatButton, h, on)) },
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
  const done = { shotVisible: '截图已保存到下载文件夹', shotArea: '', shotFull: '整页截图已保存到下载文件夹', pdf: '', md: '', clip: '已摘录到清影，在“生词与摘录 → 摘录”里查看' }[action]
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
  curUrl = tab?.url || ''
  $('page').textContent = tab?.title || tab?.url || ''
  const f = await getFeatures()
  $('pageTools').classList.toggle('hidden', !f.onDemand)
  $('vsumTools').classList.toggle('hidden', !f.vsum)
  $('subLayerTools').classList.toggle('hidden', !f.subLayer)
  $('transTools').classList.toggle('hidden', !f.webtrans)
  $('tabLater').classList.toggle('hidden', !(f.readLater && typeof renderLater === 'function'))
  await loadSettings()
  refreshLaterBadge()
  show(location.hash === '#settings' || !c.port || !c.token ? 'settings' : 'main')
  if (location.hash === '#audio') showTab('audio')
  renderSniffed()
  renderTakeover()
  renderPerms()
  refreshConn()
}

for (const b of document.querySelectorAll('#tabs button[data-tab]')) b.onclick = () => showTab(b.dataset.tab)

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
$('pickExclude').onclick = () => runOnPage('excludepick')
$('startVsum').onclick = () => runOnPage('vsum')
$('startSubLayer').onclick = () => runOnPage('sublayer')
$('startShot').onclick = () => runOnPage('shot')
$('openSettings').onclick = () => show('settings')
$('saveRules').onclick = saveRules
$('notifyDone').onchange = () => api.storage.local.set({ notifyDone: $('notifyDone').checked })

$('autoSite').onchange = async () => {
  const want = $('autoSite').checked
  if (want && !(await askCookiePerm(curUrl))) {
    $('autoSite').checked = false
    return
  }
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

$('noteLimit').onchange = async () => {
  const n = Math.max(0, Math.floor(Number($('noteLimit').value) || 0))
  $('noteLimit').value = n
  await api.storage.local.set({ noteLimit: n })
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
    const synced = await firstTimeSync(tab.url)
    const st = await streamForPage(tab)
    if (st && !st.item) return msg('这个网站要先在页面里把视频播放起来，扩展才能识别到视频流；播放后再点“发送当前页面”。', 'err')
    const r = st ? await sendUrl(sniffedPayload(st.item, tab.url, tab.title)) : await sendUrl(await tidyUrl(tab.url))
    msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : (synced ? `已发送，并首次同步了 ${synced.count} 个 Cookie（${synced.site}）` : '已发送，清影会开始解析下载'), 'ok')
  } catch (e) {
    msg(e.message, 'err')
  }
}

$('cookies').onclick = async () => {
  if (!(await askCookiePerm(curUrl))) return
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
  // 接管下载本身不需要网页权限；“按住 Alt 点击临时放行”要往网页里放一个小脚本，所以顺便申请（拒绝也照常开启）
  if ($('tkOn').checked && !(await ccPerms.hasSites())) await grantSites()
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
    streamSites: streamFirstSites(lines('streamSites')),
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

// ---------- 操作反馈：按钮按下有变化，设置改了有“已保存”提示 ----------
document.addEventListener('click', (e) => {
  const b = e.target instanceof Element ? e.target.closest('button') : null
  if (!b || b.closest('nav') || b.disabled) return
  b.classList.add('pressed')
  setTimeout(() => b.classList.remove('pressed'), 450)
})
let toastTimer = 0
function flashToast(text) {
  const t = document.getElementById('toast')
  if (!t) return
  t.textContent = text
  t.classList.remove('hidden')
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => t.classList.add('hidden'), 1600)
}
// 设置类的存储项变了（点了开关、选了下拉、保存了设置）：右下角提示一下，免得不知道生效没有
const WATCH = ['transCfg', 'features', 'videoCfg', 'audioCfg', 'siteRules', 'writeCfg', 'vsumPrefs', 'pausedUntil', 'noteLimit', 'appLang', 'autoSync', 'syncSites', 'engines']
api.storage.onChanged.addListener((ch, area) => {
  if (area === 'local' && WATCH.some((k) => k in ch)) flashToast('✓ 已保存，马上生效')
})
