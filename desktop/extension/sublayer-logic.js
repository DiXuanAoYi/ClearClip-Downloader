// 页面字幕层的纯逻辑：当前该显示哪一句、双语排版、延迟、导出 SRT、偏好整理。不碰网页和浏览器接口，可以单元测试。
// 内容脚本里挂在 globalThis.ccSubLayer 上。字幕行的格式和视频摘要一致：{ t: 开始毫秒, text }。
;(() => {
  if (globalThis.ccSubLayer) return

  /** 显示方式：原文在上 / 译文在上 / 只原文 / 只译文。 */
  const MODES = ['orig-top', 'trans-top', 'orig', 'trans']
  const MODE_NAMES = { 'orig-top': '双语（原文在上）', 'trans-top': '双语（译文在上）', orig: '只显示原文', trans: '只显示译文' }

  const DEFAULTS = { mode: 'orig-top', size: 80, bottom: 10, delay: 0, target: '' }

  const num = (v, d) => (Number.isFinite(Number(v)) && v !== '' && v !== null ? Number(v) : d)
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

  /** 偏好整理：字号 50%–300%，离底部 0%–60%，延迟 ±60 秒。 */
  function clampPrefs(p) {
    let r = p && typeof p === 'object' ? p : {}
    // 1.8.1 以前的默认是字号 100%、距底 8%：没改过的老设置换成新默认（改过的保留）
    if (!r.v && Number(r.size) === 100 && Number(r.bottom) === 8) r = { ...r, size: DEFAULTS.size, bottom: DEFAULTS.bottom }
    return {
      v: 2,
      mode: MODES.includes(r.mode) ? r.mode : DEFAULTS.mode,
      size: Math.round(clamp(num(r.size, DEFAULTS.size), 50, 300)),
      bottom: Math.round(clamp(num(r.bottom, DEFAULTS.bottom), 0, 60)),
      delay: Math.round(clamp(num(r.delay, DEFAULTS.delay), -60000, 60000)),
      target: typeof r.target === 'string' ? r.target.slice(0, 40) : '',
    }
  }

  const nextMode = (m) => MODES[(MODES.indexOf(m) + 1) % MODES.length]

  /** 第 i 句显示到什么时候：下一句开始，但最长 8 秒（避免一句字幕挂很久）。 */
  function endOf(lines, i, cap = 8000) {
    const l = lines[i]
    const next = lines[i + 1]
    const end = next ? Math.min(next.t, l.t + cap) : l.t + 4000
    return Math.max(end, l.t + 500)
  }

  /** 播放到 `ms` 毫秒时该显示第几句；没有返回 -1。`delay` 为正 = 字幕晚出现。 */
  function indexAt(lines, ms, delay = 0) {
    const t = ms - delay
    let lo = 0
    let hi = (lines || []).length - 1
    let ans = -1
    while (lo <= hi) {
      const mid = (lo + hi) >> 1
      if (lines[mid].t <= t) {
        ans = mid
        lo = mid + 1
      } else hi = mid - 1
    }
    return ans >= 0 && t < endOf(lines, ans) ? ans : -1
  }

  /** 按显示方式排出要显示的行（从上到下）。某句没有译文时显示原文，不会留空。 */
  function compose(orig, trans, mode) {
    const o = String(orig || '').trim()
    const t = String(trans || '').trim()
    if (!t || mode === 'orig') return o ? [o] : []
    if (mode === 'trans') return [t]
    if (t === o) return [o]
    return mode === 'trans-top' ? [t, o] : [o, t]
  }

  /** 还没有译文的行号。 */
  function missing(lines, trans) {
    const out = []
    for (let i = 0; i < (lines || []).length; i++) if (!String(trans?.[i] || '').trim()) out.push(i)
    return out
  }

  function chunk(arr, n) {
    const out = []
    for (let i = 0; i < arr.length; i += n) out.push(arr.slice(i, i + n))
    return out
  }

  function srtTime(ms) {
    const t = Math.max(0, Math.round(ms))
    const p = (n, w = 2) => String(n).padStart(w, '0')
    return `${p(Math.floor(t / 3600000))}:${p(Math.floor(t / 60000) % 60)}:${p(Math.floor(t / 1000) % 60)},${p(t % 1000, 3)}`
  }

  /** 导出 SRT：按当前的显示方式和延迟（所见即所得）。 */
  function toSrt(lines, trans, mode, delay = 0) {
    const blocks = []
    ;(lines || []).forEach((l, i) => {
      const rows = compose(l.text, trans?.[i], mode)
      if (!rows.length) return
      const a = l.t + delay
      const b = endOf(lines, i) + delay
      blocks.push(`${blocks.length + 1}\n${srtTime(a)} --> ${srtTime(Math.max(b, a + 500))}\n${rows.join('\n')}\n`)
    })
    return blocks.join('\n')
  }

  /** 字幕层缓存用的键：同一个视频 + 目标语言 + 译文用的提示词版本。 */
  const cacheKey = (videoKey, target, pv = '') => `${videoKey}|${target}|${pv}`

  globalThis.ccSubLayer = { MODES, MODE_NAMES, DEFAULTS, clampPrefs, nextMode, endOf, indexAt, compose, missing, chunk, srtTime, toSrt, cacheKey }
})()
