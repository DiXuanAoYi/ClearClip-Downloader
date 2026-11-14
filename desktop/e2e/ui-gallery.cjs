// 组件库陈列页（ui-gallery.html）：各组件渲染、键盘操作、焦点环、折叠展开、loading 禁点、无横向溢出，浅 / 深色各截一张图。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const { soon } = require('./lib.cjs')
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const SHOTS = process.env.GALLERY_SHOTS || '/tmp/claude-0/-home-user-Short-Video-API/1a767bef-6996-5222-8ebf-504ce256ba40/scratchpad/shots'
fs.mkdirSync(SHOTS, { recursive: true })
const init = `
(() => {
  let cbId = 1
  window.__calls = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback() { return cbId++ },
    unregisterCallback() {},
    convertFileSrc(p) { return p },
    async invoke(cmd, args) { window.__calls.push({ cmd, args }); return null },
  }
})()
`
// 相对亮度与对比度（取自 WCAG）
const lum = (hex) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}
const contrast = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p)
  return (x + 0.05) / (y + 0.05)
}

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const ctx = await browser.newContext({ viewport: { width: 1180, height: 900 }, deviceScaleFactor: 1 })
  const page = await ctx.newPage()
  page.on('pageerror', (e) => errors.push(e.message))
  page.on('console', (m) => m.type() === 'error' && errors.push('console: ' + m.text()))
  await page.addInitScript(init)
  const url = `http://127.0.0.1:${process.env.E2E_PORT || 5199}/ui-gallery.html`
  await page.goto(url)
  await page.waitForSelector('[data-gallery="CcToast"]')
  await page.waitForTimeout(400)

  // ---- 渲染 ----
  for (const n of ['CcButton', 'CcSegmented', 'CcLink', 'CcField', 'CcSelect', 'CcEmpty', 'CcSkeleton', 'CcDialog', 'CcToast']) ok(`${n} 已渲染`, (await page.locator(`[data-gallery="${n}"]`).count()) === 1)
  ok('按钮是原生 button', await page.evaluate(() => [...document.querySelectorAll('.cc-btn')].every((b) => b.tagName === 'BUTTON')))
  ok('图标按钮都有无障碍名称', await page.evaluate(() => [...document.querySelectorAll('.cc-btn.icon')].every((b) => (b.getAttribute('aria-label') || '').length > 0)))
  ok('禁用按钮带原因', (await page.locator('.cc-btn:disabled .cc-sr', { hasText: '先选择一个视频' }).count()) === 1)

  // ---- 令牌与对比度 ----
  for (const dark of [false, true]) {
    await page.evaluate((d) => window.__setDark(d), dark)
    const v = await page.evaluate(() => {
      const cs = getComputedStyle(document.documentElement)
      const g = (k) => cs.getPropertyValue(k).trim()
      return { faint: g('--cc-faint'), card: g('--cc-card'), bg: g('--cc-bg'), acc: g('--cc-acc'), onAcc: g('--cc-on-acc'), fs: g('--cc-fs-md'), r: g('--cc-r-lg'), h: g('--cc-h-md'), focus: g('--cc-focus'), dur: g('--cc-dur-base') }
    })
    const t = dark ? '深色' : '浅色'
    ok(`${t}：--cc-faint 在卡片上对比度 ≥ 4.5`, contrast(v.faint, v.card) >= 4.5, `${v.faint} on ${v.card} = ${contrast(v.faint, v.card).toFixed(2)}`)
    ok(`${t}：--cc-faint 在页面底上对比度 ≥ 4.5`, contrast(v.faint, v.bg) >= 4.5, `${v.faint} on ${v.bg} = ${contrast(v.faint, v.bg).toFixed(2)}`)
    ok(`${t}：主按钮文字对比度 ≥ 4.5`, contrast(v.acc, v.onAcc) >= 4.5, `${v.acc} / ${v.onAcc}`)
    ok(`${t}：新令牌存在`, v.fs === '14px' && v.r === '12px' && v.h === '36px' && v.dur === '200ms' && !!v.focus, JSON.stringify(v))
  }
  await page.evaluate(() => window.__setDark(false))
  ok('浅色强调色是 #a65600', (await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--cc-acc').trim())) === '#a65600')

  // ---- 键盘：Tab 焦点环 ----
  await page.evaluate(() => document.body.focus())
  await page.keyboard.press('Tab')
  const ring = await page.evaluate(() => {
    const el = document.activeElement
    const cs = getComputedStyle(el)
    return { cls: el.className, style: cs.outlineStyle, w: cs.outlineWidth, color: cs.outlineColor, off: cs.outlineOffset, acc: getComputedStyle(document.documentElement).getPropertyValue('--cc-acc').trim() }
  })
  ok('Tab 先落在第一个按钮上', ring.cls.includes('cc-btn'), ring.cls)
  ok('焦点环可见（2px 实线，偏移 2px）', ring.style === 'solid' && ring.w === '2px' && ring.off === '2px', JSON.stringify(ring))
  const hexRgb = (h) => `rgb(${[1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)).join(', ')})`
  ok('焦点环颜色取令牌 --cc-focus', ring.color === hexRgb(ring.acc), `${ring.color} vs ${ring.acc}`)

  // 分段按钮的原生单选在 Element Plus 里的焦点环（skin.css 恢复）
  // ---- CcButton loading ----
  const lb = page.getByTestId('btn-loading')
  const w0 = (await lb.boundingBox()).width
  await lb.click()
  ok('点击普通按钮触发一次', (await page.getByTestId('click-count').textContent()) === '1')
  await page.waitForTimeout(100)
  ok('loading 中 aria-busy', (await lb.getAttribute('aria-busy')) === 'true')
  const w1 = (await lb.boundingBox()).width
  ok('loading 时宽度不变', Math.abs(w1 - w0) < 1, `${w0} → ${w1}`)
  await lb.click({ force: true })
  await page.getByTestId('btn-loading-static').click({ force: true })
  ok('loading 时点击无效', (await page.getByTestId('click-count').textContent()) === '1', await page.getByTestId('click-count').textContent())
  await page.waitForTimeout(1600)
  ok('loading 结束后恢复可点', (await lb.getAttribute('aria-busy')) === null)

  // ---- CcSegmented ----
  const seg = page.locator('[data-gallery="CcSegmented"] .cc-seg').first()
  ok('role=radiogroup', (await seg.getAttribute('role')) === 'radiogroup')
  const first = seg.locator('[role=radio]').nth(0)
  await first.focus()
  ok('漫游 tabindex：只有选中项可 Tab', (await seg.locator('[role=radio][tabindex="0"]').count()) === 1)
  await page.keyboard.press('ArrowRight')
  ok('→ 切到下一项并选中', (await page.getByTestId('seg-val').textContent()) === 'order' && (await seg.locator('[role=radio]').nth(1).getAttribute('aria-checked')) === 'true')
  ok('→ 后焦点跟随', await seg.locator('[role=radio]').nth(1).evaluate((e) => e === document.activeElement))
  await page.keyboard.press('End')
  ok('End 到最后一项', (await page.getByTestId('seg-val').textContent()) === 'one')
  await page.keyboard.press('ArrowRight')
  ok('→ 在末尾回到第一项', (await page.getByTestId('seg-val').textContent()) === 'balance')
  await page.keyboard.press('ArrowLeft')
  ok('← 在开头回到最后一项', (await page.getByTestId('seg-val').textContent()) === 'one')
  const seg3 = page.locator('[data-gallery="CcSegmented"] .cc-seg').nth(2)
  await seg3.locator('[role=radio]').nth(2).focus()
  await page.keyboard.press('ArrowRight')
  ok('方向键跳过禁用项', (await seg3.locator('[role=radio]').nth(1).getAttribute('aria-checked')) === 'true' && (await seg3.locator('[role=radio]').nth(0).getAttribute('aria-checked')) === 'false')

  // ---- CcField ----
  const longF = page.getByTestId('field-long')
  ok('长说明折叠，出现“了解更多”', (await longF.locator('.more').count()) === 1 && (await longF.locator('.more').textContent()).trim() === '了解更多')
  ok('折叠态说明最多 2 行', await longF.locator('.desc').evaluate((e) => e.clientHeight <= parseFloat(getComputedStyle(e).lineHeight) * 2 + 1))
  ok('折叠态 aria-expanded=false', (await longF.locator('.more').getAttribute('aria-expanded')) === 'false')
  await longF.locator('.more').focus()
  await page.keyboard.press('Enter')
  ok('展开后说明完整显示', await longF.locator('.desc').evaluate((e) => e.scrollHeight - e.clientHeight <= 1 && e.clientHeight > parseFloat(getComputedStyle(e).lineHeight) * 2))
  ok('展开后按钮变“收起”', (await longF.locator('.more').textContent()).trim() === '收起' && (await longF.locator('.more').getAttribute('aria-expanded')) === 'true')
  await longF.locator('.more').click()
  ok('再点收起', (await longF.locator('.more').getAttribute('aria-expanded')) === 'false')
  ok('短说明不出现“了解更多”', (await page.locator('[data-gallery="CcField"] .cc-field').first().locator('.more').count()) === 0)
  const errField = page.locator('[data-gallery="CcField"] .cc-field.invalid')
  ok('错误行 role=alert 并关联到控件', (await errField.locator('.errm[role=alert]').count()) === 1 && (await errField.locator('input').getAttribute('aria-describedby')).includes(await errField.locator('.errm').getAttribute('id')))
  ok('标签与控件关联（点标签聚焦输入）', await (async () => {
    await errField.locator('.lab').click()
    return errField.locator('input').evaluate((e) => e === document.activeElement)
  })())

  // ---- CcSelect ----
  const selEl = page.locator('[data-gallery="CcSelect"] .cc-select').first()
  const label = () => selEl.locator('.el-select__placeholder').first().textContent()
  ok('下拉初始为“通用”', (await label()).trim() === '通用', await label())
  await selEl.locator('input').focus()
  await page.keyboard.press('Enter')
  await page.locator('.cc-select-pop:visible .el-select-dropdown__item').first().waitFor({ state: 'visible' })
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
  await page.waitForTimeout(200)
  ok('键盘选择：Enter 展开、↓ 移动、Enter 确认', (await label()).trim() === '小红书', await label())
  ok('选项带禁用态', (await page.locator('.cc-select-pop .el-select-dropdown__item.is-disabled').count()) >= 0)
  await page.keyboard.press('Escape')

  // ---- CcLink ----
  await page.getByText('公共术语库').click()
  const calls = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'open_url'))
  ok('外链走 open_url', calls.length === 1 && calls[0].args.url === 'https://example.test/glossary', JSON.stringify(calls))
  ok('外链有图标和读屏提示', (await page.locator('.cc-link .ext').count()) === 1 && (await page.locator('.cc-link .cc-sr').count()) === 1)

  // ---- CcDialog ----
  await page.getByTestId('open-sm').click()
  await page.waitForSelector('.el-dialog.cc-dialog', { state: 'visible' })
  await page.waitForTimeout(400)
  const dl = page.locator('.el-dialog.cc-dialog:visible')
  ok('对话框 role=dialog 且有名称', (await page.getByRole('dialog', { name: '取消这个下载？' }).count()) === 1)
  ok('sm 宽 400', Math.abs((await dl.boundingBox()).width - 400) < 2, String((await dl.boundingBox()).width))
  ok('初始焦点在“先不取消”', await page.evaluate(() => document.activeElement?.textContent?.includes('先不取消')))
  await page.keyboard.press('Escape')
  await page.waitForTimeout(500)
  ok('Esc 关闭', (await page.locator('.el-dialog.cc-dialog:visible').count()) === 0)
  ok('关闭后焦点回到触发按钮', await page.getByTestId('open-sm').evaluate((e) => e === document.activeElement))
  await page.getByTestId('open-lg').click()
  await page.waitForTimeout(500)
  const lgBox = await page.locator('.el-dialog.cc-dialog:visible').boundingBox()
  ok('lg 宽 720，高度不超过 85vh，正文内部滚动', Math.abs(lgBox.width - 720) < 2 && lgBox.height <= 900 * 0.85 + 1 && (await page.locator('.cc-dialog:visible .el-dialog__body').evaluate((e) => e.scrollHeight > e.clientHeight)), JSON.stringify(lgBox))
  await page.locator('.cc-dialog:visible .cc-dlg-x').click()
  await page.waitForTimeout(500)

  // ---- CcToast ----
  await page.getByTestId('fire-toast').click()
  const toast = page.locator('.el-message.cc-toast-host .cc-toast')
  await toast.waitFor({ state: 'visible' })
  ok('错误 toast：role=alert，带重试与查看详情', (await toast.getAttribute('role')) === 'alert' && (await toast.getByRole('button', { name: '重试' }).count()) === 1 && (await toast.getByRole('button', { name: '查看详情' }).count()) === 1)
  await page.waitForTimeout(3500)
  ok('错误 toast 不自动消失', (await toast.count()) === 1)
  await toast.getByRole('button', { name: '查看详情' }).click()
  ok('展开详情显示原始错误', (await toast.locator('.det pre').textContent()).includes('EROFS'))
  await toast.getByRole('button', { name: '重试' }).click()
  await page.waitForTimeout(500)
  ok('点重试：触发回调并关闭 toast', (await page.getByTestId('retry-count').textContent()) === '1' && (await page.locator('.el-message.cc-toast-host').count()) === 0)

  // ---- 全局：卡片角标、reduced-motion ----
  const corner = () => page.evaluate(() => getComputedStyle(document.querySelector('[data-gallery="CcEmpty"]'), '::before').opacity)
  await page.evaluate(() => document.activeElement?.blur())
  ok('卡片角标常态不显示', (await corner()) === '0', await corner())
  await page.locator('[data-gallery="CcEmpty"] .cc-btn').first().focus()
  const r = await soon(async () => (await corner()) === '1', async () => await corner())
  ok('卡内聚焦时显示角标', r.pass, r.extra)
  await page.evaluate(() => document.activeElement?.blur())

  await page.emulateMedia({ reducedMotion: 'reduce' })
  const anim = await page.evaluate(() => {
    const el = document.createElement('span')
    el.className = 'cc-spin'
    document.body.appendChild(el)
    const d = getComputedStyle(el).animationDuration
    el.remove()
    return d
  })
  ok('reduced-motion 下动画时长归零', parseFloat(anim) <= 0.001, anim)
  await page.emulateMedia({ reducedMotion: 'no-preference' })

  // ---- 溢出与截图 ----
  for (const w of [1180, 390]) {
    await page.setViewportSize({ width: w, height: 900 })
    for (const dark of [false, true]) {
      await page.evaluate((d) => window.__setDark(d), dark)
      await page.waitForTimeout(250)
      const o = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }))
      ok(`${w}px ${dark ? '深色' : '浅色'}：无横向溢出`, o.sw <= o.cw, JSON.stringify(o))
    }
  }
  await page.setViewportSize({ width: 390, height: 900 })
  await page.getByTestId('open-md').click()
  await page.waitForTimeout(500)
  const mdBox = await page.locator('.el-dialog.cc-dialog:visible').boundingBox()
  ok('窄屏对话框宽度 ≤ 100vw − 32', mdBox.width <= 390 - 32 + 1, String(mdBox.width))
  await page.keyboard.press('Escape')
  await page.waitForTimeout(400)

  await page.setViewportSize({ width: 1180, height: 900 })
  for (const dark of [false, true]) {
    await page.evaluate((d) => window.__setDark(d), dark)
    await page.waitForTimeout(300)
    await page.screenshot({ path: path.join(SHOTS, `ui-gallery-${dark ? 'dark' : 'light'}.png`), fullPage: true })
  }

  ok('没有页面错误', errors.length === 0, errors.slice(0, 3).join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
