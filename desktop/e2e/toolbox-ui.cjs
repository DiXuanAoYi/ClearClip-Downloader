// 工具箱：分组侧栏 + 返回按钮 + 文件篮 + 预览区。假后端按命令名回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = Number(process.env.E2E_PORT) || 5199
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

const INIT = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  window.__calls = []; window.__jobs = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return 'file://' + p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return ${JSON.stringify(settings)}
        case 'get_app_info': return { version: '1.12.0', commit: 'test', debug: false, providers: [] }
        case 'lock_status': return { enabled: false, locked: false, lockedOutSecs: 0 }
        case 'list_tasks': case 'list_history': case 'batch_list': return []
        case 'inbox_counts': return { unhandled: 0, total: 0 }
        case 'media_info': return { durationMs: 60000, width: 1920, height: 1080, hasVideo: !/\\.(mp3|srt)$/.test(args.path), hasAudio: true, tags: {} }
        case 'media_job_start': window.__jobs.push(args.job); return window.__jobs.length
        case 'media_jobs': return []
        case 'list_library': return []
        case 'safebox_status': return { exists: false, unlocked: false, count: 0, totalBytes: 0, lockedOutSecs: 0 }
        case 'safebox_list': return []
        case 'tool_frame_preview': return { before: '/p/before.jpg', after: '/p/after.jpg', changed: true }
        case 'tool_trial': return { before: '/p/src.mp4', after: '/p/out.mp4', afterIsImage: false, beforeBytes: 12400000, afterBytes: 3100000, secs: 5, totalSecs: 600, note: null }
        case 'video_preview': return { before: '/p/nb.jpg', after: '/p/na.jpg', notes: ['画面已经是 1080p'], warnings: [], changed: true }
        case 'video_presets': return []
        case 'audio_fx_preview': return { before: '/p/ab.wav', after: '/p/aa.wav', notes: ['响度 -23.4 → -16.0'] }
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const open = async (files = ['/m/a.mp4', '/m/b.mov', '/m/sub.srt']) => {
    const page = await browser.newPage({ viewport: { width: 1500, height: 1000 } })
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript(INIT)
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#tools`)
    await page.waitForTimeout(1200)
    await page.evaluate((f) => {
      window.__app.toolFiles = f
    }, files)
    await page.waitForTimeout(300)
    return page
  }
  const pick = async (page, name) => {
    await page.locator('.tbs .item', { hasText: name }).first().click()
    await page.waitForTimeout(250)
  }

  // 1. 侧栏：5 个分组，工具各归各位
  {
    const page = await open()
    const heads = await page.locator('.tbs .head').allInnerTexts()
    ok('侧栏是 5 个分组（还没用过工具时没有“最近用过”）', heads.join('|') === '剪辑|压制与转换|画面与调色|声音|字幕与 AI', heads.join('|'))
    const items = await page.locator('.tbs .sec .item:not(.recent)').allInnerTexts()
    const names = items.map((t) => t.split('\n')[0])
    ok('17 个工具都在', names.length === 17, names.join(','))
    ok('压缩、转换格式、竖屏转横屏在“压制与转换”，防抖在“画面与调色”', names.indexOf('压缩视频') === 4 && names.indexOf('竖屏转横屏') === 6 && names.indexOf('防抖') === 9, names.join(','))

    // 返回按钮在搜索框左边
    const back = page.getByRole('button', { name: '返回上一页' })
    const bb = await back.boundingBox()
    const sb = await page.locator('.tbs .search').boundingBox()
    ok('返回按钮在搜索框左边', bb && sb && bb.x + bb.width <= sb.x + 1 && Math.abs(bb.y - sb.y) < 6, JSON.stringify({ bb, sb }))
    await page.evaluate(() => {
      window.__app.view = 'tools'
    })
    await page.waitForTimeout(100)
    await page.evaluate(() => {
      window.__app.toolsFrom = 'library'
    })
    await back.click()
    ok('点返回回到进工具箱之前的页面', (await page.evaluate(() => window.__app.view)) === 'library')

    // 搜索
    await page.locator('.tbs .search input').fill('字幕')
    await page.waitForTimeout(150)
    const found = (await page.locator('.tbs .sec .item:not(.recent)').allInnerTexts()).map((t) => t.split('\n')[0])
    ok('搜“字幕”只剩字幕相关的工具', found.includes('生成字幕') && found.includes('字幕校对与调整') && !found.includes('压缩视频'), found.join(','))
    await page.locator('.tbs .search input').fill('zzzz')
    await page.waitForTimeout(100)
    ok('搜不到时有提示', (await page.locator('.tbs .empty').count()) === 1)
    await page.close()
  }

  // 2. 文件篮 + 压缩（试一小段）
  {
    const page = await open()
    await pick(page, '压缩视频')
    ok('工具里没有自己的工具列表和文件选择，只有一个文件篮', (await page.locator('nav.tools').count()) === 0 && (await page.getByRole('button', { name: '选择文件…' }).count()) === 0 && (await page.locator('section.basket').count()) === 1)
    const chips = await page.locator('.basket .chip').count()
    const off = await page.locator('.basket .chip.off').count()
    ok('文件篮里 3 个文件，字幕对这个工具用不到所以淡显', chips === 3 && off === 1, `${chips}/${off}`)
    ok('提示有 1 个文件会被跳过', /有 1 个文件这个工具用不到/.test(await page.locator('.basket .foot').innerText()))
    await page.getByRole('button', { name: '开始处理' }).click()
    await page.waitForTimeout(300)
    const jobs = await page.evaluate(() => window.__jobs)
    ok('每个视频各一个压缩任务，字幕被跳过', jobs.length === 2 && jobs.every((j) => j.op === 'compress' && j.inputs.length === 1) && jobs[0].inputs[0] === '/m/a.mp4' && jobs[1].inputs[0] === '/m/b.mov', JSON.stringify(jobs))

    // 预览区：试一小段
    const pv = page.locator('section.pv')
    ok('压缩有预览区，标着“试一小段”', (await pv.count()) === 1 && /试一小段/.test(await pv.locator('.pill').innerText()))
    await pv.getByRole('button', { name: '试一下' }).click()
    await page.waitForTimeout(400)
    const trialCall = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'tool_trial').pop())
    ok('试一下用文件篮里的第一个视频、压缩参数、5 秒', trialCall && trialCall.args.path === '/m/a.mp4' && trialCall.args.op.op === 'compress' && trialCall.args.durMs === 5000, JSON.stringify(trialCall))
    const stats = await pv.locator('.stats').innerText()
    ok('显示这 5 秒的体积变化和整段粗略估算', /12\.4|11\.8/.test(stats) && /整段粗略估算/.test(stats) && /仅供参考/.test(stats), stats)
    ok('原片和处理后各一个视频（并排）', (await pv.locator('video').count()) === 2)
    await page.close()
  }

  // 3. 快速处理：5 项；旋转是实时预览
  {
    const page = await open(['/m/a.mp4'])
    await pick(page, '快速处理')
    const subs = await page.locator('.quick .el-radio-button').allInnerTexts()
    ok('快速处理里有 5 项，竖屏转横屏单独成了工具', subs.length === 5 && !subs.includes('竖屏转横屏'), subs.join(','))
    ok('裁剪时预览区显示开始和结束两帧', (await page.locator('section.pv .pill').innerText()) === '实时')
    await page.locator('.quick .el-radio-button', { hasText: '旋转' }).click()
    await page.waitForTimeout(900)
    const call = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'tool_frame_preview').pop())
    ok('旋转：自动取一帧做前后对比，不用点按钮', call && call.args.op.kind === 'rotate' && call.args.op.mode === 'cw', JSON.stringify(call))
    ok('默认是“滑动”对比，两张图都在', (await page.locator('section.pv .slide img').count()) === 2)
    await page.locator('section.pv .seg button', { hasText: '并排' }).click()
    ok('切到并排后两张图并排', (await page.locator('section.pv .sidebyside img').count()) === 2)
    // 改参数 → 自动重画
    const before = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'tool_frame_preview').length)
    await page.locator('.el-radio-button', { hasText: '水平翻转' }).click()
    await page.waitForTimeout(900)
    const after = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'tool_frame_preview').pop())
    ok('改成水平翻转，预览自动重画', after.args.op.mode === 'hflip' && (await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'tool_frame_preview').length)) > before)
    // 拼接 / 去除音轨 不需要预览
    await page.locator('.quick .el-radio-button', { hasText: '去除音轨' }).click()
    await page.waitForTimeout(300)
    ok('去除音轨不需要预览：预览区不出现', (await page.locator('section.pv').count()) === 0)
    await page.close()
  }

  // 4. 竖屏转横屏 / 视频规整 / 响度 / 转换格式
  {
    const page = await open(['/m/a.mp4'])
    await pick(page, '竖屏转横屏')
    await page.waitForTimeout(900)
    const call = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'tool_frame_preview').pop())
    ok('竖屏转横屏：实时预览，带画布尺寸', call && call.args.op.kind === 'landscape' && call.args.op.width === 1920 * 1 && call.args.op.height === 1080, JSON.stringify(call))
    await pick(page, '视频规整')
    await page.waitForTimeout(900)
    ok('视频规整：用后端已有的单帧对比，且旧的“预览对比”按钮收起了', (await page.evaluate(() => window.__calls.some((c) => c.cmd === 'video_preview'))) && (await page.getByRole('button', { name: '生成第一个文件的对比图' }).count()) === 0)
    ok('视频规整预览里有后端给的说明', /画面已经是 1080p/.test(await page.locator('section.pv').innerText()))
    await pick(page, '防抖')
    await page.waitForTimeout(300)
    ok('防抖只显示防抖，不显示规整的设置', (await page.getByRole('heading', { name: '防抖' }).count()) === 1 && (await page.getByRole('heading', { name: '视频规整' }).count()) === 0)
    await pick(page, '响度标准化')
    await page.waitForTimeout(300)
    ok('响度标准化是“试一小段”，按钮叫试听', /试一小段/.test(await page.locator('section.pv .pill').innerText()) && (await page.locator('section.pv').getByRole('button', { name: '试听一下' }).count()) === 1)
    await page.locator('section.pv').getByRole('button', { name: '试听一下' }).click()
    await page.waitForTimeout(400)
    const au = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'audio_fx_preview').pop())
    ok('试听用响度标准化的目标', au && au.args.spec.normalize === -16 && au.args.spec.reverb === null, JSON.stringify(au))
    ok('前后各一个播放器', (await page.locator('section.pv audio').count()) === 2)
    await pick(page, '转换格式')
    await page.waitForTimeout(300)
    ok('转换格式不需要预览', (await page.locator('section.pv').count()) === 0)
    await page.close()
  }

  // 5. 其他入口：字幕类工具用文件篮里的字幕；最近用过
  {
    const page = await open(['/m/a.mp4', '/m/sub.srt'])
    await pick(page, '字幕校对与调整')
    ok('字幕工具：主输入由文件篮代替，只显示一个文件选择（文件篮自己的）', (await page.getByRole('button', { name: '选择文件…' }).count()) === 0)
    await pick(page, 'AI 纠错')
    await page.waitForTimeout(250)
    ok('AI 纠错：不再有页内的“生成字幕 / AI 纠错 / 摘要”切换', (await page.locator('.modes').count()) === 0)
    const recent = await page.locator('.tbs .sec').first().locator('.head').innerText()
    ok('用过的工具出现在“最近用过”', recent === '最近用过', recent)
    await page.close()
  }

  // 6. 老入口：媒体库“用工具箱处理”
  {
    const page = await open([])
    await page.evaluate(() => {
      window.__app.goToolbox(['/m/x.mp4'], 'compress')
    })
    await page.waitForTimeout(400)
    ok('从别处带文件进来：文件进文件篮、选中对应工具', (await page.evaluate(() => window.__app.toolFiles.join())) === '/m/x.mp4' && (await page.locator('.tbs .item.on').innerText()).startsWith('压缩视频'))
    await page.evaluate(() => {
      window.__app.goToolbox(['/m/s.srt'], undefined, { tab: 'ai', aiMode: 'correct' })
    })
    await page.waitForTimeout(400)
    ok('旧的 tab / aiMode 参数能找到对应的新工具', (await page.locator('.tbs .item.on').innerText()).startsWith('AI 纠错'))
    await page.close()
  }

  // 7. 主导航：没有“保险箱”；固定 88px 图标加文字，进工具箱也不收起
  {
    const page = await browser.newPage({ viewport: { width: 1500, height: 1000 } })
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript(INIT)
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#app`)
    await page.waitForTimeout(2500)
    const labels = await page.locator('nav.side .nav').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')))
    ok('主导航里没有“保险箱”', labels.length >= 9 && !labels.includes('保险箱'), labels.join(','))
    ok('主导航固定 88px，每项都有图标和文字', Math.round((await page.locator('nav.side').boundingBox()).width) === 88 && (await page.locator('nav.side .nav svg.ico').count()) === labels.length && (await page.locator('nav.side .nav span').count()) === labels.length)
    await page.locator('nav.side .nav', { hasText: '工具箱' }).click()
    await page.waitForTimeout(600)
    ok('进工具箱后主导航不变（不再收成图标条）', Math.round((await page.locator('nav.side').boundingBox()).width) === 88 && (await page.locator('nav.side .nav span').count()) === labels.length)
    ok('当前页在导航里标出（aria-current）', (await page.locator('nav.side .nav[aria-current="page"]').innerText()).includes('工具箱'))
    await page.close()
  }

  // 8. 加密保险箱在 设置 → 安全与隐私 里，不再是单独的页面
  {
    const page = await browser.newPage({ viewport: { width: 1200, height: 1000 } })
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript(INIT)
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#security`)
    await page.waitForTimeout(1500)
    const sec = page.locator('section', { has: page.getByRole('heading', { name: '加密保险箱' }) }).first()
    ok('安全与隐私里有“加密保险箱”，直接就能创建（不用再跳到别的页面）', (await sec.getByRole('heading', { name: '创建保险箱' }).count()) === 1 && (await sec.getByPlaceholder('密码（至少 6 个字符）').count()) === 1)
    ok('同一块里有空闲自动上锁的设置', /空闲自动上锁/.test(await sec.innerText()))
    ok('没有“打开保险箱”按钮了', (await page.getByRole('button', { name: '打开保险箱' }).count()) === 0)
    await page.close()
  }

  await browser.close()
  if (errors.length) {
    fail++
    console.log('FAIL 页面报错  ←', errors.slice(0, 3).join(' | '))
  }
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})()
