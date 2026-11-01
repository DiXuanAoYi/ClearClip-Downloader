// 批量添加面板 + 队列按批次显示：假后端按命令名回答，测试界面的筛选、勾选、提交和批次操作。
const { chromium } = require('playwright')
const PORT = 5199
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const INIT = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  const listeners = {}
  window.__calls = []
  window.__emit = (event, payload) => { for (const h of listeners[event] || []) cbs[h]?.({ event, id: 0, payload }) }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  const settings = { batchPreview: true, batchFolderMin: 3, batchJitterMin: 2, batchJitterMax: 6, perSiteConcurrency: 2, runWindowOn: false, runWindow: { days: [], start: '23:00', end: '07:00', limitKbps: 0 } }
  const batch = { id: 'b1', name: '批量-0101-1200', source: '粘贴', createdAt: 1, opts: { name: '', dir: '', folder: true, quality: '', concurrency: 2, jitterMin: 2, jitterMax: 6, speedLimitKbps: 0, afterDone: 'notify', useWindow: false, startPaused: false, template: '' }, total: 4, queued: 1, running: 1, paused: 0, done: 1, failed: 1, pending: 0, failedLinks: [], baseDir: '/tmp/x' }
  const task = (id, status, seq) => ({ id, platform: 'p', platformName: 'P', site: 'p', mediaId: String(id), title: '视频' + id, author: '', cover: null, assetId: 'a', assetLabel: '1080p', assetKind: 'video', filePath: '/tmp/x/v' + id + '.mp4', status, received: 10, total: 100, speed: 5, error: status === 'failed' ? '网络错误' : null, errorKind: null, note: null, warning: null, resumable: true, step: null, inputs: 1, priority: 0, startAt: null, createdAt: 1, finishedAt: null, batchId: 'b1', batchSeq: seq })
  window.__tasks = [task(1, 'done', 1), task(2, 'running', 2), task(3, 'queued', 3), task(4, 'failed', 4)]
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': (listeners[args.event] ||= []).push(args.handler); return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return settings
        case 'get_app_info': return { providers: [] }
        case 'list_tasks': return window.__tasks
        case 'list_orphan_parts': return []
        case 'get_after_all_done': return 'none'
        case 'batch_extract': return (args.text.match(/https?:\\/\\/\\S+/g) || [])
        case 'batch_preview': {
          const key = args.key
          setTimeout(() => {
            args.urls.forEach((u, i) => {
              const st = u.includes('bad') ? 'bad' : u.includes('old') ? 'done' : u.includes('login') ? 'login' : u.includes('list') ? 'playlist' : 'ok'
              const entries = st === 'playlist' ? [1, 2, 3, 4, 5].map((n) => ({ id: 'e' + n, title: '集' + n, url: u + '/' + n, durationMs: null, thumbnail: null, index: n })) : []
              window.__emit('batch://preview', { key, index: i, url: u, status: st, title: st === 'bad' ? '' : '标题' + i, platform: 'p', platformName: '测试站', site: 'p', durationMs: 60000, sizeEst: 50 << 20, quality: '1080p', error: st === 'bad' ? '不支持' : null, media: st === 'ok' ? { id: 'm' + i } : null, entries })
            })
            window.__emit('batch://preview', { key, finished: true })
          }, 50)
          return null
        }
        case 'batch_preview_cancel': return null
        case 'batch_enqueue': window.__enq = args; return { id: 'b2', name: '批量-test', queued: args.items.length, alreadyDownloaded: 0, alreadyQueued: 0, pending: 0 }
        case 'batch_list': return [batch]
        case 'batch_action': window.__act = args; return null
        case 'batch_export_failed': return 'https://x/1\\n'
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch()
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } })
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(INIT)
  await page.goto(`http://127.0.0.1:${PORT}/batch-harness.html`)
  await page.waitForFunction(() => window.__app)
  await page.waitForTimeout(400)

  // —— 队列：批次卡片与分页 ——
  ok('队列里显示批次卡片', (await page.locator('.bx').count()) === 1)
  ok('批次卡片显示完成进度', (await page.locator('.bx').innerText()).includes('1/4 完成'))
  ok('任务行显示批次序号', (await page.locator('.seq').count()) >= 1)
  await page.locator('.tabs button', { hasText: '失败' }).click()
  await page.waitForTimeout(150)
  ok('“失败”分页只剩失败的任务', (await page.locator('.task').count()) === 1)
  await page.locator('.tabs button', { hasText: '全部' }).click()
  await page.locator('.bb button', { hasText: '只重试失败项' }).click()
  await page.waitForTimeout(150)
  ok('“只重试失败项”调用批次操作 retry', await page.evaluate(() => window.__act?.what === 'retry' && window.__act?.id === 'b1'))

  // —— 批量添加面板 ——
  await page.evaluate(() => window.__app.openBatch(['https://a.com/1', 'https://a.com/old', 'https://b.com/bad', 'https://c.com/login', 'https://d.com/list', 'https://a.com/2'], '粘贴'))
  await page.waitForSelector('.bd .row')
  await page.waitForTimeout(400)
  ok('面板列出 6 条', (await page.locator('.bd .row').count()) === 6)
  const chips = await page.locator('.bd .chips').innerText()
  ok('筛选标签带数量', chips.includes('全部 6') && chips.includes('已下载过 1') && chips.includes('需登录 1') && chips.includes('不支持 1'), chips)
  ok('可下载的默认勾选、已下载过的不勾选', (await page.locator('.bd .row input[type=checkbox]:checked').count()) === 3)
  ok('6 条默认放进批次文件夹', await page.locator('.bd .kv', { hasText: '批次文件夹' }).locator('.is-checked').count() === 1)
  ok('列表项显示展开范围', (await page.locator('.bd .rng').count()) === 1)
  await page.locator('.bd .chips button', { hasText: '需登录' }).click()
  ok('点“需登录”只显示 1 条', (await page.locator('.bd .row').count()) === 1)
  await page.locator('.bd .chips button', { hasText: '全部' }).click()
  const foot = await page.locator('.bd .foot').innerText()
  ok('底部显示已勾选条数', foot.includes('已勾选 7 条'), foot) // 2 个普通 + 5 项列表
  await page.locator('.bd .foot button', { hasText: '加入队列并开始' }).click()
  await page.waitForTimeout(300)
  const enq = await page.evaluate(() => window.__enq)
  ok('提交的条目：普通 2 条带解析结果，列表展开 5 条待解析', enq && enq.items.length === 7 && enq.items.filter((i) => i.media).length === 2, JSON.stringify(enq && enq.items.length))
  ok('提交带批次选项与来源', enq && enq.opts.folder === true && enq.opts.concurrency === 2 && enq.source === '粘贴')
  ok('提交后切到队列页', await page.evaluate(() => window.__app.view === 'queue'))

  // 设置里关闭预览：直接整批加入，不打开面板
  await page.evaluate(() => { window.__enq = null; window.__app.settings.batchPreview = false; window.__app.openBatch(['https://a.com/9', 'https://a.com/10'], '剪贴板') })
  await page.waitForTimeout(300)
  ok('关闭预览时直接加入（不打开面板）', await page.evaluate(() => window.__enq && window.__enq.items.length === 2 && window.__enq.items.every((i) => !i.media)))

  ok('页面没有脚本错误', errors.length === 0, errors.join('; '))
  await page.screenshot({ path: '/tmp/batch-ui.png' })
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
