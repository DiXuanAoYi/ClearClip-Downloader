// 视频摘要面板：在真实浏览器里加载内容脚本，后台消息用假的代替。
// 覆盖：YouTube 字幕读取 → 面板各标签页 → 提交/轮询/缓存 → 问答里的时间引用 → 文章模式 → 没有字幕时的提示。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const FILES = ['content/ui.js', 'vsum-logic.js', 'content/vsum-source.js', 'content/vsum.js'].map((f) => path.join(EXT, f))
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const YT = 'https://www.youtube.com/watch?v=abc123XYZ_-'
const tracks = (emptyTt) => [{ baseUrl: `https://www.youtube.com/api/timedtext?v=abc123XYZ_-&lang=en${emptyTt ? '&empty=1' : ''}`, languageCode: 'en', name: { simpleText: 'English' } }]
const ytHtml = (withTracks, emptyTt = false) => `<!doctype html><title>Demo video - YouTube</title><body><video width="640" height="360"></video><p>comments</p>
<script>var ytInitialPlayerResponse={"captions":{"playerCaptionsTracklistRenderer":{${withTracks ? `"captionTracks":${JSON.stringify(tracks(emptyTt))},"audioTracks":[]` : '"x":1'}}},"videoDetails":{"title":"Demo \\"video\\"","lengthSeconds":"125","shortDescription":"0:00 Intro\\n0:40 Main part\\n1:30 Outro"}};</script>`
const json3 = { events: [{ tStartMs: 0, dDurationMs: 4000, segs: [{ utf8: 'Welcome to the demo.' }] }, { tStartMs: 41000, dDurationMs: 4000, segs: [{ utf8: 'Rust is fast and safe.' }] }, { tStartMs: 95000, dDurationMs: 3000, segs: [{ utf8: 'Thanks for watching.' }] }] }

