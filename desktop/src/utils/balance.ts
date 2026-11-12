/** 设置页卡片排布：把一组高度分到 k 列。返回每个下标所在的列号。 */
export type LayoutMode = 'balance' | 'order' | 'single'

export function balanceColumns(hs: number[], k: number, mode: LayoutMode): number[] {
  const n = hs.length
  if (k <= 1 || mode === 'single' || n <= 1) return hs.map(() => 0)
  if (mode === 'order') return orderColumns(hs, k)
  return balanced(hs, k)
}

/** 严格按顺序：每一张放进当前最矮的一列。 */
function orderColumns(hs: number[], k: number): number[] {
  const load = Array<number>(k).fill(0)
  return hs.map((h) => {
    let c = 0
    for (let j = 1; j < k; j++) if (load[j] < load[c]) c = j
    load[c] += h
    return c
  })
}

/** 让最高的一列尽量矮。两列、卡片不多时枚举所有分法（最优）；否则先按高度从大到小贪心，再两两交换改进。 */
function balanced(hs: number[], k: number): number[] {
  const n = hs.length
  if (k === 2 && n <= 18) {
    const total = hs.reduce((a, b) => a + b, 0)
    let best = 0
    let bestScore = Infinity
    for (let m = 0; m < 1 << n; m++) {
      let a = 0
      for (let i = 0; i < n; i++) if (m & (1 << i)) a += hs[i]
      const score = Math.max(a, total - a)
      if (score < bestScore) {
        bestScore = score
        best = m
      }
    }
    const col = hs.map((_, i) => (((best >> i) & 1) ? 0 : 1))
    // 第 0 张卡片放第一列，视觉上从左上角开始
    return col[0] === 0 ? col : col.map((c) => 1 - c)
  }
  const order = hs.map((_, i) => i).sort((a, b) => hs[b] - hs[a])
  const load = Array<number>(k).fill(0)
  const res = Array<number>(n).fill(0)
  for (const i of order) {
    let c = 0
    for (let j = 1; j < k; j++) if (load[j] < load[c]) c = j
    res[i] = c
    load[c] += hs[i]
  }
  for (let pass = 0; pass < 30; pass++) {
    let moved = false
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        if (res[i] === res[j]) continue
        const a = res[i]
        const b = res[j]
        const d = hs[i] - hs[j]
        const before = Math.max(...load)
        load[a] -= d
        load[b] += d
        if (Math.max(...load) < before) {
          res[i] = b
          res[j] = a
          moved = true
        } else {
          load[a] += d
          load[b] -= d
        }
      }
    }
    if (!moved) break
  }
  // 第 0 张卡片放第一列：把列号按第 0 张所在列重新编号
  const first = res[0]
  return res.map((c) => (c === first ? 0 : c === 0 ? first : c))
}
