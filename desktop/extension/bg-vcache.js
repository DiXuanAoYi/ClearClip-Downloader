// 视频缓存：把视频的摘要会话和双语字幕同步到清影（生词本据此显示每个词出自哪个视频），
// 并在“从生词本打开视频并恢复缓存”时（网址带 #cc-restore=1）把它们取回浏览器。
// 数据格式和内容脚本一致：摘要在 vsumSessions[key]，译文在 subLayerTrans[key]。

const vcache = {
  put: (payload) => call('/api/v1/videocache/put', payload),
  info: (url) => call(`/api/v1/videocache/info?url=${encodeURIComponent(url)}`),
  get: (url) => call(`/api/v1/videocache/get?url=${encodeURIComponent(url)}`),
}

// 与内容脚本（vsum.js / vsession.js）里的上限一致：摘要会话最多 40 个、2.5MB；双语字幕最多 12 个、2.5MB
const VCACHE_SESSIONS_KEEP = 40
const VCACHE_TRANS_KEEP = 12
const VCACHE_BYTES = 2_500_000

/** 摘要会话：按 ts 保留最新的，条数和总字数超限就丢最旧的。 */
function vcacheTrimSessions(all, keep = VCACHE_SESSIONS_KEEP, maxBytes = VCACHE_BYTES) {
  const list = Object.entries(all || {}).sort((a, b) => (b[1]?.ts || 0) - (a[1]?.ts || 0))
  const out = {}
  let bytes = 0
  let n = 0
  for (const [k, v] of list) {
    const size = JSON.stringify(v).length
    if (n >= keep || (n > 0 && bytes + size > maxBytes)) continue
    out[k] = v
    bytes += size
    n++
  }
  return out
}

/** 双语字幕：键的顺序就是新旧顺序（最新在最后），超限从最前面丢。 */
function vcacheTrimTrans(all, keep = VCACHE_TRANS_KEEP, maxBytes = VCACHE_BYTES) {
  const keys = Object.keys(all || {})
  const out = {}
  let bytes = 0
  let n = 0
  for (let i = keys.length - 1; i >= 0; i--) {
    const size = JSON.stringify(all[keys[i]]).length
    if (n >= keep || (n > 0 && bytes + size > maxBytes)) continue
    out[keys[i]] = all[keys[i]]
    bytes += size
    n++
  }
  return Object.fromEntries(Object.entries(out).reverse())
}

/** 把清影里的快照写回浏览器存储。返回恢复了哪些：{ summary, subs }。 */
async function vcacheRestore(full) {
  const out = { summary: false, subs: false }
  const s = full?.summary
  if (s && s.key && s.data) {
    const { vsumSessions = {} } = await api.storage.local.get('vsumSessions')
    // 恢复的会话视为刚用过（ts 取现在），并和内容脚本一样限条数和大小，不会把存储越撑越大
    vsumSessions[s.key] = { ts: Date.now(), data: s.data, vk: s.vk, url: s.url, key: s.key }
    await api.storage.local.set({ vsumSessions: vcacheTrimSessions(vsumSessions) })
    out.summary = true
  }
  const b = full?.subs
  if (b && b.key && Array.isArray(b.trans) && b.trans.length) {
    const { subLayerTrans = {} } = await api.storage.local.get('subLayerTrans')
    delete subLayerTrans[b.key] // 先删再放：排到最后 = 最近用过
    subLayerTrans[b.key] = b.trans
    await api.storage.local.set({ subLayerTrans: vcacheTrimTrans(subLayerTrans) })
    out.subs = true
  }
  return out
}

api.tabs.onUpdated.addListener(async (tabId, change, tab) => {
  if (change.status !== 'complete' || !/[#&]cc-restore=1/.test(tab?.url || '')) return
  try {
    const full = await vcache.get(tab.url.split('#')[0])
    const r = await vcacheRestore(full)
    await flashBadge(tabId, r.summary || r.subs)
    if (r.summary || r.subs) {
      api.notifications?.create?.({ type: 'basic', iconUrl: api.runtime.getURL('icons/128.png'), title: '清影：已恢复视频缓存', message: `${r.summary ? '摘要 ' : ''}${r.subs ? '双语字幕 ' : ''}已取回，打开视频摘要（Alt+Shift+S）或字幕层即可看到。` })
    }
  } catch (e) {
    console.warn('vcache restore failed', e)
    await flashBadge(tabId, false)
  }
})
