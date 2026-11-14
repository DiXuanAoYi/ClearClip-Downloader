import test from 'node:test'
import assert from 'node:assert/strict'
import { createErrorLog, describeError, isNoise } from '../../src/utils/errorLog.ts'

const quiet = () => {
  const orig = console.error
  console.error = () => {}
  return () => (console.error = orig)
}

test('describeError 兼容 Error / 字符串 / 后端 AppError / 其它', () => {
  assert.equal(describeError(new Error('boom')).message, 'boom')
  assert.equal(describeError('x').message, 'x')
  assert.equal(describeError({ kind: 'net', message: '超时' }).message, '超时')
  assert.equal(describeError(42).message, '42')
})

test('只保留最近 50 条', () => {
  const restore = quiet()
  const log = createErrorLog(() => {})
  for (let i = 0; i < 60; i++) log.record('t', new Error('e' + i), { toast: false })
  restore()
  const r = log.recent()
  assert.equal(r.length, 50)
  assert.equal(r[0].message, 'e10')
  assert.equal(r[49].message, 'e59')
})

test('同一条消息 5 秒内只提示一次，不同消息各提示', () => {
  const restore = quiet()
  let t = 1000
  const shown = []
  const log = createErrorLog((m) => shown.push(m), () => t)
  log.record('a', 'same')
  log.record('a', 'same')
  log.record('a', 'other')
  t += 6000
  log.record('a', 'same')
  restore()
  assert.deepEqual(shown, ['same', 'other', 'same'])
})

test('ResizeObserver 噪声被忽略；提示函数抛错不外泄', () => {
  const restore = quiet()
  assert.ok(isNoise('ResizeObserver loop completed with undelivered notifications.'))
  const log = createErrorLog(() => {
    throw new Error('ui not ready')
  })
  assert.equal(log.record('a', 'ResizeObserver loop limit exceeded'), null)
  assert.doesNotThrow(() => log.record('a', 'real'))
  restore()
  assert.equal(log.recent().length, 1)
})
