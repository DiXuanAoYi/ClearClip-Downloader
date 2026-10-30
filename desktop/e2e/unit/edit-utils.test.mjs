import assert from 'node:assert/strict'
import * as E from '../../src/utils/edit.ts'

const clip = (o) => ({ ...E.normalizeProject({ clips: [{ id: 1, path: 'a', kind: 'video', inMs: 0, outMs: 10000 }] }).clips[0], ...o })
// layout without transitions
let cs = [clip({ id: 1 }), clip({ id: 2 }), clip({ id: 3 })]
let l = E.layout(cs)
assert.deepEqual(l.map((x) => [x.startMs, x.endMs, x.overlapMs]), [[0, 10000, 0], [10000, 20000, 0], [20000, 30000, 0]])
// transitions overlap
cs[1].transition = { kind: 'fade', durationMs: 2000 }
cs[2].transition = { kind: 'fade', durationMs: 50 } // < 100 -> hard cut
l = E.layout(cs)
assert.deepEqual(l.map((x) => [x.startMs, x.endMs, x.overlapMs]), [[0, 10000, 0], [8000, 18000, 2000], [18000, 28000, 0]])
assert.equal(E.totalMs(cs), 28000)
// first clip transition is ignored
cs[0].transition = { kind: 'fade', durationMs: 3000 }
assert.equal(E.layout(cs)[0].overlapMs, 0)
// clamped to previous remaining and own duration
const short = [clip({ id: 1, outMs: 1000 }), clip({ id: 2, outMs: 600, transition: { kind: 'fade', durationMs: 5000 } })]
l = E.layout(short)
assert.equal(l[1].overlapMs, 600)
assert.equal(l[1].startMs, 400)
// speed
assert.equal(E.clipDuration(clip({ outMs: 10000, speed: 2 })), 5000)
assert.equal(E.clipDuration(clip({ kind: 'image', outMs: 4000 })), 4000)
// index at, later clip wins in overlap
l = E.layout(cs)
assert.equal(E.clipIndexAt(l, 9000), 1)
assert.equal(E.clipIndexAt(l, 7999), 0)
assert.equal(E.clipIndexAt(l, 28000), -1)
// source time
assert.equal(E.sourceTimeAt(clip({ inMs: 1000, outMs: 11000, speed: 2 }), { startMs: 5000, durMs: 5000, overlapMs: 0, endMs: 10000 }, 7000), 5000)
// split
const [a, b] = E.splitClip(clip({ id: 1, inMs: 2000, outMs: 12000, speed: 2, fadeInMs: 300, fadeOutMs: 400, transition: { kind: 'fade', durationMs: 500 } }), { startMs: 0, durMs: 5000, overlapMs: 0, endMs: 5000 }, 2000, 9)
assert.deepEqual([a.inMs, a.outMs, b.inMs, b.outMs], [2000, 6000, 6000, 12000])
assert.equal(a.fadeInMs, 300); assert.equal(a.fadeOutMs, 0); assert.equal(b.fadeInMs, 0); assert.equal(b.fadeOutMs, 400)
assert.equal(b.transition, null); assert.ok(a.transition)
assert.equal(E.splitClip(clip({}), { startMs: 0, durMs: 10000, overlapMs: 0, endMs: 10000 }, 50, 9), null)
const [i1, i2] = E.splitClip(clip({ kind: 'image', outMs: 5000 }), { startMs: 0, durMs: 5000, overlapMs: 0, endMs: 5000 }, 1500, 9)
assert.deepEqual([i1.outMs, i2.outMs], [1500, 3500])
// lanes
const tx = (s, e) => ({ startMs: s, endMs: e })
assert.deepEqual(E.textLanes([tx(0, 3000), tx(1000, 4000), tx(3000, 5000), tx(4000, 6000)]), [0, 1, 0, 1])
// parse/format
assert.equal(E.parseMs('1:23.456'), 83456)
assert.equal(E.parseMs('83.5'), 83500)
assert.equal(E.parseMs('1:02:03'), 3723000)
assert.equal(E.parseMs('abc'), null)
assert.equal(E.msText(83456), '1:23.456')
// fade
assert.equal(E.fadeFactor(clip({ fadeInMs: 1000 }), { startMs: 0, durMs: 10000, overlapMs: 0, endMs: 10000 }, 500), 0.5)
assert.equal(E.fadeFactor(clip({ fadeOutMs: 1000 }), { startMs: 0, durMs: 10000, overlapMs: 0, endMs: 10000 }, 9500), 0.5)
// move
assert.deepEqual(E.moveItem([1, 2, 3, 4], 0, 2), [2, 3, 1, 4])
// snap
assert.equal(E.snap(1010, [0, 1000, 2000], 30), 1000)
assert.equal(E.snap(1100, [0, 1000, 2000], 30), 1100)
// tick step
assert.ok(E.tickStep(100) >= 800)
// output size
const src = { a: { width: 1920, height: 1080 } }
assert.deepEqual(E.outputSize({ ...E.emptyProject(), clips: [clip({ rotate: 90 })] }, src), { w: 1080, h: 1920 })
console.log('edit utils ok')

