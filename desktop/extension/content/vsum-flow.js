// 视频摘要面板的拆分模块（由 content/vsum.js 调用）：面板状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 文稿来源（字幕轨 / 音频转写）、翻译整篇文稿、提交转写等流程。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsum ||= {})
  P.flow = (C) => {
    const { S, V, VS, h, send, toast, ui } = C
    const cached = (...a) => C.cached(...a)
    const ensureReady = (...a) => C.ensureReady(...a)
    const importFile = (...a) => C.importFile(...a)
    const isVideo = (...a) => C.isVideo(...a)
    const outLang = (...a) => C.outLang(...a)
    const persist = (...a) => C.persist(...a)
    const putCache = (...a) => C.putCache(...a)
    const rememberEditDoc = (...a) => C.rememberEditDoc(...a)
    const render = (...a) => C.render(...a)
    const run = (...a) => C.run(...a)
    const sel = (...a) => C.sel(...a)
    const setData = (...a) => C.setData(...a)
    const submit = (...a) => C.submit(...a)

    // ---------- 文稿来源：用清影解析出的字幕轨 / 音频转写，单选，后台执行 ----------

    async function loadSources() {
      const url = S.data?.url
      if (S.srcLoading || S.src?.url === url) return
      S.srcLoading = true
      try {
        const st = await send({ type: 'cc-ai-status' })
        if (!st.ok) throw new Error(st.error)
        S.status = st
        const r = await submit({ kind: 'tracks', url, title: S.data.title }, 'tracks')
        const seen = new Set()
        const tracks = (r?.tracks || []).filter((t) => t.code && !seen.has(t.code) && seen.add(t.code))
        S.src = { url, tracks, note: r?.note || '' }
      } catch (e) {
        S.src = { url, tracks: [], note: e.message === '已取消' ? '' : e.message }
      } finally {
        S.srcLoading = false
        if (!S.closed) render()
      }
    }

    /** 单选一个来源：字幕轨（语言代码）或 audio（取音频转写）。同一个视频同一个来源只做一次。 */
    async function pickSource(v) {
      if (!v) return
      if (S.busy) return toast(ui, '上一个任务还在进行', true)
      const audio = v === 'audio'
      if (audio && S.status?.transcribe === false) return toast(ui, '清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp 或填写接口', true, 6000)
      const url = S.data.url
      const key = audio ? `stt:${url}` : `sub2:${url}:${v}`
      S.busy = true
      render()
      try {
        let r = await cached(key)
        if (!r) {
          r = await submit(audio ? { kind: 'transcribe', url, title: S.data.title, track: 'audio' } : { kind: 'subtitles', url, title: S.data.title, track: v }, audio ? 'transcribe' : 'subtitles', key)
          if (r?.lines?.length) putCache(key, r)
        }
        if (!r?.lines?.length) throw new Error(audio ? '没有识别出任何语音' : '这条字幕是空的')
        setData({ ...S.data, lines: r.lines, durationMs: r.durationMs || S.data.durationMs || 0, track: r.track, noSubtitle: false, fetchFailed: false, note: '', trackUrl: '' })
        toast(ui, `${audio ? '转写' : '字幕'}完成，共 ${r.lines.length} 句`)
      } catch (e) {
        if (!S.closed) toast(ui, e.message, true, 7000)
      } finally {
        S.busy = false
        if (!S.closed) render()
      }
    }

    /** auto：没有文稿时自动去查；已有文稿时只在点按钮后才查（不为了好看多打一次网站的请求）。 */
    function sourcePicker(auto = true) {
      if (!isVideo() || !/^https?:/.test(S.data?.url || '')) return null
      if (S.src?.url !== S.data.url) {
        if (!auto && !S.srcLoading) return h('div', { class: 'tools' }, h('button', { class: 'sec', text: '换一个文稿来源（字幕轨 / 音频转写）', onclick: () => ((S.wantSrc = true), loadSources()) }))
        if (!S.recovering) loadSources()
        return h('div', { class: 'mute', text: '正在查看这个视频有哪些字幕…' })
      }
      const opts = [['', '选择文稿来源…'], ...S.src.tracks.map((t) => [t.code, `字幕 · ${t.name}`]), ['audio', '音频转写（取音频，用 Whisper 转成文字）']]
      return h('div', {},
        h('div', { class: 'tools' }, h('span', { class: 'mute', text: '文稿来源' }), sel(opts, '', pickSource, '单选：有字幕就直接取字幕，没有就取音频转写；在后台进行，结果会缓存')),
        !S.src.tracks.length && h('div', { class: 'mute', text: S.src.note ? `没有取到字幕列表：${S.src.note}` : '这个视频没有可用的在线字幕，可以选“音频转写”。' }),
      )
    }

    /** 没有字幕：让清影取音频、用语音识别转成带时间的文稿，之后和有字幕的视频一样处理。 */
    /** 告诉清影视频说的是什么语言（转写 / 取字幕都按它选）：平台给的语音识别字幕语言 > 标题里的汉字 / 假名 / 韩文；看不出来就留空，由清影按设置或自动识别。 */
    function spokenHint(d) {
      if (d?.spoken) return d.spoken
      const s = V.scriptOf(d?.title || document.title)
      return s === 'zh' || s === 'ja' || s === 'ko' ? s : ''
    }

    /** 转写进行中：把已经识别出来的先显示在文稿里（不重置摘要等结果，也不存缓存）。 */
    function showPartial(p) {
      if (S.closed || !S.data || !p?.lines?.length) return
      // 识别过程中用户改过的句子（按时间对应）一直保留
      const lines = S.liveEdits?.size ? p.lines.map((l) => (S.liveEdits.has(l.t) ? { ...l, text: S.liveEdits.get(l.t), src: l.text } : l)) : p.lines
      S.data = { ...S.data, lines, durationMs: p.durationMs || S.data.durationMs || 0, track: `清影转写（识别中 ${Math.round((p.progress || 0) * 100)}%）`, streaming: true, noSubtitle: false, note: '', trackUrl: '' }
      // 正在编辑一句时不重画（会把输入框弄没），编辑完补画
      if (S.editing) S.pendingRender = true
      else render()
    }

    async function transcribeNow() {
      if (S.busy) return toast(ui, '上一个任务还在进行', true)
      if (S.status?.transcribe === false) return toast(ui, '清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp 或填写接口', true, 6000)
      const url = S.data?.url || location.href
      const title = S.data?.title || document.title
      S.busy = true
      render()
      try {
        const key = `tr2:${url}`
        let r = await cached(key)
        if (!r) {
          // 识别出多少先显示多少；全部识别完后用完整结果覆盖，保证每句都有
          r = await submit({ kind: 'transcribe', url, title, lang: spokenHint(S.data) }, 'transcribe', key, showPartial)
          if (r?.lines?.length) putCache(key, r)
        }
        if (!r?.lines?.length) throw new Error('没有识别出任何语音')
        // 识别过程中改过的句子：按时间带到最终结果上，并记在本地
        let finalLines = r.lines
        if (S.liveEdits?.size) {
          finalLines = r.lines.map((l) => {
            if (!S.liveEdits.has(l.t)) return l
            const to = S.liveEdits.get(l.t)
            rememberEditDoc?.(l.t, l.text, to)
            return { ...l, text: to, src: l.text }
          })
          S.liveEdits = null
        }
        setData({ ...S.data, lines: finalLines, durationMs: r.durationMs || S.data?.durationMs || 0, track: r.track, spoken: r.lang || S.data?.spoken || '', trackLang: r.lang || '', trackAuto: true, streaming: false, noSubtitle: false, note: '', trackUrl: '', tracks: S.data?.tracks || [] })
        toast(ui, `转写完成，共 ${finalLines.length} 句`)
      } catch (e) {
        toast(ui, e.message, true, 7000)
      } finally {
        S.busy = false
        render()
      }
    }

    async function task(kind, extra, apply, cache = true) {
      if (S.busy) return toast(ui, '上一个任务还在进行', true)
      if (!S.data?.lines.length) return toast(ui, '还没有文稿', true)
      S.busy = true
      render()
      try {
        apply(await run(kind, extra, cache))
        S.restored = 0
        persist()
      } catch (e) {
        if (!S.closed) toast(ui, e.message, true, 5200)
      } finally {
        S.busy = false
        if (!S.closed) render()
      }
    }

    /**
     * 翻译文稿：和字幕层共用视频会话里的同一份译文（同一种任务、同一份缓存）。字幕层已经翻好就直接拿来；
     * 自己写了翻译提示词的，还是单独提交（结果也放进会话，字幕层跟着用）。
     */
    async function translateDoc(d) {
      if (S.prefs.prompts?.translate?.trim() || !isVideo()) return task('translate', {}, (x) => (S.res.translated = d.lines.map((_, i) => x.lines?.[i]?.text || '')))
      if (S.busy) return toast(ui, '上一个任务还在进行', true)
      if (!d?.lines.length) return toast(ui, '还没有文稿', true)
      S.busy = true
      let cancelled = false
      const job = { id: 0, kind: 'translate', tag: '', done: 0, total: d.lines.length, note: '正在翻译文稿', cancel: () => (cancelled = true) }
      S.job ||= job
      render()
      try {
        await ensureReady()
        VS.setLines(d.lines, d.track || '', 'vsum')
        const arr = await VS.translate(outLang(), {
          stop: () => cancelled || S.closed || S.data !== d,
          onProgress: (done, total) => {
            job.done = done
            job.total = total
            if (S.job === job && !S.closed) render()
          },
        })
        if (S.data !== d) return
        S.res.translated = arr
        S.restored = 0
        persist()
      } catch (e) {
        if (!S.closed && e.message !== '已取消') toast(ui, e.message, true, 5200)
      } finally {
        if (S.job === job) S.job = null
        S.busy = false
        if (!S.closed) render()
      }
    }

    // ---------- 各个标签页 ----------

    const noData = () => {
      const d = S.data
      return h(
        'div',
        { class: 'warn' },
        d?.note || '还没有文稿',
        d?.site === 'bilibili' && h('div', { class: 'mute', text: 'B 站的字幕（含 AI 字幕）需要登录后才能读到；登录后点上面的“重新读取”。' }),
        sourcePicker(),
        h('div', { class: 'tools', style: 'margin-top:6px' },
          (d?.noSubtitle || d?.fetchFailed) && h('button', { text: '用清影转写（语音识别）', title: S.status?.transcribe === false ? '清影里还没有配置语音转文字' : '取视频的音频，在清影里转成文字后再总结', onclick: transcribeNow }),
          h('button', { class: 'sec', text: '导入字幕文件', onclick: importFile }),
          h('button', { class: 'sec', text: '粘贴文字', onclick: () => (S.tab = 'more') && render() }),
        ),
      )
    }

    async function sendForTranscribe() {
      const r = await send({ type: 'cc-send-page' })
      toast(ui, r.ok ? (r.pairing ? '请在电脑上允许配对' : '已发送。下载完成后在清影的媒体库里点“转写”，再回来用文字稿') : r.error, !r.ok, 5200)
    }


    Object.assign(C, { spokenHint, transcribeNow, task, noData, translateDoc, sendForTranscribe, sourcePicker })
  }
})()
