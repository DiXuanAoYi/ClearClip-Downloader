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
