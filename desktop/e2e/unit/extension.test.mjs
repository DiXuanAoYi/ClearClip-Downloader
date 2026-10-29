// 浏览器扩展里的纯逻辑：接管下载的判断、嗅探的分类与清单解析。脚本在隔离的 vm 里加载，和浏览器里一样是普通全局脚本。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')

function memoryStore() {
  const data = {}
  return {
    data,
    get: async (k) => (k in data ? { [k]: data[k] } : {}),
    set: async (o) => Object.assign(data, JSON.parse(JSON.stringify(o))),
    remove: async (k) => void delete data[k],
  }
}

const session = memoryStore()
const ctx = vm.createContext({ URL, console, api: { storage: { session, local: memoryStore() } } })
for (const f of ['takeover.js', 'sniff.js']) vm.runInContext(fs.readFileSync(path.join(dir, f), 'utf8'), ctx, { filename: f })
const X = (name) => vm.runInContext(name, ctx)
const plain = (v) => JSON.parse(JSON.stringify(v)) // vm 里创建的对象原型不同，比较前转成普通对象

const cfg = (o = {}) => X('normalizeTakeover')({ enabled: true, ...o })
const take = (c, item) => X('shouldTake')(c, { url: 'https://example.com/a', ...item })

// ---- 接管：设置整理
{
  const d = plain(X('normalizeTakeover')(undefined))
  assert.equal(d.enabled, false) // 默认关闭
  assert.equal(d.mode, 'types')
  assert.ok(d.types.includes('zip') && d.types.includes('exe') && d.types.includes('mkv'))
  assert.equal(d.minSizeMB, 1)
  const c = plain(X('normalizeTakeover')({ enabled: true, mode: 'x', types: ['.ZIP', ' *.rar', 'zip', '', '7z!'], minSizeMB: -3, allowSites: ['https://WWW.Example.com:8080/a', 'a.com', ''], denySites: ['*.b.com', 'not a host'] }))
  assert.deepEqual(c.types, ['zip', 'rar', '7z'])
  assert.equal(c.mode, 'types')
  assert.equal(c.minSizeMB, 1) // 非法值回到默认
  assert.deepEqual(c.allowSites, ['www.example.com', 'a.com'])
  assert.deepEqual(c.denySites, ['b.com'])
  assert.equal(plain(X('normalizeTakeover')({ types: [] })).types.length, X('TAKEOVER_TYPES').length)
  assert.equal(plain(X('normalizeTakeover')({ enabled: 'yes' })).enabled, false) // 只有 true 才算开启
}

// ---- 接管：判断
{
  const c = cfg()
  assert.equal(take(X('normalizeTakeover')({}), { filename: 'a.zip' }).take, false) // 默认关闭
  assert.equal(take(c, { filename: 'C:\\Users\\me\\Downloads\\Setup.EXE' }).take, true) // 路径和大小写
  assert.equal(take(c, { url: 'https://x.com/files/a.tar.gz?token=1' }).take, true) // 文件名未定时看地址
  assert.equal(take(c, { filename: 'notes.txt' }).take, false) // 类型不在列表
  assert.equal(take(c, { filename: 'a.zip', fileSize: 200 * 1024 }).reason, 'too-small')
  assert.equal(take(c, { filename: 'a.zip', fileSize: 5 * 1048576 }).take, true)
  assert.equal(take(c, { filename: 'a.zip', fileSize: -1 }).take, true) // 大小未知时不因为大小拒绝
  assert.equal(take(cfg({ minSizeMB: 0 }), { filename: 'a.zip', fileSize: 10 }).take, true)
  // 协议：blob / data / file / ftp 不能交给清影
  for (const url of ['blob:https://example.com/1', 'data:text/plain,hi', 'file:///tmp/a.zip', 'ftp://example.com/a.zip']) assert.equal(take(c, { url, filename: 'a.zip' }).reason, 'scheme', url)
  // 最终地址优先（重定向之后）
  assert.equal(take(c, { url: 'https://example.com/dl', finalUrl: 'blob:https://example.com/1', filename: 'a.zip' }).reason, 'scheme')
  // 其他扩展发起的下载
  assert.equal(take(c, { filename: 'a.zip', byExtension: true }).reason, 'other-extension')
  // 站点：黑名单含子域名，白名单留空表示全部
  const deny = cfg({ denySites: ['drive.example.com'] })
  assert.equal(take(deny, { url: 'https://drive.example.com/a', filename: 'a.zip' }).reason, 'denied-site')
  assert.equal(take(deny, { url: 'https://a.drive.example.com/a', filename: 'a.zip' }).reason, 'denied-site')
  assert.equal(take(deny, { url: 'https://notdrive.example.com/a', filename: 'a.zip' }).take, true) // 不是子域名，只是后缀相似
  assert.equal(take(deny, { url: 'https://cdn.net/a', referrer: 'https://drive.example.com/p', filename: 'a.zip' }).reason, 'denied-site') // 来源页也算
  const allow = cfg({ allowSites: ['example.com'] })
  assert.equal(take(allow, { url: 'https://dl.example.com/a', filename: 'a.zip' }).take, true)
  assert.equal(take(allow, { url: 'https://other.com/a', filename: 'a.zip' }).reason, 'not-allowed-site')
  assert.equal(take(allow, { url: 'https://cdn.other.com/a', referrer: 'https://example.com/p', filename: 'a.zip' }).take, true)
  // 所有下载模式
  const all = cfg({ mode: 'all' })
  assert.equal(take(all, { filename: 'notes.txt' }).take, true)
  assert.equal(take(all, { filename: 'notes.txt', fileSize: 10 }).reason, 'too-small')
  // 没有扩展名：通用二进制 / 压缩包且大小已知才接管
  assert.equal(take(c, { url: 'https://example.com/download', mime: 'application/octet-stream', fileSize: 3e6 }).take, true)
  assert.equal(take(c, { url: 'https://example.com/download', mime: 'application/octet-stream' }).take, false)
  assert.equal(take(c, { url: 'https://example.com/download', mime: 'text/html', fileSize: 3e6 }).take, false)
}

