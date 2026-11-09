// 手机互传（电脑端）：左连接栏 + 右传输流 + 底部输入区。假后端按命令名回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))
const now = Math.floor(Date.now() / 1000)
const T = (o) => ({ id: 0, direction: 'in', kind: 'file', name: 'x', path: 'D:\\r\\x', text: '', size: 1000, done: 1000, status: 'done', message: null, extra: null, deviceName: 'iPhone', downloads: 0, createdAt: now, expiresAt: 0, active: false, missing: false, ...o })
const list = {
  receiveDir: 'D:\\清影\\手机收到的文件',
  incoming: [
    T({ id: 1, name: 'IMG_4821.MOV', kind: 'video', size: 186000000, done: 115000000, status: 'receiving', createdAt: now - 60 }),
    T({ id: 2, name: 'IMG_4819.HEIC', kind: 'image', size: 3200000, createdAt: now - 120 }),
    T({ id: 3, name: '笔记', kind: 'text', text: '记得买牛奶', size: 0, createdAt: now - 30 }),
  ],
  outgoing: [T({ id: 4, direction: 'out', name: '合同扫描件.pdf', size: 12400000, downloads: 1, status: 'ready', expiresAt: now + 23 * 3600, createdAt: now - 10 })],
}
const info = {
  enabled: true, running: true, port: 5173, url: 'http://192.168.1.23:5173/?t=x', apiUrl: null, token: 'x', qrSvg: '<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="#222"/></svg>',
  devices: [
    { id: 'd1', name: 'iPhone', addedAt: now - 9999, lastSeen: now - 5 },
    { id: 'd2', name: 'Android 手机', addedAt: now - 99999, lastSeen: now - 86400 },
  ],
  pending: [], error: null, ip: '192.168.1.23', host: '', addresses: [], suspicious: false, receiveDir: 'D:\\清影\\手机收到的文件', maxUploadMb: 2048, autoExtract: true, outboxHours: 24,
}
const init = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  window.__calls = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': return evId++
        case 'get_settings': return ${JSON.stringify(settings)}
        case 'transfer_list': return ${JSON.stringify(list)}
        case 'phone_info': return ${JSON.stringify(info)}
        case 'transfer_send_text': return 5
        default: return null
      }
    },
  }
})()
`
;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const page = await browser.newPage({ viewport: { width: 1180, height: 780 }, deviceScaleFactor: 1 })
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(init)
  await page.goto('http://127.0.0.1:5199/merge-harness.html#transfer')
  await page.waitForSelector('.xfer .bub')
  const t = (sel) => page.locator(sel).allInnerTexts()
  ok('顶栏：服务已开启 + 地址', /服务已开启 · 192\.168\.1\.23:5173/.test((await t('.pill'))[0]))
  ok('左栏有二维码、已配对 2 台', (await page.locator('.qr svg').count()) === 1 && (await page.locator('.dev').count()) === 2)
  const devs = await t('.dev')
  ok('第一台在线，第二台显示最近时间', /在线/.test(devs[0]) && !/在线/.test(devs[1]))
  ok('接收文件夹显示', (await t('.dirbox'))[0].includes('D:\\清影\\手机收到的文件'))
  const bubs = await page.locator('.msg').count()
  ok('传输流 4 条', bubs === 4, String(bubs))
  const order = await page.locator('.msg .meta b').allInnerTexts()
  ok('按时间从旧到新', order[0].includes('IMG_4819') && order[order.length - 1].includes('合同扫描件'), order.join('|'))
  const out = page.locator('.msg.out')
  ok('发出的靠右、带撤回时间', (await out.count()) === 1 && /手机已下载 1 次/.test(await out.innerText()) && /小时后撤回/.test(await out.innerText()))
  const bx = await out.locator('.bub').boundingBox()
  const bi = await page.locator('.msg.in .bub').first().boundingBox()
  ok('收到的在左、发出的在右', bi.x < bx.x, JSON.stringify({ bi, bx }))
  ok('接收中显示进度', /接收中 61%/.test(await page.locator('.msg.in').filter({ hasText: 'IMG_4821' }).innerText()) && (await page.locator('.el-progress').count()) === 1)
  ok('文字消息直接显示内容并可复制', /记得买牛奶/.test((await t('.txt'))[0]))
  ok('底部拖放区和输入区', (await page.locator('.composer .drop').count()) === 1 && (await page.locator('.send-row input').count()) === 1)
  await page.fill('.send-row input', '门禁密码 4096')
  await page.click('.send-row .el-button--primary')
  await page.waitForTimeout(200)
  const sent = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'transfer_send_text').at(-1)?.args)
  ok('发送文字调用接口', sent && sent.text === '门禁密码 4096', JSON.stringify(sent))
  const ov = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)
  ok('没有横向滚动', !ov)
  await page.screenshot({ path: process.env.SHOT || '/tmp/transfer-desktop.png' })
  ok('没有页面错误', errors.length === 0, errors.join(';'))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
