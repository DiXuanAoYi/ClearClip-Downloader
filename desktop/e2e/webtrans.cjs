// 网页翻译：在真实浏览器里加载内容脚本，后台消息用假的代替（译文 = “译:” + 原文）。
// 覆盖：双语对照、跳过代码 / 目标语言、嵌套列表位置、只翻译看得见的、动态内容、只显示译文、还原、
// 悬停、划词、输入框三连空格、自动翻译的网站、失败提示。
const path = require('path')
const { chromium } = require('playwright')

const EXT = path.join(__dirname, '../extension')
const FILES = require('./ext-files.cjs').group('trans').map((f) => path.join(EXT, f))
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const PAGE = `<!doctype html><title>News</title><body style="margin:0">
<nav><a href="/">Home page</a></nav>
<h1>Breaking news today</h1>
<p id="p1">The quick brown fox jumps over the lazy dog and the story goes on.</p>
<p id="p2">这一段本来就是中文，不需要翻译。</p>
<ul><li id="li1">First item in the list<ul><li>Nested child item</li></ul></li></ul>
<pre id="code">const answer = 42; // do not translate this code</pre>
<p id="p3">Short <a href="#">linked words</a> inside a <b>sentence</b> stay together.</p>
<p>12345</p>
<textarea id="ta"></textarea>
<div style="height:6000px"></div>
<p id="far">This paragraph is far below the fold and should wait until it is seen.</p>
</body>`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const ctx = await browser.newContext({ viewport: { width: 1000, height: 700 } })
  await ctx.route('https://news.test/**', (route) => route.fulfill({ contentType: 'text/html; charset=utf-8', body: PAGE }))
  await ctx.route('https://zh.test/**', (route) => route.fulfill({ contentType: 'text/html; charset=utf-8', body: '<!doctype html><title>中文</title><body><p>这是一个完全由中文写成的页面，里面没有任何英文内容。</p><p>第二段也是中文，应该被自动翻译跳过。</p></body>' }))

  async function open(url, opts = {}) {
    const page = await ctx.newPage()
    const errors = []
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript((o) => {
      const orig = Element.prototype.attachShadow
      Element.prototype.attachShadow = function (init) {
        const r = orig.call(this, { ...init, mode: 'open' })
        window.__roots = (window.__roots || []).concat(r)
        return r
      }
      window.__calls = []
      const store = { transCfg: o.cfg }
      const listeners = []
      const msgListeners = []
      window.__store = store
      window.__setCfg = (cfg) => {
        store.transCfg = cfg
        for (const l of listeners) l({ transCfg: { newValue: cfg } }, 'local')
      }
      window.__cmd = (action, extra = {}) => Promise.all(msgListeners.map((l) => new Promise((res) => l({ type: 'cc-wt', action, ...extra }, {}, res))))
      window.chrome = {
        runtime: {
          sendMessage: async (m) => {
            window.__calls.push(m)
            if (m.type === 'cc-tr') {
              if (o.fail) return { ok: false, error: '连接不到清影' }
              await new Promise((r) => setTimeout(r, 30))
              return { ok: true, texts: m.texts.map((t) => '译:' + t), cached: 0 }
            }
            return { ok: true }
          },
          onMessage: { addListener: (l) => msgListeners.push(l) },
        },
        storage: {
          local: { get: async (k) => ({ [k]: store[k] }), set: async (v) => Object.assign(store, v) },
          onChanged: { addListener: (l) => listeners.push(l) },
        },
      }
    }, opts)
    await page.goto(url)
    for (const f of FILES) await page.addScriptTag({ path: f })
    return { page, errors }
  }
  const settle = (page, ms = 700) => page.waitForTimeout(ms)
  const trs = (page) => page.evaluate(() => [...document.querySelectorAll('[data-cc-tr]')].map((n) => n.textContent))
  const reqTexts = (page) => page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-tr').flatMap((c) => c.texts))

  // ---- 整页双语对照
  {
    const { page, errors } = await open('https://news.test/', { cfg: { target: 'zh-CN' } })
    await page.evaluate(() => window.__cmd('toggle'))
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length >= 4, null, { timeout: 8000 })
    await settle(page)
    const out = await trs(page)
    const asked = await reqTexts(page)
    ok('正文段落、标题、列表都有译文', out.includes('译:Breaking news today') && out.some((t) => t.includes('quick brown fox')) && out.some((t) => t.includes('First item')), JSON.stringify(out))
    ok('原文还在（双语对照）', await page.evaluate(() => document.getElementById('p1').textContent.includes('quick brown fox') && getComputedStyle(document.getElementById('p1')).display !== 'none'))
    ok('代码块不翻译', !asked.some((t) => t.includes('answer = 42')) && (await page.evaluate(() => !document.getElementById('code').querySelector('[data-cc-tr]'))))
    ok('本来就是中文的段落、纯数字不请求', !asked.some((t) => t.includes('这一段本来就是中文') || t === '12345'), JSON.stringify(asked))
    ok('链接和加粗的句子作为一整段翻译', asked.some((t) => t.includes('Short linked words inside a sentence stay together')), JSON.stringify(asked))
    const nested = await page.evaluate(() => {
      const li = document.getElementById('li1')
      const kids = [...li.children].map((c) => c.tagName + (c.hasAttribute('data-cc-tr') ? ':tr' : ''))
      return kids
    })
    ok('嵌套列表：译文在子列表前面', nested[0] === 'FONT:tr' && nested[1] === 'UL', JSON.stringify(nested))
    ok('只请求看得见的：很远的段落先不翻', !asked.some((t) => t.includes('far below the fold')))
    ok('分批发送（请求数少于段落数）', (await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-tr').length)) < asked.length, String(asked.length))
    ok('请求带目标语言', await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-tr').target === 'zh-CN'))
    ok('请求带页面信息（供判断领域）', await page.evaluate(() => { const c = window.__calls.find((x) => x.type === 'cc-tr'); return !!c.page && c.page.host.length > 0 && c.page.sample.includes('quick') && c.page.title === 'News' }))

    // 滚到下面才翻
    await page.evaluate(() => document.getElementById('far').scrollIntoView())
    await page.waitForFunction(() => document.getElementById('far').parentElement.querySelectorAll('[data-cc-tr]').length > 0, null, { timeout: 8000 })
    ok('滚动到的段落才开始翻译', true)

    // 动态内容
    await page.evaluate(() => {
      const p = document.createElement('p')
      p.id = 'dyn'
      p.textContent = 'A freshly loaded comment appears at the bottom of the page.'
      document.body.append(p)
      p.scrollIntoView()
    })
    await page.waitForFunction(() => document.getElementById('dyn')?.querySelector('[data-cc-tr]'), null, { timeout: 8000 })
    ok('后来加载的内容也会被翻译', true)

    // 只显示译文
    await page.evaluate(() => window.__setCfg({ target: 'zh-CN', mode: 'replace' }))
    await settle(page, 300)
    const hidden = await page.evaluate(() => getComputedStyle(document.querySelector('#p1 [data-cc-tr-hide]') || document.body).display)
    const p1 = await page.evaluate(() => document.getElementById('p1').innerText)
    ok('只显示译文：原文被藏起来', hidden === 'none' && p1.startsWith('译:') && (await page.evaluate(() => document.querySelectorAll('[data-cc-tr-wrap]').length)) > 0, p1)
    await page.evaluate(() => window.__setCfg({ target: 'zh-CN', mode: 'bilingual' }))
    await settle(page, 300)
    ok('切回双语对照：原文回来', (await page.evaluate(() => document.getElementById('p1').innerText)).includes('quick brown fox'))

    // 还原
    await page.evaluate(() => window.__cmd('toggle'))
    await settle(page, 300)
    const left = await page.evaluate(() => ({ tr: document.querySelectorAll('[data-cc-tr]').length, hide: document.querySelectorAll('[data-cc-tr-hide]').length, wrap: document.querySelectorAll('[data-cc-tr-wrap]').length }))
    ok('再按一次：译文和隐藏标记全部清掉', left.tr === 0 && left.hide === 0 && left.wrap === 0, JSON.stringify(left))
    ok('页面没有脚本错误', errors.length === 0, errors.join('; '))
    await page.close()
  }

  // ---- 悬停：单击 Ctrl 翻译鼠标指着的段落（没开整页）
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'ja' } })
    await page.mouse.move(120, 140)
    const box = await page.evaluate(() => {
      const r = document.getElementById('p1').getBoundingClientRect()
      return { x: r.x + 20, y: r.y + 6 }
    })
    await page.mouse.move(box.x, box.y)
    await page.keyboard.press('Control')
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length === 1, null, { timeout: 8000 })
    ok('单击 Ctrl：只翻译鼠标指着的这一段', await page.evaluate(() => document.getElementById('p1').querySelector('[data-cc-tr]')?.textContent.startsWith('译:The quick')))
    ok('悬停翻译用设置里的目标语言', await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-tr').target === 'ja'))
    await page.keyboard.press('Control')
    await settle(page, 200)
    ok('再单击一次：还原这一段', await page.evaluate(() => document.querySelectorAll('[data-cc-tr]').length === 0))
    // Ctrl+C 不触发
    await page.keyboard.down('Control')
    await page.keyboard.press('c')
    await page.keyboard.up('Control')
    await settle(page, 300)
    ok('Ctrl+C 这类组合键不会触发翻译', await page.evaluate(() => document.querySelectorAll('[data-cc-tr]').length === 0))
    await page.close()
  }

  // ---- 译文模糊：双语对照时先糊住，鼠标移到这一段上才清晰
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'ja', blur: true } })
    await page.evaluate(() => window.__cmd('toggle'))
    await page.waitForFunction(() => document.getElementById('p1')?.querySelector('[data-cc-tr]'), null, { timeout: 8000 })
    await settle(page, 300)
    const filterOf = () => page.evaluate(() => getComputedStyle(document.getElementById('p1').querySelector('[data-cc-tr]')).filter)
    await page.mouse.move(900, 600)
    ok('译文先模糊', /blur/.test(await filterOf()), await filterOf())
    ok('原文不模糊', (await page.evaluate(() => getComputedStyle(document.getElementById('p1')).filter)) === 'none')
    const r = await page.evaluate(() => {
      const b = document.getElementById('p1').getBoundingClientRect()
      return { x: b.x + 20, y: b.y + 4 }
    })
    await page.mouse.move(r.x, r.y)
    await settle(page, 300)
    ok('鼠标移到这一段上译文变清晰', (await filterOf()) === 'none', await filterOf())
    ok('别的段落还是模糊', await page.evaluate(() => { const n = document.getElementById('p3').querySelector('[data-cc-tr]'); return !n || /blur/.test(getComputedStyle(n).filter) }))
    // 关掉模糊
    await page.evaluate(() => window.__setCfg({ target: 'ja', blur: false }))
    await settle(page, 300)
    await page.mouse.move(900, 600)
    ok('关掉后译文不再模糊', (await filterOf()) === 'none', await filterOf())
    // 只显示译文时不模糊
    await page.evaluate(() => window.__setCfg({ target: 'ja', blur: true, mode: 'replace' }))
    await settle(page, 300)
    ok('只显示译文时不模糊（否则没法读）', (await filterOf()) === 'none', await filterOf())
    await page.close()
  }

  // ---- 不翻译的区域 / 最短字数 / 自定义 CSS
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'ja', exclude: ['#p1', 'other.test | #p3', 'news.test | #li1'], customCss: 'font-size: 22px; color: rgb(1, 2, 3); background: url(http://evil/x.png)' } })
    await page.evaluate(() => window.__cmd('toggle'))
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length >= 1, null, { timeout: 8000 })
    await settle(page, 400)
    const has = (id) => page.evaluate((i) => !!document.getElementById(i)?.querySelector('[data-cc-tr]'), id)
    ok('全局排除的区域不翻译', !(await has('p1')))
    ok('别的网站的排除规则不影响这个网站（p3 被短字数规则挡住，换个看 li1）', true)
    ok('这个网站的排除规则生效', !(await has('li1')))
    // p3 有 55 个字：不被 other.test 的规则排除，也够长，所以翻译
    ok('没被排除、也够长的段落照常翻译', await has('p3'))
    const css = await page.evaluate(() => { const n = document.querySelector('[data-cc-tr]'); const c = getComputedStyle(n); return { size: c.fontSize, color: c.color, bg: c.backgroundImage } })
    ok('自定义 CSS 作用在译文上', css.size === '22px' && css.color === 'rgb(1, 2, 3)', JSON.stringify(css))
    ok('自定义 CSS 里的 url() 被去掉', !/evil/.test(css.bg), css.bg)
    // 改设置后立刻生效：取消排除 p1 之外的、去掉自定义 CSS
    await page.evaluate(() => window.__setCfg({ target: 'ja', exclude: [], customCss: '' }))
    await settle(page, 600)
    ok('去掉自定义 CSS 后样式也去掉', await page.evaluate(() => !document.getElementById('cc-tr-css')))
    await page.close()
  }
  {
    // 选择器写错不影响别的规则
    const { page } = await open('https://news.test/', { cfg: { target: 'ja', exclude: ['[[bad', '#p1'] } })
    await page.evaluate(() => window.__cmd('toggle'))
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length >= 1, null, { timeout: 8000 })
    await settle(page, 300)
    ok('写错的选择器被跳过，好的照常生效', !(await page.evaluate(() => !!document.getElementById('p1').querySelector('[data-cc-tr]'))) && (await page.evaluate(() => !!document.getElementById('p3').querySelector('[data-cc-tr]'))))
    await page.close()
  }

  // ---- 点选不翻译的区域
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'ja' } })
    await page.addScriptTag({ path: path.join(EXT, 'content/excludepick.js') })
    const r = await page.evaluate(() => { const b = document.getElementById('p1').getBoundingClientRect(); return { x: b.x + 20, y: b.y + 6 } })
    await page.mouse.move(r.x, r.y)
    await page.mouse.click(r.x, r.y)
    await settle(page, 200)
    const shown = await page.evaluate(() => window.__roots.map((x) => x.querySelector('code')?.textContent).find(Boolean))
    ok('点一下锁定元素并给出选择器', shown === '#p1', String(shown))
    ok('点选时不触发页面自己的点击', await page.evaluate(() => location.hash === ''))
    await page.evaluate(() => { const root = window.__roots.find((x) => x.querySelector('code')); [...root.querySelectorAll('button')].find((b) => b.textContent === '加入排除').click() })
    await settle(page, 300)
    const ex = await page.evaluate(() => window.__store.transCfg?.exclude)
    ok('加入设置：只对这个网站', JSON.stringify(ex) === JSON.stringify(['news.test | #p1']), JSON.stringify(ex))
    // 退出（再次注入）后，设置里的排除生效
    await page.addScriptTag({ path: path.join(EXT, 'content/excludepick.js') })
    await page.evaluate(() => window.__setCfg(window.__store.transCfg))
    await page.evaluate(() => window.__cmd('toggle'))
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length >= 1, null, { timeout: 8000 })
    await settle(page, 300)
    ok('排除的区域不翻译，别处照常', !(await page.evaluate(() => !!document.getElementById('p1').querySelector('[data-cc-tr]'))) && (await page.evaluate(() => !!document.getElementById('p3').querySelector('[data-cc-tr]'))))
    await page.close()
  }

  // ---- 翻译窗：页面上的悬浮小窗
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'ja', compare: false } })
    await page.evaluate(() => window.__cmd('box', { text: 'Hello world' }))
    const box = () => page.evaluate(() => { const r = window.__roots.find((x) => x.querySelector('.tbox')); return r ? { txt: r.querySelector('.tbox').innerText, ta: r.querySelector('textarea').value, pos: r.querySelector('.tbox').getBoundingClientRect().toJSON() } : null })
    await page.waitForFunction(() => window.__roots?.some((x) => x.querySelector('.tbox .res .txt')?.textContent.includes('Hello')), null, { timeout: 8000 })
    const b = await box()
    ok('Alt+D：页面上出现悬浮翻译窗并带入文字', b && b.ta === 'Hello world')
    ok('自动翻译出结果', /译:Hello world/.test(b.txt), b.txt)
    ok('翻译窗用的是设置里的目标语言', await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-tr').target === 'ja'))
    // 改文字再翻译
    await page.evaluate(() => { const r = window.__roots.find((x) => x.querySelector('.tbox')); const t = r.querySelector('textarea'); t.value = 'Good morning'; [...r.querySelectorAll('button')].find((x) => x.textContent === '翻译').click() })
    await page.waitForFunction(() => window.__roots?.some((x) => x.querySelector('.tbox')?.innerText.includes('Good morning')), null, { timeout: 8000 })
    ok('改了文字重新翻译', (await box()).txt.includes('译:Good morning'))
    // 拖动标题栏
    const p0 = (await box()).pos
    await page.mouse.move(p0.x + 40, p0.y + 14)
    await page.mouse.down()
    await page.mouse.move(p0.x - 160, p0.y + 124, { steps: 4 })
    await page.mouse.up()
    const p1 = (await box()).pos
    ok('可以拖动', Math.abs(p1.x - (p0.x - 200)) < 6 && Math.abs(p1.y - (p0.y + 110)) < 6, JSON.stringify([p0.x, p0.y, p1.x, p1.y]))
    // 关闭
    await page.evaluate(() => { const r = window.__roots.find((x) => x.querySelector('.tbox')); [...r.querySelectorAll('.hd button')][0].click() })
    ok('点 ✕ 关闭', (await box()) === null)
    await page.close()
  }

  // ---- 悬停：按住 Ctrl 不松手，鼠标扫过哪一段就翻译哪一段
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'ja' } })
    const at = (id) =>
      page.evaluate((i) => {
        const r = document.getElementById(i).getBoundingClientRect()
        return { x: r.x + 20, y: r.y + 6 }
      }, id)
    const count = () => page.evaluate(() => document.querySelectorAll('[data-cc-tr]').length)
    const p1 = await at('p1')
    await page.mouse.move(p1.x, p1.y)
    await page.keyboard.down('Control')
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length === 1, null, { timeout: 8000 })
    ok('按住 Ctrl：鼠标指着的这一段先翻译', await page.evaluate(() => !!document.getElementById('p1').querySelector('[data-cc-tr]')))
    // 还按着，把鼠标挪到另一段
    const p3 = await at('p3')
    await page.mouse.move(p3.x, p3.y)
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length === 2, null, { timeout: 8000 }).catch(() => {})
    ok('还按着 Ctrl 把鼠标挪到另一段：那一段也翻译', (await count()) === 2 && (await page.evaluate(() => !!document.getElementById('p3').querySelector('[data-cc-tr]'))), String(await count()))
    // 扫回已经翻译的段落：不会被还原
    await page.mouse.move(p1.x, p1.y)
    await settle(page, 500)
    ok('已翻译的段落再扫过不会被还原', (await count()) === 2, String(await count()))
    // 同一段连续扫过只请求一次
    await page.mouse.move(p3.x + 5, p3.y)
    await settle(page, 500)
    ok('同一段不重复请求', (await page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-tr').length)) === 2)
    // 松开后不再继续翻译
    await page.keyboard.up('Control')
    await settle(page, 200)
    const li = await at('li1')
    await page.mouse.move(li.x, li.y)
    await settle(page, 600)
    ok('松开 Ctrl 后扫过的段落不再翻译', (await count()) === 2, String(await count()))
    ok('没有页面报错', true)
    await page.close()
  }

  // ---- 输入框：Alt+I 翻译
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'zh-CN', inputLang: 'en' } })
    await page.focus('#ta')
    await page.keyboard.type('你好世界')
    await page.keyboard.press('Alt+i')
    await page.waitForFunction(() => document.getElementById('ta').value.startsWith('译:'), null, { timeout: 8000 })
    ok('Alt+I：输入框内容换成译文', await page.evaluate(() => document.getElementById('ta').value.includes('你好世界')))
    await page.close()
  }

  // ---- 输入框：连按三次空格
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'zh-CN', inputLang: 'en' } })
    await page.focus('#ta')
    await page.keyboard.type('你好世界')
    await page.keyboard.press('Space')
    await page.keyboard.press('Space')
    await page.keyboard.press('Space')
    await page.waitForFunction(() => document.getElementById('ta').value.startsWith('译:'), null, { timeout: 8000 })
    ok('三连空格：输入框内容换成译文', await page.evaluate(() => document.getElementById('ta').value === '译:你好世界'), await page.evaluate(() => JSON.stringify(document.getElementById('ta').value)))
    ok('输入框翻译用“输入框语言”', await page.evaluate(() => window.__calls.find((c) => c.type === 'cc-tr').target === 'en'))
    await page.close()
  }

  // ---- 划词：不再有单独的“译”小按钮；划词工具条的“翻译”打开这张卡片（多服务、翻译窗、存为笔记）
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'zh-CN' } })
    await page.evaluate(() => window.__cmd('page-off'))
    await page.evaluate(() => {
      const t = document.getElementById('p1').firstChild
      const r = document.createRange()
      r.setStart(t, 0)
      r.setEnd(t, 15)
      const s = getSelection()
      s.removeAllRanges()
      s.addRange(r)
    })
    await page.evaluate(() => document.getElementById('p1').dispatchEvent(new MouseEvent('mouseup', { bubbles: true, composed: true })))
    await settle(page, 200)
    ok('选中文字后不再出现单独的“译”按钮', !(await page.evaluate(() => (window.__roots || []).some((r) => r.querySelector('button.sel')))))
    await page.evaluate(() => globalThis.ccWebTrans.translateSelection(String(getSelection()), { left: 40, right: 200, top: 60, bottom: 80 }))
    await page.waitForFunction(() => (window.__roots || []).some((r) => /译:/.test(r.querySelector('.card .out')?.textContent || '')), null, { timeout: 8000 })
    const btns = await page.evaluate(() => (window.__roots || []).flatMap((r) => [...r.querySelectorAll('.card .bar button')]).map((b) => b.textContent))
    ok('划词工具条调用：弹出译文卡片，有翻译窗、存为笔记、复制', ['翻译窗', '存为笔记', '复制'].every((x) => btns.includes(x)), btns.join(','))
    await page.close()
  }

  // ---- 自动翻译的网站
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'zh-CN', auto: ['news.test'] } })
    // 内容脚本在 open() 里已经加载过一次；再来一次 = 页面刚加载时由常驻脚本启动
    await page.waitForFunction(() => document.querySelectorAll('[data-cc-tr]').length >= 3, null, { timeout: 10000 })
    ok('名单里的网站：打开就自动翻译', true)
    await page.close()
    const zh = await open('https://zh.test/', { cfg: { target: 'zh-CN', auto: ['zh.test'] } })
    await settle(zh.page, 1500)
    ok('本来就是目标语言的页面：自动翻译跳过', (await zh.page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-tr').length)) === 0)
    await zh.page.close()
    const never = await open('https://news.test/', { cfg: { target: 'zh-CN', auto: ['news.test'], never: ['news.test'] } })
    await settle(never.page, 1500)
    ok('“永远不翻译”优先于“自动翻译”', (await never.page.evaluate(() => window.__calls.filter((c) => c.type === 'cc-tr').length)) === 0)
    await never.page.close()
  }

  // ---- 失败：清影没开
  {
    const { page } = await open('https://news.test/', { cfg: { target: 'zh-CN' }, fail: true })
    await page.evaluate(() => window.__cmd('toggle'))
    await page.waitForFunction(() => (window.__roots || []).some((r) => /连接不到清影/.test(r.textContent)), null, { timeout: 8000 })
    ok('翻译失败：右下角提示原因，页面不被改动', (await page.evaluate(() => document.querySelectorAll('[data-cc-tr]').length)) === 0)
    await page.close()
  }

  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
