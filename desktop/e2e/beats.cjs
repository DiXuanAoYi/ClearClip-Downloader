// 抠像（绿幕）面板、节拍分析、让主轨按节拍切换、配乐适配。
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

  console.log('# 抠像')
  await dialogOpen([m('red.webm'), m('green.webm'), m('blue.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 3)
  await dialogOpen([m('sticker.png')])
  await addMenu(page, '添加叠加素材…')
  await page.waitForSelector('.blk.ovl')
  await settle(300)
  await click('.blk.ovl')
  ok('开抠像前没有颜色等控件', (await page.locator('.insp .f .n:text-is("相似度")').count()) === 0)
  await page.locator('.el-checkbox', { hasText: '去掉背景色' }).click()
  await settle(300)
  ok('出现颜色、相似度、边缘柔化', (await page.locator('.insp .f .n:text-is("相似度")').count()) === 1 && (await page.locator('.insp .f .n:text-is("边缘柔化")').count()) === 1)
  let p = await proj()
  ok('工程里 chroma.on = true，默认绿色', p.overlays[0].chroma.on === true && p.overlays[0].chroma.color === '#00FF00', JSON.stringify(p.overlays[0].chroma))
  ok('预览里生成了 SVG 滤镜', (await page.locator('svg.keydefs filter').count()) === 1)
  await page.getByRole('button', { name: '蓝幕' }).click()
  await settle(300)
  p = await proj()
  ok('点“蓝幕”换成蓝色', p.overlays[0].chroma.color === '#0000FF', p.overlays[0].chroma.color)
  await page.locator('.el-checkbox', { hasText: '去掉背景色' }).click()
  await settle(300)
  p = await proj()
  ok('再点一次关掉，颜色保留', p.overlays[0].chroma.on === false && p.overlays[0].chroma.color === '#0000FF')
  ok('关掉后滤镜没了', (await page.locator('svg.keydefs filter').count()) === 0)

  console.log('# 节拍')
  const beats = Array.from({ length: 30 }, (_, i) => 300 + i * 500)
  await page.evaluate((b) => (window.__beats = { bpm: 120, confidence: 0.6, beatsMs: b }), beats)
  await dialogOpen([m('music.ogg')])
  await addMenu(page, '添加配乐…')
  await page.waitForSelector('.blk.aud')
  await settle(300)
  await click('.blk.aud')
  await page.getByRole('button', { name: '分析节拍' }).click()
  await settle(500)
  p = await proj()
  ok('分析结果存进了配乐', p.audio[0].beats && p.audio[0].beats.beatsMs.length === 30 && p.audio[0].beats.bpm === 120)
  ok('请求的是整条素材', (await page.evaluate(() => window.__beatReqs[0].fromMs)) === 0)
  ok('配乐条上画出了节拍线', (await page.locator('.blk.aud .bt').count()) > 3)

  const durs = async () => (await proj()).clips.map((c) => c.outMs - c.inMs)
  const before = await durs()
  await page.locator('.insp .f', { hasText: '卡点' }).getByRole('button', { name: '让主轨按节拍切换' }).click()
  await settle(500)
  p = await proj()
  const after = p.clips.map((c) => (c.outMs - c.inMs) / c.speed)
  const at = p.audio[0].startMs // 配乐加在播放头处，第一个切点在它的第 2 个拍子上
  ok('三个片段的长度对齐到节拍', Math.abs(after[0] - (at + 800)) < 2 && Math.abs(after[1] - 1000) < 2 && Math.abs(after[2] - 1000) < 2, after.join() + ' ← 之前 ' + before.join())
  await page.keyboard.press('Control+z')
  await settle(300)
  p = await proj()
  ok('撤销回到原来的长度', p.clips.map((c) => c.outMs - c.inMs).join() === before.join())

  await page.getByRole('button', { name: '适配成片长度' }).click()
  await settle(300)
  p = await proj()
  ok('适配后有结尾淡出', p.audio[0].fadeOutMs > 0, JSON.stringify(p.audio[0]))

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
