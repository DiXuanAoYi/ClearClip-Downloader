// B 站字幕：字幕文件在 hdslb.com（另一个域名），服务器用 Access-Control-Allow-Origin: * 时，
// 带 Cookie 的跨域请求会被浏览器拦成 “Failed to fetch”。验证扩展不带 Cookie 取，并且在页面取不到时由后台代取。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const FILES = ['content/ui.js', 'vsum-logic.js', 'content/vsum-source.js'].map((f) => path.join(EXT, f))
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const json = (route, body, headers = {}) => route.fulfill({ contentType: 'application/json; charset=utf-8', headers: { 'access-control-allow-origin': 'https://www.bilibili.com', 'access-control-allow-credentials': 'true', ...headers }, body: JSON.stringify(body) })

;(async () => {
  const browser = await chromium.launch()
  for (const mode of ['wildcard', 'blocked']) {
    const ctx = await browser.newContext()
    await ctx.route('https://www.bilibili.com/**', (r) => r.fulfill({ contentType: 'text/html; charset=utf-8', body: '<!doctype html><title>t</title><body><video></video></body>' }))
    await ctx.route('https://api.bilibili.com/x/web-interface/view**', (r) => json(r, { data: { title: '测试视频', duration: 30, cid: 1, pages: [{ cid: 1, duration: 30, part: 'P1' }] } }))
    await ctx.route('https://api.bilibili.com/x/player/**', (r) => json(r, { data: { subtitle: { subtitles: [{ subtitle_url: '//aisubtitle.hdslb.com/bfs/ai_subtitle/x.json', lan: 'ai-zh', lan_doc: '中文（自动生成）', ai_type: 1 }] } } }))
    const body = { body: [{ from: 1, to: 3, content: '大家好，欢迎来到这个视频' }, { from: 4, to: 6, content: '今天我们聊聊字幕' }] }
    // wildcard：允许任意来源但不允许带凭据（带 Cookie 的请求会失败）；blocked：页面里怎么取都被拦，只能靠后台代取
    await ctx.route('https://aisubtitle.hdslb.com/**', (r) => (mode === 'blocked' ? r.abort() : json(r, body, { 'access-control-allow-origin': '*', 'access-control-allow-credentials': '' })))
    const page = await ctx.newPage()
    await page.addInitScript((m) => {
      window.chrome = { runtime: { sendMessage: async (msg) => (msg.type === 'cc-fetch-sub' ? { ok: true, text: JSON.stringify({ body: [{ from: 1, to: 3, content: '后台代取的字幕' }] }) } : { ok: false }) } }
    }, mode)
    await page.goto('https://www.bilibili.com/video/BV1xx411c7mD')
    for (const f of FILES) await page.addScriptTag({ path: f })
    const r = await page.evaluate(() => globalThis.ccVsumSource.load())
    if (mode === 'wildcard') ok('跨域允许任意来源时能读到 AI 字幕（不带 Cookie 取）', r.lines.length >= 1 && r.lines[0].text.includes('大家好') && !r.fetchFailed, JSON.stringify(r))
    else ok('页面里取不到时由后台代取', r.lines.length === 1 && r.lines[0].text === '后台代取的字幕' && !r.fetchFailed, JSON.stringify(r))
    await ctx.close()
  }
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
