// 稍后看：列表存在扩展本地（离线也能用），同一链接只留一条。纯函数部分可以单元测试，存取用 storage.local。

const LATER_MAX = 300

/** 判断是不是同一个页面：忽略 # 后面的部分和末尾的斜杠。 */
function laterKey(url) {
  try {
    const u = new URL(url)
    u.hash = ''
    return u.href.replace(/\/$/, '')
  } catch {
    return String(url || '')
  }
}

/** 加一条。已经有的不重复添加，只把它提到最前面并重新标成未看。返回 { list, added }。 */
function laterAdd(list, item, now = Date.now()) {
  const url = String(item.url || '')
  if (!/^https?:\/\//i.test(url)) return { list, added: false, invalid: true }
  const key = laterKey(url)
  const rest = list.filter((x) => laterKey(x.url) !== key)
  const had = rest.length !== list.length
  const entry = { url, title: String(item.title || '').slice(0, 200), icon: item.icon || '', at: now, seen: false }
  return { list: [entry, ...rest].slice(0, LATER_MAX), added: !had }
}

const laterRemove = (list, url) => list.filter((x) => laterKey(x.url) !== laterKey(url))
const laterSeen = (list, url, seen = true) => list.map((x) => (laterKey(x.url) === laterKey(url) ? { ...x, seen } : x))
const laterUnseen = (list) => list.filter((x) => !x.seen).length

async function getLater() {
  const { later } = await api.storage.local.get('later')
  return Array.isArray(later) ? later : []
}

const saveLater = (list) => api.storage.local.set({ later: list })

/** 加进稍后看列表（发送前去掉追踪参数）。返回 { added, invalid }。 */
async function addLater(item) {
  const r = laterAdd(await getLater(), { ...item, url: await tidyUrl(item.url) })
  if (!r.invalid) await saveLater(r.list)
  return { added: r.added, invalid: !!r.invalid }
}
