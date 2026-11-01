// 实时音效的处理链（Web Audio 部分）：浏览器扩展的内容脚本和清影内置播放器共用这一份。
//   声音 → 均衡器 → 混响 → [响度测量] → 增益 → 压缩 → 限幅 → 音量放大 → 输出
// 用法：const c = new ccAudio.Chain(ac, mediaEl)；c.wire({ cfg, bypass, measure })；播放时每 100 毫秒 c.tick()。
// 一个媒体元素只能 createMediaElementSource 一次，且接进去无法撤回，调用前要先判断能不能处理（ccAudio.unprocessable）。
;(() => {
  const A = globalThis.ccAudio
  if (!A || A.Chain) return

  const irCache = new Map()
  function irBuffer(ac, spec) {
    const key = `${spec.id}|${ac.sampleRate}`
    let b = irCache.get(key)
    if (!b) {
      const ir = A.makeImpulse(ac.sampleRate, spec)
      b = ac.createBuffer(2, ir.length, ir.sampleRate)
      b.copyToChannel(ir.left, 0)
      b.copyToChannel(ir.right, 1)
      irCache.set(key, b)
    }
    return b
  }

  class Chain {
    constructor(ac, el, boost = 1) {
      this.ac = ac
      this.el = el
      this.src = ac.createMediaElementSource(el)
      this.kShelf = new BiquadFilterNode(ac, A.K_SHELF)
      this.kHp = new BiquadFilterNode(ac, A.K_HP)
      this.split = ac.createChannelSplitter(2)
      this.split.channelInterpretation = 'discrete' // 单声道只进左路，和 BS.1770 一样不重复计算
      this.anL = ac.createAnalyser()
      this.anR = ac.createAnalyser()
      this.anL.fftSize = this.anR.fftSize = 8192
      this.anL.smoothingTimeConstant = this.anR.smoothingTimeConstant = 0
      this.loud = ac.createGain()
      this.comp = ac.createDynamicsCompressor()
      this.lim = ac.createDynamicsCompressor()
      this.compFix = ac.createGain() // 抵消压缩器自带的补偿增益
      this.limFix = ac.createGain()
      this.boost = ac.createGain()
      this.dry = ac.createGain()
      this.wet = ac.createGain()
      this.sum = ac.createGain()
      this.eqNodes = []
      this.conv = null
      this.st = new A.ShortTerm()
      this.bufL = new Float32Array(this.anL.fftSize)
      this.bufR = new Float32Array(this.anR.fftSize)
      this.tgtDb = 0
      this.curDb = 0
      this.tc = 1
      this.last = performance.now()
      this.fastUntil = performance.now() + 3000
      this.lufs = null
      this.measuring = false
      this.cfg = A.normalizeCfg()
      this.bypass = false
      for (const [k, v] of Object.entries(A.LIMITER)) this.lim[k].value = v
      this.limFix.gain.value = 1 / A.compressorMakeup(A.LIMITER)
      this.boost.gain.value = boost
    }

    disconnectAll() {
      const nodes = [this.src, this.kShelf, this.kHp, this.split, this.loud, this.comp, this.lim, this.compFix, this.limFix, this.boost, this.dry, this.wet, this.sum, this.conv, ...this.eqNodes]
      for (const n of nodes) {
        try {
          n?.disconnect()
        } catch {
          // 本来就没连
        }
      }
    }

    /** 按设置重新连线。bypass = 直通（只留音量放大）；measure = 即使没开响度标准化也测量（给仪表用）。 */
    wire({ cfg, bypass = false, measure = false }) {
      const { ac } = this
      this.cfg = cfg
      this.bypass = bypass
      this.disconnectAll()
      this.eqNodes = []
      this.conv = null
      if (bypass) {
        this.src.connect(this.boost)
        this.boost.connect(ac.destination)
        this.measuring = false
        this.setGainDb(0, 0.2)
        return
      }
      let cur = this.src
      const bands = []
      if (cfg.eq.on) bands.push(...(A.EQS.find((x) => x.id === cfg.eq.id)?.bands || []))
      if (cfg.on && cfg.preset === 'night') bands.push({ t: 'lowshelf', f: 140, g: -4 })
      for (const b of bands) {
        const f = new BiquadFilterNode(ac, { type: b.t, frequency: b.f, Q: b.q ?? 1, gain: b.g ?? 0 })
        cur.connect(f)
        this.eqNodes.push(f)
        cur = f
      }
      if (cfg.reverb.on) {
        const spec = A.reverbSpec(cfg)
        this.conv = ac.createConvolver()
        this.conv.normalize = false
        this.conv.buffer = irBuffer(ac, spec)
        this.wet.gain.value = A.dbToGain(spec.mix)
        this.dry.gain.value = 1
        cur.connect(this.dry)
        cur.connect(this.conv)
        this.conv.connect(this.wet)
        this.dry.connect(this.sum)
        this.wet.connect(this.sum)
        cur = this.sum
      }
      // 响度测量的旁路（不影响声音）
      cur.connect(this.kShelf)
      this.kShelf.connect(this.kHp)
      this.kHp.connect(this.split)
      this.split.connect(this.anL, 0)
      this.split.connect(this.anR, 1)
      this.measuring = cfg.on || measure

      cur.connect(this.loud)
      cur = this.loud
      if (cfg.on) {
        const p = A.compParams(cfg.comp)
        for (const [k, v] of Object.entries(p)) this.comp[k].value = v
        this.compFix.gain.value = 1 / A.compressorMakeup(p)
        cur.connect(this.comp)
        this.comp.connect(this.compFix)
        cur = this.compFix
      } else {
        this.setGainDb(0, 0.2)
      }
      if (cfg.limiter) {
        cur.connect(this.lim)
        this.lim.connect(this.limFix)
        cur = this.limFix
      }
      cur.connect(this.boost)
      this.boost.connect(ac.destination)
    }

    setGainDb(db, tc) {
      this.tgtDb = db
      this.tc = tc
      try {
        this.loud.gain.cancelScheduledValues(this.ac.currentTime)
        this.loud.gain.setTargetAtTime(A.dbToGain(db), this.ac.currentTime, Math.max(0.02, tc))
      } catch {
        // 忽略
      }
    }

    setBoost(v) {
      this.boost.gain.value = v
    }

    /** 换了视频 / 重新开始：清掉测量，头几秒先快速拉到目标。 */
    reset() {
      this.st.reset()
      this.lufs = null
      this.fastUntil = performance.now() + 3000
      this.last = performance.now()
    }

    /** 播放时每 100 毫秒调一次：读响度、调增益。 */
    tick(now = performance.now()) {
      const dt = Math.min(0.5, Math.max(0.01, (now - this.last) / 1000))
      this.last = now
      // 模拟增益的实际走向，给仪表用
      this.curDb += (this.tgtDb - this.curDb) * (1 - Math.exp(-dt / Math.max(0.02, this.tc)))
      if (!this.measuring) return
      const n = Math.min(this.bufL.length, Math.max(1, Math.round(this.ac.sampleRate * dt)))
      this.anL.getFloatTimeDomainData(this.bufL)
      this.anR.getFloatTimeDomainData(this.bufR)
      this.st.push(A.meanSquare(this.bufL, n) + A.meanSquare(this.bufR, n))
      this.lufs = this.st.value().lufs
      if (!this.cfg.on || this.bypass) return
      const g = A.nextGain({ cur: this.tgtDb, lufs: this.lufs, cfg: this.cfg, fast: now < this.fastUntil })
      if (!g.hold) this.setGainDb(g.target, g.tc)
    }
  }

  A.Chain = Chain
})()
