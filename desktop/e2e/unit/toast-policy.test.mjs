import test from 'node:test'
import assert from 'node:assert/strict'
import { toastDuration, toastRole } from '../../src/components/ui/toast-policy.ts'

test('错误不自动消失，警告 6 秒，其余 3 秒（带操作 6 秒）', () => {
  assert.equal(toastDuration('error'), 0)
  assert.equal(toastDuration('warning'), 6000)
  assert.equal(toastDuration('success'), 3000)
  assert.equal(toastDuration('info', true), 6000)
})

test('错误用 alert，其余 status', () => {
  assert.equal(toastRole('error'), 'alert')
  assert.equal(toastRole('info'), 'status')
})
