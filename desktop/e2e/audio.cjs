// 实时音效：在真实浏览器里加载内容脚本，用生成的正弦波当视频声音，量处理后的输出。
// 覆盖：安静的被拉到目标响度、响亮的被压下来且不爆音、关闭后还原、预设切换、混响尾巴、
// 跨域直链 / DRM 跳过并提示（声音不受影响）、本站关闭、临时关闭、音量放大共用同一条链。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const FILES = ['content/ui.js', 'content/audio-logic.js', 'content/audio-chain.js', 'content/audio-fx.js'].map((f) => path.join(EXT, f))
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** 立体声 16 位 WAV：997 Hz 正弦，峰值 peakDb dBFS，左右声道相同。 */
function wav(peakDb, sec = 12, sr = 48000) {
  const n = sec * sr
  const buf = Buffer.alloc(44 + n * 4)
  buf.write('RIFF', 0)
  buf.writeUInt32LE(36 + n * 4, 4)
  buf.write('WAVEfmt ', 8)
  buf.writeUInt32LE(16, 16)
  buf.writeUInt16LE(1, 20)
  buf.writeUInt16LE(2, 22)
  buf.writeUInt32LE(sr, 24)
  buf.writeUInt32LE(sr * 4, 28)
  buf.writeUInt16LE(4, 32)
  buf.writeUInt16LE(16, 34)
  buf.write('data', 36)
  buf.writeUInt32LE(n * 4, 40)
  const a = Math.pow(10, peakDb / 20) * 32767
  for (let i = 0; i < n; i++) {
    const v = Math.round(a * Math.sin((2 * Math.PI * 997 * i) / sr))
    buf.writeInt16LE(v, 44 + i * 4)
    buf.writeInt16LE(v, 46 + i * 4)
  }
  return buf
}

const PAGE = (src, extra = '') => `<!doctype html><title>Player</title><body style="margin:0;background:#222">
<video id="v" src="${src}" width="640" height="360" loop ${extra}></video></body>`

