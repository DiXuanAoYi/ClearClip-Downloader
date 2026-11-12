import assert from 'node:assert/strict'
import * as E from '../../src/utils/edit.ts'

const base = E.normalizeProject({ clips: [{ id: 1, path: 'a', kind: 'video', inMs: 0, outMs: 10000 }] }).clips[0]
const clip = (o) => ({ ...base, ...o })
const ovl = (o) => ({ ...E.blankOverlay(), id: 50, path: 'o.mp4', kind: 'video', startMs: 4000, inMs: 0, outMs: 6000, ...o })
const PL = (s, d, ov = 0) => ({ startMs: s, durMs: d, overlapMs: ov, endMs: s + d })

// ---- trimClipAt
{
  const c = clip({ id: 2, inMs: 1000, outMs: 11000, fadeInMs: 300, fadeOutMs: 400, transition: { kind: 'fade', durationMs: 500 } })
  const pl = PL(8000, 10000, 500)
  const head = E.trimClipAt(c, pl, 13000, 'in')
  assert.equal(head.id, 2)
  assert.deepEqual([head.inMs, head.outMs], [6000, 11000])
  assert.equal(head.fadeInMs, 300); assert.equal(head.fadeOutMs, 400)
  assert.deepEqual(head.transition, { kind: 'fade', durationMs: 500 })
  const tail = E.trimClipAt(c, pl, 13000, 'out')
  assert.deepEqual([tail.inMs, tail.outMs], [1000, 6000])
  assert.equal(tail.fadeOutMs, 400); assert.equal(tail.fadeInMs, 300)
  assert.deepEqual(tail.transition, { kind: 'fade', durationMs: 500 })
  assert.equal(E.trimClipAt(c, pl, 8050, 'in'), null) // too close to the start
  assert.equal(E.trimClipAt(c, pl, 17950, 'out'), null) // too close to the end
  // speed 2: 4s into the timeline = 8s of source
  const fast = clip({ inMs: 0, outMs: 20000, speed: 2 })
  assert.equal(E.trimClipAt(fast, PL(0, 10000), 4000, 'in').inMs, 8000)
  // images trim by duration
  const img = clip({ kind: 'image', outMs: 5000 })
  assert.equal(E.trimClipAt(img, PL(0, 5000), 1500, 'in').outMs, 3500)
  assert.equal(E.trimClipAt(img, PL(0, 5000), 1500, 'out').outMs, 1500)
}

// ---- trimOverlayAt
{
  const o = ovl({ fadeInMs: 200, fadeOutMs: 300 })
  const head = E.trimOverlayAt(o, 7000, 'in')
  assert.equal(head.id, 50)
  assert.deepEqual([head.startMs, head.inMs, head.outMs], [7000, 3000, 6000])
  assert.equal(head.fadeInMs, 200); assert.equal(head.fadeOutMs, 300)
  const tail = E.trimOverlayAt(o, 7000, 'out')
  assert.deepEqual([tail.startMs, tail.inMs, tail.outMs], [4000, 0, 3000])
  assert.equal(tail.fadeOutMs, 300)
  assert.equal(E.trimOverlayAt(o, 4050, 'in'), null)
  assert.equal(E.trimOverlayAt(o, 3000, 'in'), null) // before the overlay
}

// ---- detachedAudio
{
  const src = { hasAudio: true, durationMs: 9000 }
  const c = clip({ id: 3, inMs: 1000, outMs: 20000, volume: 1.5, fadeInMs: 100, fadeOutMs: 200 })
  const a = E.detachedAudio(c, 12345.4, 77, src)
  assert.equal(a.id, 77); assert.equal(a.path, 'a'); assert.equal(a.startMs, 12345)
  assert.deepEqual([a.inMs, a.outMs], [1000, 9000]) // clamped to the source
  assert.equal(a.volume, 1.5); assert.equal(a.fadeInMs, 100); assert.equal(a.fadeOutMs, 200); assert.equal(a.looped, false)
  assert.match(E.detachedAudio(clip({ kind: 'image' }), 0, 1), /图片/)
  assert.match(E.detachedAudio(clip({}), 0, 1, { hasAudio: false }), /没有声音/)
  assert.match(E.detachedAudio(clip({ mute: true }), 0, 1), /静音/)
  assert.match(E.detachedAudio(clip({ speed: 2 }), 0, 1), /变速/)
  assert.match(E.detachedAudio({ ...clip({}), looped: true }, 0, 1), /循环/)
  assert.equal(typeof E.detachedAudio(clip({ speed: 1.0000001 }), 0, 1), 'object')
}

