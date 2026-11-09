// 文件翻译页的纯逻辑：页范围、页状态、任务的合并和筛选、搜索、剩余时间。界面只管画，这里的东西都能单元测试。
import type { FileTransPage, FileTransView, FtOpts, FtRecent, JobSnap } from '../types'

// ---------- 页范围（和后端 parse_ranges 一致） ----------

/** `3-12, 15`、`5-` → 区间（从 1 起，含两端）。留空或全是写错的 = null（全部）。 */
export function parseRanges(s: string): [number, number][] | null {
  const out: [number, number][] = []
  for (const raw of s.split(/[,，;；、\s]+/).map((t) => t.trim()).filter(Boolean)) {
    const tok = raw.replace(/[–—~～]/g, '-')
    const [a, b] = tok.includes('-') ? [tok.slice(0, tok.indexOf('-')).trim(), tok.slice(tok.indexOf('-') + 1).trim()] : [tok, undefined]
    if (!/^\d+$/.test(a)) continue
    const from = Number(a)
    const to = b === undefined ? from : b === '' ? Number.MAX_SAFE_INTEGER : /^\d+$/.test(b) ? Number(b) : NaN
    if (from >= 1 && to >= from) out.push([from, to])
  }
  return out.length ? out : null
}

/** 页范围写得对不对：空 = 对（全部）；有写但一个都解析不出来 = 错。 */
export const rangesValid = (s: string) => !s.trim() || parseRanges(s) !== null

// ---------- 页状态 ----------

export type PageState = 'done' | 'fail' | 'run' | 'todo'

/** 一页的状态：fail 译完了但有没译出来的段；done 都译好了；run 正在译；todo 还没轮到（或不在页范围里）。 */
export function pageState(p: FileTransPage, running: boolean, isNext: boolean): PageState {
  const skip = new Set(p.skip ?? [])
  const missing = p.trans.filter((t, i) => !t && !skip.has(i)).length
  if (p.done) return missing ? 'fail' : 'done'
  if (running && isNext) return 'run'
  return 'todo'
}

/** 每一页的状态（running 时，第一页没处理完的标成“正在译”）。 */
export function pageStates(v: FileTransView | null, running: boolean): PageState[] {
  const pages = v?.pages ?? []
  const next = pages.findIndex((p) => !p.done)
  return pages.map((p, i) => pageState(p, running, i === next))
}

export interface Counts {
  pages: number
  donePages: number
  failPages: number
  segs: number
  doneSegs: number
  /** 处理过的页里没译出来的段 */
  failedSegs: number
  /** 还没轮到的页里要译的段 + 失败的段 */
  todoSegs: number
}

export function countView(v: FileTransView | null): Counts {
  const c: Counts = { pages: 0, donePages: 0, failPages: 0, segs: 0, doneSegs: 0, failedSegs: 0, todoSegs: 0 }
  for (const p of v?.pages ?? []) {
    c.pages++
    const skip = new Set(p.skip ?? [])
    const need = p.src.length - skip.size
    const got = p.trans.filter((t, i) => !!t && !skip.has(i)).length
    c.segs += need
    c.doneSegs += got
    if (p.done) {
      c.failedSegs += need - got
      if (need - got) c.failPages++
      else c.donePages++
    } else c.todoSegs += need - got
  }
  return c
}

/** 一页里要显示的片段序号：全部 / 还没译（含失败）/ 只看失败。 */
export function segIndexes(p: FileTransPage, only: 'all' | 'todo' | 'fail'): number[] {
  const skip = new Set(p.skip ?? [])
  return p.src
    .map((_, i) => i)
    .filter((i) => {
      if (only === 'all') return true
      if (skip.has(i) || p.trans[i]) return false
      return only === 'todo' ? true : !!p.done
    })
}

/** 左边页栏里要列哪些页：全部 / 有没译的 / 有失败的。返回页序号。 */
export function railPages(v: FileTransView | null, only: 'all' | 'todo' | 'fail'): number[] {
  const pages = v?.pages ?? []
  return pages.map((_, i) => i).filter((i) => only === 'all' || segIndexes(pages[i], only).length > 0)
}

// ---------- 格式标记 / 译后检查 ----------

