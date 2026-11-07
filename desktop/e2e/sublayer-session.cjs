// 视频会话：摘要面板和字幕层共用字幕和译文；字幕层查词浮窗和划词工具条共用一张词卡。
// 在真实浏览器里加载内容脚本，后台消息用假的代替。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const VSUM = ['content/ui.js', 'vsum-logic.js', 'trans-logic.js', 'content/vsum-source.js', 'content/vsession.js', 'content/vsum-style.js', 'content/vsum-transcript.js', 'content/vsum-notes.js', 'content/vsum-comments.js', 'content/vsum-vocab.js', 'content/vsum-mindmap.js', 'content/vsum-more.js', 'content/vsum.js']
const SUB = ['content/ui.js', 'vsum-logic.js', 'sublayer-logic.js', 'learn-logic.js', 'content/vsum-source.js', 'content/vsession.js', 'content/wordcard.js', 'content/sublayer.js']
const SEL = ['content/ui.js', 'selbar-logic.js', 'content/wordcard.js', 'content/selbar.js']
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const YT = 'https://www.youtube.com/watch?v=abc123XYZ_-'
const tracks = [{ baseUrl: 'https://www.youtube.com/api/timedtext?v=abc123XYZ_-&lang=en', languageCode: 'en', name: { simpleText: 'English' } }]
const ytHtml = `<!doctype html><title>Demo video - YouTube</title><body><video width="640" height="360"></video><p id="txt">The resilient garden survives winter.</p>
<script>var ytInitialPlayerResponse={"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":${JSON.stringify(tracks)},"audioTracks":[]}},"videoDetails":{"title":"Demo","lengthSeconds":"125","shortDescription":""}};</script>`
const json3 = { events: [{ tStartMs: 0, dDurationMs: 4000, segs: [{ utf8: 'Welcome to the demo.' }] }, { tStartMs: 41000, dDurationMs: 4000, segs: [{ utf8: 'Rust is fast and safe.' }] }, { tStartMs: 95000, dDurationMs: 3000, segs: [{ utf8: 'Thanks for watching.' }] }] }

