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
