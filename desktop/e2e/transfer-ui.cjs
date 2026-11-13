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
        case 'phone_set_options': return Object.assign({}, ${JSON.stringify(info)}, { peerChat: !!args.options.peerChat })
        case 'phone_chat_list': return [{ id: 1, device: 'd1', name: 'iPhone', text: '到了吗', ts: Math.floor(Date.now() / 1000) - 60 }, { id: 2, device: 'd2', name: 'Android 手机', text: '马上到', ts: Math.floor(Date.now() / 1000) - 30 }]
        case 'transfer_stage_file': return 'D:\\paste\\粘贴的图片.png'
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
  // 微信式输入框：粘贴的文件先放进输入框，点发送才发；顺序 = 放入的顺序
  await page.evaluate(() => (window.__calls.length = 0))
  await page.locator('.dev .el-checkbox').nth(0).click()
  await page.locator('.dev .el-checkbox').nth(1).click()
  await page.fill('.cbar input.tin', '先看这个')
  await page.evaluate(() => {
    const dt = new DataTransfer()
    dt.items.add(new File([new Uint8Array([137, 80, 78, 71])], 'image.png', { type: 'image/png' }))
    const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })
    document.querySelector('.cbar input.tin').dispatchEvent(ev)
  })
  await page.waitForSelector('[data-testid=draft] .dchip.file')
  const chips = await page.locator('[data-testid=draft] .dchip').evaluateAll((els) => els.map((e) => e.dataset.kind))
  ok('粘贴图片后，输入框里先打的文字固定成第 1 项、图片是第 2 项', chips.join(',') === 'text,file', chips.join(','))
  ok('粘贴时不会自动发送', !(await page.evaluate(() => window.__calls.some((c) => c.cmd === 'transfer_send_files' || c.cmd === 'transfer_send_text'))))
  ok('输入框清空，可以接着打字；发送按钮显示项数', (await page.inputValue('.cbar input.tin')) === '' && /发送（2）/.test(await page.locator('.cbar .sendb').innerText()))
  await page.fill('.cbar input.tin', '再说一句')
  await page.click('.cbar .sendb')
  await page.waitForTimeout(400)
  const seq = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'transfer_send_files' || c.cmd === 'transfer_send_text').map((c) => (c.cmd === 'transfer_send_text' ? 'T:' + c.args.text : 'F:' + c.args.paths.join('|'))))
  ok('点发送后按顺序发：文字 → 图片 → 文字', seq.length === 3 && seq[0] === 'T:先看这个' && seq[1].startsWith('F:') && seq[2] === 'T:再说一句', JSON.stringify(seq))
  ok('发完草稿清空', (await page.locator('[data-testid=draft] .dchip').count()) === 0)
  // 退格：输入框是空的时，退格删掉最后一项
  await page.evaluate(() => {
    const dt = new DataTransfer()
    dt.items.add(new File([new Uint8Array([1])], 'a.png', { type: 'image/png' }))
    document.querySelector('.cbar input.tin').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }))
  })
  await page.waitForSelector('[data-testid=draft] .dchip.file')
  await page.focus('.cbar input.tin')
  await page.keyboard.press('Backspace')
  ok('输入框为空时按退格，删掉草稿里的最后一项', (await page.locator('[data-testid=draft] .dchip').count()) === 0)
  // 手机互聊：开关在左栏，打开后右侧出现“手机互聊”页，只读显示全部消息
  ok('互聊没开时没有“手机互聊”页签', (await page.locator('.vtabs').count()) === 0)
  await page.locator('[data-testid=chat-switch]').click()
  await page.waitForSelector('[data-testid=chat-feed]')
  const opt = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'phone_set_options').at(-1)?.args)
  ok('打开开关调用设置接口，并带上 peerChat', opt && opt.options.peerChat === true, JSON.stringify(opt))
  const cm = await page.locator('.cmsg').allInnerTexts()
  ok('显示两台手机的聊天内容和名字', cm.length === 2 && /iPhone/.test(cm[0]) && /到了吗/.test(cm[0]) && /Android 手机/.test(cm[1]), cm.join('|'))
  ok('聊天页是只读的：不显示发送输入条', !(await page.locator('.composer').isVisible()))
  ok('聊天页写明只读', /只读/.test(await page.locator('.ctail').innerText()))
  await page.screenshot({ path: (process.env.SHOT || '/tmp/transfer-desktop.png').replace('.png', '-chat.png') })
  await page.locator('[data-testid=chat-switch]').click()
  await page.waitForTimeout(150)
  ok('关闭开关后聊天页消失、回到传输记录', (await page.locator('.vtabs').count()) === 0 && (await page.locator('.msg').count()) === 5)
  ok('没有页面错误', errors.length === 0, errors.join(';'))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
