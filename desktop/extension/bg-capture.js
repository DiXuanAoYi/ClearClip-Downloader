// 页面保存的后台部分：截图（可见区域 / 选区 / 整页）、另存为 PDF、保存为 Markdown、摘录文字、复制干净链接、二维码窗口。
// 都是用户点了才执行；需要页面里的信息时按需注入脚本。

const CAPTURE_GAP = 650 // Chrome 限制 captureVisibleTab 每秒最多 2 次
const FULL_MAX = 60000 // 整页截图最多截这么多 CSS 像素，防止无限滚动的页面停不下来

async function bitmapOf(dataUrl) {
  const blob = await (await fetch(dataUrl)).blob()
  return createImageBitmap(blob)
}

async function shotTab(tab) {
  try {
    return await api.tabs.captureVisibleTab(tab.windowId, { format: 'png' })
  } catch {
    throw new Error('这个页面不允许截图（浏览器的内置页面或商店页面）')
  }
}

function toDataUrl(blob) {
  return new Promise((ok, no) => {
    const r = new FileReader()
    r.onload = () => ok(r.result)
    r.onerror = () => no(r.error)
    r.readAsDataURL(blob)
  })
}

/** 存成下载文件。service worker 里没有 URL.createObjectURL，改用 data 地址。 */
async function saveBlob(blob, filename) {
  const url = typeof URL.createObjectURL === 'function' ? URL.createObjectURL(blob) : await toDataUrl(blob)
  const id = await api.downloads.download({ url, filename, conflictAction: 'uniquify', saveAs: false })
  if (url.startsWith('blob:')) setTimeout(() => URL.revokeObjectURL(url), 60000)
  return id
}

const canvasOf = (w, h) => (typeof OffscreenCanvas === 'function' ? new OffscreenCanvas(w, h) : Object.assign(document.createElement('canvas'), { width: w, height: h }))
const pngOf = (c) => (c.convertToBlob ? c.convertToBlob({ type: 'image/png' }) : new Promise((ok) => c.toBlob(ok, 'image/png')))

/** 在页面里执行一个函数，取第一个有结果的 frame 的返回值。 */
async function inPage(tabId, func, args = []) {
  const res = await api.scripting.executeScript({ target: { tabId }, func, args })
  return res[0]?.result
}

async function captureVisible(tab) {
  const bmp = await bitmapOf(await shotTab(tab))
  const c = canvasOf(bmp.width, bmp.height)
  c.getContext('2d').drawImage(bmp, 0, 0)
  await saveBlob(await pngOf(c), captureName(tab.title, new Date(), 'png'))
}

/** 选区：先截可见区域，再按选框裁出来。 */
async function captureArea(tab, r, vw) {
  const bmp = await bitmapOf(await shotTab(tab))
  const k = bmp.width / (vw || bmp.width)
  const w = Math.max(1, Math.round(r.w * k))
  const hh = Math.max(1, Math.round(r.h * k))
  const c = canvasOf(w, hh)
  c.getContext('2d').drawImage(bmp, Math.round(r.x * k), Math.round(r.y * k), w, hh, 0, 0, w, hh)
  await saveBlob(await pngOf(c), captureName(tab.title, new Date(), 'png'))
}

/** 整页：一屏一屏滚动截图再拼起来。固定在顶部的元素只保留第一屏；超长页面分成几张图。 */
function hideFixed() {
  window.__ccFixed = []
  for (const el of document.querySelectorAll('body *')) {
    const p = getComputedStyle(el).position
    if (p === 'fixed' || p === 'sticky') {
      window.__ccFixed.push([el, el.style.getPropertyValue('visibility'), el.style.getPropertyPriority('visibility')])
      el.style.setProperty('visibility', 'hidden', 'important')
    }
  }
}

