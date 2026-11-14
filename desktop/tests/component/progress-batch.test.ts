import { describe, it, expect, vi } from 'vitest'
import { createProgressBatcher, mergeProgress, frameScheduler, type Scheduler } from '../../src/utils/progressBatch'

interface T { id: number; received: number }

describe('mergeProgress', () => {
  it('替换同 id 的任务，其余对象引用不变', () => {
    const a = { id: 1, received: 0 }
    const b = { id: 2, received: 0 }
    const b2 = { id: 2, received: 9 }
    const out = mergeProgress([a, b], new Map([[2, b2]]))
    expect(out).toEqual([a, b2])
    expect(out[0]).toBe(a)
  })
  it('没有匹配（或同一对象）时返回原数组', () => {
    const list = [{ id: 1, received: 0 }]
    expect(mergeProgress(list, new Map([[5, { id: 5, received: 1 }]]))).toBe(list)
    expect(mergeProgress(list, new Map([[1, list[0]]]))).toBe(list)
    expect(mergeProgress(list, new Map())).toBe(list)
  })
})

describe('createProgressBatcher（手动调度器）', () => {
  function manual() {
    const queue: Array<() => void> = []
    const cancelled = vi.fn()
    const schedule: Scheduler = (run) => {
      queue.push(run)
      return () => {
        cancelled()
        const i = queue.indexOf(run)
        if (i >= 0) queue.splice(i, 1)
      }
    }
    return { schedule, queue, cancelled, tick: () => queue.splice(0).forEach((f) => f()) }
  }

  it('同一帧内同一任务只保留最后一条，保持首次到达顺序', () => {
    const m = manual()
    const apply = vi.fn()
    const b = createProgressBatcher<T>(apply, m.schedule)
    b.push({ id: 1, received: 1 })
    b.push({ id: 2, received: 1 })
    b.push({ id: 1, received: 5 })
    expect(b.pending).toBe(2)
    expect(m.queue).toHaveLength(1) // 只排了一次
    m.tick()
    expect(apply).toHaveBeenCalledTimes(1)
    const updates = apply.mock.calls[0][0] as Map<number, T>
    expect([...updates.keys()]).toEqual([1, 2])
    expect(updates.get(1)?.received).toBe(5)
    expect(b.pending).toBe(0)
  })

  it('flush 立即写入并取消排队；空缓冲不调用 apply', () => {
    const m = manual()
    const apply = vi.fn()
    const b = createProgressBatcher<T>(apply, m.schedule)
    b.flush()
    expect(apply).not.toHaveBeenCalled()
    b.push({ id: 1, received: 1 })
    b.flush()
    expect(apply).toHaveBeenCalledTimes(1)
    expect(m.cancelled).toHaveBeenCalled()
    m.tick()
    expect(apply).toHaveBeenCalledTimes(1)
  })

  it('clear 丢弃未写入的进度', () => {
    const m = manual()
    const apply = vi.fn()
    const b = createProgressBatcher<T>(apply, m.schedule)
    b.push({ id: 1, received: 1 })
    b.clear()
    expect(b.pending).toBe(0)
    m.tick()
    expect(apply).not.toHaveBeenCalled()
    b.push({ id: 1, received: 2 })
    m.tick()
    expect(apply).toHaveBeenCalledTimes(1)
  })
})

describe('frameScheduler', () => {
  it('后台不跑动画帧时，定时器兜底仍会执行，且只执行一次', () => {
    vi.useFakeTimers()
    const raf = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(() => 1)
    const run = vi.fn()
    frameScheduler(50)(run)
    vi.advanceTimersByTime(49)
    expect(run).not.toHaveBeenCalled()
    vi.advanceTimersByTime(2)
    expect(run).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1000)
    expect(run).toHaveBeenCalledTimes(1)
    raf.mockRestore()
    vi.useRealTimers()
  })

  it('取消后不再执行', () => {
    vi.useFakeTimers()
    vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(() => 1)
    vi.spyOn(globalThis, 'cancelAnimationFrame').mockImplementation(() => {})
    const run = vi.fn()
    const cancel = frameScheduler(50)(run)
    cancel()
    vi.advanceTimersByTime(500)
    expect(run).not.toHaveBeenCalled()
    vi.useRealTimers()
  })
})
