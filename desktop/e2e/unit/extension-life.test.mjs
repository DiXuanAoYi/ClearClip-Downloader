// 内容脚本的统一清理机制（ui.js 的 ccUI.life）：登记 / 执行顺序 / 出错隔离 / 孤儿检测 / sendMessage 失效时自清理。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const src = fs.readFileSync(path.join(dir, 'content/ui.js'), 'utf8')

function load({ id = 'ext', sendMessage } = {}) {
  const api = { runtime: { id, sendMessage: sendMessage || (async () => ({ ok: true })) }, storage: { onChanged: { addListener() {}, removeListener() {} }, local: {} } }
  const ctx = vm.createContext({ console, setInterval, clearInterval, setTimeout, clearTimeout, document: {}, location: { hostname: 'x.com' } })
  ctx.globalThis = ctx
  ctx.browser = api
  vm.runInContext(src, ctx, { filename: 'ui.js' })
  return { ui: ctx.ccUI, api }
}

test('disposeAll：后登记的先执行；一个出错不影响其余；只执行一次', () => {
  const { ui } = load()
  const order = []
  ui.life.add(() => order.push('a'))
  ui.life.add(() => {
    throw new Error('boom')
  })
  ui.life.add(() => order.push('c'))
  ui.life.disposeAll()
  ui.life.disposeAll()
  assert.deepEqual(order, ['c', 'a'])
  assert.equal(ui.life.alive, false)
})

test('on / interval / observe 都会在 disposeAll 时撤销', () => {
  const { ui } = load()
  const log = []
  const target = { addEventListener: (...a) => log.push(['add', a[0]]), removeEventListener: (...a) => log.push(['remove', a[0]]) }
  ui.life.on(target, 'mousemove', () => {})
  let ticks = 0
  ui.life.interval(() => ticks++, 5)
  const mo = { observe: () => log.push(['observe']), disconnect: () => log.push(['disconnect']) }
  ui.life.observe(mo, {}, {})
  ui.life.disposeAll()
  assert.deepEqual(log, [['add', 'mousemove'], ['observe'], ['disconnect'], ['remove', 'mousemove']])
  const before = ticks
  return new Promise((r) => setTimeout(r, 30)).then(() => assert.equal(ticks, before, '定时器已清除'))
})

test('注销函数：功能自己先清理时不会被重复清理；清理之后再登记会立刻执行', () => {
  const { ui } = load()
  let n = 0
  const off = ui.life.add(() => n++)
  off()
  ui.life.disposeAll()
  assert.equal(n, 0)
  ui.life.add(() => n++)
  assert.equal(n, 1)
})

test('孤儿检测：runtime.id 读不到 = 扩展已失效，check() 触发清理', () => {
  const { ui, api } = load()
  let n = 0
  ui.life.add(() => n++)
  assert.equal(ui.life.check(), true)
  api.runtime.id = undefined
  assert.equal(ui.life.orphaned(), true)
  assert.equal(ui.life.check(), false)
  assert.equal(n, 1)
})

test('sendMessage 抛 “Extension context invalidated”：自清理，并提示刷新页面', async () => {
  const { ui } = load({
    sendMessage: async () => {
      throw new Error('Extension context invalidated.')
    },
  })
  let n = 0
  ui.life.add(() => n++)
  const r = await ui.send({ type: 'x' })
  assert.equal(r.ok, false)
  assert.match(r.error, /刷新/)
  assert.equal(n, 1)
  assert.equal(ui.life.alive, false)
  // 清理之后不再打扰后台
  const r2 = await ui.send({ type: 'x' })
  assert.equal(r2.ok, false)
})

test('sendMessage 的普通失败（后台没就绪）不触发清理', async () => {
  const { ui } = load({
    sendMessage: async () => {
      throw new Error('Could not establish connection. Receiving end does not exist.')
    },
  })
  const r = await ui.send({ type: 'x' })
  assert.equal(r.ok, false)
  assert.equal(ui.life.alive, true)
})

test('最严重的几处常驻资源都登记了清理（回归：不要改回裸的 setInterval / observe）', () => {
  const read = (f) => fs.readFileSync(path.join(dir, 'content', f), 'utf8')
  assert.doesNotMatch(read('vdock.js'), /^\s*setInterval\(/m)
  assert.match(read('vdock.js'), /life\.interval\(/)
  assert.doesNotMatch(read('video-control.js'), /new MutationObserver\([^]*?\)\.observe\(document/)
  assert.match(read('video-control.js'), /life\.observe\(/)
  assert.match(read('audio-fx.js'), /life\.interval\(tick, 100\)/)
  assert.match(read('sublayer.js'), /removeEventListener\('mousemove', S\.onPtr/)
  assert.match(read('sublayer.js'), /life\.add\(api_destroy\)/)
})