// ---- shiftAfter (ripple)
{
  const p = E.normalizeProject({
    clips: [clip({ id: 1 })],
    overlays: [ovl({ id: 2, startMs: 5000 }), ovl({ id: 3, startMs: 12000 })],
    texts: [{ id: 4, text: 't', startMs: 11000, endMs: 14000 }, { id: 5, text: 'u', startMs: 1000, endMs: 2000 }],
    audio: [{ id: 6, path: 'm', startMs: 10000 }, { id: 7, path: 'n', startMs: 9999 }],
  })
  assert.equal(E.shiftAfter(p, 10000, 4000), 3)
  assert.deepEqual(p.overlays.map((o) => o.startMs), [5000, 8000])
  assert.deepEqual(p.texts.map((t) => [t.startMs, t.endMs]), [[7000, 10000], [1000, 2000]])
  assert.deepEqual(p.audio.map((a) => a.startMs), [6000, 9999])
  assert.equal(E.shiftAfter(p, 0, 0), 0)
  // never goes below zero
  const q = E.normalizeProject({ audio: [{ id: 1, path: 'm', startMs: 500 }] })
  E.shiftAfter(q, 0, 9000)
  assert.equal(q.audio[0].startMs, 0)
}

// ---- editPoints / neighborPoint
{
  const p = E.normalizeProject({
    clips: [clip({ id: 1 }), clip({ id: 2, transition: { kind: 'fade', durationMs: 2000 } })],
    overlays: [ovl({ id: 3, startMs: 3000 })],
    texts: [{ id: 4, text: 't', startMs: 500, endMs: 3500 }],
    audio: [{ id: 5, path: 'm', startMs: 7000 }],
  })
  const pl = E.layout(p.clips)
  assert.deepEqual(E.editPoints(p, pl), [0, 500, 3000, 3500, 7000, 8000, 9000, 10000, 18000])
  assert.deepEqual(E.editPoints(p, pl, { kind: 'text', id: 4 }), [0, 3000, 7000, 8000, 9000, 10000, 18000])
  const pts = E.editPoints(p, pl)
  assert.equal(E.neighborPoint(pts, 3000, 1), 3500)
  assert.equal(E.neighborPoint(pts, 3000, -1), 500)
  assert.equal(E.neighborPoint(pts, 0, -1), null)
  assert.equal(E.neighborPoint(pts, 18000, 1), null)
  assert.equal(E.neighborPoint(pts, 3000.3, -1), 500) // within 0.5ms of a point counts as being on it
}

// ---- stepFrame / playbackFps
{
  assert.equal(E.stepFrame(0, 25, 1), 40)
  assert.equal(E.stepFrame(40, 25, 1), 80)
  assert.equal(E.stepFrame(50, 25, 1), 80) // snaps to the next frame boundary
  assert.equal(E.stepFrame(50, 25, -1), 40) // and to the previous one
  assert.equal(E.stepFrame(80, 25, -1), 40)
  assert.equal(E.stepFrame(0, 25, -1), 0)
  assert.equal(E.stepFrame(0, null, 1), 33)
  assert.equal(E.stepFrame(0, 1000, 1), 33) // implausible rates fall back to 30
  const p = E.normalizeProject({ clips: [clip({ id: 1, path: 'a' }), clip({ id: 2, path: 'b' })] })
  const pl = E.layout(p.clips)
  const sources = { a: { fps: 24 }, b: { fps: 60 } }
  assert.equal(E.playbackFps(p, pl, 5000, sources), 24)
  assert.equal(E.playbackFps(p, pl, 15000, sources), 60)
  p.out.fps = 30
  assert.equal(E.playbackFps(p, pl, 15000, sources), 30)
  p.out.fps = 0
  assert.equal(E.playbackFps(p, pl, 99999, sources), 24) // past the end falls back to the first clip
}

