// 主页：单个解析 / 批量下载切换；粘贴多条链接自动切到批量；批次区默认显示；整批设置默认折叠。
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
const batch = { id: 'b1', name: 'B站收藏夹', source: '粘贴', createdAt: 1, opts: {}, total: 50, queued: 18, running: 3, paused: 0, done: 23, failed: 2, pending: 0, failedLinks: [], baseDir: '/tmp/x' }
const init = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  window.__calls = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      const urls = (t) => (t.match(/https?:\\/\\/[^\\s]+/g) || [])
      switch (cmd) {
        case 'plugin:event|listen': return evId++
        case 'get_settings': return ${JSON.stringify(settings)}
        case 'detect_links': return urls(args.text).map((u) => ({ url: u, platform: 'x', platformName: 'X' }))
        case 'batch_extract': return urls(args.text)
        case 'batch_preview': return null
        case 'batch_preview_cancel': return null
        case 'batch_list': return window.__noBatch ? [] : [${JSON.stringify(batch)}]
        case 'list_history': return []
        case 'downloaded_entries': return []
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
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(init)
  await page.goto('http://127.0.0.1:5299/merge-harness.html#parse')
  await page.waitForSelector('.modes')
  await page.waitForTimeout(600)

  ok('顶部有“单个解析 / 批量下载”切换，默认单个', (await page.locator('.modes button.on').innerText()) === '单个解析')
  ok('单个模式显示粘贴栏', (await page.locator('.paste').count()) === 1)
  ok('主页默认就显示批次区（没有解析结果时）', (await page.locator('.bl').count()) === 1 && (await page.locator('.bx').count()) === 1)
  ok('批次卡片带进度和操作', (await page.locator('.bx').innerText()).includes('23/50 完成') && (await page.locator('.bb button', { hasText: '只重试失败项' }).count()) === 1)

  // 手动切到批量
  await page.locator('.modes button', { hasText: '批量下载' }).click()
  ok('切到批量：显示批量粘贴框，隐藏单个粘贴栏', (await page.locator('.bpaste textarea').count()) === 1 && (await page.locator('.paste').count()) === 0)
  ok('批量模式下批次区仍在', (await page.locator('.bl').count()) === 1)

  // 在批量粘贴框里粘贴多条链接：就地进入预览
  await page.fill('.bpaste textarea', '看这几个 https://a.com/1 和 https://a.com/2\nhttps://b.com/3')
  await page.waitForSelector('.bw')
  ok('粘贴后在主页就地出现批量工作区（不是弹窗）', (await page.locator('.bw').count()) === 1 && (await page.locator('.el-dialog').count()) === 0)
  ok('识别到 3 条', (await page.locator('.bw .top').innerText()).includes('识别到 3 条'))
  ok('整批设置默认折叠为摘要行', (await page.locator('.bw .set').count()) === 0 && (await page.locator('.bw .sum .chip').count()) >= 4)
  ok('底部操作条在', (await page.locator('.bw .foot').innerText()).includes('加入队列并开始'))
  ok('批量工作区打开时隐藏批量粘贴框', (await page.locator('.bpaste').count()) === 0)
  await page.screenshot({ path: process.env.SHOT || '/tmp/home-batch.png' })

  // 切回单个：工作区关闭
  await page.locator('.modes button', { hasText: '单个解析' }).click()
  ok('切回单个：工作区关闭，粘贴栏回来', (await page.locator('.bw').count()) === 0 && (await page.locator('.paste').count()) === 1)

  // 单个模式里粘贴多条链接：自动切到批量
  await page.fill('.paste textarea', 'https://a.com/1 https://a.com/2')
  await page.waitForTimeout(1200)
  await page.waitForSelector('.bw', { timeout: 4000 })
  ok('单个模式粘贴多条链接：自动切到批量并出现工作区', (await page.locator('.modes button.on').innerText()) === '批量下载' && (await page.locator('.bw').count()) === 1)
  ok('自动切换后单个粘贴框已清空', (await page.locator('.paste textarea').count()) === 0)

  ok('没有页面错误', errors.length === 0, errors.join(';'))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
