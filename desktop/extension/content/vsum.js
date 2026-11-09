// 视频摘要面板：按需注入的侧栏。再次注入（快捷键 Alt+Shift+S）= 展开 / 收起到右边；要关掉点 ✕。
// 字幕在页面里读取，交给本机的清影，由清影调用“设置 → AI”里的大模型；密钥不会进到扩展或网页里。
;(() => {
  if (globalThis.__ccVsum) return globalThis.__ccVsum.toggle()
  const { mount, h, send, toast, api } = globalThis.ccUI
  const V = globalThis.ccVsum
  const Src = globalThis.ccVsumSource
  const VS = globalThis.ccVSession

  const { CSS, DARK: DARK0 } = globalThis.ccVsumStyle
  const Dk = globalThis.ccVsumDock
  const DARK = DARK0 + Dk.DARK
  const ui = mount(CSS + Dk.CSS)
  let dock = null // 收到右边（vsum-dock.js）：面板准备好后创建
  // 外观：主题（跟随系统 / 浅色 / 深色）和字号，改了马上生效
  const theme = document.createElement('style')
  const fontStyle = document.createElement('style')
  ui.root.append(theme, fontStyle)
  const isDark = () => (S.prefs.theme === 'dark' ? true : S.prefs.theme === 'light' ? false : matchMedia('(prefers-color-scheme: dark)').matches)
  function applyAppearance() {
    theme.textContent = S.prefs.theme === 'dark' ? DARK : S.prefs.theme === 'light' ? '' : `@media (prefers-color-scheme: dark) {${DARK}}`
    const fs = Math.min(20, Math.max(11, Number(S.prefs.fs) || 13))
    fontStyle.textContent = `.panel { font-size: ${fs}px; }`
  }
  const S = {
    appLang: '', // 清影界面语言（zh / en / ja），语言下拉框按它显示
    data: null, // 当前文稿：{ title, url, lines, chapters, tracks, ... }
    tab: 'summary',
    status: null, // 清影的状态：{ ready, model, lang, transcribe, maxChars }
    prefs: { style: 'standard', lang: '', follow: true, tab: 'summary', autoOnline: true, autoTranscribe: true, srcV: 1, view: 'both', lead: 3, prompts: {}, theme: 'auto', fs: 13 },
    res: {}, // 本次的结果：summary / outline / flow / cards / quotes / translated / rewrite
    chat: [],
    hit: 0, // 当前搜索命中的第几处
    detached: false, // 用户手动滚走了，不再自动跟随
    job: null, // { id, kind, done, total }
    busy: false,
    query: '',
    showTr: true,
    notes: [], // 所有视频的笔记
    scope: 'video',
    noteQuery: '',
    sel: null,
    timer: 0,
    closed: false,
    restored: 0, // 内容是上次带出来的：保存时间
  }

  const panel = h('div', { class: 'layer panel', role: 'complementary', 'aria-label': '视频摘要' })
  ui.add(panel)

  // ---------- 小工具 ----------

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
  const savePrefs = () => store.set('vsumPrefs', { ...S.prefs, tab: S.tab })
  const TL = globalThis.ccTrans
  /** 写进请求的输出语言（提示词里的名字）；留空 = 清影设置里的目标语言（中文视频的翻译默认译成英文）。 */
  const outLang = () => (S.prefs.lang.trim() ? TL.promptName(S.prefs.lang) : S.status?.lang || '中文')
  const outLabel = () => {
    if (!S.prefs.lang.trim()) return ''
    const code = TL.LANGS.find((l) => l.code === S.prefs.lang.trim() || l.en === TL.promptName(S.prefs.lang))?.code
    return code ? TL.label(code, S.appLang).split(' · ')[0] : S.prefs.lang.trim()
  }
  const video = () => [...document.querySelectorAll('video')].sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0] || null
  // 导入 / 粘贴的字幕带了时间戳、而且页面上有视频：和视频自己的字幕一样用（显示时间、点击跳转、跟随播放、做字幕层）
  const isVideo = () => !!S.data && S.data.site !== 'article' && (S.data.site !== 'manual' || (!!S.data.timed && !!video()))

  function seek(ms) {
    const v = video()
    if (v && isVideo()) {
      v.currentTime = ms / 1000
      v.play?.().catch(() => {})
    } else if (/^https?:/.test(S.data?.url || '') && S.data?.site !== 'article') window.open(V.timeLink(S.data.url, ms), '_blank', 'noopener')
  }

  /** 把文字里的 [m:ss] 变成能点的时间。 */
  function linkify(text) {
    const out = []
    let last = 0
    for (const m of String(text).matchAll(/\[(\d{1,2}(?::\d{2}){1,2})\]/g)) {
      out.push(text.slice(last, m.index))
      const ms = V.parseClock(m[1])
      out.push(h('button', { class: 'ts', text: `[${m[1]}]`, onclick: () => seek(ms) }))
      last = m.index + m[0].length
    }
    out.push(text.slice(last))
    return out
  }

  /** 底部操作栏：第一个按钮占满宽度，滚到哪里都点得到。 */
  const actsBar = (...kids) => h('div', { class: 'acts' }, ...kids)
  /** “更多”页的分组卡片。 */
  const grp = (title, ...kids) => h('div', { class: 'grp' }, h('div', { class: 'gh', text: title }), h('div', { class: 'gb' }, ...kids.filter(Boolean)))

  const tsBtn = (ms) => (isVideo() && ms >= 0 ? h('button', { class: 'ts', text: V.fmtClock(ms), onclick: () => seek(ms) }) : null)

  function copy(text, what = '内容') {
    navigator.clipboard.writeText(text).then(
      () => toast(ui, `已复制${what}`),
      () => toast(ui, '浏览器不让复制，请手动选中后复制', true),
    )
  }

  function saveFile(name, text, type = 'text/plain') {
    const a = document.createElement('a')
    a.href = URL.createObjectURL(new Blob([text], { type: `${type};charset=utf-8` }))
    a.download = name
    ui.add(a)
    a.click()
    setTimeout(() => (URL.revokeObjectURL(a.href), a.remove()), 1000)
  }

  const sel = (opts, value, onchange, title) => {
    const s = h('select', { title }, ...opts.map(([v, n]) => h('option', { value: v, text: n })))
    s.value = value
    s.onchange = () => onchange(s.value)
    return s
  }

  /** 输出 / 翻译语言下拉：自动 + 常用的几十种语言；以前手填的别的写法也保留成一项。 */
  function langSelect(title, after) {
    const known = TL.LANGS.some((l) => l.code === S.prefs.lang)
    const opts = [['', '跟随视频'], ...TL.LANGS.map((l) => [l.code, TL.label(l.code, S.appLang)])]
    if (S.prefs.lang && !known) opts.push([S.prefs.lang, S.prefs.lang])
    return sel(opts, S.prefs.lang, (v) => ((S.prefs.lang = v), savePrefs(), after?.()), title)
  }

  // ---------- 与清影通信 ----------

  async function ensureReady() {
    const r = await send({ type: 'cc-ai-status' })
    if (!r.ok) {
      S.status = null
      throw new Error(r.error)
    }
    S.status = r
    if (!r.ready) throw new Error('清影里还没有设置大模型。请打开清影 → 设置 → AI，填好接口地址、密钥和模型。')
    return r
  }

  /** 缓存键里用的行：只留时间和文字。 */
  function linesForReq() {
    const max = S.status?.maxChars || 600000
    const out = []
    let n = 0
    for (const l of S.data.lines) {
      n += l.text.length + 8
      if (n > max) break
      out.push({ t: l.t, text: l.text })
    }
    return out
  }

  async function cached(key) {
    const all = await store.get('vsumCache', {})
    return all[key]?.data
  }
  async function putCache(key, data) {
    const all = await store.get('vsumCache', {})
    all[key] = { ts: Date.now(), data }
    await store.set('vsumCache', V.pruneCache(all, 40, 3_000_000))
  }

  // ---------- 会话缓存：同一份文稿的所有结果、对话，关掉再开直接带出来 ----------
  const SESS_KEEP = 40
  const sessKey = () => V.cacheKey(S.data.lines, 'session')
  let saveTimer = 0
  function persist() {
    clearTimeout(saveTimer)
    saveTimer = setTimeout(saveSession, 300)
  }
  async function saveSession() {
    if (!S.data?.lines?.length) return
    const res = { ...S.res }
    delete res.rewrite
    const empty = !Object.values(res).some(Boolean) && !S.chat.length
    const all = await store.get('vsumSessions', {})
    const key = sessKey()
    if (empty) delete all[key]
    else all[key] = { ts: Date.now(), key, vk: V.videoKey(S.data.url || location.href), url: S.data.url || location.href, data: { res, chat: S.chat.slice(-40), style: S.prefs.style, lang: outLang(), title: S.data.title } }
    await store.set('vsumSessions', V.pruneCache(all, SESS_KEEP, 2_500_000))
  }
  /** 取回上次的会话；摘要风格或输出语言变了，对应的结果不带。 */
  async function restoreSession(d) {
    const all = await store.get('vsumSessions', {})
    const hit = all[V.cacheKey(d.lines, 'session')]
    if (!hit?.data || S.data !== d) return false
    const { res = {}, chat = [], style, lang } = hit.data
    const out = { ...res }
    if (style !== S.prefs.style) delete out.summary
    if (lang !== outLang()) {
      delete out.summary
      delete out.translated
    }
    if (out.translated && out.translated.length !== d.lines.length) delete out.translated
    if (!Object.values(out).some(Boolean) && !chat.length) return false
    S.res = { ...S.res, ...out }
    if (chat.length) S.chat = chat
    S.restored = hit.ts
    return true
  }
  const ago = (ts) => {
    const m = Math.max(0, Math.round((Date.now() - ts) / 60000))
    if (m < 1) return '刚刚'
    if (m < 60) return `${m} 分钟前`
    if (m < 1440) return `${Math.round(m / 60)} 小时前`
    return `${Math.round(m / 1440)} 天前`
  }

  // 正在进行的任务：关掉面板不取消，重开后接着等（清影那边会继续跑）
  const JOB_FRESH = 25 * 60 * 1000
  async function jobRecs() {
    const all = await store.get('vsumJobs', {})
    const now = Date.now()
    return Object.fromEntries(Object.entries(all).filter(([, v]) => now - v.ts < JOB_FRESH))
  }
  async function setJobRec(tag, rec) {
    const all = await jobRecs()
    if (rec) all[tag] = rec
    else delete all[tag]
    await store.set('vsumJobs', all)
  }

  /** 提交 → 轮询。cache 为 false 时不读不写缓存（问答、改写）。 */
  async function run(kind, extra = {}, cache = true) {
    await ensureReady()
    const lang = outLang()
    const style = kind === 'summary' ? S.prefs.style : ''
    const key = cache && !S.prefs.prompts?.[kind] ? V.cacheKey(S.data.lines, kind, style + (extra.format || ''), lang) : ''
    if (key) {
      const hit = await cached(key)
      if (hit) return hit
    }
    const own = String(S.prefs.prompts?.[kind] || '').trim()
    const req = { kind, title: S.data.title, lang, style, lines: linesForReq(), ...(own ? { system: own } : {}), ...extra }
    const result = await submit(req, kind, key)
    if (key) putCache(key, result)
    return result
  }

  /** 提交一个任务并轮询到结束，返回结果；进度显示在面板顶部，可取消。 */
  async function submit(req, kind, tag = '', onPartial = null) {
    const page = S.data?.url || location.href
    let id = 0
    if (tag) {
      // 上次关掉面板时这个任务还在跑：接上它，不再重新提交
      const old = (await jobRecs())[tag]
      if (old) {
        const j = await send({ type: 'cc-ai-job', id: old.id })
        if (j.ok && (j.state === 'running' || j.state === 'done')) id = old.id
      }
    }
    const job = { id, kind, tag, done: 0, total: 0, note: '' }
    let finished = false
    try {
      // 关掉面板不取消：清影继续跑，重新打开后接上（keepOnStop）
      return await globalThis.ccUI.aiRun(req, {
        id,
        interval: 700,
        keepOnStop: true,
        stop: () => S.closed,
        onSubmit: async (x) => {
          job.id = x
          if (tag) await setJobRec(tag, { id: x, kind, page, ts: Date.now() })
          if (!S.job) S.job = job // 同时有多个后台任务时，进度条只显示最先开始的那个
          renderProgress()
        },
        onProgress: (j) => {
          job.done = j.done
          job.total = j.total
          job.note = j.note || ''
          if (j.state !== 'running') finished = true
          if (onPartial && j.partial?.lines?.length) onPartial(j.partial)
          renderProgress()
        },
      })
    } catch (e) {
      finished = finished || !S.closed
      throw e
    } finally {
      if (tag && finished) setJobRec(tag, null)
      if (S.job === job) S.job = null
      if (!S.closed) renderProgress()
    }
  }

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
    const key = audio ? `stt:${url}` : `sub:${url}:${v}`
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
    S.data = { ...S.data, lines: p.lines, durationMs: p.durationMs || S.data.durationMs || 0, track: `清影转写（识别中 ${Math.round((p.progress || 0) * 100)}%）`, streaming: true, noSubtitle: false, note: '', trackUrl: '' }
    render()
  }

  async function transcribeNow() {
    if (S.busy) return toast(ui, '上一个任务还在进行', true)
    if (S.status?.transcribe === false) return toast(ui, '清影里还没有配置语音转文字：到“设置 → AI”选择本机 whisper.cpp 或填写接口', true, 6000)
    const url = S.data?.url || location.href
    const title = S.data?.title || document.title
    S.busy = true
    render()
    try {
      const key = `tr:${url}`
      let r = await cached(key)
      if (!r) {
        // 识别出多少先显示多少；全部识别完后用完整结果覆盖，保证每句都有
        r = await submit({ kind: 'transcribe', url, title, lang: spokenHint(S.data) }, 'transcribe', key, showPartial)
        if (r?.lines?.length) putCache(key, r)
      }
      if (!r?.lines?.length) throw new Error('没有识别出任何语音')
      setData({ ...S.data, lines: r.lines, durationMs: r.durationMs || S.data?.durationMs || 0, track: r.track, spoken: r.lang || S.data?.spoken || '', trackLang: r.lang || '', trackAuto: true, streaming: false, noSubtitle: false, note: '', trackUrl: '', tracks: S.data?.tracks || [] })
      toast(ui, `转写完成，共 ${r.lines.length} 句`)
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
    for (const [key, name] of [[`tr:${d.url}`, '清影转写'], [`sub:${d.url}`, null]]) {
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
        const skey = `sub:${d.url}`
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

  // ---------- 框架 ----------

  const TABS = [['summary', '摘要'], ['chapters', '章节'], ['transcript', '文稿'], ['mindmap', '导图'], ['ask', '问答'], ['notes', '笔记'], ['vocab', '生词'], ['comments', '评论'], ['more', '更多']]

  function renderProgress() {
    dock?.update()
    const el = ui.root.getElementById('prog')
    if (!el) return
    el.replaceChildren()
    if (!S.job) return
    const pct = S.job.total ? Math.round((S.job.done / S.job.total) * 100) : 5
    el.append(
      h('div', { class: 'mute' }, `${S.job.note || '清影正在处理…'}${S.job.total ? (S.job.kind === 'transcribe' ? ` ${S.job.done}%` : ` ${S.job.done}/${S.job.total}`) : ''} `, h('button', { class: 'sec', text: '取消', onclick: () => (S.job.cancel ? S.job.cancel() : send({ type: 'cc-ai-cancel', id: S.job.id })) })),
      h('div', { class: 'bar' }, h('i', { style: `width:${pct}%` })),
    )
  }

  function render() {
    const keepScroll = ui.root.querySelector('.body')?.scrollTop || 0
    const d = S.data
    const children = [
      h('div', { class: 'hd' },
        h('span', { class: 't', title: d?.title || '', text: d ? d.title : '视频摘要' }),
        h('button', { class: 'sec', text: '重新读取', title: '重新读取当前页面的字幕（切换视频后用）', onclick: () => start() }),
        dock?.collapseButton(),
        h('button', { class: 'sec', text: '✕', title: '关闭', 'aria-label': '关闭', onclick: stop }),
      ),
    ]
    if (d) {
      children.push(h('div', { class: 'meta' },
        d.lines.length ? `${d.lines.length} 段` : '无文稿',
        d.track && `· ${d.track}`,
        d.durationMs > 0 && `· ${V.fmtClock(d.durationMs)}`,
        S.status && !S.status.ready && h('span', { style: 'color:#a8362d', text: '· 清影里未设置大模型' })))
      children.push(h('div', { class: 'tabs', role: 'tablist' }, ...TABS.map(([id, n]) => h('button', { class: S.tab === id ? 'on' : '', role: 'tab', 'aria-selected': S.tab === id, text: id === 'vocab' && VOCAB.list ? `${n} ${VOCAB.list.length}` : n, onclick: () => ((S.tab = id), savePrefs(), id === 'vocab' && void loadVocab(), render()) }))))
    }
    const body = h('div', { class: 'body' }, h('div', { id: 'prog' }))
    if (!d) {
      // 读取期间也先把各标签页画出来（灰着），不要整个面板只剩一行字
      children.push(h('div', { class: 'tabs', role: 'tablist' }, ...TABS.map(([, n]) => h('button', { disabled: true, role: 'tab', text: n }))))
      body.append(h('div', { class: 'mute', text: '正在读取字幕…（最多等 25 秒）' }))
    }
    else {
      if (!d.lines.length && S.tab !== 'more' && S.tab !== 'notes' && S.tab !== 'vocab' && S.tab !== 'comments') body.append(noData())
      else {
        const fn = { summary: tabSummary, chapters: tabChapters, transcript: tabTranscript, mindmap: tabMindmap, ask: tabAsk, notes: tabNotes, vocab: tabVocab, comments: tabComments, more: tabMore }[S.tab]
        body.append(fn())
      }
    }
    children.push(body)
    if (d && S.tab === 'ask' && d.lines.length) {
      const inp = h('input', { type: 'text', placeholder: '问点什么…', id: 'q', onkeydown: (e) => e.key === 'Enter' && !e.isComposing && go() })
      const go = () => {
        const q = inp.value
        inp.value = ''
        ask(q)
      }
      children.push(h('div', { class: 'ask' }, inp, h('button', { text: '发送', disabled: S.busy, onclick: go })))
    }
    panel.replaceChildren(...children)
    renderProgress()
    dock?.update()
    ui.root.querySelector('.body').scrollTop = keepScroll
    if (S.jumpHit) {
      S.jumpHit = false
      ui.root.querySelector('.ln.now')?.scrollIntoView({ block: 'center' })
    }
    if (S.keepFocus) {
      S.keepFocus = false
      const i = ui.root.querySelector('.body input[type=text]')
      if (i) {
        i.focus()
        i.setSelectionRange(i.value.length, i.value.length)
      }
    }
  }

  /** 跟随播放：高亮当前行 / 章节。 */
  // 站内切换视频（YouTube / B 站不刷新页面）：网址对应的视频变了就自动重新读字幕，旧视频的结果先存起来、字幕层关掉
  function watchNav() {
    if (S.closed || S.navBusy) return
    const k = V.videoKey(location.href)
    if (!S.pageKey) S.pageKey = k
    if (k === S.pageKey) return
    S.pageKey = k
    S.navBusy = true
    saveSession().catch(() => {})
    globalThis.ccSubLayerRun?.destroy()
    // 等页面把标题、播放器换成新视频的再读
    setTimeout(() => start().finally(() => (S.navBusy = false)), 700)
  }

  function tick(force) {
    watchNav()
    dock?.update()
    const v = video()
    if (!v || !S.data || !isVideo()) return
    const ms = v.currentTime * 1000
    if (S.tab === 'transcript') {
      const i = V.lineAt(S.data.lines, ms)
      const prev = ui.root.querySelector('.ln.cur')
      const cur = ui.root.querySelector(`.ln[data-i="${i}"]`)
      if (prev !== cur) {
        prev?.classList.remove('cur')
        cur?.classList.add('cur')
        if (S.prefs.follow && cur && !S.query && !S.detached) cur.scrollIntoView({ block: 'center', behavior: 'smooth' })
      } else if (force && cur) cur.scrollIntoView({ block: 'center', behavior: 'smooth' })
    } else if (S.tab === 'chapters') {
      const i = V.chapterAt(chapterList().list, ms)
      const cur = ui.root.querySelector(`.ln[data-ch="${i}"]`)
      if (cur && !cur.classList.contains('cur')) {
        ui.root.querySelector('.ln.cur')?.classList.remove('cur')
        cur.classList.add('cur')
      }
    }
  }

  document.addEventListener('mousemove', (e) => (globalThis.__ccPtr = { x: e.clientX, y: e.clientY, t: Date.now() }), { capture: true, passive: true })
  // 视频胶囊（vdock.js）的菜单里：记笔记、回到面板、关闭摘要（以前视频左上角单独有个“笔记 (n)”按钮，和播放增强的工具条叠在一起）
  const Dock = globalThis.ccVDock
  Dock.register('vsum', {
    order: 40,
    title: '视频摘要',
    applies: () => !S.closed && !!S.data && isVideo(),
    menu: () => {
      const cnt = S.notes.filter((n) => n.key === localKey()).length
      return [
        { label: `记下当前这句（已有 ${cnt} 条）`, hint: 'n', onclick: () => quickNote() || toast(ui, '页面上没有找到视频', true) },
        { label: dock?.state === 'open' ? '收起到右边' : '展开摘要面板', hint: 'Alt+Shift+S', onclick: () => dock?.toggle() },
        { label: '关闭视频摘要', onclick: () => stop() },
      ]
    },
  })

  /** 读到字幕后，同时把文稿作为字幕层叠在视频上（已经开着就换成新文稿）；可在文稿页关掉。 */
  let subOpenAt = 0
  function syncSubLayer(d) {
    if (!isVideo() || !d.lines.length) return
    // 文稿放进视频会话：字幕层开着的话订阅了会话，自己跟着换
    VS.setLines(d.lines, d.track || '', 'vsum')
    if (globalThis.ccSubLayerRun) return
    if (S.prefs.autoSub === false) return
    // 正在打开的这几秒里不再重复发（再注入一次会把字幕层关掉）；字幕层打开后从会话里取文稿
    if (Date.now() - subOpenAt < 4000) return
    subOpenAt = Date.now()
    send({ type: 'cc-sublayer-open' })
  }

  // 字幕层翻好的译文（同一份文稿、同一种语言）直接显示在文稿里
  const unsubSession = VS.subscribe((what) => {
    if (what !== 'trans' || S.closed || !S.data?.lines.length || S.res.translated) return
    if (VS.fingerprint(S.data.lines.map((l) => ({ t: l.t, text: l.text }))) !== VS.fingerprint(VS.current().lines)) return
    const have = VS.translations(outLang())
    if (have && have.length === S.data.lines.length) {
      S.res.translated = have
      render()
    }
  })

  function setData(d) {
    S.data = d
    S.res = {}
    S.chat = []
    S.query = ''
    S.preview = null
    S.restored = 0
    render()
    if (!d.lines.length) return
    syncSubLayer(d)
    // 同一份文稿以前生成的所有结果和对话直接带出来（不需要大模型）；会话里没有的摘要再去缓存里找
    restoreSession(d)
      .then(async () => {
        if (S.data !== d) return
        if (!S.res.summary) {
          const hit = await cached(V.cacheKey(d.lines, 'summary', S.prefs.style, outLang()))
          if (hit && S.data === d && !S.res.summary) {
            S.res.summary = hit
            S.restored ||= Date.now()
          }
        }
        render()
        resumeJobs(d)
      })
      .catch(() => {})
  }

  const RESUME = {
    summary: (x) => (S.res.summary = x),
    mindmap: (x) => (S.res.outline = x.outline),
    cards: (x) => (S.res.cards = x.cards),
    quotes: (x) => (S.res.quotes = x.quotes),
    translate: (x) => (S.res.translated = S.data.lines.map((_, i) => x.lines?.[i]?.text || '')),
  }
  /** 上次关面板时还在生成的东西：清影还在跑或已经跑完，接上取回来。 */
  async function resumeJobs(d) {
    const page = d.url || location.href
    const recs = Object.values(await jobRecs()).filter((r) => r.page === page && RESUME[r.kind] && !S.res[{ summary: 'summary', mindmap: 'outline', cards: 'cards', quotes: 'quotes', translate: 'translated' }[r.kind]])
    for (const r of recs) {
      if (S.data !== d || S.busy) return
      toast(ui, `接上上次没做完的${r.kind === 'summary' ? '摘要' : '任务'}…`)
      await task(r.kind, {}, RESUME[r.kind])
    }
  }

  async function start() {
    S.pageKey = V.videoKey(location.href)
    S.data = null
    render()
    try {
      // 读字幕最多等 25 秒：网络慢或后台没响应时，不要让面板一直停在“正在读取”，直接给出可操作的界面
      const d = await Promise.race([
        Src.load(),
        new Promise((r) => setTimeout(() => r({ title: document.title, url: location.href, site: 'html5', lines: [], chapters: [], tracks: [], note: '读取字幕超时（网络慢或站点没响应）。可以点“重新读取”，或在“更多”里导入字幕 / 用清影转写。', fetchFailed: true }), 25000)),
      ])
      S.recovering = !d.lines.length // 先走“本地字幕 / 缓存 / 在线字幕 / 自动转写”，都没有结果再列字幕轨
      setData(d)
      try {
        if (!d.lines.length) await recover(d)
      } finally {
        if (S.recovering) {
          S.recovering = false
          render()
        }
      }
    } catch (e) {
      setData({ title: document.title, url: location.href, site: 'html5', lines: [], chapters: [], tracks: [], note: `读取失败：${e.message}` })
    }
    ensureReady().catch(() => {}).finally(() => S.data && render())
    refreshAppLang()
  }

  /** 语言下拉框按清影的界面语言显示：先用上次记下的，再问一下清影，变了就重画。 */
  async function refreshAppLang() {
    try {
      S.appLang = (await api.storage.local.get('appLang')).appLang || ''
    } catch {
      // 读不到就用浏览器语言
    }
    const r = await send({ type: 'cc-app-lang' })
    if (r?.ok && r.lang && r.lang !== S.appLang) {
      S.appLang = r.lang
      if (S.data) render()
    }
  }

  function stop() {
    clearInterval(S.timer)
    document.removeEventListener('keydown', onKey, true)
    // 正在生成的任务不取消：清影继续跑，重新打开后自动接上（要取消请点进度条上的“取消”）
    S.closed = true
    dock?.destroy()
    unsubSession()
    Dock.unregister('vsum')
    saveSession().catch(() => {})
    ui.remove()
    delete globalThis.__ccVsum
  }

  // ---------- 各标签页（vsum-*.js）：依赖从这里传进去 ----------

  const TABS_ = globalThis.ccVsumTabs
  const { getSel, tabTranscript } = TABS_.transcript({
    S, V, actsBar, addNote, copy, h, isVideo, langSelect, onSelect, outLabel, render, saveFile, savePrefs, seek,
    sel, send, setData, syncSubLayer, task, tick, toast, trackBar, translateDoc, ui, video
  })
  const { exportData, tabMore } = TABS_.more({
    S, Src, V, applyAppearance, copy, forgetLocal, grp, h, importFile, isVideo, loadArticleFromSelection,
    reloadTrack, rememberLocal, render, saveFile, savePrefs, sel, send, sendForTranscribe, setData, sourcePicker,
    store, syncToApp, task, tick, toast, transcribeNow, tsBtn, ui
  })
  const { tabNotes } = TABS_.notes({
    S, V, actsBar, copy, exportData, h, isVideo, localKey, quickNote, render, run, saveFile, seek, sel, store,
    toast, ui
  })
  const { tabComments } = TABS_.comments({
    S, V, actsBar, addNote, copy, ensureReady, h, localKey, outLang, render, saveFile, savePrefs, seek, sel, store,
    submit, toast, ui
  })
  const { VOCAB, loadVocab, tabVocab } = TABS_.vocab({
    S, V, actsBar, addNote, h, localKey, render, saveFile, seek, sel, send, toast, ui
  })
  const { tabMindmap } = TABS_.mindmap({
    S, V, actsBar, copy, h, isDark, isVideo, onKey, render, saveFile, seek, task, toast, ui
  })

  for (const ev of ['wheel', 'touchmove']) {
    ui.root.addEventListener(ev, () => {
      if (S.tab === 'transcript' && S.prefs.follow && isVideo() && !S.detached && !S.query) {
        S.detached = true
        render()
      }
    }, { passive: true })
  }

  dock = Dk.create({ ui, h, panel, V, store, S, render })
  applyAppearance()
  globalThis.__ccVsum = { stop, toggle: () => dock.toggle() }
  S.timer = setInterval(tick, 500)
  document.addEventListener('keydown', onKey, true)
  store.get('vsumNotes', []).then((n) => (S.notes = Array.isArray(n) ? n : []))
  store.get('vsumPrefs', null).then((p) => {
    if (p) {
      S.prefs = { ...S.prefs, ...p }
      // 0.10.3：没有字幕时默认自动取音频转写（以前默认关）。只改一次，之后用户怎么选就怎么算
      if (!p.srcV) {
        S.prefs.autoTranscribe = true
        S.prefs.srcV = 1
      }
      applyAppearance()
      S.tab = TABS.some(([id]) => id === p.tab) ? p.tab : 'summary'
    }
    // 设置了“打开时默认收起到右边”：直接是小标签
    if (S.prefs.collapseOnOpen) dock.set('collapse')
    return start()
  })
})()