// ---- 场景切分 / 去静音 ----
{
  let n = 100
  const ids = () => ++n
  const base = clip({ id: 1, inMs: 1000, outMs: 11000, fadeInMs: 300, fadeOutMs: 400, transition: { kind: 'fade', durationMs: 500 } })
  // 切点：贴着两端的、太近的、重复的都被忽略；乱序也行
  const parts = E.cutAtSourceTimes(base, [6000, 1050, 3000, 3000, 10990, 3050], ids)
  assert.deepEqual(parts.map((p) => [p.inMs, p.outMs]), [[1000, 3000], [3000, 6000], [6000, 11000]])
  assert.equal(parts[0].id, 1); assert.equal(parts[1].id, 101); assert.equal(parts[2].id, 102)
  assert.equal(parts[0].fadeInMs, 300); assert.equal(parts[0].fadeOutMs, 0)
  assert.equal(parts[1].fadeInMs, 0); assert.equal(parts[1].fadeOutMs, 0)
  assert.equal(parts[2].fadeInMs, 0); assert.equal(parts[2].fadeOutMs, 400)
  assert.ok(parts[0].transition); assert.equal(parts[1].transition, null)
  assert.equal(E.cutAtSourceTimes(base, [1050, 11000, 50], ids), null)
  assert.equal(E.cutAtSourceTimes(clip({ kind: 'image', outMs: 3000 }), [1000], ids), null)
  // 区域轨迹各留各的那一段
  const tracked = clip({ id: 1, inMs: 0, outMs: 6000, regions: [{ ...E.newRegion(1, { x: 0.1, y: 0.1, w: 0.2, h: 0.2 }, 0, 'focus'), track: [0, 1000, 2000, 3000, 4000, 5000, 6000].map((t) => ({ tMs: t, x: t / 10000, y: 0.1, w: 0.2, h: 0.2 })) }] })
  const [t1, t2] = E.cutAtSourceTimes(tracked, [3000], ids)
  assert.ok(t1.regions[0].track.at(-1).tMs <= 4000 && t1.regions[0].track.at(-1).tMs >= 3000)
  assert.ok(t2.regions[0].track[0].tMs >= 2000 && t2.regions[0].track[0].tMs <= 3000)

  // 去静音：1.5–3 秒静音、留 0.4 秒停顿 → 剪掉 1.7–2.8 秒；结尾的静音（9–11 秒）也剪
  const sil = [{ startMs: 2500, endMs: 4000 }, { startMs: 10000, endMs: 11000 }]
  const r = E.removeSilences(base, sil, 400, ids)
  assert.deepEqual(r.clips.map((p) => [p.inMs, p.outMs]), [[1000, 2700], [3800, 10200]])
  assert.equal(r.removedMs, 10000 - (1700 + 6400))
  assert.equal(r.clips[0].fadeInMs, 300); assert.equal(r.clips[1].fadeOutMs, 400)
  // 静音比要留的停顿短就不剪
  assert.equal(E.removeSilences(base, [{ startMs: 2000, endMs: 2300 }], 400, ids), null)
  // 整段都是静音：不动（不能把片段删光）
  assert.equal(E.removeSilences(base, [{ startMs: 0, endMs: 20000 }], 0, ids), null)
  // 静音段超出片段范围：按范围收紧
  const edge = E.removeSilences(base, [{ startMs: 0, endMs: 3000 }], 0, ids)
  assert.deepEqual(edge.clips.map((p) => [p.inMs, p.outMs]), [[3000, 11000]])
  assert.equal(edge.clips[0].fadeInMs, 300)
}
// 旧工程里没有声音处理字段：补默认值
{
  const c = E.normalizeProject({ clips: [{ id: 1, path: 'a', kind: 'video', inMs: 0, outMs: 1000 }] }).clips[0]
  assert.equal(c.denoise, 'off'); assert.equal(c.enhance, false); assert.equal(c.noiseFloorDb, null)
}

