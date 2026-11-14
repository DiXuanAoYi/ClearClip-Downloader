// 视频摘要面板的拆分模块（由 content/vsum.js 调用）：面板状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 本地导入的字幕：记住、恢复、重试失败的片段。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsum ||= {})
  P.local = (C) => {
    const { S, Src, V, h, send, store, toast, ui } = C
    const cached = (...a) => C.cached(...a)
    const isVideo = (...a) => C.isVideo(...a)
    const putCache = (...a) => C.putCache(...a)
    const render = (...a) => C.render(...a)
    const setData = (...a) => C.setData(...a)
    const spokenHint = (...a) => C.spokenHint(...a)
    const submit = (...a) => C.submit(...a)
    const task = (...a) => C.task(...a)
    const transcribeNow = (...a) => C.transcribeNow(...a)

    // ---------- 记住本地导入的字幕：在线字幕读不到时再用，在线的永远优先 ----------

    const localKey = () => V.videoKey(S.data?.url || location.href)
    async function savedLocal() {
      return (await store.get('vsumLocal', {}))[localKey()]?.data || null
    }
    async function rememberLocal(name, d) {
      if (!isVideo() && d.site !== 'manual') return
      if (!/^https?:/.test(S.data?.url || location.href) || !d.lines.length) return
      const all = await store.get('vsumLocal', {})
      all[localKey()] = { ts: Date.now(), data: { name, lines: d.lines.map((l) => ({ t: l.t, text: l.text })) } }
      await store.set('vsumLocal', V.pruneCache(all, 40, 3_000_000))
      S.hasLocal = true
      toast(ui, '已记住这份字幕：下次这个视频读不到在线字幕时会自动使用', false, 4200)
      render()
    }
    async function forgetLocal() {
      const all = await store.get('vsumLocal', {})
      delete all[localKey()]
      await store.set('vsumLocal', all)
      S.hasLocal = false
      toast(ui, '已忘记这个视频的本地字幕')
      render()
    }

    /** 在线字幕读不到时的补救：已记住的本地字幕 → 让清影用 yt-dlp 取在线字幕 → （可选）自动转写。 */
    async function recover(d) {
      S.hasLocal = !!(await savedLocal())
      const saved = await savedLocal()
      if (S.data !== d) return
      if (saved?.lines?.length) {
        setData({ ...d, lines: saved.lines, track: `${saved.name}（本地字幕）`, noSubtitle: false, note: '', fromLocal: true })
        S.hasLocal = true
        render()
        return
      }
      if (!d.url || !/^https?:/.test(d.url) || d.site === 'article' || !(d.noSubtitle || d.fetchFailed)) return
      // 以前转写 / 取到过的文稿直接用，不再请求清影
      for (const [key, name] of [[`tr2:${d.url}`, '清影转写'], [`sub2:${d.url}`, null]]) {
        const old = await cached(key)
        if (S.data !== d) return
        if (old?.lines?.length) {
          setData({ ...d, lines: old.lines, durationMs: old.durationMs || d.durationMs, track: name ? old.track || name : old.track, noSubtitle: false, note: '', trackUrl: '' })
          return
        }
      }
      const st = await send({ type: 'cc-ai-status' })
      if (S.data !== d) return
      if (st.ok) S.status = st
      if (S.prefs.autoOnline && st.ok && st.subtitles !== false && !S.busy) {
        S.busy = true
        render()
        try {
          const skey = `sub2:${d.url}`
          let r = await cached(skey)
          if (!r) {
            r = await submit({ kind: 'subtitles', url: d.url, title: d.title, lang: spokenHint(d) }, 'subtitles', skey)
            if (r?.lines?.length) putCache(skey, r)
          }
          if (S.data !== d) return
          if (r?.lines?.length) {
            S.busy = false
            setData({ ...d, lines: r.lines, durationMs: r.durationMs || d.durationMs, track: r.track, spoken: r.lang || d.spoken || '', trackLang: r.lang || '', trackAuto: !!r.auto, noSubtitle: false, note: '', trackUrl: '' })
            return
          }
          d.note = `${d.note || '没有找到字幕'}；清影也没有从网站取到在线字幕`
        } catch (e) {
          if (e.message !== '已取消') d.note = `${d.note || '没有找到字幕'}；清影取在线字幕失败：${e.message}`
        } finally {
          S.busy = false
          render()
        }
      }
      if (S.data === d && S.prefs.autoTranscribe && st.ok && st.transcribe) await transcribeNow()
    }

    function importFile() {
      const inp = h('input', { type: 'file', accept: '.srt,.vtt,.txt,.md,.json', style: 'display:none' })
      inp.onchange = async () => {
        const f = inp.files?.[0]
        if (!f) return
        const d = { ...Src.fromText(await f.text(), f.name), url: S.data?.url || location.href, title: S.data?.title || f.name }
        setData(d)
        rememberLocal(f.name, d)
      }
      ui.add(inp)
      inp.click()
    }

    /** 只把失败的那几段重新让模型总结，再并进已有的摘要（不重做成功的部分）。 */
    async function retryFailed() {
      const r = S.res.summary
      const ranges = r?.failedRanges || []
      if (!ranges.length) return
      const inAny = (t) => ranges.some(([a, b]) => t >= a && t < b)
      const lines = S.data.lines.filter((l) => inAny(l.t)).map((l) => ({ t: l.t, text: l.text }))
      if (!lines.length) return
      await task('summary', { lines }, (x) => {
        const chapters = [...(r.chapters || []), ...(x.chapters || [])].sort((a, b) => (a.startMs ?? a.start_ms ?? 0) - (b.startMs ?? b.start_ms ?? 0))
        S.res.summary = {
          ...r,
          summary: [r.summary, x.summary].filter(Boolean).join('\n'),
          points: [...(r.points || []), ...(x.points || [])],
          keywords: [...new Set([...(r.keywords || []), ...(x.keywords || [])])].slice(0, 10),
          chapters,
          failed: x.failed || [],
          failedRanges: x.failedRanges || [],
        }
      }, false)
    }


    Object.assign(C, { importFile, retryFailed, localKey, recover, forgetLocal, rememberLocal })
  }
})()
