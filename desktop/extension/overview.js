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
    const meta = document.createElement('div')
    meta.className = 'meta'
    const dim = document.createElement('span')
    dim.textContent = [it.w && it.h ? `${it.w}×${it.h}` : '', KIND_LABEL[it.kind]].filter(Boolean).join(' · ')
    const open = document.createElement('a')
    open.href = it.url
    open.target = '_blank'
    open.rel = 'noreferrer'
    open.textContent = '打开'
    open.onclick = (e) => e.stopPropagation()
    meta.append(dim, open)
    cell.title = it.url
    cell.append(pic, tick, meta)
    cell.onclick = () => {
      picked.has(it.url) ? picked.delete(it.url) : picked.add(it.url)
      cell.classList.toggle('on', picked.has(it.url))
      updateBar()
    }
    $('grid').append(cell)
  }
  updateBar()
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
