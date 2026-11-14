// 可选权限：manifest 基线、perms.js 的申请 / 检查、没授权时常驻脚本不注册、Cookie 同步给出可操作的说明。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const manifest = JSON.parse(fs.readFileSync(path.join(dir, 'manifest.json'), 'utf8'))
const plain = (v) => JSON.parse(JSON.stringify(v))

test('manifest 权限基线：cookies 与 <all_urls> 是可选的', () => {
  assert.ok(!manifest.permissions.includes('cookies'))
  assert.deepEqual(manifest.optional_permissions, ['cookies'])
  assert.deepEqual(manifest.optional_host_permissions, ['<all_urls>'])
  // 安装时只索取本机清影的地址（后台要访问 127.0.0.1 的 API）
  assert.deepEqual(manifest.host_permissions, ['http://127.0.0.1/*', 'http://localhost/*'])
  assert.ok(!manifest.content_scripts, 'bypass.js 改为按需注册，不再是对所有网页常驻的静态内容脚本')
  assert.ok(manifest.permissions.includes('activeTab'), '主机权限可选后，activeTab 是按需注入的依据')
})

function permApi(granted = []) {
  const calls = { request: [], contains: [] }
  return {
    calls,
    permissions: {
      contains: async (r) => (calls.contains.push(r), (r.permissions || []).every((p) => granted.includes(p)) && (r.origins || []).every((o) => granted.includes(o) || granted.includes('*'))),
      request: async (r) => (calls.request.push(r), granted.includes('allow')),
    },
  }
}
function loadPerms(api) {
  const ctx = vm.createContext({ browser: api })
  ctx.globalThis = ctx
  vm.runInContext(fs.readFileSync(path.join(dir, 'perms.js'), 'utf8') + '\nthis.P = ccPerms', ctx)
  return ctx.P
}

test('cookieRequest：cookies + 该网站（含子域名）；主机名不合法时不给通配', () => {
  const P = loadPerms(permApi())
  assert.deepEqual(plain(P.cookieRequest('bilibili.com')), { permissions: ['cookies'], origins: ['*://bilibili.com/*', '*://*.bilibili.com/*'] })
  assert.deepEqual(plain(P.siteOrigins('a/b*')), [])
  assert.deepEqual(plain(P.sitesRequest()), { origins: ['http://*/*', 'https://*/*'] })
})

test('has / request：按浏览器的回答返回；出错当作没有 / 拒绝；空请求不调用浏览器', async () => {
  const a = permApi(['cookies', '*://x.com/*', '*://*.x.com/*', 'allow'])
  const P = loadPerms(a)
  assert.equal(await P.hasCookies('x.com'), true)
  assert.equal(await P.hasCookies('y.com'), false)
  assert.equal(await P.request(P.cookieRequest('x.com')), true)
  assert.equal(a.calls.request.length, 1)
  assert.equal(await P.request({ origins: [] }), false)
  assert.equal(a.calls.request.length, 1)
  const bad = loadPerms({ permissions: { contains: async () => { throw new Error('x') }, request: async () => { throw new Error('y') } } })
  assert.equal(await bad.hasSites(), false)
  assert.equal(await bad.request(bad.sitesRequest()), false)
})

test('permError 带 code=noperm，文字说明怎么开启', () => {
  const P = loadPerms(permApi())
  const e = P.permError('cookies')
  assert.equal(e.code, 'noperm')
  assert.match(e.message, /授权/)
  assert.match(P.permError('sites').message, /授权访问网页/)
})

function bootBg({ granted, takeoverEnabled = false, features, withInject = false }) {
  const local = { port: 5301, token: 'tok', device: 'd', takeover: { enabled: takeoverEnabled }, ...(features ? { features } : {}) }
  const injected = []
  const listeners = []
  const store = (data) => ({
    get: async (k) => Object.fromEntries((k == null ? Object.keys(data) : [].concat(typeof k === 'object' && !Array.isArray(k) ? Object.keys(k) : k)).filter((x) => x in data).map((x) => [x, structuredClone(data[x])])),
    set: async (o) => void Object.assign(data, o),
    remove: async () => {},
  })
  const registered = []
  const noop = new Proxy(function () {}, { get: (t, k) => (k in t ? t[k] : k === 'then' ? undefined : noop), apply: () => Promise.resolve(undefined) })
  const pa = permApi(granted)
  const api = new Proxy(
    {
      runtime: { id: 'self', getURL: (x) => x, onMessage: { addListener: (f) => listeners.push(f) }, onInstalled: { addListener() {} }, onStartup: { addListener() {} } },
      storage: { local: store(local), session: store({}), onChanged: { addListener() {} } },
      alarms: { create: async () => {}, clear: async () => {}, onAlarm: { addListener() {} } },
      scripting: {
        registerContentScripts: async (l) => void registered.push(...l),
        unregisterContentScripts: async () => {},
        getRegisteredContentScripts: async () => [],
        executeScript: async (o) => void injected.push(o),
      },
      permissions: { ...pa.permissions, onAdded: { addListener() {} }, onRemoved: { addListener() {} } },
      cookies: undefined,
    },
    { get: (t, k) => (k in t ? t[k] : noop) },
  )
  const ctx = vm.createContext({
    console: { ...console, warn() {}, debug() {} }, URL, URLSearchParams, setTimeout, clearTimeout, structuredClone,
    navigator: { userAgent: 'Chrome' }, fetch: async () => ({ ok: true, status: 200, json: async () => ({}) }),
  })
  ctx.globalThis = ctx
  ctx.browser = api
  for (const f of manifest.background.scripts) vm.runInContext(fs.readFileSync(path.join(dir, f), 'utf8'), ctx, { filename: f })
  return { ctx, registered, injected, listeners }
}

