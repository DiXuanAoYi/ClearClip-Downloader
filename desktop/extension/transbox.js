// 独立翻译窗：输入或粘贴一段文字，用开着的翻译服务各翻一遍，并排给出结果。
const $ = (id) => document.getElementById(id)
const T = globalThis.ccTrans
let cfg = T.normalizeCfg(undefined)

const esc = (s) => String(s)

function renderEngines() {
  const on = T.activeEngines(cfg)
  const box = $('engines')
  box.replaceChildren()
  for (const e of on) {
    const l = document.createElement('label')
    const c = document.createElement('input')
    c.type = 'checkbox'
    c.checked = true
    c.value = e
    l.append(c, document.createTextNode(T.ENGINE_NAMES[e] || e))
    box.append(l)
  }
}

function fillTargets() {
  const sel = $('target')
  sel.replaceChildren()
  for (const l of T.LANGS) {
    const o = document.createElement('option')
    o.value = l.code
    o.textContent = `${l.name}（${l.code}）`
    sel.append(o)
  }
  sel.value = cfg.target
}

function card(r) {
  const el = document.createElement('div')
  el.className = `card${r.error ? ' err' : ''}`
  const head = document.createElement('div')
  head.className = 'eng'
  const name = document.createElement('span')
  name.textContent = r.name
  const grow = document.createElement('span')
  grow.className = 'grow'
  head.append(name, grow)
  const txt = document.createElement('div')
  txt.className = 'txt'
  txt.textContent = r.error || r.text || ''
  if (!r.error && r.text) {
    const copy = document.createElement('button')
    copy.className = 'sec'
    copy.textContent = '复制'
    copy.onclick = async () => {
      try {
        await navigator.clipboard.writeText(r.text)
        copy.textContent = '已复制'
      } catch {
        copy.textContent = '复制失败'
      }
      setTimeout(() => (copy.textContent = '复制'), 1200)
    }
    const say = document.createElement('button')
    say.className = 'sec'
    say.textContent = '朗读'
    say.onclick = () => {
      speechSynthesis.cancel()
      const u = new SpeechSynthesisUtterance(r.text)
      u.lang = $('target').value
      speechSynthesis.speak(u)
    }
    head.append(say, copy)
  }
  el.append(head, txt)
  return el
}

async function go() {
  const text = $('src').value
  if (!text.trim()) return
  const engines = [...$('engines').querySelectorAll('input:checked')].map((c) => c.value)
  $('go').disabled = true
  $('msg').textContent = '翻译中…'
  $('out').replaceChildren()
  try {
    const r = await api.runtime.sendMessage({ type: 'cc-trbox', text, target: $('target').value, engines })
    $('msg').textContent = ''
    if (!r || !r.results) {
      $('msg').textContent = '没有响应，刷新一下这个页面再试'
      return
    }
    for (const x of r.results) $('out').append(card(x))
  } catch (e) {
    $('msg').textContent = `翻译失败：${esc(e?.message || e)}`
  } finally {
    $('go').disabled = false
  }
}

async function init() {
  const { transCfg, transboxText } = await api.storage.local.get(['transCfg', 'transboxText'])
  cfg = T.normalizeCfg(transCfg)
  fillTargets()
  renderEngines()
  if (transboxText) {
    $('src').value = String(transboxText).slice(0, 5000)
    await api.storage.local.remove('transboxText')
  }
  $('src').focus()
  if ($('src').value.trim()) void go()
}

$('go').onclick = go
$('src').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.isComposing) {
    e.preventDefault()
    void go()
  }
})
void init()
