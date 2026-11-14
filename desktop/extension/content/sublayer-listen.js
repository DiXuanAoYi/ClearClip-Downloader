// 页面字幕层的拆分模块（由 content/sublayer.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 生词本抽屉、导出，以及听写 / 跟读练习。
;(() => {
  const P = ((globalThis.ccParts ||= {}).sub ||= {})
  P.listen = (C) => {
    const { L, LN, S, cap, drEl, h, lsEl, send, store, toast, ui } = C
    const hideBubble = (...a) => C.hideBubble(...a)
    const paint = (...a) => C.paint(...a)
    const togglePop = (...a) => C.togglePop(...a)


    let drOpen = false
    async function toggleDrawer(open) {
      drOpen = open ?? !drOpen
      drEl.classList.toggle('open', drOpen)
      if (!drOpen) return
      togglePop(false)
      const m = S.video?.getBoundingClientRect()
      drEl.style.left = `${Math.max(8, Math.min(window.innerWidth - 348, (m ? m.right : window.innerWidth) - 348))}px`
      drEl.style.top = `${Math.max(8, (m ? m.top : 0) + 8)}px`
      await renderDrawer('')
    }
    async function renderDrawer(q) {
      const search = h('input', { placeholder: '搜索单词 / 释义 / 原句', value: q, oninput: (e) => (clearTimeout(search._t), (search._t = setTimeout(() => renderDrawer(e.target.value), 300))) })
      const lst = h('div', { class: 'lst' }, h('div', { class: 'tip', text: '读取中…' }))
      const exp = (f, n) => h('button', { class: 'b', text: n, onclick: () => exportVocab(f) })
      drEl.replaceChildren(
        h('div', { class: 'r', style: 'display:flex;justify-content:space-between;align-items:center' }, h('b', { text: '生词本' }), h('button', { class: 'b', text: '关闭', onclick: () => toggleDrawer(false) })),
        search,
        lst,
        h('div', { class: 'ft' }, exp('anki', '导出 Anki'), exp('csv', 'CSV'), exp('md', 'Markdown'), exp('json', 'JSON')),
      )
      const r = await send({ type: 'cc-vocab', op: 'list', q })
      if (!r.ok) return lst.replaceChildren(h('div', { class: 'tip', text: r.error }))
      const st = r.stats || {}
      lst.replaceChildren(
        h('div', { class: 'tip', text: `共 ${st.total ?? 0} 个词，今天该复习 ${st.due ?? 0} 个。复习在清影应用的“生词本”里。` }),
        ...(r.words || []).slice(0, 80).map((w) => {
          const here = w.sourceUrl === location.href
          return h(
            'div',
            { class: 'it2' },
            h('b', { text: `${w.word} ${w.phonetic || ''}` }),
            h('small', { text: `${w.pos || ''} ${w.meaning || ''}${w.ctxMeaning ? `（本句：${w.ctxMeaning}）` : ''}` }),
            w.sentence ? h('small', { text: w.sentence }) : null,
            h(
              'div',
              { class: 'ac' },
              here && S.video ? h('a', { text: `跳到 ${LN.fmtTime(w.timeSec * 1000)}`, onclick: () => ((S.video.currentTime = w.timeSec), S.video.play()) }) : h('small', { text: w.sourceTitle ? `${w.sourceTitle.slice(0, 20)} ${LN.fmtTime(w.timeSec * 1000)}` : '' }),
              h('a', { text: '删除', onclick: async () => ((await send({ type: 'cc-vocab', op: 'remove', id: w.id })), S.saved.delete(LN.norm(w.word)), S.ver++, renderDrawer(q)) }),
            ),
          )
        }),
      )
    }
    const dataToBytes = (d) => Uint8Array.from(atob(d.split(',')[1] || ''), (c) => c.charCodeAt(0))
    function download(blob, name) {
      const a = h('a', { href: URL.createObjectURL(blob), download: name })
      document.body.append(a)
      a.click()
      a.remove()
    }
    async function exportVocab(format) {
      const r = await send({ type: 'cc-vocab', op: 'export', format })
      if (!r.ok) return toast(ui, r.error || '导出失败', true)
      if (format === 'anki' && r.zip) {
        // 清影把卡片和录过的原声打成了一个 zip
        download(new Blob([dataToBytes(`data:application/zip;base64,${r.zip}`)], { type: 'application/zip' }), '生词本-anki.zip')
        return toast(ui, `已导出 Anki 包：卡片 + ${r.clips || 0} 段原声。把音频复制到 Anki 的 collection.media，再导入 .txt`, false, 6000)
      }
      const name = `生词本.${r.ext}`
      download(new Blob([r.text], { type: 'text/plain;charset=utf-8' }), name)
      toast(ui, `已导出 ${name}`)
    }


    // ---------- 精听 ----------

    function toggleListen(on) {
      const want = on ?? !S.ls
      if (!want) return stopListen()
      if (!S.lines.length) return toast(ui, '还没有字幕，没法精听', true)
      togglePop(false)
      const v = S.video
      const i = Math.max(0, L.indexAt(S.lines, v.currentTime * 1000, S.prefs.delay))
      S.ls = { i, done: 0, loops: 3, rate: 0.75, hideO: true, hideT: true, revealed: false, waiting: false, typed: '', result: null, prevRate: v.playbackRate, pauseEnd: true, skip: new Set(), prog: {} }
      store.get(progKey(), {}).then((p) => S.ls && (S.ls.prog = p || {}) && renderListen())
      document.addEventListener('keydown', lsKeys, true)
      hideBubble(true)
      cap.dataset.k = ''
      playLine()
      renderListen()
    }
    function lsKeys(e) {
      const ls = S.ls
      if (!ls) return
      const t = e.composedPath()[0]
      if (t && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)) return
      if (e.ctrlKey || e.metaKey || e.altKey) return
      const k = e.key
      const hit = k.toLowerCase() === 'r' ? () => (Object.assign(ls, { done: 0 }), playLine()) : k === ' ' ? () => (S.video.paused ? S.video.play() : S.video.pause()) : k === 'ArrowLeft' ? () => gotoLine(-1) : k === 'ArrowRight' ? () => gotoLine(1) : k === 'Tab' ? () => ((ls.revealed = true), (cap.dataset.k = ''), renderListen()) : null
      if (!hit) return
      e.preventDefault()
      e.stopImmediatePropagation()
      hit()
    }
    function stopListen() {
      const ls = S.ls
      if (!ls) return
      document.removeEventListener('keydown', lsKeys, true)
      if (S.video) S.video.playbackRate = ls.prevRate || 1
      S.ls = null
      lsEl.classList.remove('open')
      cap.dataset.k = ''
      paint()
    }
    function playLine() {
      const ls = S.ls
      const sp = LN.span(S.lines, ls.i, S.prefs.delay)
      if (!sp || !S.video) return
      S.video.playbackRate = ls.rate
      S.video.currentTime = Math.max(0, sp.start / 1000)
      ls.waiting = false
      S.video.play().catch(() => {})
    }
    function gotoLine(d) {
      const ls = S.ls
      ls.i = Math.min(S.lines.length - 1, Math.max(0, ls.i + d))
      Object.assign(ls, { done: 0, revealed: false, typed: '', result: null })
      cap.dataset.k = ''
      playLine()
      renderListen()
    }
    function submitDictation(box) {
      const ls = S.ls
      ls.typed = box.value
      ls.result = LN.dictationDiff(S.lines[ls.i].text, ls.typed)
      saveProg(ls.result)
      ls.revealed = true
      S.video?.pause()
      cap.dataset.k = ''
      renderListen()
    }
    async function addMissedWord(w) {
      const ls = S.ls
      const line = S.lines[ls.i]
      const r = await send({ type: 'cc-vocab', op: 'add', word: { word: LN.norm(w), sentence: line.text, sourceTitle: document.title, sourceUrl: location.href, timeSec: line.t / 1000 } })
      if (r.ok) (S.saved.add(LN.norm(w)), S.ver++)
      return !!r.ok
    }
    const progKey = () => `listenProg:${location.origin}${location.pathname}${location.search}`.slice(0, 200)
    function saveProg(res) {
      const ls = S.ls
      if (!ls || !res) return
      ls.prog[ls.i] = { right: res.right, total: res.total }
      store.set(progKey(), ls.prog)
    }
    function renderListen() {
      const ls = S.ls
      if (!ls) return
      const v = S.video?.getBoundingClientRect()
      lsEl.classList.add('open')
      lsEl.style.left = `${Math.max(8, Math.min(window.innerWidth - 428, (v ? v.left + v.width / 2 : window.innerWidth / 2) - 210))}px`
      lsEl.style.top = `${Math.max(8, Math.min(window.innerHeight - 420, v ? v.bottom + 8 : 100))}px`
      const box = h('textarea', { placeholder: '把听到的写下来，Enter 提交对照原句' })
      box.value = ls.typed
      box.addEventListener('keydown', (e) => {
        e.stopPropagation()
        if (e.isComposing) return
        if (e.key === 'Enter' && !e.shiftKey) (e.preventDefault(), submitDictation(box))
        else if (e.key === 'Tab') (e.preventDefault(), (ls.revealed = true), (cap.dataset.k = ''), renderListen())
        else if (e.altKey && e.key.toLowerCase() === 'r') (e.preventDefault(), Object.assign(ls, { done: 0 }), playLine())
        else if (e.altKey && e.key === 'ArrowLeft') (e.preventDefault(), gotoLine(-1))
        else if (e.altKey && e.key === 'ArrowRight') (e.preventDefault(), gotoLine(1))
      })
      box.addEventListener('keyup', (e) => e.stopPropagation())
      const sel = (opts, cur, fn) => h('select', { onchange: (e) => fn(e.target.value) }, ...opts.map(([v, t]) => h('option', { value: v, text: t, selected: String(v) === String(cur) ? '' : null })))
      const chk = (t, on, fn) => h('label', {}, h('input', { type: 'checkbox', checked: on ? '' : null, onchange: (e) => fn(e.target.checked) }), document.createTextNode(t))
      const res = ls.result
      const loopTxt = ls.loops ? `单句循环 ${Math.min(ls.done + 1, ls.loops)}/${ls.loops}` : '单句循环 ∞'
      const missed = (res?.missed || []).filter((w) => !ls.skip.has(LN.norm(w)))
      const lo = Math.max(0, ls.i - 2)
      const rows = []
      for (let k = lo; k < Math.min(S.lines.length, lo + 8); k++) {
        const pg = ls.prog[k]
        rows.push(h('div', { class: `pr${k === ls.i ? ' cur' : ''}`, onclick: () => ((ls.i = k), Object.assign(ls, { done: 0, revealed: false, typed: '', result: null }), (cap.dataset.k = ''), playLine(), renderListen()) }, h('span', { text: LN.fmtTime(S.lines[k].t) }), h('span', { text: S.lines[k].text }), h('span', { text: pg ? `${pg.right}/${pg.total}` : k === ls.i ? '▶' : '' })))
      }
      lsEl.replaceChildren(
        h('div', { class: 'r' }, h('b', { text: `精听 · 第 ${ls.i + 1} 句` }), h('span', { class: 'sc', text: `共 ${S.lines.length} 句` }), h('span', { class: 'pill2', text: loopTxt }), h('span', { class: 'sp' }), h('button', { text: '退出', onclick: () => stopListen() })),
        h('div', { class: 'keys' }, h('kbd', { text: 'R' }), '重播 ', h('kbd', { text: 'Space' }), '暂停 ', h('kbd', { text: '←' }), h('kbd', { text: '→' }), '上/下句 ', h('kbd', { text: 'Tab' }), '揭晓 ', h('kbd', { text: 'Enter' }), '提交', h('div', { text: ls.hideO ? '原文已遮住，只听声音。输入框里用 Alt+R / Alt+←→。' : '' })),
        h(
          'div',
          { class: 'r' },
          h('button', { text: '上一句', onclick: () => gotoLine(-1) }),
          h('button', { class: 'p', text: '重播本句', onclick: () => (Object.assign(ls, { done: 0 }), playLine()) }),
          h('button', { text: '下一句', onclick: () => gotoLine(1) }),
          h('span', { class: 'sp' }),
          sel([[0.5, '0.5×'], [0.75, '0.75×'], [1, '1×'], [1.25, '1.25×']], ls.rate, (v) => ((ls.rate = Number(v)), S.video && (S.video.playbackRate = ls.rate))),
          sel([[1, '播 1 次'], [3, '循环 3 次'], [0, '一直循环']], ls.loops, (v) => (ls.loops = Number(v))),
        ),
        h('div', { class: 'r' }, chk('显示原文', !ls.hideO, (v) => ((ls.hideO = !v), (cap.dataset.k = ''))), chk('显示译文', !ls.hideT, (v) => ((ls.hideT = !v), (cap.dataset.k = ''))), chk('句末暂停', ls.pauseEnd, (v) => (ls.pauseEnd = v))),
        box,
        h('div', { class: 'r' }, h('button', { class: 'p', text: '提交', onclick: () => submitDictation(box) }), h('button', { text: '揭晓原文', onclick: () => ((ls.revealed = true), (cap.dataset.k = ''), renderListen()) }), h('span', { class: 'sc', text: ls.waiting ? '本句播完了，写下来，或点下一句' : '' })),
        res
          ? h(
              'div',
              {},
              h('div', { class: 'res' }, ...res.parts.map((p) => (/\p{L}/u.test(p.text) ? h('span', { class: p.ok ? 'ok' : 'no', text: p.text }) : document.createTextNode(p.text)))),
              h('div', { class: 'r' }, h('span', { class: 'sc', text: `${res.right}/${res.total}　听对 ${res.right} 个词${res.total - res.right ? `，错 ${res.total - res.right} 个` : '，全对'}` })),
              missed.length
                ? h('div', { class: 'chips' }, ...missed.map((w) => h('span', { class: 'chip' }, h('b', { text: w }), h('a', { text: S.saved.has(LN.norm(w)) ? '已加入' : '加入生词本', onclick: async (e) => ((await addMissedWord(w)) && (e.target.textContent = '已加入')) }), h('a', { text: '已掌握', onclick: () => (ls.skip.add(LN.norm(w)), renderListen()) }))))
                : null,
              h('div', { class: 'r' }, h('button', { text: '再听一遍', onclick: () => (Object.assign(ls, { done: 0, revealed: false, typed: '', result: null }), (cap.dataset.k = ''), playLine(), renderListen()) }), h('button', { class: 'p', text: '下一句', onclick: () => gotoLine(1) })),
            )
          : null,
        h('div', { class: 'prog' }, h('div', { class: 'sc', text: `本视频进度 · 已练 ${Object.keys(ls.prog).length} / ${S.lines.length} 句` }), ...rows),
      )
    }
    function listenTick() {
      const ls = S.ls
      const v = S.video
      if (!ls || !v || v.paused || ls.waiting || ls.revealed) return
      const sp = LN.span(S.lines, ls.i, S.prefs.delay)
      const act = LN.listenStep(v.currentTime * 1000, sp, ls.done, ls.loops)
      if (act === 'again') {
        ls.done++
        v.currentTime = sp.start / 1000
      } else if (act === 'stop') {
        if (!ls.pauseEnd && ls.i < S.lines.length - 1) return gotoLine(1)
        v.pause()
        ls.waiting = true
        renderListen()
      }
    }

    Object.assign(C, { toggleListen, toggleDrawer, stopListen, listenTick })
  }
})()
