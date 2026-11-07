// 视频下载：在视频胶囊（vdock.js）上放一个“下载”，点开可以选发送页面还是发送检测到的视频流。
;(() => {
  if (globalThis.__ccFloat) return
  globalThis.__ccFloat = true
  const { mount, h, send, hostOff, onSettings, toast } = globalThis.ccUI

  const Dock = globalThis.ccVDock
  let ui = null
  let menu = null
  let video = null
  let off = false

  const refresh = () => hostOff('floatButton').then((v) => {
    off = v
    if (off) closeMenu()
    Dock.refresh()
  })
  refresh()
  onSettings(refresh)

  function ensureUi() {
    if (ui) return
    ui = mount('')
  }

  // 挂进视频胶囊：胶囊上一个“下载”，点开选发送页面还是检测到的视频流
  Dock.register('download', {
    order: 10,
    title: '下载',
    applies: () => !off,
    inline: () => Dock.parts.btn('下载', () => toggleMenu(Dock.current()), '用清影下载（选清晰度或视频流）'),
    menu: () => [{ label: '用清影下载…', onclick: (v) => toggleMenu(v) }],
    sticky: () => !!menu,
  })

  function place() {
    if (!menu) return
    const p = Dock.above(280, video)
    menu.style.left = `${p.left}px`
    menu.style.top = 'auto'
    menu.style.bottom = `${p.bottom}px`
    menu.style.maxHeight = `${p.maxHeight}px`
    menu.style.overflowY = 'auto'
  }
  addEventListener('scroll', () => menu && place(), { capture: true, passive: true })
  addEventListener('resize', () => menu && place())

  function closeMenu() {
    menu?.remove()
    menu = null
  }

  async function toggleMenu(v) {
    if (menu) return closeMenu()
    ensureUi()
    video = v
    menu = h('div', { class: 'layer menu', style: 'width:280px' }, h('div', { class: 'hd', text: '正在检测视频…' }))
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
  document.addEventListener('mousedown', (e) => menu && ui && !e.composedPath().includes(ui.host) && !Dock.owns(e) && closeMenu(), true)
})()
