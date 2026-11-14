// 弹窗 / 设置页（popup.html）的拆分脚本：和 popup.js 共用同一个全局作用域（普通脚本，按 popup.html 里的顺序加载），
// 这里只放：站点权限、Cookie 同步、已发现的媒体、接管下载的表单。
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
