// 页面功能的后台部分（接上 bg-actions.js）：右键菜单、快捷键、扩展数据同步、安装 / 启动 / 权限 / 存储变化的事件，最后同步一次常驻内容脚本。

// ---------- 右键菜单 ----------

async function buildMenus() {
  const f = await getFeatures()
  await api.contextMenus.removeAll()
  const add = (o) => api.contextMenus.create(o)
  add({ id: 'cc-page', title: '发送此页面到清影', contexts: ['page'] })
  if (f.vsum) {
    add({ id: 'cc-vsum', title: '视频摘要 / 总结…', contexts: ['page', 'video'] })
    add({ id: 'cc-vsum-sel', title: '总结选中的文字…', contexts: ['selection'] })
  }
  if (f.subLayer) add({ id: 'cc-sublayer', title: '给视频加字幕层（双语 / 转写）…', contexts: ['page', 'video'] })
  if (f.webtrans) {
    add({ id: 'cc-wt', title: '翻译此页面 / 还原（Alt+A）', contexts: ['page'] })
    add({ id: 'cc-wt-sel', title: '翻译选中的文字', contexts: ['selection'] })
    add({ id: 'cc-tbox', title: '在翻译窗里翻译（多个服务对比，Alt+D）', contexts: ['selection'] })
  }
  if (f.onDemand) add({ id: 'cc-overview', title: '打开页面媒体总览', contexts: ['page'] })
  add({ id: 'cc-link', title: '发送此链接到清影', contexts: ['link'] })
  add({ id: 'cc-media', title: '发送此视频 / 音频 / 图片到清影', contexts: ['video', 'image', 'audio'] })
  if (f.readLater) {
    add({ id: 'cc-later', title: '稍后看（存进清影的待看列表）', contexts: ['page'] })
    add({ id: 'cc-later-link', title: '把此链接加入稍后看', contexts: ['link'] })
  }
  if (f.cleanLink) {
    add({ id: 'cc-copy-clean', title: '复制干净的链接', contexts: ['page'] })
    add({ id: 'cc-copy-clean-link', title: '复制干净的链接', contexts: ['link'] })
  }
  add({ id: 'cc-qr', title: '用二维码在手机上打开此页…', contexts: ['page'] })
  if (f.onDemand) {
    add({ id: 'cc-clip', title: '摘录选中文字到清影', contexts: ['selection'] })
    add({ id: 'cc-save', title: '页面保存', contexts: ['page'] })
    add({ id: 'cc-save:shotVisible', parentId: 'cc-save', title: '截图：可见区域', contexts: ['page'] })
    add({ id: 'cc-save:shotArea', parentId: 'cc-save', title: '截图：选取区域', contexts: ['page'] })
    add({ id: 'cc-save:shotFull', parentId: 'cc-save', title: '截图：整页', contexts: ['page'] })
    add({ id: 'cc-save:pdf', parentId: 'cc-save', title: '另存为 PDF', contexts: ['page'] })
    add({ id: 'cc-save:md', parentId: 'cc-save', title: '保存为 Markdown', contexts: ['page'] })
  }
  if (f.onDemand) {
    add({ id: 'cc-search', title: '以图搜图', contexts: ['image'] })
    for (const [i, e] of (await getEngines()).entries()) add({ id: `cc-search:${i}`, parentId: 'cc-search', title: e.name, contexts: ['image'] })
    add({ id: 'cc-search:all', parentId: 'cc-search', title: '全部引擎各开一个标签', contexts: ['image'] })
    add({ id: 'cc-search:upload', parentId: 'cc-search', title: '上传本地图片…', contexts: ['image'] })
  }
}

// 设置连着改几次只重建一次；待办存进存储 + alarm 兜底，后台在这 300 毫秒里被回收也会补做
function rebuildMenusSoon() {
  ccDefer.schedule('menus', 300).catch((e) => console.warn('schedule menus failed', e))
}
ccDefer.on('menus', () => buildMenus().catch((e) => console.warn('menus failed', e)))