;(async () => {
  const browser = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] })
  const ctx = await browser.newContext({ viewport: { width: 900, height: 600 } })
  const quiet = wav(-26)
  const loud = wav(-6)
  await ctx.route('https://site.test/**', (route) => {
    const u = new URL(route.request().url())
    if (u.pathname === '/q.wav') return route.fulfill({ contentType: 'audio/wav', body: quiet })
    if (u.pathname === '/l.wav') return route.fulfill({ contentType: 'audio/wav', body: loud })
    return route.fulfill({ contentType: 'text/html; charset=utf-8', body: PAGE(u.searchParams.get('src') || '/q.wav') })
  })
  await ctx.route('https://cdn.test/**', (route) => route.fulfill({ contentType: 'audio/wav', body: quiet }))

  async function open(url, opts = {}) {
    const page = await ctx.newPage()
    const errors = []
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript((o) => {
      const orig = Element.prototype.attachShadow
      Element.prototype.attachShadow = function (init) {
        const r = orig.call(this, { ...init, mode: 'open' })
        window.__roots = (window.__roots || []).concat(r)
        return r
      }
      // 在最终输出前挂一个分析器，量实际听到的声音（不改声音）
      const oc = AudioNode.prototype.connect
      AudioNode.prototype.connect = function (dest, ...a) {
        if (dest instanceof AudioDestinationNode) {
          if (!window.__tap) {
            window.__tap = this.context.createAnalyser()
            window.__tap.fftSize = 2048
            window.__tap.smoothingTimeConstant = 0
          }
          oc.call(this, window.__tap)
        }
        return oc.call(this, dest, ...a)
      }
      window.__peak = () => {
        if (!window.__tap) return -Infinity
        const b = new Float32Array(2048)
        window.__tap.getFloatTimeDomainData(b)
        let m = 0
        for (const v of b) m = Math.max(m, Math.abs(v))
        return m > 0 ? 20 * Math.log10(m) : -Infinity
      }
      window.__peakMax = async (ms) => {
        let best = -Infinity
        const end = performance.now() + ms
        while (performance.now() < end) {
          best = Math.max(best, window.__peak())
          await new Promise((r) => setTimeout(r, 20))
        }
        return best
      }
      const store = { ...o.store }
      const listeners = []
      const msgListeners = []
      window.__store = store
      window.__calls = []
      window.__setStore = (patch) => {
        const ch = {}
        for (const [k, v] of Object.entries(patch)) {
          ch[k] = { newValue: v }
          store[k] = v
        }
        for (const l of listeners) l(ch, 'local')
      }
      window.__cmd = (m) => new Promise((res) => {
        let got = false
        for (const l of msgListeners) l(m, {}, (r) => { got = true; res(r) })
        setTimeout(() => !got && res(null), 100)
      })
      window.chrome = {
        runtime: { sendMessage: async (m) => { window.__calls.push(m); return { ok: true } }, onMessage: { addListener: (l) => msgListeners.push(l) } },
        storage: {
          local: {
            get: async (k) => {
              const keys = Array.isArray(k) ? k : [k]
              return Object.fromEntries(keys.filter((x) => x in store).map((x) => [x, store[x]]))
            },
            set: async (p) => { Object.assign(store, JSON.parse(JSON.stringify(p))) },
          },
          onChanged: { addListener: (l) => listeners.push(l) },
        },
      }
      if (o.drm) {
        const d = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'mediaKeys')
        Object.defineProperty(HTMLMediaElement.prototype, 'mediaKeys', { get() { return {} }, configurable: true })
        void d
      }
    }, { store: opts.store || {}, drm: !!opts.drm })
    await page.goto(url)
    for (const f of FILES) await page.addScriptTag({ path: f })
    await sleep(150)
    return { page, errors }
  }
  const play = (page) => page.evaluate(() => document.getElementById('v').play())
  const stat = (page) => page.evaluate(() => window.__cmd({ type: 'cc-audio', action: 'stat' }))
  const peak = (page, ms = 600) => page.evaluate((m) => window.__peakMax(m), ms)
  const pillText = (page) => page.evaluate(() => (window.__roots || []).map((r) => r.querySelector('.apill')?.textContent).find(Boolean) || null)
  const hover = async (page) => { await page.mouse.move(250, 150); await sleep(200); await page.mouse.move(300, 200); await sleep(250) }

  // ---- 1. 安静的视频被拉到目标响度（-16 LUFS，输入 -26 dBFS 峰值 ≈ -25.3 LUFS → 约 +9 dB）
  {
    const { page, errors } = await open('https://site.test/')
    await play(page)
    await sleep(7000)
    const s = await stat(page)
    ok('正在处理：1 个视频已接入', s && s.active === 1 && s.blocked === 0, JSON.stringify(s))
    ok('测到的输入响度约 -25 LUFS', s && s.lufs != null && Math.abs(s.lufs - -25.3) < 1.5, JSON.stringify(s))
    ok('增益约 +9 dB', s && s.gain != null && s.gain > 7 && s.gain < 11, JSON.stringify(s))
    const p = await peak(page)
    ok('输出峰值接近目标（-21 ~ -14 dBFS）', p > -21 && p < -14, String(p))
    await hover(page)
    ok('视频上显示小按钮：音效 -16 LUFS', (await pillText(page)) === '音效 -16 LUFS', String(await pillText(page)))
    ok('没有脚本错误', errors.length === 0, errors.join('|'))

    // 关闭响度标准化 → 增益回到 0，输出回到原声（峰值约 -26）
    await page.evaluate(() => window.__setStore({ audioCfg: { on: false } }))
    await sleep(2500)
    const off = await peak(page)
    ok('关掉后输出还原为原声（约 -26 dBFS）', off > -28.5 && off < -23.5, String(off))
    // 再打开 → 又被拉起来
    await page.evaluate(() => window.__setStore({ audioCfg: { on: true } }))
    await sleep(4500)
    const back = await peak(page)
    ok('再打开又回到目标附近', back > -21 && back < -14, String(back))

    // 预设：夜间 -20 LUFS → 输出比温和低
    await page.evaluate(() => window.__setStore({ audioCfg: { on: true, preset: 'night' } }))
    await sleep(5500)
    const night = await peak(page)
    ok('夜间预设（-20 LUFS）比温和更小声', night < back - 1.5, `${night} vs ${back}`)
    // 自定义目标 -12 → 更响（最大增益 12 够用：-25.3 → -12 需要 13.3，会被封顶在 +12）
    await page.evaluate(() => window.__setStore({ audioCfg: { on: true, preset: 'custom', target: -12, comp: 0, maxGain: 24 } }))
    await sleep(5500)
    const c12 = await peak(page)
    ok('自定义 -12 LUFS 更响，且不超过限幅上限（-1.5 dBFS 附近）', c12 > back + 2 && c12 < -0.5, String(c12))
    await page.close()
  }

  // ---- 2. 很响的视频被压下来，限幅器防爆音
  {
    const { page } = await open('https://site.test/?src=/l.wav')
    await play(page)
    await sleep(6500)
    const s = await stat(page)
    ok('响亮视频：增益为负', s && s.gain != null && s.gain < -6, JSON.stringify(s))
    const p = await peak(page)
    ok('响亮视频：输出被压到目标附近（< -12 dBFS）', p < -12 && p > -24, String(p))
    // 目标定到很高 + 不压缩：限幅器把峰值挡在 0 dBFS 以下
    await page.evaluate(() => window.__setStore({ audioCfg: { on: true, preset: 'custom', target: -8, comp: 0, maxGain: 24, limiter: true } }))
    await sleep(5000)
    const hot = await peak(page, 800)
    ok('限幅器开着：输出峰值不超过 -0.5 dBFS', hot < -0.5, String(hot))
    await page.close()
  }

  // ---- 3. 混响：暂停后还有尾巴
  {
    const { page } = await open('https://site.test/', { store: { audioCfg: { on: true, reverb: { on: true, id: 'church' } } } })
    await play(page)
    await sleep(5000)
    await page.evaluate(() => document.getElementById('v').pause())
    await sleep(120)
    const tail = await page.evaluate(() => window.__peakMax(120))
    ok('开了混响（教堂）：暂停后仍有混响尾巴（> -60 dBFS）', tail > -60, String(tail))
    await page.close()
    const b = await open('https://site.test/')
    await play(b.page)
    await sleep(3000)
    await b.page.evaluate(() => document.getElementById('v').pause())
    await sleep(250)
    const none = await b.page.evaluate(() => window.__peakMax(120))
    ok('不开混响：暂停后很快安静（< -70 dBFS）', none < -70, String(none))
    await b.page.close()
  }

  // ---- 4. 跨域直链：跳过并提示，声音不受影响
  {
    const { page } = await open('https://site.test/?src=https://cdn.test/q.wav')
    await play(page)
    await sleep(2500)
    const s = await stat(page)
    ok('跨域直链：没有接入，记为无法处理', s && s.active === 0 && s.blocked === 1 && s.reasons.includes('cross'), JSON.stringify(s))
    const t = await page.evaluate(() => document.getElementById('v').currentTime)
    ok('跨域直链：照常播放', t > 1.5, String(t))
    const hasToast = await page.evaluate(() => (window.__roots || []).some((r) => /无法统一音量/.test(r.textContent || '')))
    ok('跨域直链：出现“无法统一音量”提示', hasToast)
    await hover(page)
    ok('跨域直链：小按钮显示“音效 未处理”', (await pillText(page)) === '音效 未处理', String(await pillText(page)))
    await page.close()
  }

  // ---- 5. DRM：跳过并提示
  {
    const { page } = await open('https://site.test/', { drm: true })
    await play(page)
    await sleep(1500)
    const s = await stat(page)
    ok('受版权保护的视频：跳过', s && s.active === 0 && s.reasons.includes('drm'), JSON.stringify(s))
    await page.close()
  }

  // ---- 5b. “不再提示”：设了就不弹
  {
    const { page } = await open('https://site.test/?src=https://cdn.test/q.wav', { store: { audioCfg: { notifyOff: true } } })
    await play(page)
    await sleep(1500)
    const hasToast = await page.evaluate(() => (window.__roots || []).some((r) => /无法统一音量/.test(r.textContent || '')))
    ok('选了“不再提示”就不弹提示', !hasToast)
    await page.close()
  }

  // ---- 6. 本站关闭 / 临时关闭 / 功能总开关
  {
    const { page } = await open('https://site.test/', { store: { siteRules: { off: { audio: ['site.test'] } } } })
    await play(page)
    await sleep(2500)
    const s = await stat(page)
    ok('本站关闭：不接入', s && s.active === 0 && s.siteOff === true, JSON.stringify(s))
    const p = await peak(page)
    ok('本站关闭：声音保持原样', p === -Infinity || (p > -28.5 && p < -23.5), String(p))
    await page.close()
    const t = await open('https://site.test/')
    await play(t.page)
    await sleep(6000)
    const r = await t.page.evaluate(() => window.__cmd({ type: 'cc-audio', action: 'tmp', off: true }))
    ok('临时关闭：返回已关闭', r && r.tmpOff === true, JSON.stringify(r))
    await sleep(2500)
    const q = await peak(t.page)
    ok('临时关闭：输出回到原声', q > -28.5 && q < -23.5, String(q))
    await t.page.evaluate(() => window.__cmd({ type: 'cc-audio', action: 'tmp', off: false }))
    await sleep(5000)
    const q2 = await peak(t.page)
    ok('恢复后又处理', q2 > -21 && q2 < -14, String(q2))
    await t.page.close()
    const f = await open('https://site.test/', { store: { features: { audioFx: false } } })
    await play(f.page)
    await sleep(2000)
    const fs = await stat(f.page)
    ok('功能总开关关闭：不接入', fs && fs.active === 0, JSON.stringify(fs))
    await f.page.close()
  }

  // ---- 7. 快捷面板：切预设、开混响、本站关闭
  {
    const { page } = await open('https://site.test/')
    await play(page)
    await sleep(1500)
    await hover(page)
    await page.evaluate(() => (window.__roots || []).map((r) => r.querySelector('.apill')).find(Boolean).click())
    await sleep(200)
    const chips = await page.evaluate(() => (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.apnl .c')].map((b) => b.textContent)))
    ok('面板有五个响度预设和三档混响', ['温和', '标准', '安静', '夜间', '广播', '关', '房间', '大厅'].every((x) => chips.includes(x)), chips.join(','))
    const click = (label) => page.evaluate((l) => [...(window.__roots || []).flatMap((r) => [...r.querySelectorAll('.apnl .c')])].find((b) => b.textContent === l).click(), label)
    await click('广播')
    await sleep(200)
    ok('点“广播” → 保存成广播预设', (await page.evaluate(() => window.__store.audioCfg?.preset)) === 'broadcast')
    await click('大厅')
    await sleep(200)
    const rv = await page.evaluate(() => window.__store.audioCfg?.reverb)
    ok('点“大厅” → 开混响（大厅）', rv && rv.on === true && rv.id === 'hall', JSON.stringify(rv))
    await click('关')
    await sleep(200)
    ok('点“关” → 关混响', (await page.evaluate(() => window.__store.audioCfg?.reverb?.on)) === false)
    const meter = await page.evaluate(() => (window.__roots || []).map((r) => r.querySelector('.apnl .m')?.textContent).find(Boolean))
    ok('面板里有响度读数', /LUFS/.test(meter || ''), String(meter))
    await page.evaluate(() => (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.apnl .lk')]).find((b) => b.textContent === '本站关闭').click())
    await sleep(300)
    const off = await page.evaluate(() => window.__store.siteRules?.off?.audio)
    ok('点“本站关闭” → 写进站点名单', Array.isArray(off) && off.includes('site.test'), JSON.stringify(off))
    await page.close()
  }

  // ---- 8. 音量放大共用同一条链（不会因为重复接入而报错或没声音）
  {
    const { page } = await open('https://site.test/')
    await play(page)
    await sleep(2500)
    const r = await page.evaluate(() => {
      const v = document.getElementById('v')
      const a = globalThis.ccAudioEngine.setBoost(v, 200)
      return { a, b: globalThis.ccAudioEngine.getBoost(v) }
    })
    ok('音量放大：同一条链里设置成功', r.a === true && r.b === 2, JSON.stringify(r))
    await sleep(500)
    const s = await stat(page)
    ok('音量放大后仍只有一条链', s && s.active === 1, JSON.stringify(s))
    await page.close()
  }

  await browser.close()
  console.log(`audio: ${pass} passed, ${fail} failed`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
