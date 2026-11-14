// 弹窗 / 设置页（popup.html）的拆分脚本：和 popup.js 共用同一个全局作用域（普通脚本，按 popup.html 里的顺序加载），
// 这里只放：下载标签：任务列表与进度轮询、小工具、站点开关。
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
