// 导入字幕文件：每条字幕变成时间线上的一条文字，可撤回。
const { launch, openHarness, m } = require('./lib.cjs')

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
  const texts = () => page.locator('.blk.txt').count()
  const menu = async (name) => {
    await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-dropdown__caret-button').click()
    await page.getByRole('menuitem', { name }).click()
  }

  console.log('# 导入字幕')
  await page.evaluate((f) => (window.__dialogOpen = [f]), [m('red.webm')])
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await page.waitForTimeout(300)

  await page.evaluate(() => {
    window.__cues = [
      { startMs: 0, endMs: 800, text: '第一句' },
      { startMs: 1000, endMs: 1800, text: '第二句\n两行' },
    ]
    window.__dialogOpen = ['/tmp/a.srt']
  })
  await menu('导入字幕文件')
  await page.waitForFunction(() => document.querySelectorAll('.blk.txt').length === 2, null, { timeout: 5000 }).catch(() => {})
  ok('两条字幕变成两条文字', (await texts()) === 2, String(await texts()))
  await page.keyboard.press('Control+z')
  await page.waitForFunction(() => document.querySelectorAll('.blk.txt').length === 0, null, { timeout: 5000 }).catch(() => {})
  ok('撤回一次全部去掉', (await texts()) === 0, String(await texts()))

  const bad = errors.filter((e) => !/favicon|net::ERR|Failed to load resource/.test(e))
  ok('没有控制台错误', bad.length === 0, bad.slice(0, 3).join(' | '))
  console.log(`\n通过 ${pass}，失败 ${fail}`)
  await browser.close()
  srv.close()
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(2)
})