// ---- applyToAllClips / clearTransitions
{
  const tr = { kind: 'wipeleft', durationMs: 800 }
  const p = E.normalizeProject({
    clips: [
      clip({ id: 1 }),
      clip({ id: 2, transition: tr, brightness: 0.2, contrast: 1.4, saturation: 0.8, rotate: 90, flipH: true }),
      clip({ id: 3 }),
      clip({ id: 4, transition: { kind: 'fade', durationMs: 500 } }),
    ],
  })
  assert.equal(E.applyToAllClips(p, 2, 'transition'), 2) // clip 3 and 4 change; clip 1 never gets one
  assert.equal(p.clips[0].transition, null)
  assert.deepEqual(p.clips[2].transition, tr)
  assert.notEqual(p.clips[2].transition, p.clips[1].transition) // a copy, not shared
  assert.deepEqual(p.clips[3].transition, tr)
  assert.equal(E.applyToAllClips(p, 2, 'transition'), 0) // idempotent
  assert.equal(E.applyToAllClips(p, 2, 'tone'), 3)
  assert.deepEqual([p.clips[0].brightness, p.clips[3].contrast, p.clips[2].saturation], [0.2, 1.4, 0.8])
  assert.equal(E.applyToAllClips(p, 2, 'turn'), 3)
  assert.deepEqual([p.clips[0].rotate, p.clips[3].flipH, p.clips[2].flipV], [90, true, false])
  assert.equal(E.applyToAllClips(p, 99, 'tone'), 0)
  assert.equal(E.clearTransitions(p), 3)
  assert.equal(E.clearTransitions(p), 0)
}
console.log('edit ops ok')

// ---- overlayAnimLeft
{
  const o = ovl({ kind: 'image', outMs: 4000, animIn: 'slide-left', animOut: 'zoom', animMs: 1000 })
  const left = (x, l) => E.overlayAnimLeft(x, l, 4000)
  assert.deepEqual(left(o, 0), { in: 1, out: 0 })
  assert.equal(left(o, 500).in, 0.25)
  assert.deepEqual(left(o, 2000), { in: 0, out: 0 })
  assert.equal(left(o, 3500).out, 0.25)
  assert.equal(left(o, 4000).out, 1)
  assert.equal(left({ ...o, animMs: 3000 }, 1000).in, 0.25, '时长最多取素材的一半')
  assert.deepEqual(left({ ...o, animIn: 'none', animOut: 'none' }, 0), { in: 0, out: 0 })
}

// ---- movePairBy（滚动 / 滑动共用）
{
  const a = { inMs: 1000, outMs: 6000, speed: 1 }
  const b = { inMs: 2000, outMs: 9000, speed: 1 }
  const o = { ao: 6000, bi: 2000 }
  // 右移 500：前一个出点后移、后一个入点后移，两个的总长不变
  assert.deepEqual(E.movePairBy(a, b, o, 500, 20000), { aOut: 6500, bIn: 2500 })
  // 左移不能让后一个的入点低于 0
  assert.deepEqual(E.movePairBy(a, b, o, -5000, 20000), { aOut: 4000, bIn: 0 })
  // 前一个出点不能超过素材总长
  assert.deepEqual(E.movePairBy(a, b, o, 5000, 7000), { aOut: 7000, bIn: 3000 })
  // 前一个至少留 MIN_MS；后一个也是
  assert.equal(E.movePairBy({ ...a, outMs: 1500 }, b, { ao: 1500, bi: 2000 }, -9000, 20000).aOut, 1000 + E.MIN_MS)
  assert.equal(E.movePairBy(a, { inMs: 2000, outMs: 2300, speed: 1 }, o, 9000, 20000).bIn, 2300 - E.MIN_MS)
  // 倍速：时间线上的 d 毫秒对应素材里 d * speed
  assert.deepEqual(E.movePairBy({ inMs: 0, outMs: 8000, speed: 2 }, { inMs: 0, outMs: 8000, speed: 1 }, { ao: 8000, bi: 0 }, 1000, 20000), { aOut: 10000, bIn: 1000 })
}

