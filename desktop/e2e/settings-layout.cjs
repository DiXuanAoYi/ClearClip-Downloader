// 设置页：卡片排布（开关在“通用”页）、即时保存、长说明折叠、快捷键录入。假后端按命令名回答。
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
const VW = Number(process.env.VW || 1180)
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))
const init = `
(() => {
  let cbId = 1, evId = 1
  window.__calls = []
  if (location.search.includes('dark')) window.__dark = true
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
        case 'save_settings': return { settings: args.settings, shortcutError: null }
        default: return null
      }
    },
  }
})()
`
;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const page = await browser.newPage({ viewport: { width: VW, height: 900 }, deviceScaleFactor: 1 })
  page.on('pageerror', (e) => errors.push(e.message + ' @ ' + String(e.stack || '').split('\n').slice(1, 3).join(' | ')))
  await page.addInitScript(init)
  if (process.env.DARK) await page.emulateMedia({ colorScheme: 'dark' })
  const BASE = process.env.E2E_BASE || `http://127.0.0.1:${process.env.E2E_PORT || 5199}`
  await page.goto(BASE + '/merge-harness.html' + (process.env.DARK ? '?dark' : '') + '#settings')
  await page.waitForSelector('.bgrid .card')
  await page.waitForTimeout(600)
  const SHOTS = process.env.SHOTS_DIR
  const tag = process.env.SHOT_TAG || ''
  const shoot = async (name) => { await page.evaluate(() => document.querySelectorAll('.el-message').forEach((e) => e.remove())); if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `${name}-${tag}.png`) }) }
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
  await page.locator('.cat', { hasText: '通用' }).click()
  await page.waitForTimeout(400)
  const layoutGroup = page.getByRole('radiogroup', { name: '设置页排布' })
  ok('通用页有“设置页排布”', (await layoutGroup.count()) === 1)
  ok('分类页右上不再放排布开关', (await page.locator('.sethead').getByText('设置页排布').count()) === 0)
  const nGeneral = (await stat()).n
  await layoutGroup.getByText('按顺序', { exact: true }).click()
  await page.waitForTimeout(500)
  ok('选“按顺序”后记在本机', (await page.evaluate(() => localStorage.getItem('cc-settings-layout'))) === 'order')
  await layoutGroup.getByText('单列', { exact: true }).click()
  await page.waitForTimeout(500)
  const single = await stat()
  ok('单列：所有卡片在同一列，数量不变', single.cols === 1 && single.n === nGeneral, JSON.stringify(single))
  await layoutGroup.getByText('自动平衡', { exact: true }).click()
  await page.waitForTimeout(500)
  const back = await stat()
  ok('改回自动平衡：恢复多列', back.cols >= 2 && back.n === nGeneral, JSON.stringify(back))
  // 比较三种排法的填充率：自动平衡不低于按顺序
  await layoutGroup.getByText('按顺序', { exact: true }).click()
  await page.waitForTimeout(500)
  const ord = await stat()
  await layoutGroup.getByText('自动平衡', { exact: true }).click()
  await page.waitForTimeout(500)
  const bal2 = await stat()
  ok('自动平衡的填充率不低于按顺序', bal2.fill >= ord.fill - 0.005, `${bal2.fill.toFixed(3)} vs ${ord.fill.toFixed(3)}`)
  // 窄窗口：一列
  await page.setViewportSize({ width: 520, height: 900 })
  await page.waitForTimeout(600)
  const narrow = await stat()
  ok('窄窗口自动变成一列，没有横向滚动', narrow.cols === 1 && !(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)), JSON.stringify(narrow))
  await page.setViewportSize({ width: VW, height: 900 })
  await page.waitForTimeout(600)
  ok('宽了再变回多列', (await stat()).cols >= 2)
  // 下载页签：每张卡片都还在
  await page.locator('.cat', { hasText: '下载' }).click()
  await page.waitForTimeout(500)
  const dl = await stat()
  ok('下载页签卡片没有丢', dl.n === 6, JSON.stringify(dl.titles))
  await page.screenshot({ path: process.env.SHOT || '/tmp/settings-layout.png' })

  // 其他分类页没有排布开关
  ok('下载页没有排布开关', (await page.getByRole('radiogroup', { name: '设置页排布' }).count()) === 0)
  // ---- 即时保存：改一个开关，不点任何保存按钮，0.4 秒后后端收到 save_settings ----
  const callsOf = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd)
  const before = (await callsOf('save_settings')).length
  await page.locator('.cc-field', { hasText: '按平台分子文件夹' }).locator('.el-switch').click()
  await page.waitForTimeout(900)
  const saved = await callsOf('save_settings')
  ok('改动后自动调用保存，不需要点保存', saved.length === before + 1, String(saved.length - before))
  ok('保存的内容是新值', saved.length > before && saved[saved.length - 1].args.settings.subfolderByPlatform !== settings.subfolderByPlatform)
  ok('顶部提示“已自动保存”', (await page.getByText('已自动保存').count()) >= 1)
  ok('页面里没有残留的“点保存”提示', (await page.getByText(/右下角.?保存/).count()) === 0)
  // ---- 长说明折叠 / 展开 ----
  await page.locator('.cat', { hasText: '通用' }).click()
  await page.waitForTimeout(400)
  const fold = page.locator('.cc-field', { hasText: '悬浮窗' }).first()
  const more = fold.locator('button.more')
  ok('悬浮窗的长说明被折叠，有“了解更多”', (await more.count()) === 1 && (await more.textContent()).trim() === '了解更多')
  await more.click()
  ok('点“了解更多”展开（aria-expanded）', (await more.getAttribute('aria-expanded')) === 'true' && (await more.textContent()).trim() === '收起')
  await more.click()
  ok('再点“收起”折回', (await more.getAttribute('aria-expanded')) === 'false')
  await shoot('general')
  // ---- 安全页：老板键录入 ----
  await page.locator('.cat', { hasText: '安全与隐私' }).click()
  await page.waitForTimeout(500)
  const hk = page.locator('button.hk-btn[aria-label^="老板键"]')
  ok('老板键是快捷键录入控件（不是文本框）', (await hk.count()) === 1 && (await page.locator('input[placeholder="CommandOrControl+Shift+H"]').count()) === 0)
  ok('未设置时显示“未设置”', (await hk.textContent()).includes('未设置'))
  await hk.focus()
  await page.keyboard.press('Enter')
  ok('回车进入录入状态', (await hk.getAttribute('aria-label')).includes('正在录入') && (await hk.textContent()).includes('按下新的组合键'))
  await page.keyboard.press('Escape')
  ok('Esc 取消录入，值不变', !(await hk.getAttribute('aria-label')).includes('正在录入') && (await hk.textContent()).includes('未设置'))
  await hk.click()
  await page.keyboard.press('KeyH')
  ok('缺少修饰键：就地提示', (await page.getByRole('alert').filter({ hasText: '需要和' }).count()) === 1)
  await page.keyboard.press('Control+KeyC')
  ok('抢系统常用键：提示', (await page.getByRole('alert').filter({ hasText: '常用的系统快捷键' }).count()) === 1)
  await page.keyboard.press('Control+Shift+KeyD')
  ok('与别的功能冲突：提示', (await page.getByRole('alert').filter({ hasText: '同一个组合' }).count()) === 1)
  await shoot('security-invalid')
  await page.keyboard.press('Control+Shift+KeyH')
  await page.waitForTimeout(900)
  ok('录入成功后显示当前组合', (await hk.locator('kbd').allTextContents()).join('+') === 'CommandOrControl+Shift+H')
  const lastSave = (await callsOf('save_settings')).pop()
  ok('并且即时保存', lastSave && lastSave.args.settings.security.panicShortcut === 'CommandOrControl+Shift+H', JSON.stringify(lastSave && lastSave.args.settings.security.panicShortcut))
  ok('录入完成后不再处于录入状态', !(await hk.getAttribute('aria-label')).includes('正在录入'))
  await page.getByRole('button', { name: '清除', exact: true }).first().click()
  await page.waitForTimeout(900)
  ok('“清除”后回到未设置', (await hk.textContent()).includes('未设置'))
  const lockDesc = page.locator('.cc-field', { hasText: '应用锁' }).first()
  ok('应用锁说明不被截断：放得下就全显示，放不下就有“了解更多”', (await lockDesc.locator('button.more').count()) === 1 || (await lockDesc.locator('.desc').evaluate((e) => e.scrollHeight <= e.clientHeight + 1)))
  await shoot('security')
  // ---- AI 页：无“点保存”矛盾文案，密钥按钮写明“保存密钥” ----
  await page.locator('.cat', { hasText: /^AI$/ }).click()
  await page.waitForTimeout(500)
  ok('AI 页没有“点右下角保存”的旧提示', (await page.getByText(/右下角.?保存/).count()) === 0)
  const poolDesc = page.locator('.cc-field', { hasText: '多个服务自动轮换' })
  ok('AI 页的长说明：放得下就全显示，放不下就折叠成“了解更多”', (await poolDesc.locator('button.more').count()) === 1 || (await poolDesc.locator('.desc').evaluate((e) => e.scrollHeight <= e.clientHeight + 1)))
  await shoot('ai')
  // 假后端对不认识的命令返回 null，别的面板（账号、AI 池…）会因此报错，与排布无关；只看排布相关的
  const mine = errors.filter((e) => /BalancedGrid|SettingsView|balance\.ts/.test(e))
  ok('排布相关没有页面错误', mine.length === 0, mine.join(';'))
  console.log(`\n${pass} 通过，${fail} 失败`)
  await browser.close()
  process.exit(fail ? 1 : 0)
})()
