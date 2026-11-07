// PDF 版面画布：原文 / 译文 / 对比 / 导出效果切换、Ctrl+滚轮缩放、拖动平移、直接改译文。假后端按命令名回答。
const { chromium } = require('playwright')
const PORT = 5199
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const INIT = `
(() => {
  window.__calls = []
  const svg = encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1400" height="1812"><rect width="100%" height="100%" fill="#fff"/><rect x="100" y="140" width="1200" height="120" fill="#1d3b6b"/><text x="120" y="220" font-size="60" fill="#fff">Attention Is All You Need</text><text x="120" y="420" font-size="36">An attention function maps a query</text></svg>')
  const layout = () => ({
    pages: 3, viewPage: 0, w: 612, h: 792, image: 'data:image/svg+xml;charset=utf-8,' + svg,
    blocks: [
      { index: 0, rect: [52, 700, 560, 745], size: 26, rgb: [255, 255, 255], bg: [29, 59, 107], src: 'Attention Is All You Need', trans: '注意力就是你所需要的一切' },
      { index: 1, rect: [52, 560, 560, 600], size: 11, rgb: [0, 0, 0], bg: [255, 255, 255], src: 'An attention function maps a query', trans: '注意力函数把一个查询和一组键值对映射到一个输出，其中查询、键、值和输出都是向量。输出是值的加权和。' },
    ],
  })
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback() { return 1 }, unregisterCallback() {}, convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'filetr_page_layout': return layout()
        case 'filetr_page_preview': return { orig: layout().image, trans: layout().image, pages: 3, items: 2 }
        case 'filetr_edit': return null
        case 'filetr_retranslate': return '重译结果'
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const page = await browser.newPage({ viewport: { width: 1200, height: 760 } })
  await page.addInitScript(INIT)
  const errs = []
  page.on('pageerror', (e) => errs.push(String(e)))
  await page.goto(`http://127.0.0.1:${PORT}/pdfcanvas-harness.html`)
  await page.waitForSelector('.pdfcv .stage img')
  await page.waitForTimeout(300)

  const cnt = (sel) => page.locator(sel).count()
  ok('默认看译文：一个画布、两块译文', (await cnt('.pdfcv .pane')) === 1 && (await cnt('.pdfcv .blk')) === 2)
  const b0 = await page.locator('.blk').first().evaluate((el) => ({ bg: getComputedStyle(el).backgroundColor, color: getComputedStyle(el).color, left: el.style.left, top: el.style.top }))
  ok('译文块盖着原文处的背景色，字用原文颜色', b0.bg === 'rgb(29, 59, 107)' && b0.color === 'rgb(255, 255, 255)', JSON.stringify(b0))
  ok('块放在原来的位置（按页面比例）', b0.left.startsWith('8.4') && b0.top.startsWith('5.9'), JSON.stringify(b0))
  const fits = await page.locator('.blk').evaluateAll((els) => els.map((e) => e.scrollHeight <= e.clientHeight * 1.1 + 1))
  ok('长译文自动缩小字号放进框里', fits.every(Boolean), JSON.stringify(fits))

  await page.getByText('原文', { exact: true }).first().click()
  await page.waitForTimeout(150)
  ok('切到原文：只剩原页图，没有译文块', (await cnt('.pdfcv .blk')) === 0 && (await cnt('.pdfcv .pane')) === 1)
  await page.getByText('对比', { exact: true }).click()
  await page.waitForTimeout(300)
  ok('对比：左右两个画布，只有右边有译文块', (await cnt('.pdfcv .pane')) === 2 && (await cnt('.pdfcv .blk')) === 2)

  // 缩放：Ctrl+滚轮
  const pane = page.locator('.pdfcv .pane').first()
  const box = await pane.boundingBox()
  await page.mouse.move(box.x + 200, box.y + 200)
  await page.keyboard.down('Control')
  await page.mouse.wheel(0, -400)
  await page.keyboard.up('Control')
  await page.waitForTimeout(150)
  const zoom = await page.locator('.zt').innerText()
  ok('Ctrl+滚轮放大：缩放比例超过 100%', parseInt(zoom) > 100, zoom)
  const t1 = await page.locator('.stage').first().evaluate((e) => e.style.transform)
  // 平移：拖动
  await page.mouse.move(box.x + 100, box.y + 300)
  await page.mouse.down()
  await page.mouse.move(box.x + 160, box.y + 350, { steps: 4 })
  await page.mouse.up()
  const t2 = await page.locator('.stage').first().evaluate((e) => e.style.transform)
  ok('拖动平移：变换改变', t1 !== t2, t1 + ' → ' + t2)
  const same = await page.locator('.stage').evaluateAll((es) => new Set(es.map((e) => e.style.transform)).size === 1)
  ok('对比时两边同步缩放平移', same)
  await page.getByText('适应宽度').click()
  ok('适应宽度：回到 100%', (await page.locator('.zt').innerText()) === '100%')
  await page.getByText('＋', { exact: true }).click()
  ok('＋ 放大', parseInt(await page.locator('.zt').innerText()) > 100)

  // 编辑译文
  await page.getByText('译文', { exact: true }).first().click()
  await page.waitForTimeout(200)
  const blk = page.locator('.blk').first()
  await blk.click()
  await blk.evaluate((el) => { el.innerText = '注意力即一切'; })
  await page.mouse.click(5, 5)
  await page.waitForTimeout(200)
  const edit = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'filetr_edit').pop())
  ok('改完点别处：保存到后端（视图页 0、第 0 块）', edit && edit.args.page === 0 && edit.args.index === 0 && edit.args.text === '注意力即一切', JSON.stringify(edit))
  await blk.dblclick()
  await page.waitForTimeout(200)
  ok('双击一块：重译', await page.evaluate(() => window.__calls.some((c) => c.cmd === 'filetr_retranslate' && c.args.index === 0)))

  // 导出效果
  await page.getByText('导出效果').click()
  await page.waitForTimeout(300)
  ok('导出效果：左原页右导出图', (await cnt('.pdfcv figure')) === 2 && (await page.evaluate(() => window.__calls.some((c) => c.cmd === 'filetr_page_preview'))))
  // 翻页
  await page.getByText('下一页').click()
  await page.waitForTimeout(200)
  ok('下一页：按第 1 页请求', await page.evaluate(() => window.__calls.some((c) => c.cmd === 'filetr_page_preview' && c.args.page === 1)))
  ok('没有脚本错误', errs.length === 0, errs.join('; '))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
