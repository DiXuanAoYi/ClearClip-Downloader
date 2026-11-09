// 手机网页（隔空传送）：配对页、对话流、电脑发来的卡片。用假的接口回应。
const { chromium } = require('playwright')
const fs = require('fs')
const path = require('path')
let pass = 0
let fail = 0
const ok = (c, m) => (c ? (pass++, console.log('  ok  ' + m)) : (fail++, console.log('  FAIL ' + m)))
const html = fs.readFileSync(path.join(__dirname, '../src-tauri/src/phone_page.html'), 'utf8').replace('__TOKEN__', 'tok')

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 1 })
  const page = await ctx.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  let paired = false
  const sent = []
  await page.route('http://phone.test/**', async (route) => {
    const u = new URL(route.request().url())
    const j = (o) => route.fulfill({ contentType: 'application/json', body: JSON.stringify(o) })
    if (u.pathname === '/') return route.fulfill({ contentType: 'text/html', body: html })
    if (u.pathname === '/api/hello') return j({ paired, code: paired ? '' : '472913', pcName: '办公室电脑', maxMb: 2048 })
    if (u.pathname === '/api/send') return (sent.push(route.request().postData()), j({ state: 'queued' }))
    if (u.pathname === '/api/box')
      return j({
        out: [
          { key: 'a', kind: 'file', name: '合同扫描件.pdf', size: 12400000, downloads: 1, active: false, sent: 0 },
          { key: 'b', kind: 'video', name: '东京夜景.mp4', size: 348000000, downloads: 0, active: true, sent: 143000000 },
          { key: 'c', kind: 'text', name: '', text: '门禁密码 4096', size: 0 },
        ],
        in: [{ id: 1, name: 'IMG_4819.HEIC', kind: 'image', size: 3200000, state: 'done', message: '', at: 100 }],
        links: [{ id: 2, text: 'https://b23.tv/x', title: '东京夜景延时', state: 'queued', message: '', at: 200 }],
      })
    return j({})
  })
  await page.goto('http://phone.test/')
  await page.waitForSelector('#code:not(:empty)')
  ok((await page.textContent('#code')) === '472 913', '配对页显示验证码')
  ok(await page.locator('#pairV').isVisible(), '未配对时显示等待配对')
  ok(!(await page.locator('#comp').isVisible()), '未配对时隐藏输入区')
  ok((await page.textContent('#stT')).includes('等待电脑确认'), '状态条：等待电脑确认')

  paired = true
  await page.waitForSelector('#feed .bub', { timeout: 8000 })
  ok(!(await page.locator('#pairV').isVisible()), '配对后进入对话流')
  ok((await page.textContent('#stT')).includes('已连接 · 办公室电脑'), '状态条显示电脑名')
  const bubs = await page.locator('#feed .bub').allTextContents()
  ok(bubs.length === 2 && bubs[0].includes('IMG_4819') && bubs[1].includes('东京夜景延时'), '对话流按时间排：' + bubs.length)
  ok(await page.locator('#comp').isVisible(), '底部输入区可见')
  ok((await page.locator('#comp .acts button').count()) === 3, '三个快捷按钮')

  await page.fill('#text', 'https://www.bilibili.com/video/BV1')
  await page.click('#send')
  await page.waitForFunction(() => document.getElementById('msg').textContent === '已发送')
  ok(sent.length === 1 && sent[0].includes('bilibili'), '发送链接')

  // 上传：选一个小文件（接口没有实现 /api/upload，会失败，用来看错误气泡与移除）
  await page.setInputFiles('#inFile', { name: 'a.zip', mimeType: 'application/zip', buffer: Buffer.alloc(100) })
  await page.waitForFunction(() => /a\.zip/.test(document.getElementById('feed').textContent))
  ok(true, '上传条目出现在对话流')

  await page.click('#tabGet')
  await page.waitForSelector('#outs .card')
  ok((await page.locator('#outs .card').count()) === 3, '电脑发来的三张卡片')
  ok((await page.locator('#outs .card a.dl').count()) === 2, '文件有下载按钮，文字有复制')
  ok((await page.textContent('#outs')).includes('电脑正在发送 41%'), '显示电脑正在发送的百分比')
  ok(!(await page.locator('#comp').isVisible()) && (await page.locator('#foot').isVisible()), '电脑发来的页隐藏输入区、显示说明')

  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)
  ok(!overflow, '没有横向滚动')
  await page.screenshot({ path: process.env.SHOT || '/tmp/phone-get.png' })
  await page.click('#tabSend')
  await page.screenshot({ path: (process.env.SHOT || '/tmp/phone-get.png').replace('get', 'send') })
  ok(errors.length === 0, '没有页面错误' + (errors.length ? '：' + errors.join(';') : ''))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
