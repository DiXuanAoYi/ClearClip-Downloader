// 视频摘要面板：笔记页。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.notes = ({
    S, V, actsBar, copy, exportData, h, isVideo, localKey, quickNote, render, run, saveFile, seek, sel, store,
    toast, ui
  }) => {
    const scopedNotes = () => (S.scope === 'video' ? S.notes.filter((n) => n.key === localKey()) : S.scope === 'series' ? S.notes.filter((n) => V.baseKey(n.key) === V.baseKey(localKey())) : S.notes)
    const shownNotes = () => V.searchNotes(scopedNotes(), S.noteQuery)

    async function updateNote(id, patch) {
      S.notes = S.notes.map((n) => (n.id === id ? { ...n, ...patch } : n))
      await store.set('vsumNotes', S.notes)
    }

    async function polish(n) {
      const c = V.contextAround(S.data.lines, Math.max(n.t, 0), 3)
      const refine = n.kind === 'note'
      try {
        const r = await run(refine ? 'refine' : 'note', { text: refine ? n.text : c.target || n.text, context: c.full }, false)
        S.preview = { id: n.id, quote: r.quote }
      } catch (e) {
        toast(ui, e.message, true, 5200)
      }
      render()
    }

    function noteCard(n) {
      const same = n.key === localKey()
      const stamp = n.t >= 0 ? (same && isVideo() ? h('button', { class: 'ts', text: V.fmtClock(n.t), onclick: () => seek(n.t) }) : h('a', { class: 'ts', href: V.timeLink(n.url, n.t), target: '_blank', rel: 'noopener', text: V.fmtClock(n.t) })) : null
      const pv = S.preview?.id === n.id ? S.preview : null
      const editing = S.editing === n.id
      const ta = editing ? h('textarea', { rows: 3, id: `ed-${n.id}`, text: n.text }) : null
      if (ta) ta.value = n.text
      return h('div', { class: 'note' },
        S.scope !== 'video' && h('div', { class: 'mute', text: [n.title, V.partOf(n.key) > 1 ? `P${V.partOf(n.key)}` : '', n.owner].filter(Boolean).join(' · ') }),
        h('div', {}, stamp, ' ', n.kind === 'note' ? h('span', { text: ta ? '' : n.text, style: 'white-space:pre-wrap' }) : h('span', { class: 'q', text: ta ? '' : n.text })),
        ta,
        pv && h('div', { class: 'card', style: 'margin-top:6px' }, h('div', { class: 'mute', text: 'AI 整理后：' }), pv.quote,
          h('div', { class: 'x' },
            h('button', { text: '替换', onclick: async () => (await updateNote(n.id, { text: pv.quote }), (S.preview = null), render()) }),
            h('button', { class: 'sec', text: '追加在后面', onclick: async () => (await updateNote(n.id, { text: `${n.text}\n${pv.quote}`, kind: 'note' }), (S.preview = null), render()) }),
            h('button', { class: 'sec', text: '不要', onclick: () => ((S.preview = null), render()) }))),
        h('div', { class: 'x' },
          ...(editing
            ? [h('button', { text: '保存', onclick: async () => (await updateNote(n.id, { text: ui.root.getElementById(`ed-${n.id}`).value.trim() || n.text, kind: 'note' }), (S.editing = null), render()) }), h('button', { class: 'sec', text: '取消', onclick: () => ((S.editing = null), render()) })]
            : [h('button', { class: 'sec', text: '编辑', onclick: () => ((S.editing = n.id), render()) }),
              same && S.data.lines.length && h('button', { class: 'sec', text: 'AI 整理', disabled: S.busy, title: '整理成完整通顺的句子，不加原话没有的内容；先预览再决定', onclick: () => polish(n) })]),
          h('button', { class: 'sec', text: '复制', onclick: () => copy(n.text, '笔记') }),
          n.t >= 0 && n.url && h('button', { class: 'sec', text: '复制带时间的链接', onclick: () => copy(V.timeLink(n.url, n.t), '链接') }),
          h('button', { class: 'sec', text: '删除', onclick: async () => ((S.notes = S.notes.filter((x) => x.id !== n.id)), await store.set('vsumNotes', S.notes), render()) })))
    }

    function tabNotes() {
      const list = V.sortNotes(shownNotes())
      const mine = S.notes.filter((n) => n.key === localKey())
      return h('div', {},
        h('div', { class: 'stick' }, h('div', { class: 'tools' },
          sel([['video', '本视频'], ['series', '本视频的所有分P'], ['all', '全部视频']], S.scope, (v) => ((S.scope = v), render()), '范围'),
          h('input', { type: 'text', placeholder: '搜索笔记（正文、标题、作者）', value: S.noteQuery, style: 'flex:1', oninput: (e) => ((S.noteQuery = e.target.value), (S.keepFocus = true), render()) }),
          h('span', { class: 'mute', text: `${list.length} 条` }))),
        !list.length && h('div', { class: 'mute', text: `还没有笔记。播放时按 n 键记下当前这句；在文稿里选中文字也能存。` }),
        ...list.map(noteCard),
        actsBar(
          isVideo() && h('button', { text: '记下当前这句', title: '快捷键 n', onclick: () => quickNote() || toast(ui, '页面上没有找到视频', true) }),
          h('button', { class: 'sec', text: '导出笔记 (.md)', disabled: !list.length, onclick: () => saveFile(V.fileName(S.scope !== 'all' ? S.data.title + ' 笔记' : '视频笔记', 'md'), V.notesToMarkdown(list, { title: S.scope === 'video' ? S.data.title : '视频笔记' }), 'text/markdown') }),
          h('button', { class: 'sec', text: '导出学习稿', disabled: !S.data.lines.length && !mine.length, title: '摘要 + 章节 + 金句 + 我的笔记，带 YAML 头，适合放进 Obsidian', onclick: () => saveFile(V.fileName(S.data.title + ' 学习稿', 'md'), V.studyDoc(exportData(false), mine), 'text/markdown') })),
      )
    }

    return { tabNotes }
  }
})()
