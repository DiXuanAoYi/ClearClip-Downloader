// 网页翻译的后台部分：把一批文字交给清影（大模型）或免费的谷歌翻译，带本地缓存；打开 / 关闭页面翻译。

const TR_MAX_JOBS = 3
let trRunning = 0
const trWaiters = []
const trSlot = async () => {
  if (trRunning >= TR_MAX_JOBS) await new Promise((r) => trWaiters.push(r))
  trRunning++
}
const trFree = () => {
  trRunning--
  trWaiters.shift()?.()
}

async function getTransCfg() {
  const { transCfg } = await api.storage.local.get('transCfg')
  return ccTrans.normalizeCfg(transCfg)
}

// ---------- 缓存（{ key: [译文, 时间] }，内存里一份，改动后隔两秒存盘）----------
// 后台被回收时内存里的缓存会丢：新译出来的条目（dirty）同时记进会话存储，存盘任务也走 ccDefer（有 alarm 兜底），
// 后台重新起来后由存盘任务把它们并进缓存再落盘。

let trCache = null
const TR_DIRTY_KEY = 'trDirty'
async function trCacheMap() {
  if (!trCache) {
    try {
      const { transCache } = await api.storage.local.get('transCache')
      trCache = transCache && typeof transCache === 'object' ? transCache : {}
    } catch {
      trCache = {}
    }
  }
  return trCache
}
const trDirtyQueue = makeSerial()
/** 记下新译出来的条目并安排存盘。entries：{ key: [译文, 时间] } */
function trRemember(entries) {
  return trDirtyQueue(async () => {
    try {
      const { [TR_DIRTY_KEY]: old } = await sessionStore.get(TR_DIRTY_KEY)
      await sessionStore.set({ [TR_DIRTY_KEY]: { ...(old && typeof old === 'object' ? old : {}), ...entries } })
    } catch {
      // 会话存储写不了：只剩内存里这一份
    }
    await ccDefer.schedule('tr-flush', 2000)
  })
}
async function trFlush() {
  await trDirtyQueue(async () => {
    try {
      const cache = await trCacheMap()
      const { [TR_DIRTY_KEY]: dirty } = await sessionStore.get(TR_DIRTY_KEY)
      if (dirty && typeof dirty === 'object') Object.assign(cache, dirty)
      trCache = ccTrans.pruneCache(cache)
      await api.storage.local.set({ transCache: trCache })
      await sessionStore.remove(TR_DIRTY_KEY)
    } catch {
      // 存不了就算了
    }
  })
}
ccDefer.on('tr-flush', trFlush)

// ---------- 两个引擎 ----------

async function trClearclip(texts, target, context = '', host = '') {
  const sub = await call('/api/v1/ai/run', { kind: 'webtrans', lang: ccTrans.promptName(target), context, url: host ? `https://${host}` : '', lines: texts.map((text, t) => ({ t, text })) })
  const deadline = Date.now() + 5 * 60_000
  for (;;) {
    await sleep(450)
    const j = await call(`/api/v1/ai/job?id=${encodeURIComponent(String(sub.id))}`)
    if (j.state === 'done') {
      const lines = j.result?.lines || []
      return texts.map((src, i) => lines.find((l) => l.t === i)?.text || lines[i]?.text || src)
    }
    if (j.state === 'error') throw new Error(j.error || '翻译失败了')
    if (j.state === 'cancelled') throw new Error('已取消')
    if (Date.now() > deadline) throw new Error('翻译超时')
  }
}