// ---- 接管：请求内容与右键菜单
{
  const p = plain(X('grabPayload')({ url: 'https://a.com/x', finalUrl: 'https://cdn.a.com/x?t=1', referrer: 'https://a.com/p', filename: 'D:\\dl\\包.zip', fileSize: 1234, mime: 'application/zip' }, 'a=1; b=2', 'UA', 'Chrome 浏览器扩展'))
  assert.deepEqual(p, { url: 'https://cdn.a.com/x?t=1', referer: 'https://a.com/p', cookie: 'a=1; b=2', filename: '包.zip', size: 1234, mime: 'application/zip', userAgent: 'UA', deviceName: 'Chrome 浏览器扩展' })
  assert.equal(X('grabPayload')({ url: 'https://a.com/x', fileSize: -1 }).size, null)
  assert.equal(X('cookieHeader')([{ name: 'a', value: '1' }, { name: 'b', value: '2' }]), 'a=1; b=2')
  assert.equal(X('cookieHeader')(undefined), '')
  assert.equal(X('shortUrl')('https://a.com/x/y.zip?token=secret#h'), 'https://a.com/x/y.zip')
  const act = X('linkAction')
  assert.equal(act('https://a.com/pkg/tool.zip?x=1'), 'grab')
  assert.equal(act('https://a.com/doc/manual.pdf'), 'grab')
  assert.equal(act('https://a.com/v/clip.mp4'), 'send') // 音视频走媒体解析
  assert.equal(act('https://a.com/watch?v=1'), 'send')
  assert.equal(act('https://a.com/page.html'), 'send')
}

// ---- 嗅探：分类
{
  const cm = (u, ct, cl, cr) => plain(X('classifyMedia')(u, ct, cl, cr))
  assert.deepEqual(cm('https://c.com/i.m3u8?x=1', ''), { kind: 'hls', size: 0 })
  assert.deepEqual(cm('https://c.com/a', 'application/vnd.apple.mpegurl'), { kind: 'hls', size: 0 })
  assert.deepEqual(cm('https://c.com/m.mpd', ''), { kind: 'dash', size: 0 })
  assert.deepEqual(cm('https://c.com/m', 'application/dash+xml'), { kind: 'dash', size: 0 })
  assert.deepEqual(cm('https://c.com/v.mp4', 'video/mp4', '5000000'), { kind: 'video', size: 5000000 })
  assert.deepEqual(cm('https://c.com/v.mp4', 'video/mp4', '600', 'bytes 0-599/9000000'), { kind: 'video', size: 9000000 }) // 分段请求取总大小
  assert.equal(cm('https://c.com/ad.mp4', 'video/mp4', '1000'), null) // 太小
  assert.equal(cm('https://c.com/s1.ts', 'video/mp2t', '900000'), null) // 分片
  assert.deepEqual(cm('https://c.com/a.mp3', 'audio/mpeg', '4000000'), { kind: 'audio', size: 4000000 })
  assert.deepEqual(cm('https://c.com/p.jpg', 'image/jpeg', '300000'), { kind: 'image', size: 300000 })
  assert.equal(cm('https://c.com/icon.png', 'image/png', '2000'), null) // 小图
  assert.equal(cm('https://c.com/p.jpg', 'image/jpeg', ''), null) // 大小未知的图片不记录
  assert.equal(cm('https://c.com/logo.svg', 'image/svg+xml', '900000'), null)
  assert.equal(cm('https://c.com/page.html', 'text/html', '900000'), null)
}

