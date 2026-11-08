// 分层术语的编辑：界面上把树摊成一列“行”（每行记着第几层），所有操作都是对这一列的增删移动；保存时再还原成树。
// 一行的“下级”= 紧跟在它后面、层数比它大的那些行。和后端 glossary.rs 的规则一致：最多 6 层。
import type { GlossaryTerm } from '../types'

export const MAX_DEPTH = 6

export interface Row {
  id: number
  /** 0 = 第一层 */
  level: number
  source: string
  target: string
  note: string
  group: boolean
}

let seq = 1
export const newRow = (level = 0, o: Partial<Row> = {}): Row => ({ id: seq++, level, source: '', target: '', note: '', group: false, ...o })

/** 树 → 行 */
export function toRows(terms: GlossaryTerm[]): Row[] {
  const out: Row[] = []
  const walk = (ts: GlossaryTerm[], level: number) => {
    for (const t of ts) {
      out.push(newRow(Math.min(level, MAX_DEPTH - 1), { source: t.source, target: t.target, note: t.note, group: !!t.group }))
      if (t.children?.length) walk(t.children, level + 1)
    }
  }
  walk(terms, 0)
  return out
}

/** 行 → 树（层数跳级的按上一行的下一层算） */
export function toTree(rows: Row[]): GlossaryTerm[] {
  const root: GlossaryTerm[] = []
  const stack: { level: number; list: GlossaryTerm[] }[] = [{ level: -1, list: root }]
  for (const r of rows) {
    while (stack.length > 1 && stack[stack.length - 1].level >= r.level) stack.pop()
    const t: GlossaryTerm = { source: r.source, target: r.target, note: r.note }
    if (r.group) t.group = true
    stack[stack.length - 1].list.push(t)
    t.children = []
    stack.push({ level: r.level, list: t.children })
  }
  const prune = (ts: GlossaryTerm[]) => {
    for (const t of ts) {
      if (t.children?.length) prune(t.children)
      else delete t.children
    }
  }
  prune(root)
  return root
}

/** 第 i 行连同下级占到哪里（不含）。 */
export function subtreeEnd(rows: Row[], i: number): number {
  let j = i + 1
  while (j < rows.length && rows[j].level > rows[i].level) j++
  return j
}

/** 第 i 行的上级下标（没有返回 -1）。 */
export function parentOf(rows: Row[], i: number): number {
  for (let j = i - 1; j >= 0; j--) if (rows[j].level < rows[i].level) return j
  return -1
}

/** 从第一层到第 i 行的路径（下标）。 */
export function pathOf(rows: Row[], i: number): number[] {
  const out: number[] = []
  for (let j = i; j >= 0; j = parentOf(rows, j)) out.unshift(j)
  return out
}

const maxLevel = (rows: Row[], a: number, b: number) => rows.slice(a, b).reduce((m, r) => Math.max(m, r.level), 0)

/** 在第 i 行（连同下级）后面加一个同级；i 为 -1 时加在最后（第一层）。返回新行下标。 */
export function addSibling(rows: Row[], i: number): number {
  const at = i < 0 ? rows.length : subtreeEnd(rows, i)
  rows.splice(at, 0, newRow(i < 0 ? 0 : rows[i].level))
  return at
}

/** 给第 i 行加一个下级（放在它下级的最后）。已经是第 6 层时返回 -1。 */
export function addChild(rows: Row[], i: number): number {
  if (rows[i].level >= MAX_DEPTH - 1) return -1
  const at = subtreeEnd(rows, i)
  rows.splice(at, 0, newRow(rows[i].level + 1))
  return at
}

/** 变成上一条同级的下级（Tab）。做不到返回 false。 */
export function indent(rows: Row[], i: number): boolean {
  if (i <= 0 || rows[i - 1].level < rows[i].level) return false
  const end = subtreeEnd(rows, i)
  if (maxLevel(rows, i, end) >= MAX_DEPTH - 1) return false
  for (let j = i; j < end; j++) rows[j].level++
  return true
}

/** 升一层（Shift+Tab）：连同下级移到原来上级的后面。返回新下标，做不到返回 -1。 */
export function outdent(rows: Row[], i: number): number {
  const p = parentOf(rows, i)
  if (p < 0) return -1
  const end = subtreeEnd(rows, i)
  const block = rows.splice(i, end - i)
  for (const r of block) r.level--
  const at = subtreeEnd(rows, p)
  rows.splice(at, 0, ...block)
  return at
}

/** 和上一条 / 下一条同级对调（Alt+↑↓）。返回新下标，做不到返回 -1。 */
export function move(rows: Row[], i: number, dir: -1 | 1): number {
  const L = rows[i].level
  const end = subtreeEnd(rows, i)
  if (dir < 0) {
    let p = i - 1
    while (p >= 0 && rows[p].level > L) p--
    if (p < 0 || rows[p].level !== L) return -1
    const block = rows.splice(i, end - i)
    rows.splice(p, 0, ...block)
    return p
  }
  if (end >= rows.length || rows[end].level !== L) return -1
  const nextEnd = subtreeEnd(rows, end)
  const next = rows.splice(end, nextEnd - end)
  rows.splice(i, 0, ...next)
  return i + next.length
}

/** 删除第 i 行：withKids 连同下级一起删，否则下级升一层留在原处。返回删掉的条数。 */
export function remove(rows: Row[], i: number, withKids: boolean): number {
  const end = subtreeEnd(rows, i)
  if (withKids) {
    rows.splice(i, end - i)
    return end - i
  }
  for (let j = i + 1; j < end; j++) rows[j].level--
  rows.splice(i, 1)
  return 1
}

/** 拖动：把第 from 行（连同下级）放到第 to 行之前 / 之后 / 里面（成为最后一个下级）。返回新下标，放不下返回 -1。 */
export function drop(rows: Row[], from: number, to: number, where: 'before' | 'after' | 'inside'): number {
  const end = subtreeEnd(rows, from)
  if (to >= from && to < end) return -1
  const level = where === 'inside' ? rows[to].level + 1 : rows[to].level
  const delta = level - rows[from].level
  if (maxLevel(rows, from, end) + delta > MAX_DEPTH - 1) return -1
  const target = rows[to]
  const block = rows.splice(from, end - from)
  for (const r of block) r.level += delta
  const t = rows.indexOf(target)
  const at = where === 'before' ? t : subtreeEnd(rows, t)
  rows.splice(at, 0, ...block)
  return at
}

/** 搜索：命中的行和它们的上级（上级只用来显示路径）。 */
export function searchRows(rows: Row[], q: string): { hit: Set<number>; shown: Set<number> } {
  const k = q.trim().toLowerCase()
  const hit = new Set<number>()
  const shown = new Set<number>()
  if (!k) return { hit, shown }
  rows.forEach((r, i) => {
    if (`${r.source}\n${r.target}\n${r.note}`.toLowerCase().includes(k)) {
      hit.add(i)
      for (const j of pathOf(rows, i)) shown.add(j)
    }
  })
  return { hit, shown }
}

/** 一共几层 */
export const depthOf = (rows: Row[]) => (rows.length ? maxLevel(rows, 0, rows.length) + 1 : 0)

/** 一组术语（含下级）一共几条 */
export const countTerms = (ts: GlossaryTerm[]): number => ts.reduce((n, t) => n + 1 + countTerms(t.children ?? []), 0)
