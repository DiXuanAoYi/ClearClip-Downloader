// 播放控制里不依赖页面的纯逻辑：倍速、快捷键、画面样式、截图文件名等。单元测试直接加载这个文件。
;(() => {
  const ACTIONS = [
    { id: 'slower', name: '减速', def: 's' },
    { id: 'faster', name: '加速', def: 'd' },
    { id: 'reset', name: '复位倍速', def: 'r' },
    { id: 'back', name: '后退', def: 'z' },
    { id: 'forward', name: '前进', def: 'x' },
    { id: 'frameBack', name: '上一帧', def: ',' },
    { id: 'frameForward', name: '下一帧', def: '.' },
    { id: 'shot', name: '截图', def: 'v' },
    { id: 'pip', name: '画中画', def: 'p' },
    { id: 'loopA', name: '标记 A 点', def: '[' },
    { id: 'loopB', name: '标记 B 点', def: ']' },
    { id: 'loopClear', name: '取消循环', def: '\\' },
    { id: 'mirror', name: '镜像', def: 'h' },
    { id: 'rotate', name: '旋转', def: 'j' },
  ]
  const DEFAULT_KEYS = Object.fromEntries(ACTIONS.map((a) => [a.id, a.def]))

  const DEFAULT_CFG = {
    keys: DEFAULT_KEYS,
    rateStep: 0.1,
    seekStep: 10,
    // 每个网站的跳过秒数：{ "example.com": 85 }
    skipIntro: {},
    skipOutro: {},
  }

  const num = (v, lo, hi, d) => {
    const n = Number(v)
    return Number.isFinite(n) && n >= lo && n <= hi ? n : d
  }
  const secMap = (m) => {
    const out = {}
    if (m && typeof m === 'object') for (const [k, v] of Object.entries(m)) if (k && Number(v) > 0 && Number(v) <= 3600) out[String(k).toLowerCase()] = Math.round(Number(v))
    return out
  }

  /** 存储里的原始设置 → 完整、合法的设置：缺的补默认，重复或无效的快捷键回到默认。 */
  function normalizeCfg(raw) {
    const r = raw && typeof raw === 'object' ? raw : {}
    const keys = { ...DEFAULT_KEYS }
    if (r.keys && typeof r.keys === 'object') {
      for (const a of ACTIONS) {
        const k = r.keys[a.id]
        if (typeof k === 'string' && k.length >= 1 && k.length <= 12) keys[a.id] = k.length === 1 ? k.toLowerCase() : k
      }
    }
    return { keys, rateStep: num(r.rateStep, 0.01, 2, 0.1), seekStep: num(r.seekStep, 1, 600, 10), skipIntro: secMap(r.skipIntro), skipOutro: secMap(r.skipOutro) }
  }

  /** 快捷键重复的动作：{ 键: [动作编号…] }，只列出有冲突的。 */
  function findConflicts(keys) {
    const by = {}
    for (const [id, k] of Object.entries(keys || {})) (by[k] ||= []).push(id)
    return Object.fromEntries(Object.entries(by).filter(([, v]) => v.length > 1))
  }

  /** 按下的键对应哪个动作；带 Ctrl / Alt / Meta 的组合键不算。 */
  function matchKey(e, keys) {
    if (e.ctrlKey || e.metaKey || e.altKey) return null
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key
    for (const [id, key] of Object.entries(keys)) if (key === k) return id
    return null
  }

  const clampRate = (r) => Math.round(Math.min(16, Math.max(0.1, Number.isFinite(Number(r)) && r !== '' && r != null ? Number(r) : 1)) * 100) / 100
  const stepRate = (cur, dir, step) => clampRate(Math.round((cur + dir * step) * 100) / 100)
  const fmtRate = (r) => `${Number(r.toFixed(2))}×`

  /** 画面调整 → CSS。 */
  function filterCss({ brightness = 100, contrast = 100, saturate = 100 } = {}) {
    const parts = []
    if (brightness !== 100) parts.push(`brightness(${brightness / 100})`)
    if (contrast !== 100) parts.push(`contrast(${contrast / 100})`)
    if (saturate !== 100) parts.push(`saturate(${saturate / 100})`)
    return parts.join(' ')
  }

  function transformCss({ mirror = false, rotate = 0 } = {}) {
    const parts = []
    if (rotate) parts.push(`rotate(${rotate}deg)`)
    if (mirror) parts.push('scaleX(-1)')
    return parts.join(' ')
  }

  /** 跳过片头：当前位置还在片头里就返回应跳到的秒数，否则 null。 */
  function skipIntroTarget(currentTime, seconds, duration) {
    if (!(seconds > 0) || !isFinite(duration) || duration <= seconds + 30) return null
    return currentTime < seconds - 1 ? seconds : null
  }

  /** 跳过片尾：剩余时间已进入片尾就返回 true。 */
  function inOutro(currentTime, seconds, duration) {
    return seconds > 0 && isFinite(duration) && duration > seconds + 60 && duration - currentTime <= seconds
  }

  /** 跨域且没有 CORS 的媒体：用 Web Audio 放大音量会变成没有声音。blob: / MSE 播放不算。 */
  function audioGraphUnsafe(src, pageOrigin, crossOrigin) {
    if (!src || /^(blob|data):/i.test(src)) return false
    try {
      return new URL(src).origin !== pageOrigin && crossOrigin == null
    } catch {
      return false
    }
  }

  const pad = (n) => String(n).padStart(2, '0')
  function fmtTime(sec) {
    const s = Math.max(0, Math.floor(sec))
    return `${pad(Math.floor(s / 3600))}-${pad(Math.floor((s % 3600) / 60))}-${pad(s % 60)}`
  }

  /** 截图文件名：标题-时:分:秒.扩展名，去掉文件名里不能用的字符。 */
  function shotName(title, sec, ext) {
    const base = String(title || 'video').replace(/[\\/:*?"<>|\s]+/g, ' ').trim().slice(0, 60) || 'video'
    return `${base}-${fmtTime(sec)}.${ext}`
  }

  globalThis.ccVideo = { ACTIONS, DEFAULT_KEYS, DEFAULT_CFG, normalizeCfg, findConflicts, matchKey, clampRate, stepRate, fmtRate, filterCss, transformCss, skipIntroTarget, inOutro, audioGraphUnsafe, fmtTime, shotName }
})()
