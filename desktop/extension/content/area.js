// 截图：选取区域。在页面上拖出一个框，松开后把区域交给后台截图。按需注入，用完就移除。
;(() => {
  globalThis.__ccAreaStop?.()
  const { mount, h, send, toast } = globalThis.ccUI
  const ui = mount(`
    .shade { inset: 0; cursor: crosshair; background: #0000; }
    .box { border: 2px solid #f08a3c; box-shadow: 0 0 0 9999px #0006; pointer-events: none; }
    .tip { left: 50%; top: 16px; transform: translateX(-50%); background: #1d2420; color: #fff; border-radius: 8px; padding: 6px 14px; }
  `)
  const shade = h('div', { class: 'layer shade' })
  const tip = h('div', { class: 'layer tip', text: '拖动选取要截图的区域，Esc 取消' })
  ui.add(shade)
  ui.add(tip)
  let box = null
  let x0 = 0
  let y0 = 0

  const stop = () => {
    document.removeEventListener('keydown', onKey, true)
    ui.remove()
    globalThis.__ccAreaStop = null
  }
  globalThis.__ccAreaStop = stop
  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      stop()
    }
  }
  document.addEventListener('keydown', onKey, true)

  const rect = (e) => ({ x: Math.min(x0, e.clientX), y: Math.min(y0, e.clientY), w: Math.abs(e.clientX - x0), h: Math.abs(e.clientY - y0) })
  const paint = (r) => (box.style.cssText = `left:${r.x}px;top:${r.y}px;width:${r.w}px;height:${r.h}px`)

  shade.addEventListener('mousedown', (e) => {
    e.preventDefault()
    x0 = e.clientX
    y0 = e.clientY
    tip.remove()
    box = h('div', { class: 'layer box' })
    ui.add(box)
    paint({ x: x0, y: y0, w: 0, h: 0 })
    const move = (ev) => paint(rect(ev))
    const up = async (ev) => {
      shade.removeEventListener('mousemove', move)
      shade.removeEventListener('mouseup', up)
      const r = rect(ev)
      stop()
      if (r.w < 8 || r.h < 8) return
      // 等遮罩真的从画面上消失再截
      await new Promise((ok) => setTimeout(ok, 120))
      const res = await send({ type: 'cc-area', rect: r, vw: innerWidth })
      if (!res.ok) {
        const t = mount()
        toast(t, res.error || '截图没有完成', true, 3000)
        setTimeout(() => t.remove(), 3200)
      }
    }
    shade.addEventListener('mousemove', move)
    shade.addEventListener('mouseup', up)
  })
})()