// ---- 复合片段
{
  const mk = (id, inMs, outMs, extra = {}) => ({ ...E.normalizeProject({ clips: [{ id, path: `/m/${id}.mp4`, kind: 'video', inMs, outMs }] }).clips[0], ...extra })
  const p0 = E.normalizeProject({
    clips: [mk(1, 0, 2000), mk(2, 0, 3000, { transition: { kind: 'fade', durationMs: 500 } }), mk(3, 0, 1000)],
    overlays: [ovl({ id: 10, startMs: 2500 })],
    texts: [{ id: 11, text: 't', startMs: 2200, endMs: 3000, track: 1 }],
    audio: [{ id: 12, path: 'm.mp3', startMs: 2000, track: 1 }],
  })
  assert.equal(E.totalMs(p0.clips), 2000 + 3000 - 500 + 1000)
  // 选两个连着的主轨片段加上叠加素材、文字、配乐
  const r = E.makeCompoundIn(p0, [{ kind: 'clip', id: 2 }, { kind: 'clip', id: 3 }, { kind: 'overlay', id: 10 }, { kind: 'text', id: 11 }, { kind: 'audio', id: 12 }])
  assert.equal(typeof r, 'object')
  const p1 = r.project
  assert.deepEqual(p1.clips.map((c) => c.compound ?? 'plain'), ['plain', r.compoundId])
  const cc = p1.clips[1]
  assert.equal(cc.path, '')
  assert.equal(cc.outMs, E.totalMs([{ ...mk(2, 0, 3000), transition: null }, mk(3, 0, 1000)]), '复合片段的长度是里面主轨的总时长')
  assert.deepEqual(cc.transition, { kind: 'fade', durationMs: 500 }, '原来第一个片段进来的转场留在复合片段上')
  const comp = p1.compounds[0]
  assert.equal(comp.project.clips[0].transition, null)
  assert.deepEqual([comp.project.overlays[0].startMs, comp.project.texts[0].startMs, comp.project.texts[0].endMs, comp.project.audio[0].startMs], [Math.max(0, 2500 - 1500), 700, 1500, 500], '时间减去它在外面开始的位置')
  assert.equal(p1.overlays.length + p1.texts.length + p1.audio.length, 0, '外面不再有被收进去的叠加素材、文字、配乐')
  assert.equal(E.mediaPaths(p1).filter((x) => x === '/m/3.mp4').length, 1, '素材清单里包含子时间线用到的文件')
  // 不连着、什么都没选：给出原因
  assert.match(E.makeCompoundIn(p0, [{ kind: 'clip', id: 1 }, { kind: 'clip', id: 3 }]), /连着/)
  assert.match(E.makeCompoundIn(p0, [{ kind: 'overlay', id: 10 }]), /选中主轨/)
  // 嵌套：再把复合片段和前一个片段合成一个，是第 2 层；到第 4 层被拒绝
  const r2 = E.makeCompoundIn(p1, [{ kind: 'clip', id: 1 }, { kind: 'clip', id: cc.id }])
  assert.equal(E.compoundDepth(r2.project.compounds, r2.compoundId), 2)
  const r3 = E.makeCompoundIn(r2.project, [{ kind: 'clip', id: r2.clipId }])
  assert.equal(E.compoundDepth(r3.project.compounds, r3.compoundId), 3)
  assert.match(E.makeCompoundIn(r3.project, [{ kind: 'clip', id: r3.clipId }]), /最多嵌套/)
  // 进入、改、退出
  const e = E.enterCompoundProject(p1, cc.id)
  assert.equal(e.inner.clips.length, 2)
  assert.equal(e.inner.compounds.length, 0, '里面没有别的复合片段时，表是空的')
  e.inner.clips[1].outMs = 500 // 把里面最后一个片段剪短
  const back = E.leaveCompoundProject(e.inner, e.frame)
  assert.equal(back.compounds.length, 1)
  assert.equal(back.compounds[0].project.clips[1].outMs, 500)
  assert.equal(back.clips[1].outMs, E.totalMs(back.compounds[0].project.clips), '外面的片段跟上新的总时长')
  // 外面裁过的（出点比里面总时长短）不会被拉长
  const trimmed = E.normalizeProject(JSON.parse(JSON.stringify(p1)))
  trimmed.clips[1].outMs = 1000
  const e2 = E.enterCompoundProject(trimmed, cc.id)
  e2.inner.clips[1].outMs = 3000
  assert.equal(E.leaveCompoundProject(e2.inner, e2.frame).clips[1].outMs, 1000)
  // 里面被清空：复合片段连同外面的片段一起删掉
  const e3 = E.enterCompoundProject(p1, cc.id)
  e3.inner.clips = []
  const gone = E.leaveCompoundProject(e3.inner, e3.frame)
  assert.deepEqual([gone.clips.length, gone.compounds.length], [1, 0])
  // 两层都在里面时，保存用的最外层工程要把两层都放回去
  const deep = E.enterCompoundProject(r2.project, r2.clipId)
  const deeper = E.enterCompoundProject(deep.inner, deep.inner.clips.find((c) => c.compound !== undefined).id)
  deeper.inner.clips[0].outMs = 700
  const root = E.foldToRoot(deeper.inner, [deep.frame, deeper.frame])
  assert.equal(root.compounds.length, 2)
  assert.equal(root.clips.length, r2.project.clips.length)
  assert.equal(root.compounds.find((c) => c.id === cc.compound).project.clips[0].outMs, 700)
}
