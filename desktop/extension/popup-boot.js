// 弹窗 / 设置页（popup.html）的拆分脚本：和 popup.js 共用同一个全局作用域（普通脚本，按 popup.html 里的顺序加载），
// 这里只放：启动：初始化、按钮和快捷键的绑定、提示条、存储变化监听。
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
