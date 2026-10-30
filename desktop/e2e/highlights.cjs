// 精彩片段：分析请求、候选列表、加到时间线末尾、只留这段、结合文稿、让 AI 挑选前的确认。
const { launch, openHarness, m } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const W = (t, s, e) => ({ t, startMs: s, endMs: e })
const WORDS = [W('今', 600, 1000), W('天', 1000, 1400), W('很', 2400, 2700), W('好。', 2700, 3000), W('出', 5000, 5300), W('去', 5300, 5600)]
const CANDS = [
  { startMs: 1000, endMs: 3000, score: 88.4, title: '今天很好', reasons: ['声音响', '语速快'] },
  { startMs: 4000, endMs: 5800, score: 61, title: '第二段', reasons: ['镜头切换多'] },
]

;(async () => {
  const { srv, browser, page, errors } = await launch()
  await openHarness(page, errors)
  const tab = (name) => page.locator('.insp .seg').getByText(name, { exact: true }).click()
  const proj = async () => {
    await page.waitForTimeout(950)
    return JSON.parse(await page.evaluate(() => localStorage.getItem('clearclip.edit.draft'))).project
  }

  await page.evaluate(
    ([f, w, c]) => {
      window.__dialogOpen = [f]
      window.__transcript = { words: w, wordLevel: true, engine: 'test', fromMs: 0, toMs: 6003 }
      window.__highlights = c
      window.__highlightsLlm = [c[1]]
    },
    [[m('red.webm')], WORDS, CANDS],
  )
  await page.locator('.bar .el-dropdown', { hasText: '添加素材' }).locator('.el-button-group > .el-button').first().click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 1)
  await page.waitForTimeout(300)
  await tab('精彩')

  console.log('# 分析')
  ok('有“找精彩片段”按钮', (await page.getByRole('button', { name: '找精彩片段' }).count()) === 1)
  await page.getByRole('button', { name: '找精彩片段' }).click()
  await page.waitForSelector('.hl .cand')
  ok('列出两个候选', (await page.locator('.hl .cand').count()) === 2)
  const req = await page.evaluate(() => window.__hlReqs[0])
  ok('请求带着素材范围和默认设置', req && req.fromMs === 0 && req.toMs >= 6000 && req.count === 5 && req.minSecs === 15 && req.maxSecs === 60 && req.sentences.length === 0, JSON.stringify(req))

  console.log('# 使用候选')
  await page.locator('.hl .cand').first().getByRole('button', { name: '加到时间线末尾' }).click()
  let p = await proj()
  ok('主轨多了一个片段，入出点是候选的范围', p.clips.length === 2 && p.clips[1].inMs === 1000 && p.clips[1].outMs === 3000, JSON.stringify(p.clips.map((c) => [c.inMs, c.outMs])))
  ok('原片段没有动', p.clips[0].inMs === 0 && p.clips[0].outMs > 5000)
  await tab('精彩')
  await page.locator('.blk.clip').first().click()
  await page.waitForTimeout(200)
  await page.getByRole('button', { name: '找精彩片段' }).click()
  await page.waitForSelector('.hl .cand')
  await page.locator('.hl .cand').nth(1).getByRole('button', { name: '只留这段' }).click()
  p = await proj()
  ok('“只留这段”把当前片段裁成候选范围', p.clips[0].inMs === 4000 && p.clips[0].outMs === 5800, JSON.stringify(p.clips.map((c) => [c.inMs, c.outMs])))
  await page.keyboard.press('Control+z')
  p = await proj()
  ok('可以撤销', p.clips[0].inMs === 0)

  console.log('# 结合文稿与竖屏')
  await page.locator('.hl .el-checkbox', { hasText: '结合文稿' }).click()
  await page.locator('.hl .el-checkbox', { hasText: '9:16' }).click()
  await page.getByRole('button', { name: '找精彩片段' }).click()
  await page.waitForFunction(() => (window.__hlReqs || []).length >= 3)
  const r3 = await page.evaluate(() => window.__hlReqs[2])
  ok('请求里带了文稿句子', r3.sentences.length >= 2 && r3.sentences.every((s) => s.text && s.endMs > s.startMs), JSON.stringify(r3.sentences))
  await page.locator('.hl .cand').first().getByRole('button', { name: '加到时间线末尾' }).click()
  p = await proj()
  ok('勾了竖屏后输出是 1080×1920', p.out.width === 1080 && p.out.height === 1920, JSON.stringify(p.out))

  console.log('# 让 AI 挑选')
  await page.getByRole('button', { name: '让 AI 挑选' }).click()
  await page.waitForSelector('.el-message-box')
  ok('先弹出确认，说明会发送文字', (await page.locator('.el-message-box').innerText()).includes('文字'))
  await page.locator('.el-message-box').getByRole('button', { name: '取消' }).click()
  await page.waitForTimeout(300)
  ok('取消就不发请求', (await page.evaluate(() => (window.__hlLlmReqs || []).length)) === 0)
  await page.getByRole('button', { name: '让 AI 挑选' }).click()
  await page.waitForSelector('.el-message-box')
  await page.locator('.el-message-box').getByRole('button', { name: '发送并挑选' }).click()
  await page.waitForFunction(() => (window.__hlLlmReqs || []).length === 1)
  ok('确认后发请求，且带着文稿', (await page.evaluate(() => window.__hlLlmReqs[0].sentences.length)) >= 2)
  await page.waitForFunction(() => document.querySelectorAll('.hl .cand').length === 1)
  ok('列表换成 AI 的结果', (await page.locator('.hl .cand').count()) === 1)

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
