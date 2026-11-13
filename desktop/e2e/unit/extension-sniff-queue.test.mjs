// 嗅探记录的“读 → 改 → 写”要排队：并发写入不丢记录；清除不会被排在前面的写入又写回来。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const tick = (ms = 1) => new Promise((r) => setTimeout(r, ms))

/** 有延迟的存储：get 和 set 之间别的任务会插进来——和真实的 storage.session 一样是异步的。 */
function slowStore() {
  const data = {}
  return {
    data,
    get: async (k) => (await tick(2), k in data ? { [k]: structuredClone(data[k]) } : {}),
    set: async (o) => (await tick(2), Object.assign(data, structuredClone(o)), undefined),
    remove: async (k) => (await tick(2), void delete data[k]),
  }
}

function load() {
  const session = slowStore()
  const ctx = vm.createContext({ URL, console, setTimeout, api: { storage: { session, local: slowStore() } } })
  for (const f of ['takeover.js', 'sniff.js']) vm.runInContext(fs.readFileSync(path.join(dir, f), 'utf8'), ctx, { filename: f })
  return { session, X: (e) => vm.runInContext(e, ctx), ctx }
}
const media = (i) => ({ url: `https://cdn.example.com/v${i}.mp4`, kind: 'video', size: 5_000_000, at: 1000 + i })

test('同一个标签页并发写入 20 条：一条都不丢', async () => {
  const { X, session } = load()
  const add = X('addSniffed')
  await Promise.all(Array.from({ length: 20 }, (_, i) => add(3, media(i))))
  const got = session.data['sniff:3']
  assert.equal(got.length, 20)
  assert.deepEqual(new Set(got.map((x) => x.url)).size, 20)
})

test('超过上限时仍然只留最新的 30 条媒体，且并发下条数准确', async () => {
  const { X, session } = load()
  const add = X('addSniffed')
  const counts = await Promise.all(Array.from({ length: 45 }, (_, i) => add(1, media(i))))
  assert.equal(session.data['sniff:1'].length, 30)
  assert.equal(Math.max(...counts), 30)
  const urls = new Set(session.data['sniff:1'].map((x) => x.url))
  for (let i = 15; i < 45; i++) assert.ok(urls.has(media(i).url), `最新的 v${i} 应该在`)
})

test('同时来的 setSniffDetail 和 addSniffed 互不覆盖', async () => {
  const { X, session } = load()
  const add = X('addSniffed')
  const setDetail = X('setSniffDetail')
  await add(2, { url: 'https://cdn.example.com/a.m3u8', kind: 'hls', size: 0, at: 1 })
  await Promise.all([
    setDetail(2, 'https://cdn.example.com/a.m3u8', { durationMs: 1000, height: 720 }),
    ...Array.from({ length: 10 }, (_, i) => add(2, media(i))),
    setDetail(2, 'https://cdn.example.com/a.m3u8', { durationMs: 1000, height: 1080 }),
  ])
  const got = session.data['sniff:2']
  assert.equal(got.length, 11)
  assert.equal(got.find((x) => x.kind === 'hls').detail.height, 1080)
})

test('不同标签页各记各的', async () => {
  const { X, session } = load()
  const add = X('addSniffed')
  await Promise.all([...Array.from({ length: 5 }, (_, i) => add(10, media(i))), ...Array.from({ length: 7 }, (_, i) => add(11, media(i)))])
  assert.equal(session.data['sniff:10'].length, 5)
  assert.equal(session.data['sniff:11'].length, 7)
})

test('换页清除：排在前面的写入不会把记录又写回来', async () => {
  const { X, session } = load()
  const add = X('addSniffed')
  const clear = X('clearSniffed')
  const writes = Array.from({ length: 5 }, (_, i) => add(4, media(i)))
  const cleared = clear(4)
  await Promise.all([...writes, cleared])
  assert.equal(session.data['sniff:4'], undefined)
  await add(4, media(99)) // 清除之后的新页面照常记录
  assert.equal(session.data['sniff:4'].length, 1)
})

test('makeSerial：前一个任务失败不影响后面的，结果按顺序', async () => {
  const { X } = load()
  const q = X('makeSerial')()
  const order = []
  const a = q(async () => (await tick(5), order.push('a'), 'A'))
  const b = q(async () => (order.push('b'), Promise.reject(new Error('boom'))))
  const c = q(async () => (order.push('c'), 'C'))
  assert.equal(await a, 'A')
  await assert.rejects(b, /boom/)
  assert.equal(await c, 'C')
  assert.deepEqual(order, ['a', 'b', 'c'])
})
