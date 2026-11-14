// 真实扩展端到端：把 extension/ 加载进 Chromium，用假的清影后端验证“整页翻译”真的发出请求并出现译文。需要显示环境（xvfb-run -a node e2e/ext-real.cjs ../extension ok）。不在默认套件里。
const http = require('http'), os = require('os'), fs = require('fs'), path = require('path')
const { chromium } = require('playwright')
const [ext = require("path").join(__dirname, "../extension"), domainMode = "ok"] = process.argv.slice(2)
const jobs = {}, log = []
let n = 0
const backend = http.createServer((req, res) => {
  let b = ''
  req.on('data', (d) => (b += d))
  req.on('end', () => {
    res.setHeader('Content-Type', 'application/json')
    const u = new URL(req.url, 'http://x')
    if (u.pathname === '/api/v1/ai/run') {
      const body = JSON.parse(b || '{}')
      const id = String(++n)
      jobs[id] = body
      log.push(`${Date.now() - t0}ms run ${body.kind}`)
      return res.end(JSON.stringify({ id }))
    }
    if (u.pathname === '/api/v1/ai/job') {
      const body = jobs[u.searchParams.get('id')]
      if (body.kind === 'domain') return res.end(JSON.stringify(domainMode === 'error' ? { state: 'error', error: '额度用完' } : { state: 'done', result: { domain: 'news', note: 'news' } }))
      return res.end(JSON.stringify({ state: 'done', result: { lines: body.lines.map((l) => ({ t: l.t, text: '译:' + l.text })) } }))
    }
    res.statusCode = 404; res.end('{}')
  })
})
const site = http.createServer((q, r) => {
  r.setHeader('Content-Type', 'text/html; charset=utf-8')
  r.end('<!doctype html><title>News</title><h1>Breaking news today</h1><p id=p1>The quick brown fox jumps over the lazy dog and the story goes on and on.</p><p>Another paragraph that is long enough to be translated by the page.</p>')
})
let t0
;(async () => {
  await new Promise((r) => backend.listen(0, '127.0.0.1', r))
  await new Promise((r) => site.listen(0, '127.0.0.1', r))
  const port = backend.address().port
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pw-ext-'))
  const ctx = await chromium.launchPersistentContext(dir, { headless: false, args: ['--headless=new', '--no-sandbox', `--disable-extensions-except=${ext}`, `--load-extension=${ext}`] })
  let sw = ctx.serviceWorkers()[0] || (await ctx.waitForEvent('serviceworker', { timeout: 15000 }))
  await sw.evaluate(async (port) => { await chrome.storage.local.set({ port, token: 't', device: 'd', transCfg: { engine: 'clearclip', target: 'zh-CN', mode: 'bilingual' } }) }, port)
  const page = await ctx.newPage()
  await page.goto(`http://127.0.0.1:${site.address().port}/`)
  t0 = Date.now()
  const r = await sw.evaluate(async () => {
    const [tab] = await chrome.tabs.query({ active: true, url: 'http://127.0.0.1/*' }).catch(() => [])
    const tabs = await chrome.tabs.query({})
    const t = tabs.find((x) => /^http:\/\/127\.0\.0\.1/.test(x.url))
    try { return JSON.stringify(await webTransDo(t, 'page-on')) } catch (e) { return 'ERR ' + e.message }
  })
  console.log('page-on ->', r)
  console.log('inject:', await sw.evaluate(async () => { const t=(await chrome.tabs.query({})).find((x)=>/^http:\/\/127/.test(x.url)); try { const res = await chrome.scripting.executeScript({target:{tabId:t.id},files:ccFiles.GROUPS.trans}); return JSON.stringify(res)+' | '+JSON.stringify(ccFiles.GROUPS.trans) } catch(e) { return 'ERR '+e.message } }))
  await new Promise((r) => setTimeout(r, 3000))
  console.log('probe:', await sw.evaluate(async () => { const t=(await chrome.tabs.query({})).find((x)=>/^http:\/\/127/.test(x.url)); const res = await chrome.scripting.executeScript({target:{tabId:t.id},func:()=>JSON.stringify({has:!!globalThis.ccWebTrans, ui:!!globalThis.ccUI, tr:document.querySelectorAll('[data-cc-tr]').length, vis:document.visibilityState, txt:document.body.innerText.slice(0,120)})}); return res[0].result }))
  const d = await sw.evaluate(async () => { try { const t0=Date.now(); const x = await translateTexts(['Hello world, this is a test sentence.'], { target: 'zh-CN', engine: 'clearclip', page: {host:'x.com',title:'T',sample:'some sample text here'}, domain: true }); return JSON.stringify(x)+' '+(Date.now()-t0)+'ms' } catch (e) { return 'ERR ' + e.message } })
  console.log('direct translateTexts ->', d)
  page.on('console', (m) => console.log('page:', m.text()))
  console.log('content script present:', await page.evaluate(() => !!document.querySelector('[data-cc-tr]') || typeof window.__ccwt))
  let found = null
  for (let i = 0; i < 100 && !found; i++) {
    await new Promise((r) => setTimeout(r, 500))
    found = await page.evaluate(() => [...document.querySelectorAll('*')].map((e) => e.textContent).find((t) => t && t.startsWith('译:')) || (window.__x = null))
    if (!found) found = await page.evaluate(() => (document.body.innerText.match(/译:[^\n]*/) || [])[0])
  }
  console.log(found ? `翻译出现，用时 ${Date.now() - t0}ms: ${found.slice(0, 40)}` : `100 秒内没有任何译文`)
  console.log(log.join('\n'))
  await ctx.close(); process.exit(0)
})().catch((e) => { console.log('FAIL', e.message); process.exit(1) })
