// 划词工具条：选中网页上的文字后，旁边出现 翻译 / 释义 / 朗读 / AI 搜索 / 总结 / 高亮。
// 结果在同一张卡片里显示，可复制、存笔记、加入生词本、继续追问。高亮会永久保存，下次打开同一网页自动恢复。
;(() => {
  if (globalThis.ccSelbarRun) return
  const { h, send, mount, toast } = globalThis.ccUI
  const api = globalThis.browser ?? globalThis.chrome
  const SEL = globalThis.ccSel
  globalThis.ccSelbarOn = true // 网页翻译里那个只有“译”字的小按钮让位给这条工具条
  globalThis.ccSelbarRun = true

  const CSS = `
    .bar { display: flex; gap: 2px; align-items: center; background: #10201a; border-radius: 10px; padding: 4px; box-shadow: 0 8px 24px #0006; }
    .bar button { background: transparent; color: #fff; border: 0; font-weight: 500; padding: 4px 9px; border-radius: 7px; white-space: nowrap; }
    .bar button:hover { background: #ffffff26; }
    .bar .sep { width: 1px; align-self: stretch; background: #ffffff33; margin: 4px 2px; }
    .bar .sw { width: 16px; height: 16px; border-radius: 50%; padding: 0; margin: 0 2px; border: 2px solid #ffffff55; }
    .bar .sw:hover { border-color: #fff; background-clip: padding-box; }
    .card { width: 380px; max-width: calc(100vw - 16px); background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 12px; box-shadow: 0 12px 32px #0004; padding: 12px 14px; }
    .card .hd { display: flex; gap: 8px; align-items: center; font-weight: 700; }
    .card .hd .grow { flex: 1; }
    .card .src { color: var(--mute); font-size: 12px; margin: 4px 0 8px; max-height: 3.2em; overflow: hidden; }
    .card .body { white-space: pre-wrap; line-height: 1.7; max-height: 320px; overflow: auto; user-select: text; }
    .card .body b { font-weight: 700; }
    .card .ft { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
    .card .ask { display: flex; gap: 6px; margin-top: 8px; }
    .card .ask input { flex: 1; min-width: 0; border: 1px solid var(--line); border-radius: 6px; padding: 4px 8px; background: var(--card); color: var(--fg); }
    .card .err { color: var(--bad); }
    .card .body.wcard { display: flex; white-space: normal; line-height: 1.5 }
    .card .body.wcard .hd { font-weight: 400; align-items: baseline }
    .menu2 { background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 10px 28px #0003; padding: 4px; min-width: 140px; }
    .menu2 button { display: block; width: 100%; text-align: left; background: transparent; color: inherit; font-weight: 400; }
    .menu2 button:hover { background: #d9762b24; }
  ` + (globalThis.ccWordCard?.CSS || '')
  let ui = null
  const uiRoot = () => (ui ||= mount(CSS))

  const st = { bar: null, card: null, menu: null, text: '', sentence: '', rect: null, range: null, job: 0 }

  const close = (what = 'all') => {
    if (what === 'all' || what === 'bar') (st.bar?.remove(), (st.bar = null))
    if (what === 'all' || what === 'card') (st.card?.remove(), (st.card = null), (st.job = 0))
    if (what === 'all' || what === 'menu') (st.menu?.remove(), (st.menu = null))
  }
  const place = (el, rect, w = 380, gap = 8) => {
    el.style.left = `${Math.max(8, Math.min(rect.left, innerWidth - w - 8))}px`
    const below = rect.bottom + gap
    el.style.top = `${below + 60 > innerHeight ? Math.max(8, rect.top - gap - (el.offsetHeight || 44)) : below}px`
  }

  // ---------- 整页文字索引（高亮用）：所有可见文字节点按顺序拼成一个长字符串 ----------

  function textNodes() {
    const out = []
    const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode(n) {
        const p = n.parentElement
        if (!p || !n.data) return NodeFilter.FILTER_REJECT
        if (p.closest('script,style,noscript,textarea,input,select,[contenteditable="true"]')) return NodeFilter.FILTER_REJECT
        return NodeFilter.FILTER_ACCEPT
      },
    })
    let at = 0
    for (let n = w.nextNode(); n; n = w.nextNode()) {
      out.push({ node: n, start: at })
      at += n.data.length
    }
    return { nodes: out, full: out.map((x) => x.node.data).join('') }
  }
  /** Range 的两端换成整页文字里的位置。 */
  function offsetsOf(range, idx) {
    const pos = (container, off) => {
      if (container.nodeType === Node.TEXT_NODE) {
        const f = idx.nodes.find((x) => x.node === container)
        return f ? f.start + off : -1
      }
      // 容器是元素：取它第 off 个子节点之前的第一个文字节点
      const child = container.childNodes[off] || null
      const f = idx.nodes.find((x) => (child ? child.contains(x.node) || (child.compareDocumentPosition(x.node) & Node.DOCUMENT_POSITION_FOLLOWING) : false))
      return f ? f.start : idx.full.length
    }
    return [pos(range.startContainer, range.startOffset), pos(range.endContainer, range.endOffset)]
  }

  const HL_STYLE = (bg) => `background:${bg};color:inherit;border-radius:2px;cursor:pointer;`
  /** 把整页文字里 [s,e) 这段用带颜色的 span 包起来（跨多个文字节点时逐段包）。 */
  function paint(idx, s, e, hl) {
    const bg = SEL.colorOf(hl.color).bg
    for (const { node, start } of idx.nodes) {
      const a = Math.max(s, start)
      const b = Math.min(e, start + node.data.length)
      if (a >= b || !node.parentNode) continue
      let n = node
      const lo = a - start
      const hi = b - start
      if (hi < n.data.length) n.splitText(hi)
      if (lo > 0) n = n.splitText(lo)
      const span = document.createElement('span')
      span.setAttribute('data-cc-hl', hl.id)
      span.style.cssText = HL_STYLE(bg)
      n.parentNode.insertBefore(span, n)
      span.append(n)
    }
  }
  const unpaint = (id) => {
    for (const sp of document.querySelectorAll(`span[data-cc-hl="${id}"]`)) {
      const p = sp.parentNode
      while (sp.firstChild) p.insertBefore(sp.firstChild, sp)
      sp.remove()
      p.normalize()
    }
  }
  const recolor = (id, color) => {
    for (const sp of document.querySelectorAll(`span[data-cc-hl="${id}"]`)) sp.style.background = SEL.colorOf(color).bg
  }

  const hlStore = {
    async all() {
      try {
        return (await api.storage.local.get('hlData')).hlData || {}
      } catch {
        return {}
      }
    },
    async save(all) {
      try {
        await api.storage.local.set({ hlData: SEL.prune(all) })
      } catch {
        toast(uiRoot(), '保存高亮失败（存储空间不足？）', true)
      }
    },
  }

  async function highlight(color) {
    const range = st.range
    if (!range) return
    const idx = textNodes()
    const [s, e] = offsetsOf(range, idx)
    if (s < 0 || e <= s) return toast(uiRoot(), '这段文字没法高亮（可能在输入框或特殊区域里）', true)
    const hl = SEL.makeHl({ full: idx.full, start: s, end: e, color, url: location.href, title: document.title })
    paint(idx, s, e, hl)
    const all = await hlStore.all()
    const k = SEL.pageKey(location.href)
    ;(all[k] ||= []).push(hl)
    await hlStore.save(all)
    getSelection()?.removeAllRanges()
    close()
  }

  async function restoreHighlights() {
    const all = await hlStore.all()
    const list = all[SEL.pageKey(location.href)] || []
    if (!list.length) return
    const done = new Set([...document.querySelectorAll('span[data-cc-hl]')].map((x) => x.getAttribute('data-cc-hl')))
    for (const hl of list) {
      if (done.has(hl.id)) continue
      const idx = textNodes() // 每次重建：前面的高亮会拆分文字节点
      const at = SEL.locate(idx.full, hl)
      if (at) paint(idx, at[0], at[1], hl)
    }
  }

  // 点已有的高亮：改色 / 复制 / 删除
  document.addEventListener('click', (e) => {
    const sp = e.target?.closest?.('span[data-cc-hl]')
    if (!sp || getSelection()?.toString()) return
    const id = sp.getAttribute('data-cc-hl')
    close()
    const r = sp.getBoundingClientRect()
    const text = [...document.querySelectorAll(`span[data-cc-hl="${id}"]`)].map((x) => x.textContent).join('')
    const swatches = SEL.COLORS.map((c) =>
      h('button', {
        class: 'sw',
        title: c.name,
        style: `background:${c.bg}`,
        onclick: async () => {
          recolor(id, c.id)
          const all = await hlStore.all()
          for (const l of all[SEL.pageKey(location.href)] || []) if (l.id === id) l.color = c.id
          await hlStore.save(all)
          close()
        },
      }),
    )
    st.menu = h('div', { class: 'layer bar' },
      ...swatches, h('span', { class: 'sep' }),
      h('button', { text: '复制', onclick: () => (navigator.clipboard?.writeText(text), close()) }),
      h('button', { text: '取消高亮', onclick: async () => {
        unpaint(id)
        const all = await hlStore.all()
        const k = SEL.pageKey(location.href)
        all[k] = (all[k] || []).filter((l) => l.id !== id)
        await hlStore.save(all)
        close()
      } }))
    uiRoot().add(st.menu)
    place(st.menu, r, 260)
  }, true)

  // ---------- 动作 ----------

  const clip = (s, n) => (s.length > n ? `${s.slice(0, n)}…` : s)

  async function pollJob(req) {
    const me = ++st.job
    return globalThis.ccUI.aiRun(req, { interval: 500, stop: () => st.job !== me || !st.card })
  }

  /** 打开结果卡片。run(setBody) 负责产生内容；extra(ft, getText) 往底部加按钮。 */
  async function openCard(title, run, extra) {
    close('card')
    close('menu')
    const bodyEl = h('div', { class: 'body', text: '正在处理…' })
    const ft = h('div', { class: 'ft' })
    const card = h('div', { class: 'layer card' },
      h('div', { class: 'hd' }, h('span', { text: title }), h('span', { class: 'grow' }), h('button', { class: 'sec', text: '✕', 'aria-label': '关闭', onclick: () => close('card') })),
      h('div', { class: 'src', text: clip(st.text, 120) }), bodyEl, ft)
    st.card = card
    uiRoot().add(card)
    place(card, st.rect || { left: innerWidth / 2 - 190, bottom: innerHeight / 3, top: innerHeight / 3 })
    let out = ''
    const set = (t, bad) => {
      out = t
      bodyEl.textContent = t
      bodyEl.classList.toggle('err', !!bad)
    }
    try {
      await run(set, bodyEl)
      const copyBtn = h('button', { class: 'sec', text: '复制', onclick: () => navigator.clipboard?.writeText(out).then(() => (copyBtn.textContent = '已复制'), () => (copyBtn.textContent = '复制失败')) })
      ft.append(copyBtn,
        h('button', { class: 'sec', text: '存为笔记', onclick: async () => {
          const r = await send({ type: 'cc-note', text: `${st.text}\n\n${title}：\n${out}\n\n来源：${document.title}\n${location.href}` })
          toast(uiRoot(), r.ok ? (r.pairing ? '请先在电脑上的清影里点“允许”完成配对' : '已存为清影便签') : r.error || '保存失败', !r.ok)
        } }))
      extra?.(ft, () => out, card)
    } catch (e) {
      if (e.message !== '已取消') set(e.message || '失败了', true)
    }
    place(card, st.rect || { left: innerWidth / 2 - 190, bottom: innerHeight / 3, top: innerHeight / 3 })
  }

  const doTranslate = () => openCard('翻译', async (set) => {
    const r = await send({ type: 'cc-tr', texts: [st.text], page: { host: location.host, title: document.title, sample: '' } })
    if (!r.ok) throw new Error(r.error || '翻译失败了')
    const t = r.texts?.[0] || ''
    set(t === st.text ? '这段文字已经是目标语言，不用翻译。' : t)
  })

  const doDefine = () => {
    if (SEL.isWord(st.text) && globalThis.ccWordCard) {
      const WC = globalThis.ccWordCard
      // 和字幕层的查词浮窗是同一张词卡（释义、本句意思、本句、例句、加入生词本）
      return openCard('释义', async (set, bodyEl) => {
        const me = ++st.job
        const res = await WC.lookup(st.text, st.sentence, { stop: () => st.job !== me || !st.card })
        set([`${res.lemma || st.text}　${res.phonetic || ''}`.trim(), [res.pos, res.meaning].filter(Boolean).join(' '), res.context_meaning && `在这句话里：${res.context_meaning}`].filter(Boolean).join('\n'))
        const { nodes } = WC.render({
          h,
          word: st.text,
          sentence: st.sentence,
          res,
          onSave: async (lemma) => {
            const r = await WC.save(lemma, res, { sentence: st.sentence })
            if (!r.ok) toast(uiRoot(), r.error || '加入生词本失败', true)
            return !!r.ok
          },
        })
        bodyEl.classList.add('wcard')
        bodyEl.replaceChildren(...nodes)
      })
    }
    return openCard('释义', async (set) => {
      const r = await pollJob({ kind: 'explain', text: st.text, context: st.sentence, title: document.title })
      set(r.text || '')
    })
  }

  const doSpeak = () => {
    try {
      speechSynthesis.cancel()
      const u = new SpeechSynthesisUtterance(st.text.slice(0, 2000))
      u.lang = SEL.speechLang(st.text)
      speechSynthesis.speak(u)
      toast(uiRoot(), `正在朗读（${u.lang}），再点一次停止`, false, 1800)
      st.speaking = !st.speaking
      if (!st.speaking) speechSynthesis.cancel()
    } catch {
      toast(uiRoot(), '这个浏览器不能朗读', true)
    }
  }

  const doAsk = (fmt) => openCard(fmt === 'summarize' ? '总结' : 'AI 搜索', async (set) => {
    const r = await pollJob({ kind: 'answer', format: fmt, text: st.text, context: st.sentence, title: document.title })
    set(r.text || '')
  }, (ft, get, card) => {
    if (fmt !== 'summarize') ft.append(h('button', { class: 'sec', text: '用搜索引擎打开', onclick: () => window.open(SEL.searchUrl(st.text), '_blank', 'noopener') }))
    // 继续追问：把上一轮的回答带上
    const input = h('input', { type: 'text', placeholder: '继续追问…', onkeydown: (e) => e.key === 'Enter' && !e.isComposing && go() })
    const ask = h('div', { class: 'ask' }, input, h('button', { text: '问', onclick: () => go() }))
    card.append(ask)
    async function go() {
      const q = input.value.trim()
      if (!q) return
      input.value = ''
      const prev = get()
      const body = card.querySelector('.body')
      body.textContent = `${prev}\n\n问：${q}\n正在想…`
      try {
        const r = await pollJob({ kind: 'answer', format: 'search', text: q, context: `Original selection: ${st.text}\nPrevious answer: ${prev}`, title: document.title })
        body.textContent = `${prev}\n\n问：${q}\n${r.text || ''}`
      } catch (e) {
        body.textContent = `${prev}\n\n${e.message}`
      }
    }
  })

  // ---------- 工具条 ----------

  function sentenceAround(sel) {
    const el = sel.anchorNode?.nodeType === Node.TEXT_NODE ? sel.anchorNode.parentElement : sel.anchorNode
    const block = el?.closest?.('p,li,td,div,article,section,blockquote,h1,h2,h3,h4') || el
    return String(block?.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 400)
  }

  function showBar() {
    const sel = getSelection()
    const text = String(sel || '').replace(/\s+/g, ' ').trim()
    if (!sel?.rangeCount || text.length < 2 || text.length > 4000) return
    const el = sel.anchorNode?.nodeType === Node.TEXT_NODE ? sel.anchorNode.parentElement : sel.anchorNode
    if (el?.closest?.('input,textarea,[contenteditable="true"],[contenteditable=""]')) return
    const range = sel.getRangeAt(0)
    const rect = range.getBoundingClientRect()
    if (!rect.width && !rect.height) return
    Object.assign(st, { text, sentence: sentenceAround(sel), rect, range: range.cloneRange() })
    close()
    const swatches = SEL.COLORS.map((c) => h('button', { class: 'sw', title: `高亮：${c.name}`, 'aria-label': `高亮 ${c.name}`, style: `background:${c.bg}`, onmousedown: (e) => e.preventDefault(), onclick: () => highlight(c.id) }))
    const mk = (label, fn, title) => h('button', { text: label, title, onmousedown: (e) => e.preventDefault(), onclick: fn })
    st.bar = h('div', { class: 'layer bar' },
      mk('翻译', doTranslate, '翻译选中的文字'),
      mk('释义', doDefine, SEL.isWord(text) ? '查词典，并可加入生词本' : '用一两句话解释'),
      mk('朗读', doSpeak, '朗读选中的文字（再点停止）'),
      mk('AI 搜索', () => doAsk('search'), '把选中的文字当作问题或话题，请模型回答'),
      text.length >= 60 ? mk('总结', () => doAsk('summarize'), '压成几个要点') : null,
      h('span', { class: 'sep' }), ...swatches,
      mk('⋯', () => moreMenu(), '更多'))
    uiRoot().add(st.bar)
    place(st.bar, rect, 380, 6)
  }

  function moreMenu() {
    close('menu')
    const m = h('div', { class: 'layer menu2' },
      h('button', { text: '复制', onclick: () => (navigator.clipboard?.writeText(st.text), close()) }),
      h('button', { text: '存为清影便签', onclick: async () => {
        const r = await send({ type: 'cc-note', text: `${st.text}\n\n来源：${document.title}\n${location.href}` })
        toast(uiRoot(), r.ok ? '已存为清影便签' : r.error || '保存失败', !r.ok)
        close()
      } }),
      h('button', { text: '本页高亮 → 复制 Markdown', onclick: async () => {
        const list = (await hlStore.all())[SEL.pageKey(location.href)] || []
        if (!list.length) return toast(uiRoot(), '这个页面还没有高亮', true)
        await navigator.clipboard?.writeText(SEL.toMarkdown(list, { title: document.title, url: location.href }))
        toast(uiRoot(), `已复制 ${list.length} 条高亮`)
        close()
      } }),
      h('button', { text: '管理全部高亮…', onclick: () => (send({ type: 'cc-open-highlights' }), close()) }))
    st.menu = m
    uiRoot().add(m)
    place(m, st.bar ? st.bar.getBoundingClientRect() : st.rect, 160, 4)
  }

  document.addEventListener('mouseup', (e) => {
    if (e.composedPath().some((n) => n === ui?.host)) return
    setTimeout(showBar, 20)
  }, true)
  document.addEventListener('mousedown', (e) => {
    if (ui && e.composedPath().some((n) => n === ui.host)) return
    close('bar')
    close('menu')
    if (!e.target?.closest?.('span[data-cc-hl]')) close('card')
  }, true)
  document.addEventListener('keydown', (e) => e.key === 'Escape' && close(), true)
  addEventListener('scroll', () => st.bar && close('bar'), { passive: true })

  // 打开页面后恢复高亮；单页应用晚一点再补一次
  const restore = () => restoreHighlights().catch(() => {})
  if (document.readyState === 'complete') setTimeout(restore, 400)
  else addEventListener('load', () => setTimeout(restore, 400))
  setTimeout(restore, 3500)
})()
