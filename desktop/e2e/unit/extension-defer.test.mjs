// 后台的延迟任务（bg-defer.js）：待办存进存储并设 alarm，后台被回收（内存里的定时器丢了）后还能补做。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const tick = (ms) => new Promise((r) => setTimeout(r, ms))

function memStore(data = {}) {
  return {
    data,
    get: async (k) => (k in data ? { [k]: structuredClone(data[k]) } : {}),
    set: async (o) => void Object.assign(data, structuredClone(o)),
    remove: async (k) => void delete data[k],
  }
}

/** 假的 chrome.alarms：记下创建 / 清除，并能手动“到点”。 */
function fakeAlarms() {
  const alarms = new Map()
  const listeners = []
  return {
    alarms,
    create: async (name, info) => void alarms.set(name, info),
    clear: async (name) => alarms.delete(name),
    onAlarm: { addListener: (f) => listeners.push(f) },
    ring: (name) => listeners.forEach((f) => f({ name })),
  }
}

/** 启动一个“后台”：同一份存储，新的内存状态（模拟 Service Worker 被回收后重新起来）。 */
function boot({ storage, alarms, withSession = true, handlers = {}, dead = false }) {
  const api = { storage: { local: storage.local, ...(withSession ? { session: storage.session } : {}) }, alarms }
  // dead：这个“后台”已被回收，它的定时器永远不会触发
  const ctx = vm.createContext({ api, console, setTimeout: dead ? () => 0 : setTimeout, clearTimeout, Date })
  vm.runInContext(fs.readFileSync(path.join(dir, 'bg-defer.js'), 'utf8'), ctx, { filename: 'bg-defer.js' })
  const D = vm.runInContext('ccDefer', ctx)
  for (const [k, fn] of Object.entries(handlers)) D.on(k, fn)
  return D
}

test('到点执行；执行后待办从存储和 alarm 里清掉', async () => {
  const storage = { local: memStore(), session: memStore() }
  const alarms = fakeAlarms()
  const ran = []
  const D = boot({ storage, alarms, handlers: { job: (arg) => ran.push(arg) } })
  await D.schedule('job:a', 30)
  assert.ok(alarms.alarms.has('cc-defer:job:a'), '设了 alarm 兜底')
  assert.equal(typeof storage.session.data.ccDefer['job:a'], 'number', '待办存进了会话存储')
  assert.deepEqual(ran, [])
  await tick(80)
  assert.deepEqual(ran, ['a'], '参数（冒号后面的部分）原样传入')
  assert.deepEqual(storage.session.data.ccDefer, {})
  assert.equal(alarms.alarms.has('cc-defer:job:a'), false)
})

test('去抖：重复安排会往后推，只执行一次', async () => {
  const storage = { local: memStore(), session: memStore() }
  const ran = []
  const D = boot({ storage, alarms: fakeAlarms(), handlers: { job: () => ran.push(Date.now()) } })
  await D.schedule('job:x', 60)
  await tick(30)
  await D.schedule('job:x', 60)
  await tick(30)
  await D.schedule('job:x', 60)
  await tick(40)
  assert.equal(ran.length, 0, '还在安静期')
  await tick(80)
  assert.equal(ran.length, 1)
})

test('后台被回收：内存里的定时器没了，重启后 flush 把到期的补做', async () => {
  const storage = { local: memStore(), session: memStore() }
  const alarms = fakeAlarms()
  const ran1 = []
  // 第一个后台安排了一件事，随后被回收：定时器永远不触发
  const D1 = boot({ storage, alarms, dead: true, handlers: { job: (a) => ran1.push(a) } })
  await D1.schedule('job:lost', 50)
  await tick(90)
  assert.deepEqual(ran1, [], '被回收的后台没做')
  assert.equal(typeof storage.session.data.ccDefer['job:lost'], 'number', '但待办还在存储里')
  // 重新起来的后台：启动时 flush
  const ran2 = []
  const D2 = boot({ storage, alarms, handlers: { job: (a) => ran2.push(a) } })
  await D2.flush()
  assert.deepEqual(ran2, ['lost'], '到期的待办补做了')
  assert.deepEqual(storage.session.data.ccDefer, {})
  await D2.flush()
  assert.deepEqual(ran2, ['lost'], '不会重复做')
})

