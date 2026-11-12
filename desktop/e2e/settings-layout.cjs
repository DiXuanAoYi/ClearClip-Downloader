// 设置页卡片排布：自动平衡 / 按顺序 / 单列。假后端按命令名回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))
const init = `
(() => {
  let cbId = 1, evId = 1
  window.__calls = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback() { return cbId++ },
    unregisterCallback() {},
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': return evId++
        case 'get_settings': return ${JSON.stringify(settings)}
        case 'bt_tracker_status': return { updatedAt: 1763000000, source: 'https://cdn.jsdelivr.net/gh/ngosang/trackerslist@master/trackers_best.txt', listCount: 20, effectiveCount: 20 }
        default: return null
      }
    },
  }
})()
`
;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const page = await browser.newPage({ viewport: { width: 1180, height: 900 }, deviceScaleFactor: 1 })
  page.on('pageerror', (e) => errors.push(e.message + ' @ ' + String(e.stack || '').split('\n').slice(1, 3).join(' | ')))
  await page.addInitScript(init)
  await page.goto('http://127.0.0.1:5199/merge-harness.html#settings')
  await page.waitForSelector('.bgrid .card')
  await page.waitForTimeout(600)
  // 当前页签里卡片占满的比例：卡片面积之和 / 容器面积
  const stat = () =>
    page.evaluate(() => {
      const g = document.querySelector('.el-tab-pane:not([style*="display: none"]) .bgrid') || document.querySelector('.bgrid')
      const cards = [...g.querySelectorAll('.card')]
      const gr = g.getBoundingClientRect()
      const area = cards.reduce((s, c) => { const r = c.getBoundingClientRect(); return s + r.width * r.height }, 0)
      const cols = new Set(cards.map((c) => Math.round(c.getBoundingClientRect().left))).size
      return { n: cards.length, cols, fill: area / (gr.width * gr.height), titles: cards.map((c) => c.querySelector('h3')?.textContent) }
    })
  const bal = await stat()
  ok('默认自动平衡：多列', bal.cols >= 2 && bal.n >= 4, JSON.stringify(bal))
  // 切到“通用”页签，改成按顺序 / 单列
  await page.getByRole('tab', { name: '通用' }).click()
  await page.waitForTimeout(400)
  ok('通用页有“设置页排布”', (await page.getByText('设置页排布').count()) >= 1)
  const nGeneral = (await stat()).n
  await page.getByText('按顺序', { exact: true }).click()
  await page.waitForTimeout(500)
  ok('选“按顺序”后记在本机', (await page.evaluate(() => localStorage.getItem('cc-settings-layout'))) === 'order')
  await page.getByText('单列', { exact: true }).click()
  await page.waitForTimeout(500)
  const single = await stat()
  ok('单列：所有卡片在同一列，数量不变', single.cols === 1 && single.n === nGeneral, JSON.stringify(single))
  await page.getByText('自动平衡', { exact: true }).click()
  await page.waitForTimeout(500)
  const back = await stat()
  ok('改回自动平衡：恢复多列', back.cols >= 2 && back.n === nGeneral, JSON.stringify(back))
  // 比较三种排法的填充率：自动平衡不低于按顺序
  await page.getByText('按顺序', { exact: true }).click()
  await page.waitForTimeout(500)
  const ord = await stat()
  await page.getByText('自动平衡', { exact: true }).click()
  await page.waitForTimeout(500)
  const bal2 = await stat()
  ok('自动平衡的填充率不低于按顺序', bal2.fill >= ord.fill - 0.005, `${bal2.fill.toFixed(3)} vs ${ord.fill.toFixed(3)}`)
  // 各页签（含自带版面的面板）也跟着排布选项走
  const TABS = ['BT 与 ed2k', 'AI', '写作', '安全与隐私', '网络', '账号与 Cookie', '手机与浏览器扩展', '组件']
  const colsOf = (tab) => page.getByRole('tab', { name: tab, exact: true }).click().then(() => page.waitForTimeout(450)).then(stat)
  await page.setViewportSize({ width: 1700, height: 1000 })
  await page.waitForTimeout(400)
  await page.getByRole('tab', { name: '通用' }).click()
  await page.getByText('自动平衡', { exact: true }).click()
  const wideRes = {}
  for (const t of TABS) wideRes[t] = await colsOf(t)
  // AI 页签在假后端下渲染不全（AiPool 拿到 null 会报错），不在这里要求多列
  ok('宽窗口：BT 与 ed2k、写作、安全与隐私、网络、组件 都是多列', ['BT 与 ed2k', '写作', '安全与隐私', '网络', '组件'].every((t) => wideRes[t].cols >= 2), JSON.stringify(Object.fromEntries(Object.entries(wideRes).map(([k, v]) => [k, v.cols]))))
  const pageW = await page.evaluate(() => document.querySelector('.page').getBoundingClientRect().width)
  ok('设置页不再被限制在 1040px，随窗口变宽', pageW > 1200, String(pageW))
  ok('列数最多 4 列', Object.values(wideRes).every((v) => v.cols <= 4))
  // “立即更新”按钮在卡片里面
  await page.getByRole('tab', { name: 'BT 与 ed2k', exact: true }).click()
  await page.waitForTimeout(400)
  await page.screenshot({ path: '/tmp/settings-bt-wide.png' })
  const inside = await page.evaluate(() => {
    const b = document.querySelector('.status .el-button')
    const c = b?.closest('.card')
    if (!b || !c) return null
    const br = b.getBoundingClientRect()
    const cr = c.getBoundingClientRect()
    return { inside: br.left >= cr.left && br.right <= cr.right + 0.5 && br.top >= cr.top && br.bottom <= cr.bottom + 0.5, br: [br.left, br.right], cr: [cr.left, cr.right] }
  })
  ok('“立即更新”按钮在卡片里面', inside && inside.inside, JSON.stringify(inside))
  for (const w of [1180, 700]) {
    await page.setViewportSize({ width: w, height: 900 })
    await page.waitForTimeout(500)
    const i2 = await page.evaluate(() => {
      const b = document.querySelector('.status .el-button')
      const c = b?.closest('.card')
      const br = b.getBoundingClientRect()
      const cr = c.getBoundingClientRect()
      return br.left >= cr.left && br.right <= cr.right + 0.5
    })
    ok(`窗口 ${w}px：“立即更新”仍在卡片里`, i2)
  }
  await page.setViewportSize({ width: 1700, height: 1000 })
  await page.waitForTimeout(400)
  // 单列：这些页签也只有一列
  await page.getByRole('tab', { name: '通用' }).click()
  await page.getByText('单列', { exact: true }).click()
  await page.waitForTimeout(300)
  const singleRes = {}
  for (const t of TABS) singleRes[t] = await colsOf(t)
  ok('单列：所有页签都只有一列（含 BT 与 ed2k、安全与隐私、写作）', Object.values(singleRes).every((v) => v.cols === 1), JSON.stringify(Object.fromEntries(Object.entries(singleRes).map(([k, v]) => [k, v.cols]))))
  await page.getByRole('tab', { name: '通用' }).click()
  await page.getByText('自动平衡', { exact: true }).click()
  await page.setViewportSize({ width: 1180, height: 900 })
  await page.waitForTimeout(300)
  // 窄窗口：一列
  await page.setViewportSize({ width: 520, height: 900 })
  await page.waitForTimeout(600)
  const narrow = await stat()
  ok('窄窗口自动变成一列，没有横向滚动', narrow.cols === 1 && !(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)), JSON.stringify(narrow))
  await page.setViewportSize({ width: 1180, height: 900 })
  await page.waitForTimeout(600)
  ok('宽了再变回多列', (await stat()).cols >= 2)
  // 下载页签：每张卡片都还在
  await page.getByRole('tab', { name: '下载' }).click()
  await page.waitForTimeout(500)
  const dl = await stat()
  ok('下载页签卡片没有丢', dl.n === 6, JSON.stringify(dl.titles))
  await page.screenshot({ path: process.env.SHOT || '/tmp/settings-layout.png' })
  // 假后端对不认识的命令返回 null，别的面板（账号、AI 池…）会因此报错，与排布无关；只看排布相关的
  const mine = errors.filter((e) => /BalancedGrid|SettingsView|balance\.ts/.test(e))
  ok('排布相关没有页面错误', mine.length === 0, mine.join(';'))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
