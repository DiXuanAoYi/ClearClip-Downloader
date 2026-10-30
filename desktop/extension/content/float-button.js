// 视频悬浮下载按钮：鼠标移到视频上，右上角出现“用清影下载”，点开可以选发送页面还是发送检测到的视频流。
;(() => {
  if (globalThis.__ccFloat) return
  globalThis.__ccFloat = true
  const { mount, h, send, hostOff, onSettings, toast } = globalThis.ccUI

  const MIN_W = 240
  const MIN_H = 135
  const CSS = `
    .fb { background: #b4561a; color: #fff; box-shadow: 0 2px 10px #0006; padding: 5px 12px; border-radius: 16px; font-size: 12px; line-height: 18px; }
    .fb:hover { background: #d9762b; }
  `
  let ui = null
  let btn = null
  let menu = null
  let video = null
  let hideTimer = 0
  let last = 0
  let off = false

  const refresh = () => hostOff('floatButton').then((v) => {
    off = v
    if (off) hide()
  })
  refresh()
  onSettings(refresh)

  function ensureUi() {
    if (ui) return
    ui = mount(CSS)
    btn = h('button', { class: 'layer fb', text: '用清影下载', title: '发送到清影', onclick: toggleMenu, style: 'display:none' })
    btn.addEventListener('mouseenter', () => clearTimeout(hideTimer))
    btn.addEventListener('mouseleave', scheduleHide)
    ui.add(btn)
  }

  function place() {
    if (!video || !btn) return
    const r = video.getBoundingClientRect()
    if (r.width < MIN_W || r.height < MIN_H || r.bottom < 40 || r.top > innerHeight - 40) return hide()
    const w = btn.offsetWidth || 96
    const left = Math.max(4, Math.min(r.right - 8 - w, innerWidth - w - 4))
    const top = Math.max(r.top + 8, 8)
    btn.style.left = `${left}px`
    btn.style.top = `${top}px`
    btn.style.display = ''
    if (menu) {
      menu.style.left = `${Math.max(4, Math.min(left, innerWidth - 280))}px`
      menu.style.top = `${top + 30}px`
    }
  }

  function show(v) {
    ensureUi()
    clearTimeout(hideTimer)
    video = v
    place()
  }

  function hide() {
    clearTimeout(hideTimer)
    closeMenu()
    if (btn) btn.style.display = 'none'
    video = null
  }

  function scheduleHide() {
    clearTimeout(hideTimer)
    hideTimer = setTimeout(() => !menu && hide(), 1500)
  }

  function videoAt(e) {
    for (const el of e.composedPath?.() || []) if (el.tagName === 'VIDEO') return el
    return document.elementsFromPoint(e.clientX, e.clientY).find((x) => x.tagName === 'VIDEO') || null
  }

  document.addEventListener(
    'mousemove',
    (e) => {
      const now = performance.now()
      if (off || now - last < 120) return
      last = now
      const v = videoAt(e)
      if (v) show(v)
      else if (btn && btn.style.display !== 'none') scheduleHide()
    },
    { capture: true, passive: true },
  )
  addEventListener('scroll', () => video && place(), { capture: true, passive: true })
  addEventListener('resize', () => video && place())

  function closeMenu() {
    menu?.remove()
    menu = null
  }

  async function toggleMenu() {
    if (menu) return closeMenu()
    const v = video
    menu = h('div', { class: 'layer menu' }, h('div', { class: 'hd', text: '正在检测视频…' }))
    ui.add(menu)
    place()
    const r = await send({ type: 'cc-sniffed' })
    const items = r.ok ? r.items || [] : []
    if (!menu) return
    const act = async (msg) => {
      closeMenu()
      const res = await send(msg)
      toast(ui, res.ok ? (res.pairing ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送到清影') : res.error || '发送失败', !res.ok)
    }
    const rows = [h('button', { class: 'it', text: '发送此页面（由清影选清晰度）', onclick: () => act({ type: 'cc-send-page' }) })]
    for (const it of items.slice(0, 6)) rows.push(h('button', { class: 'it', title: it.url, text: it.label, onclick: () => act({ type: 'cc-send-media', url: it.url }) }))
    const src = v?.currentSrc || ''
    if (/^https?:/i.test(src) && !items.some((x) => x.url === src)) rows.push(h('button', { class: 'it', title: src, text: '当前播放的视频文件', onclick: () => act({ type: 'cc-send-media', url: src }) }))
    menu.replaceChildren(...rows)
    place()
  }

  document.addEventListener('keydown', (e) => e.key === 'Escape' && menu && closeMenu(), true)
})()
