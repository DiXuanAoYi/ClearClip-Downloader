// 视频会话：摘要面板和字幕层共用字幕和译文；字幕层查词浮窗和划词工具条共用一张词卡。
// 在真实浏览器里加载内容脚本，后台消息用假的代替。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const { group } = require('./ext-files.cjs')
const VSUM = group('vsum')
const SUB = group('subLayer')
const SEL = group('selbar')
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
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
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
        if (m.type === 'cc-tr') return { ok: true, texts: (m.texts || []).map((t) => '译:' + t), cached: 0 }
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

  // 4a. 双击视频上的字幕直接改：会话、摘要文稿、本地记录都跟着；摘要里改了，字幕层也跟着
  await page.evaluate(() => globalThis.ccVSession.setLines([{ t: 0, text: 'Alpha line here.' }, { t: 2000, text: 'Beta line here.' }], 'English', 'vsum'))
  await page.waitForFunction(() => (window.__roots || []).some((r) => /Alpha line here/.test(r.querySelector('.cap')?.textContent || '')), null, { timeout: 5000 }).catch(() => {})
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.cap')).find(Boolean).dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
  ok('双击视频上的字幕：变成输入框', await page.evaluate(() => window.__roots.some((r) => r.querySelector('.cap textarea.ledit'))))
  await page.evaluate(() => {
    const ta = window.__roots.map((r) => r.querySelector('.cap textarea.ledit')).find(Boolean)
    ta.value = 'Alpha line fixed.'
    ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  })
  await page.waitForTimeout(300)
  ok('回车保存：视频会话里这一句变了', await page.evaluate(() => globalThis.ccVSession.current().lines[0].text === 'Alpha line fixed.'))
  ok('字幕层显示新文字', /Alpha line fixed/.test(await inRoots(() => window.__roots.map((r) => r.querySelector('.cap')).find(Boolean)?.textContent || '')))
  ok('改动记在本地', await page.evaluate(() => Object.values(window.__store.vsumEdits || {}).some((m) => m['0|Alpha line here.'] === 'Alpha line fixed.')))
  await page.evaluate(() => globalThis.ccVSession.editLine(0, 'Alpha line again.', 'vsum'))
  await page.waitForTimeout(300)
  ok('文稿那边改了一句，字幕层跟着换', /Alpha line again/.test(await inRoots(() => window.__roots.map((r) => r.querySelector('.cap')).find(Boolean)?.textContent || '')))
  // 还原成上一步的字幕，后面的用例接着用
  await page.evaluate(() => globalThis.ccVSession.setLines([{ t: 0, text: 'Another track line.' }], 'English (auto)', 'vsum'))
  await page.waitForFunction(() => (window.__roots || []).some((r) => /译:Another track line/.test(r.querySelector('.cap')?.textContent || '')), null, { timeout: 5000 }).catch(() => {})

  // 4b. 视频胶囊：字幕层的“双语”开关和视频摘要的“记笔记”都在同一个胶囊里（以前各有一个小按钮）
  await page.mouse.move(300, 120)
  await page.waitForTimeout(150)
  await page.mouse.move(320, 140)
  await page.waitForTimeout(400)
  const dock = await page.evaluate(() => {
    const all = (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.dock')])
    const d = all.find((x) => x.style.display !== 'none')
    return d ? { n: all.length, text: d.innerText.replace(/\s+/g, ' '), sw: !!d.querySelector('.sw') } : null
  })
  ok('一个胶囊，上面有字幕层的“双语”开关', dock && dock.n === 1 && /双语/.test(dock.text) && dock.sw, JSON.stringify(dock))
  ok('没有以前的字幕胶囊和“笔记 (n)”按钮', !(await page.evaluate(() => (window.__roots || []).some((r) => r.querySelector('.pill')))))
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dock .dots')).find(Boolean).click())
  await page.waitForTimeout(250)
  const mItems = await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dmenu .it')]).map((b) => b.textContent))
  ok('菜单里有字幕设置、精听、生词本、关闭字幕层', ['字幕设置…', '精听模式', '生词本', '关闭字幕层'].every((x) => mItems.some((t) => t.startsWith(x))), mItems.join(' | '))
  ok('菜单里有记笔记、关闭视频摘要；已经打开的不再出现在“打开”里', mItems.some((t) => t.startsWith('记下当前这句')) && mItems.some((t) => t.startsWith('关闭视频摘要')) && !mItems.some((t) => t.startsWith('视频摘要 / 总结')) && !mItems.some((t) => t.startsWith('双语字幕层')), mItems.join(' | '))
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.dmenu .it')]).find((b) => b.textContent.startsWith('字幕设置')).click())
  await page.waitForTimeout(300)
  const popAt = await page.evaluate(() => {
    const pop = window.__roots.map((r) => r.querySelector('.menu.open')).find(Boolean)
    const cap = window.__roots.map((r) => r.querySelector('.dock')).find(Boolean)
    return pop && cap ? { pop: pop.getBoundingClientRect().bottom, cap: cap.getBoundingClientRect().top } : null
  })
  ok('字幕设置在胶囊上方打开', popAt && popAt.pop <= popAt.cap + 1, JSON.stringify(popAt))
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.menu.open')).find(Boolean)?.classList.remove('open'))
  // 胶囊上的开关：关掉字幕再打开
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dock .sw')).find(Boolean).click())
  await page.waitForTimeout(300)
  ok('胶囊上的开关能关掉字幕', !(await page.evaluate(() => /译:/.test(window.__roots.map((r) => r.querySelector('.cap')).find(Boolean)?.textContent || ''))))
  await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dock .sw')).find(Boolean).click())
  await page.waitForTimeout(400)

  // 5. 字幕层查词：共用词卡（释义、本句意思、本句、例句、加入生词本）
  await page.evaluate(() => {
    const w = window.__roots.map((r) => r.querySelector('.cap .w')).find(Boolean)
    w.click()
  })
  await page.waitForFunction(() => (window.__roots || []).some((r) => r.querySelector('.wc-pop.open .ms')), null, { timeout: 5000 }).catch(() => {})
  const bub = await inRoots(() => {
    const b = window.__roots.filter((r) => r.querySelector('.cap')).map((r) => r.querySelector('.wc-pop')).find(Boolean)
    const x = b.getBoundingClientRect()
    return { open: b.classList.contains('open'), card: b.classList.contains('wcard'), text: b.innerText, actions: b.querySelectorAll('.row2 button').length, inView: x.top >= 0 && x.left >= 0 && x.bottom <= innerHeight && x.right <= innerWidth, old: window.__roots.some((r) => r.querySelector('.bub')) }
  })
  ok('字幕层查词浮窗是共用词卡', bub.open && bub.card && /有韧性的/.test(bub.text) && /在这句话里/.test(bub.text) && /本句/.test(bub.text) && /例句/.test(bub.text), JSON.stringify(bub))
  ok('字幕层自己的按钮还在（跳到这句 / 原声 / 朗读 / 复制 / 录原声）', bub.actions === 5, String(bub.actions))
  ok('字幕层查词浮窗完整在窗口里，旧的浮窗没有了', bub.inView && !bub.old, JSON.stringify(bub))
  // 单词靠近窗口顶部、上下都放不下：放到单词的左边或右边，不裁切
  const side = await page.evaluate(() => {
    const el = document.createElement('div')
    el.style.cssText = 'position:fixed;height:500px'
    document.body.append(el)
    const H = innerHeight
    const r = { left: 300, right: 360, width: 60, top: H / 2 - 10, bottom: H / 2 + 10, height: 20 }
    Object.defineProperty(el, 'scrollHeight', { value: H - 100 })
    globalThis.ccWordCard.place(el, r)
    const b = el.getBoundingClientRect()
    el.remove()
    return { left: b.left, top: b.top, bottom: b.bottom, right: b.right, H, W: innerWidth }
  })
  ok('上下放不下时浮窗放到单词旁边且在窗口内', side.left >= 360 && side.top >= 0 && side.bottom <= side.H && side.right <= side.W, JSON.stringify(side))
  await page.evaluate(() => window.__roots.filter((r) => r.querySelector('.cap')).map((r) => r.querySelector('.wc-pop .heart')).find(Boolean).click())
  await page.waitForTimeout(300)
  const added = await page.evaluate(() => window.__added)
  ok('字幕层词卡加入生词本带上本句和时间', added?.meaning === '有韧性的' && added.sentence === 'Another track line.' && added.timeSec === 0, JSON.stringify(added))
  const bubHidden = await page.evaluate(() => {
    const b = window.__roots.filter((r) => r.querySelector('.cap')).map((r) => r.querySelector('.wc-pop')).find(Boolean)
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

  // 7. 划词工具条的“翻译”：打开网页翻译的同一张卡片（多服务、翻译窗、存为笔记），页面上不再有单独的“译”按钮
  await inject(['trans-logic.js', 'content/webtrans.js'])
  await page.evaluate(() => {
    const p = document.getElementById('txt')
    const tn = p.firstChild
    const rng = document.createRange()
    rng.setStart(tn, 4)
    rng.setEnd(tn, 30)
    getSelection().removeAllRanges()
    getSelection().addRange(rng)
    document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: 120, clientY: 400 }))
  })
  await page.waitForFunction(() => (window.__roots || []).some((r) => [...r.querySelectorAll('.bar button')].some((b) => b.textContent.trim() === '翻译')), null, { timeout: 4000 }).catch(() => {})
  ok('页面上没有单独的“译”按钮', !(await page.evaluate(() => (window.__roots || []).some((r) => r.querySelector('button.sel')))))
  await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('.bar button')]).find((b) => b.textContent.trim() === '翻译').click())
  await page.waitForFunction(() => (window.__roots || []).some((r) => /译:/.test(r.querySelector('.card .out')?.textContent || '')), null, { timeout: 5000 }).catch(() => {})
  const trCard = await page.evaluate(() => {
    const c = window.__roots.map((r) => r.querySelector('.card .out')).find((x) => x && /译:/.test(x.textContent))
    const card = c?.closest('.card')
    return card ? { out: c.textContent, btns: [...card.querySelectorAll('.bar button')].map((b) => b.textContent) } : null
  })
  ok('划词“翻译”打开网页翻译的卡片', trCard && /^译:resilient/.test(trCard.out) && ['翻译窗', '存为笔记'].every((x) => trCard.btns.includes(x)), JSON.stringify(trCard))

  // 8. 划词高亮：深色网页上的白字，高亮后字变成黑色（高亮底色都是浅色）
  await page.evaluate(() => {
    for (const r of window.__roots || []) r.querySelector('.card')?.remove()
    const p = document.createElement('p')
    p.id = 'dark'
    p.style.cssText = 'background:#111;color:#fff'
    p.textContent = 'White words on a dark page should stay readable.'
    document.body.append(p)
  })
  // 划词工具条出现有时序，整个“选中 → 点色块”动作按条件重试，直到高亮出现
  for (let i = 0; i < 6; i++) {
    await page.evaluate(() => {
      const tn = document.querySelector('#dark').firstChild
      const rng = document.createRange()
      rng.setStart(tn, 0)
      rng.setEnd(tn, 11)
      getSelection().removeAllRanges()
      getSelection().addRange(rng)
      document.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, clientX: 100, clientY: 420 }))
    })
    await page.waitForFunction(() => (window.__roots || []).some((r) => r.querySelector('.bar .sw')), null, { timeout: 2000 }).catch(() => {})
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.bar .sw')).find(Boolean)?.click())
    const done = await page.waitForFunction(() => document.querySelector('#dark span[data-cc-hl]'), null, { timeout: 1500 }).then(() => true).catch(() => false)
    if (done) break
  }
  const hlc = await page.evaluate(() => {
    const sp = document.querySelector('#dark span[data-cc-hl]')
    return sp ? { text: sp.textContent, color: getComputedStyle(sp).color, bg: getComputedStyle(sp).backgroundColor } : null
  })
  ok('高亮后的字是黑色（深色网页的白字也看得清）', hlc?.text === 'White words' && hlc.color === 'rgb(17, 17, 17)' && hlc.bg !== 'rgba(0, 0, 0, 0)', JSON.stringify(hlc))

  ok('没有页面错误', !errors.length, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
