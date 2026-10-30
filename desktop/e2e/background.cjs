// 换背景：主轨片段的背景面板、自动识别绿幕、纯色 / 模糊 / 图片三种新背景。
const { launch, openHarness, m, addMenu } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

;(async () => {
  const { srv, browser, page, errors } = await launch()
  await openHarness(page, errors)
  const settle = (ms = 150) => page.waitForTimeout(ms)
  const proj = async () => {
    await settle(1000)
    return page.evaluate(() => JSON.parse(localStorage.getItem('clearclip.edit.draft') || 'null')?.project)
  }
  await page.evaluate((a) => (window.__dialogOpen = [a]), [m('green.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await settle(300)
  await page.locator('.blk.clip').first().click()
  await settle(200)

  console.log('# 换背景')
  ok('开之前没有背景色控件', (await page.locator('.insp .f .n:text-is("背景色")').count()) === 0)
  await page.locator('.el-checkbox', { hasText: '换掉背景' }).click()
  await settle(300)
  ok('出现背景色和新背景', (await page.locator('.insp .f .n:text-is("背景色")').count()) === 1 && (await page.locator('.insp .f .n:text-is("新背景")').count()) === 1)
  let p = await proj()
  ok('bg.on = true，默认绿幕 + 纯色', p.clips[0].bg.on === true && p.clips[0].bg.matte === 'chroma' && p.clips[0].bg.mode === 'color', JSON.stringify(p.clips[0].bg))

  console.log('# 自动识别')
  await page.evaluate(() => (window.__keyGuess = { color: '#12B34A', similarity: 0.31, blend: 0.12, coverage: 0.4 }))
  await page.locator('.insp').getByRole('button', { name: '自动识别' }).click()
  await settle(400)
  p = await proj()
  const kr = await page.evaluate(() => window.__keyReqs[0])
  ok('请求带着素材路径', kr && typeof kr.path === 'string' && kr.path.includes('green.webm'), JSON.stringify(kr))
  ok('识别结果填进了背景色和相似度', p.clips[0].bg.chroma.color === '#12B34A' && Math.abs(p.clips[0].bg.chroma.similarity - 0.31) < 1e-6, JSON.stringify(p.clips[0].bg.chroma))

  console.log('# 新背景')
  await page.locator('.insp .el-radio-button', { hasText: '模糊原画面' }).click()
  await settle(300)
  p = await proj()
  ok('切到模糊', p.clips[0].bg.mode === 'blur')
  ok('出现模糊强度', (await page.locator('.insp .f .n:text-is("模糊强度")').count()) === 1)
  await page.locator('.insp .el-radio-button', { hasText: '人像抠图' }).click()
  await settle(300)
  p = await proj()
  ok('前景切到人像抠图', p.clips[0].bg.matte === 'ai')
  ok('人像抠图时没有绿幕控件', (await page.locator('.insp .f .n:text-is("背景色")').count()) === 0)
  await page.keyboard.press('Control+z')
  await settle(200)
  p = await proj()
  ok('可以撤销', p.clips[0].bg.matte === 'chroma')
  ok('没有控制台错误', errors.filter((e) => !e.includes('favicon')).length === 0, errors.join('\n'))

  console.log(`\n通过 ${pass}，失败 ${fail}`)
  await browser.close()
  srv.close()
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
