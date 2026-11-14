/**
 * 任务进度的合并更新。
 *
 * 后端每个下载任务每隔几十毫秒就会发一次 `tasks://progress`，任务多的时候每秒几百次。
 * 每来一次就改一次响应式列表，界面会重渲染几百次；这里把同一帧内同一任务的多次进度合成最后一次，
 * 一帧只写一次列表，没变的任务对象原样保留（引用不变）。
 */

export interface HasId {
  id: number
}

/** 用 updates 里的最新快照替换列表里同 id 的任务；没有任何任务匹配时返回原数组（引用不变，不会触发更新）。 */
export function mergeProgress<T extends HasId>(list: readonly T[], updates: ReadonlyMap<number, T>): readonly T[] {
  let out: T[] | null = null
  for (let i = 0; i < list.length; i++) {
    const next = updates.get(list[i].id)
    if (next !== undefined && next !== list[i]) {
      out ??= list.slice()
      out[i] = next
    }
  }
  return out ?? list
}

/** 一次“排队等下一帧”：返回取消函数。 */
export type Scheduler = (run: () => void) => () => void

/** 默认调度：下一帧（requestAnimationFrame）。窗口在后台或最小化时浏览器不跑动画帧，所以再加一个定时器兜底，保证状态不会停住。 */
export function frameScheduler(fallbackMs = 200): Scheduler {
  return (run) => {
    let done = false
    let raf = 0
    let timer: ReturnType<typeof setTimeout> | undefined
    const fire = () => {
      if (done) return
      done = true
      if (raf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(raf)
      if (timer !== undefined) clearTimeout(timer)
      run()
    }
    if (typeof requestAnimationFrame === 'function') raf = requestAnimationFrame(fire)
    timer = setTimeout(fire, typeof requestAnimationFrame === 'function' ? fallbackMs : 16)
    return () => {
      done = true
      if (raf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(raf)
      if (timer !== undefined) clearTimeout(timer)
    }
  }
}

export interface ProgressBatcher<T extends HasId> {
  /** 收一条进度；同一任务在下一次写入前来的多条只保留最后一条。 */
  push(t: T): void
  /** 立刻写入已收到的进度（没有就什么也不做）。 */
  flush(): void
  /** 丢掉还没写入的进度（比如来了一份完整的任务列表，比它们新）。 */
  clear(): void
  /** 还没写入的任务数。 */
  readonly pending: number
}

export function createProgressBatcher<T extends HasId>(apply: (updates: ReadonlyMap<number, T>) => void, schedule: Scheduler = frameScheduler()): ProgressBatcher<T> {
  // Map 保持首次到达的顺序；同一任务再来时只换值，不改位置
  let buf = new Map<number, T>()
  let cancel: (() => void) | null = null

  const flush = () => {
    cancel?.()
    cancel = null
    if (buf.size === 0) return
    const updates = buf
    buf = new Map()
    apply(updates)
  }
  return {
    push(t) {
      buf.set(t.id, t)
      if (!cancel) cancel = schedule(flush)
    },
    flush,
    clear() {
      cancel?.()
      cancel = null
      buf = new Map()
    },
    get pending() {
      return buf.size
    },
  }
}
