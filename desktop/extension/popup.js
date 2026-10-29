const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }

function show(settings) {
  $('main').classList.toggle('hidden', settings)
  $('settings').classList.toggle('hidden', !settings)
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

function sizeText(n) {
  if (!n) return ''
  return n >= 1048576 ? ` · ${(n / 1048576).toFixed(1)} MB` : ` · ${Math.round(n / 1024)} KB`
}

async function renderSniffed() {
  const tab = await currentTab()
  const list = tab?.id != null ? await getSniffed(tab.id) : []
  $('sniffBox').classList.toggle('hidden', !list.length)
  $('sniffCount').textContent = list.length
  $('sniffList').innerHTML = ''
  for (const item of list) {
    const row = document.createElement('div')
    row.className = 'it'
    const name = document.createElement('span')
    let path = item.url
    try {
      path = new URL(item.url).pathname.split('/').filter(Boolean).pop() || new URL(item.url).hostname
    } catch {
      // keep the raw url
    }
    name.textContent = `${KIND_NAME[item.kind] || item.kind}${sizeText(item.size)} · ${path}`
    name.title = item.url
    const btn = document.createElement('button')
    btn.textContent = '发送'
    btn.onclick = async () => {
      btn.disabled = true
      try {
        const r = await call('/api/send', { text: sniffedPayload(item, tab.url, tab.title), device: (await getConfig()).device, name: browserName() })
        msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送，清影会开始下载', 'ok')
      } catch (e) {
        msg(e.message, 'err')
      } finally {
        btn.disabled = false
      }
    }
    row.append(name, btn)
    $('sniffList').append(row)
  }
}

async function init() {
  const c = await getConfig()
  $('port').value = c.port || ''
  $('token').value = c.token || ''
  const tab = await currentTab()
  $('page').textContent = tab?.title || tab?.url || ''
  show(location.hash === '#settings' || !c.port || !c.token)
  renderSync()
  renderSniffed()
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

$('openSettings').onclick = () => show(true)
$('back').onclick = () => show(false)
$('save').onclick = async () => {
  await api.storage.local.set({ port: Number($('port').value) || null, token: $('token').value.trim() })
  if (await refreshConn()) {
    msg('设置已保存', 'ok')
    show(false)
  } else {
    msg('设置已保存，但连接不到清影。请确认清影正在运行、已开启“手机与浏览器扩展”，端口和令牌正确。', 'err')
  }
}

init()
