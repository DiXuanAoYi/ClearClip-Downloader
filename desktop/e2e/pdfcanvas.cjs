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
      { index: 0, rect: [52, 700, 560, 745], size: 26, rgb: [255, 255, 255], bg: [29, 59, 107], origRgb: [255, 255, 255], origSize: 26, origBg: [29, 59, 107], bgMode: 'solid', bgStops: [], align: 'left', pad: null, styled: false, adjusted: false, skipped: false, src: 'Attention Is All You Need', trans: '注意力就是你所需要的一切' },
      { index: 1, rect: [52, 560, 560, 600], size: 11, rgb: [0, 0, 0], bg: [255, 255, 255], origRgb: [0, 0, 0], origSize: 11, origBg: [255, 255, 255], bgMode: 'solid', bgStops: [], align: 'left', pad: null, styled: false, adjusted: false, skipped: false, src: 'An attention function maps a query', trans: '注意力函数把一个查询和一组键值对映射到一个输出，其中查询、键、值和输出都是向量。输出是值的加权和。' },
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
        case 'filetr_block_adjust': return null
        case 'filetr_block_style': return null
        case 'filetr_replace_color': return args.dryRun ? 7 : 7
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
  const b0 = await page.locator('.blk').first().evaluate((el) => ({ bg: getComputedStyle(el).backgroundColor, color: getComputedStyle(el).color, left: parseFloat(el.style.left), top: parseFloat(el.style.top), w: parseFloat(el.style.width) }))
  ok('译文块盖着原文处的背景色，字用原文颜色', b0.bg === 'rgb(29, 59, 107)' && b0.color === 'rgb(255, 255, 255)', JSON.stringify(b0))
  // 遮盖比文字框外扩：字号 26 → 外扩 4.68 点；左边 52 → (52-4.68)/612 = 7.73%，宽 (508+9.36)/612 = 84.5%
  ok('遮盖比原文框外扩（不露出原文边缘）', Math.abs(b0.left - 7.73) < 0.1 && Math.abs(b0.w - 84.54) < 0.2, JSON.stringify(b0))
  const fits = await page.locator('.blk').evaluateAll((els) => els.map((e) => e.scrollHeight <= e.clientHeight + 1))
  ok('长译文不被裁切：框往下长，字号不变', fits.every(Boolean), JSON.stringify(fits))
  const fsz = await page.locator('.blk').nth(1).evaluate((e) => ({ fs: parseFloat(getComputedStyle(e).fontSize), want: Number(e.dataset.fs) }))
  ok('字号保持原字号（没有被缩小）', fsz.fs >= fsz.want * 0.85 - 0.01, JSON.stringify(fsz))

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

  // 选中一块：检查器（字号、颜色、背景、对齐）、拖动移动、拖右下角改大小、还原
  await page.getByText('适应宽度').click()
  await page.waitForTimeout(150)
  const adj = () => page.evaluate(() => window.__calls.filter((c) => c.cmd === 'filetr_block_style'))
  const lastPatch = async () => (await adj()).pop()
  await page.locator('.blk').nth(1).click()
  await page.waitForTimeout(150)
  ok('点一块：右边出现检查器和移动 / 改大小的手柄', (await cnt('.insp .swb')) >= 2 && (await cnt('.sel .grip')) === 1 && (await cnt('.sel .rsz')) === 1)
  ok('颜色和原文一致时标注“和原文一致”', (await page.locator('.insp').innerText()).includes('和原文一致'))
  await page.getByLabel('字号增大').click()
  await page.waitForTimeout(150)
  let c = await lastPatch()
  ok('字号 ＋：存 11.5 磅', c && c.args.edits.length === 1 && c.args.edits[0].index === 1 && c.args.edits[0].patch.size === 11.5, JSON.stringify(c))
  ok('字号改后显示“原 11.0 → 现 11.5”', (await page.locator('.insp').innerText()).includes('原 11.0 → 现 11.5'))
  const g = await page.locator('.sel .grip').boundingBox()
  await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2)
  await page.mouse.down()
  await page.mouse.move(g.x + g.width / 2 + 40, g.y + g.height / 2 + 30, { steps: 4 })
  await page.mouse.up()
  await page.waitForTimeout(200)
  c = await lastPatch()
  const mv = c.args.edits[0].patch.rect
  ok('拖 ✥：整块右移、下移（PDF 点，y 轴朝上所以下移 = 数值变小）', mv[0] > 52 && mv[1] < 560 && Math.abs(mv[2] - mv[0] - 508) < 0.5 && Math.abs(mv[3] - mv[1] - 40) < 0.5, JSON.stringify(mv))
  const rz = await page.locator('.sel .rsz').boundingBox()
  await page.mouse.move(rz.x + rz.width / 2, rz.y + rz.height / 2)
  await page.mouse.down()
  await page.mouse.move(rz.x + rz.width / 2 + 30, rz.y + rz.height / 2 + 20, { steps: 4 })
  await page.mouse.up()
  await page.waitForTimeout(200)
  c = await lastPatch()
  const rs = c.args.edits[0].patch.rect
  ok('拖右下角：变宽变高，左上角不动', rs[2] - rs[0] > 508 && rs[3] - rs[1] > 40 && Math.abs(rs[0] - mv[0]) < 0.01 && Math.abs(rs[3] - mv[3]) < 0.01, JSON.stringify([mv, rs]))

  // 对齐、背景模式
  await page.locator('.insp .seg span', { hasText: '居中' }).click()
  await page.waitForTimeout(100)
  c = await lastPatch()
  ok('居中：存 align=center，界面文字居中', c.args.edits[0].patch.align === 'center' && (await page.locator('.blk').nth(1).evaluate((e) => getComputedStyle(e).textAlign)) === 'center')
  await page.locator('.insp .seg span', { hasText: '横渐变' }).click()
  await page.waitForTimeout(100)
  c = await lastPatch()
  ok('横渐变：存 bgMode、至少两个色标，背景变成渐变', c.args.edits[0].patch.bgMode === 'hgrad' && c.args.edits[0].patch.bgStops.length >= 2 && (await page.locator('.blk').nth(1).evaluate((e) => e.style.background)).includes('linear-gradient'))
  await page.locator('.insp .seg span', { hasText: '纯色' }).click()
  await page.waitForTimeout(100)

  // 取色器：改文字色
  await page.locator('.insp .swb').first().click()
  await page.waitForSelector('.ccp')
  await page.locator('.ccp .f2 input').first().fill('#336699')
  await page.keyboard.press('Enter')
  await page.getByRole('button', { name: /^应用/ }).click()
  await page.waitForTimeout(200)
  c = await lastPatch()
  ok('取色器输入 #336699 并应用：文字色存 [51,102,153]', JSON.stringify(c.args.edits[0].patch.fg) === '[51,102,153]', JSON.stringify(c))
  ok('界面文字色立刻变了', (await page.locator('.blk').nth(1).evaluate((e) => getComputedStyle(e).color)) === 'rgb(51, 102, 153)')
  await page.getByRole('button', { name: '还原', exact: true }).first().click()
  await page.waitForTimeout(150)
  c = await lastPatch()
  ok('还原文字色：发 clear fg', JSON.stringify(c.args.edits[0].patch.clear) === '["fg"]')

  // 多选：Ctrl 点第二块，批量改
  await page.locator('.blk').nth(0).click({ modifiers: ['Control'] })
  await page.waitForTimeout(150)
  ok('Ctrl 点第二块：已选 2 块，不进入编辑', (await page.locator('.insp .ih b').innerText()).includes('已选 2 块'))
  await page.getByLabel('字号增大').click()
  await page.waitForTimeout(150)
  c = await lastPatch()
  ok('多选字号 ＋：一次请求带两块，各自加 0.5', c.args.edits.length === 2 && c.args.edits.some((e) => e.index === 0 && e.patch.size === 26.5) && c.args.edits.some((e) => e.index === 1 && e.patch.size === 12), JSON.stringify(c))
  await page.locator('.insp .seg span', { hasText: '靠右' }).click()
  await page.waitForTimeout(100)
  c = await lastPatch()
  ok('多选对齐：两块都存 right', c.args.edits.length === 2 && c.args.edits.every((e) => e.patch.align === 'right'))
  await page.getByRole('button', { name: '全选本页' }).click()
  ok('全选本页', (await page.locator('.insp .ih b').innerText()).includes('已选 2 块'))

  // 替换颜色
  await page.getByText('替换颜色（本页 / 整份文件）').click()
  await page.getByRole('button', { name: '查看有几处' }).click()
  await page.waitForTimeout(150)
  const dry = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'filetr_replace_color').pop())
  ok('查看有几处：dryRun、本页、给出数量', dry && dry.args.dryRun === true && dry.args.page === 0 && (await page.locator('.insp').innerText()).includes('7 处'), JSON.stringify(dry))
  await page.locator('.insp .seg span', { hasText: '整份文件' }).click()
  await page.getByRole('button', { name: '替换…' }).click()
  await page.waitForTimeout(200)
  await page.getByRole('button', { name: '替换 7 处' }).click()
  await page.waitForTimeout(250)
  const real = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'filetr_replace_color').pop())
  ok('整份文件替换：先显示 7 处，确认后 dryRun=false、page=null', real && real.args.dryRun === false && real.args.page === null, JSON.stringify(real))

  // 还原
  await page.locator('.blk').nth(1).click()
  await page.getByLabel('字号增大').click()
  await page.getByRole('button', { name: '还原位置、大小和样式' }).click()
  await page.waitForTimeout(250)
  c = await lastPatch()
  ok('还原位置、大小和样式：clear 全部项', c.args.edits[0].patch.clear.length === 8)

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
