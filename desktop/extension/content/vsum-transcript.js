// 视频摘要面板：文稿页（搜索、悬停单词查词、顺句 / 断句）。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.transcript = ({
    S, V, actsBar, addNote, copy, h, isVideo, langSelect, onSelect, outLabel, render, saveFile, savePrefs, seek,
    sel, send, setData, syncSubLayer, task, tick, toast, trackBar, translateDoc, ui, video
  }) => {
    const WC = globalThis.ccWordCard
    const LN = globalThis.ccLearn
    const VS = globalThis.ccVSession
    const getSel = () => String((ui.root.getSelection && ui.root.getSelection()) || getSelection() || '').trim()

    function tabTranscript() {
      const d = S.data
      const found = S.query.trim() ? V.searchLines(d.lines, S.query) : []
      const hits = found.length ? new Set(found) : null
      if (S.hit >= found.length) S.hit = 0
      const nowIdx = found[S.hit]
      const tr = S.res.translated
      const mode = S.prefs.view
      // 每句前面的时间戳；没有时间戳时文字要占满整行
      const showTm = isVideo()
      const body = h('div', { id: 'lines', onmouseup: onSelect, onmouseover: wordIn, onmouseout: wordOut })
      d.lines.forEach((l, i) => {
        const [main, sub] = V.viewLine(l.text, tr?.[i], mode)
        body.append(h('div', { class: `ln${showTm ? '' : ' nt'}${hits?.has(i) ? ' hit' : ''}${i === nowIdx ? ' now' : ''}${l.src && l.src !== l.text ? ' fx' : ''}`, 'data-i': i, title: l.src && l.src !== l.text ? `已修改，原文：${l.src}` : '双击修改这一句', onclick: () => !getSel() && seek(l.t), ondblclick: (e) => startEdit(e, i) },
          showTm && h('span', { class: 'tm', text: V.fmtClock(l.t) }),
          h('span', { style: 'flex:1' }, sub ? h('span', { class: 'mn' }, ...words(main)) : h('span', { class: 'one' }, ...words(main)), sub ? h('span', { class: 'tr' }, ...words(sub)) : null),
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
          isVideo() && h('label', { class: 'mute', title: '读到字幕后，把文稿同时作为字幕层叠在视频上（双语、调字号位置、导出）' }, h('input', { type: 'checkbox', checked: S.prefs.autoSub !== false, onchange: (e) => ((S.prefs.autoSub = e.target.checked), savePrefs(), e.target.checked ? syncSubLayer(S.data, true) : globalThis.ccSubLayerRun?.destroy()) }), ' 显示在视频上'),
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
          h('button', { class: 'sec', text: d.fixBefore ? `撤销纠错（${d.fixed} 处）` : 'AI 纠错', title: d.fixBefore ? '恢复纠错之前的文字' : '结合上下文和视频标题，改正识别错的字词、人名和术语（不改标点和句子）；改过的句子下面有虚线，鼠标停上去看原文。也可以双击任意一句手动改', disabled: S.busy, onclick: d.fixBefore ? unfix : fixAsr }),
        )),
        d.cleaned && h('div', { class: 'tools', id: 'cleanbar' },
          h('span', { class: 'mute', text: S.showOrig ? `已显示原始文稿（共 ${d.orig.length} 句）` : `已清理 ${(d.cleaned.hearing || 0) + (d.cleaned.filler || 0) + (d.cleaned.emptied || 0)} 处${cleanDetail(d.cleaned)}` }),
          h('button', { class: 'sec', id: 'cleantoggle', text: S.showOrig ? '显示清理后' : '显示原始', title: '清理只去掉非语音说明和口水词；摘要、翻译等送给 AI 的文字始终是清理后的', onclick: toggleOrig })),
        h('div', { class: 'mute', text: '鼠标停在英文单词上查词；选中一段文字可以解释、存为笔记；鼠标移到一行上点 ＋ 记笔记。' }),
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

    /** 一行文字：搜索时标出搜索词；不搜索时把英文单词包起来，鼠标停上去查词。 */
    function words(text) {
      if (S.query.trim() || !LN) return marks(text)
      return LN.segment(text).map((p) => (p.word ? h('i', { class: 'w', text: p.text }) : p.text))
    }

    // 文稿里鼠标停在英文单词上：弹出和视频字幕层同一个查词浮窗；双语时把这个词在另一行里对应的译文标出来
    const wpop = WC.popover(ui, {
      onHide: () => {
        clearHl()
        resumeAfterWord()
      },
    })
    // 悬停在单词上（词卡打开）时暂停视频，词卡关闭后接着播；只恢复我们自己暂停的，用户自己暂停的不动。
    // 从一个词移到下一个词时词卡会先关再开，所以恢复稍等一下，期间又开了新词就取消。
    let pausedByWord = false
    let resumeTimer = 0
    function pauseForWord() {
      clearTimeout(resumeTimer)
      const v = isVideo() ? video() : null
      if (v && !v.paused && !v.ended) {
        v.pause()
        pausedByWord = true
      }
    }
    function resumeAfterWord() {
      if (!pausedByWord) return
      clearTimeout(resumeTimer)
      resumeTimer = setTimeout(() => {
        if (!pausedByWord) return
        pausedByWord = false
        try {
          Promise.resolve(video()?.play()).catch(() => {})
        } catch {
          /* 页面不让播就算了 */
        }
      }, 250)
    }
    let hovTimer = 0
    let hl = null
    let saved = null
    function clearHl() {
      if (!hl) return
      hl.word.classList.remove('on')
      if (hl.other?.isConnected) hl.other.replaceChildren(hl.otherText)
      hl = null
    }
    function wordIn(e) {
      const el = e.target?.closest?.('i.w')
      if (!el) return
      clearTimeout(hovTimer)
      if (getSel()) return
      hovTimer = setTimeout(() => openWord(el), 300)
    }
    function wordOut(e) {
      if (!e.target?.closest?.('i.w')) return
      clearTimeout(hovTimer)
      wpop.close()
    }
    async function openWord(el) {
      const row = el.closest('.ln')
      const li = Number(row?.dataset.i)
      const l = S.data?.lines?.[li]
      if (!l || !el.isConnected) return
      const part = el.parentElement
      const other = part.classList.contains('mn') ? row.querySelector('.tr') : part.classList.contains('tr') ? row.querySelector('.mn') : null
      const sentence = part.textContent
      const trans = other?.textContent || ''
      const isOrig = sentence === l.text
      const word = el.textContent
      clearHl()
      pauseForWord()
      hl = { word: el, other, otherText: trans }
      el.classList.add('on')
      if (!saved) {
        saved = new Set()
        send({ type: 'cc-vocab', op: 'words' }).then((r) => r?.ok && (r.words || []).forEach((w) => saved.add(LN.norm(w))), () => {})
      }
      const lines = S.data.lines
      const v = isVideo() ? video() : null
      const id = `${word}|${li}|${trans ? 1 : 0}`
      wpop.open(el, {
        id,
        word,
        sentence,
        lookup: { lang: S.prefs.lang.trim() || S.status?.lang || '简体中文', trans, stop: () => wpop.current() !== id },
        build: (res) => {
          const guess = res.lemma && LN.norm(res.lemma) !== LN.norm(word) ? res.lemma : word
          return {
            saved: saved.has(LN.norm(guess)) || saved.has(LN.norm(word)),
            onSave: async (lemma) => {
              const r = await WC.save(lemma, res, { sentence: l.text, timeSec: (l.t || 0) / 1000 })
              if (!r?.ok) {
                toast(ui, r?.error || '加入生词本失败', true)
                return false
              }
              saved.add(LN.norm(lemma))
              return true
            },
            extraExamples: isOrig ? WC.lineExamples(h, lines, li, word, guess, LN.fmtTime, (j) => seek(lines[j].t)) : [],
            actions: [
              isVideo() && h('button', { class: 'p', text: `跳到这句 ${LN.fmtTime(l.t || 0)}`, onclick: () => seek(l.t) }),
              v && h('button', { text: '播放原声', onclick: () => playClip(v, LN.span(lines, li)) }),
              h('button', { text: '朗读', onclick: () => WC.speak(guess) }),
              h('button', { text: '复制', onclick: () => navigator.clipboard?.writeText(guess).catch(() => {}) }),
              v && isOrig && h('button', { text: WC.hasClip(guess) ? '♪ 已录原声' : '录原声（Anki）', title: '录下这一句的原声，导出 Anki 时带上', onclick: (e) => WC.recordClip(v, guess, LN.span(lines, li), e.target).catch((err) => toast(ui, err.message || '录音失败', true, 3000)) }),
            ].filter(Boolean),
          }
        },
        onResult: (res) => {
          if (hl?.word !== el || !other?.isConnected) return
          const a = WC.alignedIn(trans, res)
          const at = a ? trans.indexOf(a) : -1
          if (at < 0) return
          other.replaceChildren(trans.slice(0, at), h('mark', { class: 'al', text: a }), trans.slice(at + a.length))
        },
      })
    }
    /** 只播这一句：到句末自动停。 */
    function playClip(v, sp) {
      if (!sp) return
      const start = sp.start / 1000
      const end = sp.end / 1000
      const stop = () => {
        const t = v.currentTime
        // 用户自己拖到别处了：不再管
        if (t < start - 1 || t > end + 1.5) return v.removeEventListener('timeupdate', stop)
        if (t >= end) {
          v.pause()
          v.removeEventListener('timeupdate', stop)
        }
      }
      v.removeEventListener('timeupdate', v.__ccClipStop || stop)
      v.__ccClipStop = stop
      v.addEventListener('timeupdate', stop)
      v.currentTime = Math.max(0, start)
      v.play?.().catch(() => {})
    }

    function stepHit(d) {
      const n = V.searchLines(S.data.lines, S.query).length
      if (!n) return
      S.hit = (S.hit + d + n) % n
      S.jumpHit = true
      S.keepFocus = ui.root.activeElement?.id === 'tsearch'
      render()
    }


    function cleanDetail(c) {
      const parts = []
      if (c.hearing) parts.push(`非语音说明 ${c.hearing} 行`)
      if (c.filler) parts.push(`口水词 ${c.filler} 行`)
      if (c.emptied) parts.push(`整行丢掉 ${c.emptied} 行`)
      return parts.length ? `（${parts.join('、')}）` : ''
    }
    /** 在“清理后”和“原始”两份文稿之间切换：显示的行换掉，时间不变；已有的译文按行对应，换掉后清空 */
    function toggleOrig() {
      const d = S.data
      if (!d?.orig) return
      S.showOrig = !S.showOrig
      const tmp = d.lines
      d.lines = d.orig
      d.orig = tmp
      S.res.translated = null
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

    // ---------- 修改文稿：双击一句手动改；AI 结合上下文纠错 ----------
    // 改动按“视频 + 时间 + 读到的原文字”存在本地，下次打开同一个视频的同一份字幕自动带出来
    const EDITS_KEY = 'vsumEdits'
    const editStore = () => globalThis.ccUI.api.storage.local
    const editVk = () => V.videoKey(S.data?.url || location.href)
    async function rememberEdit(t, from, to) {
      try {
        const all = (await editStore().get(EDITS_KEY))[EDITS_KEY] || {}
        const k = editVk()
        let m = { ...(all[k] || {}) }
        delete m[`${t}|${from}`] // 先删再放：最近改的排在最后，超限时丢最早的
        if (to !== from) m[`${t}|${from}`] = to
        m = V.trimEdits(m)
        delete all[k]
        if (Object.keys(m).length) all[k] = m
        // 最多记 40 个视频，用得最少的先丢；写失败会提示用户
        await globalThis.ccUI.storeSet(ui, { [EDITS_KEY]: V.lruTrim(all, 40, 1_500_000) })
      } catch (e) {
        globalThis.ccUI.log('rememberEdit', e) // 存不了就只在这次有效
      }
    }
    /** 读到文稿后把以前改过的带回来（在预处理之后调用，用显示的文字对应） */
    async function applyEdits(d) {
      try {
        const m = (await editStore().get(EDITS_KEY))[EDITS_KEY]?.[editVk()]
        if (!m || S.data !== d || d.streaming) return
        let n = 0
        d.lines = d.lines.map((l) => {
          const to = m[`${l.t}|${l.text}`]
          if (to === undefined || to === l.text) return l
          n++
          return { ...l, text: to, src: l.text }
        })
        if (n) {
          S.res.translated = null
          render()
          syncSubLayer(d)
        }
      } catch {
        // 没有就算了
      }
    }
    let syncTimer = 0
    /** 改完停一下再同步到视频上的字幕层（连续改几句不会每句都重新翻译） */
    function syncLater() {
      clearTimeout(syncTimer)
      syncTimer = setTimeout(() => S.data && syncSubLayer(S.data), 1500)
    }
    function startEdit(e, i) {
      if (e.target.closest?.('button')) return
      const d = S.data
      const l = d?.lines[i]
      if (!l) return
      try {
        ;(ui.root.getSelection?.() || getSelection())?.removeAllRanges()
      } catch {
        /* 选区清不掉不影响 */
      }
      clearTimeout(hovTimer)
      wpop.close()
      const row = e.currentTarget
      const t0 = l.t
      S.editing = true
      const box = row.children[row.children.length - 2]
      const ta = h('textarea', { class: 'ledit', rows: 1 })
      ta.value = l.text
      const fit = () => {
        ta.style.height = 'auto'
        ta.style.height = `${ta.scrollHeight}px`
      }
      let done = false
      const finish = (save) => {
        if (done) return
        done = true
        S.editing = false
        if (save) saveLine(t0, i, ta.value)
        else render()
        // 编辑期间识别又来了新内容：现在补画
        if (S.pendingRender) (S.pendingRender = false, render())
      }
      ta.addEventListener('input', fit)
      ta.addEventListener('keydown', (ev) => {
        ev.stopPropagation()
        if (ev.isComposing) return
        if (ev.key === 'Enter' && !ev.shiftKey) (ev.preventDefault(), finish(true))
        else if (ev.key === 'Escape') (ev.preventDefault(), finish(false))
      })
      ta.addEventListener('blur', () => finish(true))
      for (const t of ['click', 'dblclick', 'mouseup', 'mouseover']) ta.addEventListener(t, (ev) => ev.stopPropagation())
      box.replaceChildren(ta)
      fit()
      ta.focus()
      ta.setSelectionRange(ta.value.length, ta.value.length)
    }
    function saveLine(t0, i0, raw) {
      const d = S.data
      // 识别进行中行会增减：按时间找回这一句
      const i = d?.lines[i0]?.t === t0 ? i0 : d?.lines.findIndex((x) => x.t === t0) ?? -1
      const l = d?.lines[i]
      const v = String(raw || '').replace(/\s*\n\s*/g, ' ').trim()
      if (!l || !v || v === l.text) return render()
      const src = l.src || l.text
      d.lines = d.lines.map((x, j) => (j === i ? { ...x, text: v, src } : x))
      // 这一句原来的译文已经不对了：清掉，其他句的译文保留
      if (S.res.translated) S.res.translated = S.res.translated.map((x, j) => (j === i ? '' : x))
      if (d.streaming) {
        // 识别还没完成：先记下，之后每次新结果、以及最终结果出来都按时间把这句的修改带上
        ;(S.liveEdits ||= new Map()).set(t0, v)
      } else if (!VS.editLine(i, v, 'vsum')) {
        // 会话里的文稿和这里对不上（比如还没同步）：整份同步，改动单独记
        rememberEdit(l.t, src, v)
        syncLater()
      }
      render()
      toast(ui, '已修改这一句')
    }
    async function fixAsr() {
      const before = S.data.lines
      let changed = 0
      await task('fixasr', {}, (x) => {
        const out = x.lines || []
        let k = 0
        const lines = before.map((l) => {
          if (!l.text.trim()) return l
          const text = String(out[k++]?.text || '').trim()
          // 只接受小改动：模型把一句话改写得面目全非的不要
          const r = text.length / Math.max(1, l.text.length)
          if (!text || text === l.text || r > 1.6 || r < 0.5) return l
          changed++
          return { ...l, text, src: l.src || l.text }
        })
        if (!changed) return
        S.data.fixBefore = before
        S.data.fixed = changed
        S.data.lines = lines
        S.res.translated = null
        for (const [j, l] of lines.entries()) if (l !== before[j]) rememberEdit(l.t, l.src, l.text)
        syncLater()
      }, false)
      if (!S.closed) toast(ui, changed ? `纠正了 ${changed} 处（改过的句子下有虚线，可点“撤销纠错”）` : '没有发现需要改的错字')
    }
    function unfix() {
      const d = S.data
      if (!d?.fixBefore) return
      for (const [j, l] of d.lines.entries()) if (l.text !== d.fixBefore[j]?.text) rememberEdit(l.t, l.src || d.fixBefore[j]?.text || l.text, d.fixBefore[j]?.text ?? l.text)
      d.lines = d.fixBefore
      d.fixBefore = null
      d.fixed = 0
      S.res.translated = null
      syncLater()
      render()
    }

    return { getSel, tabTranscript, punctuate, applyEdits, rememberEdit }
  }
})()
