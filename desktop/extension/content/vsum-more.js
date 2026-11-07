// 视频摘要面板：“更多”页（金句、卡片、改写、提示词、备份与导出）。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.more = ({
    S, Src, V, applyAppearance, copy, forgetLocal, grp, h, importFile, isVideo, loadArticleFromSelection,
    reloadTrack, rememberLocal, render, saveFile, savePrefs, sel, send, sendForTranscribe, setData, sourcePicker,
    store, syncToApp, task, tick, toast, transcribeNow, tsBtn, ui
  }) => {
    function tabMore() {
      const box = h('div')
      const cards = S.res.cards
      const quotes = S.res.quotes
      box.append(
        grp('清影里的缓存',
          h('div', { class: 'mute', text: '把这个视频的摘要和翻译同步到清影应用。收藏生词时也会自动同步（可在清影的生词本里关掉）；在生词本里选“打开视频并恢复缓存”会取回到这里。' }),
          h('div', { class: 'tools' }, h('button', { class: 'sec', text: '同步到清影', onclick: syncToApp }))),
        grp('学习卡片（可导入 Anki）',
        h('div', { class: 'tools' },
          h('button', { text: cards ? '重新生成' : '生成问答卡片', disabled: S.busy, onclick: () => task('cards', {}, (x) => (S.res.cards = x.cards)) }),
          cards && h('button', { class: 'sec', text: '导出 CSV', onclick: () => saveFile(V.fileName(S.data.title, 'tsv'), V.cardsToCsv(cards)) }),
        ),
        ...(cards || []).map((c) => h('div', { class: 'card' }, h('b', { text: c.q }), '\n', c.a, ' ', tsBtn(c.t))),
        ),
        grp('金句',
        h('div', { class: 'tools' }, h('button', { text: quotes ? '重新生成' : '提取金句', disabled: S.busy, onclick: () => task('quotes', {}, (x) => (S.res.quotes = x.quotes)) })),
        ...(quotes || []).map((q) => h('div', { class: 'card' }, `“${q.text}” `, tsBtn(q.t))),
        ),
        grp('改写成其他形式',
        h('div', { class: 'tools' }, ...V.REWRITES.map(([f, n]) => h('button', { class: 'sec', text: n, disabled: S.busy || !S.res.summary, title: S.res.summary ? '' : '先在“摘要”里生成', onclick: () => task('rewrite', { format: f, text: V.toMarkdown(exportData()) }, (x) => (S.res.rewrite = { name: n, text: x.text }), false) }))),
        S.res.rewrite && h('div', {}, h('div', { class: 'card', text: S.res.rewrite.text }), h('button', { class: 'sec', text: `复制${S.res.rewrite.name}`, onclick: () => copy(S.res.rewrite.text, S.res.rewrite.name) })),
        ),
        grp('导出',
        h('div', { class: 'tools' },
          h('button', { class: 'sec', text: '下载 Markdown', onclick: () => saveFile(V.fileName(S.data.title, 'md'), V.toMarkdown(exportData(true)), 'text/markdown') }),
          h('button', { class: 'sec', text: '复制到剪贴板（含文稿）', onclick: () => copy(V.toMarkdown(exportData(true)), ' Markdown') }),
          h('button', { class: 'sec', text: '存进清影的便签', onclick: saveNote }),
          h('button', { class: 'sec', text: '发到 Obsidian', onclick: toObsidian }),
        ),
        ),
        grp('笔记与偏好',
        h('div', { class: 'tools' },
          sel([['auto', '跟随系统'], ['light', '浅色'], ['dark', '深色']], S.prefs.theme, (v) => ((S.prefs.theme = v), savePrefs(), applyAppearance(), render()), '主题'),
          sel([[12, '小'], [13, '标准'], [15, '大'], [17, '特大']], String(S.prefs.fs), (v) => ((S.prefs.fs = Number(v)), savePrefs(), applyAppearance()), '字号')),
        h('label', { class: 'mute' }, '记笔记时往前提 ', h('input', { type: 'number', min: 0, max: 15, value: S.prefs.lead, style: 'width:56px', onchange: (e) => ((S.prefs.lead = Math.min(15, Math.max(0, Number(e.target.value) || 0))), savePrefs()) }), ' 秒'),
        ),
        grp('提示词',
        promptEditor(),
        ),
        grp('数据',
        h('div', { class: 'tools' },
          h('button', { class: 'sec', text: '备份设置和笔记', title: '不包含任何密钥', onclick: backup }),
          h('button', { class: 'sec', text: '恢复备份', onclick: restore }),
          h('button', { class: 'sec', text: '清除摘要缓存（含各视频已生成的内容）', onclick: async () => (await store.set('vsumCache', {}), await store.set('vsumSessions', {}), await store.set('vsumJobs', {}), await store.set('vsumComments', {}), (S.cm = null), (S.restored = 0), toast(ui, '已清除')) }),
          h('button', { class: 'sec', text: '删除全部笔记', disabled: !S.notes.length, onclick: async () => confirm(`确定删除全部 ${S.notes.length} 条笔记？`) && ((S.notes = []), await store.set('vsumNotes', []), render()) })),
        ),
        grp('没有字幕 / 想换文稿',
        sourcePicker(!S.data.lines.length || !!S.wantSrc),
        h('div', { class: 'tools' },
          isVideo() && h('button', { class: 'sec', text: '用清影转写（语音识别）', onclick: transcribeNow }),
          isVideo() && h('button', { class: 'sec', text: '只把视频发给清影下载', onclick: sendForTranscribe }),
          h('button', { class: 'sec', text: '导入 .srt / .vtt / .txt', title: '导入后会按视频记住；在线字幕读得到时仍以在线字幕为准', onclick: importFile }),
          S.hasLocal && h('button', { class: 'sec', text: '忘记已存的本地字幕', onclick: forgetLocal }),
          S.data.tracks?.length > 1 && sel(S.data.tracks.map((t) => [t.url, `${t.name}${t.auto ? '（自动）' : ''}`]), S.data.trackUrl || S.data.tracks[0].url, reloadTrack, '字幕轨'),
          h('button', { class: 'sec', text: '总结选中的文字', onclick: () => loadArticleFromSelection() }),
        ),
        h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.autoOnline, onchange: (e) => ((S.prefs.autoOnline = e.target.checked), savePrefs()) }), ' 读不到字幕时，让清影从网站取在线字幕'),
        h('label', { class: 'mute' }, h('input', { type: 'checkbox', checked: S.prefs.autoTranscribe, onchange: (e) => ((S.prefs.autoTranscribe = e.target.checked), savePrefs()) }), ' 没有字幕时，自动取音频转写（后台进行；耗时，用本机 Whisper 不花钱，用接口会占额度）'),
        h('textarea', { rows: 4, placeholder: '或者把文字稿 / 字幕粘贴到这里，再点“使用”', id: 'paste' }),
        h('div', { class: 'tools' }, h('button', { class: 'sec', text: '使用粘贴的文字', onclick: () => {
          const t = ui.root.getElementById('paste')?.value
          if (!t?.trim()) return toast(ui, '先粘贴文字', true)
          const d = { ...Src.fromText(t), url: S.data.url, title: S.data.title }
          setData(d)
          rememberLocal('粘贴的文字', d)
        } })),
        ),
      )
      return box
    }

    const PROMPT_KINDS = [['summary', '摘要'], ['mindmap', '导图'], ['ask', '问答'], ['explain', '划词解释'], ['punctuate', '顺句'], ['note', '整理笔记'], ['refine', '优化笔记'], ['rewrite', '改写']]

    /** 自己写提示词：留空用内置的；可以用 {lang} 代表输出语言。 */
    function promptEditor() {
      const kind = S.promptKind || 'explain'
      const ta = h('textarea', { rows: 5, id: 'pt', maxlength: 20000, placeholder: '留空 = 用内置提示词。可用 {lang} 代表输出语言。' })
      ta.value = S.prefs.prompts?.[kind] || ''
      return h('div', {},
        h('div', { class: 'tools' },
          sel(PROMPT_KINDS, kind, (v) => ((S.promptKind = v), render()), '要改哪一类'),
          h('button', { class: 'sec', text: '保存', onclick: () => {
            const v = ui.root.getElementById('pt').value.trim()
            S.prefs.prompts = { ...S.prefs.prompts }
            if (v) S.prefs.prompts[kind] = v
            else delete S.prefs.prompts[kind]
            savePrefs()
            toast(ui, v ? '已保存，下次生成时使用' : '已改回内置提示词')
          } })),
        ta)
    }

    async function backup() {
      const { prompts, ...rest } = S.prefs
      saveFile('清影视频摘要备份.json', JSON.stringify({ app: 'clearclip-vsum', v: 1, prefs: { ...rest, prompts }, notes: S.notes }, null, 1), 'application/json')
    }

    function restore() {
      const i = h('input', { type: 'file', accept: '.json,application/json' })
      i.onchange = async () => {
        try {
          const j = JSON.parse(await i.files[0].text())
          if (j.app !== 'clearclip-vsum') throw new Error('不是清影的备份文件')
          if (j.prefs && typeof j.prefs === 'object') S.prefs = { ...S.prefs, ...j.prefs }
          if (Array.isArray(j.notes)) {
            for (const n of j.notes) S.notes = V.addNote(S.notes, V.makeNote(n))
            await store.set('vsumNotes', S.notes)
          }
          savePrefs()
          toast(ui, '已恢复')
          render()
        } catch (e) {
          toast(ui, `恢复失败：${e.message}`, true)
        }
      }
      i.click()
    }

    function exportData(withLines = false) {
      const r = S.res.summary || {}
      return {
        title: S.data.title,
        url: isVideo() ? S.data.url : '',
        result: { ...r, chapters: r.chapters?.length ? r.chapters : S.data.chapters },
        quotes: S.res.quotes,
        cards: S.res.cards,
        outline: S.res.outline,
        lines: S.data.lines,
        includeTranscript: withLines,
      }
    }

    async function saveNote() {
      const r = await send({ type: 'cc-note', text: V.toMarkdown(exportData()) })
      toast(ui, r.ok ? (r.pairing ? '请在电脑上允许配对' : '已存进清影的“收到的文件”') : r.error, !r.ok, 4200)
    }

    /** Obsidian：obsidian://new?name=…&content=…（内容太长时改为复制 + 只建空笔记）。 */
    function toObsidian() {
      const md = V.toMarkdown(exportData())
      const name = V.fileName(S.data.title, '').replace(/\.$/, '')
      const url = `obsidian://new?name=${encodeURIComponent(name)}&content=${encodeURIComponent(md)}`
      if (url.length > 7000) {
        copy(md, ' Markdown')
        location.href = `obsidian://new?name=${encodeURIComponent(name)}&clipboard`
      } else location.href = url
    }

    return { exportData, tabMore }
  }
})()
