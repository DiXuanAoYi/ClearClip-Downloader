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
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 1, colorScheme: process.env.DARK ? 'dark' : 'light' })
  const page = await ctx.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  let paired = false
  const sent = []
  let chatOn = false
  const chatMsgs = []
  await page.route('http://phone.test/**', async (route) => {
    const u = new URL(route.request().url())
    const j = (o) => route.fulfill({ contentType: 'application/json', body: JSON.stringify(o) })
    if (u.pathname === '/') return route.fulfill({ contentType: 'text/html', body: html })
    if (u.pathname === '/api/hello') return j({ paired, code: paired ? '' : '472913', pcName: '办公室电脑', maxMb: 2048 })
    if (u.pathname === '/api/thumb') return route.fulfill({ contentType: 'image/png', body: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64') })
    if (u.pathname === '/api/chat') {
      if (route.request().method() === 'POST') {
        const b = JSON.parse(route.request().postData())
        const m = { id: chatMsgs.length + 1, device: b.device, name: 'iPhone', text: b.text, ts: 1 }
        chatMsgs.push(m)
        return j({ message: m })
      }
      const since = Number(u.searchParams.get('since') || 0)
      return j({ enabled: chatOn, messages: chatOn ? chatMsgs.filter((m) => m.id > since) : [] })
    }
    if (u.pathname === '/api/send') return (sent.push(route.request().postData()), j({ state: 'queued' }))
    if (u.pathname === '/api/box')
      return j({
        out: [
          { key: 'a', kind: 'file', name: '合同扫描件.pdf', size: 12400000, downloads: 1, active: false, sent: 0 },
          { key: 'p', kind: 'image', name: '会议白板.jpg', size: 2100000, downloads: 0, active: false, sent: 0, thumb: true },
          { key: 'b', kind: 'video', name: '东京夜景.mp4', size: 348000000, downloads: 0, active: true, sent: 143000000 },
          { key: 'c', kind: 'text', name: '', text: '门禁密码 4096', size: 0 },
        ],
        in: [
          { id: 1, name: 'IMG_4819.JPG', kind: 'image', size: 3200000, state: 'done', message: '', at: 100, thumb: true },
          { id: 9, name: 'a.zip', kind: 'archive', size: 100, state: 'done', message: '', at: 300, thumb: false },
        ],
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
  ok(bubs.length === 3 && bubs[0].includes('IMG_4819') && bubs[0].includes('已保存到电脑') && bubs[1].includes('东京夜景延时'), '对话流按时间排：' + bubs.join('|'))
  ok((await page.locator('#feed .bub img.pic').first().getAttribute('src')).includes('/api/thumb?id=1&device='), '图片记录显示缩略图（走 /api/thumb）')
  ok(await page.locator('#comp').isVisible(), '底部输入区可见')
  ok((await page.locator('#comp .in').count()) === 1 && (await page.locator('#sheetBg').isHidden()), '输入区是单行，选择面板默认收着')
  await page.click('#plus')
  ok((await page.locator('#sheet .act').count()) === 3 && !(await page.locator('#sheetBg').isHidden()), '点 + 打开选择面板：照片和视频 / 文件 / 粘贴')
  await page.mouse.click(190, 40)
  ok(await page.locator('#sheetBg').isHidden(), '点空白处收起面板')

  await page.fill('#text', 'https://www.bilibili.com/video/BV1')
  await page.click('#send')
  await page.waitForFunction(() => document.getElementById('text').value === '')
  ok(sent.length === 1 && sent[0].includes('bilibili'), '发送链接')

  // 上传：选一个小文件（接口没有实现 /api/upload，会失败，用来看错误气泡与移除）
  await page.setInputFiles('#inFile', { name: 'a.zip', mimeType: 'application/zip', buffer: Buffer.alloc(100) })
  await page.waitForFunction(() => /a\.zip/.test(document.getElementById('feed').textContent))
  ok(true, '上传条目出现在对话流')
  // 同一个文件只留一条：上传的这条（名字、大小都和电脑记录里的 a.zip 一样）和记录里的不重复显示
  await page.waitForTimeout(300)
  const zips = await page.locator('#feed .bub', { hasText: 'a.zip' }).count()
  ok(zips === 1, '同一个文件只显示一条（不会一条“已完成”一条“已保存到电脑”）：' + zips)

  await page.click('#tabGet')
  await page.waitForSelector('#outs .card')
  ok((await page.locator('#outs .card').count()) === 4, '电脑发来的四张卡片')
  ok((await page.locator('#outs .card img.pic').first().getAttribute('src')).includes('/api/thumb?key=p'), '电脑发来的图片带缩略图')
  ok((await page.locator('#outs .card a.dl').count()) === 3, '文件有下载按钮，文字有复制')
  ok((await page.textContent('#outs')).includes('电脑正在准备 41%'), '显示电脑正在发送的百分比')
  ok(!(await page.locator('#comp').isVisible()) && (await page.locator('#foot').isVisible()), '电脑发来的页隐藏输入区、显示说明')

  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)
  ok(!overflow, '没有横向滚动')
  await page.screenshot({ path: process.env.SHOT || '/tmp/phone-get.png' })
  await page.click('#tabSend')
  await page.screenshot({ path: (process.env.SHOT || '/tmp/phone-get.png').replace('get', 'send') })
  // 手机互聊：电脑端开关关闭时没有这个页；打开后出现，别的手机的消息会显示，自己发的靠右
  ok(await page.locator('#tabChat').isHidden(), '互聊没开时不显示“手机互聊”页')
  chatOn = true
  chatMsgs.push({ id: 1, device: 'other', name: '小米手机', text: '到了吗', ts: 1 })
  await page.waitForSelector('#tabChat:not([hidden])', { timeout: 9000 })
  ok(await page.locator('#tabChat').isVisible(), '电脑端打开互聊后出现“手机互聊”页')
  ok((await page.textContent('#cbadge')) === '1', '别的手机发来消息时标签上有未读数')
  await page.click('#tabChat')
  await page.waitForSelector('#cfeed .cm')
  ok((await page.locator('#cfeed .cm').count()) === 1 && (await page.textContent('#cfeed')).includes('小米手机'), '显示别的手机的名字和内容')
  ok(await page.locator('#cComp').isVisible() && !(await page.locator('#comp').isVisible()), '互聊页用自己的输入框')
  ok((await page.textContent('#cfeed')).includes('电脑端能看到全部内容'), '页面里明确写明电脑能看到')
  await page.fill('#ctext', '马上到')
  await page.click('#csend')
  await page.waitForSelector('#cfeed .cm.me')
  ok((await page.textContent('#cfeed .cm.me')).includes('马上到'), '自己发的消息靠右显示')
  chatOn = false
  await page.waitForSelector('#tabChat', { state: 'hidden', timeout: 9000 })
  ok((await page.locator('#vSend').isVisible()), '电脑端关闭互聊后回到发送页')
  ok(errors.length === 0, '没有页面错误' + (errors.length ? '：' + errors.join(';') : ''))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