// ---- 嗅探：m3u8
{
  const master = `#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS="avc1.4d401e,mp4a.40.2"
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,AVERAGE-BANDWIDTH=2500000,RESOLUTION=1920x1080
/hi/index.m3u8?t=1
`
  const m = plain(X('parseM3u8')(master, 'https://cdn.com/v/master.m3u8'))
  assert.equal(m.master, true)
  assert.equal(m.variants.length, 2)
  assert.deepEqual([m.variants[0].height, m.variants[0].bandwidth, m.variants[0].url], [1080, 2800000, 'https://cdn.com/hi/index.m3u8?t=1'])
  assert.equal(m.variants[1].url, 'https://cdn.com/v/low/index.m3u8')
  assert.equal(m.variants[1].codecs, 'avc1.4d401e,mp4a.40.2') // 引号里的逗号不截断
  const media = '#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXTINF:9.5,\na.ts\n#EXTINF:10.0,\nb.ts\n#EXTINF:5.25,\nc.ts\n#EXT-X-ENDLIST\n'
  const v = plain(X('parseM3u8')(media, 'https://cdn.com/x.m3u8'))
  assert.deepEqual([v.master, v.durationMs, v.live, v.drm], [false, 24750, false, false])
  assert.equal(plain(X('parseM3u8')(media.replace('#EXT-X-ENDLIST\n', ''), 'https://cdn.com/x.m3u8')).live, true) // 没有结束标记 = 直播
  const aes = media.replace('#EXTINF:9.5', '#EXT-X-KEY:METHOD=AES-128,URI="k.key"\n#EXTINF:9.5')
  assert.equal(plain(X('parseM3u8')(aes, 'https://c.com/x.m3u8')).drm, false) // AES-128 清影能解
  const fps = media.replace('#EXTINF:9.5', '#EXT-X-KEY:METHOD=SAMPLE-AES,KEYFORMAT="com.apple.streamingkeydelivery",URI="skd://x"\n#EXTINF:9.5')
  assert.equal(plain(X('parseM3u8')(fps, 'https://c.com/x.m3u8')).drm, true)
  const wv = media.replace('#EXTINF:9.5', '#EXT-X-KEY:METHOD=SAMPLE-AES-CTR,KEYFORMAT="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"\n#EXTINF:9.5')
  assert.equal(plain(X('parseM3u8')(wv, 'https://c.com/x.m3u8')).drm, true)
  assert.equal(X('parseM3u8')('<html>not a playlist</html>', 'https://c.com/x.m3u8'), null)
}

// ---- 嗅探：mpd
{
  assert.equal(X('isoDurationMs')('PT1H2M3.5S'), 3723500)
  assert.equal(X('isoDurationMs')('PT45S'), 45000)
  assert.equal(X('isoDurationMs')('P1DT1H'), 90000000)
  assert.equal(X('isoDurationMs')('garbage'), 0)
  const mpd = `<?xml version="1.0"?><MPD type="static" mediaPresentationDuration="PT10M0S" xmlns="urn:mpeg:dash:schema:mpd:2011"><Period>
    <AdaptationSet contentType="video"><Representation id="1" bandwidth="1000000" width="1280" height="720"/><Representation id="2" bandwidth="3000000" width="1920" height="1080"/></AdaptationSet>
    <AdaptationSet contentType="audio"><Representation id="3" bandwidth="128000"/></AdaptationSet></Period></MPD>`
  const m = plain(X('parseMpd')(mpd))
  assert.deepEqual([m.durationMs, m.live, m.drm, m.bandwidth, m.height, m.width], [600000, false, false, 3128000, 1080, 1920])
  assert.equal(plain(X('parseMpd')(mpd.replace('type="static"', 'type="dynamic"'))).live, true)
  assert.equal(plain(X('parseMpd')(mpd.replace('<Representation id="1"', '<ContentProtection schemeIdUri="urn:uuid:edef8ba9"/><Representation id="1"'))).drm, true)
  assert.equal(X('parseMpd')('<html/>'), null)
  assert.equal(X('estimateSize')(8_000_000, 10_000), 10_000_000) // 8 Mbps × 10 s = 10 MB
  assert.equal(X('estimateSize')(0, 10_000), 0)
}

