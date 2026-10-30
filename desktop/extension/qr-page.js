// 右键菜单“用二维码在手机上打开此页”打开的小窗口。
const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }
const url = new URLSearchParams(location.search).get('u') || ''
$('url').textContent = url
const svg = qrSvg(url)
if (svg) {
  const img = document.createElement('img')
  img.alt = '二维码'
  img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
  $('qrBox').append(img)
} else {
  $('qrBox').textContent = '链接太长，放不下二维码'
}
$('copy').onclick = async () => {
  try {
    await navigator.clipboard.writeText(url)
    msg('已复制', 'ok')
  } catch {
    msg('复制失败，请手动选中上面的链接', 'err')
  }
}
$('toPhone').onclick = async () => {
  try {
    const c = await getConfig()
    const r = await call('/api/to_phone', { text: url, device: c.device })
    msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发到手机盒子，手机上打开清影网页即可看到', 'ok')
  } catch (e) {
    msg(e.message, 'err')
  }
}
