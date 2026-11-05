// 实时音效的纯逻辑：预设、设置整理、响度测量（BS.1770 / EBU R128 思路）、增益控制、混响脉冲响应、能不能处理的判断。
// 不碰任何浏览器接口，可以在 Node 里单元测试；内容脚本和清影内置播放器共用这一份。
;(() => {
  if (globalThis.ccAudio) return

  const clamp = (v, lo, hi, d) => (Number.isFinite(Number(v)) ? Math.min(hi, Math.max(lo, Number(v))) : d)

  // ---------- 预设 ----------

  /** 响度预设：目标响度（LUFS）、压缩强度（0–100）、最大增益（dB）。night 另外压低低频。 */
  const LOUD_PRESETS = [
    { id: 'mild', name: '温和', target: -16, comp: 30, maxGain: 12, note: '短视频、网课，保留一点动态' },
    { id: 'standard', name: '标准', target: -14, comp: 40, maxGain: 12, note: '接近 YouTube 和音乐平台，更响更满' },
    { id: 'quiet', name: '安静', target: -18, comp: 35, maxGain: 10, note: '办公室、图书馆' },
    { id: 'night', name: '夜间', target: -20, comp: 70, maxGain: 12, lowCut: true, note: '压低爆炸声、抬高对白，同时压低低频' },
    { id: 'broadcast', name: '广播', target: -23, comp: 20, maxGain: 8, note: '电视、播客的 EBU R128 标准' },
  ]

  /** 混响预设，参数和工具箱“音效 / 混响”（audiofx.rs）一致：混合 dB、混响时间 ms、高频保留、预延迟 ms。 */
  const REVERBS = [
    { id: 'room', name: '房间', mix: -9, time: 500, hf: 0.7, pre: 8 },
    { id: 'bath', name: '浴室', mix: -5, time: 1400, hf: 0.9, pre: 5 },
    { id: 'living', name: '客厅', mix: -10, time: 700, hf: 0.55, pre: 12 },
    { id: 'class', name: '教室', mix: -8, time: 1100, hf: 0.65, pre: 18 },
    { id: 'studio', name: '录音棚', mix: -14, time: 350, hf: 0.5, pre: 4 },
    { id: 'plate', name: '板式', mix: -7, time: 1800, hf: 0.85, pre: 20 },
    { id: 'hall', name: '音乐厅', mix: -8, time: 2600, hf: 0.6, pre: 30 },
    { id: 'church', name: '教堂', mix: -6, time: 4500, hf: 0.5, pre: 45 },
  ]

  /** 均衡器预设：一串滤波器。voice 和 audiofx.rs 的“人声增强”同一组参数。 */
  const EQS = [
    { id: 'voice', name: '人声增强', bands: [{ t: 'highpass', f: 80, q: 0.7 }, { t: 'peaking', f: 250, q: 1, g: -2 }, { t: 'peaking', f: 3200, q: 1.2, g: 2.5 }] },
    { id: 'bass', name: '低音增强', bands: [{ t: 'lowshelf', f: 110, g: 5 }] },
    { id: 'treble', name: '高音增强', bands: [{ t: 'highshelf', f: 6000, g: 4 }] },
    { id: 'night', name: '压低低频', bands: [{ t: 'lowshelf', f: 140, g: -5 }] },
  ]

  const SPEEDS = { slow: 3.0, mid: 1.5, fast: 0.6 }

  const DEFAULT_CFG = {
    on: true, // 响度标准化
    preset: 'mild', // LOUD_PRESETS 的 id，或 custom
    target: -16,
    comp: 30,
    maxGain: 12,
    speed: 'mid',
    limiter: true,
    reverb: { on: false, id: 'room', mix: null }, // mix 为 null 时用预设值
    eq: { on: false, id: 'voice' },
    notifyOff: false, // 不再提示“无法处理”
    pillShow: false, // 视频上的“音效 -16 LUFS”小按钮；新安装默认不显示
    pillHover: true, // 显示时只在鼠标移到视频上才出现
  }

  /** 把存储里的原始值整理成完整设置：缺的补默认，越界的夹回范围，不认识的预设退回自定义。 */
  function normalizeCfg(raw) {
    const r = raw && typeof raw === 'object' ? raw : {}
    const d = DEFAULT_CFG
    const p = LOUD_PRESETS.find((x) => x.id === r.preset)
    const preset = p ? p.id : r.preset === 'custom' ? 'custom' : d.preset
    const base = p || (preset === 'custom' ? d : LOUD_PRESETS[0])
    const num = (k, lo, hi) => clamp(r[k], lo, hi, base[k])
    const rv = r.reverb && typeof r.reverb === 'object' ? r.reverb : {}
    const eq = r.eq && typeof r.eq === 'object' ? r.eq : {}
    return {
      on: typeof r.on === 'boolean' ? r.on : d.on,
      preset,
      // 选了预设就以预设为准（除非是自定义），避免旧数值和预设对不上
      target: preset === 'custom' ? num('target', -30, -8) : base.target,
      comp: preset === 'custom' ? num('comp', 0, 100) : base.comp,
      maxGain: preset === 'custom' ? num('maxGain', 0, 24) : base.maxGain,
      speed: SPEEDS[r.speed] ? r.speed : d.speed,
      limiter: typeof r.limiter === 'boolean' ? r.limiter : d.limiter,
      reverb: {
        on: rv.on === true,
        id: REVERBS.some((x) => x.id === rv.id) ? rv.id : d.reverb.id,
        mix: rv.mix == null ? null : clamp(rv.mix, -30, 0, null),
      },
      eq: { on: eq.on === true, id: EQS.some((x) => x.id === eq.id) ? eq.id : d.eq.id },
      notifyOff: r.notifyOff === true,
      pillShow: typeof r.pillShow === 'boolean' ? r.pillShow : d.pillShow,
      pillHover: typeof r.pillHover === 'boolean' ? r.pillHover : d.pillHover,
    }
  }

  /** 选一个预设。custom 保留当前数值。 */
  function withPreset(cfg, id) {
    const p = LOUD_PRESETS.find((x) => x.id === id)
    if (!p) return normalizeCfg({ ...cfg, preset: 'custom' })
    return normalizeCfg({ ...cfg, preset: id })
  }

  /** 改了目标 / 压缩 / 最大增益中的任一项，就变成自定义；数值恰好等于某个预设时仍算自定义（用户自己调的）。 */
  function withCustom(cfg, patch) {
    return normalizeCfg({ ...cfg, target: cfg.target, comp: cfg.comp, maxGain: cfg.maxGain, ...patch, preset: 'custom' })
  }

  // ---------- 响度测量 ----------

  /** 一段样本的均方值。 */
  function meanSquare(buf, n = buf.length) {
    const len = Math.min(n, buf.length)
    if (len <= 0) return 0
    let s = 0
    for (let i = buf.length - len; i < buf.length; i++) s += buf[i] * buf[i]
    return s / len
  }

  const toLufs = (ms) => (ms > 0 ? -0.691 + 10 * Math.log10(ms) : -Infinity)

  /**
   * 短时响度：最近 3 秒、每 100 毫秒一块的 K 计权均方值。
   * 低于 -70 LUFS 的块当静音丢掉（绝对门限），再丢掉比平均低 10 LU 的块（相对门限），所以讲话的停顿不会把响度拉低。
   */
  class ShortTerm {
    constructor(blocks = 30) {
      this.max = blocks
      this.ring = []
    }
    push(ms) {
      this.ring.push(Math.max(0, ms || 0))
      if (this.ring.length > this.max) this.ring.shift()
    }
    reset() {
      this.ring = []
    }
    /** { lufs, used }：lufs 为 null 表示有效的块太少（不足 400 毫秒），不要据此调整增益。 */
    value() {
      const abs = this.ring.filter((m) => toLufs(m) > -70)
      if (abs.length < 4) return { lufs: null, used: abs.length }
      const mean = abs.reduce((a, b) => a + b, 0) / abs.length
      const rel = abs.filter((m) => toLufs(m) > toLufs(mean) - 10)
      const use = rel.length ? rel : abs
      return { lufs: toLufs(use.reduce((a, b) => a + b, 0) / use.length), used: use.length }
    }
  }

  /**
   * 下一步的增益：返回 { target, tc }。
   * 目标 = 目标响度 - 测到的响度，夹在 [-18, 最大增益] dB；变化小于 0.5 dB 就不动，免得来回抖。
   * 需要降低（突然变响）时反应快三倍；换视频后的头几秒（fast）先用很短的时间常数拉到位，再放慢。
   */
  function nextGain({ cur, lufs, cfg, fast = false }) {
    if (lufs == null || !Number.isFinite(lufs)) return { target: cur, tc: 1, hold: true }
    const want = Math.min(cfg.maxGain, Math.max(-18, cfg.target - lufs))
    if (Math.abs(want - cur) < 0.5) return { target: cur, tc: 1, hold: true }
    const base = SPEEDS[cfg.speed] || SPEEDS.mid
    const tc = fast ? 0.25 : want < cur ? base / 3 : base
    return { target: want, tc, hold: false }
  }

  /** 压缩强度 0–100 对应的动态压缩器参数；0 = 比例 1:1，相当于不压缩。 */
  function compParams(comp) {
    const c = clamp(comp, 0, 100, 30)
    return { threshold: -12 - 0.24 * c, ratio: 1 + c * 0.05, knee: 12, attack: 0.01, release: 0.25 }
  }

  /** 末级限幅器：只防拉高音量后爆音。 */
  const LIMITER = { threshold: -1.5, ratio: 20, knee: 0, attack: 0.002, release: 0.08 }

  /**
   * DynamicsCompressorNode 自带一个关不掉的“补偿增益”（Chromium / WebKit 按压缩曲线在 0 dB 处的衰减算的，再取 0.6 次方）。
   * 不抵消的话，压缩一开声音就变大几 dB，响度控制就不准了。这里照它的算法算出来，后面接一个反向增益。返回线性倍数。
   */
  function compressorMakeup({ threshold, knee, ratio }) {
    if (!(ratio > 1.0001)) return 1
    const d2l = (d) => Math.pow(10, d / 20)
    const l2d = (x) => 20 * Math.log10(x)
    const lt = d2l(threshold)
    const kneeCurve = (x, k) => (x < lt ? x : lt + (1 - Math.exp(-k * (x - lt))) / k)
    const slopeAt = (x, k) => {
      if (x < lt) return 1
      const x2 = x * 1.001
      return (l2d(kneeCurve(x2, k)) - l2d(kneeCurve(x, k))) / (l2d(x2) - l2d(x))
    }
    const x0 = d2l(threshold + knee)
    let lo = 0.1
    let hi = 10000
    let k = 5
    for (let i = 0; i < 15; i++) {
      if (slopeAt(x0, k) < 1 / ratio) hi = k
      else lo = k
      k = Math.sqrt(lo * hi)
    }
    const kneeDb = threshold + knee
    const yKneeDb = l2d(kneeCurve(d2l(kneeDb), k))
    const full = 1 < d2l(kneeDb) ? kneeCurve(1, k) : d2l(yKneeDb + (l2d(1) - kneeDb) / ratio)
    return Math.pow(1 / full, 0.6)
  }

  const dbToGain = (db) => Math.pow(10, db / 20)

  // K 计权（BS.1770）第一级：高频搁架 4 dB / 1681.97 Hz；第二级：38.135 Hz 高通。Web Audio 里高通的 Q 以 dB 计。
  const K_SHELF = { type: 'highshelf', frequency: 1681.97, gain: 4.0 }
  const K_HP = { type: 'highpass', frequency: 38.135, Q: 20 * Math.log10(0.5003) }

  // ---------- 混响脉冲响应 ----------

  function rng(seed) {
    let a = seed >>> 0 || 1
    return () => {
      a = (a + 0x6d2b79f5) >>> 0
      let t = Math.imul(a ^ (a >>> 15), 1 | a)
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
      return (((t ^ (t >>> 14)) >>> 0) / 4294967296) * 2 - 1
    }
  }

  /**
   * 按参数合成立体声脉冲响应，不需要任何音频文件。做法和 audiofx.rs 一样：左右各一路白噪声，
   * 以 1.8 kHz 分成低频和高频，各乘指数衰减包络（低频 -60 dB 的时间 = 混响时间，高频是它乘“高频保留”），前面加预延迟，
   * 最后把每路能量归一到 1，这样混响混合 0 dB 时湿声和干声大致一样响。
   * 返回 { sampleRate, left, right, length }。
   */
  function makeImpulse(sampleRate, r, seed = 1) {
    const sr = Math.max(8000, Math.round(sampleRate) || 48000)
    const time = clamp(r.time, 100, 10000, 800)
    const hf = clamp(r.hf, 0.1, 1, 0.7)
    const pre = Math.round((clamp(r.pre, 0, 300, 15) / 1000) * sr)
    const body = Math.round((time / 1000) * sr)
    const len = Math.min(pre + body, sr * 10)
    const out = []
    for (let ch = 0; ch < 2; ch++) {
      const rand = rng(seed * 2 + ch + 17)
      const buf = new Float32Array(len)
      const a = 1 - Math.exp((-2 * Math.PI * 1800) / sr)
      let l1 = 0
      let l2 = 0
      const rtHi = (time / 1000) * hf
      const rtLo = time / 1000
      for (let i = pre; i < len; i++) {
        const t = (i - pre) / sr
        const x = rand()
        l1 += a * (x - l1)
        l2 += a * (l1 - l2)
        const lo = l2
        const hi = x - l2
        buf[i] = lo * Math.pow(10, (-3 * t) / rtLo) + hi * Math.pow(10, (-3 * t) / rtHi)
      }
      const fade = Math.min(Math.round(0.004 * sr), len - pre)
      for (let i = 0; i < fade; i++) buf[pre + i] *= i / fade
      let e = 0
      for (let i = 0; i < len; i++) e += buf[i] * buf[i]
      const k = e > 0 ? 1 / Math.sqrt(e) : 0
      for (let i = 0; i < len; i++) buf[i] *= k
      out.push(buf)
    }
    return { sampleRate: sr, left: out[0], right: out[1], length: len }
  }

  /** 混响的混合量（dB）：用户改过就用用户的，否则用预设。 */
  function reverbSpec(cfg) {
    const p = REVERBS.find((x) => x.id === cfg.reverb.id) || REVERBS[0]
    return { ...p, mix: cfg.reverb.mix == null ? p.mix : cfg.reverb.mix }
  }

  // ---------- 能不能处理 ----------

  /**
   * 判断这个媒体元素能不能接进 Web Audio。接进去之后无法撤回，所以不确定的先跳过：
   * 受版权保护（EME）的、跨域直链又没有 CORS 的，接了会变成没有声音。blob: / MSE 播放（大多数网站）没问题。
   * 返回 null 表示可以；否则 { code, text }。
   */
  function unprocessable({ src, pageOrigin, crossOrigin, hasMediaKeys }) {
    if (hasMediaKeys) return { code: 'drm', text: '这个视频受版权保护（DRM），浏览器不允许改它的声音，已按原样播放。' }
    if (!src || /^(blob|data|mediasource):/i.test(src)) return null
    try {
      const u = new URL(src)
      if (u.origin !== pageOrigin && crossOrigin == null) {
        return { code: 'cross', text: `视频文件来自另一个网站（${u.hostname}），浏览器不允许改它的声音，已按原样播放。` }
      }
    } catch {
      // 地址读不出来，当作可以处理
    }
    return null
  }

  /** 小面板 / 弹窗里显示的一句状态。 */
  function statusText({ off, siteOff, blocked, active, count }) {
    if (siteOff) return '本站已关闭，声音保持原样'
    if (off) return '已关闭，声音保持原样'
    if (blocked && !active) return `本页 ${blocked} 个视频无法处理`
    if (active) return `正在处理本页的 ${active} 个视频${blocked ? `，另有 ${blocked} 个无法处理` : ''}`
    return count ? '等待播放' : '本页没有视频'
  }

  const fmtLufs = (v) => (v == null || !Number.isFinite(v) ? '--' : v.toFixed(1))
  const fmtDb = (v) => (v == null || !Number.isFinite(v) ? '--' : `${v >= 0 ? '+' : ''}${v.toFixed(1)} dB`)

  globalThis.ccAudio = {
    LOUD_PRESETS, REVERBS, EQS, SPEEDS, DEFAULT_CFG, K_SHELF, K_HP, LIMITER,
    normalizeCfg, withPreset, withCustom, meanSquare, toLufs, ShortTerm, nextGain, compParams, compressorMakeup, dbToGain,
    makeImpulse, reverbSpec, unprocessable, statusText, fmtLufs, fmtDb,
  }
})()
