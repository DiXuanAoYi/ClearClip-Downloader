// 查词卡片：字幕层、视频摘要的文稿（悬停单词）和划词工具条的“释义”共用一个。
// 查词（清影 AI，带本句上下文）、卡片内容（词头、音标、等级、释义、本句意思、本句、例句）和“加入生词本”都在这里；
// 悬停弹出的浮窗（popover：放得下才放在单词上下，否则放左右，始终整个在屏幕里）、本视频里的别的句子、录原声也在这里，
// 字幕层和文稿用的是同一个浮窗。
;(() => {
  if (globalThis.ccWordCard) return
  const api = globalThis.browser ?? globalThis.chrome
  const send = (msg) => api.runtime.sendMessage(msg).catch((e) => ({ ok: false, error: e.message }))

  // 颜色走变量：放在哪个容器里由容器给（字幕层是固定深色，工具条跟随主题）
  const CSS = `
    .wcard { --wc-fg: var(--fg, #1d2420); --wc-head: var(--fg, #1d2420); --wc-mute: var(--mute, #5d6862); --wc-soft: var(--soft, #f3f1ec); --wc-acc: var(--acc, #b4561a); --wc-tag: var(--accsoft, #f6e6da); --wc-line: var(--line, #cfd5d1); --wc-ok: #2e8a52; flex-direction: column; gap: 6px; color: var(--wc-fg) }
    .wcard .hd { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; padding: 0; border: 0; background: none }
    .wcard .hd b { font-size: 17px; color: var(--wc-head) }
    .wcard .hd .sp { flex: 1 }
    .wcard .ph { color: var(--wc-mute); font-size: 12px }
    .wcard .lv { font-size: 11px; padding: 0 7px; border-radius: 9px; border: 1px solid var(--wc-acc); color: var(--wc-acc) }
    .wcard .ms { line-height: 1.55 }
    .wcard .ms i { font-style: normal; color: var(--wc-mute); margin-right: 6px }
    .wcard .lab { color: var(--wc-mute); font-size: 11.5px }
    .wcard .cx { padding: 6px 8px; border-radius: 8px; background: var(--wc-soft); font-size: 12.5px; line-height: 1.5 }
    .wcard .tip { color: var(--wc-mute); font-size: 12px; line-height: 1.5 }
    .wcard .quote, .wcard .ex { padding: 6px 8px; background: var(--wc-soft); border-radius: 0 8px 8px 0; font-size: 12.5px; line-height: 1.55 }
    .wcard .quote { border-left: 3px solid var(--wc-acc); margin: 0; color: var(--wc-fg) }
    .wcard .ex { border-left: 3px solid var(--wc-line) }
    .wcard .quote mark, .wcard .ex mark { background: transparent; color: var(--wc-acc); font-weight: 600 }
    .wcard .tag { float: right; font-size: 10.5px; padding: 0 6px; border-radius: 8px; background: var(--wc-tag); color: var(--wc-acc); margin-left: 6px }
    .wcard .ex .tr { color: var(--wc-mute); display: block; margin-top: 2px }
    .wcard .ex a { color: inherit; cursor: pointer; text-decoration: underline dotted }
    .wcard .heart { margin-left: 6px; border: 0; background: transparent; color: var(--wc-mute); font-size: 18px; line-height: 1; padding: 0 2px; cursor: pointer; font-weight: 400 }
    .wcard .heart.on { color: var(--wc-acc) }
    .wcard .row2 { display: flex; gap: 6px; margin-top: 2px; flex-wrap: wrap }
    .wcard .row2 button { white-space: nowrap; flex: none }
    .wcard .saved { color: var(--wc-ok); font-size: 12px }
  `

  const norm = (w) => String(w || '').trim().toLowerCase()

  // 同一个词在同一句话里只查一次（字幕层悬停来回移动不会重复请求）
  const cache = new Map()
  /** 查词：返回 { lemma, phonetic, pos, meaning, context_meaning, note, level, examples, offline }。 */
  // `trans`：这一句的译文（双语文稿里有）：清影会顺便指出单词在译文里对应的那几个字（aligned），用来高亮
  async function lookup(word, sentence = '', { lang = '', stop, title = document.title, trans = '' } = {}) {
    const key = `${norm(word)}|${String(sentence).slice(0, 80)}|${lang}|${String(trans).slice(0, 60)}`
    if (cache.has(key)) return cache.get(key)
    const res = await globalThis.ccUI.aiRun({ kind: 'lookup', text: word, context: sentence, title, ...(lang ? { lang } : {}), ...(trans ? { trans } : {}) }, { interval: 350, stop, timeoutMs: 30000 })
    cache.set(key, res)
    if (cache.size > 200) cache.delete(cache.keys().next().value)
    return res
  }

  /** 加入生词本。`at`：{ sentence, timeSec }。 */
  async function save(lemma, res, at = {}) {
    return send({
      type: 'cc-vocab',
      op: 'add',
      word: {
        word: lemma,
        phonetic: res.phonetic || '',
        pos: res.pos || '',
        meaning: res.meaning || '',
        ctxMeaning: res.context_meaning || '',
        sentence: at.sentence || '',
        sourceTitle: document.title,
        sourceUrl: location.href,
        timeSec: at.timeSec || 0,
      },
    })
  }

  /** 把一段文字里的这个词标出来。 */
  function markIn(h, txt, w) {
    const t = String(txt)
    const k = t.toLowerCase().indexOf(String(w).toLowerCase())
    return k < 0 ? [document.createTextNode(t)] : [document.createTextNode(t.slice(0, k)), h('mark', { text: t.slice(k, k + String(w).length) }), document.createTextNode(t.slice(k + String(w).length))]
  }

  /** 词头（查询中 / 出错时也用它）。 */
  const head = (h, word) => h('div', { class: 'hd' }, h('b', { text: word }))

  /**
   * 画卡片内容，返回节点数组（放进调用方自己的容器，容器加上 class="wcard"）。
   * opts: { h, word, sentence, res, saved, onSave(lemma, heartBtn), extraExamples: Node[], actions: Node[] }
   */
  function render({ h, word, sentence = '', res, saved = false, onSave, extraExamples = [], actions = [] }) {
    const lemma = res.lemma && norm(res.lemma) !== norm(word) ? res.lemma : word
    const parts = [h('div', { class: 'hd' }, h('b', { text: lemma }), res.phonetic ? h('span', { class: 'ph', text: res.phonetic }) : null, res.level ? h('span', { class: 'lv', text: res.level }) : null)]
    if (res.offline) {
      parts.push(h('div', { class: 'tip', text: '清影里没有可用的 AI 模型，只能显示词汇等级。到“设置 → AI”配置后就能看到释义和本句意思。' }))
    } else {
      if (res.meaning) parts.push(h('div', { class: 'ms' }, res.pos ? h('i', { text: res.pos }) : null, document.createTextNode(res.meaning)))
      if (res.context_meaning) parts.push(h('div', { class: 'lab', text: '在这句话里：' }), h('div', { class: 'cx', text: res.context_meaning }))
      if (res.note) parts.push(h('div', { class: 'tip', text: res.note }))
    }
    const status = h('div', { class: 'saved', text: saved ? '已加入生词本' : '' })
    const heart = h('button', {
      class: `heart${saved ? ' on' : ''}`,
      text: saved ? '♥' : '♡',
      title: saved ? '已在生词本' : '加入生词本',
      onclick: async () => {
        if (heart.classList.contains('on')) return
        const ok = await onSave?.(lemma, heart)
        if (ok === false) return
        heart.textContent = '♥'
        heart.classList.add('on')
        status.textContent = '已加入生词本'
      },
    })
    parts[0].append(h('span', { class: 'sp' }), heart)
    if (sentence) parts.push(h('div', { class: 'quote' }, h('span', { class: 'tag', text: '本句' }), document.createTextNode('“'), ...markIn(h, sentence, word), document.createTextNode('”')))
    parts.push(...extraExamples)
    for (const ex of (Array.isArray(res.examples) ? res.examples : []).slice(0, 2)) {
      if (!ex || !ex.text) continue
      parts.push(h('div', { class: 'ex' }, h('span', { class: 'tag', text: '例句' }), ...markIn(h, String(ex.text), lemma), ex.tr ? h('span', { class: 'tr', text: String(ex.tr) }) : null))
    }
    if (actions.length) parts.push(h('div', { class: 'row2' }, ...actions))
    parts.push(status)
    return { nodes: parts, lemma }
  }

  // ---------- 悬停浮窗 ----------

  // 浮窗容器的样式（固定深色，和字幕层一致）；第一次用到时放进调用方的界面容器
  const POP_CSS = `
    .wc-pop { position: fixed; display: none; width: 400px; max-width: calc(100vw - 16px); box-sizing: border-box; padding: 14px 16px; border-radius: 12px; border: 1px solid #2f383c; background: #22292c; color: #e7ecea; box-shadow: 0 10px 30px #0006; flex-direction: column; gap: 6px; overflow-y: auto; z-index: 2147483647 }
    .wc-pop.open { display: flex }
    .wc-pop.wcard { --wc-fg: #e7ecea; --wc-head: #fff; --wc-mute: #9aa6a2; --wc-soft: #14181a; --wc-acc: #f0a064; --wc-tag: #3a2c20; --wc-line: #4a5a60; --wc-ok: #6cc08a }
    .wc-pop .row2 button { height: 28px; padding: 0 10px; border-radius: 8px; border: 1px solid #2f383c; background: #14181a; color: #e7ecea; font-weight: 400; font-size: 12.5px; white-space: nowrap; flex: none; cursor: pointer }
    .wc-pop .row2 button.p { background: #f08a3c; border-color: #f08a3c; color: #14181a; font-weight: 600 }
  `

  /**
   * 把浮窗放在 `r`（单词的位置）旁边，整个放进屏幕里：
   * 上面放得下放上面，下面放得下放下面，都不行就放单词的右边或左边（上下居中、再挪进屏幕），
   * 哪儿都放不下就贴着屏幕上下边，里面滚动。以前总是往上放，浮窗高的时候顶部被屏幕裁掉。
   */
  function place(el, r, width = 400) {
    const m = 8
    const gap = 8
    const W = innerWidth
    const H = innerHeight
    const w = Math.min(width, W - 2 * m)
    el.style.width = `${w}px`
    el.style.maxHeight = `${H - 2 * m}px`
    el.style.bottom = 'auto'
    const h = Math.min(el.scrollHeight || el.offsetHeight || 200, H - 2 * m)
    const cx = Math.max(m, Math.min(W - w - m, r.left + r.width / 2 - w / 2))
    const clampY = (y) => Math.max(m, Math.min(H - h - m, y))
    let left
    let top
    if (r.top - gap - h >= m) {
      left = cx
      top = r.top - gap - h
    } else if (r.bottom + gap + h <= H - m) {
      left = cx
      top = r.bottom + gap
    } else if (W - r.right - gap - w >= m) {
      left = r.right + gap
      top = clampY(r.top + r.height / 2 - h / 2)
    } else if (r.left - gap - w >= m) {
      left = r.left - gap - w
      top = clampY(r.top + r.height / 2 - h / 2)
    } else {
      left = cx
      top = m
    }
    el.style.left = `${Math.round(left)}px`
    el.style.top = `${Math.round(top)}px`
    return { left, top, width: w, height: h }
  }

  /**
   * 一个悬停浮窗。`ui` 是调用方的界面容器（ccUI.mount 的返回值）。
   * open(anchorEl, spec)：spec = { id, word, sentence, lookup: { lang, trans, stop }, build(res) → { saved, onSave, extraExamples, actions }, onResult(res) }
   * 鼠标移进浮窗不关；移出 0.28 秒后关。onHide：关掉时调用（字幕层用来接着播放）。
   */
  function popover(ui, { onHide } = {}) {
    const h = globalThis.ccUI.h
    if (!ui.root.querySelector('style[data-wc-pop]')) {
      const st = document.createElement('style')
      st.setAttribute('data-wc-pop', '')
      st.textContent = CSS + POP_CSS
      ui.root.append(st)
    }
    const el = h('div', { class: 'wc-pop wcard', role: 'dialog' })
    ui.add(el)
    let forId = ''
    let timer = 0
    let anchor = null
    const isOpen = () => el.classList.contains('open')
    function close(now = false) {
      clearTimeout(timer)
      const go = () => {
        if (!isOpen() && !forId) return
        el.classList.remove('open')
        forId = ''
        anchor = null
        onHide?.()
      }
      if (now) go()
      else timer = setTimeout(go, 280)
    }
    el.addEventListener('mouseenter', () => clearTimeout(timer))
    el.addEventListener('mouseleave', () => close())
    const reposition = () => anchor?.isConnected && place(el, anchor.getBoundingClientRect())
    async function open(anchorEl, spec) {
      clearTimeout(timer)
      const id = spec.id || `${spec.word}|${spec.sentence}`
      if (forId === id && isOpen()) return
      forId = id
      anchor = anchorEl
      el.replaceChildren(head(h, spec.word), h('div', { class: 'tip', text: '查询中…' }))
      el.classList.add('open')
      reposition()
      let res
      try {
        res = await lookup(spec.word, spec.sentence, spec.lookup || {})
      } catch (e) {
        if (forId !== id) return
        el.replaceChildren(head(h, spec.word), h('div', { class: 'tip', text: e.message }))
        reposition()
        return
      }
      if (forId !== id) return
      const b = spec.build ? spec.build(res) : {}
      const { nodes } = render({ h, word: spec.word, sentence: spec.sentence, res, ...b })
      el.replaceChildren(...nodes)
      reposition()
      spec.onResult?.(res)
    }
    return { el, open, close, isOpen, reposition, current: () => forId }
  }

  /** 本视频里含这个词的别的句子（离当前句最近的两句），点时间跳过去。 */
  function lineExamples(h, lines, li, word, lemma, fmtTime, onJump) {
    const lw = String(lemma).toLowerCase()
    const ww = String(word).toLowerCase()
    const hit = (t) => String(t || '').toLowerCase().includes(lw) || String(t || '').toLowerCase().includes(ww)
    const others = []
    for (let d = 1; d < lines.length && others.length < 2; d++) {
      for (const j of [li + d, li - d]) {
        if (others.length < 2 && j >= 0 && j < lines.length && hit(lines[j].text)) others.push(j)
      }
    }
    return others.map((j) => {
      const txt = String(lines[j].text || '')
      return h('div', { class: 'ex' }, h('span', { class: 'tag', text: '本视频' }), ...markIn(h, txt, txt.toLowerCase().includes(lw) ? lemma : word), document.createTextNode(' '), h('a', { text: fmtTime(lines[j].t || 0), title: '跳到这句', onclick: () => onJump(j) }))
    })
  }

  /** 双语文稿里，单词在译文里对应的那几个字：清影给的 aligned 优先，没有就用释义里的词去译文里找。 */
  function alignedIn(trans, res) {
    const t = String(trans || '')
    if (!t) return ''
    const a = String(res?.aligned || '').trim()
    if (a && t.includes(a)) return a
    const parts = [res?.context_meaning, res?.meaning]
      .filter(Boolean)
      .flatMap((s) => String(s).split(/[；;，,、/（）()\s]+/))
      .map((s) => s.replace(/^(的|地|得)|(的|了)$/g, '').trim())
      .filter((s) => s.length >= 1 && s.length <= 12)
      .sort((a, b) => b.length - a.length)
    for (const p of parts) if (p.length >= 2 && t.includes(p)) return p
    for (const p of parts) if (t.includes(p)) return p
    return ''
  }

  // ---------- 录原声（导出 Anki 用）----------
  // 录下的原声存在清影桌面端（浏览器本地存储只有 10 MB，存不了几段）；这里只记“哪些词录过”
  const clipCache = {}
  let clipsLoaded = null
  const clipKey = (w) => String(w || '').trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '_').replace(/^_+|_+$/g, '').slice(0, 60)
  function loadClips() {
    clipsLoaded ||= (async () => {
      // 以前存在浏览器里的原声：搬到清影，搬完删掉
      const old = (await api.storage.local.get('vocabClips').catch(() => ({}))).vocabClips || {}
      const left = {}
      for (const [w, data] of Object.entries(old)) {
        const r = await send({ type: 'cc-vocab', op: 'clip', word: w, data })
        if (!r.ok) left[w] = data
      }
      if (Object.keys(old).length) await api.storage.local.set({ vocabClips: left }).catch(() => {})
      const r = await send({ type: 'cc-vocab', op: 'clips' })
      if (r.ok) for (const k of r.words || []) clipCache[k] = true
    })()
    return clipsLoaded
  }
  const hasClip = (w) => !!clipCache[clipKey(w)]
  /** 把这一句的原声录下来（播放一遍，用 captureStream 录；跨域或受保护的视频录不了）。span：{ start, end }（毫秒）。出错抛出。 */
  async function recordClip(video, word, span, btn) {
    const v = video
    if (!v || !span) throw new Error('页面上没有找到视频')
    if (typeof MediaRecorder === 'undefined' || !(v.captureStream || v.mozCaptureStream)) throw new Error('这个浏览器不支持录音')
    try {
      const stream = (v.captureStream || v.mozCaptureStream).call(v)
      const tracks = stream.getAudioTracks()
      if (!tracks.length) throw new Error('这个视频录不到声音（可能跨域或受保护）')
      const rec = new MediaRecorder(new MediaStream(tracks))
      const chunks = []
      rec.ondataavailable = (e) => e.data.size && chunks.push(e.data)
      const stopped = new Promise((ok) => (rec.onstop = ok))
      btn.textContent = '录制中…'
      const prevRate = v.playbackRate
      v.playbackRate = 1
      v.currentTime = Math.max(0, span.start / 1000)
      await v.play()
      rec.start()
      await new Promise((ok) => {
        const tm = setInterval(() => {
          if (v.currentTime * 1000 >= span.end || v.paused || v.ended) (clearInterval(tm), ok())
        }, 80)
      })
      rec.stop()
      v.pause()
      v.playbackRate = prevRate
      await stopped
      const blob = new Blob(chunks, { type: rec.mimeType || 'audio/webm' })
      if (blob.size > 900000) throw new Error('这一句太长了，没有保存')
      const data = await new Promise((ok, bad) => {
        const fr = new FileReader()
        fr.onload = () => ok(String(fr.result))
        fr.onerror = () => bad(new Error('读取录音失败'))
        fr.readAsDataURL(blob)
      })
      const saved = await send({ type: 'cc-vocab', op: 'clip', word, data })
      if (!saved.ok) throw new Error(`原声没有保存：${saved.error || '连不上清影'}（原声存在清影里，先把应用开着）`)
      clipCache[clipKey(word)] = true
      btn.textContent = '♪ 已录原声'
    } catch (e) {
      btn.textContent = '录原声（Anki）'
      throw e
    }
  }

  /** 朗读（英文按美音读）。 */
  function speak(t, lang = 'en-US') {
    try {
      const u = new SpeechSynthesisUtterance(t)
      u.lang = lang
      speechSynthesis.cancel()
      speechSynthesis.speak(u)
    } catch {
      // 浏览器不支持朗读
    }
  }

  globalThis.ccWordCard = { CSS, lookup, save, render, head, markIn, place, popover, lineExamples, alignedIn, loadClips, hasClip, recordClip, speak }
})()