api.contextMenus.onClicked.addListener(async (info, tab) => {
  const id = String(info.menuItemId)
  try {
    if (id === 'cc-overview') await openOverview(tab)
    else if (id === 'cc-vsum' || id === 'cc-vsum-sel') await startVsum(tab)
    else if (id === 'cc-sublayer') await startSubLayer(tab)
    else if (id === 'cc-wt') await webTransDo(tab, 'toggle')
    else if (id === 'cc-wt-sel') await webTransDo(tab, 'selection', { text: info.selectionText || '' })
    else if (id === 'cc-tbox') await openTransBox(tab, info.selectionText || '')
    else if (id === 'cc-later' || id === 'cc-later-link') {
      const url = id === 'cc-later' ? tab.url : info.linkUrl
      const r = await addLater({ url, title: id === 'cc-later' ? tab.title : info.selectionText || '', icon: id === 'cc-later' ? tab.favIconUrl : '' })
      if (r.invalid) throw new Error('这个地址不能加入稍后看')
      notify(r.added ? '已加入稍后看' : '稍后看里已经有了', '在扩展弹窗的“稍后看”标签里查看')
      await flashBadge(tab?.id, true)
    } else if (id === 'cc-copy-clean' || id === 'cc-copy-clean-link') {
      const r = await copyCleanLink(tab, id === 'cc-copy-clean' ? tab.url : info.linkUrl)
      if (r.via === 'ok') await flashBadge(tab?.id, true)
    } else if (id === 'cc-qr') await openQr(tab, tab.url)
    else if (id === 'cc-clip') {
      const r = await pageSave('clip', tab, info.selectionText)
      notify(r.pairing ? '请在电脑上允许配对' : '已摘录到清影', r.pairing ? '第一次使用需要在清影里点“允许”' : '在清影的“生词与摘录 → 摘录”里查看')
      await flashBadge(tab?.id, !r.pairing)
    } else if (id.startsWith('cc-save:')) {
      const r = await pageSave(id.slice('cc-save:'.length), tab, '')
      if (r.note) notify('页面保存', r.note)
    }
    else if (id === 'cc-search:upload') await api.tabs.create({ url: api.runtime.getURL('search.html') })
    else if (id.startsWith('cc-search:')) await searchImage(info.srcUrl, id.slice('cc-search:'.length))
  } catch (e) {
    notify('操作没有完成', e.message)
  }
})

/** 打开翻译窗：优先在当前网页上弹出可拖动的悬浮窗（选中的文字带进去）；页面不让注入脚本时才开独立页面。 */
async function openTransBox(tab, text = '') {
  const t = tab || (await api.tabs.query({ active: true, currentWindow: true }))[0]
  try {
    await webTransDo(t, 'box', { text })
    return
  } catch {
    // 内置页、PDF 查看器等：退回独立页面
  }
  await api.tabs.create({ url: api.runtime.getURL('transbox.html') })
}

api.commands?.onCommand.addListener(async (cmd) => {
  if (cmd === 'toggle-writing') {
    try {
      if (!(await getFeatures()).writing) return
      const [tab] = await api.tabs.query({ active: true, currentWindow: true })
      // 常驻脚本在每个框架里都有一份，只有有焦点的那个会响应
      if (tab?.id != null) await api.tabs.sendMessage(tab.id, { type: 'cc-write-open' })
    } catch (e) {
      notify('写作浮窗没有打开', '这个页面不支持（浏览器内置页面或刚装好的扩展需要刷新页面）')
    }
    return
  }
  if (cmd === 'toggle-webtrans') {
    try {
      if (!(await getFeatures()).webtrans) return
      const [tab] = await api.tabs.query({ active: true, currentWindow: true })
      await webTransDo(tab, 'toggle')
    } catch (e) {
      notify('网页翻译没有打开', e.message)
    }
    return
  }
  if (cmd === 'open-transbox') {
    try {
      await openTransBox(null)
    } catch (e) {
      notify('翻译窗没有打开', e.message)
    }
    return
  }
  if (cmd !== 'toggle-vsum') return
  try {
    if (!(await getFeatures()).vsum) return
    const [tab] = await api.tabs.query({ active: true, currentWindow: true })
    await startVsum(tab)
  } catch (e) {
    notify('视频摘要没有打开', e.message)
  }
})

