// 剪辑：Shift 多选和编组（Ctrl+G）：多选、编组/拆组、拖动时组里的自由项一起走、同组一起选中、删除时一起删、撤销。
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

  await dialogOpen([m('red.webm'), m('green.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 2)
  await dialogOpen([m('blue.webm')])
  await addMenu(page, '添加叠加素材…')
  await page.waitForSelector('.blk.ovl')
  await dialogOpen([m('music.ogg')])
  await addMenu(page, '添加配乐…')
  await page.waitForSelector('.blk.aud')
  await settle(300)

  console.log('# 多选')
  await page.locator('.blk.ovl').first().click()
  await page.locator('.blk.aud').first().click({ modifiers: ['Shift'] })
  await settle(100)
  ok('Shift+点击把配乐加进选择：两个块都高亮', (await page.locator('.blk.sel').count()) === 2)
  await page.locator('.blk.aud').first().click({ modifiers: ['Shift'] })
  await settle(100)
  ok('再 Shift+点击一次取消', (await page.locator('.blk.sel').count()) === 1)
  await page.locator('.blk.aud').first().click({ modifiers: ['Shift'] })

  console.log('# 编组')
  await page.keyboard.press('Control+g')
  await settle(200)
  const p1 = await proj()
  ok('叠加素材和配乐有了同一个编组编号', p1.overlays[0].group !== undefined && p1.overlays[0].group === p1.audio[0].group, JSON.stringify([p1.overlays[0].group, p1.audio[0].group]))
  ok('主轨片段没有编组', p1.clips.every((c) => c.group === undefined))
  ok('组里的块有分组标记', (await page.locator('.blk.grouped').count()) === 2)
  await page.locator('.blk.clip').first().click()
  await settle(100)
  ok('点别的片段只选它', (await page.locator('.blk.sel').count()) === 1)
  await page.locator('.blk.aud').first().click()
  await settle(100)
  ok('点组里的一个，同组的一起选中', (await page.locator('.blk.sel').count()) === 2)

  console.log('# 拖动')
  const before = { o: p1.overlays[0].startMs, a: p1.audio[0].startMs }
  const box = await page.locator('.blk.aud').first().boundingBox()
  // 点块的上沿：中间有音量线，点上去是加音量点
  await page.mouse.move(box.x + box.width / 2, box.y + 2)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width / 2 + 60, box.y + 2, { steps: 6 })
  await page.mouse.up()
  await settle(300)
  const p2 = await proj()
  const da = p2.audio[0].startMs - before.a
  const dov = p2.overlays[0].startMs - before.o
  ok('拖动配乐：它向右移了', da > 0, String(da))
  ok('组里的叠加素材平移了同样的时间', dov === da, `${dov} vs ${da}`)

  console.log('# 撤销')
  // 拖动结束时落轨会多记一步，所以撤销最多按两次；关键是两个一起回去，不会只回一个
  let p3 = p2
  for (let i = 0; i < 3 && (p3.audio[0].startMs !== before.a || p3.overlays[0].startMs !== before.o); i++) {
    await page.keyboard.press('Control+z')
    await settle(200)
    p3 = await proj()
    ok(`撤销第 ${i + 1} 次：配乐和叠加素材的位移始终一致`, p3.audio[0].startMs - before.a === p3.overlays[0].startMs - before.o, JSON.stringify([p3.audio[0].startMs, p3.overlays[0].startMs, before]))
  }
  ok('撤销后两个都回到原位', p3.audio[0].startMs === before.a && p3.overlays[0].startMs === before.o)

  console.log('# 拆组')
  await page.locator('.blk.aud').first().click()
  await page.keyboard.press('Control+Shift+g')
  await settle(200)
  const p4 = await proj()
  ok('拆组后编组编号都去掉了', p4.overlays[0].group === undefined && p4.audio[0].group === undefined)
  await page.keyboard.press('Control+z')
  await settle(200)

  console.log('# 删除')
  await page.locator('.blk.aud').first().click()
  await settle(100)
  await page.keyboard.press('Delete')
  await settle(300)
  const p5 = await proj()
  ok('删除组里的一个：整组一起删', p5.audio.length === 0 && p5.overlays.length === 0 && p5.clips.length === 2, JSON.stringify([p5.audio.length, p5.overlays.length, p5.clips.length]))

  console.log('# 没有脚本错误')
  const real = errors.filter((e) => !/favicon/.test(e))
  ok('页面没有报错', real.length === 0, real.join('; '))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  srv.kill?.()
  process.exit(fail ? 1 : 0)
})()
