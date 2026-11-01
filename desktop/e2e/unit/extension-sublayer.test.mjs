// 页面字幕层的纯逻辑：定位当前句、双语排版、延迟、导出、偏好整理。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ console })
ctx.globalThis = ctx
vm.runInContext(fs.readFileSync(path.join(dir, 'sublayer-logic.js'), 'utf8'), ctx, { filename: 'sublayer-logic.js' })
const S = vm.runInContext('ccSubLayer', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

const lines = [
  { t: 1000, text: 'Hello' },
  { t: 5000, text: 'World' },
  { t: 30000, text: 'Late' },
]

// ---- 当前句：落在句内才显示，句间的空档不显示
assert.equal(S.indexAt(lines, 500), -1)
assert.equal(S.indexAt(lines, 1000), 0)
assert.equal(S.indexAt(lines, 4999), 0)
assert.equal(S.indexAt(lines, 5000), 1)
// “World” 最多挂 8 秒：13 秒时已经消失，30 秒才出现 “Late”
assert.equal(S.indexAt(lines, 14000), -1)
assert.equal(S.indexAt(lines, 30001), 2)
assert.equal(S.indexAt(lines, 34500), -1)
// 延迟：+2 秒 = 字幕晚 2 秒出现
assert.equal(S.indexAt(lines, 1500, 2000), -1)
assert.equal(S.indexAt(lines, 3000, 2000), 0)
assert.equal(S.indexAt(lines, 600, -500), 0)
assert.equal(S.indexAt([], 100), -1)

// ---- 双语排版
assert.deepEqual(plain(S.compose('Hello', '你好', 'orig-top')), ['Hello', '你好'])
assert.deepEqual(plain(S.compose('Hello', '你好', 'trans-top')), ['你好', 'Hello'])
assert.deepEqual(plain(S.compose('Hello', '你好', 'orig')), ['Hello'])
assert.deepEqual(plain(S.compose('Hello', '你好', 'trans')), ['你好'])
// 没有译文 / 译文和原文一样 → 不留空、不重复
assert.deepEqual(plain(S.compose('Hello', '', 'trans')), ['Hello'])
assert.deepEqual(plain(S.compose('OK', 'OK', 'orig-top')), ['OK'])
assert.deepEqual(plain(S.compose('', '', 'orig-top')), [])

// ---- 切换与偏好
assert.equal(S.nextMode('orig-top'), 'trans-top')
assert.equal(S.nextMode('trans'), 'orig-top')
assert.deepEqual(plain(S.clampPrefs(null)), { mode: 'orig-top', size: 100, bottom: 8, delay: 0, target: '' })
assert.deepEqual(plain(S.clampPrefs({ mode: 'x', size: 9999, bottom: -5, delay: 'a', target: 7 })), { mode: 'orig-top', size: 300, bottom: 0, delay: 0, target: '' })
assert.equal(S.clampPrefs({ delay: 999999 }).delay, 60000)

// ---- 缺译文 / 分批
assert.deepEqual(plain(S.missing(lines, ['你好', '', undefined])), [1, 2])
assert.deepEqual(plain(S.chunk([1, 2, 3, 4, 5], 2)), [[1, 2], [3, 4], [5]])

// ---- 导出：所见即所得，带延迟
const trans = ['你好', '世界', '']
assert.equal(
  S.toSrt(lines, trans, 'orig-top', 500),
  '1\n00:00:01,500 --> 00:00:05,500\nHello\n你好\n\n2\n00:00:05,500 --> 00:00:13,500\nWorld\n世界\n\n3\n00:00:30,500 --> 00:00:34,500\nLate\n',
)
assert.equal(S.toSrt(lines, trans, 'trans', 0).split('\n\n')[0], '1\n00:00:01,000 --> 00:00:05,000\n你好')
assert.equal(S.srtTime(3_723_004), '01:02:03,004')
assert.equal(S.cacheKey('yt:abc', '简体中文', 'p1'), 'yt:abc|简体中文|p1')
console.log('extension-sublayer: ok')
