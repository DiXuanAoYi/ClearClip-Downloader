// 设置的三方合并：设置页开着时，别处（写作窗、托盘、屏幕翻译）做的改动不能被保存覆盖。
import assert from 'node:assert/strict'
import { merge3 } from '../../src/utils/merge3.ts'

const base = { ai: { model: 'a', pool: [1] }, writing: { hotkey: 'Alt+J', pausedUntil: 0, win: [0, 0, 0, 0] }, theme: 'system' }
const ours = { ai: { model: 'b', pool: [1] }, writing: { hotkey: 'Alt+J', pausedUntil: 0, win: [0, 0, 0, 0] }, theme: 'dark' }
const theirs = { ai: { model: 'a', pool: [1] }, writing: { hotkey: 'Alt+J', pausedUntil: 99, win: [1, 2, 3, 4] }, theme: 'system' }
const m = merge3(base, ours, theirs)
assert.equal(m.ai.model, 'b')
assert.equal(m.theme, 'dark')
assert.equal(m.writing.pausedUntil, 99, '别处的改动保留')
assert.deepEqual(m.writing.win, [1, 2, 3, 4])
// 表单没改：直接用最新的
assert.deepEqual(merge3(base, base, theirs), theirs)
// 两边改同一个值：用表单的
assert.equal(merge3({ x: 1 }, { x: 2 }, { x: 3 }).x, 2)
console.log('merge3 ok')
