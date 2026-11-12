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
    T({ id: 2, name: 'IMG_4819.JPG', kind: 'image', path: 'D:\\r\\IMG_4819.JPG', size: 3200000, createdAt: now - 120 }),
    T({ id: 5, name: 'IMG_0001.HEIC', kind: 'image', path: 'D:\\r\\IMG_0001.HEIC', size: 2200000, createdAt: now - 100 }),
    T({ id: 3, name: '笔记', kind: 'text', text: '记得买牛奶', size: 0, createdAt: now - 30 }),
  ],
  outgoing: [T({ id: 4, direction: 'out', name: '合同扫描件.pdf', size: 12400000, downloads: 1, status: 'ready', expiresAt: now + 23 * 3600, createdAt: now - 10, deviceName: 'iPhone、Android 手机' })],
}
const info = {
  enabled: true, running: true, port: 5173, url: 'http://192.168.1.23:5173/?t=x', apiUrl: null, token: 'x', qrSvg: '<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="#222"/></svg>',
  devices: [
    { id: 'd1', name: 'iPhone', addedAt: now - 9999, lastSeen: now - 5 },
    { id: 'd2', name: 'Android 手机', addedAt: now - 99999, lastSeen: now - 86400 },
    { id: 'ext-x', name: 'Edge 浏览器扩展', addedAt: now - 99999, lastSeen: now - 600 },
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
        case 'transfer_send_files': return { added: 1, skipped: [] }
        case 'transfer_thumb': return /\\.jpg$/i.test(args.path) ? 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==' : null
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
  if (process.env.DARK) await page.evaluate(() => document.documentElement.classList.add('dark'))
  const t = (sel) => page.locator(sel).allInnerTexts()
  ok('顶栏：服务已开启 + 地址', /服务已开启 · 192\.168\.1\.23:5173/.test((await t('.pill'))[0]))
  ok('左栏有二维码、发送对象 3 个', (await page.locator('.qr svg').count()) === 1 && (await page.locator('.dev').count()) === 3)
  const checked = await page.locator('.dev .el-checkbox.is-checked').count()
  ok('默认勾选手机，不勾浏览器扩展', checked === 2 && !(await page.locator('.dev.ext .el-checkbox.is-checked').count()), String(checked))
  ok('顶部输入条写明发给谁', (await page.locator('.tchip').innerText()) === '发给 iPhone、Android 手机')
  const devs = await t('.dev')
  ok('第一台在线，第二台显示最近时间', /在线/.test(devs[0]) && !/在线/.test(devs[1]))
  ok('接收文件夹显示', (await t('.dirbox'))[0].includes('D:\\清影\\手机收到的文件'))
  const bubs = await page.locator('.msg').count()
  ok('传输流 5 条', bubs === 5, String(bubs))
  const order = await page.locator('.msg .meta b').allInnerTexts()
  ok('按时间从旧到新', order[0].includes('IMG_4819') && order[order.length - 1].includes('合同扫描件'), order.join('|'))
  const out = page.locator('.msg.out')
  ok('发出的靠右、写明发给谁、带撤回时间', (await out.count()) === 1 && /发给 iPhone、Android 手机/.test(await out.innerText()) && /手机已下载 1 次/.test(await out.innerText()) && /小时后撤回/.test(await out.innerText()))
  ok('JPG 图片显示缩略图，HEIC 显示文件图标', (await page.locator('.bub img.pic').count()) === 1 && (await page.locator('.msg.in .thumb').count()) >= 2)
  const bx = await out.locator('.bub').boundingBox()
  const bi = await page.locator('.msg.in .bub').first().boundingBox()
  ok('收到的在左、发出的在右', bi.x < bx.x, JSON.stringify({ bi, bx }))
  ok('接收中显示进度', /接收中 61%/.test(await page.locator('.msg.in').filter({ hasText: 'IMG_4821' }).innerText()) && (await page.locator('.el-progress').count()) === 1)
  ok('文字消息直接显示内容并可复制', /记得买牛奶/.test((await t('.txt'))[0]))
  ok('底部是紧凑的一行输入条（没有常驻的大拖放区）', (await page.locator('.composer .cbar').count()) === 1 && (await page.locator('.composer .drop').count()) === 0)
  const ch = (await page.locator('.cbar').boundingBox()).height
  ok('输入条高度不超过 48px', ch <= 48, String(ch))
  await page.fill('.cbar input.tin', '门禁密码 4096')
  await page.click('.cbar .sendb')
  await page.waitForTimeout(200)
  const sent = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'transfer_send_text').at(-1)?.args)
  ok('发送文字调用接口，并带上勾选的发送对象', sent && sent.text === '门禁密码 4096' && sent.targets.length === 2 && !sent.targets.includes('ext-x'), JSON.stringify(sent))
  await page.locator('.dev .el-checkbox').nth(0).click()
  await page.locator('.dev .el-checkbox').nth(1).click()
  ok('一个都没勾时发送按钮不可用', await page.locator('.cbar .sendb').isDisabled())
  const ov = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)
  ok('没有横向滚动', !ov)
  await page.screenshot({ path: process.env.SHOT || '/tmp/transfer-desktop.png' })
  // 有设备在等配对：发送对象里直接能“允许”，不必依赖那个一次性的弹窗
  const pend = await browser.newPage({ viewport: { width: 1180, height: 780 }, deviceScaleFactor: 1 })
  pend.on('pageerror', (e) => errors.push(e.message))
  const withPending = { ...info, devices: [], pending: [{ deviceId: 'p1', name: '小米手机', ip: '192.168.1.9', code: '482913' }] }
  await pend.addInitScript(init.replace(JSON.stringify(info), JSON.stringify(withPending)))
  await pend.goto('http://127.0.0.1:5199/merge-harness.html#transfer')
  await pend.waitForSelector('.xfer .dev.pend')
  const pt = await pend.locator('.dev.pend').innerText()
  ok('待配对的设备显示在发送对象里，带验证码', /小米手机/.test(pt) && /482913/.test(pt), pt)
  ok('有待配对请求时不再显示“还没有配对的设备”', !(await pend.locator('.devs .small').allInnerTexts()).some((x) => /还没有配对/.test(x)))
  await pend.locator('.dev.pend .el-button').click()
  await pend.waitForTimeout(150)
  const resp = await pend.evaluate(() => window.__calls.filter((c) => c.cmd === 'phone_pair_respond').at(-1)?.args)
  ok('点“允许”调用配对接口', resp && resp.deviceId === 'p1' && resp.accept === true, JSON.stringify(resp))
  ok('没有页面错误', errors.length === 0, errors.join(';'))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