/** Word / PPT / Excel / EPUB 里一段有几种格式时，文字里带 `<r1>…</r1>` 标记：显示、搜索时去掉。 */
export const stripMarks = (s: string | null | undefined): string => (s ?? '').replace(/<\/?r\d{1,3}>/g, '')

/** 一页里有译后检查提示的片段数。 */
export const qaOfPage = (p: FileTransPage): number => Object.keys(p.qa ?? {}).filter((k) => !(p.skip ?? []).includes(Number(k))).length

/** 整份文件里有提示的片段数。 */
export const qaTotal = (v: FileTransView | null): number => (v?.pages ?? []).reduce((n, p) => n + qaOfPage(p), 0)

// ---------- 搜索 ----------

/** 有搜索词的页序号（原文或译文里有）。 */
export function hitPages(v: FileTransView | null, q: string): number[] {
  const k = q.trim().toLowerCase()
  if (!k) return []
  return (v?.pages ?? []).map((_, i) => i).filter((i) => v!.pages[i].src.some((s, j) => s.toLowerCase().includes(k) || (v!.pages[i].trans[j] ?? '').toLowerCase().includes(k)))
}

/** 把文字按搜索词切开：[{ t, m }]，m 为 true 的是命中的部分（界面里加亮）。 */
export function splitHits(text: string, q: string): { t: string; m: boolean }[] {
  const k = q.trim()
  if (!k) return [{ t: text, m: false }]
  const out: { t: string; m: boolean }[] = []
  const low = text.toLowerCase()
  const kl = k.toLowerCase()
  let at = 0
  for (;;) {
    const i = low.indexOf(kl, at)
    if (i < 0) break
    if (i > at) out.push({ t: text.slice(at, i), m: false })
    out.push({ t: text.slice(i, i + k.length), m: true })
    at = i + k.length
  }
  if (at < text.length) out.push({ t: text.slice(at), m: false })
  return out.length ? out : [{ t: text, m: false }]
}

// ---------- 任务 ----------

export type TaskState = 'queued' | 'running' | 'stopped' | 'failed' | 'ready' | 'generated'

export interface Task {
  /** 文件 + 目标语言 */
  key: string
  path: string
  title: string
  target: string
  opts: Partial<FtOpts>
  state: TaskState
  /** 进行中的任务编号（只有还在内存里的任务有） */
  jobId: number | null
  percent: number
  note: string
  error: string
  output: string | null
  pagesDone: number
  pagesTotal: number
  failed: number
  missing: boolean
  /** 排序用：越大越新 */
  at: number
}

export const taskKey = (path: string, target: string) => `${path}\u0000${target}`

/** 任务快照里的状态 → 任务状态。done：只预览的是“待生成”，生成过文件的是“已生成”。 */
export function stateOfJob(j: Pick<JobSnap, 'status' | 'params'>): TaskState {
  if (j.status === 'queued' || j.status === 'running') return j.status
  if (j.status === 'canceled') return 'stopped'
  if (j.status === 'failed') return 'failed'
  return j.params?.preview ? 'ready' : 'generated'
}

/**
 * 把正在进行 / 刚结束的任务和重启后还在的任务记录合成一张列表：同一个文件 + 语言只留一条，进行中的任务优先；
 * 页数、失败数从记录（或当前打开的文件的阅读器数据 `cur`）里补。
 */
