// 页面截图的纯计算：整页怎么分段滚动、超长页面怎么分块、文件名。不碰浏览器接口，可以单元测试。

/** 整页滚动截图的分段：每段滚到 scrollY，取截图里从 srcTop 开始的 height 个 CSS 像素，贴到结果的 destTop。 */
function planSlices(total, view) {
  const t = Math.max(0, Math.floor(total))
  const v = Math.max(1, Math.floor(view))
  const out = []
  const maxScroll = Math.max(0, t - v)
  for (let top = 0; top < t; top += v) {
    const scrollY = Math.min(top, maxScroll)
    out.push({ scrollY, srcTop: top - scrollY, destTop: top, height: Math.min(v, t - top) })
  }
  return out
}

/** 单个画布的高度上限（CSS 像素）：浏览器画布边长和总面积都有上限。 */
function chunkHeight(widthCss, dpr, edge = 16384, area = 268435456) {
  const w = Math.max(1, Math.round(widthCss * dpr))
  return Math.max(256, Math.floor(Math.min(edge, Math.floor(area / w)) / dpr))
}

/** 把整页高度切成若干块（每块一个文件）。返回 [{ from, to }]。 */
function planChunks(total, maxH) {
  const out = []
  for (let from = 0; from < total; from += maxH) out.push({ from, to: Math.min(total, from + maxH) })
  return out
}

const pad2 = (n) => String(n).padStart(2, '0')

/** 截图 / 保存文件名：标题-年月日-时分秒[-序号].扩展名 */
function captureName(title, date, ext, part = 0, parts = 1) {
  const base = String(title || 'page').replace(/[\\/:*?"<>|\s]+/g, ' ').trim().slice(0, 50) || 'page'
  const d = date
  const stamp = `${d.getFullYear()}${pad2(d.getMonth() + 1)}${pad2(d.getDate())}-${pad2(d.getHours())}${pad2(d.getMinutes())}${pad2(d.getSeconds())}`
  return `${base}-${stamp}${parts > 1 ? `-${part + 1}` : ''}.${ext}`
}

/** 摘录存成文字便签的格式：原文 + 来源。 */
function excerptText(selection, title, url) {
  return `${String(selection).trim()}\n\n—— ${title || ''}\n${url || ''}`.trim()
}
