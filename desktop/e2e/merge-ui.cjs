// 第二轮合并后的桌面界面：悬浮窗（拖链接下载 + 屏幕翻译控制合在一个窗）、摘录（主窗口能看）、
// 屏幕翻译历史（实时 + 截图放在一起）、工具箱 AI（翻译交给“文件翻译”页）。假后端按命令名回答。
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
settings.live.floatButton = true
settings.floatBall = true
settings.shotHistory = true

const INIT = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  const listeners = {}
  window.__calls = []
  window.__emit = (event, payload) => { for (const h of listeners[event] || []) cbs[h]?.({ event, id: 0, payload }) }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__settings = ${JSON.stringify(settings)}
  window.__excerpts = [
    { id: 'a', text: '浏览器里摘的一段', app: 'Demo 页面', note: '', created: 1700000000, url: 'https://a.test/' },
    { id: 'b', text: '写作窗里存的', app: 'winword.exe', note: '要核实', created: 1690000000 },
  ]
  window.__shot = [{ at: 1700000500000, src: 'Start Game', text: '开始游戏', saved: false }]
  window.__live = [{ at: 1700000400, mode: 'standard', exe: 'game.exe', src: 'Options', text: '选项' }]
  const term = (s) => ({ source: s, target: s + '译', note: '' })
  window.__gl = { candidates: [], sites: [], folders: ['空文件夹'], glossaries: [
    { id: 'g1', name: '原神术语', description: '', enabled: true, origin: 'user', remoteId: '', matches: [], lang: '', updated: 0, folder: '游戏/原神', terms: [term('A'), term('B'), term('C')] },
    { id: 'g2', name: '通用', description: '', enabled: true, origin: 'user', remoteId: '', matches: [], lang: '', updated: 0, folder: '', terms: [term('X')] },
  ] }
  window.__sc = { supported: true, running: false, mode: 'standard', target: null, hidden: false, paused: false, msg: '', audio: false }
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': (listeners[args.event] ||= []).push(args.handler); return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return window.__settings
        case 'save_settings': window.__saved = args.settings; window.__settings = args.settings; return window.__settings
        case 'get_app_info': return { providers: [] }
        case 'list_tasks': return []
        case 'list_orphan_parts': return []
        case 'get_after_all_done': return 'none'
        case 'sc_status': return window.__sc
        case 'sc_start': window.__sc = { ...window.__sc, running: true }; window.__emit('live://state', window.__sc); return window.__sc
        case 'sc_stop': window.__sc = { ...window.__sc, running: false }; window.__emit('live://state', window.__sc); return window.__sc
        case 'float_expand': window.__expanded = args.expanded; return null
        case 'excerpt_list': return window.__excerpts
        case 'excerpt_note': window.__excerpts.find((e) => e.id === args.id).note = args.note; return null
        case 'excerpt_remove': window.__excerpts = window.__excerpts.filter((e) => e.id !== args.id); return null
        case 'excerpt_add': window.__added = args; return { id: 'n', text: args.text, app: args.app, note: '', created: 1 }
        case 'sc_history': return window.__live
        case 'shot_hist_list': return window.__shot
        case 'shot_hist_save': window.__shot = window.__shot.map((h) => (h.at === args.at ? { ...h, saved: args.saved } : h)); return null
        case 'shot_hist_clear': window.__shotCleared = args; return null
        case 'sc_history_clear': window.__liveCleared = true; return null
        case 'ai_status': return { chatReady: true, sttReady: true }
        case 'sc_windows': case 'sc_ocr_langs': case 'app_glossary_packs': return []
        case 'experts_state': return { experts: [] }
        case 'ocr_status': return { supported: true, installed: false }
        case 'tm_stats': return [0, 0]
        case 'glossary_state': return window.__gl
        case 'glossary_folder_add': window.__gl.folders.push(args.path); return window.__gl
        case 'glossary_save': window.__glSaved = args.glossary; return args.glossary
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch()
  const errors = []
  const open = async (hash, viewport = { width: 1200, height: 900 }) => {
    const page = await browser.newPage({ viewport })
    page.on('pageerror', (e) => errors.push(`${hash}: ${e.message}`))
    await page.addInitScript(INIT)
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#${hash}`)
    await page.waitForTimeout(1500)
    return page
  }

  // 1. 悬浮窗：拖链接下载和屏幕翻译的“译”在同一个窗里
  {
    const page = await open('float', { width: 700, height: 120 })
    ok('悬浮窗里有拖链接的区域', (await page.locator('.ball .tx').innerText()).includes('拖链接到这里'))
    ok('悬浮窗里有屏幕翻译的“译”', await page.locator('.ball .tr').isVisible())
    await page.locator('.ball .tr').click()
    await page.waitForTimeout(200)
    ok('点“译”展开屏幕翻译控制，窗口变宽', (await page.evaluate(() => window.__expanded)) === true && (await page.locator('.panel').isVisible()))
    const btns = await page.locator('.panel button').allInnerTexts()
    ok('控制里有开始、模式、重选、隐藏译文', ['开始', '标准', '字幕', '漫画', '▭ 重选', '隐藏译文'].every((b) => btns.includes(b)), btns.join(','))
    await page.locator('.panel button', { hasText: '开始' }).click()
    await page.waitForTimeout(200)
    ok('点“开始”调用和主程序一样的开始，按钮变“停止”', (await page.evaluate(() => window.__calls.some((c) => c.cmd === 'sc_start'))) && (await page.locator('.panel button', { hasText: '停止' }).count()) === 1)
    await page.locator('.panel button', { hasText: '停止' }).click()
    await page.waitForTimeout(200)
    ok('点“停止”就是停止（不是切换），按钮变回“开始”', (await page.evaluate(() => window.__calls.some((c) => c.cmd === 'sc_stop'))) && (await page.locator('.panel button', { hasText: '开始' }).count()) === 1)
    await page.locator('.ball .x').click()
    await page.waitForTimeout(200)
    const saved = await page.evaluate(() => window.__saved)
    ok('关掉悬浮窗：两个开关一起关', saved && saved.floatBall === false && saved.live.floatButton === false, JSON.stringify(saved && { f: saved.floatBall, b: saved.live.floatButton }))
    await page.close()
  }
  // 1b. 只开了拖链接（屏幕翻译控制没开）：没有“译”
  {
    const page = await browser.newPage({ viewport: { width: 700, height: 120 } })
    await page.addInitScript(INIT.replace('"floatButton":true', '"floatButton":false'))
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#float`)
    await page.waitForTimeout(1200)
    ok('没开屏幕翻译控制时悬浮窗里没有“译”', (await page.locator('.ball .tr').count()) === 0)
    await page.close()
  }

  // 2. 摘录：主窗口能看、搜索、加笔记、打开网页
  {
    const page = await open('excerpts')
    const items = await page.locator('.it').count()
    ok('摘录列表显示两条', items === 2, String(items))
    ok('浏览器来的摘录显示来源和“打开网页”', (await page.locator('.it').first().innerText()).includes('Demo 页面') && (await page.locator('.it').first().locator('a', { hasText: '打开网页' }).count()) === 1)
    await page.locator('.search input').fill('写作')
    await page.waitForTimeout(200)
    ok('搜索只剩一条', (await page.locator('.it').count()) === 1)
    ok('笔记显示出来', (await page.locator('.it .note').innerText()).includes('要核实'))
    await page.locator('.search input').fill('')
    await page.locator('.it').first().getByRole('button', { name: '加笔记' }).click()
    await page.locator('.it textarea').fill('有用')
    await page.locator('.it textarea').blur()
    await page.waitForTimeout(300)
    ok('加笔记保存到后端', await page.evaluate(() => window.__calls.some((c) => c.cmd === 'excerpt_note' && c.args.note === '有用')))
    await page.close()
  }

  // 3. 屏幕翻译 → 翻译历史：实时翻译和截图翻译放在一起，截图的那条能存进摘录
  {
    const page = await open('screen')
    await page.getByRole('tab', { name: '翻译历史' }).click()
    await page.waitForTimeout(500)
    const rows = await page.locator('.hrow').allInnerTexts()
    ok('历史里同时有实时翻译和截图翻译', rows.length === 2 && rows.some((r) => r.includes('截图翻译') && r.includes('开始游戏')) && rows.some((r) => r.includes('game.exe') && r.includes('选项')), rows.join(' | '))
    await page.getByRole('radio', { name: '截图翻译' }).click({ force: true })
    await page.waitForTimeout(200)
    ok('筛选“截图翻译”只剩截图的', (await page.locator('.hrow').count()) === 1)
    await page.locator('.hrow').getByRole('button', { name: '存进摘录' }).click()
    await page.waitForTimeout(300)
    const added = await page.evaluate(() => window.__added)
    ok('截图翻译的一条存进摘录', added && added.text.includes('Start Game') && added.text.includes('开始游戏') && added.app === '截图翻译', JSON.stringify(added))
    await page.close()
  }

  // 4. 工具箱 AI：不再有“翻译字幕”“翻译文件”，改为指向“文件翻译”页
  {
    const page = await open('ai')
    const modes = await page.locator('.modes .el-radio-button').allInnerTexts()
    ok('工具箱 AI 里没有“翻译字幕”“翻译文件”', !modes.some((m) => m.includes('翻译')), modes.join(','))
    await page.locator('.tr-hint').getByRole('button', { name: '文件翻译' }).click()
    await page.waitForTimeout(200)
    ok('点“文件翻译”切到文件翻译页', (await page.evaluate(() => window.__app.view)) === 'filetr')
    await page.close()
  }

  // 5. 术语库：多层文件夹；“添加一行”插在当前选中的那一行下面，没选中就加在最下面
  {
    const page = await open('glossary')
    const rows = await page.locator('.item').allInnerTexts()
    const names = rows.map((r) => r.replace(/[▾▸]/g, '').trim().split('\n')[0].trim())
    ok('术语库按文件夹分层显示（游戏 → 原神 → 原神术语，空文件夹也在）', names.some((n) => n.includes('📁 游戏')) && names.some((n) => n.includes('📁 原神')) && names.some((n) => n.includes('📁 空文件夹')) && names.indexOf(names.find((n) => n.includes('📁 原神'))) < names.findIndex((n) => n.startsWith('原神术语')), names.join(' | '))
    const pad = await page.locator('.item', { hasText: '原神术语' }).evaluate((e) => parseInt(e.style.paddingLeft))
    ok('第二层文件夹里的术语库缩进两层', pad === 36, String(pad))
    await page.locator('.item.folder', { hasText: '游戏' }).first().locator('.fold').click()
    await page.waitForTimeout(150)
    ok('折叠“游戏”后里面的都收起来', !(await page.locator('.item', { hasText: '原神术语' }).count()))
    await page.locator('.item.folder', { hasText: '游戏' }).first().locator('.fold').click()
    await page.locator('.item', { hasText: '原神术语' }).getByRole('button', { name: '编辑' }).click()
    await page.waitForTimeout(300)
    // 没有选中的行：加在最下面
    await page.getByRole('button', { name: '添加一行' }).click()
    await page.waitForTimeout(200)
    const v1 = await page.locator('.gl-terms tbody tr').evaluateAll((trs) => trs.map((tr) => tr.querySelector('input').value))
    ok('没选中行时，新行加在最下面', JSON.stringify(v1) === JSON.stringify(['A', 'B', 'C', '']), JSON.stringify(v1))
    // 点第一行（A），再添加：插在 A 下面
    await page.locator('.gl-terms tbody tr').nth(0).locator('input').first().click()
    await page.getByRole('button', { name: '添加一行' }).click()
    await page.waitForTimeout(200)
    const v2 = await page.locator('.gl-terms tbody tr').evaluateAll((trs) => trs.map((tr) => tr.querySelector('input').value))
    ok('选中 A 后添加，新行插在 A 下面', JSON.stringify(v2) === JSON.stringify(['A', '', 'B', 'C', '']), JSON.stringify(v2))
    ok('新行成了当前行，光标在里面', await page.evaluate(() => document.activeElement?.closest('tr')?.classList.contains('current-row') === true))
    await page.close()
  }

  ok('没有页面错误', !errors.length, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
