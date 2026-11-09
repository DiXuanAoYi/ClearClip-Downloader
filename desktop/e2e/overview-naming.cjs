// 页面媒体总览：命名栏（保持原名 / 自定义模板 / AI 命名 / 单项手改）和发送时带上名字。
const { chromium } = require('playwright')
const path = require('path')
let pass = 0
let fail = 0
const ok = (c, m) => (c ? (pass++, console.log('  ok  ' + m)) : (fail++, console.log('  FAIL ' + m)))

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(() => {
    const items = [
      { kind: 'image', url: 'https://x.com/a/one.jpg', w: 800, h: 600, alt: '海边日落', cap: '摄于三亚', near: '三亚旅行攻略' },
      { kind: 'image', url: 'https://x.com/a/two.jpg', w: 800, h: 600, alt: '' },
      { kind: 'video', url: 'https://x.com/v/clip.mp4', alt: '' },
    ]
    const store = { k1: { page: 'https://www.x.com/p', title: '旅行', tabId: 1, items, sniffed: [], excerpt: '三亚三日游：海边日落、礁石和夜市' } }
    const local = {}
    window.__sent = []
    const get = (src) => async (k) => (typeof k === 'string' ? (k in src ? { [k]: src[k] } : {}) : {})
    window.chrome = {
      storage: { session: { get: get(store), remove() {} }, local: { get: get(local), set: async (o) => Object.assign(local, o) } },
      runtime: {
        sendMessage: async (m) => {
          window.__sent.push(m)
          if (m.type === 'cc-ai-name') return { ok: true, names: m.items.map((_, i) => ['日落海面', '礁石特写', '片头动画'][i]) }
          if (m.type === 'cc-probe-media') return { ok: true, ext: 'jpg', mime: 'image/jpeg', size: 1000 }
          return { ok: true, sent: m.items ? m.items.length : 0, failed: 0 }
        },
      },
      tabs: {},
    }
  })
  await page.goto('file://' + path.join(__dirname, '../extension/overview.html') + '?k=k1')
  await page.waitForSelector('.cell')
  await page.selectOption('#minSize', '0')
  const cells = page.locator('.cell')
  ok((await cells.count()) === 3, '三项都显示出来')
  ok(!(await page.locator('.cell .nm').first().isVisible()), '没选中时不显示改名框')

  await page.click('#selAll')
  ok(await page.locator('.cell.on .nm').first().isVisible(), '选中后显示改名框')
  ok((await page.textContent('#nameHint')).includes('保持原来'), '默认保持原名')

  // 模板
  await page.selectOption('#nameMode', 'tpl')
  ok(await page.locator('#tpl').isVisible(), '自定义模板显示输入框')
  await page.fill('#tpl', '{序号}-{标题}')
  const v = await page.locator('.cell .nm').evaluateAll((els) => els.map((e) => e.value))
  ok(v.join('|') === '01-旅行|02-旅行-2|03-旅行-3' || v.join('|') === '01-旅行|02-旅行|03-旅行', '模板生成了名字：' + v.join('|'))
  ok((await page.textContent('#nameHint')).startsWith('例：'), '有预览提示')
  await page.click('#tplChips button:first-child')
  ok((await page.inputValue('#tpl')).includes('{序号}{序号}') || (await page.inputValue('#tpl')).includes('{序号}'), '点占位符会插入模板')
  await page.fill('#tpl', '{原名}')

  // 手动改一项
  await page.locator('.cell .nm').nth(1).fill('我的封面')
  await page.click('#sendSel')
  await page.waitForFunction(() => window.__sent.some((m) => m.type === 'cc-send-items'))
  let sent = await page.evaluate(() => window.__sent.filter((m) => m.type === 'cc-send-items').pop().items)
  ok(sent[0].name === 'one' && sent[1].name === '我的封面' && sent[2].name === 'clip', '发送带上模板名和手改名：' + JSON.stringify(sent.map((x) => x.name)))

  // AI
  await page.selectOption('#nameMode', 'ai')
  ok(await page.locator('#aiName').isVisible(), 'AI 方式显示起名按钮')
  await page.click('#aiName')
  await page.waitForFunction(() => window.__sent.some((m) => m.type === 'cc-ai-name'))
  const req = await page.evaluate(() => window.__sent.find((m) => m.type === 'cc-ai-name'))
  ok(req.items.length === 3 && req.items[0].alt === '海边日落' && req.items[0].orig === 'one.jpg' && req.title === '旅行', 'AI 请求带上说明、原名和网页标题')
  ok(req.items[0].cap === '摄于三亚' && req.items[0].near === '三亚旅行攻略' && req.items[0].url === 'https://x.com/a/one.jpg', 'AI 请求带上图片说明、周围文字和地址')
  ok(req.excerpt.includes('三亚三日游') && req.ocr === true, 'AI 请求带上页面正文摘录，并默认读图里的文字')
  await page.waitForFunction(() => document.querySelectorAll('.cell .nm.ai').length > 0)
  const a = await page.locator('.cell .nm').evaluateAll((els) => els.map((e) => e.value))
  ok(a[0] === '日落海面' && a[1] === '我的封面' && a[2] === '片头动画', '用 AI 的名字，手改的优先：' + a.join('|'))
  await page.click('#sendSel')
  await page.waitForFunction(() => window.__sent.filter((m) => m.type === 'cc-send-items').length === 2)
  sent = await page.evaluate(() => window.__sent.filter((m) => m.type === 'cc-send-items').pop().items)
  ok(sent[0].name === '日落海面' && sent[2].name === '片头动画', '发送用 AI 名字')

  // 保持原名：没有手改的项不带 name
  await page.locator('.cell .nm').nth(1).fill('')
  await page.selectOption('#nameMode', 'orig')
  await page.click('#sendSel')
  await page.waitForFunction(() => window.__sent.filter((m) => m.type === 'cc-send-items').length === 3)
  sent = await page.evaluate(() => window.__sent.filter((m) => m.type === 'cc-send-items').pop().items)
  ok(sent.every((x) => !('name' in x)), '保持原名时不带名字')

  ok(errors.length === 0, '没有页面错误' + (errors.length ? '：' + errors.join(';') : ''))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
