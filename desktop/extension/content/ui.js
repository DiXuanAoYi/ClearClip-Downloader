// 内容脚本共用的界面工具：在页面里放一个“封闭的 Shadow DOM”容器。页面的样式和脚本碰不到里面，里面的样式也不会影响页面。
;(() => {
  if (globalThis.ccUI) return
  const api = globalThis.browser ?? globalThis.chrome

  // 主题令牌：跟随系统，也跟随摘要面板里选的 浅色 / 深色（host 的 data-theme）
  const LIGHT = '--bg:#fff;--card:#fff;--soft:#f3f1ec;--line:#cfd5d1;--fg:#1d2420;--mute:#5d6862;--acc:#b4561a;--accsoft:#f6e6da;--onacc:#fff;--bad:#b3261e;--shade:#0005;'
  const DARKV = '--bg:#1d2420;--card:#262f2a;--soft:#262f2a;--line:#3a443e;--fg:#eef1ee;--mute:#a9b3ad;--acc:#f0a064;--accsoft:#3a2c20;--onacc:#14181a;--bad:#ee917b;--shade:#000a;color-scheme:dark;'
  const BASE = `
    :host { all: initial; ${LIGHT} }
    @media (prefers-color-scheme: dark) { :host(:not([data-theme="light"])) { ${DARKV} } }
    :host([data-theme="dark"]) { ${DARKV} }
    * { box-sizing: border-box; font: 13px/1.5 -apple-system, "PingFang SC", "Microsoft YaHei", "Noto Sans SC", sans-serif; letter-spacing: normal; }
    button { cursor: pointer; border: 0; border-radius: 6px; padding: 5px 10px; color: var(--onacc); background: var(--acc); font-weight: 600; }
    button.sec { background: var(--card); color: var(--fg); border: 1px solid var(--line); font-weight: 400; }
    button:focus-visible { outline: 2px solid #d9762b; outline-offset: 2px; }
    .layer { position: fixed; z-index: 2147483647; }
    .menu { background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 10px 28px #0003; padding: 4px; min-width: 220px; max-width: 360px; }
    .menu .it { display: block; width: 100%; text-align: left; background: transparent; color: inherit; font-weight: 400; border-radius: 4px; padding: 6px 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .menu .it:hover { background: #d9762b24; }
    .menu .hd { font-size: 11px; color: var(--mute); padding: 4px 10px; }
    .toast { background: var(--fg); color: var(--bg); border-radius: 8px; padding: 8px 14px; box-shadow: 0 6px 20px #0004; }
    .toast.err { background: #a8362d; }
  `

  /** 创建容器。返回 { host, root, add, remove }：add 往里放节点。放在全屏元素里才看得见，所以全屏变化时跟着挪。 */
  function mount(css = '') {
    const host = document.createElement('div')
    host.style.cssText = 'all:initial;position:fixed;left:0;top:0;width:0;height:0;z-index:2147483647;'
    const root = host.attachShadow({ mode: 'closed' })
    const style = document.createElement('style')
    style.textContent = BASE + css
    root.append(style)
    const place = () => {
      const parent = document.fullscreenElement || document.documentElement
      if (host.parentNode !== parent) parent.append(host)
    }
    place()
    document.addEventListener('fullscreenchange', place)
    try {
      void Promise.resolve(api.storage?.local?.get('vsumPrefs'))
        .then((r) => {
          const t = r?.vsumPrefs?.theme
          if (t === 'dark' || t === 'light') host.setAttribute('data-theme', t)
        })
        .catch(() => {})
    } catch {
      /* 读不到就跟随系统 */
    }
    return {
      host,
      root,
      add: (n) => root.append(n),
      remove: () => {
        document.removeEventListener('fullscreenchange', place)
        host.remove()
      },
    }
  }

  /** h('button', { class: 'x', text: '文字', onclick: fn, style: '...' }, ...children) */
  function h(tag, attrs = {}, ...kids) {
    const el = document.createElement(tag)
    for (const [k, v] of Object.entries(attrs)) {
      if (v == null || v === false) continue
      if (k === 'text') el.textContent = v
      else if (k === 'class') el.className = v
      else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2), v)
      else el.setAttribute(k, v === true ? '' : v)
    }
    el.append(...kids.filter((x) => x != null && x !== false))
    return el
  }

  /** 给后台发消息；后台没就绪或扩展刚更新时返回 { ok: false, error }。 */
  async function send(msg) {
    try {
      return (await api.runtime.sendMessage(msg)) || { ok: false, error: '没有响应' }
    } catch {
      return { ok: false, error: '扩展刚更新，请刷新这个网页后再试' }
    }
  }

  async function siteRules() {
    try {
      const { siteRules: r } = await api.storage.local.get('siteRules')
      return r && typeof r === 'object' ? r : {}
    } catch {
      return {}
    }
  }

  const listed = (list, host) => (Array.isArray(list) ? list : []).some((s) => host === s || host.endsWith('.' + s))

  /** 这个网站是否在 siteRules.off[key] 名单里（key：floatButton / video / hoverZoom）。 */
  async function hostOff(key) {
    const r = await siteRules()
    return listed(r.off?.[key], location.hostname.toLowerCase())
  }

  /** 设置（功能开关、站点名单）变了的回调。 */
  function onSettings(cb) {
    try {
      api.storage.onChanged.addListener((ch, area) => {
        if (area === 'local' && (ch.siteRules || ch.features)) cb()
      })
    } catch {
      // 扩展已失效
    }
  }

  /** 在容器里短暂显示一条提示。 */
  function toast(ui, text, bad = false, ms = 2600, pos = 'right:16px;bottom:16px') {
    const t = h('div', { class: `layer toast${bad ? ' err' : ''}`, style: pos, text })
    ui.add(t)
    setTimeout(() => t.remove(), ms)
    return t
  }

  globalThis.ccUI = { api, mount, h, send, siteRules, hostOff, onSettings, toast, listed }
})()
