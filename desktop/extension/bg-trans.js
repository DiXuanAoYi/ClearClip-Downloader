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

let trCache = null
let trFlushTimer = 0
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
function trFlushSoon() {
  clearTimeout(trFlushTimer)
  trFlushTimer = setTimeout(async () => {
    try {
      trCache = ccTrans.pruneCache(trCache || {})
      await api.storage.local.set({ transCache: trCache })
    } catch {
      // 存不了就算了
    }
  }, 2000)
}

// ---------- 两个引擎 ----------

async function trClearclip(texts, target) {
  const sub = await call('/api/v1/ai/run', { kind: 'webtrans', lang: ccTrans.promptName(target), lines: texts.map((text, t) => ({ t, text })) })
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

/** 翻译一批文字。先查缓存，剩下的交给引擎，返回和输入一一对应的译文。 */
async function translateTexts(texts, { target, engine }) {
  const cache = await trCacheMap()
  const out = texts.map((t) => cache[ccTrans.cacheKey(engine, target, t)]?.[0] ?? null)
  const todo = out.map((v, i) => (v == null ? i : -1)).filter((i) => i >= 0)
  if (todo.length) {
    const src = todo.map((i) => texts[i])
    let res
    if (engine === 'google') res = await trGoogle(src, target)
    else {
      await trSlot()
      try {
        res = await trClearclip(src, target)
      } finally {
        trFree()
      }
    }
    todo.forEach((i, k) => {
      out[i] = res[k]
      // 原样返回（没翻译）的不进缓存，免得以后换了模型也一直是原文
      if (res[k] && res[k].trim() !== src[k].trim()) cache[ccTrans.cacheKey(engine, target, texts[i])] = [res[k], Date.now()]
    })
    trFlushSoon()
  }
  return { texts: out, cached: texts.length - todo.length }
}

async function clearTransCache() {
  trCache = {}
  await api.storage.local.remove('transCache')
}

// ---------- 打开 / 关闭页面翻译 ----------

const TR_FILES = ['content/ui.js', 'trans-logic.js', 'content/webtrans.js']

/** 让标签页里的网页翻译做一件事：toggle / page-on / page-off / hover / selection（带 text）。还没注入就先注入。 */
async function webTransDo(tab, action, extra = {}) {
  if (!isWebPage(tab)) throw new Error('当前页面不能翻译')
  const send = () => api.tabs.sendMessage(tab.id, { type: 'cc-wt', action, ...extra })
  try {
    return await send()
  } catch {
    await injectFiles(tab.id, TR_FILES)
    await sleep(60)
    return await send()
  }
}
