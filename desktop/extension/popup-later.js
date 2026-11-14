// 弹窗 / 设置页（popup.html）的拆分脚本：和 popup.js 共用同一个全局作用域（普通脚本，按 popup.html 里的顺序加载），
// 这里只放：稍后看列表，以及当前页面的动作 / 规则保存。
// ---------- 稍后看 ----------

async function renderLater() {
  const list = await getLater()
  const box = $('laterList')
  box.innerHTML = ''
  $('laterEmpty').classList.toggle('hidden', list.length > 0)
  const unseen = laterUnseen(list)
  $('laterBadge').textContent = unseen
  $('laterBadge').classList.toggle('hidden', !unseen)
  for (const it of list) {
    const row = document.createElement('div')
    row.className = 'it'
    if (it.seen) row.style.opacity = '.6'
    const text = document.createElement('span')
    const title = document.createElement('b')
    title.textContent = it.title || it.url
    title.style.fontWeight = it.seen ? '400' : '700'
    const sub = document.createElement('span')
    sub.className = 'sub'
    let host = it.url
    try {
      host = new URL(it.url).hostname
    } catch {
      // keep the raw url
    }
    sub.textContent = `${host} · ${timeAgo(it.at)}`
    text.title = it.url
    text.append(title, sub)
    const dl = document.createElement('button')
    dl.textContent = '下载'
    dl.onclick = async () => {
      try {
        const r = await sendUrl(it.url)
        await saveLater(laterSeen(await getLater(), it.url))
        msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送，清影会开始解析', 'ok')
        renderLater()
      } catch (e) {
        msg(e.message, 'err')
      }
    }
    const seen = document.createElement('button')
    seen.className = 'sec'
    seen.textContent = it.seen ? '未看' : '已看'
    seen.onclick = async () => {
      await saveLater(laterSeen(await getLater(), it.url, !it.seen))
      renderLater()
    }
    const open = document.createElement('button')
    open.className = 'sec'
    open.textContent = '打开'
    open.onclick = () => api.tabs.create({ url: it.url })
    const del = document.createElement('button')
    del.className = 'sec'
    del.textContent = '删除'
    del.onclick = async () => {
      await saveLater(laterRemove(await getLater(), it.url))
      renderLater()
    }
    row.append(text, dl, open, seen, del)
    box.append(row)
  }
}

async function refreshLaterBadge() {
  const unseen = laterUnseen(await getLater())
  $('laterBadge').textContent = unseen
  $('laterBadge').classList.toggle('hidden', !unseen)
}

$('laterSave').onclick = async () => {
  const tab = await currentTab()
  try {
    const r = await addLater({ url: tab?.url, title: tab?.title, icon: tab?.favIconUrl })
    if (r.invalid) return msg('当前页面不能加入稍后看', 'err')
    msg(r.added ? '已加入稍后看' : '稍后看里已经有这一页了，已提到最前面', 'ok')
    renderLater()
  } catch (e) {
    msg(e.message, 'err')
  }
}

// ---------- 页面保存 ----------

async function savePageAction(action) {
  const tab = await currentTab()
  if (!tab?.url?.startsWith('http')) return msg('当前页面不能使用这个功能', 'err')
  msg(action === 'shotFull' ? '正在滚动截图，请不要操作页面……' : '处理中……')
  const r = await api.runtime.sendMessage({ type: 'cc-page-save', action, tabId: tab.id }).catch((e) => ({ ok: false, error: e.message }))
  if (!r?.ok) return msg(r?.error || '没有完成', 'err')
  if (r.pairing) return msg('第一次使用：请在电脑上点“允许”完成配对', 'ok')
  const done = { shotVisible: '截图已保存到下载文件夹', shotArea: '', shotFull: '整页截图已保存到下载文件夹', pdf: '', md: '', clip: '已摘录到清影，在“生词与摘录 → 摘录”里查看' }[action]
  msg(r.note || done, 'ok')
  // 选区截图、Markdown 预览都需要用户回到页面 / 新标签页：关掉弹窗
  if (action === 'shotArea' || action === 'md' || action === 'pdf') window.close()
}

function wirePageSave() {
  for (const [id, action] of Object.entries({ shotVisible: 'shotVisible', shotArea: 'shotArea', shotFull: 'shotFull', savePdf: 'pdf', saveMd: 'md', clipText: 'clip' })) {
    $(id).onclick = () => savePageAction(action)
  }
}
wirePageSave()

async function saveRules() {
  const rules = normalizeSiteRules({
    off: { floatButton: lineList('ruleFloat'), video: lineList('ruleVideo'), hoverZoom: lineList('ruleZoom') },
    unlockOn: lineList('ruleUnlock'),
    unlockAll: $('unlockAll').checked,
    zoomRules: $('ruleZoomRules').value,
    extraParams: $('ruleParams').value,
  })
  const text = $('ruleEngines').value.trim()
  const lines = text.split(/\r?\n/).filter((l) => l.trim())
  const parsed = text ? parseEngines(text) : []
  const bad = text ? lines.length - lines.filter((l) => /\|/.test(l)).length : 0
  const same = text === enginesToText(DEFAULT_ENGINES)
  await api.storage.local.set({ siteRules: rules })
  if (!text || same) await api.storage.local.remove('engines')
  else await api.storage.local.set({ engines: enginesToText(parsed) })
  msg(bad ? `已保存。有 ${bad} 行引擎格式不对（要有“名称 | 地址”，地址里要有 {url}），已忽略。` : '已保存', bad ? 'err' : 'ok')
  loadSettings()
}