// ---------- 文稿 ----------
{
  const W = (t, s, e) => ({ t, startMs: s, endMs: e })
  // 中文每字一个词；英文词前带空格
  const zh = [W('嗯', 0, 300), W('今', 500, 700), W('天', 700, 900), W('那', 1000, 1200), W('个', 1200, 1400), W('很', 1400, 1600), W('好。', 1600, 2000), W('下', 3500, 3700), W('雨', 3700, 4000)]
  // 句子：句末标点 + 停顿 ≥ 700 ms
  const ss = E.sentencesOf(zh)
  assert.deepEqual(ss.map((s) => [s.from, s.to, s.startMs, s.endMs]), [[0, 6, 0, 2000], [7, 8, 3500, 4000]])
  // 口头禅：相邻的字接起来等于词表项
  assert.deepEqual(E.findFillers(zh, E.parseFillers('嗯,那个')), [0, 3, 4])
  assert.deepEqual(E.findFillers([W(' So', 0, 100), W(' um', 100, 200), W(' you', 200, 300), W(' know,', 300, 400)], E.parseFillers('um, you know')), [1, 2, 3])
  assert.deepEqual(E.findFillers(zh, []), [])
  // 词表解析：长的在前，去重去空，忽略大小写
  assert.deepEqual(E.parseFillers('Um， 那个,,um,you know'), ['youknow', 'um', '那个'])
  // 选中的词 → 时间段（相邻合并）
  assert.deepEqual(E.wordSpans(zh, [3, 4, 0]), [{ startMs: 0, endMs: 300 }, { startMs: 1000, endMs: 1400 }])
  // 属于片段的词
  assert.deepEqual(E.wordsInClip(zh, { inMs: 900, outMs: 1500 }), [3, 4, 5])
}
// 按词生成字幕：时间换算、变速、断行、逐字高亮
{
  const W = (t, s, e) => ({ t, startMs: s, endMs: e })
  const c = clip({ id: 1, inMs: 1000, outMs: 9000, speed: 2 })
  const pl = { startMs: 5000, endMs: 9000, overlapMs: 0 }
  const words = [W('今', 1500, 1700), W('天', 1700, 1900), W('很', 1900, 2100), W('好。', 2100, 2500), W('明', 4000, 4200), W('天', 4200, 4400)]
  let n = 100
  const frame = { w: 1080, h: 1920 }
  const hl = E.CAPTION_STYLES.find((s) => s.id === 'hl')
  const out = E.captionsFromWords(words, c, pl, hl, frame, () => n++)
  assert.equal(out.length, 2)
  assert.equal(out[0].text, '今天很好。')
  // 素材 1500 ms → 时间线 5000 + (1500-1000)/2 = 5250
  assert.equal(out[0].startMs, 5250)
  assert.deepEqual(out[0].words.map((w) => [w.t, w.a, w.b]), [['今', 0, 100], ['天', 100, 200], ['很', 200, 300], ['好。', 300, 500]])
  assert.equal(out[0].highlight, '#FFD400')
  assert.deepEqual(out.map((t) => t.id), [100, 101])
  // 不带高亮的样式：不写词
  const plain = E.captionsFromWords(words, c, pl, E.CAPTION_STYLES[0], frame, () => n++)
  assert.deepEqual(plain[0].words, []); assert.equal(plain[0].highlight, '')
  // 过长的一句按画面宽度断开
  const long = Array.from({ length: 40 }, (_, i) => W('字', 1000 + i * 100, 1100 + i * 100))
  const many = E.captionsFromWords(long, c, { startMs: 0, endMs: 4000, overlapMs: 0 }, E.CAPTION_STYLES[0], { w: 1080, h: 1920 }, () => n++)
  assert.ok(many.length >= 2 && many.every((t) => [...t.text].length <= 16), many.map((t) => t.text.length).join())
  // 旧工程里的文字没有这些字段：补默认值
  const t = E.normalizeProject({ texts: [{ id: 1, text: 'x' }] }).texts[0]
  assert.deepEqual(t.words, []); assert.equal(t.highlight, ''); assert.equal(t.fadeMs, 0)
}

