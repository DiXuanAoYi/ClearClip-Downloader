// 网页翻译的纯逻辑：语言、判断要不要翻译、分批、设置、站点名单、谷歌返回解析、缓存。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ URL, URLSearchParams, console })
ctx.globalThis = ctx
vm.runInContext(fs.readFileSync(path.join(dir, 'trans-logic.js'), 'utf8'), ctx, { filename: 'trans-logic.js' })
const T = vm.runInContext('ccTrans', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// ---- 语言
assert.ok(T.LANGS.length >= 30)
assert.equal(new Set(T.LANGS.map((l) => l.code)).size, T.LANGS.length, '语言代码不重复')
assert.equal(T.promptName('ja'), 'Japanese')
assert.equal(T.promptName('简体中文'), 'Simplified Chinese')
assert.equal(T.promptName('Klingon'), 'Klingon')
assert.equal(T.nameOf('ja'), '日本語')
assert.equal(T.nameOf('xx'), 'xx')

// ---- 是否已是目标语言 / 值不值得翻译
assert.equal(T.worthTranslating('Hello world, this is English text', 'zh-CN'), true)
assert.equal(T.worthTranslating('这是一段完全中文的内容，不需要翻译', 'zh-CN'), false)
assert.equal(T.worthTranslating('これは日本語の文章です', 'ja'), false)
assert.equal(T.worthTranslating('これは日本語の文章です', 'zh-CN'), true)
assert.equal(T.worthTranslating('Hello world, this is English text', 'en'), false)
assert.equal(T.worthTranslating('12345', 'zh-CN'), false)
assert.equal(T.worthTranslating('https://example.com/a/b', 'zh-CN'), false)
assert.equal(T.worthTranslating('a@b.com', 'zh-CN'), false)
assert.equal(T.worthTranslating('x', 'zh-CN'), false)
assert.equal(T.pageInTarget('这是一个完全由中文写成的页面', 'zh-CN'), true)
assert.equal(T.pageInTarget('An English page about news', 'zh-CN'), false)

// ---- 分批
const items = Array.from({ length: 40 }, (_, i) => ({ id: i, text: 'x'.repeat(100) }))
const plan = T.batchPlan(items, { maxChars: 1000, maxItems: 16 })
assert.ok(plan.every((b) => b.length <= 10))
assert.equal(plan.flat().length, 40)
assert.deepEqual(plain(plan.flat().map((i) => i.id)), plain(items.map((i) => i.id)), '顺序不变')
assert.equal(T.batchPlan([{ id: 1, text: 'y'.repeat(5000) }, { id: 2, text: 'z' }], { maxChars: 1000 }).length, 2, '超长单条独占一批')
assert.deepEqual(plain(T.batchPlan([])), [])

// ---- 设置
const d = T.normalizeCfg(undefined)
assert.equal(d.target, 'zh-CN')
assert.equal(d.mode, 'bilingual')
assert.equal(d.engine, 'clearclip')
const n = T.normalizeCfg({ target: ' fr ', mode: 'nope', engine: 'google', hover: 'Alt', select: false, auto: 'a.com, *.B.org\nc.net', never: ['X.com'], lazy: false })
assert.equal(n.target, 'fr')
assert.equal(n.mode, 'bilingual')
assert.equal(n.engine, 'google')
assert.equal(n.hover, 'Alt')
assert.equal(n.select, false)
assert.equal(n.lazy, false)
assert.deepEqual(plain(n.auto), ['a.com', 'b.org', 'c.net'])
assert.deepEqual(plain(n.never), ['x.com'])

// ---- 站点名单
const cfg = T.normalizeCfg({ auto: ['example.com'], never: ['bad.example.com'] })
assert.equal(T.shouldAuto(cfg, 'example.com'), true)
assert.equal(T.shouldAuto(cfg, 'www.example.com'), true)
assert.equal(T.shouldAuto(cfg, 'notexample.com'), false)
assert.equal(T.shouldAuto(cfg, 'bad.example.com'), false, 'never 优先')
assert.deepEqual(plain(T.autoMatches(cfg)), ['*://example.com/*', '*://*.example.com/*'])
assert.deepEqual(plain(T.autoMatches(T.normalizeCfg({ auto: ['bad host!'] }))), [])

// ---- 谷歌
assert.match(T.googleUrl('a b', 'zh-CN'), /tl=zh-CN&dt=t&q=a%20b$/)
assert.equal(T.parseGoogle([[['你好', 'hello'], ['世界', 'world']]]), '你好世界')
assert.equal(T.parseGoogle(null), null)
assert.equal(T.parseGoogle([[]]), null)

// ---- 缓存
assert.equal(T.cacheKey('google', 'ja', '  a \n b '), 'google|ja|a b')
const big = {}
for (let i = 0; i < 50; i++) big['k' + i] = ['v', i]
const pruned = T.pruneCache(big, { maxEntries: 10 })
assert.equal(Object.keys(pruned).length, 10)
assert.ok('k49' in pruned && !('k0' in pruned), '丢最旧的')
const small = { a: ['b', 1] }
assert.equal(T.pruneCache(small), small, '没超限就原样返回')
console.log('extension-trans: ok')
