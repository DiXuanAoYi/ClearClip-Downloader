// 视频缓存：把视频的摘要会话和双语字幕同步到清影（生词本据此显示每个词出自哪个视频），
// 并在“从生词本打开视频并恢复缓存”时（网址带 #cc-restore=1）把它们取回浏览器。
// 数据格式和内容脚本一致：摘要在 vsumSessions[key]，译文在 subLayerTrans[key]。

const vcache = {
  put: (payload) => call('/api/v1/videocache/put', payload),
  info: (url) => call(`/api/v1/videocache/info?url=${encodeURIComponent(url)}`),
  get: (url) => call(`/api/v1/videocache/get?url=${encodeURIComponent(url)}`),
}

/** 把清影里的快照写回浏览器存储。返回恢复了哪些：{ summary, subs }。 */
async function vcacheRestore(full) {
  const out = { summary: false, subs: false }
  const s = full?.summary
  if (s && s.key && s.data) {
    const { vsumSessions = {} } = await api.storage.local.get('vsumSessions')
    vsumSessions[s.key] = { ts: s.ts || Date.now(), data: s.data, vk: s.vk, url: s.url, key: s.key }
    await api.storage.local.set({ vsumSessions })
    out.summary = true
  }
  const b = full?.subs
  if (b && b.key && Array.isArray(b.trans) && b.trans.length) {
    const { subLayerTrans = {} } = await api.storage.local.get('subLayerTrans')
    subLayerTrans[b.key] = b.trans
    await api.storage.local.set({ subLayerTrans })
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
