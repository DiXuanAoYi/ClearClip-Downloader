// 解除复制 / 右键 / 选择限制。只会被注册到用户开启过的网站，运行在页面自己的环境（MAIN）里，尽早运行，
// 这样才能赶在网站的脚本之前改掉 preventDefault。不读取页面内容，不联网，没有扩展接口。
;(() => {
  if (window.__ccUnlock) return
  Object.defineProperty(window, '__ccUnlock', { value: { off: false }, enumerable: false })
  const state = window.__ccUnlock

  // 被网站拦截的事件：复制、剪切、右键菜单、开始选择、拖拽。鼠标按下 / 抬起不动，很多网页控件要用它们。
  const BLOCKED = new Set(['copy', 'cut', 'contextmenu', 'selectstart', 'dragstart'])
  // 键盘上只放行和复制有关的组合键：Ctrl/⌘ + A / C / X
  const isCopyKey = (e) => e.type === 'keydown' && (e.ctrlKey || e.metaKey) && !e.altKey && /^[acx]$/i.test(e.key || '')
  let hits = 0
  let banner = null

  const shouldPass = (e) => !state.off && e && e.isTrusted && (BLOCKED.has(e.type) || isCopyKey(e))

  // 1. 网站调用 preventDefault 阻止默认行为：对这些事件不执行
  const proto = Event.prototype
  const origPrevent = proto.preventDefault
  proto.preventDefault = function preventDefault() {
    if (shouldPass(this)) {
      hit()
      return
    }
    return origPrevent.apply(this, arguments)
  }
  // 老写法：event.returnValue = false
  const rv = Object.getOwnPropertyDescriptor(proto, 'returnValue')
  if (rv && rv.set) {
    Object.defineProperty(proto, 'returnValue', {
      configurable: true,
      enumerable: rv.enumerable,
      get: rv.get,
      set(v) {
        if (v === false && shouldPass(this)) {
          hit()
          return
        }
        rv.set.call(this, v)
      },
    })
  }

  // 2. 内联 / 属性式处理函数（oncopy="return false"、document.oncontextmenu = () => false）：
  //    处理函数还是照常执行，只是 return false 不再取消默认行为
  const HANDLERS = ['oncopy', 'oncut', 'oncontextmenu', 'onselectstart', 'ondragstart', 'onkeydown']
  const wrapped = new WeakMap()
  const wrapFn = (fn, type) => {
    if (typeof fn !== 'function') return fn
    let w = wrapped.get(fn)
    if (!w) {
      w = function (e) {
        const r = fn.apply(this, arguments)
        if (r === false && shouldPass(e || { type })) {
          hit()
          return undefined
        }
        return r
      }
      wrapped.set(fn, w)
    }
    return w
  }
  const originals = new Map() // 包装后的函数 → 网站原来设置的函数，读取时还给网站
  for (const target of [window, Document.prototype, HTMLElement.prototype, Element.prototype]) {
    for (const name of HANDLERS) {
      const d = Object.getOwnPropertyDescriptor(target, name)
      if (!d || !d.set || !d.get) continue
      const type = name.slice(2)
      Object.defineProperty(target, name, {
        configurable: true,
        enumerable: d.enumerable,
        get() {
          const v = d.get.call(this)
          return originals.get(v) || v
        },
        set(fn) {
          const w = wrapFn(fn, type)
          if (w !== fn) originals.set(w, fn)
          d.set.call(this, w)
        },
      })
    }
  }

  // 2b. HTML 属性写的处理函数（<body oncopy="return false">）不经过上面的赋值，事件发生时在最前面把它拿掉
  for (const type of BLOCKED) {
    window.addEventListener(type, (e) => {
      if (state.off || !e.isTrusted) return
      for (const el of e.composedPath()) {
        if (el.nodeType === 1 && el.hasAttribute('on' + type)) {
          el.removeAttribute('on' + type)
          hit()
        }
      }
    }, true)
  }

  // 3. 样式：允许选中文字，恢复右键光标
  const css = '*, *::before, *::after { -webkit-user-select: text !important; user-select: text !important; -webkit-touch-callout: default !important; }'
  const style = document.createElement('style')
  style.setAttribute('data-clearclip', 'unlock')
  style.textContent = css
  const addStyle = () => {
    if (!state.off && !style.isConnected) (document.head || document.documentElement).append(style)
  }
  addStyle()
  document.addEventListener('DOMContentLoaded', addStyle)

  // 4. 提示条：第一次真的拦住了网站的限制时，告诉用户“这里解除了”，并给一个撤销按钮
  function hit() {
    if (++hits !== 1) return
    const show = () => {
      if (banner || !document.body) return
      const host = document.createElement('div')
      host.style.cssText = 'all:initial;position:fixed;right:16px;bottom:16px;z-index:2147483647'
      const root = host.attachShadow({ mode: 'closed' })
      root.innerHTML = '<style>div{font:13px/1.5 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif;background:#1d2420;color:#fff;border-radius:8px;padding:8px 12px;box-shadow:0 6px 20px #0004;display:flex;gap:10px;align-items:center}button{font:inherit;cursor:pointer;border:1px solid #ffffff66;background:transparent;color:#fff;border-radius:6px;padding:2px 8px}</style><div><span>清影已解除这个网页的复制 / 右键限制</span><button id="u">撤销</button><button id="x">×</button></div>'
      root.getElementById('u').onclick = () => {
        state.off = true
        style.remove()
        host.remove()
      }
      root.getElementById('x').onclick = () => host.remove()
      document.body.append(host)
      banner = host
      setTimeout(() => host.remove(), 6000)
    }
    if (document.body) show()
    else document.addEventListener('DOMContentLoaded', show, { once: true })
  }
})()
