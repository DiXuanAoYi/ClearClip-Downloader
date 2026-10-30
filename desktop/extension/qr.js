// 本地生成二维码（vendor/qrcode.js，MIT），不联网。返回 SVG 文本。

/** 生成二维码 SVG；内容太长放不下时返回 null。 */
function qrSvg(text, { margin = 2, dark = '#000000', light = '#ffffff' } = {}) {
  const make = (level) => {
    qrcode.stringToBytes = qrcode.stringToBytesFuncs['UTF-8']
    const q = qrcode(0, level)
    q.addData(String(text), 'Byte')
    q.make()
    return q
  }
  let q
  try {
    q = make('M')
  } catch {
    try {
      q = make('L')
    } catch {
      return null
    }
  }
  const n = q.getModuleCount()
  const size = n + margin * 2
  let path = ''
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      if (q.isDark(r, c)) path += `M${c + margin} ${r + margin}h1v1h-1z`
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}" shape-rendering="crispEdges"><rect width="${size}" height="${size}" fill="${light}"/><path d="${path}" fill="${dark}"/></svg>`
}
