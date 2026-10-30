// Markdown 预览页：显示提取结果，可以修改后保存成文件、复制，或作为文字便签发给清影。
const $ = (id) => document.getElementById(id)
const msg = (t, cls = '') => { $('msg').textContent = t; $('msg').className = cls }
const sessionStore = api.storage.session || api.storage.local
let data = { title: '', url: '', markdown: '', selection: false, chars: 0 }

async function init() {
  const key = new URLSearchParams(location.search).get('k')
  const got = key ? (await sessionStore.get(key))[key] : null
  if (!got) {
    $('md').disabled = true
    return msg('预览已过期，请回到原页面重新保存。', 'err')
  }
  data = got
  $('md').value = data.markdown
  document.title = `Markdown · ${data.title || data.url}`
  $('info').textContent = data.selection ? '内容来自你选中的文字' : '内容来自自动识别的正文'
}

$('save').onclick = () => {
  const blob = new Blob([$('md').value], { type: 'text/markdown;charset=utf-8' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = captureName(data.title, new Date(), 'md')
  document.body.append(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(a.href), 4000)
  msg('已保存到下载文件夹', 'ok')
}
$('copy').onclick = async () => {
  try {
    await navigator.clipboard.writeText($('md').value)
    msg('已复制', 'ok')
  } catch {
    msg('复制失败，请手动选中文字复制', 'err')
  }
}
$('toApp').onclick = async () => {
  try {
    const c = await getConfig()
    const r = await call('/api/send', { text: $('md').value.slice(0, 20000), device: c.device, name: browserName(), note: true })
    msg(r.state === 'pending_pair' ? '第一次使用：请在电脑上点“允许”完成配对' : '已发送，在清影的“收到的文件”里查看', 'ok')
  } catch (e) {
    msg(e.message, 'err')
  }
}
init()
