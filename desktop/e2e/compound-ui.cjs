// 剪辑：复合片段（把连着的主轨片段合成一个、双击进去继续剪、返回、草稿里存的是放回去以后的整个工程）。
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

  await dialogOpen([m('red.webm'), m('green.webm'), m('blue.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 3)
  await settle(300)

  console.log('# 合成')
  await page.locator('.blk.clip').nth(0).click()
  await page.locator('.blk.clip').nth(2).click({ modifiers: ['Shift'] })
  await page.getByRole('button', { name: '合成复合片段' }).click()
  await settle(150)
  ok('选的两个片段不连着时不合成', (await page.locator('.blk.cmp').count()) === 0)
  await page.locator('.blk.clip').nth(0).click()
  await page.locator('.blk.clip').nth(1).click({ modifiers: ['Shift'] })
  await settle(100)
  await page.getByRole('button', { name: '合成复合片段' }).click()
  await settle(300)
  ok('主轨上两个片段换成一个复合片段，外加最后的蓝色', (await page.locator('.blk.clip').count()) === 2 && (await page.locator('.blk.cmp').count()) === 1)
  ok('复合片段上显示它的名字', (await page.locator('.blk.cmp').innerText()).includes('复合片段 1'))
  const p1 = await proj()
  ok('工程里有一个复合片段，里面是红和绿两个片段', p1.compounds?.length === 1 && p1.compounds[0].project.clips.length === 2, JSON.stringify(p1.compounds?.map((c) => c.project.clips.length)))
  ok('外面的复合片段没有素材路径、指向这个复合片段', p1.clips[0].compound === p1.compounds[0].id && p1.clips[0].path === '')
  const total = p1.compounds[0].project.clips.reduce((a, c) => a + (c.outMs - c.inMs), 0)
  ok('复合片段的长度是里面的总时长', Math.abs(p1.clips[0].outMs - p1.clips[0].inMs - total) < 50, `${p1.clips[0].outMs} vs ${total}`)

  console.log('# 进去、改、回来')
  await page.locator('.blk.cmp').dblclick()
  await settle(300)
  ok('双击后出现“返回”和层级', (await page.locator('.crumb').count()) === 1 && (await page.locator('.crumb').innerText()).includes('复合片段 1'))
  ok('时间线里看到的是里面的两个片段', (await page.locator('.blk.clip').count()) === 2)
  await page.locator('.blk.clip').nth(1).click()
  await page.keyboard.press('Delete')
  await settle(300)
  ok('在里面删掉一个，只剩一个', (await page.locator('.blk.clip').count()) === 1)
  const inside = await proj()
  ok('里面编辑时草稿存的还是最外层的工程（有复合片段和蓝色片段）', inside.clips.length === 2 && inside.compounds?.length === 1 && inside.compounds[0].project.clips.length === 1, JSON.stringify([inside.clips.length, inside.compounds?.length]))
  await page.locator('.crumb .b').click()
  await settle(300)
  ok('返回后层级条消失、主轨回到两个片段', (await page.locator('.crumb').count()) === 0 && (await page.locator('.blk.clip').count()) === 2)
  const p2 = await proj()
  ok('外面的复合片段跟着变短了', p2.clips[0].outMs - p2.clips[0].inMs < total - 100, `${p2.clips[0].outMs - p2.clips[0].inMs} vs ${total}`)

  console.log('# 删除复合片段')
  await page.locator('.blk.cmp').click()
  await page.keyboard.press('Delete')
  await settle(300)
  const p3 = await proj()
  ok('删掉后工程里不再带着它的子时间线', (p3.compounds ?? []).length === 0 && p3.clips.length === 1, JSON.stringify([p3.compounds?.length, p3.clips.length]))

  console.log('# 没有脚本错误')
  const real = errors.filter((e) => !/favicon|net::ERR|Failed to load resource|404/.test(e))
  ok('页面没有报错', real.length === 0, real.slice(0, 3).join(' | '))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  srv.kill?.()
  process.exit(fail ? 1 : 0)
})()
