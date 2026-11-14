// 视频摘要面板的拆分模块（由 content/vsum.js 调用）：面板状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 摘要 / 章节 / 提问标签页，划词、笔记、快捷键、同步到清影。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsum ||= {})
  P.tabs = (C) => {
    const { S, Src, TL, V, h, send, store, toast, ui } = C
    const actsBar = (...a) => C.actsBar(...a)
    const ago = (...a) => C.ago(...a)
    const copy = (...a) => C.copy(...a)
    const exportData = (...a) => C.exportData(...a)
    const getSel = (...a) => C.getSel(...a)
    const isVideo = (...a) => C.isVideo(...a)
    const langSelect = (...a) => C.langSelect(...a)
    const linkify = (...a) => C.linkify(...a)
    const localKey = (...a) => C.localKey(...a)
    const outLabel = (...a) => C.outLabel(...a)
    const persist = (...a) => C.persist(...a)
    const render = (...a) => C.render(...a)
    const retryFailed = (...a) => C.retryFailed(...a)
    const run = (...a) => C.run(...a)
    const saveFile = (...a) => C.saveFile(...a)
    const savePrefs = (...a) => C.savePrefs(...a)
    const saveSession = (...a) => C.saveSession(...a)
    const seek = (...a) => C.seek(...a)
    const sel = (...a) => C.sel(...a)
    const setData = (...a) => C.setData(...a)
    const task = (...a) => C.task(...a)
    const tsBtn = (...a) => C.tsBtn(...a)
    const video = (...a) => C.video(...a)

    function tabSummary() {
      const r = S.res.summary
      const box = h('div', { class: 'sm' })
      const d = S.data
      const sty = V.STYLES.find(([v]) => v === S.prefs.style)?.[1] || ''
      box.append(
        h('div', { class: 'vid' },
          h('b', { text: d?.title || '' }),
          h('div', { class: 'vmeta' },
            d?.durationMs > 0 && h('span', { text: V.fmtClock(d.durationMs) }),
            h('span', { text: `${sty} · ${outLabel() || '跟随视频'}` }),
            r && S.restored ? h('span', { class: 'saved', text: `✓ ${ago(S.restored)}生成，已保存` }) : null)),
        h('div', { class: 'ctrl stuck' },
          h('div', { class: 'seg', role: 'group', 'aria-label': '摘要风格' },
            ...V.STYLES.map(([v, n]) => h('button', { class: v === S.prefs.style ? 'on' : '', 'aria-pressed': v === S.prefs.style, text: n, disabled: S.busy, onclick: () => ((S.prefs.style = v), savePrefs(), render()) }))),
          h('div', { class: 'crow' },
            h('span', { class: 'mute', text: '输出语言' }),
            langSelect(`输出语言：摘要、导图、卡片、问答都用这个语言写。“跟随视频”＝用${S.status?.lang || '中文'}，中文视频译成英文`, () => render()),
            h('span', { class: 'grow' }),
            h('button', { class: r ? 'sec' : '', text: r ? '重新生成' : '生成摘要', disabled: S.busy, onclick: () => task('summary', {}, (x) => (S.res.summary = x)) }))),
      )
      if (!r) {
        box.append(h('div', { class: 'empty' }, h('b', { text: '还没有摘要' }), h('p', { class: 'mute', text: '读取字幕后，由清影用你设置的大模型生成：总结、要点、关键词和章节。长视频会自动分段处理。' })))
        return box
      }
      if (r.failed?.length) {
        box.append(h('div', { class: 'warn' }, h('span', { text: `有 ${r.failed.length} 段没生成成功（从 ${r.failed.filter(Boolean).join('、') || '部分位置'} 起），摘要不完整。` }),
          r.failedRanges?.length ? h('button', { text: '只补这几段', disabled: S.busy, onclick: () => retryFailed() }) : null))
      }
      const { lead, paras } = V.splitSummary(r.summary)
      if (lead) box.append(h('div', { class: 'lead' }, h('div', { class: 'k', text: '一句话看完' }), h('p', { text: lead })))
      if (paras.length) box.append(h('div', {}, h('div', { class: 'sh', text: '总结' }), h('div', { class: 'prose' }, ...paras.map((p) => h('p', { text: p })))))
      if (r.keywords?.length) box.append(h('div', { class: 'kw' }, ...r.keywords.map((k) => h('span', { text: k }))))
      if (r.points?.length) {
        box.append(h('div', {},
          h('div', { class: 'sh' }, '要点', h('em', { text: ` ${r.points.length} 条${isVideo() ? '，点时间跳转' : ''}` }),
            r.chapters?.length ? h('button', { class: 'lnk', text: '看章节 ›', onclick: () => ((S.tab = 'chapters'), render()) }) : null),
          h('ul', { class: 'pts' }, ...r.points.map((p) => {
            const x = V.parsePoint(p)
            const ms = x.time ? V.parseClock(x.time) : -1
            return h('li', { class: ms >= 0 && isVideo() ? 'go' : '', onclick: () => ms >= 0 && seek(ms) },
              h('span', { class: 'tm', text: x.time || '·' }),
              h('div', {}, x.title && h('b', { text: x.title }), x.detail && h('span', { class: x.title ? 'd' : '', text: x.detail })))
          }))))
      }
      box.append(h('div', { class: 'acts' },
        h('button', { text: '复制 Markdown', onclick: () => copy(V.toMarkdown(exportData()), ' Markdown') }),
        h('button', { class: 'sec', text: '下载 .md', onclick: () => saveFile(V.fileName(d.title + ' 摘要', 'md'), V.toMarkdown(exportData()), 'text/markdown') })))
      return box
    }

    function chapterList() {
      const ai = S.res.summary?.chapters || []
      return ai.length ? { list: ai, from: 'AI 章节' } : S.data.chapters?.length ? { list: S.data.chapters, from: '视频简介里的章节' } : { list: [], from: '' }
    }

    function tabChapters() {
      const { list, from } = chapterList()
      if (!list.length) return h('div', { class: 'mute', text: '这个视频的简介里没有章节。先在“摘要”里生成，会得到 AI 章节。' })
      const now = video() && isVideo() ? V.chapterAt(list, video().currentTime * 1000) : -1
      const box = h('div', {},
        h('div', { class: 'mute', style: 'margin-bottom:6px', text: `来源：${from}，共 ${list.length} 章` }),
        h('div', { class: 'chs' }, ...list.map((c, i) => {
          const end = list[i + 1]?.startMs ?? Infinity
          const qs = (S.res.quotes || []).filter((q) => q.t >= (c.startMs ?? 0) && q.t < end)
          return h('div', { class: `ln${i === now ? ' cur' : ''}`, 'data-ch': i, onclick: () => seek(c.startMs) },
            h('span', { class: 'tm', text: V.fmtClock(c.startMs) }),
            h('div', {}, h('b', { text: c.title }), ...qs.map((q) => h('div', { class: 'quote' }, `“${q.text}” `, tsBtn(q.t)))))
        })),
        !S.res.quotes && h('div', { class: 'mute', style: 'margin-top:6px', text: '在“更多”里提取金句后，会按章节显示在这里。' }),
        actsBar(
          h('button', { text: '复制章节列表', onclick: () => copy(list.map((c) => `${V.fmtClock(c.startMs)} ${c.title}`).join('\n'), '章节') }),
          h('button', { class: 'sec', text: '去“摘要”重新生成', onclick: () => ((S.tab = 'summary'), render()) })),
      )
      return box
    }

    // ---------- 划词：解释 / 存为笔记 ----------

    function onSelect(e) {
      const text = getSel()
      ui.root.getElementById('selbar')?.remove()
      if (!text || text.length > 800) return
      const row = e.target.closest?.('.ln')
      const i = row ? Number(row.dataset.i) : -1
      const t = i >= 0 ? S.data.lines[i].t : -1
      const bar = h('div', { class: 'selbar', id: 'selbar' },
        h('div', { class: 'mute', text: `“${text.length > 80 ? text.slice(0, 80) + '…' : text}”` }),
        h('div', { class: 'tools', style: 'margin:4px 0 0' },
          h('button', { text: '解释', onclick: () => explain(text, t, bar) }),
          h('button', { class: 'sec', text: '存为笔记', onclick: () => (addNote({ t, text }), bar.remove()) }),
          h('button', { class: 'sec', text: '✕', 'aria-label': '关闭', onclick: () => bar.remove() })),
        h('div', { id: 'selans' }))
      ui.root.querySelector('.body')?.prepend(bar)
    }

    async function explain(text, t, bar) {
      const out = bar.querySelector('#selans')
      out.replaceChildren(h('div', { class: 'mute', text: '解释中…' }))
      try {
        const c = V.contextAround(S.data.lines, t >= 0 ? t : 0, 3)
        const r = await run('explain', { text, context: c.full }, false)
        out.replaceChildren(h('div', { class: 'card', text: r.text }),
          h('button', { class: 'sec', text: '把解释记进笔记', onclick: () => addNote({ t, text: `${text}\n→ ${r.text}`, kind: 'note' }) }))
      } catch (e) {
        out.replaceChildren(h('div', { class: 'warn', text: e.message }))
      }
    }

    // ---------- 笔记 ----------

    async function addNote({ t = -1, text, kind = 'quote' }) {
      const n = V.makeNote({ key: localKey(), url: S.data.url, title: S.data.title, owner: S.data.owner, t, text, kind })
      if (!n) return false
      const before = S.notes.length
      S.notes = V.addNote(S.notes, n)
      await store.set('vsumNotes', S.notes)
      toast(ui, S.notes.length > before ? '已记下' : '这条已经记过了')
      if (S.tab === 'notes') render()
      return true
    }

    /** n 键 / 悬浮“笔记”：记下当前播放位置（往前提一点）那一句。 */
    function quickNote() {
      const v = video()
      if (!v || !S.data) return false
      const ms = Math.max(0, v.currentTime * 1000 - (Number(S.prefs.lead) || 0) * 1000)
      const i = V.lineAt(S.data.lines, ms)
      addNote({ t: ms, text: i >= 0 ? S.data.lines[i].text : '（此处没有字幕）' })
      return true
    }

    function onKey(e) {
      if (e.key === '/' && S.tab === 'transcript' && S.data && !e.ctrlKey && !e.metaKey && !e.altKey && !e.isComposing) {
        const t = e.composedPath?.()[0] || e.target
        if (t && (/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || t.isContentEditable)) return
        const box = ui.root.getElementById?.('tsearch') || ui.root.querySelector('#tsearch')
        if (box) (e.preventDefault(), e.stopPropagation(), box.focus())
        return
      }
      if (e.key !== 'n' || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.isComposing) return
      const el = e.composedPath?.()[0] || e.target
      if (el && (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName) || el.isContentEditable)) return
      if (isVideo() && quickNote()) e.preventDefault()
    }

    function tabAsk() {
      const box = h('div')
      if (!S.chat.length) {
        box.append(h('div', { class: 'mute', text: '针对这个视频提问。回答里的 [m:ss] 可以点，直接跳到那一刻。' }),
          h('div', { class: 'sugg' }, ...V.SUGGESTIONS.map((q) => h('button', { class: 'sec', text: q, onclick: () => ask(q) }))))
      }
      for (const m of S.chat) {
        const cite = m.role === 'assistant' ? V.citations(m.text, S.data.lines) : null
        box.append(h('div', { class: `msg${m.role === 'user' ? ' u' : ''}` }, h('div', { class: 'b' }, ...linkify(m.text),
          ...(cite?.refs || []).map((r) => h('div', { class: 'src', title: '回答引用的原话，点一下跳到那里', onclick: () => seek(r.ms) }, `[${r.label}] ${r.text}`)),
          cite?.unsupported && h('div', { class: 'warn', text: '这条回答没有给出可核对的时间点，请当作参考，别全信。' }),
          cite?.bad && h('div', { class: 'warn', text: `有 ${cite.bad} 处时间点超出了文稿范围，已忽略。` }))))
      }
      if (S.chat.length) box.append(h('div', { class: 'tools' }, h('button', { class: 'sec', text: '清空对话', onclick: () => ((S.chat = []), persist(), render()) }), h('button', { class: 'sec', text: '复制对话', onclick: () => copy(S.chat.map((m) => `${m.role === 'user' ? '问' : '答'}：${m.text}`).join('\n\n'), '对话') })))
      return box
    }

    async function ask(q) {
      q = String(q || '').trim()
      if (!q) return
      const history = S.chat.slice(-6).map((m) => ({ role: m.role, text: m.text }))
      S.chat.push({ role: 'user', text: q })
      await task('ask', { question: q, history }, (x) => S.chat.push({ role: 'assistant', text: x.answer }), false)
      if (S.chat.at(-1)?.role === 'user') S.chat.pop()
      render()
    }

    /** 手动把这个视频的摘要（和已翻译的文稿）同步到清影。 */
    async function syncToApp() {
      const d = S.data
      if (!d?.lines?.length) return toast(ui, '还没有文稿', true)
      await saveSession()
      const url = d.url || location.href
      const vk = V.videoKey(url)
      const sess = Object.values(await store.get('vsumSessions', {})).find((e) => e && e.vk === vk)
      const tr = S.res.translated
      const payload = {
        url,
        title: d.title,
        site: d.site || '',
        summary: sess ? { key: sess.key, ts: sess.ts, vk: sess.vk, url: sess.url, data: sess.data } : null,
        subs: tr?.some(Boolean) ? { key: '', target: S.prefs.lang, lines: d.lines.map((l) => ({ t: l.t, text: l.text })), trans: tr } : null,
        dstLang: S.prefs.lang || '',
      }
      if (!payload.summary && !payload.subs) return toast(ui, '还没有可同步的摘要或翻译，先生成一份', true)
      const r = await send({ type: 'cc-videocache', op: 'put', payload })
      toast(ui, r.ok ? '已同步到清影，生词本里能看到这个视频的缓存' : r.error || '同步失败', !r.ok)
    }

    /** 文稿顶部：当前原文是哪种语言、哪条字幕轨，轨道和视频语音对不上时给提示。 */
    function trackBar(d) {
      if (!d.lines?.length || d.site === 'html5' && !d.track) return null
      const code = d.trackLang || ''
      const name = code ? TL.label(TL.LANGS.find((l) => code.toLowerCase().startsWith(l.code.toLowerCase()))?.code || code, S.appLang).split(' · ')[0] : d.track || ''
      const sample = d.lines.slice(0, 30).map((l) => l.text).join(' ')
      const textScript = V.scriptOf(sample)
      const titleScript = V.scriptOf(d.title)
      const spoken = d.spoken || ''
      const lang = (c) => String(c || '').toLowerCase().split(/[-_]/)[0]
      const mismatch = (spoken && code && lang(spoken) !== lang(code)) || (!spoken && titleScript === 'latin' && textScript === 'zh')
      const kind = d.trackAuto ? '自动字幕' : code ? '人工字幕' : ''
      const en = d.tracks?.find((t) => lang(t.lang) === (lang(spoken) || 'en'))
      return h('div', { class: 'mute', style: 'display:flex;flex-direction:column;gap:4px;padding:2px 0 6px' },
        h('div', { style: 'display:flex;gap:8px;align-items:center;flex-wrap:wrap' },
          h('span', { text: '原文' }), name && h('b', { text: name, style: 'color:inherit' }), kind && h('span', { class: 'chip', text: kind }),
          spoken && !mismatch && h('span', { text: '· 与视频语音一致' }),
          d.tracks?.length > 1 && sel(d.tracks.map((t) => [t.url, `${t.name}${t.auto ? '（自动）' : ''}`]), d.trackUrl || d.tracks[0].url, reloadTrack, '字幕轨')),
        mismatch && h('div', { style: 'color:#b45309' },
          `当前轨道（${name || d.track}）可能不是视频原语言，有可能是别人上传的翻译。`,
          en && en.url !== d.trackUrl && h('button', { class: 'sec', text: `切换到 ${en.name}`, style: 'margin-left:6px', onclick: () => reloadTrack(en.url) })))
    }

    async function reloadTrack(url) {
      toast(ui, '正在读取字幕…')
      setData(await Src.load(url))
    }

    function loadArticleFromSelection() {
      const a = Src.loadArticle()
      if (!a.lines.length) return toast(ui, '页面上没有可总结的文字', true)
      setData(a)
    }


    Object.assign(C, { tabSummary, tabChapters, tabAsk, ask, chapterList, quickNote, onKey, addNote, onSelect, trackBar, loadArticleFromSelection, reloadTrack, syncToApp })
  }
})()
