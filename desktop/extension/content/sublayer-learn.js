// 页面字幕层的拆分模块（由 content/sublayer.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 字幕学习：查词气泡、生词、跟读片段、同步缓存。
;(() => {
  const P = ((globalThis.ccParts ||= {}).sub ||= {})
  P.learn = (C) => {
    const { L, LN, S, V, WC, cap, h, send, store, toast, ui, wpop } = C


    // ---------- 字幕学习：查词 / 生词 ----------

    function wordNodes(text, lineIdx) {
      return LN.segment(text).map((p) => {
        if (!p.word) return document.createTextNode(p.text)
        const w = LN.norm(p.text)
        const cls = S.saved.has(w) ? 'w sv' : S.unf.has(w) ? 'w unf' : 'w'
        return h('i', { class: cls, 'data-w': p.text, 'data-i': String(lineIdx), style: 'font-style:normal', text: p.text })
      })
    }
    const wordText = (el) => el.dataset.w

    async function setLearn(v) {
      S.learn = v === 'off' ? { on: false, level: 0 } : { on: true, level: Number(v) }
      await store.set('learnPrefs', S.learn)
      S.ver++
      hideBubble(true)
      if (S.learn.on) refreshMarks()
    }

    async function refreshMarks() {
      if (!S.learn.on || !S.lines.length) return
      const saved = await send({ type: 'cc-vocab', op: 'words' })
      if (saved.ok) S.saved = new Set(saved.words || [])
      S.unf = new Set()
      const rank = LN.levelRank(S.learn.level)
      if (rank) {
        for (const g of LN.chunk(LN.uniqueWords(S.lines), 300)) {
          const r = await send({ type: 'cc-vocab', op: 'unfamiliar', words: g, minRank: rank })
          if (r.ok) (r.words || []).forEach((w) => S.unf.add(w))
          if (S.closed) return
        }
      }
      S.ver++
    }

    // 悬停 / 点词时暂停视频，词卡关闭后接着播（只恢复我们自己暂停的）
    let pausedByWord = false
    function pauseForWord() {
      const v = S.video
      if (v && !v.paused && !v.ended) {
        v.pause()
        pausedByWord = true
      }
    }
    function resumeAfterWord() {
      if (!pausedByWord) return
      pausedByWord = false
      try {
        void S.video?.play()
      } catch {
        /* 页面不让播就算了 */
      }
    }
    const hideBubble = (now = false) => wpop.close(now)

    function showBubble(el) {
      const word = wordText(el)
      const li = Number(el.dataset.i)
      const sentence = String(S.lines[li]?.text || '')
      wpop.open(el, {
        id: `${word}|${li}`,
        word,
        sentence,
        lookup: { lang: S.target, stop: () => S.closed },
        build: (res) => {
          const guess = res.lemma && LN.norm(res.lemma) !== LN.norm(word) ? res.lemma : word
          const t0 = S.lines[li]?.t || 0
          return {
            saved: S.saved.has(LN.norm(guess)) || S.saved.has(LN.norm(word)),
            onSave: (lemma) => saveWord(lemma, res, li),
            extraExamples: WC.lineExamples(h, S.lines, li, word, guess, LN.fmtTime, (j) => seekLine(j, true)),
            actions: [
              h('button', { class: 'p', text: `跳到这句 ${LN.fmtTime(t0)}`, onclick: () => seekLine(li, true) }),
              h('button', { text: '播放原声', onclick: () => playClip(li) }),
              h('button', { text: '朗读', onclick: () => WC.speak(guess) }),
              h('button', { text: '复制', onclick: () => navigator.clipboard?.writeText(guess).catch(() => {}) }),
              h('button', { text: WC.hasClip(guess) ? '♪ 已录原声' : '录原声（Anki）', title: '录下这一句的原声，导出 Anki 时带上', onclick: (e) => recordClip(guess, li, e.target) }),
            ],
          }
        },
      })
    }

    // ---------- 跳到这句 / 播放原声 / 录原声 ----------

    function seekLine(li, play) {
      const sp = LN.span(S.lines, li, S.prefs.delay)
      if (!sp || !S.video) return
      S.video.currentTime = Math.max(0, sp.start / 1000)
      if (play) S.video.play().catch(() => {})
    }
    function playClip(li) {
      const sp = LN.span(S.lines, li, S.prefs.delay)
      if (!sp || !S.video) return
      S.clipEnd = sp.end / 1000
      S.video.currentTime = Math.max(0, sp.start / 1000)
      S.video.play().catch(() => {})
    }
    /** 把这一句的原声录下来（实现在 wordcard.js，和文稿里的查词浮窗共用）。 */
    async function recordClip(word, li, btn) {
      try {
        await WC.recordClip(S.video, word, LN.span(S.lines, li, S.prefs.delay), btn)
      } catch (e) {
        toast(ui, e.message || '录音失败', true, 3000)
      }
    }

    async function saveWord(lemma, res, li) {
      const line = S.lines[li] || { t: 0, text: '' }
      const r = await WC.save(lemma, res, { sentence: line.text, timeSec: (line.t || 0) / 1000 })
      if (!r.ok) {
        toast(ui, r.error || '收藏失败', true)
        return false
      }
      S.saved.add(LN.norm(lemma))
      S.ver++
      toast(ui, `已收藏“${lemma}”，可以在清影的“生词本”里复习`)
      syncCache(false)
      return true
    }

    /** 把这个视频的摘要会话和双语字幕同步到清影，生词本里就能看到、也能在别的浏览器恢复。 */
    let cacheSynced = 0
    async function syncCache(force) {
      try {
        if (!force && Date.now() - cacheSynced < 60000) return
        const i = await send({ type: 'cc-videocache', op: 'info', url: location.href })
        if (!i.ok || (!i.auto && !force)) return
        const vk = V.videoKey(location.href)
        const sess = Object.values(await store.get('vsumSessions', {})).find((e) => e && e.vk === vk)
        const trans = S.trans?.some(Boolean) ? S.trans : null
        const payload = {
          url: location.href,
          title: document.title,
          site: V.videoIdOf(location.href)?.site || '',
          summary: sess ? { key: sess.key, ts: sess.ts, vk: sess.vk, url: sess.url, data: sess.data } : null,
          subs: trans ? { key: L.cacheKey(V.videoKey(location.href), S.target, S.pv), target: S.target, lines: S.lines.map((l) => ({ t: l.t, text: l.text })), trans } : null,
          srcLang: '',
          dstLang: S.target || '',
        }
        if (!payload.summary && !payload.subs) return
        const r = await send({ type: 'cc-videocache', op: 'put', payload })
        if (r.ok) cacheSynced = Date.now()
      } catch {
        // 清影没开或没配置：下次收藏再试
      }
    }

    cap.addEventListener('mouseover', (e) => {
      const el = e.target?.closest?.('.w')
      if (!el) return
      clearTimeout(cap._hv)
      pauseForWord()
      cap._hv = setTimeout(() => showBubble(el), 300)
    })
    cap.addEventListener('mouseout', (e) => {
      if (!e.target?.closest?.('.w')) return
      clearTimeout(cap._hv)
      if (!wpop.isOpen()) resumeAfterWord()
      hideBubble()
    })
    cap.addEventListener('click', (e) => {
      const el = e.target?.closest?.('.w')
      if (!el) return
      // 点词：暂停视频，立刻显示
      pauseForWord()
      clearTimeout(cap._hv)
      showBubble(el)
    })

    // ---------- 生词本抽屉 ----------

    Object.assign(C, { setLearn, resumeAfterWord, wordNodes, hideBubble, refreshMarks })
  }
})()
