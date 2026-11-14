// 写作浮窗冒烟测试：真实 Chromium + 模拟的扩展接口 / 清影后端。
import { chromium } from 'playwright'
import fs from 'node:fs'
import path from 'node:path'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const ext = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const read = (f) => fs.readFileSync(path.join(ext, f), 'utf8')
// 注入的文件和顺序取自 files-manifest.js（唯一来源）
const WRITE_FILES = createRequire(import.meta.url)('../ext-files.cjs').group('write')

const html = `<!doctype html><meta charset=utf-8><body style="margin:0">
<textarea id=ta style="position:absolute;left:100px;top:100px;width:420px;height:120px">王经理你好，下周的交付可能要推迟两天。</textarea>
<input id=pw type=password style="position:absolute;left:100px;top:300px;width:300px;height:36px">
<div id=ce contenteditable=true style="position:absolute;left:100px;top:400px;width:420px;height:100px;border:1px solid #888">我们将于下周发步新产品</div>
</body>`

const mock = () => {
  // 让关闭的 Shadow DOM 可被测试访问
  const orig = Element.prototype.attachShadow
  Element.prototype.attachShadow = function (o) {
    const r = orig.call(this, { ...o, mode: 'open' })
    return r
  }
  const store = {}
  const jobs = new Map()
  window.__calls = []
  const handle = async (m) => {
    window.__calls.push(m)
    if (m.type === 'cc-ai-status') return { ok: true, ready: true, writing: true, localModel: true }
    if (m.type === 'cc-ai-run') {
      const id = String(jobs.size + 1)
      jobs.set(id, m.req)
      return { ok: true, id }
    }
    if (m.type === 'cc-ai-job') {
      const req = jobs.get(m.id)
      if (req.kind === 'stats') return { ok: true, state: 'done', result: { facts: [{ text: '2022 年中国人口约 14.12 亿', source: '世界银行', code: 'SP.POP.TOTL', url: 'https://data.worldbank.org/indicator/SP.POP.TOTL', value: 1412175000 }], note: '', failed: [] } }
      if (req.kind === 'grammar') return { ok: true, state: 'done', result: { issues: [{ from: '发步', to: '发布', start: 6, end: 8, type: 'typo', reason: '错别字' }], corrected: '' } }
      return { ok: true, state: 'done', result: { candidates: [{ text: req.text.replace('你好', '您好').replace('推迟', '延后'), guard: { warn: false, missing: [], ratio: 1 } }] } }
    }
    return { ok: true }
  }
  const listeners = []
  window.chrome = {
    runtime: {
      sendMessage: (m) => handle(m),
      onMessage: { addListener: (f) => listeners.push(f) },
      lastError: null,
    },
    storage: {
      local: {
        get: async (k) => (typeof k === 'string' ? { [k]: store[k] } : { ...store }),
        set: async (o) => Object.assign(store, o),
        remove: async (k) => delete store[k],
      },
      onChanged: { addListener() {} },
    },
  }
  window.__store = store
  window.__fire = (m) => listeners.forEach((f) => f(m))
}

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 900, height: 700 } })
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
await page.addInitScript(mock)
await page.route('http://t.test/**', (r) => r.fulfill({ contentType: 'text/html', body: html }))
await page.goto('http://t.test/')
for (const f of WRITE_FILES) await page.addScriptTag({ content: read(f) })

const shadow = (sel) => page.locator(`div[style*="2147483647"] >> ${sel}`)

// 1. 聚焦文本框 → 出现浮标，带字数
await page.focus('#ta')
await page.waitForSelector('div[style*="2147483647"] >> .badge')
assert.match(await shadow('.badge').innerText(), /\d+ 字/)

// 2. 密码框不显示
await page.focus('#pw')
await page.waitForTimeout(400)
assert.equal(await shadow('.badge').isVisible(), false, '密码框不应出现浮标')

// 3. 回到文本框，选中前半句，点浮标 → 面板，处理范围默认“选中”
await page.focus('#ta')
await page.evaluate(() => document.querySelector('#ta').setSelectionRange(0, 9))
await page.click('#ta') // 点击会改选区，重设
await page.evaluate(() => document.querySelector('#ta').setSelectionRange(0, 9))
await shadow('.badge').click()
await page.waitForSelector('div[style*="2147483647"] >> .panel')
const txt = await shadow('.panel').innerText()
for (const w of ['改写它', '语法检查', '翻译', '加入统计数据', '使其礼貌', '电子邮件', '+ 添加格式', '+ 添加语气', '自动']) assert.ok(txt.includes(w), `面板缺少：${w}`)

// 4. 选“使其礼貌”+ 正式 → 生成 → 出现差异
await shadow('.panel >> button:text-is("使其礼貌")').click()
await shadow('.panel >> button:text-is("正式")').click()
await shadow('.panel >> button:text-is("生成")').click()
await page.waitForSelector('div[style*="2147483647"] >> .hk')
const req = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-ai-run').req)
assert.equal(req.kind, 'write')
assert.equal(req.write.action, 'polite')
assert.equal(req.write.formality, 'formal')
assert.equal(req.text, '王经理你好，下周的')