// ---- 嗅探：整理清单详情（取回文本的函数用假的）
{
  const files = {
    'https://c.com/m.m3u8': '#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\nhd.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=640x360\nsd.m3u8\n',
    'https://c.com/hd.m3u8': '#EXTM3U\n#EXTINF:60.0,\na.ts\n#EXT-X-ENDLIST\n',
  }
  const fetchText = async (u) => {
    if (!(u in files)) throw new Error('404')
    return files[u]
  }
  const d = plain(await X('describeManifest')('hls', 'https://c.com/m.m3u8', fetchText))
  assert.deepEqual([d.height, d.durationMs, d.size, d.live, d.drm], [720, 60000, 15_000_000, false, false])
  assert.deepEqual(d.variantUrls, ['https://c.com/hd.m3u8', 'https://c.com/sd.m3u8'])
  // 子清单取不到：仍然有清晰度
  delete files['https://c.com/hd.m3u8']
  const d2 = plain(await X('describeManifest')('hls', 'https://c.com/m.m3u8', fetchText))
  assert.deepEqual([d2.height, d2.durationMs, d2.size], [720, 0, 0])
  await assert.rejects(() => X('describeManifest')('hls', 'https://c.com/none.m3u8', fetchText))
  files['https://c.com/live.m3u8'] = '#EXTM3U\n#EXTINF:6,\na.ts\n'
  const live = plain(await X('describeManifest')('hls', 'https://c.com/live.m3u8', fetchText))
  assert.deepEqual([live.live, live.durationMs, live.size], [true, 0, 0])
}

// ---- 嗅探：整理、排序、显示
{
  const at = (n) => 1000 + n
  const list = [
    { url: 'https://c.com/hd.m3u8', kind: 'hls', size: 0, at: at(1) }, // 主清单里的子清单：隐藏
    { url: 'https://c.com/p1.jpg', kind: 'image', size: 200000, at: at(5) },
    { url: 'https://c.com/m.m3u8', kind: 'hls', size: 0, at: at(2), detail: { height: 720, durationMs: 60000, size: 15e6, variantUrls: ['https://c.com/hd.m3u8', 'https://c.com/sd.m3u8'] } },
    { url: 'https://c.com/a.mp3', kind: 'audio', size: 4e6, at: at(3) },
    { url: 'https://c.com/big.mp4?sig=1', kind: 'video', size: 90e6, at: at(4) },
    { url: 'https://c.com/small.mp4', kind: 'video', size: 8e6, at: at(6) },
    { url: 'https://c.com/small.mp4?sig=2', kind: 'video', size: 8e6, at: at(7) }, // 同地址不同参数：重复
  ]
  const order = plain(X('arrangeSniffed')(list)).map((x) => x.url)
  assert.deepEqual(order, ['https://c.com/m.m3u8', 'https://c.com/big.mp4?sig=1', 'https://c.com/small.mp4', 'https://c.com/a.mp3', 'https://c.com/p1.jpg'])
  const L = X('sniffLabel')
  assert.equal(L({ kind: 'hls', size: 0, detail: { height: 720, durationMs: 3723000, size: 15e6 } }), '视频流 (m3u8) · 720p · 1:02:03 · ≈ 14.3 MB')
  assert.equal(L({ kind: 'video', size: 90e6 }), '视频文件 · 85.8 MB')
  assert.equal(L({ kind: 'dash', size: 0, detail: { live: true, drm: true } }), '视频流 (mpd) · 直播 · DRM 加密')
  assert.equal(X('fmtDuration')(65000), '1:05')
  assert.equal(X('fmtSize')(2 * 1073741824), '2.00 GB')
  assert.equal(X('fmtSize')(0), '')
  const payload = X('sniffedPayload')({ url: 'https://c.com/m.mpd?x=1' }, 'https://site.com/play', '标题')
  assert.equal(payload, 'https://c.com/m.mpd?x=1#cc-referer=https%3A%2F%2Fsite.com%2Fplay&cc-title=%E6%A0%87%E9%A2%98')
}

// ---- 嗅探：存储（同地址去重保留详情；媒体和图片各自限量）
{
  const A = X('addSniffed')
  assert.equal(await A(7, { url: 'https://c.com/m.m3u8', kind: 'hls', size: 0, at: 1 }), 1)
  await X('setSniffDetail')(7, 'https://c.com/m.m3u8', { height: 1080 })
  assert.equal(await A(7, { url: 'https://c.com/m.m3u8?again=1', kind: 'hls', size: 0, at: 2 }), 1) // 同地址只算一条
  assert.equal(plain(await X('getSniffed')(7))[0].detail.height, 1080) // 详情还在
  for (let i = 0; i < 40; i++) await A(7, { url: `https://c.com/v${i}.mp4`, kind: 'video', size: 5e6, at: 10 + i })
  for (let i = 0; i < 80; i++) await A(7, { url: `https://c.com/i${i}.jpg`, kind: 'image', size: 2e5, at: 100 + i })
  const all = plain(await X('getSniffed')(7))
  assert.equal(all.filter((x) => x.kind !== 'image').length, 30)
  assert.equal(all.filter((x) => x.kind === 'image').length, 60)
  assert.ok(all.some((x) => x.url === 'https://c.com/v39.mp4')) // 新的保留，旧的挤掉
  assert.ok(!all.some((x) => x.url === 'https://c.com/m.m3u8')) // 最早的 m3u8 已被挤出
}

console.log('extension ok')
