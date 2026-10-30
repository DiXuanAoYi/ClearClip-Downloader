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
const FILES = ['features.js', 'clean-link.js', 'progress.js', 'engines.js', 'vendor/qrcode.js', 'qr.js', 'later.js', 'capture-plan.js']
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

// ---- 取原图规则
{
  const ctx2 = vm.createContext({ URL, console, document: { baseURI: 'https://site.test/a/' } })
  vm.runInContext(fs.readFileSync(path.join(dir, 'content/original.js'), 'utf8'), ctx2, { filename: 'original.js' })
  const cand = (u, rules) => JSON.parse(JSON.stringify(vm.runInContext('ccOriginal', ctx2).candidates(u, rules)))
  const first = (u, rules) => cand(u, rules)[0]
  assert.equal(first('https://blog.test/wp-content/uploads/2024/a-300x200.jpg'), 'https://blog.test/wp-content/uploads/2024/a.jpg')
  assert.equal(first('https://img.test/a_thumb.png'), 'https://img.test/a.png')
  assert.equal(first('https://img.alicdn.com/imgextra/x.jpg_300x300.jpg'), 'https://img.alicdn.com/imgextra/x.jpg')
  assert.equal(first('https://img.alicdn.com/imgextra/x.jpg_.webp'), 'https://img.alicdn.com/imgextra/x.jpg')
  assert.equal(first('https://i0.hdslb.com/bfs/archive/x.jpg@672w_378h_1c.webp'), 'https://i0.hdslb.com/bfs/archive/x.jpg')
  assert.equal(first('https://cdn.test/a.jpg!small'), 'https://cdn.test/a.jpg')
  assert.equal(first('https://cdn.test/a.jpg?x-oss-process=image/resize,w_200'), 'https://cdn.test/a.jpg?')
  assert.equal(first('https://cdn.test/a.jpg?imageView2/2/w/200'), 'https://cdn.test/a.jpg')
  assert.equal(first('https://lh3.googleusercontent.com/abc=w300-h200-no'), 'https://lh3.googleusercontent.com/abc=s0')
  assert.equal(first('https://i.ytimg.com/vi/ID/hqdefault.jpg'), 'https://i.ytimg.com/vi/ID/maxresdefault.jpg')
  assert.equal(first('https://pbs.twimg.com/media/X?format=jpg&name=small'), 'https://pbs.twimg.com/media/X?format=jpg&name=orig')
  assert.equal(first('https://i.pinimg.com/236x/ab/cd/x.jpg'), 'https://i.pinimg.com/originals/ab/cd/x.jpg')
  assert.equal(first('https://wx1.sinaimg.cn/mw690/abc.jpg'), 'https://wx1.sinaimg.cn/large/abc.jpg')
  assert.equal(first('https://upload.wikimedia.org/wikipedia/commons/thumb/a/ab/Cat.jpg/220px-Cat.jpg'), 'https://upload.wikimedia.org/wikipedia/commons/a/ab/Cat.jpg')
  assert.equal(first('https://i.imgur.com/AbCdE12m.jpg'), 'https://i.imgur.com/AbCdE12.jpg')
  assert.equal(first('https://img1.doubanio.com/view/photo/m/public/p1.jpg'), 'https://img1.doubanio.com/view/photo/l/public/p1.jpg')
  assert.equal(first('https://pic1.zhimg.com/v2-abc_720w.jpg'), 'https://pic1.zhimg.com/v2-abc_r.jpg')
  assert.deepEqual(cand('https://i.pximg.net/c/250x250_80_a2/img-master/img/2020/01/01/00/00/00/123_p0_square1200.jpg').slice(0, 2), ['https://i.pximg.net/img-original/img/2020/01/01/00/00/00/123_p0.jpg', 'https://i.pximg.net/img-original/img/2020/01/01/00/00/00/123_p0.png'])
  assert.equal(first('https://64.media.tumblr.com/abc/tumblr_xyz_500.jpg'), 'https://64.media.tumblr.com/abc/tumblr_xyz_1280.jpg')
  assert.equal(first('https://m.media-amazon.com/images/I/81abc._AC_UY218_.jpg'), 'https://m.media-amazon.com/images/I/81abc.jpg')
  assert.equal(first('https://preview.redd.it/abc.jpg?width=640&auto=webp&s=zzz'), 'https://i.redd.it/abc.jpg')
  assert.equal(first('https://cdn.test/a.jpg?w=200&h=100&token=1'), 'https://cdn.test/a.jpg?token=1') // 保留其他参数
  assert.deepEqual(cand('https://cdn.test/photo.jpg'), []) // 没有线索：没有候选
  assert.deepEqual(cand('data:image/png;base64,AAA'), [])
  // 用户规则优先，坏的正则被忽略
  assert.equal(first('https://my.test/p_s.jpg', '# 注释\n([(\n_s\\.jpg$ => _big.jpg'), 'https://my.test/p_big.jpg')
  assert.equal(cand('https://my.test/p_s.jpg', '(( => x').length, 0)
  assert.equal(vm.runInContext('ccOriginal', ctx2).bestOfSrcset('a.jpg 1x, b.jpg 2x, c.jpg 3x'), 'c.jpg')
  assert.equal(vm.runInContext('ccOriginal', ctx2).bestOfSrcset('a.jpg 320w, b.jpg 1280w, c.jpg 640w'), 'b.jpg')
}

