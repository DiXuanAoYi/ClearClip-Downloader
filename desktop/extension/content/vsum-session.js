// 视频摘要面板的拆分模块（由 content/vsum.js 调用）：面板状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 会话缓存（同一份文稿的结果、对话）与进行中的任务记录，向清影提交任务。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsum ||= {})
  P.session = (C) => {
    const { S, V, send, store } = C
    const cached = (...a) => C.cached(...a)
    const ensureReady = (...a) => C.ensureReady(...a)
    const linesForReq = (...a) => C.linesForReq(...a)
    const outLang = (...a) => C.outLang(...a)
    const putCache = (...a) => C.putCache(...a)
    const renderProgress = (...a) => C.renderProgress(...a)

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
          interval: kind === 'preclean' ? 80 : 700,
          keepOnStop: true,
          stop: () => S.closed,
          onSubmit: async (x) => {
            job.id = x
            if (tag) await setJobRec(tag, { id: x, kind, page, ts: Date.now() })
            if (!S.job && kind !== 'preclean') S.job = job // 预处理很快，不显示进度也不弹“处理好了”；同时有多个后台任务时，进度条只显示最先开始的那个
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


    Object.assign(C, { submit, run, persist, ago, saveSession, restoreSession, jobRecs })
  }
})()
