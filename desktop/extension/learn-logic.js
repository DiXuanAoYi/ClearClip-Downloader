// 字幕学习的纯逻辑：把字幕拆成单词、找出可能不熟的词、精听时的听写对照、句子起止。不碰网页和浏览器接口，可以单元测试。
// 内容脚本里挂在 globalThis.ccLearn 上。字幕行的格式：{ t: 开始毫秒, text }。
;(() => {
  if (globalThis.ccLearn) return

  /** 带字母的“词”：英文、法文、德文等拉丁字母（含带音标的字母），允许中间的 ' ’ -。 */
  const WORD = /[\p{Script=Latin}][\p{Script=Latin}'’-]*/gu

  /** 把一句字幕拆成 [{ text, word }]：word 为 true 的片段是词，其余是标点和空白，拼起来等于原句。 */
  function segment(text) {
    const s = String(text || '')
    const out = []
    let last = 0
    for (const m of s.matchAll(WORD)) {
      if (m.index > last) out.push({ text: s.slice(last, m.index), word: false })
      out.push({ text: m[0], word: true })
      last = m.index + m[0].length
    }
    if (last < s.length) out.push({ text: s.slice(last), word: false })
    return out
  }

  /** 词的统一写法：小写，去掉首尾的撇号和连字符。 */
  const norm = (w) =>
    String(w || '')
      .toLowerCase()
      .replace(/’/g, "'")
      .replace(/^['-]+|['-]+$/g, '')

  /** 一批字幕里出现过的不同的词（≥3 个字母），按出现顺序。 */
  function uniqueWords(lines, max = 1500) {
    const seen = new Set()
    const out = []
    for (const l of lines || []) {
      for (const p of segment(l.text)) {
        if (!p.word) continue
        const w = norm(p.text)
        if (w.length < 3 || seen.has(w)) continue
        seen.add(w)
        out.push(w)
        if (out.length >= max) return out
      }
    }
    return out
  }

  /** 分批，给“这些词里哪些不熟”的请求用。 */
  function chunk(list, n) {
    const out = []
    for (let i = 0; i < list.length; i += n) out.push(list.slice(i, i + n))
    return out
  }

  /** “可能不熟”的门槛：词频名次超过多少算不熟。0 = 不标记。 */
  const LEVELS = [
    { id: 0, name: '不标记', rank: 0 },
    { id: 1, name: '只标生僻词', rank: 12000 },
    { id: 2, name: '标进阶以上', rank: 6000 },
    { id: 3, name: '标常用以外', rank: 2000 },
  ]
  const levelRank = (id) => (LEVELS.find((l) => l.id === Number(id)) || LEVELS[0]).rank

  /** 这一句的起止（毫秒）：到下一句开始，但最长 8 秒；`delay` 为字幕延迟。 */
  function span(lines, i, delay = 0, cap = 8000) {
    const l = lines[i]
    if (!l) return null
    const next = lines[i + 1]
    const start = l.t + delay
    const end = (next ? Math.min(next.t, l.t + cap) : l.t + 4000) + delay
    return { start, end: Math.max(end, start + 500) }
  }

  /**
   * 听写对照：把听写的内容和原句按词做最长公共子序列比对。
   * 返回 { parts: [{ text, ok }]（原句的每个词是否写对，标点原样保留）, missed: [词]（没写出来的词）, score: 0–100 }。
   */
  function dictationDiff(ref, typed) {
    const seg = segment(ref)
    const refWords = seg.filter((p) => p.word).map((p) => norm(p.text))
    const typedWords = segment(typed)
      .filter((p) => p.word)
      .map((p) => norm(p.text))
    const n = refWords.length
    const m = typedWords.length
    const dp = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0))
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = refWords[i] === typedWords[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    const ok = new Array(n).fill(false)
    for (let i = 0, j = 0; i < n && j < m; ) {
      if (refWords[i] === typedWords[j]) {
        ok[i] = true
        i++
        j++
      } else if (dp[i + 1][j] >= dp[i][j + 1]) i++
      else j++
    }
    let k = 0
    const parts = seg.map((p) => (p.word ? { text: p.text, ok: ok[k++] } : { text: p.text, ok: true }))
    const missed = refWords.filter((_, i) => !ok[i])
    return { parts, missed, score: n ? Math.round((ok.filter(Boolean).length / n) * 100) : 0 }
  }

  /** 精听时“这一句播到哪了”的判断：到句末要不要暂停 / 循环。返回 'play' | 'again' | 'stop'。 */
  function listenStep(nowMs, sp, done, loops) {
    if (!sp || nowMs < sp.end - 60) return 'play'
    if (loops === 0 || done + 1 < loops) return 'again'
    return 'stop'
  }

  const fmtTime = (ms) => {
    const s = Math.max(0, Math.floor(ms / 1000))
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  }

  globalThis.ccLearn = { segment, norm, uniqueWords, chunk, LEVELS, levelRank, span, dictationDiff, listenStep, fmtTime }
})()