// 关键帧：插值、删改、切开、带关键帧的取值
{
  const k = (tMs, value, ease = 'linear') => ({ tMs, value, ease })
  const pts = [k(0, 0), k(1000, 10)]
  assert.equal(E.kfEval(pts, -5), 0)
  assert.equal(E.kfEval(pts, 500), 5)
  assert.equal(E.kfEval(pts, 5000), 10)
  assert.equal(E.kfEval([], 5), null)
  assert.equal(E.kfEval([k(0, 0, 'hold'), k(1000, 10)], 999), 0)
  assert.equal(E.kfEval([k(0, 0, 'in'), k(1000, 10)], 500), 2.5)
  assert.equal(E.kfEval([k(0, 0, 'out'), k(1000, 10)], 500), 7.5)
  assert.equal(E.kfEval([k(0, 0, 'inout'), k(1000, 10)], 500), 5)
  // 整理：按时间排序、同一时刻留最后一个、坏数据去掉
  const t = E.kfTidy([k(500, 1), k(100, 2), k(500, 3), { tMs: NaN, value: 1, ease: 'linear' }, { tMs: 5, value: 1, ease: 'bogus' }])
  assert.deepEqual(t.map((x) => [x.tMs, x.value, x.ease]), [[5, 1, 'linear'], [100, 2, 'linear'], [500, 3, 'linear']])
  // 放 / 改 / 删
  let m = E.kfSet({}, 'x', 200, 0.3)
  m = E.kfSet(m, 'x', 800, 0.9, 'out')
  m = E.kfSet(m, 'x', 200, 0.4)
  assert.deepEqual(m.x.map((x) => [x.tMs, x.value, x.ease]), [[200, 0.4, 'linear'], [800, 0.9, 'out']])
  m = E.kfDelete(m, 'x', 800)
  m = E.kfDelete(m, 'x', 200)
  assert.deepEqual(m, {})
  // 切开：切点补等值点，后半从 0 算
  const [a, b] = E.kfSplit({ x: [k(0, 0), k(1000, 10)] }, 400)
  assert.deepEqual(a.x.map((x) => [x.tMs, x.value]), [[0, 0], [400, 4]])
  assert.deepEqual(b.x.map((x) => [x.tMs, x.value]), [[0, 4], [600, 10]])
  // 带关键帧的取值：没有关键帧的属性原样
  const ov = { x: 0.5, y: 0.5, scale: 1, keyframes: { x: [k(0, 0), k(1000, 1)] } }
  const w = E.withKeys(ov, 250)
  assert.equal(w.x, 0.25); assert.equal(w.y, 0.5); assert.equal(E.withKeys({ x: 1, keyframes: {} }, 10).x, 1)
  assert.deepEqual(E.kfClean({ x: [], y: [k(0, 1)], z: 'bad' }), { y: [k(0, 1)] })
}

