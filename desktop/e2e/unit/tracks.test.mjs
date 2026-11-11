import test from 'node:test'
import assert from 'node:assert/strict'
import { effectiveProject, dropTrack, freeTrack, nextNo, packTracks, roomFor, shiftUp, shownNos, swapTracks, trackCode, trackOf, trackTitle, metaFor } from '../../src/utils/tracks.ts'
import { normalizeProject } from '../../src/utils/edit.ts'

const dur = { overlay: (o) => o.outMs - o.inMs, audio: (a) => (a.outMs ?? 5000) - a.inMs }
const proj = (extra = {}) => normalizeProject({ clips: [{ id: 1, path: '/a.mp4', outMs: 10000 }], ...extra })

test('编号牌和名字', () => {
  assert.equal(trackCode('video', 0), 'V1')
  assert.equal(trackCode('video', 2), 'V3')
  assert.equal(trackCode('audio', 2), 'A2')
  assert.equal(trackCode('text', 1), 'T1')
  const p = proj()
  assert.equal(trackTitle(trackOf(p, 'video', 0)), '主轨')
  assert.equal(trackTitle({ ...trackOf(p, 'video', 2), role: 'pip' }), '画中画')
  assert.equal(trackTitle({ ...trackOf(p, 'audio', 1), name: '  配乐 ' }), '配乐')
})

test('旧工程：重叠的配乐、文字分到不同轨道，不重叠的共用一条', () => {
  const p = proj({
    audio: [
      { id: 2, path: '/m1.mp3', startMs: 0, inMs: 0, outMs: 5000 },
      { id: 3, path: '/m2.mp3', startMs: 1000, inMs: 0, outMs: 5000 },
      { id: 4, path: '/m3.mp3', startMs: 6000, inMs: 0, outMs: 2000 },
    ],
    texts: [
      { id: 5, text: 'a', startMs: 0, endMs: 2000 },
      { id: 6, text: 'b', startMs: 1000, endMs: 3000 },
    ],
  })
  assert.deepEqual(p.audio.map((a) => a.track), [1, 2, 1])
  assert.deepEqual(p.texts.map((t) => t.track), [1, 2])
  assert.deepEqual(shownNos(p, 'audio'), [1, 2])
})

test('新工程文件里写了轨道编号就不再重新分配', () => {
  const p = proj({ audio: [{ id: 2, track: 3, path: '/m.mp3', startMs: 0, outMs: 1000 }, { id: 3, track: 3, path: '/n.mp3', startMs: 0, outMs: 1000 }] })
  assert.deepEqual(p.audio.map((a) => a.track), [3, 3])
})

test('显示哪几条轨道、还能建几条、下一条用哪个编号', () => {
  const p = proj({ overlays: [{ id: 2, path: '/b.mp4', track: 3, outMs: 1000 }], tracks: [{ kind: 'video', no: 5, name: '空轨' }] })
  assert.deepEqual(shownNos(p, 'video'), [0, 3, 5])
  assert.equal(nextNo(p, 'video'), 6)
  assert.equal(roomFor(p, 'video'), 12 - 3)
  assert.equal(nextNo(p, 'audio'), 2, 'A1 一直显示，新建的是 A2')
  const full = proj({ tracks: Array.from({ length: 8 }, (_, i) => ({ kind: 'text', no: i + 1 })) })
  assert.equal(nextNo(full, 'text'), null)
  assert.equal(roomFor(full, 'text'), 0)
})

test('放不下就找最近的空轨道，锁定的和主轨不放', () => {
  const p = proj({
    overlays: [
      { id: 2, path: '/b.mp4', track: 1, startMs: 0, outMs: 5000 },
      { id: 3, path: '/c.mp4', track: 2, startMs: 0, outMs: 5000 },
    ],
    tracks: [{ kind: 'video', no: 3, lock: true }, { kind: 'video', no: 4 }],
  })
  assert.equal(freeTrack(p, 'video', 1, 1000, 2000, dur), 4, '1、2 被占，3 锁定，4 空')
  assert.equal(freeTrack(p, 'video', 1, 6000, 7000, dur), 1, '这段时间 1 号是空的')
  assert.equal(freeTrack(p, 'video', 1, 1000, 2000, dur, 2), 1, '跳过自己')
})