export function mergeTasks(jobs: JobSnap[], recents: FtRecent[], defaultTarget: string, cur?: { key: string; counts: Counts } | null): Task[] {
  const out = new Map<string, Task>()
  const rec = new Map(recents.map((r) => [taskKey(r.path, r.target), r]))
  const sorted = [...jobs].filter((j) => j.kind === 'filetr' && j.input).sort((a, b) => a.id - b.id)
  for (const j of sorted) {
    const path = j.input as string
    const o = (j.params ?? {}) as Partial<FtOpts>
    const target = (o.target || '').trim() || defaultTarget
    const key = taskKey(path, target)
    const r = rec.get(key)
    out.set(key, {
      key,
      path,
      title: j.title,
      target,
      opts: o,
      state: stateOfJob(j),
      jobId: j.id,
      percent: j.status === 'done' ? 100 : j.percent,
      note: j.note ?? '',
      error: j.error ?? '',
      output: j.output && !o.preview ? j.output : (r?.output ?? null),
      pagesDone: r?.pagesDone ?? 0,
      pagesTotal: r?.pagesTotal ?? 0,
      failed: r?.failed ?? 0,
      missing: r?.missing ?? false,
      at: r?.at ?? j.id,
    })
  }
  for (const r of recents) {
    const key = taskKey(r.path, r.target)
    if (out.has(key)) continue
    out.set(key, {
      key,
      path: r.path,
      title: r.title,
      target: r.target,
      opts: r.opts,
      state: r.state,
      jobId: null,
      percent: r.pagesTotal ? Math.round((r.pagesDone / r.pagesTotal) * 100) : 0,
      note: '',
      error: r.error,
      output: r.output,
      pagesDone: r.pagesDone,
      pagesTotal: r.pagesTotal,
      failed: r.failed,
      missing: r.missing,
      at: r.at,
    })
  }
  // 当前打开的文件：页数、失败数以阅读器里的为准（比记录新）
  if (cur) {
    const t = out.get(cur.key)
    if (t && cur.counts.pages) {
      t.pagesDone = cur.counts.donePages + cur.counts.failPages
      t.pagesTotal = cur.counts.pages
      t.failed = cur.counts.failedSegs
    }
  }
  // 进行中的在前（排队的靠后），然后按时间新的在前
  const rank: Record<TaskState, number> = { running: 0, queued: 1, stopped: 2, failed: 2, ready: 2, generated: 3 }
  return [...out.values()].sort((a, b) => rank[a.state] - rank[b.state] || b.at - a.at)
}

export type TaskFilter = 'all' | 'active' | 'done' | 'failed'

export function filterTasks(list: Task[], f: TaskFilter): Task[] {
  switch (f) {
    case 'active':
      return list.filter((t) => t.state === 'running' || t.state === 'queued')
    case 'done':
      return list.filter((t) => t.state === 'ready' || t.state === 'generated')
    case 'failed':
      return list.filter((t) => t.state === 'failed')
    default:
      return list
  }
}

export function taskCounts(list: Task[]): Record<TaskFilter, number> {
  return { all: list.length, active: filterTasks(list, 'active').length, done: filterTasks(list, 'done').length, failed: filterTasks(list, 'failed').length }
}

/** 这个状态下卡片的主按钮（界面按它决定画什么）：none 表示只有“⋯”。 */
export function primaryAction(t: Pick<Task, 'state' | 'missing' | 'failed'>): 'cancel' | 'stop' | 'resume' | 'retry' | 'generate' | 'open' | 'none' {
  if (t.missing && t.state !== 'running' && t.state !== 'queued') return 'none'
  switch (t.state) {
    case 'queued':
      return 'cancel'
    case 'running':
      return 'stop'
    case 'stopped':
      return 'resume'
    case 'failed':
      return 'retry'
    case 'ready':
      return t.failed ? 'retry' : 'generate'
    case 'generated':
      return 'open'
  }
}

export const STATE_LABEL: Record<TaskState, string> = { queued: '排队中', running: '翻译中', stopped: '已停止', failed: '失败', ready: '已译完，待生成', generated: '已生成' }

// ---------- 剩余时间 ----------

/** 按“从 (t0, p0) 走到 (t1, p1)”估算还要多久（秒）。进度动得太少（不到 3 个百分点）或没动返回 null。 */
export function etaSeconds(t0: number, p0: number, t1: number, p1: number): number | null {
  const dp = p1 - p0
  if (dp < 3 || p1 >= 100 || t1 <= t0) return null
  return Math.round(((t1 - t0) / 1000 / dp) * (100 - p1))
}

export function etaText(sec: number | null): string {
  if (sec === null) return ''
  if (sec < 45) return '约剩不到 1 分钟'
  if (sec < 3600) return `约剩 ${Math.round(sec / 60)} 分钟`
  return `约剩 ${Math.floor(sec / 3600)} 小时 ${Math.round((sec % 3600) / 60)} 分钟`
}

// ---------- 其他 ----------

export const baseName = (p: string) => p.split(/[\\/]/).pop() || p

/** “3 页、112 段、约 3.1 万字” */
export function sizeText(pages: number, segs: number, chars: number): string {
  const c = chars >= 10000 ? `${(chars / 10000).toFixed(1)} 万字` : `${chars} 字`
  return `${pages} 页 · 约 ${segs} 段 · 约 ${c}`
}