// ---- 播放增强的纯逻辑
{
  const ctx3 = vm.createContext({ URL, console })
  vm.runInContext(fs.readFileSync(path.join(dir, 'content/video-logic.js'), 'utf8'), ctx3, { filename: 'video-logic.js' })
  const V = vm.runInContext('ccVideo', ctx3)
  const json = (x) => JSON.parse(JSON.stringify(x))
  // 配置：缺的补默认，越界回默认，重复键能查出来
  const d = json(V.normalizeCfg())
  assert.equal(d.keys.faster, 'd')
  assert.equal(d.rateStep, 0.1)
  const c = json(V.normalizeCfg({ keys: { faster: 'D', slower: 'ArrowDown', back: 'x' }, rateStep: 9, seekStep: 30, skipIntro: { 'A.com': 85, 'b.com': -1, 'c.com': 9999 } }))
  assert.equal(c.keys.faster, 'd')
  assert.equal(c.keys.slower, 'ArrowDown')
  assert.equal(c.rateStep, 0.1)
  assert.equal(c.seekStep, 30)
  assert.deepEqual(c.skipIntro, { 'a.com': 85 })
  assert.deepEqual(json(V.findConflicts(c.keys)), { x: ['back', 'forward'] })
  assert.deepEqual(json(V.findConflicts(d.keys)), {})
  // 快捷键匹配：组合键不算，大小写不敏感
  assert.equal(V.matchKey({ key: 'S' }, d.keys), 'slower')
  assert.equal(V.matchKey({ key: 's', ctrlKey: true }, d.keys), null)
  assert.equal(V.matchKey({ key: 'q' }, d.keys), null)
  // 倍速：步进、上下限
  assert.equal(V.stepRate(1, 1, 0.1), 1.1)
  assert.equal(V.stepRate(1.1, 1, 0.1), 1.2)
  assert.equal(V.stepRate(0.1, -1, 0.1), 0.1)
  assert.equal(V.stepRate(16, 1, 0.5), 16)
  assert.equal(V.clampRate(99), 16)
  assert.equal(V.clampRate('x'), 1)
  assert.equal(V.fmtRate(1.25), '1.25×')
  assert.equal(V.fmtRate(2), '2×')
  // 画面
  assert.equal(V.filterCss({}), '')
  assert.equal(V.filterCss({ brightness: 120, contrast: 100, saturate: 80 }), 'brightness(1.2) saturate(0.8)')
  assert.equal(V.transformCss({ mirror: true, rotate: 90 }), 'rotate(90deg) scaleX(-1)')
  // 片头片尾：太短的视频不跳
  assert.equal(V.skipIntroTarget(0.3, 85, 1500), 85)
  assert.equal(V.skipIntroTarget(90, 85, 1500), null)
  assert.equal(V.skipIntroTarget(0, 85, 100), null)
  assert.equal(V.skipIntroTarget(0, 0, 1500), null)
  assert.equal(V.inOutro(1450, 60, 1500), true)
  assert.equal(V.inOutro(1000, 60, 1500), false)
  assert.equal(V.inOutro(40, 60, 100), false)
  // 音量增益：跨域且没有 CORS 才不安全
  assert.equal(V.audioGraphUnsafe('https://cdn.other.com/a.mp4', 'https://site.test', null), true)
  assert.equal(V.audioGraphUnsafe('https://cdn.other.com/a.mp4', 'https://site.test', 'anonymous'), false)
  assert.equal(V.audioGraphUnsafe('https://site.test/a.mp4', 'https://site.test', null), false)
  assert.equal(V.audioGraphUnsafe('blob:https://site.test/xyz', 'https://site.test', null), false)
  // 文件名
  assert.equal(V.fmtTime(3725), '01-02-05')
  assert.equal(V.shotName('第1集: a/b?', 65, 'png'), '第1集 a b -00-01-05.png'.replace(' -', '-'))
}

