// 真实扩展冒烟测试：把整个 extension/ 加载进 Chromium（真的扩展、真的后台脚本、真的内容脚本），
// 用假的清影后端验证最基本的几件事：后台脚本能启动、整页翻译真的发出请求并出现译文、弹窗“音效”页默认预设是“奥懿”并能存多个自定义预设。
// 和 e2e/ext-real.cjs（排查用，不判断对错）不同，这个脚本有判断，失败时退出码非 0，CI 会跑。
// 需要能开扩展的 Chromium（headless=new 即可，不需要显示环境）：CHROMIUM_PATH 可指向已有的浏览器。
const http = require('http')
const os = require('os')
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')

const ext = path.join(__dirname, '../extension')
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

// 假的清影后端：翻译任务立刻完成，译文 = “译:” + 原文
const jobs = {}
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
      return res.end(JSON.stringify({ id }))
    }
    if (u.pathname === '/api/v1/ai/job') {
      const body = jobs[u.searchParams.get('id')]
      if (body.kind === 'domain') return res.end(JSON.stringify({ state: 'done', result: { domain: 'news', note: 'news' } }))
      return res.end(JSON.stringify({ state: 'done', result: { lines: body.lines.map((l) => ({ t: l.t, text: '译:' + l.text })) } }))
    }
    res.statusCode = 404
    res.end('{}')
  })
})
const site = http.createServer((q, r) => {
  r.setHeader('Content-Type', 'text/html; charset=utf-8')
  r.end('<!doctype html><title>News</title><h1>Breaking news today</h1><p>The quick brown fox jumps over the lazy dog and the story goes on and on.</p><p>Another paragraph that is long enough to be translated by the page.</p>')
})

;(async () => {
  await new Promise((r) => backend.listen(0, '127.0.0.1', r))
  await new Promise((r) => site.listen(0, '127.0.0.1', r))
  const port = backend.address().port
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pw-ext-smoke-'))
  const ctx = await chromium.launchPersistentContext(dir, {
    headless: false,
    executablePath: process.env.CHROMIUM_PATH || undefined,
    args: ['--headless=new', '--no-sandbox', `--disable-extensions-except=${ext}`, `--load-extension=${ext}`],
  })
  const errors = []
  try {
    // 1. 后台脚本启动，扩展 id 拿得到
    let sw = ctx.serviceWorkers()[0]
    if (!sw) sw = await ctx.waitForEvent('serviceworker', { timeout: 20000 }).catch(() => null)
    ok('扩展的后台脚本启动了', !!sw)
    if (!sw) throw new Error('后台脚本没有启动，后面的检查都做不了')
    const extId = new URL(sw.url()).host
    const manifest = JSON.parse(fs.readFileSync(path.join(ext, 'manifest.json'), 'utf8'))
    ok('后台脚本里读到的版本和 manifest.json 一致', (await sw.evaluate(() => chrome.runtime.getManifest().version)) === manifest.version)
    await sw.evaluate(
      async (port) => {
        await chrome.storage.local.set({ port, token: 't', device: 'd', transCfg: { engine: 'clearclip', target: 'zh-CN', mode: 'bilingual' } })
      },
      port,
    )

    // 2. 整页翻译：内容脚本被注入、请求发给后端、译文出现在页面上
    const page = await ctx.newPage()
    page.on('pageerror', (e) => errors.push(e.message))
    await page.goto(`http://127.0.0.1:${site.address().port}/`)
    const started = await sw.evaluate(async () => {
      const tab = (await chrome.tabs.query({})).find((x) => /^http:\/\/127\.0\.0\.1/.test(x.url))
      try {
        await webTransDo(tab, 'page-on')
        return 'ok'
      } catch (e) {
        return 'ERR ' + e.message
      }
    })
    ok('整页翻译：后台脚本把翻译脚本注入了页面', started === 'ok', started)
    let found = null
    for (let i = 0; i < 40 && !found; i++) {
      await sleep(500)
      found = await page.evaluate(() => (document.body.innerText.match(/译:[^\n]*/) || [])[0] || null)
    }
    ok('整页翻译：页面上出现了译文', !!found, '20 秒内没有译文')
    ok('整页翻译：后端收到了翻译任务', Object.values(jobs).some((j) => j.kind !== 'domain' && Array.isArray(j.lines) && j.lines.length > 0))

    // 3. 弹窗“音效”页：默认预设“奥懿”，可以存多个自定义预设
    const popup = await ctx.newPage()
    popup.on('pageerror', (e) => errors.push('popup: ' + e.message))
    await popup.goto(`chrome-extension://${extId}/popup.html`)
    await popup.waitForSelector('[data-tab="audio"]', { timeout: 10000 })
    await popup.click('[data-tab="audio"]')
    await popup.waitForSelector('#auPresets button', { timeout: 10000 })
    const pressed = await popup.locator('#auPresets [aria-pressed=true]').allInnerTexts()
    ok('音效页：默认预设是“奥懿 -11”', pressed.length === 1 && /^奥懿\s*-11$/.test(pressed[0]), JSON.stringify(pressed))
    const vals = [await popup.locator('#auTargetV').innerText(), await popup.locator('#auCompV').innerText(), await popup.locator('#auMaxV').innerText()]
    ok('音效页：奥懿 = -11 LUFS、15%、+24 dB', vals.join('|') === '-11 LUFS|15%|+24 dB', vals.join('|'))
    const groups = await popup.locator('.auGroup > .mute').allInnerTexts()
    ok('音效页：预设按用途分组', ['常用', '平台与内容', '电视广播', '环境', '我的预设'].every((g) => groups.includes(g)), groups.join(','))

    await popup.locator('#auTarget').evaluate((e) => {
      e.value = -13
      e.dispatchEvent(new Event('input'))
    })
    await popup.fill('#auName', '会议')
    await popup.click('#auSave')
    await sleep(200)
    await popup.locator('#auTarget').evaluate((e) => {
      e.value = -18
      e.dispatchEvent(new Event('input'))
    })
    await popup.fill('#auName', '睡前')
    await popup.click('#auSave')
    await sleep(300)
    const stored = await sw.evaluate(async () => {
      const { audioCfg } = await chrome.storage.local.get('audioCfg')
      return audioCfg
    })
    ok('音效页：存了两个自定义预设，保存到了扩展存储里', stored?.userPresets?.length === 2 && stored.userPresets.map((p) => p.name).join() === '会议,睡前', JSON.stringify(stored?.userPresets))
    ok('音效页：存完选中最后存的那个', stored?.preset === stored?.userPresets?.[1]?.id && stored?.target === -18)
    // 重新打开弹窗：自定义预设还在，选中的还是它
    await popup.reload()
    await popup.waitForSelector('[data-tab="audio"]')
    await popup.click('[data-tab="audio"]')
    await popup.waitForSelector('#auPresets button')
    const mine = await popup.locator('.auGroup').last().locator('button').allInnerTexts()
    ok('音效页：重新打开后“我的预设”里还有这两个', /会议/.test(mine.join()) && /睡前/.test(mine.join()), mine.join(','))
    ok('页面和弹窗没有脚本错误', errors.length === 0, errors.join(' | '))
  } catch (e) {
    ok('没有中途出错', false, e.message)
  } finally {
    await ctx.close().catch(() => {})
    backend.close()
    site.close()
  }
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})()
