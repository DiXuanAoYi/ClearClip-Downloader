import test from 'node:test'
import assert from 'node:assert/strict'
import { balanceColumns } from '../../src/utils/balance.ts'

const load = (hs, a, k) => Array.from({ length: k }, (_, c) => hs.reduce((s, h, i) => s + (a[i] === c ? h : 0), 0))
const tallest = (hs, a, k) => Math.max(...load(hs, a, k))

test('单列 / 一列 / 一张卡片：都放第一列', () => {
  assert.deepEqual(balanceColumns([5, 3, 2], 1, 'balance'), [0, 0, 0])
  assert.deepEqual(balanceColumns([5, 3, 2], 3, 'single'), [0, 0, 0])
  assert.deepEqual(balanceColumns([9], 2, 'balance'), [0])
  assert.deepEqual(balanceColumns([], 2, 'balance'), [])
})

test('按顺序：每一张放进当前最矮的一列', () => {
  // 5→列0；3→列1；2→列1（3<5）；4→列1 或 列0？此时 列0=5 列1=5，取前一个（列0）
  assert.deepEqual(balanceColumns([5, 3, 2, 4], 2, 'order'), [0, 1, 1, 0])
})

test('自动平衡：最高一列不比按顺序的更高，第 0 张在第一列', () => {
  const sets = [
    [4, 9, 3, 7, 2, 6, 5, 8, 1, 4, 3, 6, 5, 2],
    [10, 1, 1, 1, 1, 1],
    [3, 3, 3, 3, 3, 3, 3],
    [8, 2, 6, 4, 5, 5, 3, 7, 1, 9, 2, 2, 6, 6, 4, 3, 5, 7, 1, 8, 2, 4],
  ]
  for (const hs of sets) {
    for (const k of [2, 3, 4]) {
      const a = balanceColumns(hs, k, 'balance')
      const o = balanceColumns(hs, k, 'order')
      assert.equal(a.length, hs.length)
      assert.ok(a.every((c) => c >= 0 && c < k))
      assert.equal(a[0], 0, `第 0 张在第一列 k=${k}`)
      assert.ok(tallest(hs, a, k) <= tallest(hs, o, k), `k=${k} ${tallest(hs, a, k)} > ${tallest(hs, o, k)}`)
    }
  }
})

test('两列：卡片不多时是最优分法', () => {
  const hs = [7, 5, 4, 3, 3, 2]
  const a = balanceColumns(hs, 2, 'balance')
  assert.equal(tallest(hs, a, 2), 12) // 总和 24，能分成 12 + 12
})
