// 实时音效的纯逻辑：设置整理、响度测量与门限、增益控制、压缩参数、混响脉冲响应、能不能处理的判断。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ URL, Float32Array, Math, Number, Object, Array })
vm.runInContext(fs.readFileSync(path.join(dir, 'content/audio-logic.js'), 'utf8'), ctx, { filename: 'audio-logic.js' })
const A = vm.runInContext('globalThis.ccAudio', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// ---- 设置：默认值与夹范围
{
  const d = plain(A.normalizeCfg(undefined))
  assert.equal(d.on, true, '响度标准化默认开')
  assert.equal(d.preset, 'aoyi', '默认预设“奥懿”')
  assert.equal(d.target, -11)
  assert.equal(d.comp, 15)
  assert.equal(d.maxGain, 24)
  assert.deepEqual(plain(d.userPresets), [])
  assert.equal(d.limiter, true)
  assert.equal(d.reverb.on, false, '混响默认关')
  assert.equal(d.eq.on, false, '均衡器默认关')
  assert.equal(d.pillShow, false, '视频上的音效按钮新安装默认不显示')
  assert.equal(d.pillHover, true)
  assert.equal(A.normalizeCfg({ pillShow: true }).pillShow, true)
  assert.equal(A.normalizeCfg({ pillShow: 'yes' }).pillShow, false, '不是布尔值就用默认')
  // 选了预设，以预设为准，不管存的旧数值
  assert.equal(A.normalizeCfg({ preset: 'night', target: -5 }).target, -20)
  assert.equal(A.normalizeCfg({ preset: 'night' }).comp, 70)
  // 自定义：夹在范围内；坏值退回默认
  const c = plain(A.normalizeCfg({ preset: 'custom', target: -50, comp: 500, maxGain: -3, speed: 'x' }))
  assert.equal(c.target, -30)
  assert.equal(c.comp, 100)
  assert.equal(c.maxGain, 0)
  assert.equal(c.speed, 'mid')
  assert.equal(A.normalizeCfg({ preset: 'custom', target: 'abc' }).target, -11)
  // 不认识的预设 / 混响 / 均衡器 → 默认
  assert.equal(A.normalizeCfg({ preset: 'zzz' }).preset, 'aoyi')
  assert.equal(A.normalizeCfg({ reverb: { on: true, id: 'nope' } }).reverb.id, 'room')
  assert.equal(A.normalizeCfg({ eq: { on: true, id: 'nope' } }).eq.id, 'voice')
  assert.equal(A.normalizeCfg({ reverb: { on: 'yes' } }).reverb.on, false, '只认布尔 true')
  assert.equal(A.normalizeCfg({ reverb: { mix: -99 } }).reverb.mix, -30)
}

// ---- 预设：按行业和用途分组，目标各不相同；选预设 / 改数值
{
  assert.deepEqual(plain(A.LOUD_PRESETS.map((p) => p.id)), ['aoyi', 'standard', 'short', 'podcast', 'broadcast', 'cn-tv', 'atsc', 'mild', 'quiet', 'night'])
  assert.equal(new Set(A.LOUD_PRESETS.map((p) => p.id)).size, A.LOUD_PRESETS.length)
  const by = (id) => plain(A.LOUD_PRESETS.find((p) => p.id === id))
  assert.deepEqual([by('aoyi').target, by('aoyi').comp, by('aoyi').maxGain], [-11, 15, 24])
  assert.equal(by('broadcast').target, -23, '广播 EBU R128')
  assert.equal(by('cn-tv').target, -24)
  assert.equal(by('standard').target, -14)
  assert.ok(A.LOUD_PRESETS.every((p) => p.group && p.name && p.note))
  const base = A.normalizeCfg()
  const std = plain(A.withPreset(base, 'standard'))
  assert.equal(std.target, -14)
  assert.equal(std.preset, 'standard')
  const cu = plain(A.withCustom(std, { target: -19 }))
  assert.equal(cu.preset, 'custom')
  assert.equal(cu.target, -19)
  assert.equal(cu.comp, 40, '没改的项沿用当前预设的数值')
  assert.equal(plain(A.withCustom(cu, { comp: 10 })).target, -19)
}

// ---- 用户自己存的预设：可以存很多个，选中、更新、删除，存储里的坏数据被整理掉
{
  let c = A.normalizeCfg()
  c = A.saveUserPreset(A.withCustom(c, { target: -13, comp: 25, maxGain: 10 }), '会议')
  assert.equal(c.userPresets.length, 1)
  assert.equal(c.preset, c.userPresets[0].id, '存完就选中它')
  c = A.saveUserPreset(A.withCustom(c, { target: -18, comp: 60, maxGain: 6 }), '睡前')
  assert.equal(c.userPresets.length, 2, '可以存多个，不是只有一个')
  assert.deepEqual(plain(A.allPresets(c).slice(-2).map((p) => p.name)), ['会议', '睡前'])
  // 选回第一个：数值跟着它走
  const meeting = c.userPresets[0]
  const back = plain(A.withPreset(c, meeting.id))
  assert.deepEqual([back.preset, back.target, back.comp, back.maxGain], [meeting.id, -13, 25, 10])
  // 同名就更新，不新增
  const upd = A.saveUserPreset(A.withCustom(back, { target: -15 }), '会议')
  assert.equal(upd.userPresets.length, 2)
  assert.equal(upd.userPresets[0].target, -15)
  assert.equal(upd.preset, meeting.id)
  // 空名字自动起名
  assert.match(A.saveUserPreset(A.normalizeCfg(), '  ').userPresets[0].name, /^自定义 1$/)
  // 存取一轮后还在
  const again = plain(A.normalizeCfg(JSON.parse(JSON.stringify(upd))))
  assert.equal(again.preset, meeting.id)
  assert.equal(again.target, -15)
  assert.equal(again.userPresets.length, 2)
  // 删掉正在用的：变成自定义，数值不变；删别的：选择不变
  const del = plain(A.deleteUserPreset(upd, meeting.id))
  assert.equal(del.preset, 'custom')
  assert.equal(del.target, -15)
  assert.equal(del.userPresets.length, 1)
  assert.equal(plain(A.deleteUserPreset(upd, c.userPresets[1].id)).preset, meeting.id)
  // 改了数值就离开预设（变成自定义），预设本身不动
  const edit = plain(A.withCustom(back, { comp: 5 }))
  assert.equal(edit.preset, 'custom')
  assert.equal(edit.userPresets[0].comp, 25)
  // 数量上限；满了新名字存不进去
  let full = A.normalizeCfg()
  for (let i = 0; i < A.MAX_USER_PRESETS + 3; i++) full = A.saveUserPreset(full, `p${i}`)
  assert.equal(full.userPresets.length, A.MAX_USER_PRESETS)
  // 坏数据：id 不合法、没名字、重复 id、数值越界
  const dirty = plain(A.normalizeCfg({ preset: 'u-a', userPresets: [{ id: 'u-a', name: ' 好 ', target: -99, comp: 500, maxGain: 'x' }, { id: 'u-a', name: '重复' }, { id: 'bad', name: '坏 id' }, { id: 'u-b', name: '' }, null, 3] }))
  assert.equal(dirty.userPresets.length, 1)
  assert.deepEqual([dirty.userPresets[0].name, dirty.userPresets[0].target, dirty.userPresets[0].comp, dirty.userPresets[0].maxGain], ['好', -30, 100, 24])
  assert.equal(dirty.preset, 'u-a')
  // 选的预设已经不存在 → 回到默认
  assert.equal(A.normalizeCfg({ preset: 'u-gone', userPresets: [] }).preset, 'aoyi')
}

// ---- 响度公式
{
  // 双声道各一个峰值 0.1 的正弦：均方 0.005，两路相加 0.01 → -0.691 + 10·log10(0.01) = -20.691
  assert.ok(Math.abs(A.toLufs(0.01) - -20.691) < 0.01)
  assert.equal(A.toLufs(0), -Infinity)
  const buf = new Float32Array(1000).fill(0.5)
  assert.ok(Math.abs(A.meanSquare(buf) - 0.25) < 1e-9)
  assert.ok(Math.abs(A.meanSquare(buf, 100) - 0.25) < 1e-9)
  assert.equal(A.meanSquare(new Float32Array(0)), 0)
  // 只取最后 n 个样本
  const half = new Float32Array(10)
  half.fill(1, 5)
  assert.equal(A.meanSquare(half, 5), 1)
}

// ---- 短时响度：门限
{
  const st = new A.ShortTerm()
  assert.equal(st.value().lufs, null, '没有数据')
  for (let i = 0; i < 3; i++) st.push(0.01)
  assert.equal(st.value().lufs, null, '不足 400 毫秒不算')
  st.push(0.01)
  assert.ok(Math.abs(st.value().lufs - A.toLufs(0.01)) < 1e-9)
  // 静音块（低于 -70 LUFS）被丢掉，不会把响度拉低
  const s2 = new A.ShortTerm()
  for (let i = 0; i < 20; i++) s2.push(0.01)
  for (let i = 0; i < 10; i++) s2.push(0)
  assert.ok(Math.abs(s2.value().lufs - A.toLufs(0.01)) < 1e-9)
  // 讲话停顿（比平均低 10 LU 以上的块）被相对门限丢掉
  const s3 = new A.ShortTerm()
  for (let i = 0; i < 20; i++) s3.push(0.01)
  for (let i = 0; i < 10; i++) s3.push(0.0001) // -20 LU
  assert.ok(Math.abs(s3.value().lufs - A.toLufs(0.01)) < 0.05)
  // 只保留最近 3 秒（30 块）
  const s4 = new A.ShortTerm()
  for (let i = 0; i < 100; i++) s4.push(0.0001)
  for (let i = 0; i < 30; i++) s4.push(0.01)
  assert.ok(Math.abs(s4.value().lufs - A.toLufs(0.01)) < 1e-9)
  s4.reset()
  assert.equal(s4.value().lufs, null)
}

// ---- 增益控制
{
  const cfg = A.normalizeCfg({ preset: 'mild' }) // -16，最大 +12，中速
  // 太小声：往上拉，时间常数 = 1.5 秒
  let g = plain(A.nextGain({ cur: 0, lufs: -26, cfg }))
  assert.equal(g.target, 10)
  assert.equal(g.tc, 1.5)
  // 太响：往下压，反应快三倍
  g = plain(A.nextGain({ cur: 0, lufs: -10, cfg }))
  assert.equal(g.target, -6)
  assert.equal(g.tc, 0.5)
  // 刚换视频：先快速拉到位
  assert.equal(A.nextGain({ cur: 0, lufs: -26, cfg, fast: true }).tc, 0.25)
  // 限制在 [-18, 最大增益]
  assert.equal(A.nextGain({ cur: 0, lufs: -60, cfg }).target, 12)
  assert.equal(A.nextGain({ cur: 0, lufs: 0, cfg }).target, -16)
  assert.equal(A.nextGain({ cur: 0, lufs: -5, cfg: { ...cfg, target: -30 } }).target, -18)
  // 变化小于 0.5 dB 不动；没测到就不动
  assert.equal(A.nextGain({ cur: 3.8, lufs: -19.9, cfg }).hold, true)
  assert.equal(A.nextGain({ cur: 3, lufs: null, cfg }).hold, true)
  assert.equal(A.nextGain({ cur: 3, lufs: NaN, cfg }).hold, true)
  // 速度三档
  const slow = A.normalizeCfg({ speed: 'slow' })
  const fast = A.normalizeCfg({ speed: 'fast' })
  assert.equal(A.nextGain({ cur: 0, lufs: -26, cfg: slow }).tc, 3)
  assert.equal(A.nextGain({ cur: 0, lufs: -26, cfg: fast }).tc, 0.6)
  // 模拟收敛：目标 -16，输入 -28 → 增益 +12 恰好到上限，输出 -16
  const c = A.normalizeCfg({ preset: 'custom', target: -16, maxGain: 12 })
  assert.equal(A.nextGain({ cur: 0, lufs: -28, cfg: c }).target, 12)
}

// ---- 压缩与限幅参数
{
  const p0 = A.compParams(0)
  assert.equal(p0.ratio, 1, '0 = 不压缩')
  const p30 = A.compParams(30)
  assert.ok(p30.threshold < -12 && p30.threshold > -24)
  assert.ok(Math.abs(p30.ratio - 2.5) < 1e-9)
  const p100 = A.compParams(100)
  assert.equal(p100.ratio, 6)
  assert.ok(p100.threshold >= -100 && p100.threshold <= 0)
  assert.ok(A.compParams(70).ratio > A.compParams(30).ratio, '夜间比温和压得更狠')
  const lim = plain(A.LIMITER)
  assert.ok(lim.threshold < 0 && lim.ratio >= 20)
  assert.ok(Math.abs(A.dbToGain(6.0206) - 2) < 1e-3)
  assert.equal(A.dbToGain(0), 1)
}

// ---- K 计权滤波器参数
{
  assert.equal(A.K_SHELF.type, 'highshelf')
  assert.ok(Math.abs(A.K_SHELF.frequency - 1681.97) < 1e-6)
  assert.ok(Math.abs(A.K_SHELF.gain - 4) < 1e-6)
  assert.equal(A.K_HP.type, 'highpass')
  assert.ok(Math.abs(A.K_HP.Q - -6.0) < 0.1, 'Web Audio 高通的 Q 以 dB 计：Q=0.5 ≈ -6 dB')
}

// ---- 混响脉冲响应
{
  const spec = A.reverbSpec(A.normalizeCfg({ reverb: { on: true, id: 'hall' } }))
  assert.equal(spec.time, 2600)
  assert.equal(spec.mix, -8, '没改混合量就用预设的')
  assert.equal(A.reverbSpec(A.normalizeCfg({ reverb: { on: true, id: 'hall', mix: -3 } })).mix, -3)
  // 和工具箱“音效 / 混响”的八个预设一致
  assert.deepEqual(plain(A.REVERBS.map((r) => r.id)), ['room', 'bath', 'living', 'class', 'studio', 'plate', 'hall', 'church'])
  const ir = A.makeImpulse(48000, spec, 1)
  assert.equal(ir.length, Math.round(0.03 * 48000) + Math.round(2.6 * 48000))
  // 能量归一到 1：混响混合 0 dB ≈ 和干声一样响
  const en = (b) => b.reduce((s, v) => s + v * v, 0)
  assert.ok(Math.abs(en(ir.left) - 1) < 1e-3)
  assert.ok(Math.abs(en(ir.right) - 1) < 1e-3)
  // 预延迟这一段是静音
  const pre = Math.round(0.03 * 48000)
  assert.ok(ir.left.slice(0, pre).every((v) => v === 0))
  // 越往后越弱
  const seg = (b, a, z) => en(b.slice(a, z))
  assert.ok(seg(ir.left, pre, pre + 4800) > seg(ir.left, pre + 48000, pre + 52800) * 5)
  // 同一个种子结果相同；左右声道不同（有立体感）
  const again = A.makeImpulse(48000, spec, 1)
  assert.equal(again.left[pre + 100], ir.left[pre + 100])
  assert.notEqual(ir.left[pre + 100], ir.right[pre + 100])
  // 高频保留越小，高频消散越快 → 尾部越闷
  const dull = A.makeImpulse(48000, { ...spec, hf: 0.2 }, 1)
  const hi = (b, a, z) => { let e = 0; for (let i = a + 1; i < z; i++) e += (b[i] - b[i - 1]) ** 2; return e }
  const tailFrom = pre + 80000
  assert.ok(hi(dull.left, tailFrom, tailFrom + 8000) / en(dull.left.slice(tailFrom, tailFrom + 8000)) < hi(ir.left, tailFrom, tailFrom + 8000) / en(ir.left.slice(tailFrom, tailFrom + 8000)))
  // 参数越界会夹回去
  assert.ok(A.makeImpulse(48000, { time: 99999, hf: 5, pre: 9999 }, 1).length <= 48000 * 10)
}

// ---- 均衡器预设
{
  assert.deepEqual(plain(A.EQS.map((e) => e.id)), ['voice', 'bass', 'treble', 'night'])
  const v = A.EQS.find((e) => e.id === 'voice')
  assert.equal(v.bands.length, 3)
  // 与 audiofx.rs 的人声增强同一组参数：高通 80、250 Hz -2 dB、3200 Hz +2.5 dB
  assert.equal(v.bands[0].f, 80)
  assert.equal(v.bands[1].g, -2)
  assert.equal(v.bands[2].f, 3200)
  assert.equal(v.bands[2].g, 2.5)
}

// ---- 能不能处理
{
  const base = { pageOrigin: 'https://a.test', crossOrigin: null, hasMediaKeys: false }
  assert.equal(A.unprocessable({ ...base, src: 'blob:https://a.test/xyz' }), null, 'MSE / blob 没问题')
  assert.equal(A.unprocessable({ ...base, src: '' }), null)
  assert.equal(A.unprocessable({ ...base, src: 'https://a.test/v.mp4' }), null, '同源没问题')
  const cross = plain(A.unprocessable({ ...base, src: 'https://cdn.other.net/v.mp4' }))
  assert.equal(cross.code, 'cross')
  assert.ok(cross.text.includes('cdn.other.net'))
  assert.equal(A.unprocessable({ ...base, src: 'https://cdn.other.net/v.mp4', crossOrigin: 'anonymous' }), null, '声明了 CORS 的可以')
  assert.equal(A.unprocessable({ ...base, src: 'https://cdn.other.net/v.mp4', crossOrigin: '' }), null)
  assert.equal(plain(A.unprocessable({ ...base, src: 'blob:x', hasMediaKeys: true })).code, 'drm')
  assert.equal(A.unprocessable({ ...base, src: 'not a url' }), null, '读不出来的地址不拦')
}

// ---- 状态文字
{
  assert.equal(A.statusText({ siteOff: true }), '本站已关闭，声音保持原样')
  assert.equal(A.statusText({ off: true }), '已关闭，声音保持原样')
  assert.equal(A.statusText({ active: 1, count: 1 }), '正在处理本页的 1 个视频')
  assert.equal(A.statusText({ blocked: 2, active: 0, count: 2 }), '本页 2 个视频无法处理')
  assert.equal(A.statusText({ active: 1, blocked: 1, count: 2 }), '正在处理本页的 1 个视频，另有 1 个无法处理')
  assert.equal(A.statusText({ count: 0 }), '本页没有视频')
  assert.equal(A.statusText({ count: 1 }), '等待播放')
  assert.equal(A.fmtLufs(-16.04), '-16.0')
  assert.equal(A.fmtLufs(null), '--')
  assert.equal(A.fmtDb(8.14), '+8.1 dB')
  assert.equal(A.fmtDb(-3), '-3.0 dB')
}

console.log('audio-logic: ok')

// ---- 压缩器自带补偿增益
{
  assert.equal(A.compressorMakeup({ threshold: -20, knee: 12, ratio: 1 }), 1, '比例 1:1 没有补偿')
  const m30 = A.compressorMakeup(A.compParams(30))
  const m70 = A.compressorMakeup(A.compParams(70))
  assert.ok(m30 > 1.2 && m30 < 3, `压缩强度 30 的补偿约几 dB：${m30}`)
  assert.ok(m70 > m30, '压得越狠，自带补偿越大')
  const ml = A.compressorMakeup(A.LIMITER)
  assert.ok(ml > 1 && ml < 1.2, `限幅器补偿不到 1 dB：${ml}`)
}
