// HDR 素材：检查器里提示，一键转成普通画面并替换（可撤回）。
const { launch, openHarness, m } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

;(async () => {
  globalThis.__hdrFiles = [m('red.webm')]
  const { srv, browser, page, errors } = await launch()
  await openHarness(page, errors)
  const names = () => page.$$eval('.blk.clip .cap', (els) => els.map((e) => e.textContent.trim()))
  const job = (o) => page.evaluate((j) => window.__emit('media://jobs', [{ id: 88, title: 'x', op: '视频规整', output: null, error: null, note: null, finishedAt: null, ...j }]), o)

  console.log('# HDR')
  await page.evaluate((f) => (window.__dialogOpen = [f]), [m('red.webm'), m('green.webm')])
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 2)
  await page.waitForTimeout(400)
  const first = page.locator('.blk.clip').first()
  await first.click()
  await page.waitForTimeout(200)
  ok('HDR 素材在检查器里有提示和按钮', (await page.getByRole('button', { name: '转成普通画面并替换' }).count()) === 1)
  await page.locator('.blk.clip').nth(1).click()
  await page.waitForTimeout(200)
  ok('普通素材没有这个提示', (await page.getByRole('button', { name: '转成普通画面并替换' }).count()) === 0)
  await first.click()
  await page.waitForTimeout(200)

  await page.getByRole('button', { name: '转成普通画面并替换' }).click()
  await page.waitForTimeout(300)
  const started = await page.evaluate(() => window.__exported)
  ok('发出的是视频规整任务', started && started.op === 'normalize' && started.inputs.length === 1 && started.inputs[0].endsWith('red.webm'), JSON.stringify(started))
  ok('只转 HDR：尺寸、帧率不动，HDR 转换保持开启', started.spec.size === 'keep' && started.spec.fps === 'keep' && started.spec.hdr === true && started.spec.fixSync === false, JSON.stringify(started.spec))
  await job({ status: 'running', percent: 40 })
  await page.waitForTimeout(200)
  ok('显示转换进度', (await page.locator('.insp').textContent()).includes('正在转换 40%'))
  ok('转换期间按钮不可再点', (await page.getByRole('button', { name: '转成普通画面并替换' }).count()) === 0)
  await job({ status: 'done', percent: 100, output: m('blue.webm'), finishedAt: 1 })
  await page.waitForTimeout(600)
  ok('转完后片段换成转换后的文件', JSON.stringify(await names()) === JSON.stringify(['blue.webm', 'green.webm']), JSON.stringify(await names()))
  ok('换完不再提示 HDR', (await page.locator('.insp .alert').count()) === 0)
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(300)
  ok('撤回一次就换回原素材', JSON.stringify(await names()) === JSON.stringify(['red.webm', 'green.webm']), JSON.stringify(await names()))

  console.log('# 失败')
  await page.locator('.blk.clip').first().click()
  await page.getByRole('button', { name: '转成普通画面并替换' }).click()
  await page.waitForTimeout(300)
  await job({ status: 'failed', percent: 10, error: '转换失败了' })
  await page.waitForTimeout(300)
  ok('失败后按钮恢复，素材不变', (await page.getByRole('button', { name: '转成普通画面并替换' }).count()) === 1 && (await names())[0] === 'red.webm')

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
