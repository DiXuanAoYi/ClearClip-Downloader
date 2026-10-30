// 与清影联动的后台逻辑：下载进度角标和完成通知、“需要登录”时询问是否同步 Cookie。

const POLL_ALARM = 'cc-poll'
const ASK_COOLDOWN = 24 * 3600 * 1000
const BADGE_SNIFF = '#d9772f'
const BADGE_TASKS = '#2b6cb0'

let polling = false

/** 标签页图标上的角标：嗅探到的媒体数优先，没有就显示进行中的下载数。 */
async function paintBadge(tabId) {
  const action = api.action || api.browserAction
  if (tabId === undefined || tabId < 0) return
  try {
    const n = (await getSniffed(tabId)).filter((x) => x.kind !== 'image').length
    const { progressBadge = '' } = await sessionStore.get('progressBadge')
    await action.setBadgeText({ tabId, text: n ? String(n) : progressBadge })
    await action.setBadgeBackgroundColor({ tabId, color: n ? BADGE_SNIFF : BADGE_TASKS })
  } catch {
    // 标签页可能已关闭
  }
}

async function paintAllBadges() {
  try {
    for (const t of await api.tabs.query({})) await paintBadge(t.id)
  } catch {
    // ignore
  }
}

async function ensureAlarm() {
  try {
    await api.alarms.create(POLL_ALARM, { periodInMinutes: 0.5, delayInMinutes: 0.1 })
  } catch {
    // 没有 alarms 权限的旧环境：只靠弹窗刷新
  }
}

async function pollOnce() {
  if (polling) return
  polling = true
  try {
    const cfg = await getConfig()
    if (!cfg.port || !cfg.token) return
    const f = await getFeatures()
    if (f.progress) await pollProgress()
    if (f.loginAsk) await pollLogin(cfg)
  } finally {
    polling = false
  }
}

async function pollProgress() {
  let tasks
  try {
    tasks = (await call('/api/v1/tasks')).tasks || []
  } catch {
    // 清影没有运行：清掉角标，下次再试
    await sessionStore.set({ progressBadge: '' })
    await paintAllBadges()
    return
  }
  const sum = summarizeTasks(tasks)
  const text = badgeFor(sum)
  const { progressBadge = '' } = await sessionStore.get('progressBadge')
  if (text !== progressBadge) {
    await sessionStore.set({ progressBadge: text })
    await paintAllBadges()
  }
  const { doneSeen } = await sessionStore.get('doneSeen')
  const doneIds = tasks.filter((t) => t.status === 'done').map((t) => t.id)
  await sessionStore.set({ doneSeen: doneIds })
  if (!doneSeen) return // 第一次轮询只记录，不通知
  const { notifyDone = true } = await api.storage.local.get('notifyDone')
  if (!notifyDone) return
  for (const t of newlyDone(doneSeen, tasks).slice(0, 3)) {
    api.notifications.create(`cc-done:${t.id}`, { type: 'basic', iconUrl: 'icons/128.png', title: '下载完成', message: t.fileName || t.title || '' })
  }
}

/** 域名对应的 Cookie 条数（浏览器里有没有这个网站的登录状态）。 */
async function cookieCount(host) {
  try {
    return (await api.cookies.getAll({ domain: registrable(host) })).length
  } catch {
    return 0
  }
}

async function pollLogin(cfg) {
  let sites
  try {
    sites = (await call('/api/v1/login_needed')).sites || []
  } catch {
    return
  }
  const { loginAsked = {} } = await api.storage.local.get('loginAsked')
  for (const s of sites) {
    let host
    try {
      host = registrable(new URL(s.url).hostname)
    } catch {
      continue
    }
    if (Date.now() - (loginAsked[host] || 0) < ASK_COOLDOWN) continue
    if (!(await cookieCount(host))) continue
    // 已设为自动同步的网站：不再询问，直接同步
    if (cfg.autoSync && cfg.syncSites.includes(host)) {
      loginAsked[host] = Date.now()
      await api.storage.local.set({ loginAsked })
      await doLoginSync(s.url, host)
      continue
    }
    loginAsked[host] = Date.now()
    await api.storage.local.set({ loginAsked })
    const { loginPending = {} } = await sessionStore.get('loginPending')
    loginPending[host] = s.url
    await sessionStore.set({ loginPending })
    const opts = { type: 'basic', iconUrl: 'icons/128.png', title: `${host} 需要登录才能下载`, message: '浏览器里已有这个网站的登录状态。同步给清影后，会自动重新开始失败的任务。只发给本机的清影。' }
    try {
      await api.notifications.create(`cc-login:${host}`, { ...opts, buttons: [{ title: '同步并重试' }, { title: '以后自动同步' }], requireInteraction: true })
    } catch {
      // Firefox 不支持按钮：点通知本身就是“同步并重试”
      await api.notifications.create(`cc-login:${host}`, { ...opts, message: opts.message + '（点击此通知同步）' })
    }
  }
}

async function doLoginSync(url, host) {
  try {
    const r = await syncCookies(url, '', false)
    const retried = Number(r.retried) || 0
    api.notifications.create(`cc-login-ok:${host}`, {
      type: 'basic',
      iconUrl: 'icons/128.png',
      title: `已同步 ${r.count} 条 Cookie`,
      message: retried ? `${host}：已重新开始 ${retried} 个任务` : `${host} 的登录状态已同步给清影`,
    })
    return true
  } catch (e) {
    api.notifications.create(`cc-login-err:${host}`, { type: 'basic', iconUrl: 'icons/128.png', title: '同步登录失败', message: e.message })
    return false
  }
}

async function onLoginNotice(id, button) {
  if (!id.startsWith('cc-login:')) return
  const host = id.slice('cc-login:'.length)
  const { loginPending = {} } = await sessionStore.get('loginPending')
  const url = loginPending[host] || `https://${host}/`
  api.notifications.clear(id)
  if (button === 1) {
    // 以后自动同步：加入自动同步名单
    const c = await getConfig()
    await api.storage.local.set({ autoSync: true, syncSites: [...new Set([...c.syncSites, host])] })
  }
  await doLoginSync(url, host)
}

if (api.notifications?.onClicked) {
  api.notifications.onClicked.addListener((id) => onLoginNotice(id, 0))
  api.notifications.onButtonClicked?.addListener((id, b) => onLoginNotice(id, b))
}
if (api.alarms) {
  api.alarms.onAlarm.addListener((a) => {
    if (a.name === POLL_ALARM) pollOnce().catch((e) => console.warn('poll failed', e))
  })
}
api.runtime.onStartup?.addListener(() => ensureAlarm())
api.runtime.onInstalled.addListener(() => ensureAlarm())
// 设置改了：马上刷新一次
api.storage.onChanged.addListener((ch, area) => {
  if (area === 'local' && (ch.features || ch.port || ch.token)) pollOnce().catch(() => {})
})