test('没有授权“访问所有网页”：常驻脚本一个都不注册', async () => {
  const { ctx, registered } = bootBg({ granted: [] })
  await ctx.syncResident(true)
  assert.deepEqual(registered, [])
})

test('授权后按开关注册；接管下载开启时才注册 Alt 点击放行脚本', async () => {
  const off = bootBg({ granted: ['*'] })
  await off.ctx.syncResident(true)
  assert.ok(off.registered.length > 0)
  assert.ok(!off.registered.some((s) => s.id === 'cc-bypass'))
  const on = bootBg({ granted: ['*'], takeoverEnabled: true })
  await on.ctx.syncResident(true)
  const b = on.registered.find((s) => s.id === 'cc-bypass')
  assert.deepEqual(plain(b.js), ['bypass.js'])
  assert.equal(b.runAt, 'document_start')
})

test('没有 Cookie 权限：同步登录给出 noperm 错误，不是“没有 Cookie”', async () => {
  const { ctx } = bootBg({ granted: [] })
  await assert.rejects(() => ctx.syncCookies('https://example.com/a', ''), (e) => e.code === 'noperm' && /授权/.test(e.message))
})

// ---- 探测 stub：视频类常驻功能不再对每个页面全量注入 ----

test('视频类功能只注册一个很小的探测 stub，不直接注册主体', async () => {
  const { ctx, registered } = bootBg({ granted: ['*'] })
  await ctx.syncResident(true)
  const ids = registered.map((s) => s.id)
  assert.ok(ids.includes('cc-stub'))
  for (const id of ['cc-float', 'cc-audio', 'cc-video']) assert.ok(!ids.includes(id), id)
  const stub = registered.find((s) => s.id === 'cc-stub')
  assert.deepEqual(plain(stub.js), ['content/stub.js'])
  assert.ok(fs.statSync(path.join(dir, 'content/stub.js')).size < 2048, 'stub 要足够小')
  // 非视频的常驻功能（划词、写作等）照旧直接注册
  assert.ok(ids.includes('cc-selbar') && ids.includes('cc-write'))
})

test('视频类功能全部关闭：不注册 stub', async () => {
  const { ctx, registered } = bootBg({ granted: ['*'], features: { floatButton: false, audioFx: false, videoMaster: false } })
  await ctx.syncResident(true)
  assert.ok(!registered.some((s) => s.id === 'cc-stub'))
})

test('stub 命中 <video> 才请后台注入：没有媒体元素时只观察，出现后发一次消息并停止观察', async () => {
  const src = fs.readFileSync(path.join(dir, 'content/stub.js'), 'utf8')
  const run = (initial) => {
    const sent = []
    let cb = null
    const state = { media: initial, disconnected: false }
    const ctx = vm.createContext({
      browser: { runtime: { id: 'x', sendMessage: async (m) => (sent.push(m), { ok: true }) } },
      document: { querySelector: () => (state.media ? {} : null) },
      MutationObserver: class { constructor(f) { cb = f } observe() {} disconnect() { state.disconnected = true } },
      setTimeout, clearTimeout,
    })
    ctx.globalThis = ctx
    vm.runInContext(src, ctx)
    return { sent, state, fire: () => cb() }
  }
  const a = run(true)
  assert.deepEqual(plain(a.sent), [{ type: 'cc-stub-load' }])
  const b = run(false)
  assert.equal(b.sent.length, 0)
  b.fire()
  await new Promise((r) => setTimeout(r, 350))
  assert.equal(b.sent.length, 0, '仍然没有媒体元素')
  b.state.media = true
  b.fire()
  await new Promise((r) => setTimeout(r, 350))
  assert.deepEqual(plain(b.sent), [{ type: 'cc-stub-load' }])
  assert.equal(b.state.disconnected, true)
})

test('cc-stub-load：按当前开关把视频类主体注入到发消息的 frame', async () => {
  const { ctx, injected, listeners } = bootBg({ granted: ['*'], withInject: true, features: { floatButton: true, audioFx: false } })
  const res = await new Promise((resolve) => {
    for (const l of listeners) l({ type: 'cc-stub-load' }, { id: 'self', tab: { id: 7 }, frameId: 3 }, resolve)
  })
  assert.equal(res.ok, true)
  assert.ok(injected.length >= 1)
  assert.ok(injected.every((x) => x.target.tabId === 7 && x.target.frameIds[0] === 3))
  const files = injected.flatMap((x) => plain(x.files))
  assert.ok(files.includes('content/float-button.js'))
  assert.ok(!files.includes('content/audio-fx.js'), '关了的功能不注入')
})
