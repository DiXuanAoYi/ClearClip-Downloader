// 视频摘要面板：文稿页（搜索、悬停译文、顺句 / 断句）。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.transcript = ({
    S, V, actsBar, addNote, copy, h, isVideo, langSelect, onSelect, outLabel, render, saveFile, savePrefs, seek,
    sel, send, setData, syncSubLayer, task, tick, toast, trackBar, translateDoc, ui
  }) => {
    const getSel = () => String((ui.root.getSelection && ui.root.getSelection()) || getSelection() || '').trim()

    function tabTranscript() {
      const d = S.data
      const found = S.query.trim() ? V.searchLines(d.lines, S.query) : []
      const hits = found.length ? new Set(found) : null
      if (S.hit >= found.length) S.hit = 0
      const nowIdx = found[S.hit]
      const tr = S.res.translated
      const mode = S.prefs.view
      const body = h('div', { id: 'lines', onmouseup: onSelect })
      d.lines.forEach((l, i) => {
        const [main, sub] = V.viewLine(l.text, tr?.[i], mode)
        body.append(h('div', { class: `ln${hits?.has(i) ? ' hit' : ''}${i === nowIdx ? ' now' : ''}`, 'data-i': i, onclick: () => !getSel() && seek(l.t), onmouseenter: (e) => hovIn(l, i, e.currentTarget), onmouseleave: hovOut },
          isVideo() && h('span', { class: 'tm', text: V.fmtClock(l.t) }),
          h('span', { style: 'flex:1' }, sub ? h('span', { class: 'mn' }, ...marks(main)) : h('span', {}, ...marks(main)), sub ? h('span', { class: 'tr' }, ...marks(sub)) : null),
          h('button', { class: 'rec', title: '把这句记成笔记', text: '＋', onclick: (e) => (e.stopPropagation(), addNote({ t: l.t, text: l.text })) })))
      })
      return h('div', {},
        h('div', { class: 'stick' }, trackBar(d), h('div', { class: 'tools' },
          h('input', { type: 'text', id: 'tsearch', placeholder: '搜索文稿（/）', value: S.query, style: 'flex:1', oninput: (e) => ((S.query = e.target.value), (S.hit = 0), (S.keepFocus = true), (S.jumpHit = true), render()), onkeydown: (e) => e.key === 'Enter' && !e.isComposing && (e.preventDefault(), stepHit(e.shiftKey ? -1 : 1)) }),
          found.length > 0 && h('span', { class: 'mute', text: `${S.hit + 1}/${found.length}` }),
          S.query.trim() && !found.length && h('span', { class: 'mute', text: '没有' }),
          found.length > 1 && h('button', { class: 'sec', text: '↑', title: '上一处（Shift+Enter）', onclick: () => stepHit(-1) }),
          found.length > 1 && h('button', { class: 'sec', text: '↓', title: '下一处（Enter）', onclick: () => stepHit(1) }),
        ),
        h('div', { class: 'tools' },
          S.detached && S.prefs.follow && isVideo() && h('button', { text: '回到当前句', onclick: () => ((S.detached = false), tick(true)) }),
          isVideo() && h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.follow, onchange: (e) => ((S.prefs.follow = e.target.checked), savePrefs()) }), ' 跟随播放'),
          isVideo() && h('label', { class: 'mute', title: '读到字幕后，把文稿同时作为字幕层叠在视频上（双语、调字号位置、导出）' }, h('input', { type: 'checkbox', checked: S.prefs.autoSub !== false, onchange: (e) => ((S.prefs.autoSub = e.target.checked), savePrefs(), e.target.checked ? syncSubLayer(S.data) : globalThis.ccSubLayerRun?.destroy()) }), ' 显示在视频上'),
          langSelect('翻译成哪种语言（任意语言互译，自动识别原文）', () => ((S.res.translated = null), render())),
          h('button', { class: 'sec', text: tr ? '重新翻译' : outLabel() ? `翻译成${outLabel()}` : '翻译（自动方向）', disabled: S.busy, onclick: () => translateDoc(d) }),
          sel([['orig', '原文'], ['tr', '译文'], ['both', '双语']], tr ? mode : 'orig', (v) => {
            S.prefs.view = v
            savePrefs()
            render()
            // 还没有译文就选了译文 / 双语：直接去翻译（原文永远保留，不会被译文替换）
            if (v !== 'orig' && !S.res.translated && !S.busy) translateDoc(d)
          }, '文稿显示：原文 / 译文 / 双语'),
          h('button', { class: 'sec', text: '重新断句', title: '把太长的句子按标点和长度拆开（时间按字数分配）；不改字，也不用重新识别', disabled: S.busy, onclick: resegment }),
          h('button', { class: 'sec', text: d.raw ? '还原顺句前' : '顺句', title: d.raw ? '恢复识别出来的原样' : '给没有标点的字幕（自动识别）补标点、改同音错字，行数不变', disabled: S.busy, onclick: d.raw ? unpunctuate : punctuate }),
        )),
        h('div', { class: 'mute', text: '选中一段文字可以解释、存为笔记；鼠标移到一行上点 ＋ 记笔记。' }),
        body,
        actsBar(
          h('button', { text: '复制文稿', onclick: () => copy(V.toTxt(d.lines), '文稿') }),
          isVideo() && h('button', { class: 'sec', text: '.srt', onclick: () => saveFile(V.fileName(d.title, 'srt'), V.toSrt(d.lines)) }),
          h('button', { class: 'sec', text: '.txt', onclick: () => saveFile(V.fileName(d.title, 'txt'), V.toTxt(d.lines)) })),
      )
    }

    /** 把文字里的搜索词用 <mark> 标出来。 */
    function marks(text) {
      return V.splitMarks(text, S.query).map((p) => (p.hit ? h('mark', { text: p.s }) : p.s))
    }

    // 文稿里鼠标停在一句上：旁边出现小悬浮窗，给出这一句的译文（可复制、可在翻译窗里继续）
    let hovCard = null
    let hovTimer = 0
    let hovHide = 0
    function hovClose() {
      clearTimeout(hovTimer)
      clearTimeout(hovHide)
      hovCard?.remove()
      hovCard = null
    }
    function hovOut() {
      clearTimeout(hovTimer)
      clearTimeout(hovHide)
      hovHide = setTimeout(hovClose, 260)
    }
    function hovIn(l, i, el) {
      clearTimeout(hovHide)
      clearTimeout(hovTimer)
      if (getSel()?.toString()) return
      hovTimer = setTimeout(async () => {
        hovClose()
        const known = S.res.translated?.[i]
        const out = h('div', { class: 'out', text: known || '正在翻译…' })
        const copy = h('button', { class: 'sec', text: '复制', onclick: () => navigator.clipboard?.writeText(out.textContent).then(() => (copy.textContent = '已复制'), () => (copy.textContent = '复制失败')) })
        const card = h('div', { class: 'hov', onmouseenter: () => clearTimeout(hovHide), onmouseleave: hovOut }, h('div', { class: 'src', text: l.text }), out, h('div', { class: 'bar' }, copy, h('button', { class: 'sec', text: '翻译窗', onclick: () => (hovClose(), send({ type: 'cc-open-transbox', text: l.text })) })))
        hovCard = card
        ui.add(card)
        const r = el.getBoundingClientRect()
        const w = Math.min(340, innerWidth - 16)
        card.style.left = `${r.left - w - 8 >= 8 ? r.left - w - 8 : Math.max(8, Math.min(r.left, innerWidth - w - 8))}px`
        card.style.top = `${Math.max(8, Math.min(r.top, innerHeight - 150))}px`
        if (known) return
        const target = S.prefs.lang.trim() || S.status?.lang || '简体中文'
        const res = await send({ type: 'cc-tr', texts: [l.text], target, engine: 'clearclip' })
        if (hovCard !== card) return
        out.textContent = res.ok ? res.texts?.[0] || '' : res.error || '翻译失败了'
      }, 450)
    }

    function stepHit(d) {
      const n = V.searchLines(S.data.lines, S.query).length
      if (!n) return
      S.hit = (S.hit + d + n) % n
      S.jumpHit = true
      S.keepFocus = ui.root.activeElement?.id === 'tsearch'
      render()
    }


    function resegment() {
      const d = S.data
      const lines = V.resegmentLines(d.lines, d.durationMs, S.prefs.cueLevel || 'standard')
      if (lines.length === d.lines.length) return toast(ui, '每一句都已经够短，不用拆')
      setData({ ...d, lines, raw: d.raw || d.lines })
      S.res.translated = null
      toast(ui, `已重新断句：${d.lines.length} → ${lines.length} 句`)
      render()
    }
    async function punctuate() {
      const before = S.data.lines
      await task('punctuate', {}, (x) => {
        const out = (x.lines || []).map((l) => ({ t: l.t, text: l.text })).filter((l) => l.text)
        if (!out.length) throw new Error('没有得到结果')
        S.data.raw = before
        S.data.lines = out
        S.res.translated = null
      }, false)
    }

    function unpunctuate() {
      S.data.lines = S.data.raw
      S.data.raw = null
      S.res.translated = null
      render()
    }

    return { getSel, tabTranscript }
  }
})()
