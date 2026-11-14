// 媒体库筛选弹层、空状态统一（订阅 / 生词本 / 直播 / 媒体库）、屏幕翻译首屏、写作窗一层 tab + CcSelect。
// 假后端按命令名回答；SHOTS=目录 时顺便按浅 / 深色、1280 / 900 宽截图（不入库）。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = Number(process.env.E2E_PORT || process.env.PORT || 5199)
const SHOTS = process.env.SHOTS || ''
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))
const item = (id, title, extra = {}) => ({ id, platform: 'bilibili', mediaId: 'm' + id, assetId: 'video', title, author: 'a', cover: null, path: '/d/' + id + '.mp4', size: 7e7, finishedAt: 1.7e9, exists: true, kind: 'video', source: 'download', sourceUrl: 'https://x/' + id, platformName: 'B站', coverPath: null, favorite: false, rating: 0, tags: [], durationMs: 60000, ...extra })

const INIT = (o) => `
(() => {
  let cbId = 1, evId = 1
  window.__calls = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  const S = ${JSON.stringify({ ...settings, theme: o.theme, subscriptionsEnabled: true })}
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    transformCallback() { return cbId++ },
    unregisterCallback() {},
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      if (cmd === 'plugin:event|listen') return evId++
      if (cmd === 'get_settings') return S
      if (cmd === 'list_library') { if (${o.slow ? 'true' : 'false'}) await new Promise((r) => setTimeout(r, 4000)); return ${JSON.stringify(o.files || [])} }
      if (cmd === 'library_platforms') return [{ platform: 'bilibili', name: 'B站', count: 3 }]
      if (cmd === 'sc_status') return { supported: true, running: false, mode: 'standard', target: ${JSON.stringify(o.target || null)}, hidden: false, paused: false, msg: '', audio: false }
      if (cmd === 'vocab_stats') return { total: 0, due: 0, fresh: 0, learned: 0, today: 0 }
      if (cmd === 'write_settings') return { hotkey: 'Ctrl+Alt+J', badge: true, pausedUntil: 0, offApps: [], route: 'local_first', candidates: 1, style: '', pin: true, autoClose: false, win: [0, 0, 0, 0], lastAction: 'rewrite', extras: [], relations: {} }
      if (cmd === 'write_take') return { text: '下周评审会提前到周三下午，报价今天发你。', fromSelection: true, app: 'WeChat.exe', canReplace: true, note: '' }
      if (cmd === 'write_personas') return []
      if (cmd === 'videocache_stats') return { videos: 0, bytes: 0 }
      if (cmd === 'sc_history' || cmd === 'sc_audio_transcript' || cmd === 'sc_ocr_langs') return []
      if (/list|rooms|tags|subs|history|vocab|windows|excerpts|pack|glossar|experts|voice|langs/.test(cmd)) return []
      return null
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const open = async (hash, o = {}) => {
    const page = await browser.newPage({ viewport: { width: o.w || 1280, height: o.h || 800 } })
    page.on('pageerror', (e) => errors.push(hash + ': ' + e.message))
    await page.addInitScript(INIT({ theme: 'light', ...o }))
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#${hash}`)
    if (o.dark) await page.evaluate(() => document.documentElement.classList.add('dark'))
    await page.waitForTimeout(900)
    return page
  }
  const shot = async (page, name) => {
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, name + '.png') })
  }
  const overflowX = (page) => page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1)
  const files = [item(1, '【4K 延时】上海 · 夜未央', { favorite: true, tags: ['延时'] }), item(2, 'Lo-fi 学习歌单', { kind: 'audio' }), item(3, '春分 | 城市里的第一场雨', { platform: 'x', platformName: 'X' })]

  for (const dark of [false, true]) {
    for (const w of [1280, 900]) {
      const tag = `${dark ? 'dark' : 'light'}-${w}`
      // 媒体库：有数据 + 筛选弹层
      {
        const page = await open('library', { w, dark, files })
        ok(`[${tag}] 筛选条只有一个“筛选”按钮（没有裸的下拉）`, (await page.locator('.filters .fbtn').count()) === 1 && (await page.locator('.filters .el-select').count()) === 0)
        if (!dark && w === 1280) {
          const btn = page.locator('.fbtn')
          ok('筛选按钮是带读屏名称的原生按钮，可聚焦', (await btn.getAttribute('aria-label')) === '筛选' && (await btn.evaluate((e) => e.tagName)) === 'BUTTON')
          await btn.focus()
          await page.keyboard.press('Enter')
          await page.waitForTimeout(300)
          ok('回车展开弹层，里面有平台 / 类型 / 排序 / 两个开关', (await page.locator('.fpanel').isVisible()) && (await page.locator('.fpanel .cc-select').count()) >= 4 && (await page.locator('.fpanel .el-switch').count()) === 2)
          await page.locator('.fpanel .cc-seg .seg-item', { hasText: '视频' }).click()
          await page.waitForTimeout(400)
          ok('选“视频”后按钮出现数字 1，下面出现可移除的小标签', (await page.locator('.fnum').innerText()) === '1' && (await page.locator('.chips .chip').allInnerTexts()).join('|').includes('类型：视频'))
          ok('列表请求带上 kind=video', await page.evaluate(() => window.__calls.some((c) => c.cmd === 'list_library' && c.args.filter.kind === 'video')))
          await shot(page, `library-popover-${tag}`)
          await page.locator('.fpanel').getByRole('button', { name: '重置' }).click()
          await page.waitForTimeout(300)
          ok('重置后数字消失', (await page.locator('.fnum').count()) === 0)
        }
        await shot(page, `library-${tag}`)
        ok(`[${tag}] 媒体库无横向溢出`, !(await overflowX(page)))
        await page.close()
      }
      // 媒体库：空态（主操作“去解析链接”）和加载骨架
      {
        const page = await open('library', { w, dark })
        ok(`[${tag}] 空态：“还没下载过文件” + 去解析链接`, (await page.locator('.cc-empty h4').innerText()) === '还没下载过文件' && (await page.locator('.cc-empty .cc-btn, .cc-empty button').first().innerText()).includes('去解析链接'))
        await shot(page, `library-empty-${tag}`)
        await page.close()
        const p2 = await open('library', { w, dark, slow: true })
        ok(`[${tag}] 加载中是骨架屏`, (await p2.locator('.skel .cc-sk').count()) > 0)
        await shot(p2, `library-loading-${tag}`)
        await p2.close()
      }
      // 空状态：订阅 / 生词 / 直播
      {
        const page = await open('subs', { w, dark })
        ok(`[${tag}] 订阅空态：添加订阅`, (await page.locator('.cc-empty h4').first().innerText()) === '还没有订阅' && (await page.locator('.cc-empty').first().innerText()).includes('添加订阅'))
        await shot(page, `subs-empty-${tag}`)
        await page.close()
        const v = await open('vocab', { w, dark })
        ok(`[${tag}] 生词本空态：没有表头也没有 0 统计卡`, (await v.locator('.cc-empty h4').innerText()) === '还没有生词' && (await v.locator('.th').count()) === 0 && (await v.locator('.stat-row').count()) === 0)
        await shot(v, `vocab-empty-${tag}`)
        await v.close()
        const l = await open('live', { w, dark })
        ok(`[${tag}] 直播空态用 CcEmpty`, (await l.locator('.cc-empty h4').first().innerText()) === '没有正在录制的直播')
        await shot(l, `live-empty-${tag}`)
        await l.close()
      }
      // 屏幕翻译首屏
      {
        const page = await open('screen', { w, dark })
        ok(`[${tag}] 屏幕翻译：无目标时只有“选择目标”一个主按钮，开始翻译禁用`, (await page.locator('.scr .cc-btn.primary, .scr button.primary').count()) <= 1 && (await page.getByRole('button', { name: /^选择目标/ }).count()) === 1 && (await page.getByRole('button', { name: /^开始翻译/ }).isDisabled()))
        ok(`[${tag}] 模式卡紧凑：4 张一行说明`, (await page.locator('.modes .mode').count()) === 4 && (await page.locator('.mode').first().evaluate((e) => e.getBoundingClientRect().height)) < 80)
        ok(`[${tag}] 选项默认折叠`, !(await page.locator('#sc-opts').isVisible()))
        await shot(page, `screen-${tag}`)
        ok(`[${tag}] 屏幕翻译无横向溢出`, !(await overflowX(page)))
        await page.close()
      }
    }
    // 屏幕翻译：已选目标后主按钮换成“开始翻译”
    {
      const page = await open('screen', { dark, target: { kind: 'window', rect: [0, 0, 800, 600], hwnd: 1, exe: 'chrome.exe', title: 'Chrome' } })
      ok('已选目标：按钮变“重新选择”，开始翻译可点', (await page.getByRole('button', { name: /^重新选择/ }).count()) === 1 && !(await page.getByRole('button', { name: /^开始翻译/ }).isDisabled()))
      await page.getByRole('button', { name: '展开' }).click()
      ok('点展开后出现刷新速度 / 术语库 / 专家', (await page.locator('#sc-opts').isVisible()) && (await page.locator('#sc-opts .drow').count()) === 3)
      await shot(page, `screen-target-${dark ? 'dark' : 'light'}`)
      await page.close()
    }
    // 写作窗：一层 tab + CcSegmented，CcSelect
    {
      const page = await open('write', { w: 520, h: 900, dark })
      await page.locator('.tab', { hasText: '改写润色' }).click()
      await page.waitForTimeout(300)
      ok('写作窗没有原生 <select>', (await page.locator('select').count()) === 0)
      ok('顶部一层 tab 共 5 个', (await page.locator('.tabs .tab').count()) === 5)
      ok('“怎么改”是 CcSegmented（radiogroup）', (await page.locator('.seg .cc-seg[role=radiogroup] .seg-item').count()) === 6)
      await page.locator('.cc-select').first().click()
      await page.waitForTimeout(300)
      ok('场景下拉是 CcSelect，展开后有“小红书”等选项', (await page.locator('.el-select-dropdown__item', { hasText: '小红书' }).count()) >= 1)
      await shot(page, `write-${dark ? 'dark' : 'light'}`)
      await page.keyboard.press('Escape')
      await page.close()
    }
  }
  ok('页面没有未捕获错误', errors.length === 0, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})()
