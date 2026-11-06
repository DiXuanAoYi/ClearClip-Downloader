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
      [${ATTR}][data-style="card"] { background: rgba(37,99,235,.13); border-left: 3px solid rgba(37,99,235,.75); border-radius: 4px; padding: .3em .6em; margin: .35em 0 .45em; }
      [${ATTR}][data-style="dim"] { opacity: .78; }
      [${ATTR}][data-style="underline"] { text-decoration: underline dashed; text-underline-offset: 3px; text-decoration-color: #b4561a99; }
      [${ATTR}][data-style="highlight"] { background: #ffe9a8; color: #1d2420 !important; border-radius: 3px; padding: 0 .25em; }
      [${ATTR}][data-blur] { filter: blur(6px); transition: filter .12s ease; }
      [${ATTR}][data-blur]:hover, :hover > [${ATTR}][data-blur] { filter: none; }
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
    .card .cmpRow + .cmpRow { margin-top: 8px; padding-top: 8px; border-top: 1px dashed #c9d2cd; }
    .card .eng { font-size: 11px; color: #5d6862; margin-bottom: 2px; }
    .card .bar { display: flex; gap: 6px; margin-top: 8px; align-items: center; }
    .card .bar .grow { flex: 1; color: #5d6862; font-size: 12px; }
    .bar-pill { position: fixed; z-index: 2147483647; right: 16px; bottom: 16px; display: flex; gap: 8px; align-items: center; background: #1d2420; color: #fff;
      border-radius: 999px; padding: 6px 12px; box-shadow: 0 6px 20px #0004; }
    .bar-pill button { padding: 2px 8px; font-size: 12px; }
    .tbox { position: fixed; z-index: 2147483647; width: min(460px, calc(100vw - 24px)); max-height: 80vh; display: flex; flex-direction: column; background: #fff; color: #1d2420;
      border: 1px solid #cfd5d1; border-radius: 12px; box-shadow: 0 16px 40px #0004; overflow: hidden; }
    .tbox .hd { display: flex; align-items: center; gap: 8px; padding: 8px 10px; cursor: grab; background: #f3f1ec; border-bottom: 1px solid #e1e5e2; user-select: none; touch-action: none; font-weight: 600; }
    .tbox .hd .grow { flex: 1; }
    .tbox .bd { padding: 10px; display: flex; flex-direction: column; gap: 8px; overflow: auto; }
    .tbox textarea { width: 100%; min-height: 70px; resize: vertical; font: 14px/1.5 -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif; padding: 6px 8px; border: 1px solid #cfd5d1; border-radius: 6px; background: #fff; color: #1d2420; }
    .tbox .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
    .tbox select { padding: 4px 6px; border: 1px solid #cfd5d1; border-radius: 6px; background: #fff; color: #1d2420; }
    .tbox .res { border: 1px solid #e1e5e2; border-radius: 8px; padding: 8px 10px; }
    .tbox .res .eng { font-size: 11px; color: #5d6862; display: flex; gap: 6px; align-items: center; margin-bottom: 2px; }
    .tbox .res .txt { white-space: pre-wrap; word-break: break-word; font-size: 14px; line-height: 1.6; }
    .tbox .res.err .txt { color: #a8362d; }
    @media (prefers-color-scheme: dark) { .tbox { background: #1d2420; color: #eef1ee; border-color: #3a443f } .tbox .hd { background: #262f2a; border-color: #3a443f } .tbox textarea, .tbox select { background: #262f2a; color: #eef1ee; border-color: #3a443f } .tbox .res { border-color: #3a443f } }
    @media (prefers-color-scheme: dark) { .card { background: #1d2420; color: #eef1ee; border-color: #3a443f } .card .src { color: #a3aea8 } }
  `))

  // ---------- 找段落 ----------

  const ours = (el) => !!el.closest?.(`[${ATTR}]`)

  function skippable(el) {
    if (S.exclSel && el?.closest) {
      try {
        if (el.closest(S.exclSel)) return true
      } catch {
        // 选择器有问题：当没写
      }
    }
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

  /** 译文模糊只用在双语对照里（只显示译文时原文看不到，再模糊就没法读了）。 */
  const blurOn = () => S.cfg.blur === true && S.cfg.mode !== 'replace'

  function showTranslation(block, text) {
    const rec = S.blocks.get(block)
    if (!rec) return
    rec.node?.remove()
    const node = document.createElement('font')
    node.setAttribute(ATTR, '')
    node.setAttribute('data-style', S.cfg.style)
    if (blurOn()) node.setAttribute('data-blur', '')
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

  /** 给后台判断网页领域用：站名、标题、描述、标题文字和开头一段正文（只取一次，很短）。 */
  function pageInfo() {
    if (S.pageInfo) return S.pageInfo
    const meta = (n) => document.querySelector(`meta[name="${n}"],meta[property="${n}"]`)?.content || ''
    const heads = [...document.querySelectorAll('h1,h2,h3')].slice(0, 12).map((e) => (e.textContent || '').trim()).filter(Boolean)
    const body = (document.body?.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 600)
    const sample = [meta('description') || meta('og:description'), heads.join(' | '), body].filter(Boolean).join('\n').slice(0, 1500)
    return (S.pageInfo = { host: location.hostname, title: document.title || '', sample })
  }

  async function run(batch) {
    S.inflight++
    for (const it of batch) {
      const rec = S.blocks.get(it.id)
      if (rec) rec.state = 'busy'
    }
    status()
    const r = await send({ type: 'cc-tr', texts: batch.map((b) => b.text), target: T.engineTarget(S.cfg, T.pickEngine(S.cfg)), engine: T.pickEngine(S.cfg), page: pageInfo() })
    S.inflight--
    if (r.ok && r.fallback && !S.fallbackTold) {
      S.fallbackTold = true
      globalThis.ccUI.toast(uiRoot(), `清影没开，这一页先用 ${T.ENGINE_NAMES[r.fallback] || r.fallback} 备用翻译（不带术语库）`, false, 5000)
    }
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
      rec.node.toggleAttribute('data-blur', blurOn())
      unhide(b)
      if (S.cfg.mode === 'replace') hideOriginal(b, rec, true)
    }
  }

  // ---------- 悬停翻译：单击一下修饰键 ----------

  let tapKey = null
  let sweepTimer = 0
  document.addEventListener(
    'mousemove',
    (e) => {
      const el = e.target instanceof Element ? e.target : null
      S.lastBlock = el
      S.lastNode = null
      // 按住修饰键已经进入“扫描”状态：鼠标停在哪一段超过一小会儿，就翻译那一段（已翻译的不动）
      if (held) {
        clearTimeout(sweepTimer)
        sweepTimer = setTimeout(() => held && hoverTranslate(true), 140)
      }
    },
    { passive: true, capture: true },
  )
  // 按住：超过 holdMs 就翻译（不用松手）；松手早于这个时间算单击。其他键或鼠标按下都取消。
  let holdTimer = 0
  let held = false
  const clearHold = () => {
    clearTimeout(holdTimer)
    holdTimer = 0
  }
  // 切到别的窗口时收不到松开键的事件，不重置的话回来就一直处在“扫描”状态
  addEventListener('blur', () => {
    clearHold()
    clearTimeout(sweepTimer)
    tapKey = null
    held = false
  })
  document.addEventListener('keydown', (e) => {
    if (e.repeat && e.key === S.cfg.hover) return
    tapKey = S.cfg.hover !== 'off' && e.key === S.cfg.hover ? e.key : null
    if (e.key !== S.cfg.hover) {
      tapKey = null
      clearHold()
      return
    }
    held = false
    clearHold()
    if (tapKey && S.cfg.holdMs > 0) {
      holdTimer = setTimeout(() => {
        holdTimer = 0
        if (!tapKey) return
        held = true
        // 按住只负责“翻译”，已经翻过的段落不动（还原用单击）
        hoverTranslate(true)
      }, S.cfg.holdMs)
    }
  }, true)
  document.addEventListener('mousedown', () => ((tapKey = null), clearHold()), true)
  document.addEventListener('keyup', (e) => {
    clearHold()
    clearTimeout(sweepTimer)
    if (tapKey && e.key === tapKey && !held) hoverTranslate()
    tapKey = null
    held = false
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

  let hoverDepth = 0
  let hoverWasOn = false
  async function hoverTranslate(onlyTranslate = false) {
    const b = S.lastBlock && blockUnder(S.lastBlock)
    if (!b) return
    ensureStyle()
    const rec = S.blocks.get(b)
    if (rec && !rec.node && rec.state === 'busy') return // 这一段正在翻
    if (rec?.node) {
      if (onlyTranslate) return
      // 再点一次 = 还原这一段
      rec.node.remove()
      unhide(b)
      S.blocks.delete(b)
      return
    }
    const { text, last } = textOf(b)
    if (!text) return
    S.blocks.set(b, { text, last, state: 'busy', node: null })
    // 扫过好几段时会同时有几个在翻：只有最先开始的记下“整页翻译开没开”，最后一个结束时才还原
    if (hoverDepth++ === 0) hoverWasOn = S.on
    S.on = true // 让 run() 在没开整页时也能工作
    try {
      await run([{ id: b, text }])
    } finally {
      if (--hoverDepth === 0) {
        S.on = hoverWasOn
        if (!hoverWasOn) {
          pill?.remove()
          pill = null
        }
      }
    }
    if (!hoverWasOn && S.blocks.get(b)?.state === 'fail') toast(uiRoot(), S.err || '翻译失败了', true, 4000)
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
    if (!S.cfg.select || globalThis.ccSelbarOn) return
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
    const list = h('div', { class: 'cmp' })
    const copyBtn = h('button', { class: 'sec', text: '复制', disabled: true })
    const cmpBtn = h('button', { class: 'sec', text: '对比其他服务', onclick: () => compareMore() })
    selCard = h('div', { class: 'card' }, h('div', { class: 'src', text: text.slice(0, 200) }), list,
      h('div', { class: 'bar' }, h('span', { class: 'grow', text: T.nameOf(S.cfg.target) }), T.activeEngines(S.cfg).length > 1 ? cmpBtn : null, h('button', { class: 'sec', text: '翻译窗', title: '在可拖动的翻译窗里继续（可改文字、换语言）', onclick: () => openBox(text) }), copyBtn, h('button', { class: 'sec', text: '关闭', onclick: closeCard })))
    const box = rect || { left: innerWidth / 2 - 190, bottom: innerHeight / 3 }
    selCard.style.left = `${Math.max(8, Math.min(box.left, innerWidth - 400))}px`
    selCard.style.top = `${Math.max(8, Math.min(box.bottom + 8, innerHeight - 200))}px`
    uiRoot().add(selCard)

    const rows = new Map()
    const shown = new Set()
    let best = ''
    const row = (engine) => {
      const out = h('div', { class: 'out', text: '正在翻译…' })
      const el = h('div', { class: 'cmpRow' }, T.activeEngines(S.cfg).length > 1 ? h('div', { class: 'eng', text: T.ENGINE_NAMES[engine] || engine }) : null, out)
      list.append(el)
      rows.set(engine, out)
      return out
    }
    const one = async (engine) => {
      if (shown.has(engine)) return
      shown.add(engine)
      const out = row(engine)
      const r = await send({ type: 'cc-tr', texts: [text], target: T.engineTarget(S.cfg, engine), engine, page: pageInfo() })
      if (!selCard) return
      if (!r.ok) {
        out.textContent = r.error || '翻译失败了'
        return
      }
      const tr = r.texts?.[0] || ''
      out.textContent = tr === text ? '这段文字已经是目标语言，不用翻译。' : tr
      if (tr !== text && !best) {
        best = tr
        copyBtn.disabled = false
      }
      // 每行点一下复制自己的译文
      if (tr !== text) {
        out.title = '点一下复制这个译文'
        out.style.cursor = 'copy'
        out.onclick = () => navigator.clipboard?.writeText(tr).then(() => (copyBtn.textContent = '已复制'), () => (copyBtn.textContent = '复制失败'))
      }
    }
    const compareMore = () => {
      cmpBtn.remove()
      return Promise.all(T.activeEngines(S.cfg).map(one))
    }
    copyBtn.onclick = async () => {
      try {
        await navigator.clipboard.writeText(best)
        copyBtn.textContent = '已复制'
      } catch {
        copyBtn.textContent = '复制失败'
      }
    }
    // 默认只用设置里选的服务；开了“并排对比”就一次问全部
    if (S.cfg.compare) await compareMore()
    else await one(T.pickEngine(S.cfg))
  }

  // ---------- 翻译窗：悬浮在页面上的小窗，可拖动，输入或粘贴文字、选语言、几个服务并排对比 ----------

  let tbox = null
  function closeBox() {
    tbox?.remove()
    tbox = null
  }
  function openBox(text0) {
    closeCard()
    closeSel()
    if (tbox) {
      const ta = tbox.querySelector('textarea')
      if (text0) ta.value = text0
      ta.focus()
      return tbox.__go?.()
    }
    const ta = h('textarea', { placeholder: '输入或粘贴要翻译的文字（Ctrl+Enter 翻译）', rows: '3' })
    ta.value = text0 || ''
    const sel = h('select', { title: '翻译成' })
    for (const l of T.LANGS) sel.append(h('option', { value: l.code, text: l.name, selected: l.code === S.cfg.target }))
    const cmp = h('input', { type: 'checkbox', checked: !!S.cfg.compare })
    const out = h('div', { style: 'display:flex;flex-direction:column;gap:8px' })
    const msg = h('div', { class: 'eng', text: '' })
    const goBtn = h('button', { text: '翻译' })
    const hd = h('div', { class: 'hd' }, h('span', { class: 'grow', text: '翻译窗' }), h('button', { class: 'sec', text: '✕', title: '关闭（Esc）', onclick: closeBox }))
    tbox = h('div', { class: 'tbox' }, hd, h('div', { class: 'bd' }, ta, h('div', { class: 'row' }, h('span', { text: '翻译成' }), sel, h('label', { class: 'row', style: 'gap:4px' }, cmp, document.createTextNode('几个服务并排对比')), h('span', { style: 'flex:1' }), goBtn), msg, out))
    tbox.style.left = `${Math.max(8, innerWidth - 480)}px`
    tbox.style.top = '72px'
    uiRoot().add(tbox)
    // 拖动标题栏
    hd.addEventListener('pointerdown', (e) => {
      if (e.target.closest('button')) return
      const r = tbox.getBoundingClientRect()
      const dx = e.clientX - r.left
      const dy = e.clientY - r.top
      hd.setPointerCapture?.(e.pointerId)
      const move = (ev) => {
        tbox.style.left = `${Math.max(0, Math.min(ev.clientX - dx, innerWidth - 80))}px`
        tbox.style.top = `${Math.max(0, Math.min(ev.clientY - dy, innerHeight - 40))}px`
      }
      const up = () => {
        hd.removeEventListener('pointermove', move)
        hd.removeEventListener('pointerup', up)
      }
      hd.addEventListener('pointermove', move)
      hd.addEventListener('pointerup', up)
    })
    // 打字、快捷键不要被网页拦走
    for (const ev of ['keydown', 'keyup', 'keypress', 'beforeinput', 'input', 'paste', 'cut', 'copy']) tbox.addEventListener(ev, (e) => e.stopPropagation())
    const card = (engine, r) => {
      const name = T.ENGINE_NAMES[engine] || engine
      const txt = h('div', { class: 'txt', text: r.error || r.text || '' })
      const head = h('div', { class: 'eng' }, h('span', { text: name }))
      const el = h('div', { class: `res${r.error ? ' err' : ''}` }, head, txt)
      if (!r.error && r.text) {
        const copy = h('button', { class: 'sec', text: '复制', style: 'padding:1px 8px;font-size:12px' })
        copy.onclick = () => navigator.clipboard?.writeText(r.text).then(() => ((copy.textContent = '已复制'), setTimeout(() => (copy.textContent = '复制'), 1200)), () => (copy.textContent = '复制失败'))
        head.append(h('span', { style: 'flex:1' }), copy)
      }
      return el
    }
    const go = async () => {
      const text = ta.value.trim()
      if (!text) return
      goBtn.disabled = true
      msg.textContent = '翻译中…'
      out.replaceChildren()
      const engines = cmp.checked ? T.activeEngines(S.cfg) : [T.pickEngine(S.cfg)]
      await Promise.all(
        engines.map(async (engine) => {
          const r = await send({ type: 'cc-tr', texts: [text], target: sel.value, engine, page: pageInfo() })
          out.append(card(engine, r.ok ? { text: r.texts?.[0] || '' } : { error: r.error || '翻译失败了' }))
        }),
      )
      msg.textContent = ''
      goBtn.disabled = false
    }
    tbox.__go = go
    goBtn.onclick = go
    ta.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.isComposing) {
        e.preventDefault()
        go()
      } else if (e.key === 'Escape') closeBox()
    })
    ta.focus()
    if (ta.value.trim()) go()
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

  // Alt+I：把当前输入框里的字译成“输入框语言”（和连按三次空格一样，用在不想连按空格的时候）
  document.addEventListener('keydown', (e) => {
    if (!S.cfg.input || !e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.key.toLowerCase() !== 'i' || e.isComposing) return
    const el = document.activeElement
    const editable = el && (el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && /^(text|search|)$/i.test(el.type)) || el.isContentEditable)
    if (!editable) return
    e.preventDefault()
    translateInput(el)
  }, true)

  const textOfInput = (el) => (el.isContentEditable ? el.innerText : el.value)

  async function translateInput(el) {
    const src = textOfInput(el).replace(/\s+$/, '')
    if (src.trim().length < 1) return
    const t = toast(uiRoot(), `正在翻译成${T.nameOf(S.cfg.inputLang)}…`, false, 30000)
    const r = await send({ type: 'cc-tr', texts: [src], target: S.cfg.inputLang, engine: T.pickEngine(S.cfg) })
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
      S.cfg = T.effectiveCfg(T.normalizeCfg(transCfg), location.hostname)
    } catch {
      S.cfg = T.normalizeCfg()
    }
    applyCfgExtras()
  }

  /** 设置里“不翻译的区域”和“自定义 CSS”：每次设置变了都重新算。写错的选择器跳过，不影响别的。 */
  function applyCfgExtras() {
    const ok = []
    for (const sel of T.excludeFor(S.cfg, location.hostname)) {
      try {
        document.createDocumentFragment().querySelector(sel)
        ok.push(sel)
      } catch {
        // 选择器写错了
      }
    }
    S.exclSel = ok.join(',')
    const css = T.cleanCss(S.cfg.customCss)
    let el = document.getElementById('cc-tr-css')
    if (!css) return void el?.remove()
    ensureStyle() // 内置样式先放进去，自定义的接在后面、选择器更具体，才压得过内置的
    if (!el) {
      el = document.createElement('style')
      el.id = 'cc-tr-css'
    }
    ;(document.head || document.documentElement).append(el)
    el.textContent = `[${ATTR}][data-style] { ${css} }`
  }

  function reinit() {
    return init()
  }

  async function handle(action, extra = {}) {
    await loadCfg()
    if (action === 'toggle') pageToggle()
    else if (action === 'page-on') pageOn()
    else if (action === 'page-off') pageOff()
    else if (action === 'box') {
      const t = String(extra.text || getSelection() || '').trim()
      if (window === window.top || t) openBox(t)
    } else if (action === 'selection') translateSelection(String(extra.text || getSelection() || '').trim() || '', null)
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
      S.cfg = T.effectiveCfg(T.normalizeCfg(ch.transCfg.newValue), location.hostname)
      applyCfgExtras()
      if (S.on && (was.mode !== S.cfg.mode || was.style !== S.cfg.style || was.blur !== S.cfg.blur)) restyle()
      // 目标语言 / 引擎变了：已经翻好的作废，重新来
      if (S.on && (was.target !== S.cfg.target || was.engine !== S.cfg.engine || JSON.stringify(was.services) !== JSON.stringify(S.cfg.services))) {
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
