// 批量选取：按需注入。点页面里的图片、视频、链接来勾选，勾完一起发送到清影。再次注入 = 退出。
;(() => {
  if (globalThis.__ccPicker) return globalThis.__ccPicker.stop()
  const { mount, h, send, toast } = globalThis.ccUI

  const CSS = `
    .box { position: fixed; pointer-events: none; border: 2px solid #d9762b; background: #d9762b22; border-radius: 3px; }
    .box.sel { border-color: #2f8a57; background: #2f8a5733; }
    .box.sel::after { content: "✓"; position: absolute; right: 2px; top: 2px; background: #2f8a57; color: #fff; border-radius: 50%; width: 18px; height: 18px; text-align: center; line-height: 18px; font-size: 12px; }
    .bar { left: 50%; bottom: 18px; transform: translateX(-50%); display: flex; gap: 8px; align-items: center; background: #1d2420; color: #fff; border-radius: 12px; padding: 8px 12px; box-shadow: 0 8px 24px #0006; }
    .bar button.sec { background: transparent; color: #fff; border-color: #ffffff66; }
  `
  const ui = mount(CSS)
  const hover = h('div', { class: 'layer box', style: 'display:none' })
  const count = h('span', { text: '点击要发送的图片、视频或链接' })
  const sendBtn = h('button', { text: '发送所选', onclick: doSend, disabled: true })
  const bar = h('div', { class: 'layer bar' }, count, h('button', { class: 'sec', text: '全选图片', onclick: pickAllImages }), sendBtn, h('button', { class: 'sec', text: '退出', onclick: stop }))
  ui.add(hover)
  ui.add(bar)

  /** el → { kind, url, box } */
  const chosen = new Map()
  let raf = 0

  const MEDIA = /\.(mp4|m4v|webm|mkv|mov|flv|m3u8|mp3|m4a|flac|wav)(\?|#|$)/i
  const abs = (v) => {
    try {
      const u = new URL(String(v), document.baseURI)
      return /^https?:$/.test(u.protocol) ? u.href : ''
    } catch {
      return ''
    }
  }

  /** 事件路径里第一个能选的元素：视频 > 图片 > 链接。 */
  function targetOf(path) {
    for (const el of path) {
      if (!el.tagName) continue
      if (el.tagName === 'VIDEO') {
        const src = abs(el.currentSrc || el.src)
        return { el, kind: 'video', url: src || location.href }
      }
      if (el.tagName === 'IMG') {
        const big = el.dataset.original || el.dataset.src || el.currentSrc || el.src
        const url = abs(big)
        if (url) return { el, kind: 'image', url }
      }
      if (el.tagName === 'A' && el.getAttribute('href')) {
        const url = abs(el.getAttribute('href'))
        if (url && !url.startsWith(location.href.split('#')[0] + '#')) return { el, kind: MEDIA.test(url) ? 'video' : 'link', url }
      }
    }
    return null
  }

  function rectBox(el, box) {
    const r = el.getBoundingClientRect()
    Object.assign(box.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px`, display: '' })
  }

  function onMove(e) {
    const t = targetOf(e.composedPath())
    if (t) rectBox(t.el, hover)
    else hover.style.display = 'none'
  }

  function onClick(e) {
    if (e.composedPath().includes(ui.host)) return
    const t = targetOf(e.composedPath())
    if (!t) return
    e.preventDefault()
    e.stopImmediatePropagation()
    toggle(t)
  }

  function toggle(t) {
    const old = chosen.get(t.el)
    if (old) {
      old.box.remove()
      chosen.delete(t.el)
    } else {
      const box = h('div', { class: 'layer box sel' })
      ui.add(box)
      chosen.set(t.el, { kind: t.kind, url: t.url, box })
    }
    refresh()
  }

  function refresh() {
    count.textContent = chosen.size ? `已选 ${chosen.size} 项` : '点击要发送的图片、视频或链接'
    sendBtn.disabled = !chosen.size
    sendBtn.textContent = chosen.size ? `发送所选（${chosen.size}）` : '发送所选'
    cancelAnimationFrame(raf)
    if (chosen.size) raf = requestAnimationFrame(follow)
  }

  /** 页面滚动或变化时，让勾选框跟着元素走。 */
  function follow() {
    for (const [el, c] of chosen) rectBox(el, c.box)
    raf = requestAnimationFrame(follow)
  }

  function pickAllImages() {
    for (const img of document.images) {
      const r = img.getBoundingClientRect()
      if (r.width < 80 || r.height < 80 || chosen.has(img)) continue
      const t = targetOf([img])
      if (t) toggle(t)
    }
  }

  async function doSend() {
    sendBtn.disabled = true
    const items = [...chosen.values()].map((c) => ({ kind: c.kind, url: c.url }))
    const r = await send({ type: 'cc-send-batch', items })
    if (r.ok) {
      toast(ui, r.pairing ? '第一次使用：请在电脑上点“允许”完成配对' : `已发送 ${r.sent} 项${r.failed ? `，${r.failed} 项失败` : ''}`, !!r.failed && !r.sent, 3200, 'left:50%;bottom:76px;transform:translateX(-50%)')
      if (r.sent && !r.failed) setTimeout(stop, 900)
    } else {
      toast(ui, r.error || '发送失败', true, 3200, 'left:50%;bottom:76px;transform:translateX(-50%)')
    }
    sendBtn.disabled = !chosen.size
  }

  function onKey(e) {
    if (e.key === 'Escape') stop()
  }

  function stop() {
    cancelAnimationFrame(raf)
    document.removeEventListener('mousemove', onMove, true)
    document.removeEventListener('click', onClick, true)
    document.removeEventListener('keydown', onKey, true)
    ui.remove()
    delete globalThis.__ccPicker
  }

  document.addEventListener('mousemove', onMove, { capture: true, passive: true })
  document.addEventListener('click', onClick, true)
  document.addEventListener('keydown', onKey, true)
  globalThis.__ccPicker = { stop }
})()
