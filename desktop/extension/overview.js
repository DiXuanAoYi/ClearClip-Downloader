// 页面媒体总览：把页面里的图片、视频、音频按网格列出来，勾选后一起发送到清影、复制地址或以图搜图。
const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }

const KIND_LABEL = { all: '全部', image: '图片', video: '视频', audio: '音频' }
let data = { page: '', title: '', items: [], sniffed: [] }
let kind = 'all'
const picked = new Set()

const sizeOf = (it) => Math.max(it.w || 0, it.h || 0)

function visible() {
  const min = Number($('minSize').value) || 0
  return data.items.filter((it) => (kind === 'all' || it.kind === kind) && (it.kind !== 'image' || !min || !sizeOf(it) || sizeOf(it) >= min))
}

function renderKinds() {
  $('kinds').innerHTML = ''
  for (const k of Object.keys(KIND_LABEL)) {
    const n = k === 'all' ? data.items.length : data.items.filter((x) => x.kind === k).length
    if (k !== 'all' && !n) continue
    const b = document.createElement('button')
    b.textContent = `${KIND_LABEL[k]} ${n}`
    b.className = k === kind ? 'on' : ''
    b.onclick = () => {
      kind = k
      render()
    }
    $('kinds').append(b)
  }
}

function render() {
  renderKinds()
  const list = visible()
  $('grid').innerHTML = ''
  $('empty').classList.toggle('hidden', list.length > 0)
  for (const it of list) {
    const cell = document.createElement('div')
    cell.className = `cell${picked.has(it.url) ? ' on' : ''}`
    const pic = document.createElement('div')
    pic.className = 'pic'
    const thumb = it.kind === 'image' ? it.thumb || it.url : it.thumb
    if (thumb) {
      const img = document.createElement('img')
      img.referrerPolicy = 'no-referrer'
      img.loading = 'lazy'
      img.src = thumb
      img.onerror = () => {
        pic.textContent = '无法预览'
        pic.className = 'pic ph'
      }
      pic.append(img)
    } else {
      pic.textContent = it.kind === 'audio' ? '♪' : '▶'
      pic.className = 'pic ph'
    }
    const tick = document.createElement('div')
    tick.className = 'tick'
    tick.textContent = '✓'
    const fmt = document.createElement('div')
    fmt.className = 'fmt'
    paintFormat(fmt, it)
    it._fmtEl = fmt
    const meta = document.createElement('div')
    meta.className = 'meta'
    const dim = document.createElement('span')
    dim.textContent = [it.w && it.h ? `${it.w}×${it.h}` : '', it.size ? fmtSize(it.size) : KIND_LABEL[it.kind]].filter(Boolean).join(' · ')
    it._dimEl = dim
    const open = document.createElement('a')
    open.href = it.url
    open.target = '_blank'
    open.rel = 'noreferrer'
    open.textContent = '打开'
    open.onclick = (e) => e.stopPropagation()
    meta.append(dim, open)
    cell.title = it.url
    cell.append(pic, tick, fmt, meta)
    cell.ondblclick = (e) => (e.preventDefault(), openViewer(it))
    cell.onclick = () => {
      picked.has(it.url) ? picked.delete(it.url) : picked.add(it.url)
      cell.classList.toggle('on', picked.has(it.url))
      updateBar()
    }
    $('grid').append(cell)
  }
  updateBar()
  probeAll(list)
}

function chosen() {
  return data.items.filter((x) => picked.has(x.url))
}

function updateBar() {
  const n = picked.size
  $('sendSel').textContent = n ? `发送所选（${n}）` : '发送所选'
  $('sendSel').disabled = !n
  $('copyUrls').disabled = !n
  $('searchOne').disabled = !(n === 1 && chosen()[0]?.kind === 'image')
}

function renderStreams() {
  $('streams').classList.toggle('hidden', !data.sniffed.length)
  $('streamList').innerHTML = ''
  for (const s of data.sniffed.filter((x) => x.kind !== 'image').slice(0, 10)) {
    const row = document.createElement('div')
    row.className = 'it'
    const name = document.createElement('span')
    name.textContent = s.label
    name.title = s.url
    const btn = document.createElement('button')
    btn.textContent = '发送'
    btn.onclick = () => sendItems([{ kind: 'video', url: s.url }])
    row.append(name, btn)
    $('streamList').append(row)
  }
}


