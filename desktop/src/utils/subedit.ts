// 字幕校对用的纯函数：拆分、合并、查找替换、整体平移、显示方式、撤销重做。不碰界面，方便单独测试。

export interface ECue {
  startMs: number
  endMs: number
  lines: string[]
}

export const clone = (c: ECue[]): ECue[] => c.map((x) => ({ ...x, lines: [...x.lines] }))

export const textOf = (c: ECue): string => c.lines.join('\n')

/** 拆成两条。`pos` 是第一行文字里的字符位置；`atMs` 不给就按字符比例分时间。 */
export function splitCue(cues: ECue[], i: number, pos: number, atMs?: number): ECue[] {
  const c = cues[i]
  if (!c) return cues
  const full = c.lines.join(' ')
  const p = Math.min(Math.max(pos, 1), Math.max(full.length - 1, 1))
  const a = full.slice(0, p).trim()
  const b = full.slice(p).trim()
  if (!a || !b) return cues
  const t = atMs && atMs > c.startMs && atMs < c.endMs ? atMs : Math.round(c.startMs + ((c.endMs - c.startMs) * p) / full.length)
  const out = clone(cues)
  out.splice(i, 1, { startMs: c.startMs, endMs: t, lines: [a] }, { startMs: t, endMs: c.endMs, lines: [b] })
  return out
}

/** 把第 i 条和下一条合成一条（文字用空格连接，CJK 之间不加空格）。 */
export function mergeNext(cues: ECue[], i: number): ECue[] {
  const a = cues[i]
  const b = cues[i + 1]
  if (!a || !b) return cues
  const join = (x: string, y: string) => (/[　-鿿＀-￯]$/.test(x) || /^[　-鿿＀-￯]/.test(y) ? x + y : `${x} ${y}`)
  const lines = a.lines.map((l, k) => join(l, b.lines[k] ?? '')).concat(b.lines.slice(a.lines.length))
  const out = clone(cues)
  out.splice(i, 2, { startMs: a.startMs, endMs: Math.max(a.endMs, b.endMs), lines })
  return out
}

export function removeCue(cues: ECue[], i: number): ECue[] {
  const out = clone(cues)
  out.splice(i, 1)
  return out
}

export function insertAfter(cues: ECue[], i: number): ECue[] {
  const out = clone(cues)
  const prev = cues[i]
  const start = prev ? prev.endMs : 0
  out.splice(i + 1, 0, { startMs: start, endMs: start + 2000, lines: [''] })
  return out
}

export interface FindOpts {
  regex?: boolean
  matchCase?: boolean
}

function matcher(find: string, o: FindOpts): RegExp | null {
  if (!find) return null
  try {
    const src = o.regex ? find : find.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    return new RegExp(src, o.matchCase ? 'g' : 'gi')
  } catch {
    return null
  }
}

/** 含有查找内容的条目序号。 */
export function findCues(cues: ECue[], find: string, o: FindOpts = {}): number[] {
  const re = matcher(find, o)
  if (!re) return []
  // 带 g 的正则 test 会记住上次位置，每次新建一个不带 g 的
  const once = new RegExp(re.source, re.flags.replace('g', ''))
  return cues.flatMap((c, i) => (once.test(textOf(c)) ? [i] : []))
}

/** 查找替换；`only` 给出时只替换这些条目。返回新数组和替换的处数。 */
export function replaceAll(cues: ECue[], find: string, to: string, o: FindOpts = {}, only?: number[]): { cues: ECue[]; count: number } {
  const re = matcher(find, o)
  if (!re) return { cues, count: 0 }
  let count = 0
  const out = cues.map((c, i) => {
    if (only && !only.includes(i)) return c
    return { ...c, lines: c.lines.map((l) => l.replace(re, () => (count++, to))) }
  })
  return { cues: count ? out : cues, count }
}

/** 整体平移（毫秒，可负）。不会小于 0。 */
export function shiftAll(cues: ECue[], ms: number): ECue[] {
  return cues.map((c) => ({ ...c, startMs: Math.max(0, c.startMs + ms), endMs: Math.max(0, c.endMs + ms) }))
}

/** 重叠的条目：把上一条的结束时间收到下一条开始之前。 */
export function fixOverlaps(cues: ECue[]): ECue[] {
  const out = clone(cues)
  for (let i = 1; i < out.length; i++) {
    if (out[i].startMs < out[i - 1].endMs && out[i].startMs > out[i - 1].startMs) out[i - 1].endMs = out[i].startMs
  }
  return out
}

/** 当前时间落在哪一条（二分查找），没有返回 -1。 */
export function cueAt(cues: { startMs: number; endMs: number }[], ms: number): number {
  let lo = 0
  let hi = cues.length - 1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const c = cues[mid]
    if (ms < c.startMs) hi = mid - 1
    else if (ms >= c.endMs) lo = mid + 1
    else return mid
  }
  return -1
}

export type View = 'both' | 'first' | 'second' | 'swap'

/** 双语字幕的显示方式：两行都显示 / 只第一行 / 只第二行 / 上下互换。 */
export function viewLines(text: string, v: View): string {
  const ls = text.split('\n')
  if (ls.length < 2) return text
  switch (v) {
    case 'first':
      return ls[0]
    case 'second':
      return ls.slice(1).join('\n')
    case 'swap':
      return [...ls.slice(1), ls[0]].join('\n')
    default:
      return text
  }
}

/** 撤销 / 重做栈（保存整份快照，字幕条目不多，足够简单可靠）。 */
export class History {
  past: ECue[][] = []
  future: ECue[][] = []
  limit: number
  constructor(limit = 100) {
    this.limit = limit
  }
  push(state: ECue[]) {
    this.past.push(clone(state))
    if (this.past.length > this.limit) this.past.shift()
    this.future = []
  }
  undo(current: ECue[]): ECue[] | null {
    const p = this.past.pop()
    if (!p) return null
    this.future.push(clone(current))
    return p
  }
  redo(current: ECue[]): ECue[] | null {
    const f = this.future.pop()
    if (!f) return null
    this.past.push(clone(current))
    return f
  }
  get canUndo() {
    return this.past.length > 0
  }
  get canRedo() {
    return this.future.length > 0
  }
}
