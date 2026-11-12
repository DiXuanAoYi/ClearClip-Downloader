// 视频胶囊：每个视频上只有一个清影入口（右下角），下载、播放增强、字幕层、视频摘要都挂在里面。
// 以前是各自定位的小按钮：左上角的播放工具条和“笔记 (n)”、右下角的字幕胶囊和音效按钮两两重叠。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const BASE = ['content/ui.js', 'content/vdock.js', 'content/video-logic.js', 'content/video-control.js', 'content/float-button.js', 'content/audio-logic.js', 'content/audio-chain.js', 'content/audio-fx.js']
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const html = `<!doctype html><title>Video page</title><body style="margin:0"><video id="v" width="640" height="360" style="display:block;margin:40px;background:#000"></video><p>text</p>`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 800 } })
  await ctx.route('https://video.test/**', (route) => route.fulfill({ contentType: 'text/html', body: html }))
  const page = await ctx.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  await page.addInitScript(() => {
    const orig = Element.prototype.attachShadow
    Element.prototype.attachShadow = function (init) {
      const r = orig.call(this, { ...init, mode: 'open' })
      window.__roots = (window.__roots || []).concat(r)
      return r
    }
    window.__calls = []
    const store = { features: { videoMaster: true, speed: true, seek: true, videoTools: true, floatButton: true, vsum: true, subLayer: true } }
    window.__store = store
    const rt = {
      sendMessage: async (m) => {
        window.__calls.push(m)
        if (m.type === 'cc-sniffed') return { ok: true, items: [{ url: 'https://cdn.test/a.m3u8', label: '1080p 视频流' }] }
        if (m.type === 'cc-vsum-open' || m.type === 'cc-sublayer-open' || m.type === 'cc-send-page') return { ok: true }
        return { ok: false, error: 'unknown ' + m.type }
      },
    }
    const listeners = []
    window.chrome = {
      runtime: rt,
      storage: {
        local: {
          get: async (k) => {
            const keys = Array.isArray(k) ? k : typeof k === 'string' ? [k] : Object.keys(store)
            return Object.fromEntries(keys.filter((x) => x in store).map((x) => [x, store[x]]))
          },
          set: async (v) => {
            Object.assign(store, JSON.parse(JSON.stringify(v)))
            const ch = Object.fromEntries(Object.keys(v).map((k) => [k, { newValue: v[k] }]))
            for (const f of listeners) f(ch, 'local')
          },
        },
        onChanged: { addListener: (f) => listeners.push(f) },
      },
    }
  })
  await page.goto('https://video.test/')
  for (const f of BASE) await page.addScriptTag({ path: path.join(EXT, f) })
  await page.waitForTimeout(300)

  const docks = () => page.evaluate(() => (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.dock')]).filter((d) => d.style.display !== 'none' && d.style.opacity !== '0'))
  const dockInfo = () =>
    page.evaluate(() => {
      const all = (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.dock')])
      const d = all.find((x) => x.style.display !== 'none')
      if (!d) return null
      const b = d.getBoundingClientRect()
      return { n: all.length, text: d.innerText.replace(/\s+/g, ' '), opacity: d.style.opacity, left: b.left, top: b.top, right: b.right, bottom: b.bottom }
    })
  const hover = async (x = 400, y = 200) => {
    await page.mouse.move(x - 30, y - 20)
    await page.waitForTimeout(150)
    await page.mouse.move(x, y)
    await page.waitForTimeout(300)
  }

  // 1. 鼠标移到视频上：只有一个胶囊，在视频右下角；没有以前那些各自的小按钮
  await hover()
  const d = await dockInfo()
  const v = await page.evaluate(() => document.getElementById('v').getBoundingClientRect().toJSON())
  ok('鼠标移到视频上出现一个胶囊', d && d.n === 1 && d.opacity === '1', JSON.stringify(d))
  ok('胶囊在视频右下角（控制栏上方）', d && d.right <= v.right && d.right > v.right - 40 && d.bottom < v.bottom && d.bottom > v.bottom - 110, JSON.stringify({ d, v }))
  ok('胶囊上有“下载”和倍速', d && /下载/.test(d.text) && /1×|1\.0×/.test(d.text), d && d.text)
  const legacy = await page.evaluate(() => (window.__roots || []).some((r) => r.querySelector('.vbar, .fb, .apill, .pill')))
  ok('没有以前各自的小按钮（播放工具条、下载按钮、音效按钮、笔记按钮）', !legacy)

  // 2. 倍速：胶囊上点 › 加速，显示跟着变
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dock .b')]).find((b) => b.textContent === '›').click())
  await page.waitForTimeout(200)
  const rate = await page.evaluate(() => document.getElementById('v').playbackRate)
  ok('胶囊上点 › 加速', rate > 1, String(rate))
  ok('胶囊上的倍速跟着变', /1\.25×|1\.1×|1\.5×/.test((await dockInfo()).text), (await dockInfo()).text)

  // 3. ⋮ 菜单：播放工具、下载、打开摘要 / 字幕层都在
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dock .dots')).find(Boolean).click())
  await page.waitForTimeout(200)
  const items = await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dmenu .it')]).map((b) => b.textContent))
  for (const want of ['实时音效…', '用清影下载…', '截图', '画中画', '循环 A–B', '播放增强设置…', '视频摘要 / 总结', '双语字幕层']) ok(`菜单里有“${want}”`, items.some((t) => t.startsWith(want)), items.join(' | '))
  const menuBox = await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dmenu')).find(Boolean).getBoundingClientRect().toJSON())
  const capBox = await dockInfo()
  ok('菜单在胶囊上方', menuBox.bottom <= capBox.top + 1, JSON.stringify({ menuBox, capBox }))

  // 4. 菜单里点“播放增强设置…”：面板在胶囊上方，胶囊不消失
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dmenu .it')]).find((b) => b.textContent.startsWith('播放增强设置')).click())
  await page.waitForTimeout(200)
  const pnl = await page.evaluate(() => window.__roots.map((r) => r.querySelector('.pnl')).find(Boolean)?.getBoundingClientRect().toJSON())
  ok('播放增强面板打开，在胶囊上方', pnl && pnl.bottom <= capBox.top + 1, JSON.stringify(pnl))
  await page.mouse.move(1150, 750)
  await page.waitForTimeout(2200)
  ok('面板开着时，鼠标离开视频胶囊也不隐藏', (await dockInfo()).opacity === '1')
  await page.keyboard.press('Escape')
  await page.waitForTimeout(2200)
  ok('关掉面板、鼠标离开后胶囊淡出', (await dockInfo()).opacity === '0')

  // 5. 下载：胶囊上点“下载”，菜单列出检测到的视频流
  await hover()
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dock .b')]).find((b) => b.textContent === '下载').click())
  await page.waitForTimeout(300)
  const dl = await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.menu .it')]).map((b) => b.textContent))
  ok('点“下载”列出发送页面和检测到的视频流', dl.includes('发送此页面（由清影选清晰度）') && dl.includes('1080p 视频流'), dl.join(' | '))
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.menu .it')]).find((b) => b.textContent === '1080p 视频流').click())
  await page.waitForTimeout(200)
  ok('选一项就发给清影', await page.evaluate(() => window.__calls.some((c) => c.type === 'cc-send-media' && c.url === 'https://cdn.test/a.m3u8')))

  // 6. 菜单里“视频摘要 / 总结”：交给后台打开
  await hover()
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dock .dots')).find(Boolean).click())
  await page.waitForTimeout(200)
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dmenu .it')]).find((b) => b.textContent.startsWith('视频摘要')).click())
  await page.waitForTimeout(200)
  ok('菜单里点“视频摘要”让后台打开面板', await page.evaluate(() => window.__calls.some((c) => c.type === 'cc-vsum-open')))

  // 7. 本站关掉播放增强：胶囊上不再有倍速，下载还在（还是同一个胶囊）
  await page.evaluate(() => window.chrome.storage.local.set({ siteRules: { off: { video: ['video.test'] } } }))
  await page.waitForTimeout(300)
  await hover(420, 220)
  const d2 = await dockInfo()
  ok('本站关掉播放增强：胶囊上只剩下载', d2 && /下载/.test(d2.text) && !/×/.test(d2.text) && d2.n === 1, JSON.stringify(d2))

  ok('没有页面错误', !errors.length, errors.join(' | '))

  // 8. 默认设置（只有实时音效，没开“显示音效状态”）：视频上什么都不出现，和以前一样
  {
    const p2 = await ctx.newPage()
    await p2.addInitScript(() => {
      const orig = Element.prototype.attachShadow
      Element.prototype.attachShadow = function (init) {
        const r = orig.call(this, { ...init, mode: 'open' })
        window.__roots = (window.__roots || []).concat(r)
        return r
      }
      const store = {}
      window.chrome = { runtime: { sendMessage: async () => ({ ok: false }) }, storage: { local: { get: async () => ({}), set: async (v) => Object.assign(store, v) }, onChanged: { addListener() {} } } }
    })
    await p2.goto('https://video.test/')
    for (const f of ['content/ui.js', 'content/vdock.js', 'content/audio-logic.js', 'content/audio-chain.js', 'content/audio-fx.js']) await p2.addScriptTag({ path: path.join(EXT, f) })
    await p2.mouse.move(370, 180)
    await p2.waitForTimeout(200)
    await p2.mouse.move(400, 200)
    await p2.waitForTimeout(400)
    const shown = await p2.evaluate(() => (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.dock')]).some((d) => d.style.display !== 'none' && d.style.opacity === '1'))
    ok('默认设置下视频上不出现胶囊', !shown)
    await p2.close()
  }

  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
