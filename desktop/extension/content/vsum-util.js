// 视频摘要面板的拆分模块（由 content/vsum.js 调用）：面板状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 小工具：存取、外观无关的小函数、复制 / 保存、语言下拉框、与清影通信前的准备。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsum ||= {})
  P.util = (C) => {
    const { S, V, api, h, send, toast, ui } = C


    // ---------- 小工具 ----------

    const store = {
      async get(k, d) {
        try {
          return (await api.storage.local.get(k))[k] ?? d
        } catch (e) {
          globalThis.ccUI.log(`store.get ${k}`, e)
          return d
        }
      },
      // 写失败（多半是存储配额满了）：先清可再生成的缓存重试，还不行就提示用户，不再静默
      async set(k, v) {
        return globalThis.ccUI.storeSet(ui, { [k]: v })
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


    Object.assign(C, { store, outLang, ensureReady, cached, linesForReq, putCache, isVideo, sel, outLabel, savePrefs, langSelect, seek, copy, saveFile, video, tsBtn, actsBar, linkify, TL, grp })
  }
})()
