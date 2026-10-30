// 上传本地图片到搜索引擎：只在点击时、只发给被点的那个引擎。
const $ = (id) => document.getElementById(id)

function postForm(action, fields, files = {}) {
  const f = document.createElement('form')
  f.method = 'post'
  f.action = action
  f.target = '_blank'
  if (Object.keys(files).length) f.enctype = 'multipart/form-data'
  for (const [k, v] of Object.entries(fields)) {
    const i = document.createElement('input')
    i.type = 'hidden'
    i.name = k
    i.value = v
    f.append(i)
  }
  for (const [k, file] of Object.entries(files)) {
    const i = document.createElement('input')
    i.type = 'file'
    i.name = k
    const dt = new DataTransfer()
    dt.items.add(file)
    i.files = dt.files
    f.append(i)
  }
  document.body.append(f)
  f.submit()
  f.remove()
}

const toBase64 = (file) =>
  new Promise((ok, no) => {
    const r = new FileReader()
    r.onload = () => ok(String(r.result).split(',')[1] || '')
    r.onerror = () => no(r.error)
    r.readAsDataURL(file)
  })

const ENGINES = {
  tineye: (file) => postForm('https://tineye.com/search', {}, { image: file }),
  google: (file) => postForm('https://www.google.com/searchbyimage/upload', { image_content: '' }, { encoded_image: file }),
  bing: async (file) => postForm('https://www.bing.com/images/search?view=detailv2&iss=sbiupload&FORM=SBIHMP', { imageBin: await toBase64(file) }),
}

for (const b of document.querySelectorAll('button[data-e]')) {
  b.onclick = async () => {
    const file = $('file').files[0]
    if (!file) {
      $('msg').textContent = '先选择一张图片'
      $('msg').className = 'err'
      return
    }
    $('msg').textContent = ''
    await ENGINES[b.dataset.e](file)
  }
}
