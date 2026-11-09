// 批量发送时的命名：模板（{序号} {标题} {原名} {站点} {日期} {类型}）、文件名清理、去重。纯逻辑，后台和总览页共用。
;(function () {
  const BAD = /[\\/:*?"<>|\u0000-\u001f]/g

  /** 清理成可做文件名的一段：去掉非法字符、合并空白、限长；清理后为空返回空串。 */
  function cleanName(s, max = 80) {
    const t = String(s ?? '')
      .replace(BAD, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      .replace(/^\.+/, '')
      .replace(/[. ]+$/, '')
    return Array.from(t).slice(0, max).join('').trim()
  }

  /** 去掉文件名末尾的扩展名（.jpg、.mp4 …）。 */
  const stripExt = (s) => String(s ?? '').replace(/\.[A-Za-z0-9]{2,5}$/, '')

  const KIND_ZH = { image: '图片', video: '视频', audio: '音频' }

  /**
   * 按模板给一项生成名字。ctx：{ index（从 0 起）, total, title（网页标题）, orig（原文件名，可带扩展名）, host, kind, date（Date）}。
   * 模板里没有的占位符原样保留；结果为空时退回原名。
   */
  function applyTemplate(tpl, ctx) {
    const d = ctx.date instanceof Date ? ctx.date : new Date()
    const p2 = (n) => String(n).padStart(2, '0')
    const width = Math.max(2, String(ctx.total || 1).length)
    const map = {
      序号: String((ctx.index || 0) + 1).padStart(width, '0'),
      标题: cleanName(ctx.title, 60),
      原名: cleanName(stripExt(ctx.orig), 60),
      站点: cleanName(String(ctx.host || '').replace(/^www\./, ''), 30),
      日期: `${d.getFullYear()}${p2(d.getMonth() + 1)}${p2(d.getDate())}`,
      类型: KIND_ZH[ctx.kind] || '',
      说明: cleanName(ctx.alt, 40),
    }
    const out = String(tpl || '').replace(/\{([^{}]+)\}/g, (m, k) => (k in map ? map[k] : m))
    return cleanName(out) || cleanName(stripExt(ctx.orig)) || ''
  }

  /** 让一组名字互不重复（重名的后面加 -2、-3 …）；空名保持为空。区分大小写无关。 */
  function uniqueNames(names) {
    const seen = new Map()
    return names.map((n) => {
      if (!n) return n
      const key = n.toLowerCase()
      const c = (seen.get(key) || 0) + 1
      seen.set(key, c)
      return c === 1 ? n : `${n}-${c}`
    })
  }

  /** 给一组项目按模板批量生成名字（带去重）。items：[{url, kind, name?}]，orig 取地址最后一段。 */
  function planNames(tpl, items, page) {
    let host = ''
    try {
      host = new URL(page.url).hostname
    } catch {
      // 页面地址不规范：没有站点名
    }
    const origOf = (u) => {
      try {
        return decodeURIComponent(new URL(u).pathname.split('/').filter(Boolean).pop() || '')
      } catch {
        return ''
      }
    }
    const date = page.date instanceof Date ? page.date : new Date()
    return uniqueNames(
      items.map((it, index) => applyTemplate(tpl, { index, total: items.length, title: page.title, orig: origOf(it.url), host, kind: it.kind, date, alt: it.cap || it.alt || '' })),
    )
  }

  /** 把名字加上真实扩展名；名字已带同样的扩展名就不重复加。 */
  function withExt(name, ext) {
    if (!name) return ''
    if (!ext) return name
    return name.toLowerCase().endsWith(`.${ext.toLowerCase()}`) ? name : `${name}.${ext}`
  }

  globalThis.ccNaming = { cleanName, stripExt, applyTemplate, uniqueNames, planNames, withExt }
})()
