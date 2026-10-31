// 网页翻译：整页双语对照 / 只显示译文、只翻译看得见的段落、动态内容跟着翻、悬停翻译、划词翻译、输入框三连空格翻译。
// 原页面的节点一个都不改（只在旁边插入译文节点；“只显示译文”是把原文藏起来），所以随时能还原。
;(() => {
  const ex = globalThis.ccWebTrans
  if (ex) return ex.reinit?.()
  const { api, mount, h, send, toast } = globalThis.ccUI
  const T = globalThis.ccTrans

  const ATTR = 'data-cc-tr' // 译文节点
  const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'CODE', 'PRE', 'KBD', 'SAMP', 'TEXTAREA', 'INPUT', 'SELECT', 'OPTION', 'SVG', 'MATH', 'CANVAS', 'VIDEO', 'AUDIO', 'IFRAME', 'OBJECT', 'TEMPLATE', 'HEAD'])
  const INLINE = /^(inline|inline-block|contents|ruby|ruby-text|ruby-base)$/

  const S = {
    cfg: T.normalizeCfg(),
    on: false, // 整页翻译开着
    blocks: new Map(), // 段落元素 → { text, state: 'new'|'queued'|'busy'|'done'|'fail', node, last }
    queue: [],
    pumping: false,
    inflight: 0,
    done: 0,
    failed: 0,
    obs: null, // IntersectionObserver
    mo: null, // MutationObserver
    moTimer: 0,
    ui: null,
    lastBlock: null,
    err: '',
  }

  // ---------- 样式（放在页面里，才能继承页面的字体）----------

  function ensureStyle() {
    if (document.getElementById('cc-tr-style')) return
    const st = document.createElement('style')
    st.id = 'cc-tr-style'
    st.textContent = `
      [${ATTR}] { display: block; margin: .25em 0 .35em; font-size: .95em; line-height: 1.5; text-indent: 0; white-space: pre-wrap; word-break: break-word; unicode-bidi: plaintext; }
      [${ATTR}][data-style="dim"] { opacity: .78; }
      [${ATTR}][data-style="underline"] { text-decoration: underline dashed; text-underline-offset: 3px; text-decoration-color: #b4561a99; }
      [${ATTR}][data-style="highlight"] { background: #ffe9a8; color: #1d2420 !important; border-radius: 3px; padding: 0 .25em; }
      [${ATTR}][data-state="busy"] { opacity: .5; }
      [${ATTR}][data-state="busy"]::after { content: " …"; }
      [${ATTR}][data-state="fail"] { color: #a8362d; cursor: pointer; }
      [data-cc-tr-hide] { display: none !important; }
    `
    ;(document.head || document.documentElement).append(st)
  }

  const uiRoot = () => (S.ui ||= mount(`
    .sel { position: fixed; z-index: 2147483647; padding: 3px 9px; font-size: 12px; }
    .card { position: fixed; z-index: 2147483647; width: min(380px, calc(100vw - 24px)); max-height: 60vh; overflow: auto; background: #fff; color: #1d2420;
      border: 1px solid #cfd5d1; border-radius: 10px; box-shadow: 0 12px 32px #0003; padding: 10px 12px; }
    .card .src { color: #5d6862; font-size: 12px; max-height: 4.5em; overflow: hidden; margin-bottom: 6px; }
    .card .out { white-space: pre-wrap; word-break: break-word; font-size: 14px; line-height: 1.6; }
    .card .bar { display: flex; gap: 6px; margin-top: 8px; align-items: center; }
    .card .bar .grow { flex: 1; color: #5d6862; font-size: 12px; }
    .bar-pill { position: fixed; z-index: 2147483647; right: 16px; bottom: 16px; display: flex; gap: 8px; align-items: center; background: #1d2420; color: #fff;
      border-radius: 999px; padding: 6px 12px; box-shadow: 0 6px 20px #0004; }
    .bar-pill button { padding: 2px 8px; font-size: 12px; }
    @media (prefers-color-scheme: dark) { .card { background: #1d2420; color: #eef1ee; border-color: #3a443f } .card .src { color: #a3aea8 } }
  `))

  // ---------- 找段落 ----------

  const ours = (el) => !!el.closest?.(`[${ATTR}]`)

  function skippable(el) {
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      if (SKIP_TAGS.has(e.tagName?.toUpperCase?.())) return true
      if (e.isContentEditable || e.getAttribute?.('translate') === 'no' || e.classList?.contains('notranslate')) return true
      if (e.hasAttribute?.(ATTR) || e.hasAttribute?.('data-cc-tr-hide')) return true
      if (e.getAttribute?.('aria-hidden') === 'true' && !e.textContent.trim()) return true
    }
    return false
  }

  /** 文字节点所属的“段落”：往上找到第一个不是行内的元素。 */
  function blockOf(node) {
    let el = node.parentElement
    while (el && el !== document.body) {
      const cs = getComputedStyle(el)
      if (!INLINE.test(cs.display)) return el
      el = el.parentElement
    }
    return el || document.body
  }

  const visible = (el) => {
    const r = el.getBoundingClientRect()
    if (!r.width && !r.height) return false
    const cs = getComputedStyle(el)
    return cs.visibility !== 'hidden' && cs.display !== 'none'
  }

  /** 一个段落里要翻译的文字：它自己的（不含已经是别的段落的子元素）文字节点拼起来。 */
  function textOf(block) {
    const parts = []
    let last = null
    const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => {
        if (!n.nodeValue.trim()) return NodeFilter.FILTER_REJECT
        if (skippable(n.parentElement)) return NodeFilter.FILTER_REJECT
        return blockOf(n) === block ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT
      },
    })
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      parts.push(n.nodeValue)
      last = n
    }
    return { text: parts.join('').replace(/\s+/g, ' ').trim(), last }
  }

  function scan(root = document.body) {
    if (!root) return 0
    let added = 0
    const seen = new Set()
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (n.nodeValue.trim().length >= 2 && !skippable(n.parentElement) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
    })
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const b = blockOf(n)
      if (seen.has(b)) continue
      seen.add(b)
      if (S.blocks.has(b)) {
        const rec = S.blocks.get(b)
        // 页面把这段文字改了（动态内容）：重新翻译
        const now = textOf(b).text
        if (rec.state === 'done' && now && now !== rec.text && !ours(b)) {
          rec.node?.remove()
          unhide(b)
          S.blocks.delete(b)
        } else continue
      }
      const { text, last } = textOf(b)
      if (!text || !T.worthTranslating(text, S.cfg.target) && S.cfg.skipTarget) continue
      if (!visible(b)) continue
      S.blocks.set(b, { text, last, state: 'new', node: null })
      added++
      if (S.cfg.lazy && S.obs) S.obs.observe(b)
      else enqueue(b)
    }
    return added
  }

  // ---------- 译文节点 ----------

  /** 译文放在文字节点所在的那一支之后：这样 <li>文字<ul>子列表</ul></li> 里译文在子列表前面。 */
  function placeNode(block, rec, node) {
    let anchor = rec.last
    while (anchor && anchor.parentNode !== block) anchor = anchor.parentNode
    if (anchor && anchor.parentNode === block) anchor.after(node)
    else block.append(node)
  }

  function showTranslation(block, text) {
    const rec = S.blocks.get(block)
    if (!rec) return
    rec.node?.remove()
    const node = document.createElement('font')
    node.setAttribute(ATTR, '')
    node.setAttribute('data-style', S.cfg.style)
    node.setAttribute('translate', 'no')
    node.textContent = text
    placeNode(block, rec, node)
    rec.node = node
    rec.state = 'done'
    if (S.cfg.mode === 'replace') hideOriginal(block, rec, true)
  }

  /** “只显示译文”：把原文的文字节点所在的那几支藏起来（译文节点自己不藏）。 */
  function hideOriginal(block, rec, on) {
    for (const el of block.children) {
      if (el === rec.node) continue
      if (on) {
        // 只藏含有文字的行内元素；子列表、图片等块级内容保持原样
        if (INLINE.test(getComputedStyle(el).display) && el.textContent.trim()) el.setAttribute('data-cc-tr-hide', '')
      } else el.removeAttribute('data-cc-tr-hide')
    }
    // 文字节点直接挂在段落下：用 Range 不好藏，改成把段落的文字颜色透明太粗暴；这里用一个标记包起来
    if (on) wrapLooseText(block, rec)
    else unwrapLooseText(block)
  }

  function wrapLooseText(block, rec) {
    const loose = [...block.childNodes].filter((n) => n.nodeType === 3 && n.nodeValue.trim())
    for (const n of loose) {
      const w = document.createElement('span')
      w.setAttribute('data-cc-tr-hide', '')
      w.setAttribute('data-cc-tr-wrap', '')
      n.replaceWith(w)
      w.append(n)
    }
  }
  function unwrapLooseText(block) {
    for (const w of block.querySelectorAll('[data-cc-tr-wrap]')) {
      w.replaceWith(...w.childNodes)
    }
  }
  /** 把一个段落里所有“藏起来”的标记去掉（段落自己、里面的行内元素、包文字的壳）。 */
  function unhide(block) {
    block.removeAttribute('data-cc-tr-hide')
    for (const el of block.querySelectorAll('[data-cc-tr-hide]')) el.removeAttribute('data-cc-tr-hide')
    unwrapLooseText(block)
  }

  // ---------- 排队、分批、请求 ----------

  function enqueue(block) {
    const rec = S.blocks.get(block)
    if (!rec || rec.state === 'queued' || rec.state === 'busy' || rec.state === 'done') return
    rec.state = 'queued'
    S.queue.push(block)
    pump()
  }

  const wait = (ms) => new Promise((r) => setTimeout(r, ms))

  async function pump() {
    if (S.pumping) return
    S.pumping = true
    try {
      await wait(120) // 攒一攒，一批发出去
      while (S.queue.length && S.on) {
        if (S.inflight >= 3) {
          await wait(150)
          continue
        }
        const items = []
        while (S.queue.length) {
          const b = S.queue.shift()
          const rec = S.blocks.get(b)
          if (rec && rec.state === 'queued') items.push({ id: b, text: rec.text })
          if (items.length >= 40) break
        }
        for (const batch of T.batchPlan(items)) run(batch)
      }
    } finally {
      S.pumping = false
    }
  }

  async function run(batch) {
    S.inflight++
    for (const it of batch) {
      const rec = S.blocks.get(it.id)
      if (rec) rec.state = 'busy'
    }
    status()
    const r = await send({ type: 'cc-tr', texts: batch.map((b) => b.text), target: S.cfg.target, engine: S.cfg.engine })
    S.inflight--
    if (!r.ok) {
      S.err = r.error || '翻译失败了'
      for (const it of batch) markFail(it.id)
      S.queue.length = 0
      status()
      return
    }
    S.err = ''
    batch.forEach((it, i) => {
      const rec = S.blocks.get(it.id)
      if (!rec) return
      const out = String(r.texts?.[i] || '').trim()
      if (!out) markFail(it.id)
      else if (out === it.text) {
        rec.state = 'done' // 模型认为不用翻（已经是目标语言）
        S.done++
      } else {
        showTranslation(it.id, out)
        S.done++
      }
    })
    status()
  }

  function markFail(block) {
    const rec = S.blocks.get(block)
    if (!rec) return
    rec.state = 'fail'
    S.failed++
  }

  // ---------- 进度条（右下角）----------

  let pill = null
  function status() {
    if (!S.on) return pill?.remove(), (pill = null)
    const left = [...S.blocks.values()].filter((r) => r.state === 'queued' || r.state === 'busy').length
    const idle = !left && !S.inflight
    const text = S.err ? S.err : idle ? (S.done ? `已翻译 ${S.done} 段` : '没有找到要翻译的内容') : `正在翻译… 已完成 ${S.done} 段`
    if (!pill) {
      pill = h('div', { class: 'bar-pill' }, h('span', { class: 'txt' }), h('button', { class: 'sec', text: '还原', onclick: () => pageOff() }))
      uiRoot().add(pill)
    }
    pill.querySelector('.txt').textContent = text
  }

  // ---------- 整页开关 ----------

  function pageOn() {
    if (S.on) return
    ensureStyle()
    S.on = true
    S.done = 0
    S.failed = 0
    S.err = ''
    if (S.cfg.lazy && 'IntersectionObserver' in window) {
      S.obs = new IntersectionObserver(
        (entries) => {
          for (const e of entries) if (e.isIntersecting) (S.obs.unobserve(e.target), enqueue(e.target))
        },
        { rootMargin: '600px 0px' },
      )
    }
    S.mo = new MutationObserver((muts) => {
      if (muts.every((m) => [...m.addedNodes].every((n) => n.nodeType === 1 && ours(n)) && !m.removedNodes.length)) return
      clearTimeout(S.moTimer)
      S.moTimer = setTimeout(() => S.on && scan(), 700)
    })
    S.mo.observe(document.body, { childList: true, subtree: true, characterData: true })
    scan()
    status()
  }

  function pageOff() {
    S.on = false
    S.obs?.disconnect()
    S.mo?.disconnect()
    S.obs = S.mo = null
    S.queue.length = 0
    for (const [b, rec] of S.blocks) {
      rec.node?.remove()
      unhide(b)
    }
    S.blocks.clear()
    pill?.remove()
    pill = null
  }

  const pageToggle = () => (S.on ? pageOff() : pageOn())

  /** 切换“双语 / 只看译文 / 样式”后，已经翻好的段落跟着改。 */
  function restyle() {
    for (const [b, rec] of S.blocks) {
      if (!rec.node) continue
      rec.node.setAttribute('data-style', S.cfg.style)
      unhide(b)
      if (S.cfg.mode === 'replace') hideOriginal(b, rec, true)
    }
  }

  // ---------- 悬停翻译：单击一下修饰键 ----------

  let tapKey = null
  document.addEventListener(
    'mousemove',
    (e) => {
      const el = e.target instanceof Element ? e.target : null
      S.lastBlock = el
      S.lastNode = null
    },
    { passive: true, capture: true },
  )
  document.addEventListener('keydown', (e) => {
    tapKey = S.cfg.hover !== 'off' && e.key === S.cfg.hover ? e.key : null
    if (e.key !== S.cfg.hover) tapKey = null
  }, true)
  document.addEventListener('mousedown', () => (tapKey = null), true)
  document.addEventListener('keyup', (e) => {
    if (tapKey && e.key === tapKey) hoverTranslate()
    tapKey = null
  }, true)

  function blockUnder(el) {
    let e = el
    while (e && e !== document.body) {
      if (!skippable(e)) {
        const own = [...e.childNodes].some((n) => n.nodeType === 3 && n.nodeValue.trim().length >= 2)
        if (own && !INLINE.test(getComputedStyle(e).display)) return e
        if (own) {
          // 行内元素：用它所在的段落
          const t = [...e.childNodes].find((n) => n.nodeType === 3 && n.nodeValue.trim())
          return blockOf(t)
        }
      }
      e = e.parentElement
    }
    return null
  }

  async function hoverTranslate() {
    const b = S.lastBlock && blockUnder(S.lastBlock)
    if (!b) return
    ensureStyle()
    const rec = S.blocks.get(b)
    if (rec?.node) {
      // 再点一次 = 还原这一段
      rec.node.remove()
      unhide(b)
      S.blocks.delete(b)
      return
    }
    const { text, last } = textOf(b)
    if (!text) return
    S.blocks.set(b, { text, last, state: 'busy', node: null })
    const wasOn = S.on
    S.on = true // 让 run() 在没开整页时也能工作
    await run([{ id: b, text }])
    S.on = wasOn
    if (!wasOn) {
      pill?.remove()
      pill = null
      const r = S.blocks.get(b)
      if (r?.state === 'fail') toast(uiRoot(), S.err || '翻译失败了', true, 4000)
    }
  }

  // ---------- 划词翻译 ----------

  let selBtn = null
  let selCard = null
  const closeSel = () => {
    selBtn?.remove()
    selBtn = null
  }
  const closeCard = () => {
    selCard?.remove()
    selCard = null
  }

  document.addEventListener('mouseup', (e) => {
    if (!S.cfg.select) return
    if (e.composedPath().some((n) => n === S.ui?.host)) return
    setTimeout(() => {
      closeSel()
      const sel = getSelection()
      const text = String(sel || '').trim()
      if (text.length < 2 || text.length > 3000 || !sel.rangeCount) return
      const el = sel.anchorNode?.parentElement
      if (el && (el.closest('input,textarea') || el.isContentEditable)) return
      const r = sel.getRangeAt(0).getBoundingClientRect()
      selBtn = h('button', { class: 'sel', text: '译', title: '翻译选中的文字', onmousedown: (ev) => ev.preventDefault(), onclick: () => (closeSel(), translateSelection(text, r)) })
      selBtn.style.left = `${Math.min(Math.max(6, r.right), innerWidth - 40)}px`
      selBtn.style.top = `${Math.min(r.bottom + 6, innerHeight - 30)}px`
      uiRoot().add(selBtn)
    }, 10)
  }, true)
  document.addEventListener('mousedown', (e) => {
    if (!e.composedPath().some((n) => n === S.ui?.host)) {
      closeSel()
      closeCard()
    }
  }, true)
  document.addEventListener('keydown', (e) => e.key === 'Escape' && (closeSel(), closeCard()), true)

  async function translateSelection(text, rect) {
    closeCard()
    const out = h('div', { class: 'out', text: '正在翻译…' })
    const copyBtn = h('button', { class: 'sec', text: '复制', disabled: true })
    selCard = h('div', { class: 'card' }, h('div', { class: 'src', text: text.slice(0, 200) }), out,
      h('div', { class: 'bar' }, h('span', { class: 'grow', text: T.nameOf(S.cfg.target) }), copyBtn, h('button', { class: 'sec', text: '关闭', onclick: closeCard })))
    const box = rect || { left: innerWidth / 2 - 190, bottom: innerHeight / 3 }
    selCard.style.left = `${Math.max(8, Math.min(box.left, innerWidth - 400))}px`
    selCard.style.top = `${Math.max(8, Math.min(box.bottom + 8, innerHeight - 200))}px`
    uiRoot().add(selCard)
    const r = await send({ type: 'cc-tr', texts: [text], target: S.cfg.target, engine: S.cfg.engine })
    if (!selCard) return
    if (!r.ok) {
      out.textContent = r.error || '翻译失败了'
      return
    }
    const tr = r.texts?.[0] || ''
    out.textContent = tr === text ? '这段文字已经是目标语言，不用翻译。' : tr
    copyBtn.disabled = tr === text
    copyBtn.onclick = async () => {
      try {
        await navigator.clipboard.writeText(tr)
        copyBtn.textContent = '已复制'
      } catch {
        copyBtn.textContent = '复制失败'
      }
    }
  }

  // ---------- 输入框：连按三次空格，译成“输入框语言”----------

  let spaces = []
  document.addEventListener('keydown', (e) => {
    if (!S.cfg.input || e.key !== ' ' || e.ctrlKey || e.metaKey || e.altKey || e.isComposing) return
    const el = document.activeElement
    const editable = el && (el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && /^(text|search|)$/i.test(el.type)) || el.isContentEditable)
    if (!editable) return
    const now = Date.now()
    spaces = spaces.filter((t) => now - t < 700).concat(now)
    if (spaces.length >= 3) {
      spaces = []
      e.preventDefault()
      translateInput(el)
    }
  }, true)

  const textOfInput = (el) => (el.isContentEditable ? el.innerText : el.value)

  async function translateInput(el) {
    const src = textOfInput(el).replace(/\s+$/, '')
    if (src.trim().length < 1) return
    const t = toast(uiRoot(), `正在翻译成${T.nameOf(S.cfg.inputLang)}…`, false, 30000)
    const r = await send({ type: 'cc-tr', texts: [src], target: S.cfg.inputLang, engine: S.cfg.engine })
    t.remove()
    if (!r.ok) return void toast(uiRoot(), r.error || '翻译失败了', true, 4000)
    const out = String(r.texts?.[0] || '').trim()
    if (!out) return
    setInput(el, out)
    toast(uiRoot(), '已替换为译文（Ctrl+Z 可撤销）', false, 2400)
  }

  function setInput(el, value) {
    el.focus()
    if (el.isContentEditable) {
      document.execCommand('selectAll')
      if (!document.execCommand('insertText', false, value)) el.textContent = value
      return
    }
    // 让 React / Vue 这类框架也知道值变了
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
    el.select()
    if (!document.execCommand('insertText', false, value)) {
      Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value)
      el.dispatchEvent(new Event('input', { bubbles: true }))
    }
  }

  // ---------- 来自后台的指令、设置变化、自动翻译 ----------

  async function loadCfg() {
    try {
      const { transCfg } = await api.storage.local.get('transCfg')
      S.cfg = T.normalizeCfg(transCfg)
    } catch {
      S.cfg = T.normalizeCfg()
    }
  }

  function reinit() {
    return init()
  }

  async function handle(action, extra = {}) {
    await loadCfg()
    if (action === 'toggle') pageToggle()
    else if (action === 'page-on') pageOn()
    else if (action === 'page-off') pageOff()
    else if (action === 'selection') translateSelection(String(extra.text || getSelection() || '').trim() || '', null)
    return { on: S.on }
  }

  try {
    api.runtime.onMessage.addListener((m, _s, reply) => {
      if (m?.type !== 'cc-wt') return false
      handle(m.action, m).then(reply, () => reply({}))
      return true
    })
    api.storage.onChanged.addListener((ch, area) => {
      if (area !== 'local' || !ch.transCfg) return
      const was = S.cfg
      S.cfg = T.normalizeCfg(ch.transCfg.newValue)
      if (S.on && (was.mode !== S.cfg.mode || was.style !== S.cfg.style)) restyle()
      // 目标语言 / 引擎变了：已经翻好的作废，重新来
      if (S.on && (was.target !== S.cfg.target || was.engine !== S.cfg.engine)) {
        pageOff()
        pageOn()
      }
    })
  } catch {
    // 扩展已失效
  }

  async function init() {
    await loadCfg()
    // 设为“自动翻译”的网站：页面本来就是目标语言就不翻
    if (!S.on && T.shouldAuto(S.cfg, location.hostname) && window === window.top) {
      const sample = (document.body?.innerText || '').slice(0, 1500)
      if (!(S.cfg.skipTarget && T.pageInTarget(sample, S.cfg.target))) {
        await wait(400)
        pageOn()
      }
    }
  }

  globalThis.ccWebTrans = { reinit, handle, state: S }
  init()
})()