test('在中间插入一条轨道：上面的编号和素材整体加一', () => {
  const p = proj({ overlays: [{ id: 2, path: '/b.mp4', track: 1 }, { id: 3, path: '/c.mp4', track: 2 }], tracks: [{ kind: 'video', no: 2, name: 'B' }] })
  shiftUp(p, 'video', 2)
  assert.deepEqual(p.overlays.map((o) => o.track), [1, 3])
  assert.equal(p.tracks[0].no, 3)
  assert.equal(p.tracks[0].name, 'B')
})

test('删掉一条轨道：连素材一起删，更高的往下补', () => {
  const p = proj({ audio: [{ id: 2, track: 1, path: '/a.mp3' }, { id: 3, track: 2, path: '/b.mp3' }, { id: 4, track: 3, path: '/c.mp3' }], tracks: [{ kind: 'audio', no: 3, name: 'C' }] })
  assert.equal(dropTrack(p, 'audio', 2), 1)
  assert.deepEqual(p.audio.map((a) => [a.id, a.track]), [[2, 1], [4, 2]])
  assert.equal(p.tracks[0].no, 2)
})

test('对调两条轨道', () => {
  const p = proj({ texts: [{ id: 2, text: 'a', track: 1 }, { id: 3, text: 'b', track: 2 }], tracks: [{ kind: 'text', no: 1, name: '一' }] })
  swapTracks(p, 'text', 1, 2)
  assert.deepEqual(p.texts.map((t) => t.track), [2, 1])
  assert.equal(p.tracks[0].no, 2)
})

test('metaFor 第一次用时登记，之后取同一条', () => {
  const p = proj()
  metaFor(p, 'audio', 2).mute = true
  assert.equal(metaFor(p, 'audio', 2).mute, true)
  assert.equal(p.tracks.length, 1)
})

test('预览 / 导出的有效工程：隐藏、静音、音量、独奏', () => {
  const p = proj({
    overlays: [{ id: 2, path: '/b.mp4', track: 1 }, { id: 3, path: '/c.mp4', track: 2 }],
    audio: [{ id: 4, track: 1, path: '/a.mp3', volume: 0.8 }, { id: 5, track: 2, path: '/b.mp3' }, { id: 6, track: 3, path: '/c.mp3' }],
    texts: [{ id: 7, text: 'x', track: 3 }, { id: 8, text: 'y', track: 1 }, { id: 9, text: 'z', track: 2 }],
    tracks: [
      { kind: 'video', no: 1, hidden: true },
      { kind: 'video', no: 2, mute: true },
      { kind: 'audio', no: 1, volume: 0.5, duck: true },
      { kind: 'audio', no: 2, mute: true },
      { kind: 'audio', no: 3, solo: true },
      { kind: 'text', no: 3, hidden: true },
    ],
  })
  const e = effectiveProject(p)
  assert.deepEqual(e.overlays.map((o) => [o.id, o.mute]), [[3, true]])
  assert.deepEqual(e.audio.map((a) => a.id), [4, 6], '导出不看独奏')
  assert.ok(Math.abs(e.audio[0].volume - 0.4) < 1e-9 && e.audio[0].duck)
  assert.deepEqual(e.texts.map((t) => t.id), [8, 9], '隐藏的去掉，编号小的先画')
  assert.deepEqual(effectiveProject(p, true).audio.map((a) => a.id), [6], '预览时只听独奏的')
  assert.equal(p.overlays.length, 2, '不改原来的工程')
})

test('packTracks：贪心分配，超过上限的挤进最后一条', () => {
  const items = [0, 1, 2, 3].map((i) => ({ id: i, start: 0, end: 1000 }))
  const m = packTracks(items, 3)
  assert.deepEqual([...m.values()].sort(), [1, 2, 3, 3])
})
