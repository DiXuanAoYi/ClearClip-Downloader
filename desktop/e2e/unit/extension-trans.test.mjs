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
assert.equal(T.label('ja', 'zh'), '日语 · 日本語')
assert.equal(T.label('ja', 'en'), 'Japanese · 日本語')
assert.equal(T.label('en', 'en'), 'English')
assert.equal(T.label('zh-CN', 'zh'), '简体中文')
assert.equal(T.uiLang('ja'), 'ja')
assert.equal(T.uiLang('zh'), 'zh')
assert.equal(T.uiLang('en'), 'en')
assert.ok(T.LANGS.every((l) => T.label(l.code, 'zh') && T.label(l.code, 'en') && T.label(l.code, 'ja')), '三种界面语言都有名字')
assert.equal(T.DEFAULT_CFG ? T.DEFAULT_CFG.style : 'card', 'card')
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

// ---- 各网站单独设置
{
  const cfg = T.normalizeCfg({ target: 'zh-CN', mode: 'bilingual', sites: { 'GitHub.com': { target: 'ja', mode: 'replace' }, '*.docs.github.com': { target: 'fr' }, 'x.com': { mode: 'bad' }, '': { target: 'en' } } })
  assert.deepEqual(Object.keys(cfg.sites).sort(), ['docs.github.com', 'github.com'])
  assert.equal(T.effectiveCfg(cfg, 'github.com').target, 'ja')
  assert.equal(T.effectiveCfg(cfg, 'gist.github.com').mode, 'replace')
  // 最长匹配的域名赢；没写的字段沿用全局
  const d = T.effectiveCfg(cfg, 'a.docs.github.com')
  assert.equal(d.target, 'fr')
  assert.equal(d.mode, 'bilingual')
  assert.equal(T.effectiveCfg(cfg, 'example.com'), cfg)
  const text = 'github.com ja replace\nnews.site.com - replace\n\nbad.com - nope'
  const parsed = T.parseSites(text)
  assert.deepEqual(JSON.parse(JSON.stringify(parsed)), { 'github.com': { target: 'ja', mode: 'replace' }, 'news.site.com': { mode: 'replace' } })
  assert.equal(T.formatSites(parsed), 'github.com ja replace\nnews.site.com - replace')
}

// ---- 对比翻译与按住翻译
{
  const d = plain(T.normalizeCfg(undefined))
  assert.equal(d.holdMs, 450)
  assert.equal(d.compare, false)
  assert.equal(T.normalizeCfg({ holdMs: 0 }).holdMs, 0)
  assert.equal(T.normalizeCfg({ holdMs: 99999 }).holdMs, 2000)
  assert.equal(T.normalizeCfg({ holdMs: -5 }).holdMs, 0)
  assert.equal(T.normalizeCfg({ holdMs: 'abc' }).holdMs, 450)
  assert.equal(T.normalizeCfg({ compare: true }).compare, true)
  assert.equal(T.normalizeCfg({ compare: 'yes' }).compare, false)
  assert.ok(T.ENGINES.every((e) => T.ENGINE_NAMES[e]))
}

// ---- 各翻译服务单独设置
{
  assert.deepEqual(plain(T.normalizeCfg(undefined).services), {})
  const c = T.normalizeCfg({ services: { clearclip: { target: ' ja ', prompt: '  口语化 ', enabled: true }, google: { enabled: false, prompt: 'x' }, bogus: { enabled: false } } })
  assert.deepEqual(plain(c.services), { clearclip: { target: 'ja', prompt: '口语化' }, google: { enabled: false } })
  assert.equal(T.engineTarget(c, 'clearclip'), 'ja')
  assert.equal(T.engineTarget(c, 'google'), c.target)
  assert.equal(T.enginePrompt(c, 'clearclip'), '口语化')
  assert.equal(T.enginePrompt(c, 'google'), '')
  assert.deepEqual(plain(T.activeEngines(c)), ['clearclip'])
  // 选的服务被关掉就换一个还开着的
  assert.equal(T.pickEngine({ ...c, engine: 'google' }), 'clearclip')
  // 全关掉：退回全部，不至于什么都翻不了
  const none = T.normalizeCfg({ services: { clearclip: { enabled: false }, google: { enabled: false } } })
  assert.deepEqual(plain(T.activeEngines(none)), ['clearclip', 'google'])
  assert.equal(T.normalizeCfg({ services: { clearclip: { prompt: 'x'.repeat(900) } } }).services.clearclip.prompt.length, 500)
}
