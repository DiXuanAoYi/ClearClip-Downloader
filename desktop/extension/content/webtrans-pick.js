// 网页翻译的拆分模块（由 content/webtrans.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 划词翻译卡片、多服务对比的翻译框、输入框翻译。
;(() => {
  const P = ((globalThis.ccParts ||= {}).trans ||= {})
  P.pick = (C) => {
    const { S, T, h, send, toast } = C
    const pageInfo = (...a) => C.pageInfo(...a)
    const uiRoot = (...a) => C.uiRoot(...a)

    // ---------- 划词翻译 ----------
    // 划词后的入口只有划词工具条的“翻译”（以前这里还有一个单独的“译”小按钮，和工具条重复）；
    // 工具条、右键菜单“翻译选中的文字”都打开下面这同一张卡片（多个服务对比、转到翻译窗、存为笔记）。

    let selCard = null
    const closeCard = () => {
      selCard?.remove()
      selCard = null
    }

    document.addEventListener('mousedown', (e) => {
      if (!e.composedPath().some((n) => n === S.ui?.host)) closeCard()
    }, true)
    document.addEventListener('keydown', (e) => e.key === 'Escape' && closeCard(), true)

    async function translateSelection(text, rect) {
      closeCard()
      const list = h('div', { class: 'cmp' })
      const copyBtn = h('button', { class: 'sec', text: '复制', disabled: true })
      const cmpBtn = h('button', { class: 'sec', text: '对比其他服务', onclick: () => compareMore() })
      const noteBtn = h('button', { class: 'sec', text: '存为笔记', disabled: true, onclick: async () => {
        const r = await send({ type: 'cc-note', text: `${text}\n\n翻译：\n${best}\n\n来源：${document.title}\n${location.href}` })
        noteBtn.textContent = r.ok ? (r.pairing ? '请先在清影里允许配对' : '已存为笔记') : r.error || '保存失败'
      } })
      selCard = h('div', { class: 'card' }, h('div', { class: 'src', text: text.slice(0, 200) }), list,
        h('div', { class: 'bar' }, h('span', { class: 'grow', text: T.nameOf(S.cfg.target) }), T.activeEngines(S.cfg).length > 1 ? cmpBtn : null, h('button', { class: 'sec', text: '翻译窗', title: '在可拖动的翻译窗里继续（可改文字、换语言）', onclick: () => openBox(text) }), noteBtn, copyBtn, h('button', { class: 'sec', text: '关闭', onclick: closeCard })))
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
          noteBtn.disabled = false
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


    Object.assign(C, { openBox, translateSelection })
  }
})()
