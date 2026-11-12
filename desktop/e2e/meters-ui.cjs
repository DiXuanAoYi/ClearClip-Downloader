// 剪辑：混音器里的电平表（实验，默认关）。开关默认关；打开后在播放时每条音频轨和主输出的电平条会动；停下后回到底。
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
  // 开关记在 localStorage，在页面脚本跑之前就设好（这个功能要“重新打开剪辑页才生效”）
  await page.addInitScript(() => {
    if (!sessionStorage.getItem('meters-test-init')) {
      localStorage.removeItem('clearclip.edit.meters')
      sessionStorage.setItem('meters-test-init', '1')
    }
  })
  await openHarness(page, errors)
  const settle = (ms = 150) => page.waitForTimeout(ms)
  const dialogOpen = (v) => page.evaluate((a) => (window.__dialogOpen = [a]), v)

  await dialogOpen([m('red.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForSelector('.blk.clip')
  await dialogOpen([m('music.ogg')])
  await addMenu(page, '添加配乐…')
  await page.waitForSelector('.blk.aud')
  await page.getByRole('button', { name: '混音器', exact: true }).click()
  await settle(200)

  console.log('# 默认关')
  ok('开关默认关着，没有电平条', (await page.locator('.mixer .meter').count()) === 0)
  ok('开关就在混音器里', (await page.getByText('电平表（实验').count()) === 1)

  console.log('# 打开后重新载入')
  await page.getByText('电平表（实验').click()
  await settle(200)
  ok('开关记在本机', (await page.evaluate(() => localStorage.getItem('clearclip.edit.meters'))) === '1')
  await page.reload()
  await page.waitForSelector('.bar')
  await dialogOpen([m('red.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForSelector('.blk.clip')
  await dialogOpen([m('music.ogg')])
  await addMenu(page, '添加配乐…')
  await page.waitForSelector('.blk.aud')
  await page.getByRole('button', { name: '混音器', exact: true }).click()
  await settle(300)
  ok('打开后每条音频轨一个电平条，外加主输出', (await page.locator('.mixer .meter').count()) === 2, String(await page.locator('.mixer .meter').count()))
  ok('没播放时电平条是空的', (await page.locator('.mixer .meter[role=meter]').first().getAttribute('aria-valuenow')) === '-90')

  console.log('# 播放')
  await page.evaluate(() => document.activeElement?.blur())
  await page.keyboard.press('Space')
  await page.waitForFunction(() => [...document.querySelectorAll('.mixer .meter[role=meter]')].some((e) => Number(e.getAttribute('aria-valuenow')) > -80), null, { timeout: 8000 }).catch(() => {})
  const vals = await page.locator('.mixer .meter[role=meter]').evaluateAll((els) => els.map((e) => Number(e.getAttribute('aria-valuenow'))))
  ok('播放时配乐轨的电平条动了（高于 −80 dBFS）', vals.some((v) => v > -80), JSON.stringify(vals))
  ok('电平不超过 0 dBFS', vals.every((v) => v <= 0.5), JSON.stringify(vals))
  await page.keyboard.press('Space')
  await page.waitForFunction(() => [...document.querySelectorAll('.mixer .meter[role=meter]')].every((e) => Number(e.getAttribute('aria-valuenow')) === -90), null, { timeout: 4000 }).catch(() => {})
  ok('停下后电平条回到底', (await page.locator('.mixer .meter[role=meter]').evaluateAll((els) => els.every((e) => Number(e.getAttribute('aria-valuenow')) === -90))))
  ok('开关还开着（没有被误关）', (await page.evaluate(() => localStorage.getItem('clearclip.edit.meters'))) === '1')

  const real = errors.filter((e) => !/favicon|net::ERR|Failed to load resource|404/.test(e))
  ok('页面没有报错', real.length === 0, real.slice(0, 3).join(' | '))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  srv.kill?.()
  process.exit(fail ? 1 : 0)
})()
