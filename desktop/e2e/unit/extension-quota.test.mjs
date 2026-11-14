// 存储上限与写失败反馈：笔记、改字幕记录、双语字幕、视频缓存恢复都有条数和大小上限；写失败（配额满）会重试并提示用户；
// 后台被回收时翻译缓存的新条目不丢。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const read = (f) => fs.readFileSync(path.join(dir, f), 'utf8')
const plain = (v) => JSON.parse(JSON.stringify(v))
const run = (files, globals = {}) => {
  const ctx = vm.createContext({ URL, URLSearchParams, console: { ...console, debug() {}, warn() {} }, navigator: { language: 'zh-CN' }, setTimeout, clearTimeout, structuredClone, ...globals })
  ctx.globalThis = ctx
  for (const f of files) vm.runInContext(read(f), ctx, { filename: f })
  return ctx
}
const memStore = (data = {}) => ({
  data,
  get: async (k) => (typeof k === 'string' ? (k in data ? { [k]: structuredClone(data[k]) } : {}) : Object.fromEntries([].concat(k).filter((x) => x in data).map((x) => [x, structuredClone(data[x])]))),
  set: async (o) => void Object.assign(data, structuredClone(o)),
  remove: async (k) => void [].concat(k).forEach((x) => delete data[x]),
})

// ---------- vsum-logic：笔记、改字幕、最近使用淘汰 ----------

const V = vm.runInContext('ccVsum', run(['vsum-logic.js']))
const note = (i, len = 10) => ({ id: `n${i}`, key: 'k', t: i, text: 'x'.repeat(len), created: i })

test('笔记：条数上限 2000，留最新的', () => {
  const list = Array.from({ length: 2005 }, (_, i) => note(i))
  const out = plain(V.trimNotes(list))
  assert.equal(out.length, 2000)
  assert.equal(out[0].id, 'n5')
  assert.equal(out.at(-1).id, 'n2004')
  assert.equal(V.trimNotes(list.slice(0, 5)).length, 5, '没超限原样')
})

test('笔记：总字数上限（2000 条 × 6000 字会超过存储配额，所以也按大小限）', () => {
  const list = Array.from({ length: 2000 }, (_, i) => note(i, 6000))
  const out = plain(V.trimNotes(list))
  const bytes = JSON.stringify(out).length
  assert.ok(out.length < 2000 && out.length > 100)
  assert.ok(bytes <= 3_000_000 + 200, `${bytes}`)
  assert.equal(out.at(-1).id, 'n1999', '丢的是最早的')
  // 一条就很大也至少留下这一条（刚存的不能被自己挤掉）
  assert.equal(V.trimNotes([note(1, 10)], 2000, 5).length, 1)
})

test('addNote 走同一个上限；同一条不重复', () => {
  let list = []
  for (let i = 0; i < 700; i++) list = plain(V.addNote(list, { ...note(i, 6000), key: `v${i}` }))
  assert.ok(JSON.stringify(list).length <= 3_000_000 + 200)
  assert.equal(list.at(-1).t, 699)
  assert.equal(V.addNote(list, list.at(-1)).length, list.length, '重复的不加')
})

test('改字幕记录：每个视频最多 2000 条，丢最早的', () => {
  const m = Object.fromEntries(Array.from({ length: 2100 }, (_, i) => [`${i}|a`, `b${i}`]))
  const out = plain(V.trimEdits(m))
  assert.equal(Object.keys(out).length, 2000)
  assert.ok(!('0|a' in out) && '2099|a' in out)
  assert.equal(V.trimEdits({ '1|a': 'b' })['1|a'], 'b')
})

test('lruTrim：按键顺序（最新在最后）淘汰，条数和大小两个上限', () => {
  const all = {}
  for (let i = 0; i < 15; i++) all[`k${i}`] = ['x'.repeat(100)]
  assert.deepEqual(Object.keys(plain(V.lruTrim(all, 12))), Array.from({ length: 12 }, (_, i) => `k${i + 3}`))
  const bySize = plain(V.lruTrim(all, 100, 350)) // 每条约 106 字节：只留 3 条
  assert.deepEqual(Object.keys(bySize), ['k12', 'k13', 'k14'])
  // “用过”：先 delete 再放回去，就排到最后，不会先被淘汰
  const touched = { ...all }
  const v = touched.k0
  delete touched.k0
  touched.k0 = v
  assert.ok('k0' in plain(V.lruTrim(touched, 12)))
  assert.ok(!('k1' in plain(V.lruTrim(touched, 12))))
})