/** 自定义接口（OpenAI 兼容：OpenAI、DeepSeek、通义、智谱、Ollama、LM Studio、各种中转…）。一批文字发成 JSON 数组，要求原样回一个同样长度的数组。 */
async function trCustomBatch(s, target, items, context) {
  const url = ccTrans.chatUrl(s.url)
  const body = {
    model: s.model,
    temperature: 0.2,
    messages: [
      { role: 'system', content: ccTrans.customSystem(ccTrans.promptName(target), s.prompt || '', context) },
      { role: 'user', content: JSON.stringify(items) },
    ],
  }
  const ctl = new AbortController()
  const timer = setTimeout(() => ctl.abort(), 90_000)
  let r
  try {
    r = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json', ...(s.key ? { Authorization: `Bearer ${s.key}` } : {}) }, body: JSON.stringify(body), signal: ctl.signal })
  } catch {
    throw new Error(`连不上自定义接口（${url}）。本机的 Ollama 需要设置环境变量 OLLAMA_ORIGINS=* 才允许浏览器扩展访问`)
  } finally {
    clearTimeout(timer)
  }
  if (r.status === 401 || r.status === 403) throw new Error('自定义接口拒绝了请求（密钥不对或没有权限）')
  if (r.status === 429) throw new Error('自定义接口限流了（HTTP 429），稍后再试')
  if (!r.ok) throw new Error(`自定义接口返回了错误（HTTP ${r.status}）：${(await r.text().catch(() => '')).slice(0, 160)}`)
  const j = await r.json().catch(() => null)
  const reply = j?.choices?.[0]?.message?.content
  if (!reply) throw new Error('自定义接口没有返回内容，检查模型名对不对')
  return reply
}

async function trCustom(texts, target, context = '') {
  const cfg = await getTransCfg()
  const s = cfg.services?.custom || {}
  if (!ccTrans.customReady(cfg)) throw new Error('自定义接口还没填好：在“设置 → 翻译”里填接口地址和模型名')
  const out = new Array(texts.length)
  const batches = ccTrans.splitBatches(texts)
  let next = 0
  const worker = async () => {
    while (next < batches.length) {
      const idx = batches[next++]
      const items = idx.map((i) => texts[i])
      let arr = ccTrans.parseArrayReply(await trCustomBatch(s, target, items, context), items.length)
      if (!arr && items.length > 1) {
        // 模型没按数组回、条数对不上：逐条重来一遍，慢一点但不会错位
        arr = []
        for (const one of items) arr.push(ccTrans.parseArrayReply(await trCustomBatch(s, target, [one], context), 1)?.[0] ?? one)
      }
      if (!arr) arr = [ccTrans.parseArrayReply(await trCustomBatch(s, target, items, context), 1)?.[0] ?? items[0]]
      idx.forEach((i, k) => (out[i] = arr[k]))
    }
  }
  await Promise.all(Array.from({ length: Math.min(3, batches.length) }, worker))
  return out
}

/** DeepLX（自己部署或别人提供）和 DeepL 官方 API 的兼容写法：POST {text, source_lang:'auto', target_lang}，回 {data} 或 {translations:[{text}]}。 */
async function trDeeplx(texts, target) {
  const s = (await getTransCfg()).services?.deeplx || {}
  if (!s.url) throw new Error('DeepLX 还没填好：在“设置 → 翻译”里填接口地址')
  const out = new Array(texts.length)
  let next = 0
  const worker = async () => {
    while (next < texts.length) {
      const i = next++
      let r
      try {
        r = await fetch(s.url, { method: 'POST', headers: { 'Content-Type': 'application/json', ...(s.key ? { Authorization: `Bearer ${s.key}` } : {}) }, body: JSON.stringify({ text: texts[i], source_lang: 'auto', target_lang: ccTrans.deeplLang(target) }) })
      } catch {
        throw new Error(`连不上 DeepLX（${s.url}）`)
      }
      if (r.status === 429) throw new Error('DeepLX 限流了（HTTP 429），稍后再试')
      if (!r.ok) throw new Error(`DeepLX 返回了错误（HTTP ${r.status}）`)
      const j = await r.json().catch(() => null)
      const t = j?.data ?? j?.translations?.[0]?.text
      if (typeof t !== 'string' || !t) throw new Error('DeepLX 没有返回译文')
      out[i] = t
    }
  }
  await Promise.all(Array.from({ length: Math.min(3, texts.length) }, worker))
  return out
}

/** 长文本按句子切成不超过 1500 字的几段（谷歌这个接口用 GET，太长会被拒）。 */
function trSplit(text, max = 1500) {
  if (text.length <= max) return [text]
  const out = []
  let cur = ''
  for (const s of text.split(/(?<=[.!?。！？\n])\s*/)) {
    if (cur && cur.length + s.length > max) {
      out.push(cur)
      cur = ''
    }
    cur += (cur ? ' ' : '') + s
    while (cur.length > max) {
      out.push(cur.slice(0, max))
      cur = cur.slice(max)
    }
  }
  if (cur) out.push(cur)
  return out
}

