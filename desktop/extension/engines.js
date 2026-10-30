// 以图搜图引擎：名称 + 地址模板（{url} 会换成编码后的图片地址）。只在用户点击后才打开。

const DEFAULT_ENGINES = [
  { name: 'Google', url: 'https://lens.google.com/uploadbyurl?url={url}' },
  { name: 'Bing', url: 'https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:{url}' },
  { name: 'Yandex', url: 'https://yandex.com/images/search?rpt=imageview&url={url}' },
  { name: 'TinEye', url: 'https://tineye.com/search?url={url}' },
  { name: '百度', url: 'https://graph.baidu.com/details?isfromtusoupc=1&tn=pc&carousel=0&image={url}' },
  { name: '搜狗', url: 'https://pic.sogou.com/ruquery?query={url}' },
]

/** 文本（每行 “名称 | 地址”）→ 引擎列表；地址必须是 http(s) 且含 {url}。解析不出任何一行时用默认列表。 */
function parseEngines(text) {
  const out = []
  for (const line of String(text || '').split(/\r?\n/)) {
    const i = line.indexOf('|')
    if (i < 0) continue
    const name = line.slice(0, i).trim()
    const url = line.slice(i + 1).trim()
    if (name && /^https?:\/\//i.test(url) && url.includes('{url}')) out.push({ name, url })
  }
  return out.length ? out : DEFAULT_ENGINES.map((e) => ({ ...e }))
}

function enginesToText(list) {
  return (list || []).map((e) => `${e.name} | ${e.url}`).join('\n')
}

/** 引擎 + 图片地址 → 搜索页地址。 */
function engineUrl(engine, imageUrl) {
  return engine.url.split('{url}').join(encodeURIComponent(imageUrl))
}

async function getEngines() {
  const { engines } = await api.storage.local.get('engines')
  return typeof engines === 'string' && engines.trim() ? parseEngines(engines) : DEFAULT_ENGINES.map((e) => ({ ...e }))
}
