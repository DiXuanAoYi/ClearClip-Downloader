// 任务进度合并更新：同一帧内多次进度合成一次、顺序不变、不丢最终状态、没变的任务引用不变。
import assert from 'node:assert/strict'
import { createProgressBatcher, mergeProgress, frameScheduler } from '../../src/utils/progressBatch.ts'

const task = (id, over = {}) => ({ id, status: 'running', percent: 0, speed: 0, ...over })

// ---- mergeProgress
{
  const a = task(1), b = task(2), c = task(3)
  const list = [a, b, c]
  const b2 = task(2, { percent: 50 })
  const out = mergeProgress(list, new Map([[2, b2]]))
  assert.notEqual(out, list, '有变化时返回新数组')
  assert.deepEqual(out.map((t) => t.id), [1, 2, 3], '顺序不变')
  assert.equal(out[0], a, '没变的任务引用不变')
  assert.equal(out[1], b2)
  assert.equal(out[2], c)
  assert.equal(list[1], b, '原数组不被修改')

  assert.equal(mergeProgress(list, new Map()), list, '没有更新时原样返回')
  assert.equal(mergeProgress(list, new Map([[99, task(99)]])), list, '列表里没有的任务忽略')
  assert.equal(mergeProgress(list, new Map([[1, a]])), list, '同一个对象不算变化')
}

// ---- 手动调度的批处理器
function manual() {
  const queue = []
  const schedule = (run) => {
    const h = { run, cancelled: false }
    queue.push(h)
    return () => { h.cancelled = true }
  }
  const tick = () => {
    const hs = queue.splice(0)
    for (const h of hs) if (!h.cancelled) h.run()
  }
  return { schedule, tick, queue }
}

// 同一帧内同一任务的多次进度只留最后一次；一帧只写一次
{
  const m = manual()
  const calls = []
  const b = createProgressBatcher((u) => calls.push([...u.values()].map((t) => `${t.id}:${t.percent}`)), m.schedule)
  for (let p = 1; p <= 100; p++) b.push(task(7, { percent: p }))
  b.push(task(3, { percent: 10 }))
  assert.equal(b.pending, 2)
  assert.equal(m.queue.length, 1, '只排了一次帧')
  assert.equal(calls.length, 0, '帧到之前不写入')
  m.tick()
  assert.deepEqual(calls, [['7:100', '3:10']], '每个任务只留最后一条，按首次到达的顺序')
  assert.equal(b.pending, 0)
}

// 写入后再来的进度进入下一帧；最终状态不丢
{
  const m = manual()
  const seen = []
  const b = createProgressBatcher((u) => seen.push(new Map(u)), m.schedule)
  b.push(task(1, { percent: 30 }))
  m.tick()
  b.push(task(1, { percent: 60 }))
  b.push(task(1, { status: 'done', percent: 100 }))
  m.tick()
  assert.equal(seen.length, 2)
  assert.equal(seen[1].get(1).status, 'done', '最后的 done 状态保留')
  m.tick()
  assert.equal(seen.length, 2, '没有新进度就不再写入')
}

// clear：完整列表到达后，没写入的旧进度作废；flush：立刻写入
{
  const m = manual()
  const seen = []
  const b = createProgressBatcher((u) => seen.push(u.size), m.schedule)
  b.push(task(1))
  b.clear()
  m.tick()
  assert.equal(seen.length, 0, 'clear 后不写入')
  assert.equal(b.pending, 0)
  b.push(task(2))
  b.push(task(3))
  b.flush()
  assert.deepEqual(seen, [2], 'flush 立刻写入')
  m.tick()
  assert.deepEqual(seen, [2], '已排的帧被取消，不会重复写入')
  b.flush()
  assert.deepEqual(seen, [2], '没有进度时 flush 什么也不做')
}

// 端到端：模拟任务列表 + 密集进度，列表引用一帧只换一次，最终状态和逐条应用一致
{
  const m = manual()
  let list = Array.from({ length: 200 }, (_, i) => task(i + 1))
  let writes = 0
  const b = createProgressBatcher((u) => { list = mergeProgress(list, u); writes++ }, m.schedule)
  const expected = new Map()
  for (let round = 1; round <= 20; round++) {
    for (let i = 1; i <= 200; i++) {
      const t = task(i, { percent: round * 5, speed: i * round })
      expected.set(i, t)
      b.push(t)
    }
  }
  m.tick()
  assert.equal(writes, 1, '4000 条进度只写一次')
  assert.deepEqual(list.map((t) => t.id), Array.from({ length: 200 }, (_, i) => i + 1), '顺序不变')
  for (const t of list) assert.deepEqual(t, expected.get(t.id), `任务 ${t.id} 的最终状态`)
}

// 默认调度器：定时器兜底（没有 requestAnimationFrame 的环境，或窗口在后台）
{
  let ran = 0
  const cancel = frameScheduler(5)(() => ran++)
  await new Promise((r) => setTimeout(r, 40))
  assert.equal(ran, 1, '没有动画帧也会触发，且只触发一次')
  cancel()
  const c2 = frameScheduler(5)(() => ran++)
  c2()
  await new Promise((r) => setTimeout(r, 40))
  assert.equal(ran, 1, '取消后不触发')
}

// 模拟有动画帧的环境：帧先到、定时器后到，只触发一次；帧不来时定时器兜底
{
  const frames = []
  globalThis.requestAnimationFrame = (cb) => (frames.push(cb), frames.length)
  globalThis.cancelAnimationFrame = () => {}
  try {
    let ran = 0
    frameScheduler(30)(() => ran++)
    frames.shift()()
    await new Promise((r) => setTimeout(r, 60))
    assert.equal(ran, 1, '帧到了就触发，定时器不再重复')
    frameScheduler(30)(() => ran++)
    await new Promise((r) => setTimeout(r, 60))
    assert.equal(ran, 2, '帧一直不来（后台窗口）时定时器兜底')
  } finally {
    delete globalThis.requestAnimationFrame
    delete globalThis.cancelAnimationFrame
  }
}
console.log('progress-batch ok')