test('vsumSessions 的 pruneCache 确实生效（条数和大小）', () => {
  const e = Object.fromEntries(Array.from({ length: 60 }, (_, i) => [`s${i}`, { ts: i, data: 'x'.repeat(50_000) }]))
  const out = plain(V.pruneCache(e, 40, 2_500_000))
  assert.ok(Object.keys(out).length <= 40 && JSON.stringify(out).length <= 2_500_000)
  assert.ok('s59' in out && !('s0' in out))
})

// ---------- 视频缓存恢复（bg-vcache.js） ----------

function bgVcache(local) {
  const ctx = run(['bg-vcache.js'], {
    api: { storage: { local }, tabs: { onUpdated: { addListener() {} } }, runtime: { getURL: (x) => x } },
    call: async () => ({}),
    flashBadge: async () => {},
  })
  return ctx
}

test('视频缓存恢复：摘要会话与双语字幕写回时受条数和大小限制，恢复的排在最新', async () => {
  const local = memStore({
    vsumSessions: Object.fromEntries(Array.from({ length: 40 }, (_, i) => [`s${i}`, { ts: 1000 + i, key: `s${i}`, data: { x: 'y'.repeat(1000) } }])),
    subLayerTrans: Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`t${i}`, ['译'.repeat(200)]])),
  })
  const ctx = bgVcache(local)
  const r = await vm.runInContext('vcacheRestore', ctx)({ summary: { key: 'new', ts: 5, data: { res: {} }, vk: 'v', url: 'https://a/' }, subs: { key: 'newt', trans: ['一', '二'] } })
  assert.deepEqual(plain(r), { summary: true, subs: true })
  const sess = local.data.vsumSessions
  assert.equal(Object.keys(sess).length, 40, '总数仍是 40：丢了最旧的一个')
  assert.ok('new' in sess && !('s0' in sess))
  const trans = Object.keys(local.data.subLayerTrans)
  assert.equal(trans.length, 12)
  assert.equal(trans.at(-1), 'newt')
  assert.ok(!trans.includes('t0'))
})

test('视频缓存恢复：单个超大会话不会撑爆存储（总大小 2.5MB）', async () => {
  const big = Object.fromEntries(Array.from({ length: 10 }, (_, i) => [`s${i}`, { ts: i, key: `s${i}`, data: 'z'.repeat(600_000) }]))
  const local = memStore({ vsumSessions: big })
  const ctx = bgVcache(local)
  await vm.runInContext('vcacheRestore', ctx)({ summary: { key: 'n', data: { a: 1 } } })
  assert.ok(JSON.stringify(local.data.vsumSessions).length <= 2_500_000 + 1000)
  assert.ok('n' in local.data.vsumSessions)
})

// ---------- ui.js：写失败的反馈 ----------

function uiCtx(setImpl) {
  const toasts = []
  const doc = { createElement: () => ({ style: {}, append() {}, remove() {}, setAttribute() {} }), documentElement: {}, addEventListener() {} }
  const local = { ...memStore(), set: setImpl || memStore().set }
  const ctx = run(['content/ui.js'], { document: doc, api: undefined, chrome: { storage: { local, onChanged: { addListener() {} } }, runtime: { sendMessage: async () => ({}) } }, location: { hostname: 'a.com' } })
  const UI = ctx.ccUI
  return { UI, local, toasts }
}

test('storeSet：成功返回 true；失败（非配额）提示一次并返回 false', async () => {
  let n = 0
  const { UI } = uiCtx(async () => {
    n++
    throw new Error('disk error')
  })
  const shown = []
  const ui = { add: (el) => shown.push(el) }
  // toast 需要 DOM 创建节点：这里只检查 saveFailed 的节流与返回值
  const first = UI.saveFailed(null, 'x', new Error('e'))
  assert.equal(first, false, '没有界面可提示时只记日志')
  assert.equal(await UI.storeSet(ui, { a: 1 }), false)
  assert.equal(n, 1)
})