const S = globalThis.ccSniff
const fmtSize = (n) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`)

/** 格式徽标：显示探测出的真实格式；和地址后缀对不上时变橙并写明。 */
function paintFormat(el, it) {
  if (!el) return
  const real = it.fmt || ''
  const url = S.extOfUrl(it.url)
  const mismatch = real && url && url !== real && (S.MIME[url] || S.MIME[real])
  el.textContent = real ? (S.FORMAT_LABEL[real] || real.toUpperCase()) + (mismatch ? ` ≠ .${url}` : '') : it.probing ? '…' : ''
  el.classList.toggle('warn', !!mismatch)
  el.title = mismatch ? `地址后缀是 .${url}，实际内容是 ${S.FORMAT_LABEL[real] || real}。保存时按真实格式命名。` : ''
  el.hidden = !el.textContent
}

let probeToken = 0
/** 逐个探测当前列表里还不知道格式的项目（同时 4 个），探测到就更新卡片，不重画整个网格。 */
async function probeAll(list) {
  const token = ++probeToken
  const todo = list.filter((it) => it.kind !== 'audio' && !it.fmt && !it.probed)
  let i = 0
  const worker = async () => {
    while (i < todo.length && token === probeToken) {
      const it = todo[i++]
      it.probing = true
      paintFormat(it._fmtEl, it)
      const r = await chrome_send({ type: 'cc-probe-media', url: it.url, referrer: data.page })
      it.probing = false
      it.probed = true
      if (r.ext) it.fmt = r.ext
      if (r.mime) it.mime = r.mime
      if (r.size) it.size = r.size
      paintFormat(it._fmtEl, it)
      if (it._dimEl && r.size) it._dimEl.textContent = [it.w && it.h ? `${it.w}×${it.h}` : '', fmtSize(r.size)].filter(Boolean).join(' · ')
    }
  }
  await Promise.all([worker(), worker(), worker(), worker()])
}

// ---------- 双击查看原图 ----------
let vIdx = 0
let vList = []
let vScale = 'fit'
function openViewer(it) {
  vList = visible().filter((x) => x.kind === 'image' || x.kind === 'video')
  vIdx = Math.max(0, vList.indexOf(it))
  $('viewer').classList.remove('hidden')
  document.body.classList.add('noscroll')
  showViewer()
}
function closeViewer() {
  $('viewer').classList.add('hidden')
  document.body.classList.remove('noscroll')
  $('vStage').innerHTML = ''
}
function showViewer() {
  const it = vList[vIdx]
  if (!it) return closeViewer()
  vScale = 'fit'
  const stage = $('vStage')
  stage.innerHTML = ''
  stage.classList.remove('full')
  let el
  if (it.kind === 'video') {
    el = document.createElement('video')
    el.controls = true
    el.src = it.url
  } else {
    el = document.createElement('img')
    el.referrerPolicy = 'no-referrer'
    el.src = it.url
    el.onload = () => {
      it.w = it.w || el.naturalWidth
      it.h = it.h || el.naturalHeight
      infoLine(it)
    }
    el.onerror = () => (stage.textContent = '原图加载失败（可能需要登录，或地址已失效）')
  }
  stage.append(el)
  $('vName').textContent = ccSniff.fileNameFor(it.url, it.fmt || S.extOfUrl(it.url))
  $('vPos').textContent = `${vIdx + 1} / ${vList.length}`
  infoLine(it)
  paintFormat($('vFmt'), it)
  if (!it.fmt && it.kind === 'image' && !it.probed) {
    chrome_send({ type: 'cc-probe-media', url: it.url, referrer: data.page }).then((r) => {
      it.probed = true
      if (r.ext) it.fmt = r.ext
      if (r.size) it.size = r.size
      if (vList[vIdx] === it) (infoLine(it), paintFormat($('vFmt'), it), ($('vName').textContent = ccSniff.fileNameFor(it.url, it.fmt || S.extOfUrl(it.url))))
    })
  }
}
function infoLine(it) {
  $('vInfo').textContent = [it.fmt ? S.FORMAT_LABEL[it.fmt] || it.fmt : '', it.w && it.h ? `${it.w}×${it.h}` : '', it.size ? fmtSize(it.size) : ''].filter(Boolean).join(' · ')
}
function stepViewer(d) {
  if (!vList.length) return
  vIdx = (vIdx + d + vList.length) % vList.length
  showViewer()
}
function toggleScale() {
  vScale = vScale === 'fit' ? 'full' : 'fit'
  $('vStage').classList.toggle('full', vScale === 'full')
}
async function saveViewer() {
  const it = vList[vIdx]
  if (!it) return
  const r = await chrome_send({ type: 'cc-save', url: it.url, referrer: data.page })
  $('vMsg').textContent = r.ok ? '已保存' : r.error || '保存失败'
}
async function copyViewerImage() {
  const it = vList[vIdx]
  try {
    const blob = await (await fetch(it.url)).blob()
    const png = blob.type === 'image/png' ? blob : await new Promise((ok) => {
      const img = new Image()
      img.crossOrigin = 'anonymous'
      img.onload = () => {
        const c = document.createElement('canvas')
        c.width = img.naturalWidth
        c.height = img.naturalHeight
        c.getContext('2d').drawImage(img, 0, 0)
        c.toBlob(ok, 'image/png')
      }
      img.src = URL.createObjectURL(blob)
    })
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': png })])
    $('vMsg').textContent = '已复制图片'
  } catch {
    $('vMsg').textContent = '这张图不能复制（跨域限制），可以用“复制地址”'
  }
}
document.addEventListener('keydown', (e) => {
  if ($('viewer').classList.contains('hidden')) return
  if (e.key === 'Escape') closeViewer()
  else if (e.key === 'ArrowLeft') stepViewer(-1)
  else if (e.key === 'ArrowRight') stepViewer(1)
  else if (e.key === ' ') (e.preventDefault(), toggleScale())
  else if (e.key.toLowerCase() === 's') saveViewer()
  else return
  e.preventDefault()
})

async function sendItems(items) {
  msg('发送中…')
  const r = await chrome_send({ type: 'cc-send-items', items, page: data.page, title: data.title })
  if (!r.ok) return msg(r.error || '发送失败', 'err')
  msg(r.pairing ? '第一次使用：请在电脑上点“允许”完成配对' : `已发送 ${r.sent} 项${r.failed ? `，${r.failed} 项失败：${r.error}` : ''}`, r.failed && !r.sent ? 'err' : 'ok')
}

async function chrome_send(m) {
  try {
    return (await api.runtime.sendMessage(m)) || { ok: false, error: '没有响应' }
  } catch (e) {
    return { ok: false, error: e.message }
  }
}

async function load() {
  const key = new URLSearchParams(location.search).get('k')
  const r = key ? await (api.storage.session || api.storage.local).get(key) : {}
  data = { page: '', title: '', items: [], sniffed: [], ...(r[key] || {}) }
  $('pageInfo').textContent = data.title || data.page
  document.title = `页面媒体总览 - ${data.title || data.page}`
  renderStreams()
  render()
}

$('minSize').onchange = render
$('selAll').onclick = () => {
  for (const it of visible()) picked.add(it.url)
  render()
}
$('selNone').onclick = () => {
  picked.clear()
  render()
}
$('sendSel').onclick = () => sendItems(chosen().map((x) => ({ kind: x.kind, url: x.url })))
$('copyUrls').onclick = async () => {
  try {
    await navigator.clipboard.writeText(chosen().map((x) => x.url).join('\n'))
    msg(`已复制 ${picked.size} 个地址`, 'ok')
  } catch {
    msg('复制失败', 'err')
  }
}
$('searchOne').onclick = async () => {
  const it = chosen()[0]
  const engines = await getEngines()
  const r = await chrome_send({ type: 'cc-search', url: it.url, engine: 'all' })
  if (!r.ok) msg(r.error, 'err')
  else msg(`已用 ${engines.length} 个搜索引擎各打开一个标签`, 'ok')
}
$('rescan').onclick = async () => {
  const key = new URLSearchParams(location.search).get('k')
  const tabId = data.tabId
  if (tabId == null) return msg('原网页已经关闭，无法重新扫描', 'err')
  const r = await chrome_send({ type: 'cc-run', action: 'overview', tabId })
  if (r.ok) {
    ;(api.storage.session || api.storage.local).remove(key)
    window.close()
  } else msg(r.error || '重新扫描失败', 'err')
}

load()

$('vClose').onclick = closeViewer
$('vPrev').onclick = () => stepViewer(-1)
$('vNext').onclick = () => stepViewer(1)
$('vScale').onclick = toggleScale
$('vSave').onclick = saveViewer
$('vCopyImg').onclick = copyViewerImage
$('vCopyUrl').onclick = () => navigator.clipboard.writeText(vList[vIdx]?.url || '').then(() => ($('vMsg').textContent = '已复制地址'))
$('viewer').addEventListener('click', (e) => e.target === $('vBody') && closeViewer())
