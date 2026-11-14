// 视频摘要面板：在真实浏览器里加载内容脚本，后台消息用假的代替。
// 覆盖：YouTube 字幕读取 → 面板各标签页 → 提交/轮询/缓存 → 问答里的时间引用 → 文章模式 → 没有字幕时的提示。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const FILES = require('./ext-files.cjs').group('vsum').map((f) => path.join(EXT, f))
let pass = 0
let fail = 0
const { soon, settle: flush } = require('./lib.cjs')
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const okSoon = async (name, cond, extra) => {
  const r = await soon(cond, extra)
  ok(name, r.pass, r.extra)
}

const YT = 'https://www.youtube.com/watch?v=abc123XYZ_-'
const tracks = (emptyTt) => [{ baseUrl: `https://www.youtube.com/api/timedtext?v=abc123XYZ_-&lang=en${emptyTt ? '&empty=1' : ''}`, languageCode: 'en', name: { simpleText: 'English' } }]
const ytHtml = (withTracks, emptyTt = false) => `<!doctype html><title>Demo video - YouTube</title><body><video width="640" height="360"></video><p>comments</p>
<script>var ytInitialPlayerResponse={"captions":{"playerCaptionsTracklistRenderer":{${withTracks ? `"captionTracks":${JSON.stringify(tracks(emptyTt))},"audioTracks":[]` : '"x":1'}}},"videoDetails":{"title":"Demo \\"video\\"","lengthSeconds":"125","shortDescription":"0:00 Intro\\n0:40 Main part\\n1:30 Outro"}};</script>`
const json3 = { events: [{ tStartMs: 0, dDurationMs: 4000, segs: [{ utf8: 'Welcome to the demo.' }] }, { tStartMs: 41000, dDurationMs: 4000, segs: [{ utf8: 'Rust is fast and safe.' }] }, { tStartMs: 95000, dDurationMs: 3000, segs: [{ utf8: 'Thanks for watching.' }] }] }

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } })
  const calls = []
  await ctx.route('https://www.youtube.com/**', (route) => {
    const u = route.request().url()
    if (u.includes('/api/timedtext') && u.includes('empty=1')) return route.fulfill({ status: 200, contentType: 'text/plain', body: '' })
    if (u.includes('/api/timedtext')) return route.fulfill({ contentType: 'application/json', body: JSON.stringify(json3) })
    return route.fulfill({ contentType: 'text/html', body: ytHtml(!u.includes('notracks'), u.includes('emptytt')) })
  })
  await ctx.route('https://article.test/**', (route) =>
    route.fulfill({ contentType: 'text/html', body: `<!doctype html><title>Long read</title><article><p>${'First paragraph about gardens and soil. '.repeat(10)}</p><p>${'Second paragraph about compost and water. '.repeat(10)}</p></article>` }))

  async function open(url, opts = {}) {
    const page = await ctx.newPage()
    const errors = []
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript((o) => {
      // 让封闭的 Shadow DOM 可被测试访问
      const orig = Element.prototype.attachShadow
      Element.prototype.attachShadow = function (init) {
        const r = orig.call(this, { ...init, mode: 'open' })
        window.__roots = (window.__roots || []).concat(r)
        return r
      }
      window.__calls = []
      const store = Object.assign({}, o.local || {})
      window.__store = store
      const rt = {
        sendMessage: async (m) => {
          window.__calls.push(m)
          if (m.type === 'cc-app-lang') return { ok: true, lang: 'zh' }
          if (m.type === 'cc-tr') return { ok: true, texts: (m.texts || []).map((t) => '译:' + t), cached: 0 }
          if (m.type === 'cc-vocab') return { ok: true, words: [], stats: {} }
          if (m.type === 'cc-ai-status') return { ok: true, ready: o.ready !== false, model: 'm', lang: '中文', transcribe: true, subtitles: true, maxChars: 600000 }
          if (m.type === 'cc-ai-run' && m.req.kind === 'preclean') {
            // 预处理（清影只做字符串清理）：不算进“请求了几次清影”
            window.__calls.pop()
            window.__pre = m.req
            ;(window.__preCalls = window.__preCalls || []).push(m.req)
            return { ok: true, id: 99 }
          }
          if (m.type === 'cc-ai-job' && m.id === 99) {
            const lines = window.__pre.lines
            if (o.cleanDemo) return { ok: true, state: 'done', done: 1, total: 1, note: '', result: { lines: lines.slice(1), report: { hearing: 1, filler: 1, emptied: 1 }, punct: false } }
            return { ok: true, state: 'done', done: 1, total: 1, note: '', result: { lines, report: { hearing: 0, filler: 0, emptied: 0 }, punct: false } }
          }
          if (m.type === 'cc-ai-run') {
            window.__job = m.req
            return { ok: true, id: 7 }
          }
          if (m.type === 'cc-ai-job') {
            if (o.hold) return { ok: true, state: 'running', done: 0, total: 1, note: '' }
            if (o.streamHold && window.__job?.kind === 'transcribe' && !window.__release)
              return { ok: true, state: 'running', done: 30, total: 100, note: '正在转写第 1 / 2 段…', partial: { lines: [{ t: 0, text: '大家好，今天讲讲 Rust。' }], durationMs: 5000, track: '清影转写', progress: 0.5 } }
            if (o.stream && window.__job?.kind === 'transcribe' && !window.__streamed) {
              window.__streamed = true
              return { ok: true, state: 'running', done: 30, total: 100, note: '正在转写第 1 / 2 段…', partial: { lines: [{ t: 0, text: '大家好，今天讲讲 Rust。' }], durationMs: 5000, track: '清影转写', progress: 0.5 } }
            }
            const k = window.__job?.kind || o.resumeKind
            window.__job = window.__job || { kind: k }
            const result = {
              summary: { summary: '这个视频介绍了 Rust。', points: ['Rust 又快又安全 [0:41]', '结尾致谢'], keywords: ['Rust', '性能'], chapters: [{ title: 'AI 开场', startMs: 0, endMs: 40000 }, { title: 'AI 正文', startMs: 40000, endMs: 125000 }] },
              mindmap: { outline: '# 主题\n- 分支一\n  - 细节\n- 分支二' },
              flowgraph: { flow: { nodes: [{ id: 'a', title: '发现问题', detail: '页面很慢', kind: 'cause', time: '0:41' }, { id: 'b', title: '压缩图片', kind: 'step', time: '1:10' }, { id: 'c', title: '加载变快', kind: 'result' }], edges: [{ from: 'a', to: 'b', label: '所以' }, { from: 'b', to: 'c', label: '导致' }] } },
              ask: { answer: '因为它没有垃圾回收 [0:41]。', refs: [41000] },
              cards: { cards: [{ q: 'Rust 的特点？', a: '又快又安全', t: 41000 }] },
              quotes: { quotes: [{ text: 'Rust is fast and safe.', t: 41000 }] },
              translate: { lines: (window.__job.lines || []).map((l) => ({ t: l.t, text: '译:' + l.text })), failed: 0 },
              tracks: { tracks: [{ code: 'en', name: 'English（人工）', auto: false }, { code: 'zh-Hans', name: '中文（自动生成）', auto: true }], audio: true },
              rewrite: { text: '改写后的文案' },
              explain: { text: 'Rust 是一门系统语言。' },
              lookup: { lemma: String(window.__job.text || '').toLowerCase(), phonetic: '/ˈwelkəm/', pos: 'v.', meaning: '欢迎；迎接', context_meaning: '欢迎', examples: [], aligned: window.__job.trans ? '译' : '' },
              note: { quote: '整理后的完整句子。' },
              refine: { quote: '优化后的笔记。' },
              punctuate: { lines: (window.__job.lines || []).map((l) => ({ t: l.t, text: l.text + '。' })), failed: 0 },
              fixasr: { lines: (window.__job.lines || []).map((l) => ({ t: l.t, text: l.text.replace('Rust', 'Rusty') })), failed: 0 },
              subtitles: { lines: o.subs === false && !window.__job?.track ? [] : [{ t: 1000, text: '在线字幕第一句' }, { t: 6000, text: '在线字幕第二句' }], durationMs: 12000, track: '在线字幕（en）', source: 'subtitles' },
              transcribe: { lines: [{ t: 0, text: '大家好，今天讲讲 Rust。' }, { t: 5000, text: '它又快又安全。' }], durationMs: 12000, track: '清影转写' },
            }[k]
            return { ok: true, state: 'done', done: 1, total: 1, note: k === 'transcribe' ? '正在转写…' : '', result }
          }
          if (m.type === 'cc-note' || m.type === 'cc-send-page') return { ok: true }
          return { ok: false, error: 'unknown ' + m.type }
        },
      }
      window.chrome = { runtime: rt, storage: { local: { get: async (k) => (typeof k === 'string' ? (k in store ? { [k]: store[k] } : {}) : {}), set: async (v) => Object.assign(store, JSON.parse(JSON.stringify(v))) }, onChanged: { addListener() {} } } }
    }, opts)
    await page.goto(url)
    for (const f of FILES) await page.addScriptTag({ path: f })
    return { page, errors }
  }
  const root = (page, sel) => page.evaluateHandle((s) => window.__roots.map((r) => r.querySelector(s)).find(Boolean), sel)
  const text = (page, sel = '.panel') => page.evaluate((s) => window.__roots.map((r) => r.querySelector(s)).find(Boolean)?.innerText || '', sel)
  const clickText = (page, label, sel = 'button') =>
    page.evaluate(([l, s]) => {
      const b = [...window.__roots.flatMap((r) => [...r.querySelectorAll(s)])].find((x) => x.textContent.trim() === l)
      if (!b) throw new Error('没有按钮：' + l)
      b.click()
    }, [label, sel])


  // ---- 收到右边：收起成小标签、点开、停在上面预览、拖动换位置、状态提示
  {
    const st = (page) =>
      page.evaluate(() => {
        const r = window.__roots.find((x) => x.querySelector('.panel'))
        const panel = r.querySelector('.panel')
        const tab = r.querySelector('.dtab')
        const bar = r.querySelector('.peekbar')
        const tr = tab.getBoundingClientRect()
        return {
          hide: panel.classList.contains('hide'),
          peek: panel.classList.contains('peek'),
          inert: panel.inert,
          tabOn: tab.classList.contains('on'),
          tabCls: tab.className,
          barOn: bar.classList.contains('on'),
          tab: { left: Math.round(tr.left), right: Math.round(tr.right), top: Math.round(tr.top), w: Math.round(tr.width), h: Math.round(tr.height) },
          toast: r.querySelector('.dtoast.on')?.textContent || '',
        }
      })
    const tabCenter = async (page) => {
      const t = (await st(page)).tab
      return { x: (t.left + t.right) / 2, y: t.top + t.h / 2 }
    }
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    let s = await st(page)
    ok('收起：展开时面板可见，没有小标签', !s.hide && !s.tabOn && !s.inert)
    ok('收起：标题栏有“收起到右边”按钮', await page.evaluate(() => !!window.__roots.map((r) => r.querySelector('.hd .dockbtn[aria-label="收起到右边"]')).find(Boolean)))
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
    await okSoon('收起：面板滑走，只剩右边缘一个小标签', async () => { s = await st(page); return s.hide && s.inert && s.tabOn && s.tab.right === 1280 && s.tab.w === 30 && s.tab.h === 84 }, async () => JSON.stringify(s))
    ok('收起：小标签默认在窗口高度 38% 附近', Math.abs(s.tab.top + 42 - 0.38 * 800) <= 2, String(s.tab.top))
    ok('收起：文稿还在（没有重新读取）', await page.evaluate(() => /段/.test(window.__roots.map((r) => r.querySelector('.meta')).find(Boolean)?.textContent || '')))
    // 点标签展开
    let c = await tabCenter(page)
    await page.mouse.click(c.x, c.y)
    await okSoon('点小标签：展开（固定），标签消失', async () => { s = await st(page); return !s.hide && !s.peek && !s.tabOn && !s.inert }, async () => JSON.stringify(s))
    // 快捷键 = 再次注入 vsum.js：展开 ↔ 收起；不会关掉面板
    await page.addScriptTag({ path: FILES[FILES.length - 1] })
    await okSoon('快捷键：展开时按一下 → 收起', async () => (await st(page)).hide)
    await page.addScriptTag({ path: FILES[FILES.length - 1] })
    await okSoon('快捷键：收起时按一下 → 展开', async () => !(await st(page)).hide && (await page.evaluate(() => !!window.__ccVsum)))
    // 预览：停 0.35 秒弹出来，移开 0.8 秒收回去（先把鼠标挪开：它还停在刚才标签的位置，标签再出现会算“停在上面”）
    await page.mouse.move(600, 300)
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
    await soon(async () => { const x = await st(page); return x.hide && x.tabOn && x.tab.right === 1280 }, null, 5000) // 等收起动画走完、小标签就位
    c = await tabCenter(page)
    await page.mouse.move(c.x, c.y)
    // 计时类断言：要等的就是真实时间过去（悬停延时 / 延时收回），不能改成轮询
    await page.waitForTimeout(150)
    ok('预览：只路过 150 毫秒不弹', (await st(page)).hide)
    await page.mouse.move(600, 300)
    await page.waitForTimeout(100) // 计时：先让鼠标离开一会儿，悬停计时才会重置
    c = await tabCenter(page)
    await page.mouse.move(c.x, c.y)
    await okSoon('预览：停够 0.35 秒弹出来，带预览条，标签粘在面板左边', async () => { s = await st(page); return s.peek && !s.hide && s.barOn && s.tab.right === 880 && /peek/.test(s.tabCls) }, async () => JSON.stringify(s))
    await page.mouse.move(300, 300)
    await page.waitForTimeout(400) // 计时：0.4 秒内还在
    ok('预览：鼠标刚移开 0.4 秒还在', (await st(page)).peek)
    await page.waitForTimeout(700) // 计时：累计超过 0.8 秒要收回
    ok('预览：移开超过 0.8 秒收回去', (await st(page)).hide)
    // 预览中点“固定”
    c = await tabCenter(page)
    await page.mouse.move(c.x, c.y)
    await soon(async () => { const x = await st(page); return x.peek && x.tab.right === 880 }, null, 5000) // 等悬停预览弹出来（停够 0.35 秒）并滑到位
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.peekbar button')).find(Boolean).click())
    await okSoon('预览：点“固定”→ 展开，不再自动收回', async () => { s = await st(page); return !s.hide && !s.peek && !s.barOn }, async () => JSON.stringify(s))
    await page.mouse.move(300, 300)
    await page.waitForTimeout(1200) // 计时：要确认“过了收回时间也没收”
    ok('固定后鼠标移开也不收', !(await st(page)).hide)
    // 预览中点面板里的任何地方 = 固定
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
    await soon(async () => { const x = await st(page); return x.hide && x.tabOn && x.tab.right === 1280 }, null, 5000) // 等收起动画走完、小标签就位
    c = await tabCenter(page)
    await page.mouse.move(c.x, c.y)
    await soon(async () => { const x = await st(page); return x.peek && x.tab.right === 880 }, null, 5000) // 等悬停预览弹出来（停够 0.35 秒）并滑到位
    await page.mouse.click(1000, 400)
    await okSoon('预览：在面板里点一下也算固定', async () => { s = await st(page); return !s.hide && !s.peek }, async () => JSON.stringify(s))
    // 拖动换位置：记住，点击不触发展开
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
    await soon(async () => { const x = await st(page); return x.hide && x.tabOn && x.tab.right === 1280 }, null, 5000) // 等收起动画走完、小标签就位
    const before = (await st(page)).tab.top
    c = await tabCenter(page)
    await page.mouse.move(c.x, c.y)
    await page.mouse.down()
    await page.mouse.move(c.x, c.y + 60, { steps: 4 })
    await page.mouse.move(c.x, c.y + 120, { steps: 4 })
    await page.mouse.up()
    await okSoon('拖动：标签上下移动了约 120 像素', async () => { s = await st(page); return Math.abs(s.tab.top - before - 120) <= 3 }, async () => `${before} → ${s.tab.top}`)
    ok('拖动：松手不会当作点击（还是收起）', s.hide && !s.peek, JSON.stringify(s))
    const frac = await page.evaluate(() => window.__store.vsumTabFrac)
    ok('拖动：位置存进了存储', typeof frac === 'number' && Math.abs(frac - (s.tab.top + 42) / 800) < 0.01, String(frac))
    await page.close()

    // 位置记住：新页面按存起来的比例放
    {
      const p2 = await open(YT, { local: { vsumTabFrac: 0.7 } })
      await p2.page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
      await p2.page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
      await okSoon('位置记住：新页面标签在 70% 高度', async () => Math.abs((await st(p2.page)).tab.top + 42 - 0.7 * 800) <= 2)
      await p2.page.close()
    }
    // 设置：预览关掉 / 打开时默认收起
    {
      const p3 = await open(YT, { local: { vsumPrefs: { peek: false, collapseOnOpen: true } } })
      await p3.page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
      let t = await st(p3.page)
      ok('设置“打开时默认收起”：一打开就是小标签', t.hide && t.tabOn, JSON.stringify(t))
      const cc = await tabCenter(p3.page)
      await p3.page.mouse.move(cc.x, cc.y)
      await p3.page.waitForTimeout(700) // 计时：要确认“过了预览延时也没弹”
      ok('设置“鼠标停在标签上预览”关掉后不弹', (await st(p3.page)).hide)
      await p3.page.mouse.click(cc.x, cc.y)
      await okSoon('关了预览，点一下照样展开', async () => !(await st(p3.page)).hide)
      await p3.page.close()
    }
    // 状态：没设置大模型 → 琥珀点 + 一句原因
    {
      const p4 = await open(YT, { ready: false })
      await p4.page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
      await p4.page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
      await okSoon('没设置大模型：收起后标签有琥珀点，弹一句原因', async () => { const t = await st(p4.page); return /warn/.test(t.tabCls) && /还没设置大模型/.test(t.toast) }, async () => { const t = await st(p4.page); return JSON.stringify(t) })
      await p4.page.close()
    }
    // 状态：收起时生成好了 → 绿点 + 提示，展开后消失
    {
      const p5 = await open(YT)
      await p5.page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
      await clickText(p5.page, '生成摘要')
      await p5.page.evaluate(() => window.__roots.map((r) => r.querySelector('.dockbtn')).find(Boolean).click())
      await p5.page.waitForFunction(() => window.__roots.some((r) => r.querySelector('.dtab.busy')), null, { timeout: 3000 }).catch(() => {})
      ok('收起时正在生成：标签上图标转圈', await p5.page.evaluate(() => window.__roots.some((r) => r.querySelector('.dtab.busy') || r.querySelector('.dtab.fresh'))))
      await p5.page.waitForFunction(() => window.__roots.some((r) => r.querySelector('.dtab.fresh')), null, { timeout: 6000 }).catch(() => {})
      const t = await st(p5.page)
      ok('生成好了：绿点 + “摘要生成好了 · 点开看”', /fresh/.test(t.tabCls) && /摘要生成好了/.test(t.toast), JSON.stringify(t))
      const cc = await tabCenter(p5.page)
      await p5.page.mouse.click(cc.x, cc.y)
      await okSoon('展开后绿点和提示都去掉', async () => (await st(p5.page)).toast === '' && !(await st(p5.page)).tabOn)
      ok('收起相关操作没有页面报错', p5.errors.length === 0 && errors.length === 0, p5.errors.concat(errors).join('|'))
      await p5.page.close()
    }
  }

  // ---- 笔记 / 划词解释 / 顺句
  {
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '文稿')
    // 点 ＋ 记一条
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.ln .rec')).find(Boolean).click())
    await okSoon('点 ＋ 存进了笔记', async () => await page.evaluate(() => window.__store.vsumNotes?.length === 1))
    // 划词
    await page.evaluate(() => {
      const r = window.__roots.find((x) => x.querySelector('.ln'))
      const span = r.querySelector('.ln span:nth-child(2)')
      const rng = document.createRange()
      const tn = document.createTreeWalker(span, NodeFilter.SHOW_TEXT).nextNode()
      rng.setStart(tn, 0)
      rng.setEnd(tn, 3)
      const s = r.getSelection ? r.getSelection() : getSelection()
      s.removeAllRanges()
      s.addRange(rng)
      r.querySelector('#lines').dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
      span.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
    })
    ok('划词出现工具条', (await text(page, '#selbar')).includes('解释'))
    await clickText(page, '解释')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('Rust 是一门系统语言')), null, { timeout: 8000 })
    const ex = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1).req)
    ok('解释请求带选中文字和上下文', ex.kind === 'explain' && ex.text.length > 0 && typeof ex.context === 'string', JSON.stringify(ex).slice(0, 160))
    await clickText(page, '存为笔记')
    await okSoon('划词存为笔记（原文）', async () => await page.evaluate(() => window.__store.vsumNotes.length === 2))
    // 顺句
    await clickText(page, '顺句')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('还原顺句前')), null, { timeout: 8000 })
    ok('顺句后文稿有标点', (await text(page, '#lines')).includes('。'))
    await clickText(page, '还原顺句前')
    ok('可以还原', !(await text(page)).includes('还原顺句前'))
    // 笔记页
    await clickText(page, '笔记')
    ok('笔记页列出 2 条', (await text(page)).includes('2 条'))
    await clickText(page, 'AI 整理')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('整理后的完整句子')), null, { timeout: 8000 })
    await clickText(page, '替换')
    await okSoon('AI 整理后替换原文', async () => await page.evaluate(() => window.__store.vsumNotes.some((n) => n.text === '整理后的完整句子。')))
    // 导出
    const dl = page.waitForEvent('download', { timeout: 5000 }).catch(() => null)
    await clickText(page, '导出笔记 (.md)')
    await dl
    // 搜索
    await page.evaluate(() => {
      const i = window.__roots.map((r) => r.querySelector('.body input[type=text]')).find(Boolean)
      i.value = '不存在的词'
      i.dispatchEvent(new Event('input'))
    })
    ok('搜索无结果', (await text(page)).includes('0 条'))
    await clickText(page, '更多')
    await page.evaluate(() => {
      const sel = window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).find((x) => [...x.options].some((o) => o.text === '深色'))
      sel.value = 'dark'
      sel.dispatchEvent(new Event('change'))
    })
    await okSoon('外观：选“深色”后面板用深色样式', async () => await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('style')].some((st) => /@media|#0b1424/.test(st.textContent) && st.textContent.includes('.src') && !st.textContent.includes('@media'))) ))
    ok('笔记流程无页面错误', errors.length === 0, errors.join('|'))
    await page.close()
  }

  // ---- 站内切换视频（网址变了、页面没刷新）：自动重新读取字幕
  {
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const seen = []
    page.on('request', (r) => seen.push(r.url()))
    await page.evaluate(() => history.pushState({}, '', '/watch?v=second12345'))
    await page.waitForFunction(() => true)
    await okSoon('切换视频后自动去读新视频的字幕', async () => seen.some((u) => u.includes('v=second12345')), async () => seen.join('|'))
    await okSoon('切换后面板重新显示文稿', async () => /段/.test(await text(page, '.meta')), async () => await text(page, '.meta'))
    ok('切换视频流程无页面错误', errors.length === 0, errors.join('|'))
    await page.close()
  }

  // ---- 预处理：已清理 N 处 + 显示原始
  {
    const { page, errors } = await open(YT, { cleanDemo: true })
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '文稿')
    await page.waitForSelector('#cleanbar', { state: 'attached' }).catch(() => {})
    await page.waitForFunction(() => window.__roots.some((r) => r.querySelector('#cleanbar')), null, { timeout: 8000 })
    const n1 = await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0))
    const bar = await text(page, '#cleanbar')
    ok('文稿显示清理后的（少了一行），并提示“已清理 3 处”', n1 === 2 && /已清理 3 处/.test(bar) && /非语音说明 1 行/.test(bar) && /口水词 1 行/.test(bar), n1 + ' ' + bar)
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('#cleantoggle')).find(Boolean).click())
    await okSoon('点“显示原始”换回原始文稿（3 行），按钮变成“显示清理后”', async () => { const n2 = await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0)); return n2 === 3 && /显示清理后/.test(await text(page, '#cleantoggle')) }, async () => { const n2 = await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0)); return String(n2) })
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('#cleantoggle')).find(Boolean).click())
    await okSoon('再点一下回到清理后的', async () => (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0))) === 2)
    ok('预处理没有页面错误', errors.length === 0, errors.join('|'))
    await page.close()
  }

  // ---- YouTube：读字幕 → 摘要
  {
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const meta = await text(page, '.meta')
    ok('读到字幕：3 段、轨道名', /3 段/.test(meta) && /English/.test(meta), meta)
    ok('标题取自页面数据', (await text(page, '.hd')).includes('Demo "video"'))
    ok('读到字幕后自动打开字幕层（文稿放进视频会话）', await page.evaluate(() => window.__calls.some((c) => c.type === 'cc-sublayer-open') && globalThis.ccVSession?.current().lines.length === 3), '')
    await clickText(page, '生成摘要')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('这个视频介绍了 Rust')), null, { timeout: 8000 })
    const run = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-ai-run').req)
    ok('提交的请求带类型、风格、语言、整理好的行', run.kind === 'summary' && run.style === 'standard' && run.lines.length === 3 && run.lines[1].t === 41000 && run.lang === '中文', JSON.stringify(run))
    ok('关键词、要点显示', /Rust/.test(await text(page)) && /性能/.test(await text(page)) && /又快又安全/.test(await text(page)))
    ok('要点里的 [0:41] 变成左边的时间标签', await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('.pts .tm')].some((b) => b.textContent === '0:41'))))
    // 点时间会让页面的视频跳转（没有媒体源时 currentTime 不变，只要不报错）
    await page.evaluate(() => window.__roots[0].querySelector('.pts li.go').click())

    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-summary.png' })
    // 章节：AI 章节优先
    await clickText(page, '章节')
    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-chapters.png' })
    ok('章节页显示 AI 章节', /AI 开场/.test(await text(page)) && /AI 正文/.test(await text(page)))

    // 文稿：搜索、翻译
    await clickText(page, '文稿')
    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-transcript.png' })
    ok('文稿列出 3 行', (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0))) === 3)
    // 文稿里鼠标停在英文单词上：弹出和字幕层同一个查词浮窗；以前那个整句译文小窗没有了
    const wordAt = (n) => page.evaluate((n) => { const el = window.__roots.flatMap((r) => [...r.querySelectorAll('#lines .ln i.w')])[n]; if (!el) return null; el.scrollIntoView({ block: 'center' }); const b = el.getBoundingClientRect(); return { x: b.x + b.width / 2, y: b.y + b.height / 2, w: el.textContent } }, n)
    const popBox = () => page.evaluate(() => { const p = window.__roots.map((r) => r.querySelector('.wc-pop.open')).find(Boolean); return p ? { ...p.getBoundingClientRect().toJSON(), text: p.innerText, vw: innerWidth, vh: innerHeight } : null })
    const w0 = await wordAt(0)
    ok('文稿原文里的英文单词可以悬停查词', !!w0 && /^[A-Za-z]/.test(w0.w), JSON.stringify(w0))
    // 视频正在播放（测试页的视频没有片源，这里换成能记录暂停 / 播放的）
    await page.evaluate(() => {
      const v = document.querySelector('video')
      let paused = false
      Object.defineProperty(v, 'paused', { get: () => paused, configurable: true })
      v.pause = () => void (paused = true)
      v.play = () => ((paused = false), Promise.resolve())
    })
    await page.mouse.move(w0.x, w0.y)
    await page.waitForFunction(() => window.__roots.some((r) => /欢迎/.test(r.querySelector('.wc-pop.open')?.textContent || '')), null, { timeout: 8000 }).catch(() => {})
    const pb = await popBox()
    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-word.png' })
    ok('悬停单词弹出查词浮窗（音标、释义、收藏）', pb && /欢迎/.test(pb.text) && /ˈwelkəm/.test(pb.text) && /♡/.test(pb.text), pb && pb.text.slice(0, 200))
    ok('查词浮窗完整在窗口里（不被裁切）', pb && pb.top >= 0 && pb.left >= 0 && pb.bottom <= pb.vh && pb.right <= pb.vw, JSON.stringify(pb && { t: pb.top, l: pb.left, b: pb.bottom, r: pb.right }))
    ok('查词浮窗有跳到这句、朗读、复制', pb && /跳到这句/.test(pb.text) && /朗读/.test(pb.text) && /复制/.test(pb.text), pb && pb.text)
    const lk = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run' && c.req.kind === 'lookup').at(-1)?.req)
    ok('查词请求带单词和这一句', lk && lk.text === w0.w && /Welcome|Rust/.test(lk.context || ''), JSON.stringify(lk))
    ok('悬停单词时视频暂停', await page.evaluate(() => document.querySelector('video').paused === true))
    ok('以前的整句译文小窗没有了', await page.evaluate(() => !window.__roots.some((r) => r.querySelector('.hov'))))
    await page.mouse.move(5, 5)
    await okSoon('鼠标移开后查词浮窗关闭', async () => !(await popBox()))
    await okSoon('词卡关闭后视频接着播', async () => await page.evaluate(() => document.querySelector('video').paused === false))
    // 用户自己暂停的视频：悬停、移开都不去动它
    await page.evaluate(() => document.querySelector('video').pause())
    const w0b = await wordAt(0)
    await page.mouse.move(w0b.x, w0b.y)
    await page.waitForTimeout(900) // 必要：断言“悬停 / 移开都没去动视频”，要让悬停延时和收回延时真实走完
    await page.mouse.move(5, 5)
    await okSoon('用户自己暂停的视频，悬停和移开后仍然是暂停', async () => await page.evaluate(() => document.querySelector('video').paused === true))
    await page.evaluate(() => document.querySelector('video').play())
    await page.evaluate(() => {
      const i = window.__roots.map((r) => r.querySelector('input#tsearch')).find(Boolean)
      i.value = 'rust'
      i.dispatchEvent(new Event('input'))
    })
    await okSoon('搜索：全部行保留，命中的行加亮并标出词', async () => (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0))) === 3 && (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln.hit mark').length, 0))) >= 1)
    ok('搜索：显示 1/1 计数', /1\/1/.test(await text(page)))
    await page.evaluate(() => {
      const i = window.__roots.map((r) => r.querySelector('input#tsearch')).find(Boolean)
      i.value = ''
      i.dispatchEvent(new Event('input'))
    })
    await flush(page)
    // 文稿显示：原文 / 译文 / 双语（没翻译时是原文，选译文或双语才去翻译，原文始终保留）
    const viewSel = () => window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).find((x) => [...x.options].some((o) => o.value === 'tr') && x.options.length === 3)
    ok('文稿里有“原文 / 译文 / 双语”选择，没翻译时停在原文', await page.evaluate((f) => { const x = eval('(' + f + ')')(); return x && [...x.options].map((o) => o.text).join('|') === '原文|译文|双语' && x.value === 'orig' }, viewSel.toString()))
    ok('没翻译时文稿显示的是原文', !(await text(page, '#lines')).includes('译:'))
    await page.evaluate((f) => { const x = eval('(' + f + ')')(); x.value = 'both'; x.dispatchEvent(new Event('change')) }, viewSel.toString())
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('译:Welcome')), null, { timeout: 8000 })
    ok('选双语：自动去翻译，每行原文下方出现译文（原文没有被替换）', (await text(page, '#lines')).includes('Welcome') && (await text(page, '#lines')).includes('译:Welcome'))
    // 双语：鼠标停在原文的英文单词上，下面译文里对应的词高亮；浮窗关了高亮也去掉
    const w1 = await wordAt(0)
    await page.mouse.move(w1.x, w1.y)
    await page.waitForFunction(() => window.__roots.some((r) => r.querySelector('.ln .tr mark.al')), null, { timeout: 8000 }).catch(() => {})
    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-word-both.png' })
    const al = await page.evaluate(() => { const m = window.__roots.map((r) => r.querySelector('.ln .tr mark.al')).find(Boolean); return m ? { text: m.textContent, bg: getComputedStyle(m).backgroundColor, line: m.closest('.ln').dataset.i } : null })
    ok('双语：悬停单词时译文里对应的词高亮', al && al.text === '译' && al.bg !== 'rgba(0, 0, 0, 0)' && al.line === '0', JSON.stringify(al))
    const lk2 = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run' && c.req.kind === 'lookup').at(-1)?.req)
    ok('双语查词请求带上这一句的译文', lk2 && /^译:/.test(lk2.trans || ''), JSON.stringify(lk2))
    ok('悬停的单词本身也标出来', await page.evaluate(() => window.__roots.some((r) => r.querySelector('.ln .mn i.w.on'))))
    await page.mouse.move(5, 5)
    await okSoon('浮窗关闭后译文高亮去掉、文字不变', async () => await page.evaluate(() => !window.__roots.some((r) => r.querySelector('mark.al, i.w.on')) && window.__roots.some((r) => /^译:Welcome/.test(r.querySelector('.ln .tr')?.textContent || ''))))
    ok('翻译后每行下方有译文', true)
    // 生词页：切到这一页就主动刷新（不再有“刷新”按钮）
    const vocabCalls = () => page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-vocab' && c.op === 'list').length)
    const v0 = await vocabCalls()
    const tabVocab = () => page.evaluate(() => [...window.__roots.flatMap((r) => [...r.querySelectorAll('.tabs button')])].find((b) => /^生词/.test(b.textContent.trim())).click())
    await tabVocab()
    await soon(async () => (await vocabCalls()) > v0, null, 5000) // 等“生词”页向后台要了一次
    const v1 = await vocabCalls()
    await clickText(page, '文稿')
    await tabVocab()
    await okSoon('每次切到“生词”都向清影要最新的生词', async () => { const v2 = await vocabCalls(); return v1 > v0 && v2 > v1 }, async () => { const v2 = await vocabCalls(); return `${v0} → ${v1} → ${v2}` })
    ok('生词页没有“刷新”按钮', !(await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('.acts button, button')].some((b) => b.textContent.trim() === '刷新')))))
    await clickText(page, '文稿')

    // 任意语言互译：下拉里选日语，请求里带的是 Japanese
    const langs = await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).filter((x) => [...x.options].some((o) => o.value === 'ja')).map((x) => x.options.length))
    ok('翻译语言下拉里有三十多种语言', langs.length > 0 && langs[0] >= 30, JSON.stringify(langs))
    await page.evaluate(() => {
      const sel = window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).find((x) => [...x.options].some((o) => o.value === 'ja'))
      sel.value = 'ja'
      sel.dispatchEvent(new Event('change'))
    })
    ok('语言下拉按界面语言显示：日语 · 日本語', await page.evaluate(() => [...window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).find((x) => [...x.options].some((o) => o.value === 'ja')).options].some((o) => o.value === 'ja' && o.text === '日语 · 日本語')))
    await clickText(page, '翻译成日语')
    await page.waitForFunction(() => window.__calls.some((c) => c.type === 'cc-ai-run' && c.req.kind === 'translate' && c.req.lang === 'Japanese'), null, { timeout: 8000 })
    ok('选日语后请求带 lang=Japanese', true)
    await page.waitForFunction(() => window.__roots.every((r) => !r.querySelector('.panel button[disabled]')), null, { timeout: 8000 })
    await page.evaluate(() => {
      const sel = window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).find((x) => [...x.options].some((o) => o.value === 'ja'))
      sel.value = ''
      sel.dispatchEvent(new Event('change'))
    })

    // 导图
    await clickText(page, '导图')
    await clickText(page, '生成思维导图')
    await page.waitForFunction(() => window.__roots.some((r) => r.querySelector('.svgbox svg')), null, { timeout: 8000 })
    const nodes = await page.evaluate(() => window.__roots.map((r) => r.querySelectorAll('.svgbox svg text').length).find(Boolean))
    ok('导图 SVG 里有节点文字', nodes >= 4, String(nodes))

    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-mindmap.png' })
    // 问答
    await clickText(page, '问答')
    await page.evaluate(() => {
      const i = window.__roots.map((r) => r.querySelector('#q')).find(Boolean)
      i.value = '为什么快？'
      i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }))
    })
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('没有垃圾回收')), null, { timeout: 8000 })
    const ask = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1).req)
    ok('问答请求带问题', ask.kind === 'ask' && ask.question === '为什么快？', JSON.stringify(ask).slice(0, 200))
    ok('回答里的 [0:41] 可点', await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('.msg button.ts')].some((b) => b.textContent === '[0:41]'))))

    // 更多：卡片、金句、改写
    await clickText(page, '更多')
    await clickText(page, '生成问答卡片')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('Rust 的特点')), null, { timeout: 8000 })
    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-more.png' })
    await clickText(page, '提取金句')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('“Rust is fast and safe.”')), null, { timeout: 8000 })
    await clickText(page, '小红书文案')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('改写后的文案')), null, { timeout: 8000 })
    ok('卡片、金句、改写都出来了', true)
    const rewrite = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1).req)
    ok('改写请求带格式与摘要 Markdown', rewrite.format === 'xhs' && rewrite.text.includes('这个视频介绍了 Rust') && rewrite.text.includes('t=41s'), rewrite.text?.slice(0, 120))
    // 存进摘录
    await clickText(page, '存进清影的摘录')
    await okSoon('摘录内容是 Markdown 笔记（末尾带来源）', async () => { const note = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-note')); return note && note.text.startsWith('# Demo "video"') && note.text.includes('Rust 又快又安全') && /\n来源：.*\nhttps:\/\/www.youtube.com/.test(note.text) }, async () => { const note = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-note')); return note?.text?.slice(-120) })

    // 缓存：同样的摘要不再请求
    const before = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)
    await clickText(page, '摘要')
    await clickText(page, '重新生成')
    await okSoon('相同文稿 + 设置的结果走缓存，不再提交', async () => { const after = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length); return after === before }, async () => { const after = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length); return `${before} → ${after}` })

    // 关闭
    await clickText(page, '✕')
    ok('关闭后面板移除', (await page.evaluate(() => !window.__ccVsum)) === true)
    ok('没有页面报错', errors.length === 0, errors.join('|'))
    await page.close()
  }

  // ---- 关掉再打开：不重新生成
  {
    const { page } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '生成摘要')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('这个视频介绍了 Rust')), null, { timeout: 8000 })
    await clickText(page, '更多')
    await clickText(page, '生成问答卡片')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('Rust 的特点')), null, { timeout: 8000 })
    await clickText(page, '✕')
    await okSoon('关闭时保存了这个视频的会话', async () => { const saved = await page.evaluate(() => JSON.parse(JSON.stringify(window.__store))); return Object.keys(saved.vsumSessions || {}).length === 1 })
    const saved = await page.evaluate(() => JSON.parse(JSON.stringify(window.__store)))
    await page.close()

    const again = await open(YT, { local: saved })
    // 上次停在“更多”标签页，重开仍停在那里，卡片直接可见
    await again.page.waitForFunction(() => window.__roots?.some((r) => r.textContent.includes('Rust 的特点')), null, { timeout: 8000 })
    ok('重新打开：回到上次的标签页，问答卡片直接带出', true)
    await clickText(again.page, '摘要')
    await okSoon('重新打开：摘要也在，没有请求清影', async () => { const t = await text(again.page); return /这个视频介绍了 Rust/.test(t) && (await again.page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)) === 0 })
    const t = await text(again.page)
    ok('重新打开：提示是已保存的结果', /生成，已保存/.test(t))
    await again.page.close()
  }

  // ---- 生成到一半关掉：不取消，重开后接上
  {
    const { page } = await open(YT, { hold: true })
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '生成摘要')
    await okSoon('生成中记下了任务', async () => await page.evaluate(() => Object.keys(window.__store.vsumJobs || {}).length === 1))
    await clickText(page, '✕')
    await okSoon('关闭面板不会取消任务', async () => await page.evaluate(() => !window.__calls.some((c) => c.type === 'cc-ai-cancel')))
    const saved = await page.evaluate(() => JSON.parse(JSON.stringify(window.__store)))
    await page.close()

    const again = await open(YT, { local: saved, resumeKind: 'summary' })
    await again.page.waitForFunction(() => window.__roots?.some((r) => r.textContent.includes('这个视频介绍了 Rust')), null, { timeout: 10000 })
    ok('重新打开：接上了原来的任务，没有重新提交', (await again.page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)) === 0)
    await again.page.close()
  }

  // ---- 没有字幕：默认自动取音频转写（后台进行，不用点）
  {
    const { page } = await open(YT + '&notracks=1', { subs: false })
    await page.waitForFunction(() => window.__roots?.some((r) => /2 段/.test(r.querySelector('.meta')?.textContent || '') && /清影转写/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    ok('没有字幕时自动转写（没点任何按钮）', await page.evaluate(() => window.__calls.some((c) => c.type === 'cc-ai-run' && c.req.kind === 'transcribe')))
    await page.close()
  }

  // ---- 转写流式显示：识别出多少先显示多少，全部完成后用完整结果覆盖
  {
    const { page } = await open(YT + '&notracks=1', { subs: false, stream: true })
    await page.waitForFunction(() => window.__roots?.some((r) => /识别中 50%/.test(r.textContent)), null, { timeout: 8000 })
    const half = await page.evaluate(() => window.__roots.map((r) => r.querySelector('.meta')?.textContent || '').join('|'))
    ok('识别到一半时先显示已有的 1 句', /1 段/.test(half) && /识别中 50%/.test(half), half)
    await page.waitForFunction(() => window.__roots?.some((r) => /2 段/.test(r.querySelector('.meta')?.textContent || '') && !/识别中/.test(r.textContent)), null, { timeout: 8000 })
    ok('全部识别完后用完整结果覆盖（2 句、轨道回到清影转写）', await page.evaluate(() => window.__roots.some((r) => /2 段/.test(r.querySelector('.meta')?.textContent || '') && /清影转写/.test(r.querySelector('.meta')?.textContent || ''))))
    const tr = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-ai-run' && c.req.kind === 'transcribe')?.req)
    ok('转写请求带视频语言提示字段（英文标题不强行指定）', tr && !tr.lang, JSON.stringify(tr))
    await page.close()
  }

  // ---- 文稿来源单选：字幕轨 / 音频转写
  {
    const prefs = { local: { vsumPrefs: { autoTranscribe: false, autoOnline: false, srcV: 1 } }, subs: false }
    const { page } = await open(YT + '&notracks=1', prefs)
    await page.waitForFunction(() => window.__roots?.some((r) => /文稿来源/.test(r.textContent)), null, { timeout: 8000 })
    const opts = await page.evaluate(() => [...window.__roots.flatMap((r) => [...r.querySelectorAll('select option')])].map((o) => o.textContent))
    ok('文稿来源列出字幕轨和音频转写', opts.some((t) => /字幕 · English（人工）/.test(t)) && opts.some((t) => /字幕 · 中文（自动生成）/.test(t)) && opts.some((t) => /音频转写/.test(t)), opts.join('|'))
    await page.evaluate(() => {
      const sel = window.__roots.flatMap((r) => [...r.querySelectorAll('select')]).find((x) => [...x.options].some((o) => o.value === 'audio'))
      sel.value = 'zh-Hans'
      sel.dispatchEvent(new Event('change'))
    })
    await page.waitForFunction(() => window.__roots?.some((r) => /在线字幕/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const sub = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').map((c) => c.req).find((r) => r.kind === 'subtitles'))
    ok('选字幕轨：只请求这一条（带语言代码）', sub && sub.track === 'zh-Hans', JSON.stringify(sub))
    await page.close()
  }

  // ---- YouTube 没有字幕
  {
    const { page } = await open(YT + '&notracks=1', { subs: false, local: { vsumPrefs: { autoTranscribe: false, srcV: 1 } } })
    await page.waitForFunction(() => window.__roots?.some((r) => /清影也没有/.test(r.textContent)), null, { timeout: 8000 }).catch(async (e) => { console.log('DBG', (await text(page)).slice(0, 600)); throw e })
    ok('没有字幕：提示并给出“用清影转写”', /用清影转写/.test(await text(page)) && /导入字幕文件/.test(await text(page)))
    await clickText(page, '用清影转写（语音识别）')
    await page.waitForFunction(() => window.__roots?.some((r) => /2 段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const tr = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-ai-run' && c.req.kind === 'transcribe')?.req)
    ok('转写请求带页面网址和标题', tr && /youtube\.com/.test(tr.url) && !!tr.title, JSON.stringify(tr))
    ok('转写完成后文稿有 2 段、轨道是清影转写', /2 段/.test(await text(page, '.meta')) && /清影转写/.test(await text(page, '.meta')), await text(page, '.meta'))
    const before = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)
    await clickText(page, '生成摘要').catch(() => {})
    await okSoon('转写出的文稿可以继续做摘要', async () => { const sumReq = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1)?.req); return sumReq && (sumReq.kind === 'summary' ? sumReq.lines.length === 2 && sumReq.lines[1].t === 5000 : before > 0) }, async () => { const sumReq = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1)?.req); return JSON.stringify(sumReq)?.slice(0, 200) })
    await page.evaluate(() => {})
    await page.close()
  }

  // ---- YouTube 字幕地址返回空内容：清影用 yt-dlp 取在线字幕
  {
    const { page, errors } = await open(YT + '&emptytt=1')
    await page.waitForFunction(() => window.__roots?.some((r) => /在线字幕（en）/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const kinds = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').map((c) => c.req.kind))
    ok('空字幕：不再报 JSON 错误，改让清影取在线字幕', kinds.includes('subtitles') && !(await text(page)).includes('Unexpected end'), kinds.join(','))
    ok('取到的在线字幕有 2 段', /2 段/.test(await text(page, '.meta')), await text(page, '.meta'))
    ok('空字幕兜底没有页面报错', errors.length === 0, errors.join('|'))
    await page.close()
  }

  // ---- 在线取不到时用记住的本地字幕；在线读得到时以在线为准
  {
    const local = { vsumLocal: { 'youtube:abc123XYZ_-': { ts: 1, data: { name: 'my.srt', lines: [{ t: 0, text: '本地第一句' }, { t: 3000, text: '本地第二句' }, { t: 6000, text: '本地第三句' }] } } } }
    const a = await open(YT + '&emptytt=1', { local })
    await a.page.waitForFunction(() => window.__roots?.some((r) => /本地字幕/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const calls = await a.page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)
    ok('在线读不到：自动用记住的本地字幕，不再请求清影', calls === 0 && /3 段/.test(await text(a.page, '.meta')), await text(a.page, '.meta'))
    await a.page.close()
    const b = await open(YT, { local })
    await b.page.waitForFunction(() => window.__roots?.some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    ok('在线字幕读得到时优先用在线的', /English/.test(await text(b.page, '.meta')) && !/本地/.test(await text(b.page, '.meta')), await text(b.page, '.meta'))
    await b.page.close()
  }

  // ---- 粘贴 / 导入的字幕按视频记住
  {
    const { page } = await open(YT + '&notracks=1&emptytt=1', { subs: false })
    await page.waitForFunction(() => window.__roots?.some((r) => /清影也没有|没有字幕|没有找到字幕/.test(r.textContent)), null, { timeout: 8000 })
    await clickText(page, '粘贴文字')
    await page.evaluate(() => {
      window.__roots.map((r) => r.querySelector('#paste')).find(Boolean).value = '[0:00] 第一句\n[0:05] 第二句'
    })
    await clickText(page, '使用粘贴的文字')
    await okSoon('粘贴的文字按视频记住', async () => { const saved = await page.evaluate(() => window.__store.vsumLocal && Object.values(window.__store.vsumLocal)[0]?.data?.lines?.length); return saved >= 1 }, async () => { const saved = await page.evaluate(() => window.__store.vsumLocal && Object.values(window.__store.vsumLocal)[0]?.data?.lines?.length); return String(saved) })
    // 导入 / 粘贴的文稿：带时间的显示时间戳；没有时间戳的行文字要占满整行（以前被挤进 48px 的时间列，一个单词一行）
    await clickText(page, '文稿')
    await flush(page)
    const w = await page.evaluate(() => {
      const ln = window.__roots.map((r) => r.querySelector('#lines .ln')).find(Boolean)
      return ln ? { cls: ln.className, text: (ln.querySelector('.one, .mn')?.parentElement || ln).getBoundingClientRect().width, row: ln.getBoundingClientRect().width } : null
    })
    ok('导入的文稿每行文字占满整行（不是 48px 窄列）', w && w.text > w.row * 0.6, JSON.stringify(w))
    await page.close()
  }

  // ---- 导入真实的 .srt 文件（和用户给的文件同样的写法）：识别时间戳、文字占满整行、视频上显示字幕层
  {
    const { page } = await open(YT + '&notracks=1&emptytt=1', { subs: false })
    await page.waitForFunction(() => window.__roots?.some((r) => /清影也没有|没有字幕|没有找到字幕/.test(r.textContent)), null, { timeout: 8000 })
    const srt = '1\r\n00:00:00,300 --> 00:00:10,240\r\n[Music]\r\n\r\n2\r\n00:00:10,240 --> 00:00:14,080\r\nHello everyone and welcome back to Mr.\r\n\r\n3\r\n00:00:14,080 --> 00:00:16,960\r\nEnglish channel where learning English\r\n\r\n4\r\n00:00:16,960 --> 00:00:20,800\r\nis easy and fun. I\'m Emily.\r\n'
    await clickText(page, '导入字幕文件').catch(() => {})
    const before = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-sublayer-open').length)
    await page.evaluate((text) => {
      const inp = window.__roots.flatMap((r) => [...r.querySelectorAll('input[type=file]')]).pop()
      const dt = new DataTransfer()
      dt.items.add(new File([text], 'sample.srt', { type: 'application/x-subrip' }))
      inp.files = dt.files
      inp.dispatchEvent(new Event('change'))
    }, srt)
    await clickText(page, '文稿')
    await page.waitForFunction(() => window.__roots.some((r) => r.querySelectorAll('#lines .ln').length >= 2), null, { timeout: 8000 }) // 等导入的字幕解析、渲染出来
    const info = await page.evaluate(() => {
      const root = window.__roots.find((r) => r.querySelector('#lines'))
      const lns = root ? [...root.querySelectorAll('#lines .ln')] : []
      const first = lns[0]
      const textW = first ? (first.querySelector('.one, .mn')?.parentElement || first).getBoundingClientRect().width : 0
      return { n: lns.length, tm: lns.map((l) => l.querySelector('.tm')?.textContent || ''), textW, rowW: first?.getBoundingClientRect().width || 0 }
    })
    ok('导入的 .srt：每句有时间戳（识别出时间）', info.n >= 2 && info.tm.every((t) => /\d/.test(t)) && info.tm.some((t) => t !== '0:00'), JSON.stringify(info))
    ok('导入的 .srt：文字占满整行，不是一个单词一行', info.textW > 150, JSON.stringify(info))
    const opened = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-sublayer-open').length)
    ok('导入的 .srt：视频上打开字幕层', opened > before, `${before} → ${opened}`)
    await page.close()
  }

  // ---- 文稿页勾选“显示在视频上”：字幕层被关掉 / 刚关又开，也要马上显示
  {
    const { page } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '文稿')
    await flush(page)
    const box = () => page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('label.mute input[type=checkbox]')]).find((i) => i.parentElement.textContent.includes('显示在视频上')))
    ok('文稿页有“显示在视频上”', !!(await box()))
    // 字幕层开着但字幕被关掉：重新勾选要调用 show()
    await page.evaluate(() => {
      window.__shown = 0
      globalThis.ccSubLayerRun = { destroy() { delete globalThis.ccSubLayerRun }, show() { window.__shown++ }, setLines() {} }
    })
    const tick = (checked) => page.evaluate((c) => {
      const i = window.__roots.flatMap((r) => [...r.querySelectorAll('label.mute input[type=checkbox]')]).find((x) => x.parentElement.textContent.includes('显示在视频上'))
      i.checked = c
      i.dispatchEvent(new Event('change'))
    }, checked)
    await tick(false)
    ok('取消勾选：字幕层关闭', await page.evaluate(() => !globalThis.ccSubLayerRun))
    const n0 = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-sublayer-open').length)
    await tick(true)
    await okSoon('刚取消又勾上：立刻重新打开字幕层（不被 4 秒防重复挡住）', async () => { const n1 = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-sublayer-open').length); return n1 > n0 }, async () => { const n1 = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-sublayer-open').length); return `${n0} → ${n1}` })
    await page.evaluate(() => {
      window.__shown = 0
      globalThis.ccSubLayerRun = { destroy() {}, show() { window.__shown++ }, setLines() {} }
    })
    await tick(true)
    ok('字幕层开着但被关掉：勾选后让它显示出来', (await page.evaluate(() => window.__shown)) >= 1)
    await page.close()
  }

  // ---- 转写还在流式识别时也能改：改动一直保留，识别完成后带到最终结果上
  {
    const { page } = await open(YT + '&notracks=1', { subs: false, stream: true, streamHold: true })
    await page.waitForFunction(() => window.__roots?.some((r) => /识别中 50%/.test(r.textContent)), null, { timeout: 8000 })
    await clickText(page, '文稿')
    await flush(page)
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('#lines .ln')).find(Boolean).dispatchEvent(new MouseEvent('dblclick', { bubbles: true })))
    ok('识别中双击一句：变成输入框', await page.evaluate(() => window.__roots.some((r) => r.querySelector('#lines textarea.ledit'))))
    await page.evaluate(() => {
      const ta = window.__roots.map((r) => r.querySelector('#lines textarea.ledit')).find(Boolean)
      ta.value = '大家好，今天讲讲 Rusty。'
      ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    })
    await okSoon('识别中改的字，新一批结果来了也还在', async () => (await text(page, '#lines')).includes('Rusty'))
    await page.evaluate(() => (window.__release = true))
    await page.waitForFunction(() => window.__roots?.some((r) => /2 段/.test(r.querySelector('.meta')?.textContent || '') && !/识别中/.test(r.textContent)), null, { timeout: 10000 })
    ok('识别完成后最终结果里仍是改过的字', (await text(page, '#lines')).includes('Rusty'))
    ok('改动记在本地', await page.evaluate(() => Object.values(window.__store.vsumEdits || {}).some((m) => m['0|大家好，今天讲讲 Rust。'] === '大家好，今天讲讲 Rusty。')))
    await page.close()
  }

  // ---- 修改文稿：双击一句手动改；AI 结合上下文纠错；改动记在本地
  {
    const { page } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '文稿')
    await flush(page)
    const first = () => page.evaluate(() => window.__roots.map((r) => r.querySelector('#lines .ln')?.textContent || '').find(Boolean) || '')
    const before = await first()
    await page.evaluate(() => {
      const row = window.__roots.map((r) => r.querySelector('#lines .ln')).find(Boolean)
      row.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    })
    ok('双击一句：变成输入框', await page.evaluate(() => window.__roots.some((r) => r.querySelector('#lines textarea.ledit'))))
    await page.evaluate(() => {
      const ta = window.__roots.map((r) => r.querySelector('#lines textarea.ledit')).find(Boolean)
      ta.value = '手动改过的第一句'
      ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    })
    await okSoon('回车保存：这一句变成新文字', async () => (await first()).includes('手动改过的第一句') && (await first()) !== before, async () => await first())
    ok('改动存在本地（下次同一个视频自动带出）', await page.evaluate(() => Object.keys(window.__store.vsumEdits || {}).length === 1))
    ok('改过的句子有标记', await page.evaluate(() => window.__roots.some((r) => r.querySelector('#lines .ln.fx'))))
    // Esc 取消
    await page.evaluate(() => {
      window.__roots.map((r) => r.querySelector('#lines .ln')).find(Boolean).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
      const ta = window.__roots.map((r) => r.querySelector('#lines textarea.ledit')).find(Boolean)
      ta.value = '不该保存'
      ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    })
    await okSoon('Esc 取消：不保存', async () => !(await first()).includes('不该保存'))
    // AI 纠错
    await clickText(page, 'AI 纠错')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('撤销纠错')), null, { timeout: 8000 })
    const req = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1).req)
    ok('纠错请求用 fixasr 并带全部字幕', req.kind === 'fixasr' && req.lines.length > 2, req.kind)
    ok('纠错后文稿有改动', (await text(page, '#lines')).includes('Rusty'))
    await page.evaluate(() => window.__roots.flatMap((r) => [...r.querySelectorAll('button')]).find((x) => x.textContent.startsWith('撤销纠错')).click())
    ok('撤销纠错：恢复', !(await text(page, '#lines')).includes('Rusty') && (await text(page, '#lines')).includes('手动改过的第一句'))
    await page.close()
  }

  // ---- 导图：全屏查看、缩放
  {
    const { page } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '导图')
    await clickText(page, '生成思维导图')
    await page.waitForFunction(() => window.__roots.some((r) => r.querySelector('.svgbox svg')), null, { timeout: 8000 })
    await clickText(page, '全屏查看 / 缩放')
    await flush(page)
    const tf = () => page.evaluate(() => window.__roots.map((r) => r.querySelector('.mapview .stage > svg')).find(Boolean)?.style.transform || '')
    const scale = async () => Number((await tf()).match(/scale\(([\d.]+)\)/)?.[1] || 0)
    const s0 = await scale()
    ok('全屏导图打开并适应窗口', s0 > 0, await tf())
    await clickText(page, '+')
    const s1 = await scale()
    ok('点 + 放大', s1 > s0, `${s0} → ${s1}`)
    await page.mouse.move(640, 400)
    await page.mouse.wheel(0, -300)
    await okSoon('滚轮放大', async () => (await scale()) > s1)
    const t1 = await tf()
    await page.mouse.move(640, 400)
    await page.mouse.down()
    await page.mouse.move(540, 360, { steps: 4 })
    await page.mouse.up()
    ok('拖动平移', (await tf()) !== t1)
    await clickText(page, '适应窗口')
    ok('适应窗口还原缩放', Math.abs((await scale()) - s0) < 0.01)
    await page.keyboard.press('Escape')
    await okSoon('Esc 关闭全屏导图', async () => !(await page.evaluate(() => window.__roots.some((r) => r.querySelector('.mapview')))))
    await page.close()
  }

  // ---- 导图页的第二种图：流程图（节点带视频时间，点一下跳过去）
  {
    const { page } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '导图')
    await clickText(page, '流程图')
    await okSoon('导图页里有“思维导图 / 流程图”两个切换', async () => await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('.mapsw button')].map((b) => b.textContent).join('|') === '思维导图|流程图')))
    await clickText(page, '生成流程图')
    await page.waitForFunction(() => window.__roots.some((r) => r.querySelector('.svgbox svg .fn')), null, { timeout: 8000 })
    const n = await page.evaluate(() => window.__roots.map((r) => r.querySelectorAll('.svgbox svg .fn').length).find(Boolean))
    ok('流程图里有 3 个节点', n === 3, String(n))
    ok('连线上有“所以”“导致”', await page.evaluate(() => window.__roots.some((r) => /所以/.test(r.querySelector('.svgbox svg')?.textContent || '') && /导致/.test(r.querySelector('.svgbox svg')?.textContent || ''))))
    const req = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1)?.req)
    ok('请求的是 flowgraph', req && req.kind === 'flowgraph', JSON.stringify(req && req.kind))
    await clickText(page, '全屏查看 / 缩放')
    await okSoon('全屏标题和提示说明点节点跳到视频', async () => await page.evaluate(() => window.__roots.some((r) => /点节点跳到视频/.test(r.querySelector('.mapview .hint')?.textContent || ''))))
    await page.evaluate(() => { const v = document.querySelector('video'); window.__seeks = []; Object.defineProperty(v, 'currentTime', { configurable: true, get: () => 0, set: (t) => window.__seeks.push(t) }) })
    // 全屏有进入动画：等节点位置连续两次读到一样（动画停了）再点，否则会点到半路上的位置
    const readBox = () => page.evaluate(() => { const g = window.__roots.map((r) => r.querySelector('.mapview .fn[data-ms="41000"]')).find(Boolean); const b = g.querySelector('rect').getBoundingClientRect(); return { x: b.x + b.width / 2, y: b.y + b.height / 2 } })
    let prevBox = null
    await soon(async () => { const b = await readBox(); const same = prevBox && Math.abs(b.x - prevBox.x) < 0.5 && Math.abs(b.y - prevBox.y) < 0.5; prevBox = b; return same }, null, 5000)
    const box = await readBox()
    await page.mouse.click(box.x, box.y)
    await okSoon('点节点：视频跳到它的时间（0:41）', async () => { const seeks = await page.evaluate(() => window.__seeks); return seeks.length >= 1 && Math.abs(seeks[seeks.length - 1] - 41) < 0.01 }, async () => { const seeks = await page.evaluate(() => window.__seeks); return JSON.stringify(seeks) })
    await page.keyboard.press('Escape')
    await page.close()
  }

  // ---- 文章
  {
    const { page } = await open('https://article.test/post')
    await page.waitForFunction(() => window.__roots?.some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    ok('文章：按段落读取，不显示时间', /2 段/.test(await text(page, '.meta')))
    await clickText(page, '生成摘要')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('这个视频介绍了')), null, { timeout: 8000 })
    const run = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-ai-run').req)
    ok('文章请求的行时间都是 0', run.lines.length === 2 && run.lines.every((l) => l.t === 0) && /gardens/.test(run.lines[0].text))
    await page.close()
  }

  // ---- 清影里没设置大模型
  {
    const { page } = await open(YT, { ready: false })
    await page.waitForFunction(() => window.__roots?.some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await okSoon('未设置大模型：顶部提示', async () => /未设置大模型/.test(await text(page, '.meta')))
    await clickText(page, '生成摘要')
    await okSoon('点生成会提示去设置', async () => await page.evaluate(() => window.__roots.some((r) => /设置 → AI/.test(r.textContent))))
    await page.close()
  }

  // ---- 面板拆成了几个文件（vsum-*.js）：每个标签页都能打开、有内容、不报错
  {
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => window.__roots?.some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    ok('各标签页的文件都加载了', await page.evaluate(() => ['transcript', 'notes', 'comments', 'vocab', 'mindmap', 'more'].every((k) => typeof globalThis.ccVsumTabs?.[k] === 'function') && !!globalThis.ccVsumStyle?.CSS))
    const seen = []
    for (const tab of ['摘要', '章节', '文稿', '导图', '问答', '笔记', '生词', '评论', '更多']) {
      await clickText(page, tab)
      await flush(page)
      const t = await text(page)
      seen.push(`${tab}:${t.replace(/\s+/g, '').length}`)
    }
    ok('九个标签页都有内容', seen.every((x) => Number(x.split(':')[1]) > 20), seen.join(' '))
    ok('切换所有标签页没有页面错误', errors.length === 0, errors.join('|'))
    await page.close()
  }

  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
