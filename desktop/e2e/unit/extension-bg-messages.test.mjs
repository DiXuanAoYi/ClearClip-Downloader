// 后台脚本整体加载（假的浏览器接口）后，消息入口的行为：发送者检查、needTab、字段白名单、超限拒绝，以及合规的请求照常交给处理函数。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const manifest = JSON.parse(fs.readFileSync(path.join(dir, 'manifest.json'), 'utf8'))
const plain = (v) => JSON.parse(JSON.stringify(v))

function memStore(data = {}) {
  const pick = (k) => (k == null ? Object.keys(data) : typeof k === 'string' ? [k] : Array.isArray(k) ? k : Object.keys(k))
  return {
    data,
    get: async (k) => Object.fromEntries(pick(k).filter((x) => x in data).map((x) => [x, structuredClone(data[x])])),
    set: async (o) => void Object.assign(data, structuredClone(o)),
    remove: async (k) => void [].concat(k).forEach((x) => delete data[x]),
  }
}

function boot() {
  const listeners = []
  const fetchCalls = []
  const local = memStore({ port: 5301, token: 'tok', device: 'dev1' })
  const session = memStore()
  const noop = new Proxy(function () {}, { get: (t, k) => (k in t ? t[k] : k === 'then' ? undefined : noop), apply: () => Promise.resolve(undefined) })
  const api = new Proxy(
    {
      runtime: { id: 'self-id', getURL: (x) => x, onMessage: { addListener: (f) => listeners.push(f) }, onInstalled: { addListener() {} }, onStartup: { addListener() {} } },
      storage: { local, session, onChanged: { addListener() {} } },
      alarms: { create: async () => {}, clear: async () => {}, onAlarm: { addListener() {} } },
      scripting: undefined,
    },
    { get: (t, k) => (k in t ? t[k] : noop) },
  )
  const ctx = vm.createContext({
    console: { ...console, warn() {}, debug() {} }, URL, URLSearchParams, setTimeout, clearTimeout, structuredClone,
    navigator: { userAgent: 'Chrome' },
    fetch: async (url, init) => {
      fetchCalls.push({ url, body: init?.body ? JSON.parse(init.body) : null })
      return { ok: true, status: 200, json: async () => ({ id: 'job1', state: 'done' }) }
    },
  })
  ctx.globalThis = ctx
  ctx.browser = api
  for (const f of manifest.background.scripts) vm.runInContext(fs.readFileSync(path.join(dir, f), 'utf8'), ctx, { filename: f })
  // 后台启动时自己会轮询清影（pollOnce），所以只看消息引出的请求：ai/run、job、cancel
  const ai = () => fetchCalls.filter((c) => /\/api\/v1\/ai\/(run|job|cancel)/.test(c.url))
  return { listeners, fetchCalls, ai, session, send: send(listeners) }
}

// 把监听器当成 runtime.onMessage 用：返回 { handled, response }
function send(listeners) {
  return (msg, sender) =>
    new Promise((resolve) => {
      let answered = false
      const done = (response) => {
        answered = true
        resolve({ handled: true, response: plain(response) })
      }
      let async = false
      for (const l of listeners) async = l(msg, sender, done) === true || async
      if (!async && !answered) resolve({ handled: false })
    })
}

const tab = { id: 7, url: 'https://example.com/v', title: 'T', windowId: 1 }

test('manifest.background.scripts 全部能加载（顺序里每个依赖都在前面）', () => {
  assert.ok(boot().listeners.length >= 2)
})

test('别的扩展发来的消息被拒绝，处理函数没有被调用（没有网络请求）', async () => {
  const b = boot()
  const r = await b.send({ type: 'cc-ai-run', req: { kind: 'summary', lines: [{ t: 0, text: 'x' }] } }, { id: 'other-ext', tab })
  assert.equal(r.response.ok, false)
  assert.equal(r.response.code, 'forbidden')
  assert.equal(b.ai().length, 0)
})

test('扩展页面（没有 tab）发 needTab 的消息：统一报错，不再是没有回应', async () => {
  const b = boot()
  const r = await b.send({ type: 'cc-ai-job', id: 'x' }, { id: 'self-id' })
  assert.equal(r.response.code, 'no_tab')
  assert.equal(b.ai().length, 0)
})

test('越界 / 缺字段 / 类型不对：拒绝并返回 bad_request，不会转发给清影', async () => {
  const b = boot()
  for (const m of [
    { type: 'cc-ai-run', req: { kind: 'translate', text: 'a'.repeat(600_001) } },
    { type: 'cc-ai-run', req: { title: 'no kind' } },
    { type: 'cc-ai-run' },
    { type: 'cc-ai-job' },
    { type: 'cc-tr', texts: 'not a list' },
  ]) {
    const r = await b.send(m, { id: 'self-id', tab })
    assert.equal(r.response.ok, false, JSON.stringify(m).slice(0, 80))
    assert.equal(r.response.code, 'bad_request')
  }
  assert.equal(b.ai().length, 0)
})

test('合规的 cc-ai-run：只把白名单字段转发给清影', async () => {
  const b = boot()
  const r = await b.send({ type: 'cc-ai-run', req: { kind: 'summary', title: 'T', lines: [{ t: 0, text: 'hi', x: 1 }], evil: 'drop me', terms: [1] } }, { id: 'self-id', tab })
  assert.equal(r.response.ok, true)
  assert.equal(r.response.id, 'job1')
  assert.equal(b.ai().length, 1)
  assert.match(b.ai()[0].url, /\/api\/v1\/ai\/run$/)
  assert.deepEqual(b.ai()[0].body, { kind: 'summary', title: 'T', lines: [{ t: 0, text: 'hi' }] })
})

test('没登记 / 没有处理函数的消息类型不拦截（别的监听器照常处理）', async () => {
  const b = boot()
  assert.equal((await b.send({ type: 'cc-audio', action: 'stat' }, { id: 'self-id', tab })).handled, false)
  assert.equal((await b.send({ type: 'constructor' }, { id: 'self-id', tab })).handled, false)
})

test('cc-bypass：只认本扩展发来的', async () => {
  const b = boot()
  await b.send({ type: 'cc-bypass' }, { id: 'other-ext', tab })
  assert.equal(b.session.data.bypassUntil, undefined)
  await b.send({ type: 'cc-bypass' }, { id: 'self-id', tab })
  assert.ok(b.session.data.bypassUntil > Date.now())
})