;(async () => {
  const browser = await chromium.launch()
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } })
  let timedtext = 0
  await ctx.route('https://www.youtube.com/**', (route) => {
    const u = route.request().url()
    if (u.includes('/api/timedtext')) {
      timedtext++
      return route.fulfill({ contentType: 'application/json', body: JSON.stringify(json3) })
    }
    return route.fulfill({ contentType: 'text/html', body: ytHtml })
  })

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
    const store = {}
    window.__store = store
    const jobs = {}
    let next = 1
    const rt = {
      sendMessage: async (m) => {
        window.__calls.push(m)
        if (m.type === 'cc-app-lang') return { ok: true, lang: 'zh' }
        if (m.type === 'cc-vocab') {
          if (m.op === 'add') window.__added = m.word
          return { ok: true, words: [], stats: {} }
        }
        if (m.type === 'cc-ai-status') return { ok: true, ready: true, model: 'm', lang: '中文', transcribe: true, subtitles: true, maxChars: 600000, promptVersion: 'p1' }
        if (m.type === 'cc-ai-run') {
          const id = next++
          jobs[id] = m.req
          return { ok: true, id }
        }
        if (m.type === 'cc-ai-job') {
          const req = jobs[m.id] || {}
          const result = {
            translate: { lines: (req.lines || []).map((l) => ({ t: l.t, text: '译:' + l.text })), failed: 0 },
            lookup: { lemma: req.text === 'resilient' ? 'resilient' : String(req.text).toLowerCase(), phonetic: '/x/', pos: 'adj.', meaning: '有韧性的', context_meaning: '这里指能挺过去', level: 'CET6', examples: [{ text: 'A resilient plant.', tr: '一株顽强的植物。' }] },
            summary: { summary: '摘要', points: [], keywords: [], chapters: [] },
          }[req.kind]
          return { ok: true, state: 'done', done: 1, total: 1, note: '', result }
        }
        if (m.type === 'cc-ai-cancel' || m.type === 'cc-note' || m.type === 'cc-sublayer-open' || m.type === 'cc-videocache') return { ok: true }
        return { ok: false, error: 'unknown ' + m.type }
      },
    }
    window.chrome = { runtime: rt, storage: { local: { get: async (k) => (typeof k === 'string' ? (k in store ? { [k]: store[k] } : {}) : {}), set: async (v) => Object.assign(store, JSON.parse(JSON.stringify(v))), remove: async () => {} }, onChanged: { addListener() {} } } }
  })
  await page.goto(YT)
  const inject = async (files) => {
    for (const f of files) await page.addScriptTag({ path: path.join(EXT, f) })
  }
  const inRoots = (fn, arg) => page.evaluate(fn, arg)

  // 1. 摘要面板读到文稿 → 放进视频会话
  await inject(VSUM)
  await page.waitForFunction(() => globalThis.ccVSession?.current().lines.length === 3, null, { timeout: 8000 })
  ok('摘要读到的文稿放进了视频会话', true)
  ok('摘要要求打开字幕层', await page.evaluate(() => window.__calls.some((c) => c.type === 'cc-sublayer-open')))
  const before = timedtext

  // 2. 字幕层打开：从会话里拿文稿，不再读一遍字幕；译文走会话（字幕翻译任务，t 是序号）
  await inject(SUB)
  await page.waitForFunction(() => (window.__roots || []).some((r) => /译:Welcome/.test(r.querySelector('.cap')?.textContent || '')), null, { timeout: 8000 }).catch(() => {})
  ok('字幕层没有再读一遍字幕', timedtext === before, `timedtext ${before} → ${timedtext}`)
  const capText = await inRoots(() => window.__roots.map((r) => r.querySelector('.cap')).find(Boolean)?.textContent || '')
  ok('字幕层显示了会话里的译文', /译:Welcome to the demo\./.test(capText), capText)
  const trReqs = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run' && c.req.kind === 'translate').map((c) => c.req))
  ok('字幕层的翻译是一次字幕翻译任务（不是网页翻译）', trReqs.length === 1 && trReqs[0].lines.map((l) => l.t).join() === '0,1,2', JSON.stringify(trReqs))
  ok('没有再走网页翻译（cc-tr）', !(await page.evaluate(() => window.__calls.some((c) => c.type === 'cc-tr'))))

  // 3. 摘要面板里看译文：直接用字幕层翻好的，不再提交翻译
  const shown = await page.evaluate(() => {
    const r = window.__roots.find((x) => x.querySelector('.panel'))
    const b = [...r.querySelectorAll('button')].find((x) => x.textContent.trim() === '文稿')
    b?.click()
    return !!b
  })
  await page.waitForTimeout(400)
  const panelHasTr = await page.evaluate(() => {
    const r = window.__roots.find((x) => x.querySelector('.panel'))
    const s = [...r.querySelectorAll('select')].find((x) => x.options.length === 3 && [...x.options].some((o) => o.value === 'both'))
    if (!s) return 'no select'
    s.value = 'both'
    s.dispatchEvent(new Event('change', { bubbles: true }))
    return 'ok'
  })
  await page.waitForTimeout(600)
  const panelText = await page.evaluate(() => window.__roots.map((r) => r.querySelector('.panel')).find(Boolean)?.innerText || '')
  const trReqs2 = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run' && c.req.kind === 'translate').length)
  ok('摘要面板的双语文稿用的是同一份译文', shown && panelHasTr === 'ok' && /译:Rust is fast and safe\./.test(panelText), panelHasTr + ' ' + panelText.slice(0, 300))
  ok('摘要面板没有再提交一次翻译', trReqs2 === 1, String(trReqs2))

  // 4. 会话里换了字幕（比如摘要里选了别的字幕轨）：字幕层跟着换
  await page.evaluate(() => globalThis.ccVSession.setLines([{ t: 0, text: 'Another track line.' }], 'English (auto)', 'vsum'))
  await page.waitForFunction(() => (window.__roots || []).some((r) => /译:Another track line/.test(r.querySelector('.cap')?.textContent || '')), null, { timeout: 5000 }).catch(() => {})
  const cap2 = await inRoots(() => window.__roots.map((r) => r.querySelector('.cap')).find(Boolean)?.textContent || '')
  ok('会话换了字幕，字幕层跟着换并重新翻译', /Another track line/.test(cap2) && /译:Another/.test(cap2), cap2)

  // 5. 字幕层查词：共用词卡（释义、本句意思、本句、例句、加入生词本）
  await page.evaluate(() => {
    const w = window.__roots.map((r) => r.querySelector('.cap .w')).find(Boolean)
    w.click()
  })
  await page.waitForFunction(() => (window.__roots || []).some((r) => r.querySelector('.bub.open .ms')), null, { timeout: 5000 }).catch(() => {})
  const bub = await inRoots(() => {
    const b = window.__roots.map((r) => r.querySelector('.bub')).find(Boolean)
    return { open: b.classList.contains('open'), card: b.classList.contains('wcard'), text: b.innerText, actions: b.querySelectorAll('.row2 button').length }
  })
  ok('字幕层查词浮窗是共用词卡', bub.open && bub.card && /有韧性的/.test(bub.text) && /在这句话里/.test(bub.text) && /本句/.test(bub.text) && /例句/.test(bub.text), JSON.stringify(bub))
  ok('字幕层自己的按钮还在（跳到这句 / 原声 / 朗读 / 复制 / 录原声）', bub.actions === 5, String(bub.actions))
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.bub .heart')).find(Boolean).click())
  await page.waitForTimeout(300)
  const added = await page.evaluate(() => window.__added)
  ok('字幕层词卡加入生词本带上本句和时间', added?.meaning === '有韧性的' && added.sentence === 'Another track line.' && added.timeSec === 0, JSON.stringify(added))
  const bubHidden = await page.evaluate(() => {
    const b = window.__roots.map((r) => r.querySelector('.bub')).find(Boolean)
    b.classList.remove('open')
    return getComputedStyle(b).display
  })
  ok('词卡样式不会让关着的浮窗露出来', bubHidden === 'none', bubHidden)

  // 6. 划词工具条的“释义”：同一张词卡
  await inject(SEL)
  await page.evaluate(() => {
    const p = document.getElementById('txt')
    const tn = p.firstChild
    const i = tn.textContent.indexOf('resilient')
    const rng = document.createRange()
    rng.setStart(tn, i)
    rng.setEnd(tn, i + 'resilient'.length)
    getSelection().removeAllRanges()
    getSelection().addRange(rng)
    document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: 100, clientY: 400 }))
  })
  await page.waitForFunction(() => (window.__roots || []).some((r) => r.querySelector('.bar')), null, { timeout: 4000 }).catch(() => {})
  const clicked = await page.evaluate(() => {
    const b = window.__roots.flatMap((r) => [...r.querySelectorAll('.bar button')]).find((x) => x.textContent.trim() === '释义')
    b?.click()
    return !!b
  })
  await page.waitForFunction(() => (window.__roots || []).some((r) => r.querySelector('.card .body.wcard .ms')), null, { timeout: 5000 }).catch(() => {})
  const card = await inRoots(() => {
    const c = window.__roots.map((r) => r.querySelector('.card .body')).find(Boolean)
    return c ? { card: c.classList.contains('wcard'), text: c.innerText } : null
  })
  ok('划词“释义”用的是同一张词卡', clicked && card?.card && /有韧性的/.test(card.text) && /本句/.test(card.text) && /例句/.test(card.text), JSON.stringify(card))
  const lk = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run' && c.req.kind === 'lookup').map((c) => c.req.context))
  ok('划词查词带上了所在的句子', lk.some((c) => /resilient garden/.test(c)), JSON.stringify(lk))
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.card .heart')).find(Boolean).click())
  await page.waitForTimeout(300)
  const added2 = await page.evaluate(() => window.__added)
  ok('划词词卡加入生词本', added2?.word === 'resilient' && /resilient garden/.test(added2.sentence), JSON.stringify(added2))

  ok('没有页面错误', !errors.length, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
