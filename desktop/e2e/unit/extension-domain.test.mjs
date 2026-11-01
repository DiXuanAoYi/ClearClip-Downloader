// 网页翻译的“先判断领域”：每个页面只问一次，领域说明随翻译请求发给清影，缓存按领域分开。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const calls = []
const store = {}
const ctx = vm.createContext({
  URL, URLSearchParams, console, setTimeout, clearTimeout, Date,
  api: { storage: { local: { get: async () => store, set: async (o) => Object.assign(store, o), remove: async () => {} } } },
  sleep: async () => {},
  call: async (url, body) => {
    calls.push({ url, body })
    if (body?.kind === 'domain') return { id: 'd1' }
    if (body?.kind === 'webtrans') return { id: 'w1', ctx: body.context }
    if (url.includes('id=d1')) return { state: 'done', result: { domain: 'software development', note: 'bug = defect' } }
    return { state: 'done', result: { lines: calls.filter((c) => c.body?.kind === 'webtrans').at(-1).body.lines.map((l) => ({ t: l.t, text: '译:' + l.text })) } }
  },
})
ctx.globalThis = ctx
vm.runInContext(fs.readFileSync(path.join(dir, 'trans-logic.js'), 'utf8'), ctx)
vm.runInContext(fs.readFileSync(path.join(dir, 'bg-trans.js'), 'utf8') + '\nglobalThis.__t = translateTexts', ctx)
const tr = ctx.__t
const page = { host: 'dev.example', title: 'Fix a bug', sample: 'git commit merge' }

let r = await tr(['a bug'], { target: 'zh-CN', engine: 'clearclip', page, domain: true })
assert.equal(r.domain, 'software development')
assert.deepEqual(r.texts, ['译:a bug'])
const wt = calls.filter((c) => c.body?.kind === 'webtrans').at(-1).body
assert.match(wt.context, /Domain: software development\. bug = defect/)

await tr(['another line'], { target: 'zh-CN', engine: 'clearclip', page, domain: true })
assert.equal(calls.filter((c) => c.body?.kind === 'domain').length, 1, '同一个页面只判断一次领域')

const n = calls.length
await tr(['a bug'], { target: 'zh-CN', engine: 'clearclip', page, domain: true })
assert.equal(calls.length, n, '同领域同原文走缓存')

await tr(['a bug'], { target: 'zh-CN', engine: 'clearclip', page: null, domain: true })
assert.equal(calls.filter((c) => c.body?.kind === 'webtrans').at(-1).body.context, '', '没有页面信息就不带领域，缓存键也不同，重新翻译')

const before = calls.filter((c) => c.body?.kind === 'domain').length
await tr(['xyz'], { target: 'zh-CN', engine: 'clearclip', page: { ...page, host: 'other.example' }, domain: false })
assert.equal(calls.filter((c) => c.body?.kind === 'domain').length, before, '关掉开关不判断领域')
console.log('ok')
