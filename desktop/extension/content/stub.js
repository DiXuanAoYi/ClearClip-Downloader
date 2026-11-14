// 视频类常驻功能（视频上的“下载”胶囊、播放增强、实时音效）的探测 stub：几百字节，注册到每个网页和 frame 里，
// 只做一件事：页面里出现 <video> / <audio> 时，请后台把对应功能的主体脚本注入到本 frame。没有视频的页面（绝大多数 frame、广告 iframe）
// 不再加载 ui.js / vdock.js / 一堆 logic。主体脚本本身是幂等的，和以前直接注册时的行为一致。
;(() => {
  if (globalThis.__ccStub) return
  globalThis.__ccStub = true
  const api = globalThis.browser ?? globalThis.chrome
  const hasMedia = () => {
    try {
      return !!document.querySelector('video,audio')
    } catch {
      return false
    }
  }
  let mo = null
  let timer = 0
  const stop = () => {
    clearTimeout(timer)
    if (mo) mo.disconnect()
    mo = null
  }
  const load = () => {
    stop()
    try {
      const p = api.runtime.sendMessage({ type: 'cc-stub-load' })
      if (p && p.catch) p.catch(() => {})
    } catch {
      // 扩展刚更新 / 后台没就绪：什么也不做
    }
  }
  if (hasMedia()) return load()
  const check = () => {
    try {
      if (!api.runtime?.id) return stop() // 扩展已更新 / 卸载：孤儿 stub 自己退出
    } catch {
      return stop()
    }
    if (hasMedia()) load()
  }
  mo = new MutationObserver(() => {
    if (timer) return
    timer = setTimeout(() => {
      timer = 0
      check()
    }, 300)
  })
  mo.observe(document, { childList: true, subtree: true })
})()
