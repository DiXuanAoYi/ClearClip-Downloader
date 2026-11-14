// 写作浮窗的拆分模块（由 content/writing.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 面板的结果（差异预览）、采用 / 写回、问题、事实核对和历史视图。
;(() => {
  const P = ((globalThis.ccParts ||= {}).write ||= {})
  P.result = (C) => {
    const { W, api, h } = C
    const asArray = (...a) => C.asArray(...a)
    const chip = (...a) => C.chip(...a)
    const closePanel = (...a) => C.closePanel(...a)
    const header = (...a) => C.header(...a)
    const isField = (...a) => C.isField(...a)
    const render = (...a) => C.render(...a)
    const run = (...a) => C.run(...a)
    const scopeText = (...a) => C.scopeText(...a)
    const showBadge = (...a) => C.showBadge(...a)

    // ---- 结果：差异预览
    function buildParts() {
      const r = C.st.res
      r.parts = W.hunks(W.diffOps(r.src, r.cands[r.idx].text))
      r.rejected = new Set()
    }

    const finalText = () => {
      const r = C.st.res
      return r.mode === 'new' || C.st.sel.action === 'continue' ? r.cands[r.idx].text : W.compose(r.parts, r.rejected)
    }

    function viewResult() {
      const r = C.st.res
      const c = r.cands[r.idx]
      const isCont = C.st.sel.action === 'continue'
      const a = W.actionOf(C.st.sel.action)
      const head = h('div', { class: 'acts' }, h('span', { class: 'mut', text: `${W.countText(r.src)} 字 → ${W.countText(c.text)} 字` }))
      const kids = [header(a?.name || '结果', head)]
      const bd = h('div', { class: 'bd' })
      const bar = h('div', { class: 'acts' })
      if (r.cands.length > 1) {
        bar.append(h('span', { class: 'mut', text: '候选' }))
        r.cands.forEach((_, i) => bar.append(chip(`方案 ${i + 1}`, r.idx === i, () => ((r.idx = i), buildParts(), render()))))
      }
      if (!isCont) {
        const modes = h('span', { class: 'acts', style: 'margin-left:auto' })
        modes.append(chip('差异', r.mode === 'diff', () => ((r.mode = 'diff'), render())), chip('只看新文', r.mode === 'new', () => ((r.mode = 'new'), render())), chip('对比原文', r.mode === 'cmp', () => ((r.mode = 'cmp'), render())))
        bar.append(modes)
      }
      bd.append(bar)
      if (c.guard?.warn) {
        const bits = []
        if (c.guard.missing?.length) bits.push(`原文里的 ${c.guard.missing.slice(0, 6).join('、')} 在结果里不见了，请核对。`)
        const free = ['continue', 'reply'].includes(C.st.sel.action) // 续写、回复的长短和原文无关
        const shrinks = ['summarize', 'shorten', 'concise', 'translate'].includes(C.st.sel.action)
        if (!free && (c.guard.ratio > 4 || (!shrinks && c.guard.ratio < 0.25))) bits.push(c.guard.ratio > 1 ? '结果比原文长了很多，可能多出内容。' : '结果比原文短了很多，可能漏掉内容。')
        if (bits.length) bd.append(h('div', { class: 'warn', text: '⚠ ' + bits.join(' ') }))
      }
      const box = h('div', { class: 'box' })
      if (isCont) {
        box.append(h('span', { class: 'add', text: c.text }))
      } else if (r.mode === 'new') {
        box.textContent = c.text
      } else if (r.mode === 'cmp') {
        box.append(h('div', { class: 'mut', text: '原文' }), h('div', { text: r.src, style: 'opacity:.75;white-space:pre-wrap;margin-bottom:8px' }), h('div', { class: 'mut', text: '结果' }), h('div', { text: c.text, style: 'white-space:pre-wrap' }))
      } else {
        for (const p of r.parts) {
          if (p.eq != null) box.append(p.eq)
          else {
            const off = r.rejected.has(p.id)
            const span = h('span', { class: `hk${off ? ' off' : ''}`, title: off ? '点击改为采用' : '点击放弃这处改动', onclick: () => ((off ? r.rejected.delete(p.id) : r.rejected.add(p.id)), render()) })
            if (p.del) span.append(h('span', { class: 'del', text: p.del }))
            if (p.add) span.append(h('span', { class: 'add', text: p.add }))
            box.append(span)
          }
        }
      }
      bd.append(box)
      if (!isCont && r.mode === 'diff') {
        const n = r.parts.filter((p) => p.id != null).length
        bd.append(h('div', { class: 'mut', text: n ? `共 ${n} 处改动，点击某一处可单独放弃或重新采用。` : '没有改动。' }))
      }
      const readonly = C.st.snap.readonly
      const acts = h('div', { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' })
      if (!readonly) {
        if (isCont) acts.append(h('button', { text: '插入到光标处', onclick: () => apply('insert') }))
        else acts.append(h('button', { text: C.st.scope === 'sel' ? '替换选中' : '替换原文', onclick: () => apply('replace') }), h('button', { class: 'sec', text: '插入到光标处', onclick: () => apply('insert') }))
      }
      acts.append(h('button', { class: 'sec', text: '复制', onclick: (e) => copy(finalText(), e.target) }), h('button', { class: 'sec', style: 'margin-left:auto', text: '再来一个', onclick: () => run() }), h('button', { class: 'sec', text: '返回', onclick: () => ((C.st.view = 'options'), render()) }))
      bd.append(acts)
      bd.append(h('div', { class: 'mut', text: readonly ? '这里不是输入框，只能复制结果。' : '替换后可用 Ctrl+Z 撤销。结果自动保存到“历史记录”。' }))
      kids.push(bd)
      return kids
    }

    async function copy(text, btn) {
      try {
        await navigator.clipboard.writeText(text)
        btn.textContent = '已复制'
      } catch {
        btn.textContent = '复制失败，请手动选择'
      }
      remember(text)
    }

    async function remember(out) {
      // 面板可能马上关掉，所以先把要记的东西取出来再异步保存
      const rec = { src: String(C.st?.res?.src || '').slice(0, 2000), out: String(out).slice(0, 4000), action: C.st?.sel?.action || '', host: location.hostname, ts: Date.now(), fav: false }
      try {
        const { writeHistory } = await api.storage.local.get('writeHistory')
        await api.storage.local.set({ writeHistory: W.pushHistory(writeHistory, rec) })
      } catch {
        // 忽略
      }
    }

    // ---- 写回
    function apply(mode) {
      const r = C.st.res
      const text = finalText()
      const where = mode === 'insert' || C.st.sel.action === 'continue' ? 'insert' : C.st.scope === 'sel' ? 'sel' : 'all'
      if (!writeBack(C.st.snap, text, where)) {
        C.st.note = '写不进这个输入框，已为你复制到剪贴板，请手动粘贴。'
        navigator.clipboard?.writeText(text).catch(() => {})
        C.st.view = 'options'
        render()
        return
      }
      remember(text)
      closePanel()
      if (C.target) showBadge()
      void r
    }

    /** 写进输入框。用 execCommand('insertText')，这样网页自己的撤销栈和输入事件都正常；不行再直接改值并补发事件。 */
    function writeBack(snap, text, where) {
      const el = snap.el
      if (!el || !el.isConnected) return false
      try {
        el.focus({ preventScroll: true })
        if (isField(el)) {
          const len = el.value.length
          let s = snap.start
          let e = snap.end
          if (where === 'all') {
            s = 0
            e = len
          } else if (where === 'insert') {
            s = e
          } else if (s === e) {
            return false
          }
          el.setSelectionRange(s, e)
          let ok = false
          try {
            ok = document.execCommand('insertText', false, text)
          } catch {
            ok = false
          }
          if (!ok || el.value.length !== len - (e - s) + text.length) {
            const proto = Object.getPrototypeOf(el)
            const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set
            const nv = el.value.slice(0, s) + text + el.value.slice(e)
            if (setter) setter.call(el, nv)
            else el.value = nv
            el.setSelectionRange(s + text.length, s + text.length)
            el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }))
          }
          return true
        }
        const sel = getSelection()
        if (!sel) return false
        if (where === 'all') {
          sel.selectAllChildren(el)
        } else if (snap.range) {
          sel.removeAllRanges()
          const rg = snap.range.cloneRange()
          if (where === 'insert') rg.collapse(false)
          sel.addRange(rg)
        } else if (where === 'sel') {
          return false
        } else {
          sel.selectAllChildren(el)
          sel.collapseToEnd()
        }
        let ok = false
        try {
          ok = document.execCommand('insertText', false, text)
        } catch {
          ok = false
        }
        if (!ok) {
          const rg = sel.getRangeAt(0)
          rg.deleteContents()
          rg.insertNode(document.createTextNode(text))
          rg.collapse(false)
          el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }))
        }
        return true
      } catch {
        return false
      }
    }

    // ---- 语法检查
    function viewIssues() {
      const r = C.st.res
      const text = scopeText()
      const kids = [header('语法检查', h('span', { class: 'mut', text: r.issues.length ? `发现 ${r.issues.length} 处` : '没有发现问题' }))]
      const bd = h('div', { class: 'bd' })
      if (!r.issues.length) {
        bd.append(h('div', { text: '没有发现需要修改的地方。' }), h('div', { class: 'acts' }, h('button', { class: 'sec', text: '返回', onclick: () => ((C.st.view = 'options'), render()) })))
        kids.push(bd)
        return kids
      }
      const chars = Array.from(text)
      const box = h('div', { class: 'box' })
      let pos = 0
      r.issues.forEach((it, i) => {
        box.append(chars.slice(pos, it.start).join(''))
        const cls = it.type === 'awkward' ? 'e2' : it.type === 'style' ? 'e3' : 'e1'
        box.append(h('span', { class: r.skipped.has(i) ? '' : cls, text: chars.slice(it.start, it.end).join(''), title: it.reason }))
        pos = it.end
      })
      box.append(chars.slice(pos).join(''))
      bd.append(box)
      const legend = h('div', { class: 'acts mut' })
      legend.append(h('span', { class: 'e1', text: '错别字/语法' }), h('span', { class: 'e2', text: '不通顺' }), h('span', { class: 'e3', text: '风格建议' }))
      bd.append(legend)
      r.issues.forEach((it, i) => {
        const skip = r.skipped.has(i)
        bd.append(
          h(
            'div',
            { class: `card${skip ? ' skip' : ''}` },
            h('div', { class: 'acts' },
              h('span', { class: 'grow' }, h('span', { class: 'del', text: it.from }), ' → ', h('b', { text: it.to })),
              chip(skip ? '忽略' : '采用', !skip, () => ((skip ? r.skipped.delete(i) : r.skipped.add(i)), render()), 'x'),
            ),
            h('div', { class: 'mut', text: it.reason || '' }),
          ),
        )
      })
      const readonly = C.st.snap.readonly
      const acts = h('div', { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' })
      const fixed = () => W.applyIssues(text, r.issues, r.skipped)
      if (!readonly) acts.append(h('button', { text: '应用所选建议', onclick: () => {
        C.st.res = { src: text, cands: [{ text: fixed() }], idx: 0, mode: 'new', rejected: new Set(), parts: [] }
        apply('replace')
      } }))
      acts.append(h('button', { class: 'sec', text: '复制修改后的文字', onclick: (e) => copy(fixed(), e.target) }), h('button', { class: 'sec', style: 'margin-left:auto', text: '返回', onclick: () => ((C.st.view = 'options'), render()) }))
      bd.append(acts)
      kids.push(bd)
      return kids
    }

    // ---- 统计数据
    function viewFacts() {
      const f = C.st.facts
      const kids = [header('加入统计数据', h('span', { class: 'mut', text: '来自开放数据库，不凭空编造' }))]
      const bd = h('div', { class: 'bd' })
      if (!f.list.length) {
        bd.append(h('div', { class: 'warn', text: f.note || '没有找到可用的数据。' }))
      } else {
        bd.append(h('div', { class: 'mut', text: '勾选要写进文中的数据。数字和出处原样使用，不会被改动。' }))
        f.list.forEach((x, i) => {
          const cb = h('input', { type: 'checkbox', checked: f.picked.has(i), onchange: (e) => (e.target.checked ? f.picked.add(i) : f.picked.delete(i)) })
          bd.append(
            h('label', { class: 'card', style: 'cursor:pointer' },
              h('div', { class: 'acts' }, cb, h('span', { class: 'grow', text: x.text })),
              h('div', { class: 'mut' }, `来源：${x.source} · 指标代码 ${x.code} · `, h('a', { href: x.url, target: '_blank', rel: 'noopener noreferrer', text: '查看原始数据' })),
            ),
          )
        })
        if (f.note) bd.append(h('div', { class: 'warn', text: f.note }))
      }
      const acts = h('div', { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' })
      if (f.list.length) acts.append(h('button', { text: '写入文中', onclick: () => {
        const facts = [...f.picked].sort((a, b) => a - b).map((i) => f.list[i])
        if (!facts.length) return
        run({ facts })
      } }))
      acts.append(h('button', { class: 'sec', style: 'margin-left:auto', text: '返回', onclick: () => ((C.st.view = 'options'), render()) }))
      bd.append(acts)
      kids.push(bd)
      return kids
    }

    // ---- 历史
    async function showHistory() {
      try {
        const { writeHistory } = await api.storage.local.get('writeHistory')
        C.st.history = asArray(writeHistory)
      } catch {
        C.st.history = []
      }
      C.st.view = 'history'
      render()
    }

    function viewHistory() {
      const kids = [header('历史记录', h('span', { class: 'mut', text: '只保存在这台电脑的浏览器里' }))]
      const bd = h('div', { class: 'bd' })
      if (!C.st.history.length) bd.append(h('div', { class: 'mut', text: '还没有记录。采用或复制结果后会出现在这里。' }))
      C.st.history.slice(0, 20).forEach((x, i) => {
        bd.append(
          h('div', { class: 'card' },
            h('div', { class: 'mut', text: `${new Date(x.ts).toLocaleString()} · ${x.host || ''}${x.fav ? ' · ★' : ''}` }),
            h('div', { text: String(x.out).slice(0, 160) }),
            h('div', { class: 'acts' },
              h('button', { class: 'sec chip x', text: '复制', onclick: (e) => copy(x.out, e.target) }),
              h('button', { class: 'sec chip x', text: x.fav ? '取消收藏' : '收藏', onclick: async () => {
                x.fav = !x.fav
                await api.storage.local.set({ writeHistory: C.st.history }).catch(() => {})
                render()
              } }),
              h('button', { class: 'sec chip x', text: '删除', onclick: async () => {
                C.st.history.splice(i, 1)
                await api.storage.local.set({ writeHistory: C.st.history }).catch(() => {})
                render()
              } }),
            ),
          ),
        )
      })
      bd.append(h('div', { class: 'acts' },
        h('button', { class: 'sec', text: '清空（保留收藏）', onclick: async () => {
          C.st.history = C.st.history.filter((x) => x.fav)
          await api.storage.local.set({ writeHistory: C.st.history }).catch(() => {})
          render()
        } }),
        h('button', { class: 'sec', style: 'margin-left:auto', text: '返回', onclick: () => ((C.st.view = 'options'), render()) }),
      ))
      kids.push(bd)
      return kids
    }


    Object.assign(C, { buildParts, showHistory, viewResult, viewIssues, viewFacts, viewHistory })
  }
})()