// 倒放 / 定格 / 变速曲线 / 顺延
{
  let n = 1000
  const ids = () => n++
  const base = (o) => ({ ...E.normalizeProject({ clips: [{ id: 1, path: 'a', kind: 'video', inMs: 0, outMs: 6000 }] }).clips[0], ...o })
  const rev = base({ reverse: true })
  const pl = { startMs: 0, endMs: 6000, durMs: 6000, overlapMs: 0 }
  assert.equal(E.clipDuration(rev), 6000)
  assert.equal(E.sourceTimeAt(rev, pl, 1000), 5000)
  assert.equal(E.cutAtSourceTimes(rev, [3000], ids), null)
  const fz = base({ freeze: true, inMs: 2000, outMs: 5000 })
  assert.equal(E.clipDuration(fz), 3000)
  assert.equal(E.sourceTimeAt(fz, { startMs: 0, endMs: 3000, durMs: 3000, overlapMs: 0 }, 1500), 2000)
  // 变速曲线：N 段，总素材不变，速度相乘
  const c = base({ speed: 1 })
  const ramp = E.speedRamp(c, [0.5, 1, 2], ids)
  assert.equal(ramp.length, 3)
  assert.deepEqual(ramp.map((p) => p.speed), [0.5, 1, 2])
  assert.equal(ramp[0].inMs, 0); assert.equal(ramp[2].outMs, 6000)
  assert.equal(ramp.reduce((s, p) => s + (p.outMs - p.inMs), 0), 6000)
  assert.equal(E.speedRamp(rev, [1, 2], ids), null)
  assert.equal(E.speedRamp(base({ outMs: 100 }), [1, 2, 3, 4, 5, 6], ids), null)
  // 定格：中间切开，插一段停住的单帧
  const parts = E.freezeAt(c, pl, 2000, 1500, ids)
  assert.equal(parts.length, 3)
  assert.equal(parts[1].freeze, true)
  assert.equal(parts[1].inMs, 2000)
  assert.equal(E.clipDuration(parts[1]), 1500)
  assert.equal(parts[0].outMs, 2000); assert.equal(parts[2].inMs, 2000)
  // 贴着开头：只多一段，不切开
  const head = E.freezeAt(c, pl, 10, 1000, ids)
  assert.equal(head.length, 2); assert.equal(head[0].freeze, true)
  assert.equal(E.freezeAt(fz, pl, 10, 1000, ids), null)
  // 顺延：起点在 from 及以后的叠加素材后移
  const pr = E.normalizeProject({ overlays: [{ id: 1, path: 'x', kind: 'image', startMs: 1000, endMs: 3000 }, { id: 2, path: 'y', kind: 'image', startMs: 5000, endMs: 7000 }], texts: [{ id: 3, text: 't', startMs: 6000, endMs: 8000 }] })
  const moved = E.rippleBy(pr, 4000, 1500)
  assert.equal(moved, 2)
  assert.equal(pr.overlays[0].startMs, 1000); assert.equal(pr.overlays[1].startMs, 6500)
  assert.equal(pr.texts[0].startMs, 7500); assert.equal(pr.texts[0].endMs, 9500)
}

