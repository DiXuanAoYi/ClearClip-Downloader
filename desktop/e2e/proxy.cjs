// 系统播放器解不了的素材：实时画面里提示，点按钮后换成预览用的小文件。
const { launch, openHarness, m, urlOf } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

;(async () => {
  globalThis.__badFiles = [m('red.webm')]
  globalThis.__bigFiles = [m('green.webm')]
  const { srv, browser, page, errors } = await launch()
  await openHarness(page, errors)

  console.log('# 预览代理')
  await page.evaluate((u) => (window.__proxyUrl = u), urlOf(m('red.webm')))
  await page.evaluate((f) => (window.__dialogOpen = [f]), [m('red.webm')])
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await page.waitForSelector('.live .warn', { timeout: 8000 })
  ok('播不出来时提示并给出按钮', (await page.getByRole('button', { name: '生成预览用的小文件' }).count()) === 1)
  await page.getByRole('button', { name: '生成预览用的小文件' }).click()
  await page.waitForFunction(() => window.__proxied === 1)
  await page.waitForFunction(() => !document.querySelector('.live .warn'), null, { timeout: 8000 }).catch(() => {})
  ok('生成后提示消失', (await page.locator('.live .warn').count()) === 0)
  ok('实时画面改用小文件播放', (await page.locator('.live video').first().getAttribute('src')) === urlOf(m('red.webm')))

  console.log('# 大素材自动用小文件')
  ok('开关默认关着', !(await page.getByRole('checkbox', { name: '大素材用小文件预览' }).isChecked()))
  await page.getByText('大素材用小文件预览').click()
  await page.evaluate((f) => (window.__dialogOpen = [f]), [m('green.webm')])
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 2)
  await page.waitForFunction(() => window.__proxied === 2, null, { timeout: 8000 }).catch(() => {})
  ok('4K 的素材一加进来就自动生成了小文件（没点任何按钮）', (await page.evaluate(() => window.__proxied)) === 2, String(await page.evaluate(() => window.__proxied)))
  ok('开关记在本机，下次打开还是开着', (await page.evaluate(() => localStorage.getItem('clearclip.edit.proxyBig'))) === '1')

  const bad = errors.filter((e) => !/favicon|net::ERR|Failed to load resource|404/.test(e))
  ok('没有控制台错误', bad.length === 0, bad.slice(0, 3).join(' | '))
  console.log(`\n通过 ${pass}，失败 ${fail}`)
  await browser.close()
  srv.close()
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(2)
})
