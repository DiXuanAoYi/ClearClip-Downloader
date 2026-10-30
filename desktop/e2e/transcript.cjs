// 文稿：生成、选词、剪掉、找口头禅、按词生成字幕。语音识别用假的后端返回固定的词。
const { launch, openHarness, m } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const W = (t, s, e) => ({ t, startMs: s, endMs: e })
// 6 秒的素材：嗯(0–0.4) 今天(0.6–1.4) 那个(1.6–2.2) 天气很好。(2.4–4) —— 停顿 —— 出去玩(5–5.8)
const WORDS = [W('嗯', 0, 400), W('今', 600, 1000), W('天', 1000, 1400), W('那', 1600, 1900), W('个', 1900, 2200), W('天', 2400, 2700), W('气', 2700, 3000), W('很', 3000, 3400), W('好。', 3400, 4000), W('出', 5000, 5300), W('去', 5300, 5600), W('玩', 5600, 5800)]

;(async () => {
  const { srv, browser, page, errors } = await launch()
  await openHarness(page, errors)
  const tab = (name) => page.locator('.insp .seg').getByText(name, { exact: true }).click()
  const proj = async () => {
    await page.waitForTimeout(950)
    return JSON.parse(await page.evaluate(() => localStorage.getItem('clearclip.edit.draft'))).project
  }

  console.log('# 准备')
  await page.evaluate(([f, w]) => { window.__dialogOpen = [f]; window.__transcript = { words: w, wordLevel: true, engine: 'test', fromMs: 0, toMs: 6003 } }, [[m('red.webm')], WORDS])
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await page.waitForTimeout(300)
  await tab('文稿')
  ok('文稿页有“生成文稿”', (await page.getByRole('button', { name: '生成文稿' }).count()) === 1)

  console.log('# 生成与显示')
  await page.getByRole('button', { name: '生成文稿' }).click()
  await page.waitForSelector('.tr .w')
  ok('词都显示出来', (await page.locator('.tr .w').count()) === WORDS.length, String(await page.locator('.tr .w').count()))
  ok('分成两句（句号 + 停顿）', (await page.locator('.tr .sent').count()) === 2, String(await page.locator('.tr .sent').count()))
  const req = await page.evaluate(() => window.__trReqs[0])
  ok('请求的是片段的素材范围', req && req.fromMs === 0 && req.toMs >= 6000, JSON.stringify(req))

  console.log('# 口头禅')
  await page.getByRole('button', { name: '找出口头禅' }).click()
  const marked = await page.locator('.tr .w.on').allTextContents()
  ok('找到 嗯 / 那 / 个', marked.join('') === '嗯那个', marked.join('|'))

  console.log('# 剪掉选中的词')
  await page.getByRole('button', { name: /剪掉选中的词/ }).click()
  await page.waitForTimeout(300)
  let p = await proj()
  const total = p.clips.reduce((n, c) => n + (c.outMs - c.inMs), 0)
  ok('片段被切开', p.clips.length >= 2, String(p.clips.length))
  ok('剪掉了约 1 秒（嗯 + 那个，各留一点余量）', total < 6003 - 500 && total > 6003 - 1500, String(total))
  ok('剪掉的词从文稿里消失', (await page.locator('.tr .w').count()) <= WORDS.length, '')

  console.log('# 撤回')
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(300)
  p = await proj()
  ok('撤回一次回到一个片段', p.clips.length === 1 && p.clips[0].outMs - p.clips[0].inMs > 6000, JSON.stringify(p.clips.map((c) => [c.inMs, c.outMs])))

  console.log('# 点词选择')
  await page.locator('.tr .w', { hasText: '今' }).click({ modifiers: [] })
  await page.locator('.tr .w', { hasText: '好。' }).click({ modifiers: ['Shift'] })
  ok('Shift + 点选一段', (await page.locator('.tr .w.on').count()) === 8, String(await page.locator('.tr .w.on').count()))
  await page.getByRole('button', { name: '取消选择' }).click()
  ok('取消选择', (await page.locator('.tr .w.on').count()) === 0)

  console.log('# 生成字幕')
  await page.locator('.tr .style', { hasText: '逐字高亮' }).click()
  await page.getByRole('button', { name: '按词生成字幕' }).click()
  await page.waitForTimeout(300)
  p = await proj()
  ok('生成了字幕（两句）', p.texts.length === 2, String(p.texts.length))
  ok('字幕带着词和高亮色', p.texts[0].words.length > 0 && p.texts[0].highlight === '#FFD400', JSON.stringify(p.texts[0]).slice(0, 200))
  ok('字幕文字是识别的文字', p.texts[0].text === '嗯今天那个天气很好。', p.texts[0].text)
  ok('第二句从 5 秒开始', Math.abs(p.texts[1].startMs - 5000) <= 20, String(p.texts[1].startMs))
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(300)
  ok('撤回一次去掉全部字幕', (await proj()).texts.length === 0)

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