// 节拍卡点、配乐适配、抠像
{
  const mk = (o) => ({ ...E.normalizeProject({ clips: [{ id: 1, path: 'a', kind: 'video', inMs: 0, outMs: 10000 }] }).clips[0], ...o })
  const au = (o) => ({ id: 1, path: 'm', startMs: 0, inMs: 0, outMs: null, volume: 1, fadeInMs: 0, fadeOutMs: 0, looped: false, duck: false, beats: { bpm: 120, confidence: 0.5, beatsMs: Array.from({ length: 40 }, (_, i) => 300 + i * 500) }, ...o })
  // 旧工程没有这些字段
  const old = E.normalizeProject({ overlays: [{ id: 1, path: 'x', kind: 'image', startMs: 0, endMs: 1000 }], audio: [{ id: 1, path: 'm', startMs: 0, inMs: 0, outMs: null }] })
  assert.equal(old.overlays[0].chroma.on, false); assert.equal(old.overlays[0].chroma.color, '#00FF00'); assert.equal(old.audio[0].beats, null)
  assert.equal(E.cleanBeats({ bpm: 1, confidence: 5, beatsMs: [500, 100, 100, -3, NaN] }).confidence, 1)
  assert.deepEqual(E.cleanBeats({ bpm: 1, confidence: 1, beatsMs: [500, 100, 100, -3, NaN] }).beatsMs, [100, 500])
  assert.equal(E.cleanBeats({ bpm: 1, confidence: 1, beatsMs: [5] }), null)
  // 时间线上的位置：跟着配乐的起点和入点走
  const tl = E.beatsOnTimeline(au({ startMs: 1000, inMs: 800, outMs: 2300 }))
  assert.deepEqual(tl, [1000, 1500, 2000, 2500]) // 素材 800、1300、1800、2300 → 1000 + (b - 800)
  assert.equal(E.beatsOnTimeline(au({ beats: null })).length, 0)
  // 按节拍排列：每片 2 拍；视频缩短、图片设时长、转场去掉
  const beats = E.beatsOnTimeline(au({}))
  const dur = () => 20000
  const clips = [mk({ id: 1 }), mk({ id: 2, transition: { kind: 'fade', durationMs: 500 } }), mk({ id: 3, kind: 'image', outMs: 1000 })]
  const r = E.arrangeToBeats(clips, beats, 2, dur)
  assert.equal(r.arranged, 3); assert.equal(r.short, 0); assert.equal(r.left, 0)
  // 第一个 0 → 第 2 拍(800)；第二个 → 第 4 拍(1800)；第三个 → 第 6 拍(2800)
  assert.deepEqual(r.clips.map(E.clipDuration), [800, 1000, 1000])
  assert.deepEqual(E.layout(r.clips).map((x) => x.startMs), [0, 800, 1800])
  assert.equal(r.clips[1].transition, null)
  assert.equal(clips[1].transition.durationMs, 500) // 原数据不动
  // 视频素材比一格短：用满它，算进 short，后面的从实际结尾接着排
  const sh = E.arrangeToBeats([mk({ id: 1, outMs: 300 }), mk({ id: 2 })], beats, 2, dur)
  assert.equal(sh.short, 1)
  assert.equal(E.clipDuration(sh.clips[0]), 300)
  assert.equal(E.layout(sh.clips)[1].endMs, 1800)
  // 拍子用完了
  assert.equal(E.arrangeToBeats(clips, beats.slice(0, 3), 2, dur).left, 2)
  assert.equal(E.arrangeToBeats([], beats, 2, dur), null)
  // 变速的视频：一格里要的素材长度乘上速度
  const fast = E.arrangeToBeats([mk({ id: 1, speed: 2 })], beats, 2, dur)
  assert.equal(E.clipDuration(fast.clips[0]), 800)
  assert.equal(fast.clips[0].outMs, 1600)
  // 配乐适配
  const src = { durationMs: 8000 }
  assert.deepEqual(E.fitAudio(au({ beats: null }), src, 20000), { looped: true, outMs: 8000, fadeOutMs: 1500 })
  const cut = E.fitAudio(au({ beats: null }), { durationMs: 60000 }, 10000)
  assert.equal(cut.outMs, 10000); assert.equal(cut.fadeOutMs, 1500)
  const onBeat = E.fitAudio(au({}), { durationMs: 60000 }, 10100)
  assert.equal(onBeat.outMs, 9800) // 最后一个不超过 10100 的拍子
  assert.equal(E.fitAudio(au({ startMs: 9950 }), src, 10000), null)
  // 抠像近似矩阵：绿色的键色，纯绿像素透明、红色像素不透明
  const mx = E.chromaMatrix({ on: true, color: '#00FF00', similarity: 0.2, blend: 0.1, despill: true })
  const alpha = (r, g, b) => Math.min(1, Math.max(0, mx[15] * r + mx[16] * g + mx[17] * b + mx[19]))
  assert.equal(alpha(0, 1, 0), 0); assert.equal(alpha(1, 0, 0), 1); assert.equal(alpha(0.5, 0.5, 0.5), 1)
  assert.ok(alpha(0.1, 0.9, 0.1) < 0.05)
  const blue = E.chromaMatrix({ on: true, color: '#0000FF', similarity: 0.2, blend: 0.1, despill: true })
  assert.ok(blue[17] < 0 && Math.min(1, Math.max(0, blue[17] * 1 + blue[19])) === 0)
}
