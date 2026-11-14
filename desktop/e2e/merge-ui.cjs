// 第二轮合并后的桌面界面：悬浮窗（拖链接下载 + 屏幕翻译控制合在一个窗）、摘录（主窗口能看）、
// 屏幕翻译历史（实时 + 截图放在一起）、工具箱 AI（翻译交给“文件翻译”页）。假后端按命令名回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = 5299
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
    { id: 'g1', name: '原神术语', description: '', enabled: true, origin: 'user', remoteId: '', matches: [], lang: '', updated: 0, folder: '游戏/原神', terms: [{ ...term('A'), note: '上级说明', children: [{ ...term('A1'), children: [term('A1a')] }] }, term('B'), term('C')] },
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
        case 'float_fit': window.__fit = args.width; return null
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
        case 'media_job_start': window.__job = args.job; return 5
        case 'sc_audio_transcript': return [{ startMs: 1000, endMs: 3000, src: 'Hello everyone', text: '大家好' }, { startMs: 65000, endMs: 67000, src: 'Next topic', text: '下一个话题' }]
        case 'sc_audio_summary': return '## Overview\\n本次会议做了开场。'
        case 'sc_set_mode': window.__sc = { ...window.__sc, mode: args.mode }; return window.__sc
        case 'sc_audio_start': window.__audioStarted = true; return { ...window.__sc, audio: true }
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
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
    ok('点“译”展开屏幕翻译控制，窗口变宽', (await page.evaluate(() => window.__fit)) > 500 && (await page.locator('.panel').isVisible()))
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

  // 2b. 屏幕翻译 → 语音模式：不用框选屏幕，有本次记录、导出和一键总结
  {
    const page = await open('screen')
    ok('四种模式里有“语音”', (await page.locator('.modes .mode').allInnerTexts()).some((t) => t.includes('语音')))
    await page.locator('.modes .mode', { hasText: '语音' }).click()
    await page.waitForTimeout(400)
    const head = await page.locator('.scr .card h3').allInnerTexts()
    ok('语音模式：第一步是“听什么”，不再要求框选屏幕', head.some((t) => t.includes('听什么')) && !head.some((t) => t.includes('翻译哪里')), head.join('|'))
    ok('可选电脑声音 / 麦克风', (await page.locator('.scr .row label.el-radio-button').allInnerTexts()).some((t) => t.includes('麦克风')))
    const rows = await page.locator('.vrow').allInnerTexts()
    ok('本次记录列出每一句（带时间）', rows.length === 2 && rows[0].includes('00:01') && rows[1].includes('01:05') && rows[1].includes('下一个话题'), rows.join(' | '))
    await page.getByRole('button', { name: '一键总结', exact: true }).click()
    await page.waitForTimeout(300)
    ok('一键总结显示模型给的总结', (await page.locator('.vsum').innerText()).includes('本次会议做了开场'))
    await page.getByRole('button', { name: /^开始听/ }).first().click()
    await page.waitForTimeout(200)
    ok('点“开始听”：直接开始，不需要目标窗口', await page.evaluate(() => window.__audioStarted === true))
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
    // 分层术语：每一层一种底色、缩进；收起 / 展开；Tab / Shift+Tab；加同级 / 下级；移动；菜单；删除；撤回；搜索；拖动；保存成树
    const rowsOf = () => page.locator('.tt .trow').evaluateAll((els) => els.map((e) => `${e.getAttribute('aria-level')}:${e.querySelector('input[data-col=source]').value}`))
    const srcInput = (src) => page.locator('.tt .trow input[data-col=source]').evaluateAll((els, s) => els.findIndex((e) => e.value === s), src).then((i) => page.locator('.tt .trow input[data-col=source]').nth(i))
    ok('术语按层显示：A › A1 › A1a，B、C 在第一层', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '1:C']), JSON.stringify(await rowsOf()))
    const bgs = await page.locator('.tt .trow').evaluateAll((els) => els.slice(0, 3).map((e) => getComputedStyle(e).backgroundColor))
    ok('每一层的底色不同（第一层最深）', new Set(bgs).size === 3, bgs.join(' '))
    ok('工具条显示总数和层数', /共 5 条 · 3 层/.test(await page.locator('.tt .cnt').innerText()))
    // 收起 A
    await page.locator('.tt .trow').nth(0).locator('.caret').click()
    await page.waitForTimeout(100)
    ok('收起 A：只剩第一层，A 上标“2 条下级”', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '1:B', '1:C']) && /2 条下级/.test(await page.locator('.tt .trow').nth(0).innerText()), JSON.stringify(await rowsOf()))
    await page.getByRole('button', { name: '全部展开' }).click()
    await page.waitForTimeout(100)
    ok('全部展开', (await rowsOf()).length === 5)
    // 当前路径
    await (await srcInput('A1a')).click()
    await page.waitForTimeout(100)
    ok('底栏显示当前这条的完整路径和译文路径', /A\s*›\s*A1\s*›\s*A1a/.test(await page.locator('.efoot .cur').innerText()) && /A译 › A1译 › A1a译/.test(await page.locator('.efoot .cur').innerText()), await page.locator('.efoot .cur').innerText())
    // Tab：B 变成 A 的下级；Shift+Tab 回来
    await (await srcInput('B')).click()
    await page.keyboard.press('Tab')
    await page.waitForTimeout(150)
    ok('Tab：B 变成 A 的下级（排在 A 的下级最后）', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '2:B', '1:C']), JSON.stringify(await rowsOf()))
    await page.keyboard.press('Shift+Tab')
    await page.waitForTimeout(150)
    ok('Shift+Tab：B 升回第一层', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '1:C']), JSON.stringify(await rowsOf()))
    // Enter 加同级、打字、空行退格删掉
    await page.keyboard.press('Enter')
    await page.waitForTimeout(150)
    await page.keyboard.type('D')
    ok('Enter：在 B 后面加同级，光标在新行', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '1:D', '1:C']), JSON.stringify(await rowsOf()))
    await page.keyboard.press('Enter')
    await page.waitForTimeout(150)
    await page.keyboard.press('Backspace')
    await page.waitForTimeout(150)
    ok('空行上退格：删掉这一行', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '1:D', '1:C']), JSON.stringify(await rowsOf()))
    // Alt+↑ 上移
    await (await srcInput('C')).click()
    await page.keyboard.press('Alt+ArrowUp')
    await page.waitForTimeout(150)
    ok('Alt+↑：C 和 D 对调', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '1:C', '1:D']), JSON.stringify(await rowsOf()))
    // 添加下级按钮
    await (await srcInput('B')).click()
    await page.getByRole('button', { name: '添加下级' }).first().click()
    await page.waitForTimeout(150)
    await page.keyboard.type('B1')
    ok('添加下级：B 下面多一条 B1', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '2:B1', '1:C', '1:D']), JSON.stringify(await rowsOf()))
    // 右键菜单 → 删除（有下级要确认）→ 只删 A → 撤回
    await page.locator('.tt .trow').nth(0).click({ button: 'right' })
    await page.waitForTimeout(150)
    const items = await page.locator('.tt-menu .mi').allInnerTexts()
    ok('右键菜单：加同级 / 下级、升降层、上下移、分组、复制路径、删除', ['添加同级', '添加下级', '升一层', '放进上一条下面', '上移', '下移', '只作分组', '复制路径', '删除'].every((w) => items.some((t) => t.startsWith(w))), items.join(' | '))
    await page.locator('.tt-menu .mi', { hasText: '删除' }).click()
    await page.waitForTimeout(200)
    ok('删除有下级的术语先问一次', /它下面还有 2 条术语/.test(await page.locator('.el-dialog .del').innerText()))
    await page.locator('.el-dialog .del .opt').first().click()
    await page.waitForTimeout(200)
    ok('只删 A：下级升一层', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A1', '2:A1a', '1:B', '2:B1', '1:C', '1:D']), JSON.stringify(await rowsOf()))
    await page.locator('.tt .pz-link', { hasText: '撤回' }).click()
    await page.waitForTimeout(150)
    ok('撤回：A 回来了', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '2:B1', '1:C', '1:D']), JSON.stringify(await rowsOf()))
    // 只作分组
    await page.locator('.tt .trow').nth(5).click({ button: 'right' })
    await page.locator('.tt-menu .mi', { hasText: '只作分组' }).click()
    await page.waitForTimeout(100)
    ok('只作分组：标上“分组”', /分组/.test(await page.locator('.tt .trow').nth(5).innerText()))
    // 拖动：D 放进 C 下面
    await page.locator('.tt .trow').nth(6).hover()
    await page.locator('.tt .trow').nth(6).locator('.grip').dragTo(page.locator('.tt .trow').nth(5), { targetPosition: { x: 300, y: 19 } })
    await page.waitForTimeout(200)
    ok('拖动：D 放进 C 下面成为下级', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a', '1:B', '2:B1', '1:C', '2:D']), JSON.stringify(await rowsOf()))
    // 搜索：命中的连同上级一起显示
    await page.locator('.tt .q input').fill('a1a')
    await page.waitForTimeout(150)
    ok('搜索：命中的 A1a 连同上级 A、A1 一起显示，上级变灰', JSON.stringify(await rowsOf()) === JSON.stringify(['1:A', '2:A1', '3:A1a']) && (await page.locator('.tt .trow.dim').count()) === 2, JSON.stringify(await rowsOf()))
    await page.locator('.tt .q input').fill('')
    await page.waitForTimeout(100)
    // 保存：存成树
    await page.getByRole('button', { name: '保存' }).click()
    await page.waitForTimeout(300)
    const saved = await page.evaluate(() => window.__glSaved)
    const t = saved?.terms
    ok('保存成树：A › A1 › A1a、B › B1、C（分组）› D', t && t[0].source === 'A' && t[0].children[0].children[0].source === 'A1a' && t[1].children[0].source === 'B1' && t[2].group === true && t[2].children[0].source === 'D' && t.length === 3, JSON.stringify(t))
    await page.close()
  }

  ok('没有页面错误', !errors.length, errors.join(' | '))
  // 工具箱：剪辑里也能做的几样合成“快速处理”，可以一键送进剪辑
  {
    const page = await open('video')
    const nav = await page.locator('nav.tools .tool').allInnerTexts()
    ok('工具列表里有“快速处理”，没有单独的裁剪 / 拼接 / 倍速 / 旋转 / 去除音轨', nav.includes('快速处理') && !['裁剪片段', '拼接', '倍速', '旋转 / 翻转', '去除音轨'].some((t) => nav.includes(t)), nav.join(','))
    ok('压缩、转换格式、GIF、截图、响度、音效还在', ['压缩视频', '转换格式', '转 GIF', '截图', '响度标准化', '音效 / 混响', '竖屏转横屏'].every((t) => nav.includes(t)), nav.join(','))
    ok('不是快速处理时没有“在剪辑里打开”', (await page.getByRole('button', { name: '在剪辑里打开' }).count()) === 0)
    await page.locator('nav.tools .tool', { hasText: '快速处理' }).click()
    await page.waitForTimeout(200)
    const subs = await page.locator('.quick .el-radio-button').allInnerTexts()
    ok('快速处理里有 5 项（竖屏转横屏单独成了工具）', subs.length === 5 && subs.includes('倍速') && !subs.includes('竖屏转横屏'), subs.join(','))
    await page.locator('.quick .el-radio-button', { hasText: '倍速' }).click()
    await page.waitForTimeout(150)
    await page.getByRole('button', { name: '开始处理' }).click()
    await page.waitForTimeout(300)
    const job = await page.evaluate(() => window.__job)
    ok('选倍速开始处理：发出倍速任务', job && job.op === 'speed' && job.inputs[0] === '/m/clip.mp4', JSON.stringify(job))
    await page.getByRole('button', { name: '在剪辑里打开' }).click()
    await page.waitForTimeout(200)
    const req = await page.evaluate(() => window.__app.toolboxRequest)
    ok('“在剪辑里打开”把文件交给剪辑', req && req.tab === 'edit' && req.paths[0] === '/m/clip.mp4', JSON.stringify(req))
    // 切到别的工具再回来，停在上次选的那一项
    await page.locator('nav.tools .tool', { hasText: '压缩视频' }).click()
    await page.locator('nav.tools .tool', { hasText: '快速处理' }).click()
    await page.waitForTimeout(150)
    ok('再回到快速处理时停在上次的“倍速”', (await page.locator('.quick .el-radio-button.is-active').innerText()) === '倍速')
    await page.close()
  }

  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
