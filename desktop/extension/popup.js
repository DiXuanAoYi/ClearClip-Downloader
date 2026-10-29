const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }

function show(screen) {
  // screen：'main' | 'settings' | 'takeover'（兼容旧调用：true = settings，false = main）
  const name = screen === true ? 'settings' : screen === false ? 'main' : screen
  $('main').classList.toggle('hidden', name !== 'main')
  $('settings').classList.toggle('hidden', name !== 'settings')
  $('takeover').classList.toggle('hidden', name !== 'takeover')
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
    name.title = shortUrl(g.url)
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

async function init() {
  const c = await getConfig()
  $('port').value = c.port || ''
  $('token').value = c.token || ''
  const tab = await currentTab()
  $('page').textContent = tab?.title || tab?.url || ''
  show(location.hash === '#settings' || !c.port || !c.token ? 'settings' : 'main')
  renderSync()
  renderSniffed()
  renderTakeover()
  refreshConn()
}

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
    const r = await sendUrl(tab.url)
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

$('openSettings').onclick = () => show('settings')
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
