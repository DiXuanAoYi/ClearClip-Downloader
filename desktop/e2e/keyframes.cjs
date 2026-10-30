// 关键帧动画、变速曲线、定格、倒放：界面操作 → 工程里的数据。
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
  const dialogOpen = (v) => page.evaluate((a) => (window.__dialogOpen = [a]), v)
  const proj = async () => {
    await page.waitForTimeout(1000)
    return page.evaluate(() => JSON.parse(localStorage.getItem('clearclip.edit.draft') || 'null')?.project)
  }
  const click = async (sel) => {
    const b = await page.locator(sel).first().boundingBox()
    await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2)
    await settle(150)
  }

  console.log('# 准备')
  await dialogOpen([m('red.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await dialogOpen([m('sticker.png')])
  await addMenu(page, '添加叠加素材…')
  await page.waitForSelector('.blk.ovl')
  await settle(300)

  console.log('# 叠加素材关键帧')
  await click('.blk.ovl')
  const panel = page.locator('.insp .kfc')
  ok('检查器里有“关键帧动画”', (await panel.count()) === 1)
  await panel.locator('.el-collapse-item__header').click()
  await settle(200)
  const diamonds = page.locator('.kfp .row .head .el-button').filter({ hasText: '◇' })
  ok('每个属性一个菱形', (await diamonds.count()) >= 5, String(await diamonds.count()))
  await diamonds.first().click()
  await settle(200)
  let p = await proj()
  ok('点菱形：在播放头处加了一个关键点', Object.values(p.overlays[0].keyframes).flat().length === 1, JSON.stringify(p.overlays[0].keyframes))
  const prop = Object.keys(p.overlays[0].keyframes)[0]
  ok('时间线上的素材条出现菱形标记', (await page.locator('.blk.ovl .kfm').count()) >= 1)
  await page.locator('.kfp .row .head .el-button').filter({ hasText: '◆' }).first().click()
  await settle(200)
  p = await proj()
  ok('再点一次删掉它', Object.keys(p.overlays[0].keyframes).length === 0, JSON.stringify(p.overlays[0].keyframes))
  ok(`属性名是已知的（${prop}）`, ['x', 'y', 'scale', 'rotate', 'opacity', 'volume'].includes(prop))

  console.log('# 变速曲线 / 定格 / 倒放')
  await click('.blk.clip')
  await page.getByRole('button', { name: '编辑曲线…' }).click()
  await settle(200)
  await page.locator('.sc .presets').getByRole('button', { name: '渐快' }).click()
  await page.getByRole('button', { name: '应用曲线' }).click()
  await settle(300)
  p = await proj()
  ok('平滑变速曲线把片段拆成许多小段', p.clips.length >= 16, String(p.clips.length))
  const sp = p.clips.map((c) => c.speed)
  ok('速度由慢到快、相邻两段只差一点点', sp[0] < 0.7 && sp[sp.length - 1] > 2 && sp.every((v, i) => i === 0 || (v >= sp[i - 1] - 1e-9 && v / sp[i - 1] < 1.25)), sp.join())
  ok('素材范围首尾相接', p.clips.every((c, i) => i === 0 || c.inMs === p.clips[i - 1].outMs))
  await page.keyboard.press('Control+z')
  await settle(300)
  p = await proj()
  ok('撤销一次回到 1 段', p.clips.length === 1, String(p.clips.length))

  await click('.blk.clip')
  await page.getByRole('button', { name: '在播放头处定格' }).click()
  await settle(300)
  p = await proj()
  ok('定格：多出一段只有一帧的定格片段', p.clips.length >= 2 && p.clips.filter((c) => c.freeze).length === 1, JSON.stringify(p.clips.map((c) => c.freeze)))
  await page.keyboard.press('Control+z')
  await settle(300)

  await click('.blk.clip')
  await page.locator('.el-checkbox', { hasText: '倒放' }).click()
  await settle(300)
  p = await proj()
  ok('倒放勾选后写进工程', p.clips[0].reverse === true)
  ok('倒放后变速曲线按钮不可用', await page.getByRole('button', { name: '编辑曲线…' }).isDisabled())
  ok('片段名带“倒放”标记', ((await page.locator('.blk.clip .cap').first().textContent()) || '').includes('倒放'))

  const bad = errors.filter((e) => !/favicon|net::ERR|Failed to load resource|404/.test(e))
  ok('没有控制台错误', bad.length === 0, bad.join(' | '))
  console.log(`\n通过 ${pass}，失败 ${fail}`)
  await browser.close()
  srv.close()
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
