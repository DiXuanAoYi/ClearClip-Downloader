// 文件翻译页：任务卡片按状态给按钮、“⋯”菜单、批量清理、开始前确认、阅读器页栏和“只看未译”。假后端按命令名回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = 5199
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))

const INIT = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  const listeners = {}
  window.__calls = []
  window.__emit = (event, payload) => { for (const h of listeners[event] || []) cbs[h]?.({ event, id: 0, payload }) }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  const O = { bilingual: true, target: '简体中文', pdfMode: 'inplace', ocr: 'vision', preview: true, pages: '', skipHeaders: true, skipCode: true, skipRefs: false }
  const rec = (path, state, extra = {}) => ({ path, title: path.split('/').pop(), target: '简体中文', opts: O, at: 100, output: null, last: '', error: '', state, pagesTotal: 12, pagesDone: 5, failed: 0, missing: false, ...extra })
  window.__recents = [
    rec('/d/paper-b.pdf', 'stopped', { at: 90 }),
    rec('/d/chapter3.pdf', 'failed', { at: 80, error: '第 9 页：模型没有返回译文（2 段）', failed: 2 }),
    rec('/d/manual.epub', 'generated', { at: 70, output: '/d/manual.zh.epub', pagesDone: 12 }),
    rec('/d/gone.pdf', 'stopped', { at: 60, missing: true }),
  ]
  window.__jobs = [{ id: 7, kind: 'filetr', title: 'Attention.pdf', input: '/d/Attention.pdf', status: 'running', percent: 38, note: '已缓存 112 段', params: O, error: null, output: null }]
  window.__view = { v: 2, layout: false, totalPages: 4, pages: [
    { src: ['Intro A', 'Intro B'], trans: ['引言甲', '引言乙'], page: 0, done: true, skip: [] },
    { src: ['<r1>Body </r1><r2>A</r2>', 'Body B'], trans: ['<r1>正文</r1><r2>甲</r2>', null], page: 1, done: true, skip: [], qa: { 0: ['数字可能不一致：原文有 322，译文里没找到'] } },
    { src: ['Later A'], trans: [null], page: 2, done: false, skip: [] },
    { src: ['Later B'], trans: [null], page: 3, done: false, skip: [] },
  ] }
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': (listeners[args.event] ||= []).push(args.handler); return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return ${JSON.stringify(settings)}
        case 'media_jobs': return window.__jobs
        case 'filetr_recent': return window.__recents
        case 'filetr_view': return window.__view
        case 'filetr_preview': return window.__view
        case 'filetr_plan': return { pages: 12, selectedPages: 12, segments: 200, chars: 31000, cached: 0, skipped: 14, requests: 21, scanned: false, layout: true, donePages: 0 }
        case 'filetr_start': return 9
        case 'filetr_recent_remove': window.__recents = window.__recents.filter((r) => !(r.path === args.path)); return null
        case 'media_job_remove': window.__jobs = window.__jobs.filter((j) => j.id !== args.id); return null
        case 'experts_state': return { experts: [] }
        case 'ocr_status': return { supported: true, installed: false }
        case 'pdfx_status': return { supported: true, installed: true, path: '', fontOk: true, note: '已安装' }
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const page = await browser.newPage({ viewport: { width: 1280, height: 860 } })
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(INIT)
  await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#filetr`)
  await page.waitForSelector('.job')
  await page.waitForTimeout(500)
  const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd)
  const card = (t) => page.locator('.job', { hasText: t })

  // 1. 各状态的按钮
  ok('进行中：有“停止”，没有“继续”', (await card('Attention').getByRole('button', { name: '停止' }).count()) === 1 && (await card('Attention').getByRole('button', { name: '继续' }).count()) === 0)
  ok('已停止：主按钮“继续”', (await card('paper-b').getByRole('button', { name: '继续' }).count()) === 1)
  ok('失败：主按钮“重试失败的”，写清原因', (await card('chapter3').getByRole('button', { name: '重试失败的' }).count()) === 1 && (await card('chapter3').innerText()).includes('第 9 页'))
  ok('已生成：有“打开”“所在文件夹”', (await card('manual').getByRole('button', { name: '打开' }).count()) === 1 && (await card('manual').getByRole('button', { name: '所在文件夹' }).count()) === 1)
  ok('找不到源文件：给“重新选择文件”', (await card('gone').getByRole('button', { name: '重新选择文件' }).count()) === 1)
  ok('每张卡片（排队的除外）都有“⋯”', (await page.locator('.job [aria-label="更多"]').count()) === 5)

  // 2. 筛选
  await page.locator('.flt span', { hasText: '失败' }).click()
  ok('筛选“失败”只剩失败的', (await page.locator('.job').count()) === 1)
  await page.locator('.flt span', { hasText: '进行中' }).click()
  ok('筛选“进行中”只剩在跑的', (await page.locator('.job').count()) === 1 && (await card('Attention').count()) === 1)
  await page.locator('.flt span', { hasText: '全部' }).click()

  // 3. 继续 = 用记着的设置再开始（已译的页走缓存）
  await card('paper-b').getByRole('button', { name: '继续' }).click()
  await page.waitForTimeout(300)
  const st = await calls('filetr_start')
  ok('点“继续”按原来的设置开始', st.length === 1 && st[0].args.path === '/d/paper-b.pdf' && st[0].args.opts.target === '简体中文', JSON.stringify(st))

  // 4. 打开 / 所在文件夹
  await card('manual').getByRole('button', { name: '打开' }).click()
  await card('manual').getByRole('button', { name: '所在文件夹' }).click()
  await page.waitForTimeout(200)
  ok('“打开”打开成品，“所在文件夹”定位成品', (await calls('open_file'))[0]?.args.path === '/d/manual.zh.epub' && (await calls('reveal_file'))[0]?.args.path === '/d/manual.zh.epub')

  // 5. ⋯ 菜单：从列表里移除
  await card('chapter3').locator('[aria-label="更多"]').click()
  await page.waitForTimeout(200)
  const items = await page.locator('.el-dropdown-menu:visible .el-dropdown-menu__item').allInnerTexts()
  ok('“⋯”菜单项齐全', ['在阅读器里打开', '打开原文件所在位置', '换设置重新生成…', '重新翻译（清掉这份译文）', '复制出错信息', '从列表里移除', '删除译文缓存…'].every((x) => items.includes(x)), items.join('|'))
  await page.locator('.el-dropdown-menu:visible .el-dropdown-menu__item', { hasText: '从列表里移除' }).click()
  await page.waitForTimeout(300)
  ok('移除：只去掉记录（不删缓存）', (await calls('filetr_recent_remove')).length === 1 && (await calls('filetr_cache_delete')).length === 0 && (await card('chapter3').count()) === 0)

  // 6. 批量：清除已结束的，进行中的留着
  await page.getByRole('button', { name: '批量操作' }).click()
  await page.locator('.el-dropdown-menu:visible .el-dropdown-menu__item', { hasText: '清除已结束的' }).click()
  await page.waitForTimeout(400)
  ok('清除已结束的：只剩进行中的', (await page.locator('.job').count()) === 1 && (await card('Attention').count()) === 1, String(await page.locator('.job').count()))

  // 7. 阅读器：页栏状态点、只看未译
  await card('Attention').click()
  await page.waitForTimeout(500)
  ok('页栏有 4 页', (await page.locator('.rail .thumb').count()) === 4)
  const dots = await page.locator('.rail .thumb i').evaluateAll((els) => els.map((e) => getComputedStyle(e).backgroundColor))
  ok('第 2 页（有没译的段）标红点，第 1 页绿', dots[0] !== dots[1], dots.join('|'))
  await page.locator('.rail .only span', { hasText: '未译' }).click()
  await page.waitForTimeout(150)
  ok('“只看未译”只列有没译的页（第 2~4 页）', (await page.locator('.rail .thumb').count()) === 3)
  await page.locator('.rail .only span', { hasText: '全部' }).click()
  await page.locator('.rail .thumb').nth(1).click()
  await page.waitForTimeout(200)
  const body = await page.locator('.ftr .body').innerText()
  ok('格式标记不显示在阅读器里', body.includes('Body A') && body.includes('正文甲') && !/<\/?r\d/.test(body), body)
  ok('译后检查的提示显示在那一段下面和底栏', body.includes('数字可能不一致') && (await page.locator('.st .qa').innerText()).includes('1 段需要检查'))
  await page.getByPlaceholder('搜索原文 / 译文').fill('Later')
  await page.waitForTimeout(150)
  ok('搜索：命中的页在页栏标出来', (await page.locator('.rail .thumb.hit').count()) === 2)
  await page.getByPlaceholder('搜索原文 / 译文').fill('')

  // 8. 开始前确认
  await page.evaluate(() => window.__app.goFiletr(['/d/new.pdf']))
  await page.waitForSelector('.el-dialog')
  await page.waitForTimeout(500)
  const dtxt = await page.locator('.el-dialog').innerText()
  ok('确认框显示文件概况和预计请求数', dtxt.includes('new.pdf') && dtxt.includes('12 页') && dtxt.includes('21') && dtxt.includes('跳过 14 段'), dtxt)
  await page.getByPlaceholder('全部。如 3-12, 15 或 5-').fill('3-12, 15')
  await page.locator('.el-dialog .el-checkbox', { hasText: '参考文献' }).click()
  await page.waitForTimeout(400)
  const plans = await calls('filetr_plan')
  ok('改页范围 / 排除项后重新估算', plans.length >= 2 && plans[plans.length - 1].args.opts.pages === '3-12, 15' && plans[plans.length - 1].args.opts.skipRefs === true, JSON.stringify(plans[plans.length - 1]))
  await page.getByPlaceholder('全部。如 3-12, 15 或 5-').fill('abc')
  await page.waitForTimeout(150)
  ok('页范围写错：不能开始并提示', (await page.getByRole('button', { name: '开始翻译' }).isDisabled()) && (await page.locator('.el-dialog').innerText()).includes('没看懂'))
  await page.getByPlaceholder('全部。如 3-12, 15 或 5-').fill('3-12, 15')
  await page.waitForTimeout(150)
  await page.getByRole('button', { name: '开始翻译' }).click()
  await page.waitForTimeout(400)
  const st2 = (await calls('filetr_start')).pop()
  ok('开始：带上页范围和排除项', st2.args.path === '/d/new.pdf' && st2.args.opts.pages === '3-12, 15' && st2.args.opts.skipRefs === true && st2.args.opts.skipHeaders === true, JSON.stringify(st2))

  ok('页面没有脚本错误', errors.length === 0, errors.join(';'))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