test('后台被回收时还没到期：新后台重新挂上定时器，到点再做', async () => {
  const storage = { local: memStore(), session: memStore() }
  const alarms = fakeAlarms()
  const D1 = boot({ storage, alarms, dead: true, handlers: {} })
  await D1.schedule('job:soon', 80)
  const ran = []
  const D2 = boot({ storage, alarms, handlers: { job: (a) => ran.push(a) } })
  await D2.flush()
  assert.deepEqual(ran, [])
  await tick(120)
  assert.deepEqual(ran, ['soon'])
})

test('alarm 唤醒：到期的待办由 onAlarm 触发补做', async () => {
  const storage = { local: memStore(), session: memStore() }
  const alarms = fakeAlarms()
  // 旧后台留下的待办：已经过期 100ms，没有任何内存定时器
  storage.session.data.ccDefer = { 'job:old': Date.now() - 100, 'job:later': Date.now() + 60_000 }
  const ran = []
  boot({ storage, alarms, handlers: { job: (a) => ran.push(a) } })
  alarms.ring('cc-defer:job:old')
  await tick(30)
  assert.deepEqual(ran, ['old'])
  assert.ok('job:later' in storage.session.data.ccDefer, '没到期的留着')
  assert.ok(!('job:old' in storage.session.data.ccDefer))
  alarms.ring('some-other-alarm') // 别的 alarm（如 cc-poll）不处理
  await tick(10)
  assert.deepEqual(ran, ['old'])
})

test('cancel：取消后不再执行', async () => {
  const storage = { local: memStore(), session: memStore() }
  const alarms = fakeAlarms()
  const ran = []
  const D = boot({ storage, alarms, handlers: { job: (a) => ran.push(a) } })
  await D.schedule('job:c', 30)
  await D.cancel('job:c')
  await tick(70)
  assert.deepEqual(ran, [])
  assert.equal(alarms.alarms.size, 0)
})

test('处理函数抛错不会拖垮后面的待办', async () => {
  const storage = { local: memStore(), session: memStore() }
  storage.session.data.ccDefer = { 'bad:1': Date.now() - 1, 'job:2': Date.now() - 1 }
  const ran = []
  const D = boot({ storage, alarms: fakeAlarms(), handlers: { bad: () => Promise.reject(new Error('x')), job: (a) => ran.push(a) } })
  await D.flush()
  assert.deepEqual(ran, ['2'])
})

test('Firefox 旧版没有 storage.session：退回 storage.local，同样能补做', async () => {
  const storage = { local: memStore(), session: undefined }
  const alarms = fakeAlarms()
  const ran = []
  const D1 = boot({ storage, alarms, withSession: false, dead: true, handlers: { job: (a) => ran.push(a) } })
  await D1.schedule('job:ff', 40)
  assert.equal(typeof storage.local.data.ccDefer['job:ff'], 'number')
  await tick(60)
  const ran2 = []
  const D2 = boot({ storage, alarms, withSession: false, handlers: { job: (a) => ran2.push(a) } })
  await D2.flush()
  assert.deepEqual([ran, ran2], [[], ['ff']])
})

test('没有 alarms 接口（权限缺失）也不报错，只靠定时器', async () => {
  const storage = { local: memStore(), session: memStore() }
  const ran = []
  const D = boot({ storage, alarms: undefined, handlers: { job: (a) => ran.push(a) } })
  await D.schedule('job:na', 20)
  await tick(60)
  assert.deepEqual(ran, ['na'])
})
