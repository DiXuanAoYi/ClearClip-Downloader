// 下载队列行操作（图标按钮 + 更多）、侧栏滚动与收起、工具箱首页快捷卡、禁用原因、英文搜索。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = process.env.E2E_PORT || 5199
const SHOTS = process.env.SHOTS_DIR || ''
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))
settings.disclaimerAccepted = true
settings.onboarded = true
const MB = 1024 * 1024
const mk = (id, over) => ({
  id, platform: 'x', platformName: '示例', site: 'x', mediaId: 'm' + id, title: '任务' + id, author: '', cover: null,
  assetId: 'a', assetLabel: '1080P', assetKind: 'video', filePath: `/d/任务${id}.mp4`, status: 'queued', received: 0, total: 100 * MB,
  speed: 0, error: null, errorKind: null, note: null, warning: null, resumable: true, step: 'download', inputs: 1,
  priority: 0, startAt: null, createdAt: 1700000000 + id, finishedAt: null, ...over,
})
const TASKS = [
  mk(1, { status: 'running', received: 30 * MB, speed: 2 * MB, priority: 5 }),
  mk(2, { status: 'running', received: 60 * MB, speed: 3 * MB, priority: 4, warning: '磁盘剩余空间不足 2 GB，继续下载可能失败。' }),
  mk(3, { status: 'queued', priority: 3 }),
  mk(4, { status: 'paused', received: 5 * MB }),
  mk(5, { status: 'failed', error: '连接被重置（HTTP 403）。可能需要登录或更换节点。', errorKind: 'network' }),
  mk(6, { status: 'done', received: 100 * MB, finishedAt: 1700001000 }),
  mk(7, { status: 'done', received: 52 * MB, finishedAt: 1700000900 }),
  mk(8, { status: 'done', received: 52 * MB, finishedAt: 1700000800 }),
]
const mkInit = (lang, theme) => `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  window.__calls = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  const st = ${JSON.stringify({ ...settings, language: lang, theme })}
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return st
        case 'get_app_info': return { version: '1.12.0', commit: 'test', debug: false, providers: [] }
        case 'lock_status': return { enabled: false, locked: false, lockedOutSecs: 0 }
        case 'list_tasks': return ${JSON.stringify(TASKS)}
        case 'list_orphan_parts': case 'list_history': case 'batch_list': case 'media_jobs': case 'list_library': return []
        case 'get_after_all_done': return 'none'
        case 'inbox_counts': return { unhandled: 0, total: 0 }
        case 'media_info': return { durationMs: 60000, width: 1920, height: 1080, hasVideo: true, hasAudio: true, tags: {} }
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const open = async (hash, vp, { lang = 'zh', scheme = 'dark' } = {}) => {
    const ctx = await browser.newContext({ viewport: vp, colorScheme: scheme })
    const page = await ctx.newPage()
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript(mkInit(lang, scheme))
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#app`)
    await page.waitForTimeout(1800)
    await page.evaluate((v) => { window.__app.view = v }, hash)
    await page.waitForTimeout(500)
    return page
  }
  const shot = async (page, name) => {
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, name + '.png') })
  }

  // 1. 队列行：图标按钮都有名称，其余收进“更多”
  {
    const page = await open('queue', { width: 1280, height: 800 })
    const row = page.locator('.task', { hasText: '任务1' })
    ok('进行中：暂停是图标按钮，有 aria-label', (await row.locator('button[aria-label="暂停"]').count()) === 1)
    ok('旧的文字按钮（详情/取消/移除）不再直接露出', (await row.locator('.actions').innerText()).trim() === '')
    const unnamed = await page.locator('.task .actions button').evaluateAll((bs) => bs.filter((b) => !b.getAttribute('aria-label') && !b.innerText.trim()).length)
    ok('每个操作按钮都有名称', unnamed === 0, String(unnamed))
    ok('失败行：“重试”是带文字的主按钮', (await page.locator('.task', { hasText: '任务5' }).locator('button', { hasText: '重试' }).count()) === 1)
    ok('警告行有图标和读屏前缀', (await page.locator('.warn-line svg').count()) === 1 && (await page.locator('.warn-line .cc-sr').innerText()).includes('警告'))
    await row.locator('[data-testid="task-more"]').click()
    await page.locator('.el-dropdown-menu:visible .el-dropdown-menu__item').first().waitFor()
    await page.waitForTimeout(300)
    const items = await page.locator('.el-dropdown-menu:visible .el-dropdown-menu__item').allInnerTexts()
    ok('更多菜单：详情 + 取消下载', items.join('|') === '详情|取消下载', items.join('|'))
    await shot(page, 'queue-1280-dark-menu')
    await page.keyboard.press('Escape')
    await page.locator('.task', { hasText: '任务1' }).locator('[data-testid="task-more"]').focus()
    await page.keyboard.press('Enter')
    await page.waitForTimeout(300)
    ok('键盘 Enter 能打开更多菜单', (await page.locator('.el-dropdown-menu:visible').count()) >= 1)
    await page.keyboard.press('Escape')
    await page.waitForTimeout(200)
    // 等待行：优先 + 更多里有排序
    await page.locator('.task', { hasText: '任务3' }).locator('[data-testid="task-more"]').click()
    await page.waitForTimeout(300)
    const wi = await page.locator('.el-dropdown-menu:visible .el-dropdown-menu__item').allInnerTexts()
    ok('等待行更多菜单含置顶/定时/取消', wi.includes('置顶') && wi.some((x) => x.includes('定时')) && wi.includes('取消下载'), wi.join('|'))
    // 取消下载要确认
    await page.locator('.danger-item:visible').first().evaluate((e) => e.click())
    await page.waitForTimeout(400)
    ok('取消下载弹确认，还没真取消', (await page.locator('.el-message-box').count()) === 1 && !(await page.evaluate(() => window.__calls.some((c) => c.cmd === 'cancel_task'))))
    await page.keyboard.press('Escape')
    await page.close()
  }
  // 2. 队列 900×700：最后一行可滚到并完整显示
  for (const scheme of ['dark', 'light']) {
    const page = await open('queue', { width: 900, height: 700 }, { scheme })
    const last = await page.evaluate(() => {
      const l = document.querySelector('.list')
      l.scrollTop = l.scrollHeight
      return new Promise((r) => setTimeout(() => {
        const rows = [...document.querySelectorAll('.task')]
        const lastRow = rows[rows.length - 1].getBoundingClientRect()
        const foot = document.querySelector('.qfoot').getBoundingClientRect()
        r({ bottom: lastRow.bottom, foot: foot.top })
      }, 300))
    })
    ok(`900×700 ${scheme}：最后一行不被底栏裁切`, last.bottom <= last.foot + 0.5, JSON.stringify(last))
    await shot(page, `queue-900x700-${scheme}`)
    await page.close()
  }
  {
    const page = await open('queue', { width: 1280, height: 800 }, { scheme: 'light' })
    await shot(page, 'queue-1280-light')
    await page.close()
  }

  // 3. 侧栏：900×700 logo 在视口内，设置可见；手动收起并记住；矮窗自动图标
  for (const scheme of ['dark', 'light']) {
    const page = await open('queue', { width: 900, height: 700 }, { scheme })
    const box = await page.evaluate(() => {
      const r = (s) => document.querySelector(s).getBoundingClientRect()
      return { logo: r('.side .logo'), settings: r('.navbot .nav'), vh: innerHeight, sideH: r('.side').height }
    })
    ok(`900×700 ${scheme}：logo 与设置都在视口内`, box.logo.top >= 0 && box.settings.bottom <= box.vh, JSON.stringify(box))
    const foot = await page.locator('.foot .ver').innerText()
    ok('页脚只有版本，不再有“下载中 2 · 等…”', !foot.includes('下载中'), foot)
    await shot(page, `shell-900x700-${scheme}`)
    await page.close()
  }
  {
    const page = await open('queue', { width: 900, height: 700 })
    const w0 = Math.round((await page.locator('nav.side').boundingBox()).width)
    await page.locator('[data-testid="nav-toggle"]').click()
    await page.waitForTimeout(500)
    const w1 = Math.round((await page.locator('nav.side').boundingBox()).width)
    ok('手动收起：88 → 56', w0 === 88 && w1 === 56, `${w0}→${w1}`)
    ok('收起后按钮仍有名称', (await page.locator('nav.side .nav[aria-label="下载队列"]').count()) === 1 && (await page.locator('[data-testid="nav-toggle"]').getAttribute('aria-label')) === '展开侧栏')
    ok('收起后队列徽标保留', /^\d+$/.test(await page.locator('nav.side .nav em').first().innerText()))
    await shot(page, 'shell-900x700-collapsed')
    await page.reload()
    await page.waitForTimeout(1500)
    ok('收起状态刷新后记住', Math.round((await page.locator('nav.side').boundingBox()).width) === 56)
    await page.locator('[data-testid="nav-toggle"]').click()
    await page.waitForTimeout(500)
    ok('再点展开', Math.round((await page.locator('nav.side').boundingBox()).width) === 88)
    await page.close()
    const p2 = await open('queue', { width: 900, height: 520 })
    ok('窗口矮于 560：自动只显示图标，且没有手动开关', Math.round((await p2.locator('nav.side').boundingBox()).width) === 56 && (await p2.locator('[data-testid="nav-toggle"]').count()) === 0)
    await shot(p2, 'shell-900x520-auto')
    await p2.close()
  }

  // 4. 工具箱首页：四张快捷卡直达；禁用原因；英文搜索
  for (const [vp, tag] of [[{ width: 1280, height: 800 }, '1280'], [{ width: 900, height: 700 }, '900x700']]) {
    for (const scheme of ['dark', 'light']) {
      const page = await open('tools', vp, { scheme })
      if (scheme === 'dark' && tag === '1280') {
        const ids = await page.locator('.qcard').evaluateAll((b) => b.map((e) => e.getAttribute('data-testid')))
        ok('四张快捷卡：剪辑/截图/GIF/压缩', ids.join() === 'quick-edit,quick-frame,quick-gif,quick-compress', ids.join())
        await page.locator('[data-testid="quick-gif"]').click()
        await page.waitForTimeout(300)
        ok('点“转 GIF”卡直达该工具', (await page.locator('.qcard.on').getAttribute('data-testid')) === 'quick-gif' && (await page.locator('.tbs .item.on').innerText()).includes('GIF'))
        await page.locator('[data-testid="quick-frame"]').focus()
        await page.keyboard.press('Enter')
        await page.waitForTimeout(300)
        ok('键盘 Enter 触发快捷卡', (await page.locator('.qcard.on').getAttribute('data-testid')) === 'quick-frame')
        await page.locator('[data-testid="quick-compress"]').click()
        await page.waitForTimeout(300)
        const btn = page.locator('button', { hasText: '开始处理' }).first()
        ok('没文件时“开始处理”禁用，并说明原因', (await btn.isDisabled()) && ((await btn.getAttribute('title')) || '').includes('文件篮') && (await page.locator('[data-testid="go-hint"]').count()) === 1)
        await page.evaluate(() => { window.__app.toolFiles = ['/m/a.mp4'] })
        await page.waitForTimeout(400)
        ok('有文件后启用，原因提示消失', !(await btn.isDisabled()) && (await page.locator('[data-testid="go-hint"]').count()) === 0)
        await page.evaluate(() => { window.__app.toolFiles = [] })
      }
      await shot(page, `toolbox-${tag}-${scheme}`)
      await page.close()
    }
  }
  {
    const page = await open('tools', { width: 1280, height: 800 }, { lang: 'en' })
    await page.locator('.tbs .search input').fill('compress')
    await page.waitForTimeout(300)
    ok('英文界面：搜 compress 找得到', (await page.locator('.tbs .sec .item').count()) >= 1)
    await page.locator('.tbs .search input').fill('screenshot')
    await page.waitForTimeout(300)
    ok('英文界面：搜 screenshot 找得到', (await page.locator('.tbs .sec .item').count()) >= 1)
    await page.locator('.tbs .search input').fill('timeline')
    await page.waitForTimeout(300)
    ok('英文界面：搜 timeline 找得到', (await page.locator('.tbs .sec .item').count()) >= 1)
    await page.close()
  }

  ok('没有页面错误', errors.length === 0, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})()
