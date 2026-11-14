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