async function trGoogleOne(text, target) {
  const parts = []
  for (const piece of trSplit(text)) {
    let r
    try {
      r = await fetch(ccTrans.googleUrl(piece, target))
    } catch {
      throw new Error('连不上谷歌翻译，请检查网络，或在设置里改用“清影（大模型）”')
    }
    if (r.status === 429) throw new Error('谷歌翻译暂时限制了请求（HTTP 429），请稍后再试，或改用“清影（大模型）”')
    if (!r.ok) throw new Error(`谷歌翻译返回了错误（HTTP ${r.status}）`)
    const out = ccTrans.parseGoogle(await r.json().catch(() => null))
    if (!out) throw new Error('谷歌翻译没有返回译文')
    parts.push(out)
  }
  return parts.join(' ')
}

async function trGoogle(texts, target) {
  const out = new Array(texts.length)
  let next = 0
  const worker = async () => {
    while (next < texts.length) {
      const i = next++
      out[i] = await trGoogleOne(texts[i], target)
    }
  }
  await Promise.all(Array.from({ length: Math.min(4, texts.length) }, worker))
  return out
}

// ---------- 网页领域（翻译前先判断一次，按页面记住）----------

const trDomains = new Map() // 页面键 -> Promise<{domain, note}|null>
const trDomainFail = new Map() // 页面键 -> 失败时间；几分钟内不再重试，免得每一批都空等
const trDomainKey = (p) => `${p.host}|${String(p.title || '').slice(0, 80)}`

/** 看页面标题和一小段文字，让大模型判断领域并给出选词说明；失败返回 null（翻译照常进行）。 */
function pageDomain(page) {
  if (!page || !page.host || !String(page.sample || '').trim()) return Promise.resolve(null)
  const key = trDomainKey(page)
  if (Date.now() - (trDomainFail.get(key) || 0) < 5 * 60_000) return Promise.resolve(null)
  if (!trDomains.has(key)) {
    const job = (async () => {
      const sub = await call('/api/v1/ai/run', { kind: 'domain', title: String(page.title || '').slice(0, 200), url: `https://${page.host}`, text: String(page.sample).slice(0, 1500) })
      const deadline = Date.now() + 20_000
      for (;;) {
        await sleep(400)
        const j = await call(`/api/v1/ai/job?id=${encodeURIComponent(String(sub.id))}`)
        if (j.state === 'done') {
          const domain = String(j.result?.domain || '').trim()
          const note = String(j.result?.note || '').trim()
          return domain || note ? { domain, note } : null
        }
        if (j.state === 'error' || j.state === 'failed' || j.state === 'cancelled' || Date.now() > deadline) return null
      }
    })().catch(() => null)
    trDomains.set(key, job)
    if (trDomains.size > 50) trDomains.delete(trDomains.keys().next().value)
    job.then((v) => {
      if (v) return
      trDomains.delete(key)
      trDomainFail.set(key, Date.now())
      if (trDomainFail.size > 50) trDomainFail.delete(trDomainFail.keys().next().value)
    })
  }
  return trDomains.get(key)
}

// 模型 / AI 专家 / 术语库变了，应用端的 promptVersion 会变，旧译文不再命中缓存（60 秒内不重复问）
let pvMemo = { v: '', t: 0 }
async function promptVersion() {
  if (Date.now() - pvMemo.t < 60_000) return pvMemo.v
  try {
    const s = await call('/api/v1/ai/status')
    pvMemo = { v: String(s.promptVersion || ''), t: Date.now() }
  } catch {
    pvMemo = { v: pvMemo.v, t: Date.now() - 50_000 }
  }
  return pvMemo.v
}

