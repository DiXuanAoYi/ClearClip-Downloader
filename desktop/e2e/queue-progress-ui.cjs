// 下载队列的进度更新：后端每秒发很多次 tasks://progress，界面把同一帧内的多次进度合并成一次写入。
// 这里验证合并之后显示、排序、速度统计、最终状态都和逐条应用一致。假后端按命令名回答。
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
const MB = 1024 * 1024
const mk = (id, over) => ({
  id, platform: 'x', platformName: '示例', site: 'x', mediaId: 'm' + id, title: '任务' + id, author: '', cover: null,
  assetId: 'a', assetLabel: '', assetKind: 'video', filePath: `/d/任务${id}.mp4`, status: 'queued', received: 0, total: 100 * MB,
  speed: 0, error: null, errorKind: null, note: null, warning: null, resumable: true, step: 'download', inputs: 1,
  priority: 0, startAt: null, createdAt: 1700000000 + id, finishedAt: null, ...over,
})
const TASKS = [
  mk(1, { status: 'running', received: 10 * MB, speed: 1 * MB, priority: 3 }),
  mk(2, { status: 'running', received: 20 * MB, speed: 2 * MB, priority: 2 }),
  mk(3, { status: 'queued', priority: 1 }),
  mk(4, { status: 'paused', received: 5 * MB }),
  mk(5, { status: 'failed', error: '网络断了', errorKind: 'network' }),
  mk(6, { status: 'done', received: 100 * MB, finishedAt: 1700001000 }),
]

const INIT = `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  const listeners = {}
  window.__emit = (event, payload) => { for (const h of listeners[event] || []) cbs[h]?.({ event, id: 0, payload }) }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__settings = ${JSON.stringify(settings)}
  window.__tasks = ${JSON.stringify(TASKS)}
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      switch (cmd) {
        case 'plugin:event|listen': (listeners[args.event] ||= []).push(args.handler); return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return window.__settings
        case 'get_app_info': return { providers: [] }
        case 'list_tasks': return window.__tasks
        case 'list_orphan_parts': return []
        case 'batch_list': return []
        case 'get_after_all_done': return 'none'
        default: return null
      }
    },
  }
})()
`

const rowNames = (page) => page.locator('.task .name').allInnerTexts().then((a) => a.map((s) => s.replace(/\s+/g, ' ').trim()))
const stat = (page, label) => page.locator('.dstat', { hasText: label }).locator('b').innerText()

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const page = await browser.newPage({ viewport: { width: 1200, height: 900 } })
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(INIT)
  await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#queue`)
  await page.waitForTimeout(1200)
  await page.evaluate(() => window.__queue.start())
  await page.waitForTimeout(500)

  // 1. 初始：进行中（优先级高的在前）→ 等待 → 暂停 / 失败 → 完成
  let names = await rowNames(page)
  ok('初始排序：进行中、等待、暂停、失败、完成', names.map((s) => s.match(/任务(\d)/)?.[1]).join('') === '123456', names.join(' | '))
  ok('初始统计：2 进行中，总速度 3.0 MB/s', (await stat(page, '进行中')) === '2' && /3(\.0)?\s*MB\/s/.test(await stat(page, '总速度')), await stat(page, '总速度'))

  // 2. 同一帧内对同一任务发很多条进度：显示最后一条
  await page.evaluate((MB) => {
    for (let i = 1; i <= 80; i++) window.__emit('tasks://progress', { ...window.__tasks[0], received: (10 + i) * MB * 0.5, speed: 5 * MB * (i === 80 ? 1 : 0.1) })
    window.__emit('tasks://progress', { ...window.__tasks[1], received: 60 * MB, speed: 2 * MB })
  }, MB)
  await page.waitForTimeout(400)
  let detail1 = await page.locator('.task', { hasText: '任务1' }).locator('.detail').first().innerText()
  ok('合并后显示最后一条进度（45 MB / 100 MB，5 MB/s）', /45(\.0)?\s*MB.*100(\.0)?\s*MB/.test(detail1) && /5(\.0)?\s*MB\/s/.test(detail1), detail1)
  ok('总速度 = 各进行中任务速度之和（5 + 2 = 7 MB/s）', /7(\.0)?\s*MB\/s/.test(await stat(page, '总速度')), await stat(page, '总速度'))
  const w2 = await page.locator('.task', { hasText: '任务2' }).locator('.dprog i').first().evaluate((e) => e.style.width)
  ok('另一个任务的进度条同步更新（60%）', w2 === '60%', w2)
  names = await rowNames(page)
  ok('只更新进度时顺序不变', names.map((s) => s.match(/任务(\d)/)?.[1]).join('') === '123456', names.join(' | '))

  // 3. 进度里带来的状态变化不丢：任务 2 完成、任务 3 开始，排序跟着变
  await page.evaluate((MB) => {
    window.__emit('tasks://progress', { ...window.__tasks[1], status: 'running', received: 90 * MB })
    window.__emit('tasks://progress', { ...window.__tasks[1], status: 'done', received: 100 * MB, speed: 0, finishedAt: 1700002000 })
    window.__emit('tasks://progress', { ...window.__tasks[2], status: 'running', received: 1 * MB, speed: 3 * MB })
  }, MB)
  await page.waitForTimeout(400)
  names = await rowNames(page)
  const order = names.map((s) => s.match(/任务(\d)/)?.[1]).join('')
  ok('排序：进行中 1、3，暂停 4，失败 5，完成 2（最近）、6', order === '134526', order)
  ok('统计：2 进行中、1 暂停、1 失败、2 完成', (await stat(page, '进行中')) === '2' && (await stat(page, '暂停')) === '1' && (await stat(page, '失败')) === '1' && (await stat(page, '完成')) === '2', [await stat(page, '进行中'), await stat(page, '暂停'), await stat(page, '失败'), await stat(page, '完成')].join(','))
  ok('总速度 8 MB/s（5 + 3）', /8(\.0)?\s*MB\/s/.test(await stat(page, '总速度')), await stat(page, '总速度'))

  // 4. 完整列表比还没写入的进度新：同一时刻先发进度、再发完整列表，以列表为准
  await page.evaluate((MB) => {
    const list = window.__tasks.map((t) => ({ ...t }))
    list[0].status = 'paused'
    list[0].speed = 0
    list[0].received = 33 * MB
    window.__emit('tasks://progress', { ...window.__tasks[0], status: 'running', received: 99 * MB, speed: 9 * MB })
    window.__emit('tasks://updated', list)
  }, MB)
  await page.waitForTimeout(400)
  detail1 = await page.locator('.task', { hasText: '任务1' }).locator('.detail').first().innerText()
  ok('完整列表之后，旧进度不会把状态盖回去（任务 1 仍是已暂停）', /已下载\s*33(\.0)?\s*MB/.test(detail1), detail1)

  // 5. 窗口在后台时没有动画帧：定时器兜底，状态照样更新
  await page.evaluate(() => { window.requestAnimationFrame = () => 0 })
  await page.evaluate((MB) => window.__emit('tasks://progress', { ...window.__tasks[1], status: 'done', received: 100 * MB, speed: 0, note: '后台更新' }), MB)
  await page.waitForTimeout(600)
  ok('没有动画帧时也会更新（定时器兜底）', (await page.locator('.task', { hasText: '任务2' }).locator('.detail').first().innerText()).includes('后台更新'))

  ok('没有页面报错', errors.length === 0, errors.join('; '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
