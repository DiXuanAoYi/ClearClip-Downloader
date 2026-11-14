// 页面字幕层的拆分模块（由 content/sublayer.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 字幕与译文：读取字幕、翻译全部、转写、导出 SRT。
;(() => {
  const P = ((globalThis.ccParts ||= {}).sub ||= {})
  P.data = (C) => {
    const { L, S, VS, cap, h, send, toast, ui } = C
    const paint = (...a) => C.paint(...a)
    const setStatus = (...a) => C.setStatus(...a)


    // ---------- 字幕与译文 ----------

    async function loadLines() {
      setStatus('正在读取字幕…')
      // 视频会话：视频摘要已经读到的文稿直接用，不再读一遍
      const d = await VS.load({ source: 'sublayer' })
      S.data = d
      S.lines = (d.lines || []).map((l) => ({ t: l.t, text: l.text }))
      S.trans = []
      if (!S.lines.length) {
        setStatus('这个视频没有读到字幕。可以点“用清影转写”。')
        return false
      }
      setStatus(`已读取 ${S.lines.length} 句字幕${d.track ? `（${d.track}）` : ''}`)
      return true
    }

    /** 译文来自视频会话：和摘要面板的“译文”是同一份，谁先翻好另一个直接用。 */
    async function translateAll() {
      if (S.busy || !S.lines.length) return
      S.busy = true
      const lines = S.lines
      try {
        const have = VS.translations(S.target)
        if (have && have.length === lines.length) {
          S.trans = have
          setStatus(`已翻译成${S.target}，共 ${lines.length} 句`)
          return
        }
        S.trans = new Array(lines.length).fill('')
        const arr = await VS.translate(S.target, {
          stop: () => S.closed || S.lines !== lines,
          onProgress: (done, total, part) => {
            if (S.lines !== lines) return
            S.trans = part.slice()
            setStatus(`正在翻译 ${done}/${total}…`)
            cap.dataset.k = ''
            paint()
          },
        })
        if (S.lines !== lines) return
        S.trans = arr
        setStatus(`已翻译成${S.target}，共 ${lines.length} 句`)
      } catch (e) {
        if (e.message === '已取消') return
        setStatus(e.message)
        toast(ui, e.message, true, 6000)
      } finally {
        S.busy = false
        cap.dataset.k = ''
        paint()
      }
    }

    async function transcribe() {
      if (S.busy) return toast(ui, '上一个任务还在进行', true)
      S.busy = true
      try {
        const st = await send({ type: 'cc-ai-status' })
        if (!st.ok) throw new Error(st.error)
        if (st.transcribe === false) throw new Error('清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp，或在模型池里勾选云端语音转文字')
        const res = await globalThis.ccUI.aiRun(
          { kind: 'transcribe', url: location.href, title: document.title },
          { interval: 800, stop: () => S.closed, onProgress: (j) => setStatus(`${j.note || '清影正在转写…'}${j.total ? ` ${j.done}%` : ''}`) },
        )
        const got = (res.lines || []).map((l) => ({ t: l.t, text: l.text }))
        if (!got.length) throw new Error('没有识别出任何语音')
        // 放进视频会话：摘要面板开着的话也会用上这份转写
        VS.setLines(got, '清影转写', 'sublayer')
        S.lines = got
        S.trans = []
      } catch (e) {
        setStatus(e.message)
        toast(ui, e.message, true, 6000)
        return
      } finally {
        S.busy = false
      }
      setStatus(`转写完成，共 ${S.lines.length} 句`)
      translateAll()
    }

    function exportSrt() {
      if (!S.lines.length) return toast(ui, '还没有字幕', true)
      const text = L.toSrt(S.lines, S.trans, S.prefs.mode, S.prefs.delay)
      const name = `${(document.title || 'subtitle').replace(/[\\/:*?"<>|]/g, '_').slice(0, 80)}.srt`
      const a = h('a', { href: URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' })), download: name })
      document.body.append(a)
      a.click()
      a.remove()
      toast(ui, `已导出 ${name}`)
    }

    Object.assign(C, { exportSrt, transcribe, translateAll, loadLines })
  }
})()
