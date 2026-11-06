// 自定义翻译接口（OpenAI 兼容）、不翻译的区域、自定义 CSS 的纯逻辑 + 后台引擎（用假的 fetch / 存储）。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const read = (f) => fs.readFileSync(path.join(dir, f), 'utf8')

function load(store = {}, fetchImpl = async () => ({ ok: false, status: 500, text: async () => '' })) {
  const calls = []
  const ctx = vm.createContext({
    URL, URLSearchParams, console, AbortController, setTimeout, clearTimeout, Date, JSON, Promise, Math, Array, Object, String, Number,
    fetch: async (url, init) => {
      calls.push({ url, init, body: init?.body ? JSON.parse(init.body) : null })
      return fetchImpl(url, init, calls.length)
    },
  })
  ctx.globalThis = ctx
  ctx.browser = {
    storage: { local: { get: async (k) => (typeof k === 'string' ? { [k]: store[k] } : { ...store }), set: async (o) => Object.assign(store, o), remove: async (k) => delete store[k] } },
    runtime: { onMessage: { addListener() {} } },
  }
  vm.runInContext('const api = globalThis.browser', ctx)
  vm.runInContext(read('trans-logic.js'), ctx, { filename: 'trans-logic.js' })
  ctx.call = async () => ({})
  ctx.sleep = async () => {}
  vm.runInContext(read('bg-trans.js'), ctx, { filename: 'bg-trans.js' })
  return { ctx, calls, store, T: vm.runInContext('ccTrans', ctx) }
}
const plain = (v) => JSON.parse(JSON.stringify(v))
const reply = (content) => async () => ({ ok: true, status: 200, json: async () => ({ choices: [{ message: { content } }] }), text: async () => '' })

test('接口地址补全成聊天补全地址', () => {
  const { T } = load()
  assert.equal(T.chatUrl('https://api.openai.com/v1'), 'https://api.openai.com/v1/chat/completions')
  assert.equal(T.chatUrl('https://api.deepseek.com'), 'https://api.deepseek.com/v1/chat/completions')
  assert.equal(T.chatUrl('http://localhost:11434/v1/'), 'http://localhost:11434/v1/chat/completions')
  assert.equal(T.chatUrl('https://x.com/v1/chat/completions'), 'https://x.com/v1/chat/completions')
  assert.equal(T.chatUrl('https://open.bigmodel.cn/api/paas/v4'), 'https://open.bigmodel.cn/api/paas/v4/chat/completions')
  assert.equal(T.chatUrl(''), '')
})

test('一批文字按字数和条数切开', () => {
  const { T } = load()
  assert.deepEqual(plain(T.splitBatches(['a', 'b', 'c'])), [[0, 1, 2]])
  assert.deepEqual(plain(T.splitBatches(['aaaa', 'bbbb', 'cc'], 8)), [[0, 1], [2]])
  assert.deepEqual(plain(T.splitBatches(['x'.repeat(100), 'y'], 50)), [[0], [1]], '单条超长自成一批，不拆')
  assert.equal(T.splitBatches(Array(45).fill('a')).length, 3, '每批最多 20 条')
  assert.deepEqual(plain(T.splitBatches([])), [])
})

test('模型回的数组：容忍代码围栏，条数不对算失败', () => {
  const { T } = load()
  assert.deepEqual(plain(T.parseArrayReply('["你好","世界"]', 2)), ['你好', '世界'])
  assert.deepEqual(plain(T.parseArrayReply('```json\n["a"]\n```', 1)), ['a'])
  assert.deepEqual(plain(T.parseArrayReply('好的，结果如下：["a","b"] 希望有帮助', 2)), ['a', 'b'])
  assert.equal(T.parseArrayReply('["a"]', 2), null)
  assert.equal(T.parseArrayReply('不是 JSON', 1), null)
  assert.equal(T.parseArrayReply('[bad json]', 1), null)
  assert.deepEqual(plain(T.parseArrayReply('[1, null]', 2)), ['1', ''])
})

test('自定义接口默认关着；填了地址和模型并打开才参与', () => {
  const { T } = load()
  assert.deepEqual(plain(T.activeEngines(T.normalizeCfg({}))), ['clearclip', 'google'])
  const on = (o) => T.normalizeCfg({ services: { custom: { enabled: true, ...o } } })
  assert.deepEqual(plain(T.activeEngines(on({ url: 'http://x/v1' }))), ['clearclip', 'google'], '没填模型不算')
  assert.deepEqual(plain(T.activeEngines(on({ url: 'http://x/v1', model: 'm' }))), ['clearclip', 'google', 'custom'])
  assert.equal(T.customReady(T.normalizeCfg({ services: { custom: { url: 'u', model: 'm' } } })), false, '没勾选不算')
  // 设置往返：密钥、地址、模型、额外要求都保留，过长的被截断
  const c = on({ url: ' http://x/v1 ', key: 'sk-1', model: 'm', prompt: 'p'.repeat(900) })
  assert.equal(c.services.custom.url, 'http://x/v1')
  assert.equal(c.services.custom.key, 'sk-1')
  assert.equal(c.services.custom.prompt.length, 500)
  assert.equal(T.normalizeCfg(c).services.custom.model, 'm')
  assert.equal(T.pickEngine(T.normalizeCfg({ engine: 'custom' })), 'clearclip', '自定义没填好时，选了它也退回第一个可用的')
})

