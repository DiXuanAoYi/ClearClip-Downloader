// 页面功能的后台部分（接上 bg-page.js）：内容脚本和扩展页面发来的消息处理（HANDLERS 与 bg-guard.js 的规则表一一对应）。

// ---------- 来自内容脚本和扩展页面的消息 ----------

const HANDLERS = {
  'cc-sniffed': async (_m, sender) => {
    const list = arrangeSniffed(await getSniffed(sender.tab.id)).filter((x) => x.kind !== 'image')
    return { items: list.map((x) => ({ url: x.url, kind: x.kind, label: sniffLabel(x) })) }
  },
  'cc-send-page': async (_m, sender) => {
    await firstTimeSync(sender.tab.url)
    return { pairing: await sendText(await tidyUrl(sender.tab.url)) }
  },
  'cc-send-media': async (m, sender) => ({ pairing: await sendText(sniffedPayload({ url: String(m.url) }, sender.tab.url, sender.tab.title)) }),
  'cc-send-batch': async (m, sender) => {
    const r = await sendBatch(Array.isArray(m.items) ? m.items : [], sender.tab)
    if (!r.sent && r.failed) throw new Error(r.error || '发送失败')
    return r
  },
  // 扩展页面（弹窗、总览）发来的：带着标签页编号
  'cc-run': async (m) => {
    const tab = await api.tabs.get(Number(m.tabId))
    if (m.action === 'overview') await openOverview(tab)
    else if (m.action === 'picker') await startPicker(tab)
    else if (m.action === 'vsum') await startVsum(tab)
    else if (m.action === 'sublayer') await startSubLayer(tab)
    else if (m.action === 'shot') await startShot(tab)
    else if (m.action === 'batch-links') return await sendPageVideoLinks(tab)
    else if (m.action === 'webtrans') await webTransDo(tab, String(m.mode || 'toggle'))
    else if (m.action === 'excludepick') await injectFiles(tab.id, ccFiles.GROUPS.excludePick)
    else throw new Error('未知操作')
    return {}
  },
  'cc-send-items': async (m) => ({ ...(await sendBatch(Array.isArray(m.items) ? m.items : [], { url: m.page || '', title: m.title || '' })) }),
  // 视频上的音效面板点“更多音效设置”：在新标签页打开弹窗的音效页
  'cc-open-audio': async () => (await api.tabs.create({ url: api.runtime.getURL('popup.html#audio') }), {}),
  'cc-engines': async () => ({ engines: await getEngines() }),
  'cc-save': async (m) => {
    if (!/^https?:/i.test(String(m.url))) throw new Error('这张图片不能保存')
    // 地址没有后缀或后缀不对时，按真实格式命名（不再保存成 .bin）
    const real = (await probeMedia(String(m.url), String(m.referrer || ''))).ext
    const cur = ccSniff.extOfUrl(String(m.url))
    const filename = real && real !== cur ? ccSniff.fileNameFor(String(m.url), real) : undefined
    await api.downloads.download({ url: String(m.url), filename, conflictAction: 'uniquify' })
    return {}
  },
  // 总览：探测图片的真实格式和大小
  'cc-probe-media': async (m) => ({ ...(await probeMedia(String(m.url), String(m.referrer || ''))) }),
  'cc-search': async (m) => {
    await searchImage(m.url, m.engine)
    return {}
  },
  // 页面保存：弹窗发来（带标签页编号）；选区截图由页面里的选框发来
  'cc-page-save': async (m) => ({ ...(await pageSave(String(m.action), await api.tabs.get(Number(m.tabId)), '')) }),
  // 截图翻译：裁出选区，连同“翻译成什么语言”一起交给页面
  'cc-shot': async (m, sender) => {
    const cfg = await getTransCfg()
    const r = m.rect || {}
    const rect = { x: Number(r.x) || 0, y: Number(r.y) || 0, w: Number(r.w) || 0, h: Number(r.h) || 0 }
    if (rect.w < 8 || rect.h < 8) throw new Error('选区太小')
    return { ...(await cropForTranslate(sender.tab, rect, Number(m.vw) || 0)), lang: ccTrans.promptName(cfg.target) }
  },
  'cc-area': async (m, sender) => {
    await captureArea(sender.tab, m.rect, m.vw)
    return {}
  },
  // 在当前页面立刻启用解除限制（不用刷新）：弹窗里给这个网站打开开关之后调用
  'cc-unlock-now': async (m) => {
    const tab = await api.tabs.get(Number(m.tabId))
    if (!isWebPage(tab)) throw new Error('当前页面不能使用这个功能')
    await api.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, files: ccFiles.GROUPS.unlock, world: 'MAIN' })
    return {}
  },
  // 视频摘要：面板把请求交给清影（本机），由清影调用设置里的大模型；长任务提交后轮询
  'cc-ai-status': async () => ({ ...(await call('/api/v1/ai/status')) }),
  // 页面里直接取字幕文件被跨域规则拦住时，由后台代取（只允许 B 站的字幕域名，只读，不带 Cookie）
  'cc-fetch-sub': async (m) => {
    const u = new URL(String(m.url || '').replace(/^\/\//, 'https://').replace(/^http:/, 'https:'))
    if (!/(^|\.)(hdslb\.com|bilibili\.com|biliapi\.net)$/.test(u.hostname)) throw new Error('不支持的地址')
    const r = await fetch(u.href, { credentials: 'omit' })
    if (!r.ok) throw new Error(`HTTP ${r.status}`)
    return { text: await r.text() }
  },
  // 清影界面语言：问一次清影并记下来；清影没开就用上次记下的
  'cc-app-lang': async () => {
    try {
      const r = await call('/api/ping')
      if (r.lang) await api.storage.local.set({ appLang: r.lang })
    } catch {
      // 清影没开：用上次的
    }
    return { lang: (await api.storage.local.get('appLang')).appLang || '' }
  },
  // 字幕学习：生词本（清影桌面端保存）
  'cc-open-highlights': async () => (await api.tabs.create({ url: api.runtime.getURL('highlights.html') }), {}),
  // 视频缓存：同步到清影 / 查询是否允许自动同步
  'cc-videocache': async (m) => {
    if (m.op === 'put') return { ...(await vcache.put(m.payload && typeof m.payload === 'object' ? m.payload : {})) }
    if (m.op === 'info') return { ...(await vcache.info(String(m.url))) }
    throw new Error('不支持的操作')
  },
  'cc-vocab': async (m) => {
    const enc = encodeURIComponent
    switch (m.op) {
      case 'words':
        return { ...(await call('/api/v1/vocab/words')) }
      case 'list':
        return { ...(await call(`/api/v1/vocab/list?q=${enc(String(m.q || '').slice(0, 80))}&state=${enc(String(m.state || 'all'))}`)) }
      case 'add':
        return { ...(await call('/api/v1/vocab/add', m.word && typeof m.word === 'object' ? m.word : {})) }
      case 'remove':
        return { ...(await call('/api/v1/vocab/remove', { id: Number(m.id) || 0 })) }
      case 'unfamiliar':
        return { ...(await call('/api/v1/vocab/unfamiliar', { words: Array.isArray(m.words) ? m.words.slice(0, 400) : [], minRank: Number(m.minRank) || 0 })) }
      case 'export':
        return { ...(await call(`/api/v1/vocab/export?format=${enc(String(m.format || 'csv'))}`)) }
      case 'clip':
        return { ...(await call('/api/v1/vocab/clip', { word: String(m.word || '').slice(0, 80), data: String(m.data || '') })) }
      case 'clips':
        return { ...(await call('/api/v1/vocab/clips')) }
      default:
        throw new Error('不支持的操作')
    }
  },
  // 总览：让清影里的大模型起名字。依据：网页标题、页面正文摘录（理解主题）、每项的图片说明 / 周围文字，以及图片里识出的文字（OCR，本机识字）。
  'cc-ai-name': async (m) => {
    const items = (Array.isArray(m.items) ? m.items : []).slice(0, 60)
    if (!items.length) return { names: [] }
    // 先给图片读字（最多 12 张，3 张并行；读不出来就不带，不影响起名）
    const ocr = new Array(items.length).fill('')
    if (m.ocr !== false) {
      const todo = items.map((x, i) => i).filter((i) => items[i].kind === 'image' && /^https?:/i.test(items[i].url || '')).slice(0, 12)
      let next = 0
      const worker = async () => {
        while (next < todo.length) {
          const i = todo[next++]
          try {
            ocr[i] = await ocrOfImage(items[i].url, String(m.page || ''))
          } catch {
            /* 这张读不了：只用其他信息 */
          }
        }
      }
      await Promise.all([worker(), worker(), worker()])
    }
    const lines = items.map((x, t) => ({
      t,
      text: [x.kind, x.alt && `alt: ${x.alt}`, x.cap && `caption: ${x.cap}`, x.text && x.text !== x.alt && `link: ${x.text}`, x.orig && `file: ${x.orig}`, x.near && `near: ${x.near}`, ocr[t] && `OCR: ${ocr[t]}`].filter(Boolean).join(' | ').slice(0, 420),
    }))
    const sub = await call('/api/v1/ai/run', { kind: 'namer', title: String(m.title || '').slice(0, 200), text: String(m.excerpt || '').slice(0, 1500), lang: String(m.lang || ''), lines })
    const deadline = Date.now() + 2 * 60_000
    for (;;) {
      await sleep(450)
      const j = await call(`/api/v1/ai/job?id=${encodeURIComponent(String(sub.id))}`)
      if (j.state === 'done') return { names: j.result?.names || [], ocr: ocr.map((t) => !!t) }
      if (j.state === 'error') throw new Error(j.error || 'AI 命名失败了')
      if (j.state === 'cancelled') throw new Error('已取消')
      if (Date.now() > deadline) throw new Error('AI 命名超时')
    }
  },
  'cc-ai-run': async (m) => ({ ...(await call('/api/v1/ai/run', m.req && typeof m.req === 'object' ? m.req : {})) }),
  'cc-ai-job': async (m) => ({ ...(await call(`/api/v1/ai/job?id=${encodeURIComponent(String(m.id))}`)) }),
  'cc-ai-cancel': async (m) => ({ ...(await call(`/api/v1/ai/cancel?id=${encodeURIComponent(String(m.id))}`, {})) }),
  // 网页翻译：内容脚本把一批文字发来，返回译文
  'cc-tr': async (m) => {
    const cfg = await getTransCfg()
    const texts = (Array.isArray(m.texts) ? m.texts : []).map((t) => String(t)).slice(0, 60)
    if (!texts.length) return { texts: [], cached: 0 }
    const engine = ccTrans.ENGINES.includes(m.engine) ? m.engine : ccTrans.pickEngine(cfg)
    const p = m.page && typeof m.page === 'object' ? m.page : null
    const page = p ? { host: String(p.host || '').slice(0, 100), title: String(p.title || ''), sample: String(p.sample || '') } : null
    return translateTexts(texts, { target: String(m.target || ccTrans.engineTarget(cfg, engine)), engine, page, domain: cfg.domain, extra: ccTrans.enginePrompt(cfg, engine) })
  },
  // 独立翻译窗：同一段文字用几个翻译服务各翻一遍，并排给出结果（任何一个失败不影响别的）
  'cc-trbox': async (m) => {
    const cfg = await getTransCfg()
    const text = String(m.text || '').slice(0, 5000)
    if (!text.trim()) return { results: [] }
    const target = String(m.target || cfg.target)
    const wanted = Array.isArray(m.engines) ? m.engines.filter((e) => ccTrans.ENGINES.includes(e)) : []
    const engines = wanted.length ? wanted : ccTrans.activeEngines(cfg)
    const results = await Promise.all(
      engines.map(async (engine) => {
        const name = ccTrans.ENGINE_NAMES[engine] || engine
        try {
          const r = await translateTexts([text], { target, engine, page: null, domain: false, extra: ccTrans.enginePrompt(cfg, engine) })
          return { engine, name, text: r.texts[0] }
        } catch (e) {
          return { engine, name, error: String(e?.message || e) }
        }
      }),
    )
    return { results, engines: ccTrans.activeEngines(cfg), names: ccTrans.ENGINE_NAMES }
  },
  // 划词工具条的“翻译”：在选区所在的那个页面（框架）里打开网页翻译的划词卡片；那里还没有翻译脚本就先注入
  'cc-wt-sel': async (m, sender) => {
    const tab = sender?.tab
    if (!tab) throw new Error('找不到页面')
    const frameId = sender.frameId ?? 0
    const r0 = m.rect && typeof m.rect === 'object' ? m.rect : null
    const rect = r0 ? { left: Number(r0.left) || 0, right: Number(r0.right) || 0, top: Number(r0.top) || 0, bottom: Number(r0.bottom) || 0 } : null
    const msg = { type: 'cc-wt', action: 'selection', text: String(m.text || '').slice(0, 5000), rect }
    const ready = (r) => r && typeof r === 'object' && 'on' in r
    try {
      if (ready(await api.tabs.sendMessage(tab.id, msg, { frameId }))) return {}
    } catch {
      // 还没注入
    }
    await api.scripting.executeScript({ target: { tabId: tab.id, frameIds: [frameId] }, files: ccFiles.GROUPS.trans })
    await sleep(60)
    if (!ready(await api.tabs.sendMessage(tab.id, msg, { frameId }))) throw new Error('划词翻译没有启动（这个页面可能不允许扩展运行脚本）')
    return {}
  },
  'cc-open-transbox': async (m, sender) => (await openTransBox(sender?.tab || null, String(m.text || '').slice(0, 5000)), {}),
  // 视频摘要读到字幕后，把文稿作为字幕层叠在视频上
  'cc-sublayer-open': async (_m, sender) => (sender?.tab && (await startSubLayer(sender.tab)), {}),
  // 视频胶囊菜单里的“视频摘要”（再注入一次会关掉，所以胶囊只在没打开时显示这一项）
  // 探测 stub 发现本 frame 里有 <video>/<audio>：按当前开关把视频类常驻功能的主体注入进来
  'cc-stub-load': async (_m, sender) => {
    const f = normalizeFeatures(await getFeatures())
    const ctx = { takeover: await getTakeover() }
    const loaded = []
    for (const s of RESIDENT_SCRIPTS) {
      if (!s.lazy || !s.need(f, ctx)) continue
      await api.scripting.executeScript({ target: { tabId: sender.tab.id, frameIds: [sender.frameId || 0] }, files: s.js })
      loaded.push(s.id)
    }
    return { loaded }
  },
  'cc-vsum-open': async (_m, sender) => (sender?.tab && (await startVsum(sender.tab)), {}),
  'cc-tr-clear': async () => (await clearTransCache(), {}),
  // 把笔记存进清影的“摘录”；字数上限在设置里（0 = 不限制，默认）
  'cc-note': async (m) => {
    const { noteLimit } = await api.storage.local.get('noteLimit')
    const max = Math.max(0, Math.floor(Number(noteLimit) || 0))
    const text = String(m.text || '')
    return { pairing: await sendText(max ? text.slice(0, max) : text, { note: true }) }
  },
  // 视频画布被跨域污染时，用浏览器对可见区域的截图代替（只在用户点了截图之后）
  'cc-capture': async (_m, sender) => ({ dataUrl: await api.tabs.captureVisibleTab(sender.tab.windowId, { format: 'png' }) }),
}

api.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  const handler = msg && typeof msg.type === 'string' && Object.prototype.hasOwnProperty.call(HANDLERS, msg.type) ? HANDLERS[msg.type] : null
  if (!handler) return false
  // 入口检查（bg-guard.js）：发送者是本扩展、需要标签页的消息来自内容脚本、字段按白名单整理并限长；不合规返回统一的错误
  const g = ccGuard.check(msg, sender, api.runtime.id)
  if (!g.ok) {
    sendResponse({ ok: false, error: g.error, code: g.code })
    return false
  }
  handler(g.msg, sender)
    .then((r) => sendResponse({ ok: true, ...r }))
    .catch(async (e) => {
      if (e.code === 'unconfigured') await openSetup()
      sendResponse({ ok: false, error: e.message })
    })
  return true
})
