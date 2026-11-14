import test from 'node:test'
import assert from 'node:assert/strict'
import { effectScope } from 'vue'
import { useTauriEvents } from '../../src/composables/useTauriEvent.ts'

function fakeReg() {
  let calls = 0
  const reg = () => Promise.resolve(() => void calls++)
  return { reg, calls: () => calls }
}

test('作用域销毁时自动取消监听', async () => {
  const f = fakeReg()
  const scope = effectScope()
  const ev = scope.run(() => useTauriEvents())
  await ev.on(f.reg())
  await ev.on(f.reg())
  assert.equal(f.calls(), 0)
  scope.stop()
  assert.equal(f.calls(), 2)
})

test('await 注册期间已卸载：注册完成后立刻取消', async () => {
  let calls = 0
  let resolve
  const pending = new Promise((r) => (resolve = r))
  const scope = effectScope()
  const ev = scope.run(() => useTauriEvents())
  const p = ev.on(pending)
  scope.stop()
  assert.equal(calls, 0)
  resolve(() => void calls++)
  await p
  assert.equal(calls, 1)
})

test('取消函数幂等：手动取消后卸载不重复调用', async () => {
  const f = fakeReg()
  const scope = effectScope()
  const ev = scope.run(() => useTauriEvents())
  const off = await ev.on(f.reg())
  off()
  off()
  scope.stop()
  assert.equal(f.calls(), 1)
})

test('取消函数抛错不影响其它监听', async () => {
  let ok = 0
  const scope = effectScope()
  const ev = scope.run(() => useTauriEvents())
  await ev.on(
    Promise.resolve(() => {
      throw new Error('gone')
    }),
  )
  await ev.on(Promise.resolve(() => void ok++))
  scope.stop()
  assert.equal(ok, 1)
})
