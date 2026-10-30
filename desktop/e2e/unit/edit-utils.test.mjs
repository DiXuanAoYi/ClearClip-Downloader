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