async function captureFull(tab) {
  const tabId = tab.id
  const m = await inPage(tabId, () => {
    const de = document.documentElement
    return { sh: Math.max(de.scrollHeight, document.body ? document.body.scrollHeight : 0), vh: innerHeight, vw: innerWidth, sy: scrollY, sx: scrollX }
  })
  if (!m) throw new Error('这个页面不能截图')
  if (m.sh <= m.vh + 4) {
    await captureVisible(tab)
    return { note: '页面没有更多内容可滚动，保存的是可见区域。如果页面内部有自己的滚动区域，请用“选取区域”。' }
  }
  const total = Math.min(m.sh, FULL_MAX)
  const caps = []
  const action = api.action || api.browserAction
  let slices = []
  const actual = [] // 每屏实际滚动到的位置（滚不到底时和计划的不一样）
  try {
    for (let i = 0; i === 0 || i < slices.length; i++) {
      const y = i === 0 ? 0 : slices[i].scrollY
      actual[i] = await inPage(tabId, (yy) => {
        document.documentElement.style.scrollBehavior = 'auto'
        scrollTo(0, yy)
        return scrollY
      }, [y])
      await sleep(CAPTURE_GAP)
      caps.push(await bitmapOf(await shotTab(tab)))
      if (i === 0) {
        // 以真实截图的尺寸为准（有些环境里窗口和 innerHeight 不一致）
        const k0 = caps[0].width / m.vw
        slices = planSlices(total, Math.min(m.vh, Math.floor(caps[0].height / k0)))
        // 第一屏截完后，把固定元素藏起来，后面的屏就不会重复出现它们
        await inPage(tabId, hideFixed)
      }
      action.setBadgeText({ tabId, text: `${i + 1}/${slices.length}` }).catch(() => {})
    }
  } finally {
    await inPage(tabId, (x, y) => {
      for (const [el, v, pr] of window.__ccFixed || []) {
        if (v) el.style.setProperty('visibility', v, pr)
        else el.style.removeProperty('visibility')
      }
      window.__ccFixed = null
      document.documentElement.style.scrollBehavior = ''
      scrollTo(x, y)
    }, [m.sx, m.sy]).catch(() => {})
  }
  const k = caps[0].width / m.vw
  const chunks = planChunks(total, chunkHeight(m.vw, k))
  const now = new Date()
  for (const [ci, ch] of chunks.entries()) {
    const c = canvasOf(Math.round(m.vw * k), Math.round((ch.to - ch.from) * k))
    const ctx = c.getContext('2d')
    for (const [i, s] of slices.entries()) {
      const a = Math.max(s.destTop, ch.from)
      const b = Math.min(s.destTop + s.height, ch.to)
      if (b <= a) continue
      const srcY = (Math.max(0, s.destTop - (actual[i] ?? s.scrollY)) + (a - s.destTop)) * k
      ctx.drawImage(caps[i], 0, srcY, caps[i].width, (b - a) * k, 0, (a - ch.from) * k, caps[i].width, (b - a) * k)
    }
    await saveBlob(await pngOf(c), captureName(tab.title, now, 'png', ci, chunks.length))
  }
  return { note: m.sh > FULL_MAX ? `页面太长，只截取了前 ${FULL_MAX} 像素` : chunks.length > 1 ? `页面很长，已分成 ${chunks.length} 张图保存` : '' }
}

async function savePdf(tab) {
  if (typeof api.tabs.saveAsPDF === 'function') {
    await api.tabs.saveAsPDF({ toFileName: captureName(tab.title, new Date(), 'pdf'), silentMode: false })
    return
  }
  // Chrome 没有直接保存 PDF 的接口：打开打印对话框，目标选“另存为 PDF”
  await inPage(tab.id, () => window.print())
}

/** 保存为 Markdown：先提取，放进会话存储，再打开预览页让用户检查、修改后保存。 */
async function openMarkdown(tab) {
  const [res] = await injectFiles(tab.id, ['content/extract.js'])
  if (!res) throw new Error('没有提取到内容')
  const key = `capture:${Date.now()}`
  await sessionStore.set({ [key]: res })
  await api.tabs.create({ url: api.runtime.getURL(`capture.html?k=${encodeURIComponent(key)}`), index: tab.index + 1 })
}

/** 摘录：选中的文字 + 来源，存成清影里的文字便签。 */
async function clipSelection(tab, selected) {
  let text = String(selected || '').trim()
  if (!text) {
    const res = await api.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, func: () => String(getSelection()) })
    text = (res.map((r) => r.result).find((t) => t && t.trim()) || '').trim()
  }
  if (!text) throw new Error('请先在页面上选中要摘录的文字')
  const pairing = await sendText(excerptText(text, tab.title, tab.url), { note: true })
  return { pairing }
}

/** 复制干净链接：先试剪贴板；浏览器不让（没有用户手势）就弹出一个可以手动复制的框。 */
async function copyCleanLink(tab, url) {
  const clean = await tidyUrl(url)
  const via = await inPage(tab.id, async (t) => {
    try {
      await navigator.clipboard.writeText(t)
      return 'ok'
    } catch {
      // 继续尝试
    }
    try {
      const a = document.createElement('textarea')
      a.value = t
      a.style.cssText = 'position:fixed;top:0;left:0;opacity:0'
      document.body.append(a)
      a.select()
      const ok = document.execCommand('copy')
      a.remove()
      if (ok) return 'ok'
    } catch {
      // 继续尝试
    }
    prompt('复制这个干净的链接（Ctrl+C / ⌘C）', t)
    return 'prompt'
  }, [clean])
  return { clean, via }
}

async function openQr(tab, url) {
  const clean = await tidyUrl(url)
  const w = await api.windows.create({ url: api.runtime.getURL(`qr.html?u=${encodeURIComponent(clean)}`), type: 'popup', width: 380, height: 520 })
  return w
}

const PAGE_SAVE = {
  shotVisible: async (tab) => captureVisible(tab),
  shotArea: async (tab) => startAreaPicker(tab),
  shotFull: async (tab) => captureFull(tab),
  pdf: async (tab) => savePdf(tab),
  md: async (tab) => openMarkdown(tab),
  clip: async (tab, sel) => clipSelection(tab, sel),
}

async function startAreaPicker(tab) {
  await injectFiles(tab.id, ['content/ui.js', 'content/area.js'])
}

/** 弹窗和右键菜单共用的入口：检查页面、执行、给出反馈。返回 { note?, pairing? }。 */
async function pageSave(action, tab, selected) {
  if (!isWebPage(tab)) throw new Error('当前页面不能使用这个功能')
  const f = PAGE_SAVE[action]
  if (!f) throw new Error('未知操作')
  return (await f(tab, selected)) || {}
}