test('不翻译的区域：全局的 + 只对某个网站的；自定义 CSS 去掉危险写法', () => {
  const { T } = load()
  const cfg = T.normalizeCfg({ exclude: '.ad\n\n github.com | .markdown-body pre \n*.news.com | #side\n.ad' })
  assert.deepEqual(plain(cfg.exclude), ['.ad', 'github.com | .markdown-body pre', '*.news.com | #side'])
  assert.deepEqual(plain(T.excludeFor(cfg, 'github.com')), ['.ad', '.markdown-body pre'])
  assert.deepEqual(plain(T.excludeFor(cfg, 'gist.github.com')), ['.ad', '.markdown-body pre'], '子域名也算')
  assert.deepEqual(plain(T.excludeFor(cfg, 'a.news.com')), ['.ad', '#side'])
  assert.deepEqual(plain(T.excludeFor(cfg, 'example.org')), ['.ad'])
  assert.equal(T.cleanCss('font-size:15px; </style><script>x</script>'), 'font-size:15px; <script>x</script>'.replace('<script>x</script>', '<script>x</script>'), '只去掉 style 标签')
  assert.ok(!/@import|url\(/i.test(T.cleanCss('@import "x.css"; background: url(http://e/x.png); color: red')))
  assert.equal(T.cleanCss('a'.repeat(5000)).length, 3000)
})

test('后台引擎：批量发 JSON 数组，带密钥，译文对上号并进缓存', async () => {
  const store = { transCfg: { services: { custom: { enabled: true, url: 'https://api.example.com/v1', key: 'sk-test', model: 'm1', prompt: '口语化' } } } }
  const { ctx, calls } = load(store, reply('["你好","世界"]'))
  const r = await vm.runInContext('translateTexts(["Hello","World"], { target: "zh-CN", engine: "custom", extra: "口语化" })', ctx)
  assert.deepEqual(plain(r.texts), ['你好', '世界'])
  assert.equal(calls.length, 1)
  assert.equal(calls[0].url, 'https://api.example.com/v1/chat/completions')
  assert.equal(calls[0].init.headers.Authorization, 'Bearer sk-test')
  assert.equal(calls[0].body.model, 'm1')
  assert.equal(calls[0].body.messages[1].content, '["Hello","World"]')
  assert.match(calls[0].body.messages[0].content, /Chinese/)
  assert.match(calls[0].body.messages[0].content, /口语化/)
  // 再翻同样的：走缓存，不再请求
  const again = await vm.runInContext('translateTexts(["Hello","World"], { target: "zh-CN", engine: "custom", extra: "口语化" })', ctx)
  assert.deepEqual(plain(again.texts), ['你好', '世界'])
  assert.equal(again.cached, 2)
  assert.equal(calls.length, 1)
})

test('后台引擎：模型没按数组回就逐条重来，不会错位；没填好时报清楚', async () => {
  const store = { transCfg: { services: { custom: { enabled: true, url: 'http://localhost:11434', model: 'q' } } } }
  let n = 0
  const { ctx, calls } = load(store, async () => {
    n++
    const content = n === 1 ? '这是翻译：你好 世界' : n === 2 ? '["你好"]' : '["世界"]'
    return { ok: true, status: 200, json: async () => ({ choices: [{ message: { content } }] }), text: async () => '' }
  })
  const r = await vm.runInContext('translateTexts(["Hello","World"], { target: "zh-CN", engine: "custom" })', ctx)
  assert.deepEqual(plain(r.texts), ['你好', '世界'])
  assert.equal(calls.length, 3)
  assert.equal(calls[0].init.headers.Authorization, undefined, '没填密钥就不带')

  const bad = load({ transCfg: {} }, reply('["x"]'))
  await assert.rejects(vm.runInContext('translateTexts(["Hello"], { target: "zh-CN", engine: "custom" })', bad.ctx), /还没填好/)
  const unauth = load(store, async () => ({ ok: false, status: 401, text: async () => 'no' }))
  await assert.rejects(vm.runInContext('translateTexts(["Hi"], { target: "zh-CN", engine: "custom" })', unauth.ctx), /密钥/)
})

test('DeepLX：默认关；POST 目标语言代码；兼容两种返回', async () => {
  const { T } = load()
  assert.deepEqual(plain(T.activeEngines(T.normalizeCfg({ services: { deeplx: { enabled: true } } }))), ['clearclip', 'google'], '没填地址不算')
  assert.ok(T.activeEngines(T.normalizeCfg({ services: { deeplx: { enabled: true, url: 'http://x/translate' } } })).includes('deeplx'))
  assert.deepEqual(['zh-CN', 'zh-TW', 'ja', 'pt-BR', 'en'].map(T.deeplLang), ['ZH', 'ZH-HANT', 'JA', 'PT-BR', 'EN'])
  const store = { transCfg: { services: { deeplx: { enabled: true, url: 'http://localhost:1188/translate', key: 'k' } } } }
  const { ctx, calls } = load(store, async (_u, init) => {
    const first = JSON.parse(init.body).text === 'Hello'
    return { ok: true, status: 200, json: async () => (first ? { data: '你好' } : { translations: [{ text: '世界' }] }), text: async () => '' }
  })
  const r = await vm.runInContext('translateTexts(["Hello","World"], { target: "zh-CN", engine: "deeplx" })', ctx)
  assert.deepEqual(plain(r.texts), ['你好', '世界'])
  assert.equal(calls[0].body.target_lang, 'ZH')
  assert.equal(calls[0].init.headers.Authorization, 'Bearer k')
})