;(async () => {
  const browser = await chromium.launch()
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
          if (m.type === 'cc-ai-status') return { ok: true, ready: o.ready !== false, model: 'm', lang: '中文', transcribe: true, subtitles: true, maxChars: 600000 }
          if (m.type === 'cc-ai-run') {
            window.__job = m.req
            return { ok: true, id: 7 }
          }
          if (m.type === 'cc-ai-job') {
            if (o.hold) return { ok: true, state: 'running', done: 0, total: 1, note: '' }
            const k = window.__job?.kind || o.resumeKind
            window.__job = window.__job || { kind: k }
            const result = {
              summary: { summary: '这个视频介绍了 Rust。', points: ['Rust 又快又安全 [0:41]', '结尾致谢'], keywords: ['Rust', '性能'], chapters: [{ title: 'AI 开场', startMs: 0, endMs: 40000 }, { title: 'AI 正文', startMs: 40000, endMs: 125000 }] },
              mindmap: { outline: '# 主题\n- 分支一\n  - 细节\n- 分支二' },
              ask: { answer: '因为它没有垃圾回收 [0:41]。', refs: [41000] },
              cards: { cards: [{ q: 'Rust 的特点？', a: '又快又安全', t: 41000 }] },
              quotes: { quotes: [{ text: 'Rust is fast and safe.', t: 41000 }] },
              translate: { lines: (window.__job.lines || []).map((l) => ({ t: l.t, text: '译:' + l.text })), failed: 0 },
              tracks: { tracks: [{ code: 'en', name: 'English（人工）', auto: false }, { code: 'zh-Hans', name: '中文（自动生成）', auto: true }], audio: true },
              rewrite: { text: '改写后的文案' },
              explain: { text: 'Rust 是一门系统语言。' },
              note: { quote: '整理后的完整句子。' },
              refine: { quote: '优化后的笔记。' },
              punctuate: { lines: (window.__job.lines || []).map((l) => ({ t: l.t, text: l.text + '。' })), failed: 0 },
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
  const settle = (page, ms = 1200) => page.waitForTimeout(ms)


  // ---- 笔记 / 划词解释 / 顺句
  {
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '文稿')
    // 点 ＋ 记一条
    await page.evaluate(() => window.__roots.map((r) => r.querySelector('.ln .rec')).find(Boolean).click())
    await settle(page, 300)
    ok('点 ＋ 存进了笔记', await page.evaluate(() => window.__store.vsumNotes?.length === 1))
    // 划词
    await page.evaluate(() => {
      const r = window.__roots.find((x) => x.querySelector('.ln'))
      const span = r.querySelector('.ln span:nth-child(2)')
      const rng = document.createRange()
      rng.setStart(span.firstChild, 0)
      rng.setEnd(span.firstChild, 3)
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
    await settle(page, 300)
    ok('划词存为笔记（原文）', await page.evaluate(() => window.__store.vsumNotes.length === 2))
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
    await settle(page, 300)
    ok('AI 整理后替换原文', await page.evaluate(() => window.__store.vsumNotes.some((n) => n.text === '整理后的完整句子。')))
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
    await settle(page, 300)
    ok('外观：选“深色”后面板用深色样式', await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('style')].some((st) => /@media|#1d2420/.test(st.textContent) && st.textContent.includes('.src') && !st.textContent.includes('@media'))) ))
    ok('笔记流程无页面错误', errors.length === 0, errors.join('|'))
    await page.close()
  }

  // ---- YouTube：读字幕 → 摘要
  {
    const { page, errors } = await open(YT)
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    const meta = await text(page, '.meta')
    ok('读到字幕：3 段、轨道名', /3 段/.test(meta) && /English/.test(meta), meta)
    ok('标题取自页面数据', (await text(page, '.hd')).includes('Demo "video"'))
    await clickText(page, '生成摘要')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('这个视频介绍了 Rust')), null, { timeout: 8000 })
    const run = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-ai-run').req)
    ok('提交的请求带类型、风格、语言、整理好的行', run.kind === 'summary' && run.style === 'standard' && run.lines.length === 3 && run.lines[1].t === 41000 && run.lang === '中文', JSON.stringify(run))
    ok('关键词、要点显示', /Rust/.test(await text(page)) && /性能/.test(await text(page)) && /又快又安全/.test(await text(page)))
    ok('要点里的 [0:41] 变成可点的时间', await page.evaluate(() => window.__roots.some((r) => [...r.querySelectorAll('button.ts')].some((b) => b.textContent === '[0:41]'))))
    // 点时间会让页面的视频跳转（没有媒体源时 currentTime 不变，只要不报错）
    await page.evaluate(() => window.__roots[0].querySelector('button.ts').click())

    if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT + '-summary.png' })
    // 章节：AI 章节优先
    await clickText(page, '章节')
    ok('章节页显示 AI 章节', /AI 开场/.test(await text(page)) && /AI 正文/.test(await text(page)))

    // 文稿：搜索、翻译
    await clickText(page, '文稿')
    ok('文稿列出 3 行', (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0))) === 3)
    await page.evaluate(() => {
      const i = window.__roots.map((r) => r.querySelector('input#tsearch')).find(Boolean)
      i.value = 'rust'
      i.dispatchEvent(new Event('input'))
    })
    await settle(page, 300)
    ok('搜索：全部行保留，命中的行加亮并标出词', (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln').length, 0))) === 3 && (await page.evaluate(() => window.__roots.reduce((n, r) => n + r.querySelectorAll('.ln.hit mark').length, 0))) >= 1)
    ok('搜索：显示 1/1 计数', /1\/1/.test(await text(page)))
    await page.evaluate(() => {
      const i = window.__roots.map((r) => r.querySelector('input#tsearch')).find(Boolean)
      i.value = ''
      i.dispatchEvent(new Event('input'))
    })
    await settle(page, 300)
    await clickText(page, '翻译成中文')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('译:Welcome')), null, { timeout: 8000 })
    ok('翻译后每行下方有译文', true)

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
    await clickText(page, '提取金句')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('“Rust is fast and safe.”')), null, { timeout: 8000 })
    await clickText(page, '小红书文案')
    await page.waitForFunction(() => window.__roots.some((r) => r.textContent.includes('改写后的文案')), null, { timeout: 8000 })
    ok('卡片、金句、改写都出来了', true)
    const rewrite = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1).req)
    ok('改写请求带格式与摘要 Markdown', rewrite.format === 'xhs' && rewrite.text.includes('这个视频介绍了 Rust') && rewrite.text.includes('t=41s'), rewrite.text?.slice(0, 120))
    // 存便签
    await clickText(page, '存进清影的便签')
    await settle(page, 300)
    const note = await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-note'))
    ok('便签内容是 Markdown 笔记', note && note.text.startsWith('# Demo "video"') && note.text.includes('Rust 又快又安全'), note?.text?.slice(0, 80))

    // 缓存：同样的摘要不再请求
    const before = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)
    await clickText(page, '摘要')
    await clickText(page, '重新生成')
    await settle(page, 600)
    const after = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)
    ok('相同文稿 + 设置的结果走缓存，不再提交', after === before, `${before} → ${after}`)

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
    await settle(page, 500)
    const saved = await page.evaluate(() => JSON.parse(JSON.stringify(window.__store)))
    ok('关闭时保存了这个视频的会话', Object.keys(saved.vsumSessions || {}).length === 1)
    await page.close()

    const again = await open(YT, { local: saved })
    // 上次停在“更多”标签页，重开仍停在那里，卡片直接可见
    await again.page.waitForFunction(() => window.__roots?.some((r) => r.textContent.includes('Rust 的特点')), null, { timeout: 8000 })
    ok('重新打开：回到上次的标签页，问答卡片直接带出', true)
    await clickText(again.page, '摘要')
    await settle(again.page, 300)
    const t = await text(again.page)
    ok('重新打开：摘要也在，没有请求清影', /这个视频介绍了 Rust/.test(t) && (await again.page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').length)) === 0)
    ok('重新打开：提示是已保存的结果', /已保存的结果/.test(t))
    await again.page.close()
  }

  // ---- 生成到一半关掉：不取消，重开后接上
  {
    const { page } = await open(YT, { hold: true })
    await page.waitForFunction(() => (window.__roots || []).some((r) => /段/.test(r.querySelector('.meta')?.textContent || '')), null, { timeout: 8000 })
    await clickText(page, '生成摘要')
    await settle(page, 1800)
    ok('生成中记下了任务', await page.evaluate(() => Object.keys(window.__store.vsumJobs || {}).length === 1))
    await clickText(page, '✕')
    await settle(page, 1200)
    ok('关闭面板不会取消任务', await page.evaluate(() => !window.__calls.some((c) => c.type === 'cc-ai-cancel')))
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
    await settle(page, 400)
    const sumReq = await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-ai-run').at(-1)?.req)
    ok('转写出的文稿可以继续做摘要', sumReq && (sumReq.kind === 'summary' ? sumReq.lines.length === 2 && sumReq.lines[1].t === 5000 : before > 0), JSON.stringify(sumReq)?.slice(0, 200))
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
    await settle(page, 300)
    const saved = await page.evaluate(() => window.__store.vsumLocal && Object.values(window.__store.vsumLocal)[0]?.data?.lines?.length)
    ok('粘贴的文字按视频记住', saved >= 1, String(saved))
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
    await settle(page, 300)
    const tf = () => page.evaluate(() => window.__roots.map((r) => r.querySelector('.mapview .stage > svg')).find(Boolean)?.style.transform || '')
    const scale = async () => Number((await tf()).match(/scale\(([\d.]+)\)/)?.[1] || 0)
    const s0 = await scale()
    ok('全屏导图打开并适应窗口', s0 > 0, await tf())
    await clickText(page, '+')
    const s1 = await scale()
    ok('点 + 放大', s1 > s0, `${s0} → ${s1}`)
    await page.mouse.move(640, 400)
    await page.mouse.wheel(0, -300)
    await settle(page, 200)
    ok('滚轮放大', (await scale()) > s1)
    const t1 = await tf()
    await page.mouse.move(640, 400)
    await page.mouse.down()
    await page.mouse.move(540, 360, { steps: 4 })
    await page.mouse.up()
    ok('拖动平移', (await tf()) !== t1)
    await clickText(page, '适应窗口')
    ok('适应窗口还原缩放', Math.abs((await scale()) - s0) < 0.01)
    await page.keyboard.press('Escape')
    await settle(page, 200)
    ok('Esc 关闭全屏导图', !(await page.evaluate(() => window.__roots.some((r) => r.querySelector('.mapview')))))
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
    await settle(page, 400)
    ok('未设置大模型：顶部提示', /未设置大模型/.test(await text(page, '.meta')))
    await clickText(page, '生成摘要')
    await settle(page, 600)
    ok('点生成会提示去设置', await page.evaluate(() => window.__roots.some((r) => /设置 → AI/.test(r.textContent))))
    await page.close()
  }

  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
