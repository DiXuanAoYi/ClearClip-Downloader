// 页面字幕层：在任意网页的视频上叠一层字幕——可以双语、调字号和位置、对齐延迟、导出 SRT。
// 字幕来自 YouTube / B 站 / 播放器自带字幕轨，或让清影把音频转写出来；译文由清影用你设置的大模型翻译（走翻译缓存）。
// 再注入一次 = 关闭。
;(() => {
  const prev = globalThis.ccSubLayerRun
  if (prev) {
    prev.destroy()
    return
  }
  const { api, mount, h, send, toast } = globalThis.ccUI
  const L = globalThis.ccSubLayer
  const V = globalThis.ccVsum

  const store = {
    async get(k, d) {
      try {
        return (await api.storage.local.get(k))[k] ?? d
      } catch {
        return d
      }
    },
    async set(k, v) {
      try {
        await api.storage.local.set({ [k]: v })
      } catch {
        // 存不了就算了
      }
    },
  }

  const CSS = `
    .cap { position: fixed; pointer-events: none; text-align: center; display: flex; flex-direction: column; align-items: center; gap: 2px }
    .cap span { display: inline-block; max-width: 92%; background: #000a; color: #fff; padding: 2px 10px; border-radius: 4px; white-space: pre-wrap; text-shadow: 0 1px 2px #000; line-height: 1.35 }
    .cap span.second { color: #ffe9a8 }
    .bar { position: fixed; display: flex; flex-wrap: wrap; gap: 4px; align-items: center; background: #1d2420e6; color: #fff; padding: 5px 8px; border-radius: 8px; font-size: 12px; max-width: 96vw }
    .bar button { padding: 3px 8px; font-weight: 400; background: #ffffff1f; color: #fff }
    .bar button:hover { background: #ffffff38 }
    .bar .st { color: #cfd5d1; max-width: 22em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .bar select { font-size: 12px; padding: 2px; border-radius: 4px }
  `
  const ui = mount(CSS)
  const S = {
    prefs: L.clampPrefs(null),
    lines: [],
    trans: [],
    video: null,
    data: null,
    status: '',
    busy: false,
    closed: false,
    target: '',
    pv: '',
    timer: 0,
    drag: false,
  }

  // ---------- 找视频 ----------

  const pickVideo = () => {
    const list = [...document.querySelectorAll('video')].filter((v) => v.clientWidth > 120 && v.clientHeight > 80)
    return list.sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0] || null
  }

  // ---------- 界面 ----------

  const cap = h('div', { class: 'cap' })
  const stEl = h('span', { class: 'st' })
  const modeBtn = h('button', { onclick: () => update({ mode: L.nextMode(S.prefs.mode) }) })
  const sizeEl = h('span')
  const delayEl = h('span')
  const langSel = h(
    'select',
    { onchange: (e) => update({ target: e.target.value }, true) },
    ...['简体中文', '繁體中文', 'English', '日本語', '한국어', 'Français', 'Deutsch', 'Español', 'Русский'].map((l) => h('option', { value: l, text: l })),
  )
  const bar = h(
    'div',
    { class: 'bar' },
    modeBtn,
    langSel,
    h('button', { text: 'A−', title: '字号减小', onclick: () => update({ size: S.prefs.size - 10 }) }),
    sizeEl,
    h('button', { text: 'A+', title: '字号增大', onclick: () => update({ size: S.prefs.size + 10 }) }),
    h('button', { text: '↑', title: '字幕上移', onclick: () => update({ bottom: S.prefs.bottom + 2 }) }),
    h('button', { text: '↓', title: '字幕下移', onclick: () => update({ bottom: S.prefs.bottom - 2 }) }),
    h('button', { text: '−0.5s', title: '字幕提前 0.5 秒', onclick: () => update({ delay: S.prefs.delay - 500 }) }),
    delayEl,
    h('button', { text: '+0.5s', title: '字幕延后 0.5 秒', onclick: () => update({ delay: S.prefs.delay + 500 }) }),
    h('button', { text: '导出 SRT', onclick: exportSrt }),
    h('button', { text: '用清影转写', title: '这个视频没有字幕时，让清影把音频转成文字', onclick: transcribe }),
    stEl,
    h('button', { text: '×', title: '关闭字幕层', onclick: () => api_destroy() }),
  )
  ui.add(cap)
  ui.add(bar)

  const setStatus = (t) => {
    S.status = t
    stEl.textContent = t
    stEl.title = t
  }

  function layout() {
    const v = S.video
    const r = v && v.isConnected ? v.getBoundingClientRect() : null
    if (!r || r.width < 10) {
      cap.style.display = 'none'
      bar.style.display = 'none'
      return
    }
    bar.style.display = ''
    bar.style.left = `${Math.max(4, r.left + 8)}px`
    bar.style.top = `${Math.max(4, r.top + 8)}px`
    const fs = Math.max(12, Math.round((r.height / 22) * (S.prefs.size / 100)))
    cap.style.cssText = `left:${r.left}px;width:${r.width}px;bottom:${Math.max(0, window.innerHeight - r.bottom) + (r.height * S.prefs.bottom) / 100}px;font-size:${fs}px`
  }

  function paint() {
    const v = S.video
    if (!v || !S.lines.length) return cap.replaceChildren()
    const i = L.indexAt(S.lines, v.currentTime * 1000, S.prefs.delay)
    const rows = i < 0 ? [] : L.compose(S.lines[i].text, S.trans[i], S.prefs.mode)
    const key = `${i}|${S.prefs.mode}|${S.trans[i] || ''}`
    if (cap.dataset.k === key) return
    cap.dataset.k = key
    cap.replaceChildren(...rows.map((t, k) => h('span', { class: k ? 'second' : '', text: t })))
  }

  function renderBar() {
    modeBtn.textContent = L.MODE_NAMES[S.prefs.mode]
    sizeEl.textContent = `${S.prefs.size}%`
    delayEl.textContent = `${S.prefs.delay >= 0 ? '+' : ''}${(S.prefs.delay / 1000).toFixed(1)}s`
    langSel.value = S.target
  }

  async function update(p, retranslate = false) {
    S.prefs = L.clampPrefs({ ...S.prefs, ...p })
    if (p.target !== undefined) S.target = S.prefs.target
    await store.set('subLayerPrefs', S.prefs)
    renderBar()
    layout()
    cap.dataset.k = ''
    paint()
    if (retranslate) {
      S.trans = []
      translateAll()
    }
  }

  // ---------- 字幕与译文 ----------

  async function loadLines() {
    setStatus('正在读取字幕…')
    const d = await globalThis.ccVsumSource.load()
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

  async function transKey() {
    const vk = V.videoKey(location.href)
    return L.cacheKey(vk, S.target, S.pv)
  }

  async function translateAll() {
    if (S.busy || !S.lines.length) return
    S.busy = true
    try {
      const key = await transKey()
      const saved = (await store.get('subLayerTrans', {}))[key]
      if (saved && saved.length === S.lines.length) {
        S.trans = saved
        setStatus('已载入保存的译文')
        return
      }
      S.trans = new Array(S.lines.length).fill('')
      const idx = L.missing(S.lines, S.trans)
      const groups = L.chunk(idx, 40)
      let done = 0
      for (const g of groups) {
        if (S.closed) return
        const r = await send({ type: 'cc-tr', texts: g.map((i) => S.lines[i].text), target: S.target, engine: 'clearclip' })
        if (!r.ok) throw new Error(r.error || '翻译失败')
        g.forEach((i, k) => (S.trans[i] = String(r.texts?.[k] || '')))
        done += g.length
        setStatus(`正在翻译 ${done}/${idx.length}…`)
        cap.dataset.k = ''
        paint()
      }
      const all = await store.get('subLayerTrans', {})
      all[key] = S.trans
      // 只留最近的 12 份，避免占满存储
      const keys = Object.keys(all)
      for (const k of keys.slice(0, Math.max(0, keys.length - 12))) delete all[k]
      await store.set('subLayerTrans', all)
      setStatus(`已翻译成${S.target}，共 ${S.lines.length} 句`)
    } catch (e) {
      setStatus(e.message)
      toast(ui, e.message, true, 6000)
    } finally {
      S.busy = false
    }
  }

  async function transcribe() {
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    S.busy = true
    try {
      const st = await send({ type: 'cc-ai-status' })
      if (!st.ok) throw new Error(st.error)
      if (st.transcribe === false) throw new Error('清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp，或在模型池里勾选云端语音转文字')
      const sub = await send({ type: 'cc-ai-run', req: { kind: 'transcribe', url: location.href, title: document.title } })
      if (!sub.ok) throw new Error(sub.error)
      for (;;) {
        await new Promise((r) => setTimeout(r, 800))
        if (S.closed) return
        const j = await send({ type: 'cc-ai-job', id: sub.id })
        if (!j.ok) throw new Error(j.error)
        setStatus(`${j.note || '清影正在转写…'}${j.total ? ` ${j.done}%` : ''}`)
        if (j.state === 'error') throw new Error(j.error || '转写失败')
        if (j.state === 'cancelled') throw new Error('已取消')
        if (j.state === 'done') {
          S.lines = (j.result?.lines || []).map((l) => ({ t: l.t, text: l.text }))
          if (!S.lines.length) throw new Error('没有识别出任何语音')
          S.trans = []
          break
        }
      }
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

  // ---------- 生命周期 ----------

  function api_destroy() {
    S.closed = true
    clearInterval(S.timer)
    window.removeEventListener('resize', layout)
    ui.remove()
    delete globalThis.ccSubLayerRun
  }
  globalThis.ccSubLayerRun = { destroy: api_destroy }

  ;(async () => {
    S.prefs = L.clampPrefs(await store.get('subLayerPrefs', null))
    const st = await send({ type: 'cc-ai-status' })
    S.pv = st.ok ? String(st.promptVersion || '') : ''
    S.target = S.prefs.target || (st.ok && st.lang) || '简体中文'
    if (![...langSel.options].some((o) => o.value === S.target)) langSel.append(h('option', { value: S.target, text: S.target }))
    S.video = pickVideo()
    if (!S.video) {
      toast(ui, '这个页面没有找到视频', true, 4000)
      return api_destroy()
    }
    renderBar()
    layout()
    window.addEventListener('resize', layout)
    S.timer = setInterval(() => {
      if (!S.video?.isConnected) S.video = pickVideo()
      layout()
      paint()
    }, 150)
    if (await loadLines().catch((e) => (setStatus(e.message), false))) translateAll()
  })()
})()