/** 翻译一批文字。先查缓存，剩下的交给引擎，返回和输入一一对应的译文。 */
async function translateTexts(texts, { target, engine, page = null, domain = false, extra = '' }) {
  const dom = engine === 'clearclip' && domain ? await pageDomain(page) : null
  const ctx = [dom ? [dom.domain && `Domain: ${dom.domain}.`, dom.note].filter(Boolean).join(' ') : '', engine === 'clearclip' && extra ? `Additional requirement from the user: ${extra}` : ''].filter(Boolean).join(' ')
  const pv = engine === 'clearclip' ? await promptVersion() : ''
  const cm = engine === 'custom' ? (await getTransCfg()).services?.custom?.model || '' : ''
  const keyEngine = (engine === 'custom' ? `custom:${cm}` : dom?.domain ? `${engine}:${dom.domain.toLowerCase().slice(0, 40)}` : engine) + (pv ? `@${pv}` : '') + ((engine === 'clearclip' || engine === 'custom') && extra ? `#${[...extra].reduce((h, c) => (Math.imul(h, 31) + c.charCodeAt(0)) | 0, 7).toString(36)}` : '')
  const cache = await trCacheMap()
  const out = texts.map((t) => cache[ccTrans.cacheKey(keyEngine, target, t)]?.[0] ?? null)
  const todo = out.map((v, i) => (v == null ? i : -1)).filter((i) => i >= 0)
  let fallback = ''
  if (todo.length) {
    const src = todo.map((i) => texts[i])
    let res
    if (engine === 'google') res = await trGoogle(src, target)
    else if (engine === 'custom') res = await trCustom(src, target, ctx)
    else if (engine === 'deeplx') res = await trDeeplx(src, target)
    else {
      await trSlot()
      try {
        res = await trClearclip(src, target, ctx, page?.host || '')
      } catch (e) {
        // 清影没开：用你自己开着的备用服务（DeepLX / 自定义接口）。谷歌不自动顶上，免得网页文字在你不知道时发给谷歌
        const cfg = await getTransCfg()
        const backup = ccTrans.activeEngines(cfg).find((x) => x === 'deeplx' || x === 'custom')
        if (e.code !== 'offline' || !backup) throw e
        fallback = backup
        res = backup === 'deeplx' ? await trDeeplx(src, target) : await trCustom(src, target, '')
      } finally {
        trFree()
      }
    }
    const fresh = {}
    todo.forEach((i, k) => {
      out[i] = res[k]
      // 原样返回（没翻译）的不进缓存，免得以后换了模型也一直是原文
      if (!fallback && res[k] && res[k].trim() !== src[k].trim()) fresh[ccTrans.cacheKey(keyEngine, target, texts[i])] = [res[k], Date.now()]
    })
    Object.assign(cache, fresh)
    if (Object.keys(fresh).length) await trRemember(fresh)
  }
  return { texts: out, cached: texts.length - todo.length, domain: dom?.domain || '', fallback }
}

async function clearTransCache() {
  trCache = {}
  await ccDefer.cancel('tr-flush')
  await sessionStore.remove(TR_DIRTY_KEY).catch(() => {})
  await api.storage.local.remove('transCache')
}

// ---------- 打开 / 关闭页面翻译 ----------

/** 让标签页里的网页翻译做一件事：toggle / page-on / page-off / hover / selection（带 text）。还没注入就先注入。 */
async function webTransDo(tab, action, extra = {}) {
  if (!isWebPage(tab)) throw new Error('当前页面不能翻译')
  const send = () => api.tabs.sendMessage(tab.id, { type: 'cc-wt', action, ...extra })
  // 页面里没有翻译脚本时，别的内容脚本（如实时音效）也在监听消息，这时 sendMessage 不会报错，而是安静地返回 undefined；
  // 所以“没抛错”不等于“已送达”，必须拿到翻译脚本自己的回复（带 on 字段）才算。
  const ready = (r) => r && typeof r === 'object' && 'on' in r
  try {
    const r = await send()
    if (ready(r)) return r
  } catch {
    // 还没注入
  }
  await injectFiles(tab.id, ccFiles.GROUPS.trans)
  await sleep(60)
  const r = await send()
  if (!ready(r)) throw new Error('页面翻译没有启动（这个页面可能不允许扩展运行脚本）')
  return r
}
