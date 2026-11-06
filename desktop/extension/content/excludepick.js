// 选取“不翻译的区域”：按需注入。鼠标移到哪里就框出哪个元素，点一下锁定，可以往上一级，然后加入设置里的“不翻译这些区域”。再次注入 = 退出。
;(() => {
  if (globalThis.__ccExcl) return globalThis.__ccExcl.stop()
  const { api, mount, h, toast } = globalThis.ccUI

  const STYLE = `
    .box { position: fixed; pointer-events: none; border: 2px solid #c2453b; background: #c2453b22; border-radius: 3px; }
    .bar { left: 50%; bottom: 18px; transform: translateX(-50%); display: flex; flex-direction: column; gap: 6px; width: min(560px, calc(100vw - 24px)); background: #1d2420; color: #fff; border-radius: 12px; padding: 10px 12px; box-shadow: 0 8px 24px #0006; }
    .bar .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
    .bar code { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; background: #ffffff1a; border-radius: 6px; padding: 3px 8px; font-family: ui-monospace, Consolas, monospace; font-size: 12px; }
    .bar button.sec { background: transparent; color: #fff; border-color: #ffffff66; }
    .bar label { display: flex; gap: 4px; align-items: center; font-size: 12px; }
    .bar .mut { color: #a9b3ad; font-size: 12px; }
  `
  const ui = mount(STYLE)
  const box = h('div', { class: 'layer box', style: 'display:none' })
  const code = h('code', { text: '把鼠标移到要排除的区域上，点一下' })
  const info = h('span', { class: 'mut', text: '' })
  const siteOnly = h('input', { type: 'checkbox', checked: true })
  const up = h('button', { class: 'sec', text: '↑ 上一级', disabled: true, onclick: () => pick(locked?.parentElement) })
  const add = h('button', { text: '加入排除', disabled: true, onclick: save })
  const bar = h(
    'div',
    { class: 'layer bar' },
    h('div', { class: 'row' }, code, info),
    h('div', { class: 'row' }, up, h('label', {}, siteOnly, document.createTextNode('只对这个网站')), h('span', { style: 'flex:1' }), add, h('button', { class: 'sec', text: '退出', onclick: stop })),
  )
  ui.add(box)
  ui.add(bar)

  let locked = null
  let hovered = null

  const stable = (c) => c && c.length <= 24 && !/\d{3,}/.test(c) && !/^(css|jss|sc|styled|_)[-_\w]*\d/.test(c) && !/[A-Za-z0-9_-]{16,}/.test(c)
  const esc = (s) => (globalThis.CSS?.escape ? CSS.escape(s) : String(s).replace(/[^\w-]/g, '\\$&'))
  const count = (sel) => {
    try {
      return document.querySelectorAll(sel).length
    } catch {
      return 0
    }
  }

  /** 元素 → 尽量短又稳的选择器：有唯一 id 用 id；否则标签 + 稳定的类名，从下往上加祖先，直到能区分开。 */
  function selectorFor(el) {
    if (el.id && stable(el.id) && count(`#${esc(el.id)}`) === 1) return `#${esc(el.id)}`
    const parts = []
    let cur = el
    for (let depth = 0; cur && cur !== document.body && depth < 5; depth++, cur = cur.parentElement) {
      const cls = [...cur.classList].filter(stable).slice(0, 2).map((c) => `.${esc(c)}`).join('')
      let part = cur.tagName.toLowerCase() + cls
      if (!cls) {
        const sibs = cur.parentElement ? [...cur.parentElement.children].filter((x) => x.tagName === cur.tagName) : []
        if (sibs.length > 1) part += `:nth-of-type(${sibs.indexOf(cur) + 1})`
      }
      parts.unshift(part)
      const sel = parts.join(' > ')
      if (cur.id && stable(cur.id) && count(`#${esc(cur.id)} > ${parts.slice(1).join(' > ')}`) === 1 && parts.length > 1) return `#${esc(cur.id)} > ${parts.slice(1).join(' > ')}`
      if (count(sel) === 1) return sel
    }
    return parts.join(' > ')
  }

  const place = (el) => {
    if (!el) return void (box.style.display = 'none')
    const r = el.getBoundingClientRect()
    Object.assign(box.style, { display: 'block', left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` })
  }

  function pick(el) {
    if (!el || el === document.documentElement || el === document.body) return
    locked = el
    const sel = selectorFor(el)
    code.textContent = sel
    code.title = sel
    info.textContent = `匹配 ${count(sel)} 处`
    up.disabled = !el.parentElement || el.parentElement === document.body
    add.disabled = false
    place(el)
  }

  const target = (e) => {
    const t = e.composedPath?.()[0]
    return t instanceof Element && t !== ui.host ? t : null
  }
  const onMove = (e) => {
    if (locked) return
    hovered = target(e)
    place(hovered)
  }
  const onDown = (e) => {
    if (e.composedPath?.().includes(ui.host)) return
    e.preventDefault()
    e.stopPropagation()
  }
  const onClick = (e) => {
    if (e.composedPath?.().includes(ui.host)) return
    e.preventDefault()
    e.stopPropagation()
    const t = target(e)
    if (t) pick(t)
  }
  const onKey = (e) => {
    if (e.key === 'Escape') stop()
  }

  async function save() {
    if (!locked) return
    const sel = selectorFor(locked)
    const line = siteOnly.checked ? `${location.hostname} | ${sel}` : sel
    try {
      const { transCfg } = await api.storage.local.get('transCfg')
      const cur = transCfg && typeof transCfg === 'object' ? transCfg : {}
      const list = Array.isArray(cur.exclude) ? cur.exclude : String(cur.exclude || '').split('\n').filter(Boolean)
      if (!list.includes(line)) list.push(line)
      await api.storage.local.set({ transCfg: { ...cur, exclude: list } })
      toast(ui, `已加入：${line}`, false, 2600)
      locked = null
      add.disabled = true
      up.disabled = true
      code.textContent = '继续点别的区域，或退出'
      info.textContent = ''
      place(null)
    } catch {
      toast(ui, '保存失败，刷新页面后再试', true, 3000)
    }
  }

  function stop() {
    document.removeEventListener('mousemove', onMove, true)
    document.removeEventListener('mousedown', onDown, true)
    document.removeEventListener('click', onClick, true)
    document.removeEventListener('keydown', onKey, true)
    ui.remove()
    globalThis.__ccExcl = null
  }

  document.addEventListener('mousemove', onMove, true)
  document.addEventListener('mousedown', onDown, true)
  document.addEventListener('click', onClick, true)
  document.addEventListener('keydown', onKey, true)
  globalThis.__ccExcl = { stop }
})()
