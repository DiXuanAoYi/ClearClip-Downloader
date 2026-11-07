// 视频会话：同一个页面里，视频摘要面板和字幕层共用“这个视频的字幕和译文”。
// - 谁先读到字幕（摘要读文稿、字幕层自己读、转写完成）就放进会话，另一个直接用，不再各读一遍。
// - 译文只有一条路：会话里按目标语言翻（分批、能看到进度），翻过的同一份字幕不再翻；摘要面板的“译文”和字幕层的译文是同一份。
// - 字幕层只管显示：它订阅会话，会话里的字幕换了（换了字幕轨、换了视频），它跟着换。
;(() => {
  if (globalThis.ccVSession) return
  const api = globalThis.browser ?? globalThis.chrome
  const KEEP = 12
  const BATCH = 60

  const S = { vk: '', lines: [], track: '', source: '', trans: new Map(), pending: new Map(), subs: new Set(), pv: '' }

  const videoKey = () => globalThis.ccVsum?.videoKey?.(location.href) || location.href.split('#')[0]

  /** 一份字幕的指纹：同一个视频换了字幕轨，译文缓存不会错配。 */
  function fingerprint(lines) {
    let h = 2166136261
    for (const l of lines) {
      const s = `${l.t}|${l.text}\n`
      for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619)
    }
    return (h >>> 0).toString(36) + lines.length
  }

  /** 目标语言统一成一个键（“中文”“简体中文”“Chinese”是同一个）。 */
  function langKey(target) {
    const t = String(target || '').trim().toLowerCase()
    if (/^(zh|zh-cn|zh-hans|中文|简体中文|汉语|chinese|simplified chinese)$/.test(t)) return 'zh'
    if (/^(zh-tw|zh-hant|繁體中文|繁体中文|traditional chinese)$/.test(t)) return 'zh-tw'
    if (/^(en|english|英语|英文)$/.test(t)) return 'en'
    if (/^(ja|japanese|日语|日文|日本語)$/.test(t)) return 'ja'
    if (/^(ko|korean|韩语|韩文|한국어)$/.test(t)) return 'ko'
    return t
  }

  function emit(what) {
    for (const fn of S.subs) {
      try {
        fn(what, snapshot())
      } catch {
        // 订阅者出错不影响别的
      }
    }
  }

  function snapshot() {
    return { vk: S.vk, lines: S.lines, track: S.track, source: S.source }
  }

  /** 当前页面的视频换了（单页应用里切换视频）：会话清空。 */
  function current() {
    const vk = videoKey()
    if (vk !== S.vk) {
      S.vk = vk
      S.lines = []
      S.track = ''
      S.source = ''
      S.trans = new Map()
      S.pending = new Map()
    }
    return snapshot()
  }

  /** 放进一份字幕。`source`：谁读到的（vsum / sublayer / transcribe）。内容没变就什么也不做。 */
  function setLines(lines, track = '', source = '') {
    current()
    const next = (lines || []).map((l) => ({ t: Number(l.t) || 0, text: String(l.text || '') }))
    if (next.length && fingerprint(next) === fingerprint(S.lines) && track === S.track) return false
    S.lines = next
    S.track = track || ''
    S.source = source
    S.trans = new Map()
    S.pending = new Map()
    emit('lines')
    return true
  }

  /** 读字幕：会话里有就直接给；没有就读一遍（`ccVsumSource.load`）并放进会话。 */
  async function load(opts = {}) {
    const cur = current()
    if (cur.lines.length && !opts.force) return { lines: cur.lines, track: cur.track, fromSession: true }
    const d = await globalThis.ccVsumSource.load(opts.url)
    if (videoKey() === S.vk) setLines(d.lines || [], d.track || '', opts.source || '')
    return d
  }

  const cacheKey = (target) => `${S.vk}|${langKey(target)}|${fingerprint(S.lines)}|${S.pv}`

  /** 已有的译文（没有返回 null）。 */
  function translations(target) {
    current()
    return S.trans.get(langKey(target)) || null
  }

  /** 别处已经翻好的整份译文（比如摘要面板的 AI 翻译）：放进会话，字幕层直接用。 */
  async function putTranslations(target, arr) {
    current()
    if (!Array.isArray(arr) || arr.length !== S.lines.length) return
    S.trans.set(langKey(target), arr.map((x) => String(x || '')))
    emit('trans')
    await saveCache(target, arr)
  }

  async function saveCache(target, arr) {
    try {
      const all = (await api.storage.local.get('subLayerTrans')).subLayerTrans || {}
      all[cacheKey(target)] = arr
      const keys = Object.keys(all)
      for (const k of keys.slice(0, Math.max(0, keys.length - KEEP))) delete all[k]
      await api.storage.local.set({ subLayerTrans: all })
    } catch {
      // 存储满了：下次再翻
    }
  }

  /**
   * 翻译整份字幕（分批 40 句），同一时间同一种语言只翻一次：字幕层和摘要面板同时要，等的是同一个结果。
   * `onProgress(done, total, arr)`：每批译完调用一次，arr 是到目前为止的译文（没译到的是空串）。
   */
  function translate(target, { onProgress, stop } = {}) {
    current()
    const lk = langKey(target)
    const have = S.trans.get(lk)
    if (have) return Promise.resolve(have)
    if (S.pending.has(lk)) {
      const p = S.pending.get(lk)
      if (onProgress) p.listeners.add(onProgress)
      return p.promise
    }
    const listeners = new Set(onProgress ? [onProgress] : [])
    const vk = S.vk
    const lines = S.lines
    const promise = (async () => {
      const key = cacheKey(target)
      const saved = ((await api.storage.local.get('subLayerTrans')).subLayerTrans || {})[key]
      if (Array.isArray(saved) && saved.length === lines.length) {
        if (S.vk === vk && S.lines === lines) S.trans.set(lk, saved)
        return saved
      }
      const arr = new Array(lines.length).fill('')
      const idx = lines.map((_, i) => i).filter((i) => lines[i].text.trim())
      const groups = []
      for (let i = 0; i < idx.length; i += BATCH) groups.push(idx.slice(i, i + BATCH))
      let done = 0
      for (const g of groups) {
        if (stop?.() || S.vk !== vk || S.lines !== lines) throw new Error('已取消')
        // 和摘要面板的“翻译文稿”是同一种任务（字幕档案、翻译记忆、术语库）；t 用序号，结果按序号对回去
        const x = await globalThis.ccUI.aiRun(
          { kind: 'translate', lang: target, title: document.title, url: location.href, lines: g.map((i) => ({ t: i, text: lines[i].text })) },
          { interval: 500, stop, timeoutMs: 5 * 60_000 },
        )
        const got = new Map((x.lines || []).map((l) => [Number(l.t), String(l.text || '')]))
        for (const i of g) arr[i] = got.get(i) || ''
        done += g.length
        for (const fn of listeners) fn(done, idx.length, arr)
      }
      if (S.vk === vk && S.lines === lines) {
        S.trans.set(lk, arr)
        emit('trans')
      }
      await saveCache(target, arr)
      return arr
    })()
    S.pending.set(lk, { promise, listeners })
    promise.finally(() => S.pending.get(lk)?.promise === promise && S.pending.delete(lk)).catch(() => {})
    return promise
  }

  /** 订阅会话变化：fn('lines' | 'trans', snapshot)。返回取消订阅的函数。 */
  function subscribe(fn) {
    S.subs.add(fn)
    return () => S.subs.delete(fn)
  }

  globalThis.ccVSession = {
    current,
    setLines,
    load,
    translations,
    putTranslations,
    translate,
    subscribe,
    setPromptVersion: (pv) => (S.pv = String(pv || '')),
    langKey,
    fingerprint,
  }
})()
