// 网页高亮与摘录管理页：按网页分组，可搜索、删除、导出 Markdown。
const api = globalThis.browser ?? globalThis.chrome
const $ = (id) => document.getElementById(id)
const SEL = globalThis.ccSel
let all = {}

const save = () => api.storage.local.set({ hlData: all })
function download(name, text) {
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([text], { type: 'text/markdown;charset=utf-8' }))
  a.download = name
  document.body.append(a)
  a.click()
  a.remove()
}

function render() {
  const q = $('q').value.trim().toLowerCase()
  const groups = SEL.groupByPage(all).filter((g) => !q || g.title.toLowerCase().includes(q) || g.list.some((x) => x.text.toLowerCase().includes(q)))
  const total = Object.values(all).reduce((n, l) => n + l.length, 0)
  $('sum').textContent = `共 ${total} 条，${Object.keys(all).length} 个网页`
  $('empty').classList.toggle('hidden', total > 0)
  const box = $('list')
  box.replaceChildren()
  for (const g of groups) {
    const pg = document.createElement('div')
    pg.className = 'pg'
    const h3 = document.createElement('h3')
    const a = document.createElement('a')
    a.href = g.url
    a.target = '_blank'
    a.rel = 'noreferrer'
    a.textContent = g.title
    h3.append(a)
    const u = document.createElement('div')
    u.className = 'u'
    u.textContent = g.url
    pg.append(h3, u)
    for (const it of g.list.filter((x) => !q || x.text.toLowerCase().includes(q) || g.title.toLowerCase().includes(q))) {
      const row = document.createElement('div')
      row.className = 'it'
      const dot = document.createElement('mark')
      dot.style.background = SEL.colorOf(it.color).bg
      dot.title = SEL.colorOf(it.color).name
      const t = document.createElement('div')
      t.className = 't'
      t.textContent = it.text
      const del = document.createElement('button')
      del.className = 'sec'
      del.textContent = '删除'
      del.onclick = async () => {
        all[g.url] = (all[g.url] || []).filter((x) => x.id !== it.id)
        if (!all[g.url].length) delete all[g.url]
        await save()
        render()
      }
      row.append(dot, t, del)
      pg.append(row)
    }
    const tb = document.createElement('div')
    tb.className = 'tb'
    const exp = document.createElement('button')
    exp.className = 'sec'
    exp.textContent = '导出 Markdown'
    exp.onclick = () => download(`${g.title.replace(/[\\/:*?"<>|]/g, '_').slice(0, 60) || '摘录'}.md`, SEL.toMarkdown(g.list, { title: g.title, url: g.url }))
    const cp = document.createElement('button')
    cp.className = 'sec'
    cp.textContent = '复制 Markdown'
    cp.onclick = () => navigator.clipboard.writeText(SEL.toMarkdown(g.list, { title: g.title, url: g.url })).then(() => (cp.textContent = '已复制'))
    const rm = document.createElement('button')
    rm.className = 'sec'
    rm.textContent = '删除这个网页的全部高亮'
    rm.onclick = async () => {
      delete all[g.url]
      await save()
      render()
    }
    tb.append(exp, cp, rm)
    pg.append(tb)
    box.append(pg)
  }
}

;(async () => {
  all = (await api.storage.local.get('hlData')).hlData || {}
  $('q').addEventListener('input', render)
  render()
})()
