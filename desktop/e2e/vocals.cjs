// 人声分离：智能页的模型面板、分离请求（范围 / 模型）、取消、把结果加入音频轨、用伴奏替换原声。
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
  const settle = (ms = 150) => page.waitForTimeout(ms)
  const tab = (name) => page.locator('.insp .seg').getByText(name, { exact: true }).click()
  const proj = async () => {
    await settle(1000)
    return page.evaluate(() => JSON.parse(localStorage.getItem('clearclip.edit.draft') || 'null')?.project)
  }
  await page.evaluate(([f, v]) => {
    window.__dialogOpen = [f]
    window.__vocResult = { vocals: v, instrumental: v, durationMs: 6000 }
  }, [[m('red.webm')], m('music.ogg')])
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await settle(300)
  await page.locator('.blk.clip').first().click()
  await tab('智能')
  await settle(300)

  console.log('# 模型')
  const box = page.locator('.smart [data-model]')
  ok('只列人声模型', (await box.count()) === 1 && (await page.locator('.smart [data-model="vocal_mdx"]').count()) === 1)
  const go = page.getByRole('button', { name: '分离人声和伴奏' })
  ok('没装模型时不能分离', await go.isDisabled())
  await page.locator('.smart [data-model="vocal_mdx"]').getByRole('button', { name: '下载' }).click()
  await settle(400)
  ok('装好后可以分离', !(await go.isDisabled()))

  console.log('# 分离')
  await page.evaluate(() => (window.__hold = true))
  await go.click()
  await settle(300)
  ok('出现进度和取消', (await page.locator('.smart .prog').count()) === 1 && (await page.getByRole('button', { name: '取消' }).count()) >= 1)
  await page.evaluate(() => (window.__hold = false))
  await page.waitForSelector('.smart .stems')
  const req = await page.evaluate(() => window.__vocReqs[0])
  ok('请求带素材、范围、模型', req && req.path.includes('red.webm') && req.fromMs === 0 && req.toMs > 1000 && req.model === 'vocal_mdx', JSON.stringify(req))
  ok('没填 FFT 点数就不传', req.nFft === undefined, JSON.stringify(req))

  console.log('# 取消')
  await page.evaluate(() => (window.__hold = true))
  await go.click()
  await settle(300)
  await page.locator('.smart .row').getByRole('button', { name: '取消' }).click()
  await settle(400)
  await page.evaluate(() => (window.__hold = false))
  ok('取消后回到按钮', (await go.count()) === 1 && (await page.locator('.smart .prog').count()) === 0)
  await page.evaluate(() => (window.__hold = false))
  await go.click()
  await page.waitForSelector('.smart .stems')

  console.log('# 加入音频轨')
  await page.locator('.stems .row', { hasText: '伴奏' }).getByRole('button', { name: '加入音频轨' }).click()
  await settle(500)
  let p = await proj()
  ok('音频轨多了一条', p.audio.length === 1 && p.audio[0].path.includes('music.ogg'), JSON.stringify(p.audio))
  ok('原片段声音没变', p.clips[0].mute === false)
  ok('加完仍停在原片段的智能页', (await page.locator('.smart .stems').count()) === 1)
  await page.getByRole('button', { name: '用伴奏替换原声' }).click()
  await settle(500)
  p = await proj()
  ok('替换：再加一条并把原片段静音', p.audio.length === 2 && p.clips[0].mute === true, JSON.stringify([p.audio.length, p.clips[0].mute]))
  ok('没有控制台错误', errors.filter((e) => !e.includes('favicon')).length === 0, errors.join('\n'))

  await browser.close()
  srv.close()
  console.log(`\n通过 ${pass}，失败 ${fail}`)
  process.exit(fail ? 1 : 0)
})()