test('storeSet：配额满时先清可再生成的缓存再重试一次', async () => {
  const calls = []
  const removed = []
  let fail = true
  const local = {
    set: async (o) => {
      calls.push(Object.keys(o))
      if (fail) {
        fail = false
        throw new Error('QUOTA_BYTES quota exceeded')
      }
    },
    remove: async (k) => void removed.push(...[].concat(k)),
  }
  const ctx = run(['content/ui.js'], { document: { createElement: () => ({ style: {} }) }, chrome: { storage: { local, onChanged: { addListener() {} } }, runtime: { sendMessage: async () => ({}) } }, location: { hostname: 'a.com' } })
  const ok = await ctx.ccUI.storeSet(null, { vsumNotes: [1] })
  assert.equal(ok, true)
  assert.deepEqual(calls, [['vsumNotes'], ['vsumNotes']])
  assert.deepEqual(removed, ['vsumCache', 'transCache'])
  // 要写的就是缓存本身时，不能把它自己清掉
  fail = true
  removed.length = 0
  await ctx.ccUI.storeSet(null, { vsumCache: {} })
  assert.deepEqual(removed, ['transCache'])
})

test('storeSet：配额满且重试仍失败 → saveFailed 给界面一个错误提示（30 秒内只提示一次）', async () => {
  const local = {
    set: async () => {
      throw new Error('QUOTA_BYTES quota exceeded')
    },
    remove: async () => {},
  }
  const nodes = []
  const el = () => ({ style: {}, className: '', textContent: '', remove() {}, setAttribute() {}, append() {}, addEventListener() {} })
  const ctx = run(['content/ui.js'], { document: { createElement: el }, chrome: { storage: { local, onChanged: { addListener() {} } }, runtime: { sendMessage: async () => ({}) } }, location: { hostname: 'a.com' } })
  const ui = { add: (n) => nodes.push(n) }
  assert.equal(await ctx.ccUI.storeSet(ui, { vsumNotes: [1] }), false)
  assert.equal(nodes.length, 1, '提示了一次')
  assert.match(String(nodes[0].textContent || nodes[0].text || ''), /存储空间满了/)
  assert.equal(await ctx.ccUI.storeSet(ui, { vsumNotes: [2] }), false)
  assert.equal(nodes.length, 1, '30 秒内不重复提示')
})

// ---------- 后台被回收时翻译缓存的新条目不丢（bg-trans.js + bg-defer.js）----------

test('翻译缓存：新条目先记进会话存储，后台重启后由存盘任务并进缓存并落盘', async () => {
  const local = memStore()
  const session = memStore()
  const boot = (dead) => {
    const api = { storage: { local, session } }
    const ctx = run(['trans-logic.js'], { api, sleep: async () => {}, call: async () => ({}) })
    vm.runInContext('const sessionStore = api.storage.session', ctx)
    ctx.setTimeout = dead ? () => 0 : setTimeout
    for (const f of ['sniff.js', 'bg-defer.js', 'bg-trans.js']) vm.runInContext(read(f), ctx, { filename: f })
    return ctx
  }
  const a = boot(true) // 这个后台随后被回收，存盘定时器不会触发
  await vm.runInContext('trRemember', a)({ 'google|zh|hello': ['你好', 1] })
  assert.deepEqual(plain(session.data.trDirty), { 'google|zh|hello': ['你好', 1] })
  assert.equal(local.data.transCache, undefined, '还没落盘')
  assert.ok(session.data.ccDefer['tr-flush'] > Date.now(), '存盘任务已安排（2 秒后）')
  session.data.ccDefer['tr-flush'] = Date.now() - 1 // 时间过去了 2 秒，后台没来得及做
  const b = boot(false) // 重新起来的后台
  await vm.runInContext('ccDefer', b).flush()
  assert.deepEqual(plain(local.data.transCache), { 'google|zh|hello': ['你好', 1] })
  assert.equal(session.data.trDirty, undefined)
})

test('后台里的内存定时器都换成了 ccDefer（待办持久化 + alarm 兜底）', () => {
  const bgActions = read('bg-actions.js')
  assert.ok(!/menuTimer|extPushTimer/.test(bgActions))
  assert.match(bgActions, /ccDefer\.schedule\('menus'/)
  assert.match(bgActions, /ccDefer\.schedule\('ext-push'/)
  assert.ok(!/trFlushTimer/.test(read('bg-trans.js')))
  const bg = read('background.js')
  assert.match(bg, /ccDefer\.schedule\(`cookie-sync:/)
  assert.ok(!/const pending = new Map/.test(bg))
  assert.match(bg, /ccDefer\.flush\(\)/, '后台起来时补做')
})
