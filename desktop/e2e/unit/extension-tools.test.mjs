// 扩展新增功能里的纯逻辑：功能注册表、链接清理、下载进度、以图搜图引擎、二维码。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')

const store = {}
const local = {
  get: async (k) => (typeof k === 'string' ? (k in store ? { [k]: store[k] } : {}) : Object.fromEntries(k.filter((x) => x in store).map((x) => [x, store[x]]))),
  set: async (o) => Object.assign(store, JSON.parse(JSON.stringify(o))),
}
const ctx = vm.createContext({ URL, URLSearchParams, TextEncoder, console, api: { storage: { local } } })
const FILES = ['features.js', 'clean-link.js', 'progress.js', 'engines.js', 'vendor/qrcode.js', 'qr.js']
for (const f of FILES) vm.runInContext(fs.readFileSync(path.join(dir, f), 'utf8'), ctx, { filename: f })
const X = (name) => vm.runInContext(name, ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// ---- 功能注册表
{
  const f = plain(X('normalizeFeatures')(undefined))
  assert.equal(f.speed, true) // 倍速、记住倍速、快进快退默认开
  assert.equal(f.speedMemory, true)
  assert.equal(f.seek, true)
  assert.equal(f.floatButton, false) // 其他常驻注入类默认关
  assert.equal(f.hoverZoom, false)
  assert.equal(f.unlock, false)
  assert.equal(f.cleanLink, true)
  assert.equal(plain(X('normalizeFeatures')({ floatButton: true, speed: 'yes', nope: true })).floatButton, true)
  assert.equal(plain(X('normalizeFeatures')({ speed: 'yes' })).speed, true) // 不是布尔值就用默认
  assert.equal('nope' in plain(X('normalizeFeatures')({ nope: true })), false)
  // 默认只注册播放控制脚本
  assert.deepEqual(plain(X('neededScripts')(undefined)), ['cc-video'])
  assert.deepEqual(plain(X('neededScripts')({ floatButton: true, hoverZoom: true, unlock: true })), ['cc-float', 'cc-zoom', 'cc-video', 'cc-unlock'])
  assert.deepEqual(plain(X('neededScripts')({ speed: false, speedMemory: false, seek: false })), [])
  assert.deepEqual(plain(X('neededScripts')({ speed: false, speedMemory: false, seek: false, videoTools: true })), ['cc-video'])
}

// ---- 站点规则
{
  const r = plain(X('normalizeSiteRules')({ off: { video: [' A.com ', 'a.com', ''] }, unlockOn: ['B.com'], unlockAll: 'x', zoomRules: 5 }))
  assert.deepEqual(r.off.video, ['a.com'])
  assert.deepEqual(r.unlockOn, ['b.com'])
  assert.equal(r.unlockAll, false)
  assert.equal(r.zoomRules, '')
  assert.equal(X('hostListed')(['a.com'], 'www.a.com'), true)
  assert.equal(X('hostListed')(['a.com'], 'nota.com'), false)
}

// ---- 链接清理
{
  const clean = (u, extra) => plain(X('cleanUrl')(u, extra))
  let r = clean('https://example.com/p?id=7&utm_source=x&utm_medium=y&fbclid=abc#top')
  assert.equal(r.url, 'https://example.com/p?id=7#top')
  assert.deepEqual(r.removed, ['utm_source', 'utm_medium', 'fbclid'])
  r = clean('https://example.com/p?utm_source=x')
  assert.equal(r.url, 'https://example.com/p') // 参数全去掉后不留 “?”
  r = clean('https://example.com/p?a=1&b=%E4%B8%AD&gclid=1')
  assert.equal(r.url, 'https://example.com/p?a=1&b=%E4%B8%AD') // 剩下的参数保持原样
  r = clean('https://example.com/p?foo=1&bar=2', ['foo'])
  assert.equal(r.url, 'https://example.com/p?bar=2') // 用户自己加的参数名
  r = clean('https://www.amazon.com/dp/B000/ref=sr_1_1?keywords=x&pf_rd_r=zzz')
  assert.ok(!r.url.includes('/ref='))
  assert.equal(clean('not a url').url, 'not a url')
  assert.equal(clean('magnet:?xt=urn:btih:abc&utm_source=1').url, 'magnet:?xt=urn:btih:abc&utm_source=1') // 只处理 http(s)
  r = clean('https://www.google.com/url?q=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1&sa=D')
  assert.equal(r.url, 'https://example.com/a?x=1')
  assert.equal(r.unwrapped, true)
}

// ---- 下载进度
{
  const t = (o) => ({ id: 1, status: 'running', received: 0, total: 0, speed: 0, ...o })
  const s = plain(X('summarizeTasks')([t({ status: 'running', speed: 100 }), t({ id: 2, status: 'queued' }), t({ id: 3, status: 'paused' }), t({ id: 4, status: 'failed', errorKind: 'need_login' }), t({ id: 5, status: 'done' })]))
  assert.equal(s.active, 2)
  assert.equal(s.needLogin, 1)
  assert.equal(s.speed, 100)
  assert.equal(X('badgeFor')({ active: 0 }), '')
  assert.equal(X('badgeFor')({ active: 7 }), '7')
  assert.equal(X('badgeFor')({ active: 250 }), '99+')
  const d = plain(X('describeTask')(t({ fileName: 'a.mp4', received: 500, total: 1000, speed: 100 })))
  assert.equal(d.pct, 50)
  assert.match(d.line, /50%/)
  assert.deepEqual(d.actions, ['pause', 'reveal'])
  assert.deepEqual(plain(X('describeTask')(t({ status: 'failed', errorKind: 'need_login' }))).actions, ['login', 'resume'])
  assert.equal(plain(X('describeTask')(t({ status: 'running', received: 2048 }))).pct, null) // 不知道总大小：不显示百分比
  const order = plain(X('arrangeTasks')([t({ id: 1, status: 'done' }), t({ id: 2, status: 'running' }), t({ id: 3, status: 'canceled' }), t({ id: 4, status: 'queued' })])).map((x) => x.id)
  assert.deepEqual(order, [2, 4, 1])
  assert.deepEqual(plain(X('newlyDone')([1], [t({ id: 1, status: 'done' }), t({ id: 2, status: 'done' })])).map((x) => x.id), [2])
  assert.equal(X('fmtBytes')(1536), '1.5 KB')
}

// ---- 以图搜图引擎
{
  assert.equal(X('DEFAULT_ENGINES').length, 6)
  const list = plain(X('parseEngines')('A | https://a.test/?u={url}\nbad line\nB | ftp://b.test/{url}\nC | https://c.test/no-placeholder'))
  assert.deepEqual(list, [{ name: 'A', url: 'https://a.test/?u={url}' }])
  assert.equal(plain(X('parseEngines')('')).length, 6) // 空内容用默认
  assert.equal(X('engineUrl')({ url: 'https://a.test/?u={url}' }, 'https://x.com/a.jpg?w=1&h=2'), 'https://a.test/?u=https%3A%2F%2Fx.com%2Fa.jpg%3Fw%3D1%26h%3D2')
  const text = X('enginesToText')(X('DEFAULT_ENGINES'))
  assert.equal(plain(X('parseEngines')(text)).length, 6)
}

// ---- 二维码
{
  const svg = X('qrSvg')('https://example.com/a?b=中文')
  assert.match(svg, /^<svg /)
  assert.ok(svg.includes('<path'))
  assert.equal(X('qrSvg')('x'.repeat(5000)), null) // 放不下
}

console.log('extension-tools ok')
