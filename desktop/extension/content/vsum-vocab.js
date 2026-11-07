// 视频摘要面板：生词页。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.vocab = ({
    S, V, actsBar, addNote, h, localKey, render, saveFile, seek, sel, send, toast, ui
  }) => {
    // ---------- 生词页：字幕学习里收藏的词，按本视频 / 全部查看 ----------
    const VOCAB = { list: null, err: '', scope: 'video', hard: true, loading: false }
    async function loadVocab() {
      VOCAB.loading = true
      const r = await send({ type: 'cc-vocab', op: 'list', q: '' })
      VOCAB.loading = false
      if (!r?.ok) VOCAB.err = r?.error || '连不上清影应用'
      else {
        VOCAB.err = ''
        VOCAB.list = r.words || []
        VOCAB.stats = r.stats || {}
      }
      if (S.tab === 'vocab') render()
    }
    const sameVideo = (w) => {
      try {
        return V.videoKey(w.sourceUrl) === localKey()
      } catch {
        return w.sourceUrl === (S.data?.url || location.href)
      }
    }
    const shownVocab = () => {
      let l = VOCAB.list || []
      if (VOCAB.scope === 'video') l = l.filter(sameVideo)
      if (VOCAB.hard) l = l.filter((w) => w.level && w.level !== '基础')
      return l
    }
    const fmtClock = (sec) => {
      const t = Math.max(0, Math.floor(sec || 0))
      return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`
    }
    function vocabCard(w) {
      const here = sameVideo(w)
      return h('div', { class: 'card vcard' },
        h('div', { class: 'vh' },
          h('b', { text: w.word }),
          w.phonetic ? h('span', { class: 'mute', text: w.phonetic }) : null,
          w.level ? h('span', { class: 'chip', text: w.level }) : null),
        h('div', { text: `${w.pos ? w.pos + ' ' : ''}${w.meaning || ''}${w.ctxMeaning ? `（本句：${w.ctxMeaning}）` : ''}` }),
        w.sentence ? h('div', { class: 'mute', text: w.sentence }) : null,
        h('div', { class: 'vf' },
          here ? h('button', { class: 'ts', text: `跳到 ${fmtClock(w.timeSec)}`, onclick: () => seek((w.timeSec || 0) * 1000) }) : h('span', { class: 'mute', text: `${(w.sourceTitle || '').slice(0, 24)} ${fmtClock(w.timeSec)}` })))
    }
    function tabVocab() {
      // 切到这一页时 onclick 已经主动刷新；这里只处理“第一次进来就在生词页”的情况
      if (!VOCAB.list && !VOCAB.loading && !VOCAB.err) void loadVocab()
      const list = shownVocab()
      const exportAnki = async () => {
        const r = await send({ type: 'cc-vocab', op: 'export', format: 'anki' })
        if (!r?.ok) return toast(ui, r?.error || '导出失败', true)
        if (r.zip) {
          // 录过原声：清影把卡片和原声打成了 zip
          const a = h('a', { href: `data:application/zip;base64,${r.zip}`, download: '生词本-anki.zip' })
          document.body.append(a)
          a.click()
          a.remove()
          return toast(ui, `已导出 Anki 包：卡片 + ${r.clips || 0} 段原声`)
        }
        saveFile(`生词本.${r.ext}`, r.text)
      }
      return h('div', {},
        h('div', { class: 'stick' }, h('div', { class: 'tools' },
          sel([['video', '本视频'], ['all', '全部视频']], VOCAB.scope, (v) => ((VOCAB.scope = v), render()), '范围'),
          h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: VOCAB.hard ? true : null, onchange: (e) => ((VOCAB.hard = e.target.checked), render()) }), ' 只看难词'),
          h('span', { class: 'mute', text: VOCAB.loading ? '读取中…' : `${list.length} 个` }))),
        VOCAB.err ? h('div', { class: 'mute', text: `${VOCAB.err}。生词保存在清影应用里，先把应用开着。` }) : null,
        !VOCAB.err && VOCAB.list && !list.length ? h('div', { class: 'mute', text: '还没有收藏的词。看视频时在字幕上悬停生词，点“收藏到生词本”。' }) : null,
        ...list.map(vocabCard),
        actsBar(
          h('button', { text: '导出 Anki', disabled: !list.length, onclick: exportAnki }),
          h('button', { class: 'sec', text: '存为笔记', disabled: !list.length, onclick: () => addNote({ t: -1, text: list.map((w) => `- ${w.word} ${w.phonetic || ''} ${w.meaning || ''}`.trim()).join('\n'), kind: 'note' }) }),
          h('button', { class: 'sec', text: '去应用里复习', title: '复习在清影应用的“生词本”页', onclick: () => toast(ui, '打开清影应用 → 生词本，就能复习今天到期的词') })),
      )
    }

    return { VOCAB, loadVocab, tabVocab }
  }
})()
