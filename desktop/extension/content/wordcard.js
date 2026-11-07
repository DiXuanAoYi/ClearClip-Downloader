// 查词卡片：字幕层的查词浮窗和划词工具条的“释义”共用一个。
// 查词（清影 AI，带本句上下文）、卡片内容（词头、音标、等级、释义、本句意思、本句、例句）和“加入生词本”都在这里；
// 用的地方只负责放在哪儿，以及加自己的东西（字幕层加“本视频里的别的句子”和跳转、原声按钮）。
;(() => {
  if (globalThis.ccWordCard) return
  const api = globalThis.browser ?? globalThis.chrome
  const send = (msg) => api.runtime.sendMessage(msg).catch((e) => ({ ok: false, error: e.message }))

  // 颜色走变量：放在哪个容器里由容器给（字幕层是固定深色，工具条跟随主题）
  const CSS = `
    .wcard { --wc-fg: var(--fg, #1d2420); --wc-head: var(--fg, #1d2420); --wc-mute: var(--mute, #5d6862); --wc-soft: var(--soft, #f3f1ec); --wc-acc: var(--acc, #b4561a); --wc-tag: var(--accsoft, #f6e6da); --wc-line: var(--line, #cfd5d1); --wc-ok: #2e8a52; flex-direction: column; gap: 6px; color: var(--wc-fg) }
    .wcard .hd { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap }
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
    .wcard .quote { border-left: 3px solid var(--wc-acc) }
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
  async function lookup(word, sentence = '', { lang = '', stop, title = document.title } = {}) {
    const key = `${norm(word)}|${String(sentence).slice(0, 80)}|${lang}`
    if (cache.has(key)) return cache.get(key)
    const res = await globalThis.ccUI.aiRun({ kind: 'lookup', text: word, context: sentence, title, ...(lang ? { lang } : {}) }, { interval: 350, stop, timeoutMs: 30000 })
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

  globalThis.ccWordCard = { CSS, lookup, save, render, head, markIn }
})()