// 5. 放弃一处改动，替换选中 → 文本框里只替换了选中部分
const hunks = await shadow('.hk').count()
assert.ok(hunks >= 1)
await shadow('.panel >> button:text-is("替换选中")').click()
const v = await page.inputValue('#ta')
assert.equal(v, '王经理您好，下周的交付可能要推迟两天。', `只替换选中部分：${v}`)

// 6. 历史记录写入
await page.waitForTimeout(200)
const hist = await page.evaluate(() => window.__store.writeHistory)
assert.ok(Array.isArray(hist) && hist.length === 1)

// 7. 语法检查（富文本 contenteditable，全文）
await page.click('#ce')
await page.keyboard.press('Control+End')
await page.evaluate(() => window.__fire({ type: 'cc-write-open' }))
await page.waitForSelector('div[style*="2147483647"] >> .panel')
await shadow('.panel >> button:text-is("语法检查")').click()
await shadow('.panel >> button:text-is("检查")').click()
await page.waitForSelector('div[style*="2147483647"] >> .card')
assert.ok((await shadow('.panel').innerText()).includes('发步'))
await shadow('.panel >> button:text-is("应用所选建议")').click()
assert.equal((await page.innerText('#ce')).trim(), '我们将于下周发布新产品')

// 8. Esc 关闭
await page.focus('#ta')
await shadow('.badge').click()
await page.waitForSelector('div[style*="2147483647"] >> .panel')
await page.keyboard.press('Escape')
await page.waitForTimeout(200)
assert.equal(await shadow('.panel').count(), 0)

// 9. 加入统计数据：先找数据，勾选，再写入
await page.focus('#ta')
await shadow('.badge').click()
await page.waitForSelector('div[style*="2147483647"] >> .panel')
await shadow('.panel >> button:text-is("加入统计数据")').click()
await shadow('.panel >> button:text-is("查找数据")').click()
await page.waitForSelector('div[style*="2147483647"] >> input[type=checkbox]')
assert.ok((await shadow('.panel').innerText()).includes('世界银行'))
await shadow('.panel >> button:text-is("写入文中")').click()
await page.waitForSelector('div[style*="2147483647"] >> .box')
const last = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').pop().req)
assert.equal(last.kind, 'write')
assert.equal(last.write.action, 'stats')
assert.equal(last.write.facts.length, 1)
assert.equal(last.write.facts[0].code, 'SP.POP.TOTL')

// 10. 面板底下不能多出一行“null”（Element.append(null) 会把 null 当文字），额外要求输入框能打字
await page.goto('http://t.test/')
for (const f of WRITE_FILES) await page.addScriptTag({ content: read(f) })
await page.focus('#ta')
await shadow('.badge').click()
await page.waitForSelector('div[style*="2147483647"] >> .panel')
assert.ok(!/\bnull\b/.test(await shadow('.panel').innerText()), '面板里出现了 null')
const extra = shadow('.panel >> input[type=text]').first()
await extra.click()
await page.keyboard.type('对方是客户')
assert.equal(await extra.inputValue(), '对方是客户')

// 11. 网页的模态对话框 / 焦点陷阱里，面板也能点、能打字（面板要放进同一个对话框）
const dialogs = {
  'http://d.test/modal': `<!doctype html><meta charset=utf-8><body><dialog id=dlg style="width:500px;height:300px"><textarea id=ta style="width:420px;height:120px">王经理你好</textarea></dialog><script>document.getElementById('dlg').showModal()</script></body>`,
  'http://d.test/trap': `<!doctype html><meta charset=utf-8><body><div id=box role=dialog aria-modal=true style="position:absolute;left:50px;top:50px;width:520px;height:300px"><textarea id=ta style="width:420px;height:120px">王经理你好</textarea></div><script>const box=document.getElementById('box');document.addEventListener('focusin',(e)=>{ if(!box.contains(e.target)) setTimeout(()=>document.getElementById('ta').focus(),0) },true)</script></body>`,
}
for (const [url, body] of Object.entries(dialogs)) {
  await page.route(url, (r) => r.fulfill({ contentType: 'text/html', body }))
  await page.goto(url)
  for (const f of WRITE_FILES) await page.addScriptTag({ content: read(f) })
  await page.evaluate(() => document.activeElement?.blur())
  await page.focus('#ta')
  await shadow('.badge').click()
  await page.waitForSelector('div[style*="2147483647"] >> .panel')
  const box = shadow('.panel >> input[type=text]').first()
  await box.click()
  await page.waitForTimeout(400)
  await page.keyboard.type('客户')
  assert.equal(await box.inputValue(), '客户', `${url}：额外要求输入框没打上字`)
  assert.equal(await box.evaluate((e) => e.getRootNode().activeElement === e), true, `${url}：焦点被网页抢走了`)
}

assert.deepEqual(errors, [])
await browser.close()
console.log('writing-smoke ok')