// ---------- 扩展的用户数据同步到清影（笔记、高亮、本地导入的字幕）：进清影的备份；新装扩展时自动取回 ----------

const EXT_KEYS = ['vsumNotes', 'hlData', 'vsumLocal']
function pushExtDataSoon() {
  ccDefer.schedule('ext-push', 15000).catch((e) => console.warn('schedule ext push failed', e))
}
ccDefer.on('ext-push', async () => {
  try {
    const data = await api.storage.local.get(EXT_KEYS)
    await call('/api/v1/extdata', { data })
  } catch {
    // 清影没开：下次改动时再同步
  }
})
async function pullExtData() {
  try {
    const r = await call('/api/v1/extdata')
    const have = await api.storage.local.get(EXT_KEYS)
    const fill = {}
    for (const k of EXT_KEYS) {
      const v = r?.data?.[k]
      const empty = have[k] == null || (Array.isArray(have[k]) ? !have[k].length : typeof have[k] === 'object' && !Object.keys(have[k]).length)
      if (v != null && empty) fill[k] = v
    }
    if (Object.keys(fill).length) await api.storage.local.set(fill)
  } catch {
    // 清影没开或没配对：不取回
  }
}

api.runtime.onInstalled.addListener(async (d) => {
  if (d?.reason === 'install') pullExtData()
  // 升级上来的用户保持原来的样子：视频上的音效按钮继续显示（新安装默认不显示）
  if (d?.reason === 'update') {
    try {
      // 播放增强新增了总开关（默认关）。升级上来、自己存过功能开关的用户保持原来的样子：原来用着播放增强的继续开着
      const { features } = await api.storage.local.get('features')
      if (features && typeof features === 'object' && typeof features.videoMaster !== 'boolean') {
        const was = features.speed !== false || features.speedMemory !== false || features.seek !== false || features.videoTools === true || features.videoAdvanced === true
        await api.storage.local.set({ features: { ...features, videoMaster: was } })
      }
      // 1.18 起“解除复制 / 右键限制”默认只对开启过的网站生效。以前默认是所有网站，升级时改掉一次并提示（还能在设置里改回来）
      const { siteRules, unlockMigrated } = await api.storage.local.get(['siteRules', 'unlockMigrated'])
      if (!unlockMigrated) {
        if (!siteRules || siteRules.unlockAll !== false) {
          await api.storage.local.set({ siteRules: { ...(siteRules || {}), unlockAll: false } })
          notify('解除复制限制改为按网站开启', '以前对所有网站生效，会影响在线文档、设计工具的右键和拖放。现在只对你开启过的网站生效：在弹窗“工具”里对当前网站打开，或在设置里改回“所有网站”。')
        }
        await api.storage.local.set({ unlockMigrated: true })
      }
      const { audioCfg } = await api.storage.local.get('audioCfg')
      if (!audioCfg || typeof audioCfg.pillShow !== 'boolean') await api.storage.local.set({ audioCfg: { ...(audioCfg || {}), pillShow: true } })
    } catch {
      // 读不到存储：按默认
    }
  }
  buildMenus().catch((e) => console.warn('menus failed', e))
  syncResident(true)
})
api.runtime.onStartup?.addListener(() => syncResident())
// 用户在弹窗里授权 / 撤销网页访问权限后，重新决定要注册哪些常驻脚本
api.permissions?.onAdded?.addListener(() => syncResident())
api.permissions?.onRemoved?.addListener(() => syncResident())
api.storage.onChanged.addListener((ch, area) => {
  if (area !== 'local') return
  if (ch.features) {
    syncResident()
    rebuildMenusSoon()
  }
  if (ch.engines) rebuildMenusSoon()
  if (ch.siteRules || ch.transCfg || ch.takeover) syncResident()
  if (EXT_KEYS.some((k) => ch[k])) pushExtDataSoon()
  // 刚连上清影（填了令牌 / 配对成功）：本地还没有的笔记、高亮从清影取回
  if (ch.token?.newValue && !ch.token.oldValue) pullExtData()
})
syncResident()