// ---- 稍后看
{
  vm.runInContext('async function tidyUrl(u) { return cleanUrl(u, []).url }', ctx) // shared.js 里的版本依赖浏览器接口，这里用等价的简化版
  const add = X('laterAdd')
  let r = plain(add([], { url: 'https://a.test/x#frag', title: 'A' }, 100))
  assert.equal(r.added, true)
  assert.equal(r.list[0].seen, false)
  r = plain(add([{ url: 'https://b.test/', title: 'B', at: 1, seen: true }, ...r.list], { url: 'https://a.test/x', title: 'A2' }, 200))
  assert.equal(r.added, false) // 同一页（忽略 #）：不重复，提到最前面并重新标成未看
  assert.equal(r.list.length, 2)
  assert.equal(r.list[0].title, 'A2')
  assert.equal(plain(add([], { url: 'chrome://settings' })).invalid, true)
  assert.equal(plain(add([], { url: 'javascript:alert(1)' })).invalid, true)
  const L = r.list
  assert.equal(plain(X('laterSeen')(L, 'https://b.test')).find((x) => x.url === 'https://b.test/').seen, true)
  assert.equal(X('laterUnseen')(L), 1)
  assert.equal(plain(X('laterRemove')(L, 'https://a.test/x')).length, 1)
  let big = []
  for (let i = 0; i < 320; i++) big = plain(add(big, { url: `https://s.test/${i}` }, i)).list
  assert.equal(big.length, 300)
  assert.equal(big[0].url, 'https://s.test/319')
  // 存取：去追踪参数后保存
  const r2 = await X('addLater')({ url: 'https://c.test/p?utm_source=x&id=1', title: 'C' })
  assert.equal(r2.added, true)
  assert.equal((await X('getLater')())[0].url, 'https://c.test/p?id=1')
}

// ---- 页面截图的分段与分块
{
  const slices = plain(X('planSlices')(2500, 1000))
  assert.equal(slices.length, 3)
  assert.deepEqual(slices[0], { scrollY: 0, srcTop: 0, destTop: 0, height: 1000 })
  assert.deepEqual(slices[2], { scrollY: 1500, srcTop: 500, destTop: 2000, height: 500 }) // 最后一屏滚不到底：从截图中间取
  assert.equal(slices.reduce((n, s) => n + s.height, 0), 2500)
  assert.deepEqual(plain(X('planSlices')(800, 1000)), [{ scrollY: 0, srcTop: 0, destTop: 0, height: 800 }])
  assert.equal(X('chunkHeight')(1280, 1), 16384)
  assert.equal(X('chunkHeight')(1280, 2), 8192)
  assert.ok(X('chunkHeight')(20000, 1) < 16384) // 太宽：受总面积限制
  assert.deepEqual(plain(X('planChunks')(40000, 16384)), [{ from: 0, to: 16384 }, { from: 16384, to: 32768 }, { from: 32768, to: 40000 }])
  assert.equal(X('captureName')('第1集: a/b?', new Date(2026, 0, 2, 3, 4, 5), 'png'), '第1集 a b-20260102-030405.png')
  assert.equal(X('captureName')('', new Date(2026, 0, 2, 3, 4, 5), 'png', 1, 3), 'page-20260102-030405-2.png')
  assert.equal(X('excerptText')(' 内容 ', '标题', 'https://x.test/'), '内容\n\n—— 标题\nhttps://x.test/')
}

// ---- 解除限制的注册范围
{
  const um = (r) => plain(X('unlockMatches')(r))
  assert.deepEqual(um({}), []) // 名单是空的：不注册
  assert.deepEqual(um({ unlockOn: ['Example.com'] }), ['*://example.com/*', '*://*.example.com/*'])
  assert.deepEqual(um({ unlockOn: ['bad host!', 'a-b.co'] }), ['*://a-b.co/*', '*://*.a-b.co/*']) // 不合法的主机名丢掉
  assert.deepEqual(um({ unlockAll: true, unlockOn: ['x.com'] }), ['http://*/*', 'https://*/*'])
  assert.ok(X('neededScripts')({ unlock: true }).includes('cc-unlock'))
  assert.ok(!X('neededScripts')({}).includes('cc-unlock'))
}
