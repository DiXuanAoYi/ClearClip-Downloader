// 按住 Alt 点击时，告诉后台“这次下载留给浏览器”（接管下载默认开启时，临时绕过用）。
// 只发送一个固定的信号，不读取、不记录页面内容。
;(() => {
  const api = globalThis.browser ?? globalThis.chrome
  const ping = (e) => {
    if (!e.altKey) return
    try {
      api.runtime.sendMessage({ type: 'cc-bypass' })
    } catch {
      // 扩展刚更新或后台未就绪
    }
  }
  addEventListener('mousedown', ping, true)
  addEventListener('click', ping, true)
})()
