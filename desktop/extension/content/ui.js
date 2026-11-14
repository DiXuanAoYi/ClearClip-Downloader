// 内容脚本共用的界面工具：在页面里放一个“封闭的 Shadow DOM”容器。页面的样式和脚本碰不到里面，里面的样式也不会影响页面。
;(() => {
  if (globalThis.ccUI) return
  const api = globalThis.browser ?? globalThis.chrome

  // 主题令牌：跟随系统，也跟随摘要面板里选的 浅色 / 深色（host 的 data-theme）
  const LIGHT = '--bg:#fff;--card:#fff;--soft:#e4ebf5;--line:#aebfd8;--fg:#0b1424;--mute:#4d5f7b;--acc:#b45f00;--accsoft:#f6e5cc;--onacc:#fff;--bad:#b3261e;--shade:#0005;--inp:#f1f5fb;--ok:#007a70;'
  const DARKV = '--bg:#0b1424;--card:#0e1625;--soft:#0e1625;--line:#2c4366;--fg:#e8f0fd;--mute:#9aabc6;--acc:#ffb547;--accsoft:#3a2c1a;--onacc:#060a12;--bad:#ff7b8a;--shade:#000a;--inp:#060a12;--ok:#4be3d0;color-scheme:dark;'
  const BASE = `
    :host { all: initial; ${LIGHT} }
    @media (prefers-color-scheme: dark) { :host(:not([data-theme="light"])) { ${DARKV} } }
    :host([data-theme="dark"]) { ${DARKV} }
    * { box-sizing: border-box; font: 13px/1.5 -apple-system, "PingFang SC", "Microsoft YaHei", "Noto Sans SC", sans-serif; letter-spacing: normal; }
    button { cursor: pointer; border: 0; border-radius: 10px; padding: 5px 10px; color: var(--onacc); background: var(--acc); font-weight: 600; }
    button.sec { background: var(--card); color: var(--fg); border: 1px solid var(--line); font-weight: 400; }
    button { transition: filter .12s, transform .08s; }
    button:hover:not(:disabled) { filter: brightness(1.1); }
    button:active:not(:disabled) { transform: translateY(1px) scale(.97); }
    button:focus-visible { outline: 2px solid #4be3d0; outline-offset: 2px; }
    .layer { position: fixed; z-index: 2147483647; }
    .menu { background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 12px; box-shadow: 0 10px 28px #0003; padding: 4px; min-width: 220px; max-width: 360px; }
    .menu .it { display: block; width: 100%; text-align: left; background: transparent; color: inherit; font-weight: 400; border-radius: 4px; padding: 6px 10px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .menu .it:hover { background: #b45f0024; }
    .menu .hd { font-size: 11px; color: var(--mute); padding: 4px 10px; }
    .toast { background: var(--fg); color: var(--bg); border-radius: 12px; padding: 8px 14px; box-shadow: 0 6px 20px #0004; }
    .toast.err { background: #c4283d; color: #fff; }
  `

  /** 创建容器。返回 { host, root, add, remove }：add 往里放节点。放在全屏元素里才看得见，所以全屏变化时跟着挪。 */
  function mount(css = '') {
    const host = document.createElement('div')
    host.style.cssText = 'all:initial;position:fixed;left:0;top:0;width:0;height:0;z-index:2147483647;'
    const root = host.attachShadow({ mode: 'closed' })
    const style = document.createElement('style')
    style.textContent = BASE + css
    root.append(style)
    // 模态对话框（<dialog>.showModal()）打开时，对话框外面的东西都是“惰性”的，点不了、打不了字：放进对话框里
    const modal = () => {
      try {
        return document.querySelector('dialog:modal')
      } catch {
        return null
      }
    }
    /** `preferred`：想放进去的容器（网页里的对话框 / 焦点陷阱）。放进去后如果容器有 transform，fixed 定位会错位，就退回到最外层。 */
    const place = (preferred) => {
      const el = preferred instanceof Element ? preferred : null
      const parent = document.fullscreenElement || el || modal() || document.documentElement
      if (host.parentNode !== parent) parent.append(host)
      if (parent !== document.documentElement && parent !== document.fullscreenElement && parent !== modal()) {
        const r = host.getBoundingClientRect()
        if (Math.abs(r.left) > 1 || Math.abs(r.top) > 1) document.documentElement.append(host)
      }
    }
    const auto = () => place()
    place()
    document.addEventListener('fullscreenchange', auto)
    document.addEventListener('close', auto, true)
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
      place,
      remove: () => {
        document.removeEventListener('fullscreenchange', auto)
        document.removeEventListener('close', auto, true)
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

  // ---------- 生命周期：统一的清理函数 ----------
  // 每个内容脚本把“移除监听 / 定时器 / MutationObserver / DOM 节点”的清理函数登记在这里。
  // 扩展被更新或卸载后，页面里的旧脚本就成了孤儿（runtime 失效，sendMessage 抛“Extension context invalidated”）：
  // 这时统一执行所有清理函数，别继续在页面上空转（定时器、观察整个文档的 MutationObserver 都很贵）。
  const hadId = (() => {
    try {
      return !!api.runtime?.id
    } catch {
      return false
    }
  })()
  const disposers = new Set()
  let disposed = false
  const life = {
    get alive() {
      return !disposed
    },
    /** 登记一个清理函数，返回“注销它”的函数（功能自己先清理完时调用）。已经被清理过的页面里立刻执行。 */
    add(fn) {
      if (disposed) {
        try {
          fn()
        } catch {
          // 已经在清理
        }
        return () => {}
      }
      disposers.add(fn)
      return () => disposers.delete(fn)
    },
    /** 加事件监听，并登记移除。 */
    on(target, type, fn, opts) {
      target.addEventListener(type, fn, opts)
      return life.add(() => target.removeEventListener(type, fn, opts))
    },
    /** 开定时器，并登记清除。返回定时器编号。 */
    interval(fn, ms) {
      const id = setInterval(fn, ms)
      life.add(() => clearInterval(id))
      return id
    },
    /** 开始观察并登记断开。 */
    observe(mo, target, opts) {
      mo.observe(target, opts)
      life.add(() => mo.disconnect())
      return mo
    },
    /** 扩展失效了吗：被更新 / 卸载后 runtime.id 读不到。 */
    orphaned() {
      if (!hadId) return false // 加载时就读不到 id（测试里的假接口）：不据此判断
      try {
        return !api.runtime?.id
      } catch {
        return true
      }
    },
    /** 孤儿了就清理。返回脚本是否还活着。 */
    check() {
      if (!disposed && life.orphaned()) life.disposeAll()
      return !disposed
    },
    /** 执行所有清理函数（后登记的先执行），一个出错不影响其余。 */
    disposeAll() {
      if (disposed) return
      disposed = true
      for (const fn of [...disposers].reverse()) {
        try {
          fn()
        } catch {
          // 清理时页面可能已经变了
        }
      }
      disposers.clear()
    },
  }

  /** 给后台发消息；后台没就绪或扩展刚更新时返回 { ok: false, error }。发现扩展已失效时顺便清理本页所有内容脚本。 */
  async function send(msg) {
    if (disposed) return { ok: false, error: '扩展已更新或被卸载，请刷新这个网页' }
    try {
      return (await api.runtime.sendMessage(msg)) || { ok: false, error: '没有响应' }
    } catch (e) {
      if (life.orphaned() || /context invalidated/i.test(String(e?.message || e))) life.disposeAll()
      return { ok: false, error: '扩展刚更新，请刷新这个网页后再试' }
    }
  }

  /**
   * 统一的 AI 调用：交给清影，等结果。所有内容脚本都用它，不再各写一套轮询。
   * opts：interval 查询间隔（毫秒）；stop() 返回真就放弃并通知清影取消（keepOnStop 时不取消，清影继续跑）；
   * onProgress(job) 进度；onSubmit(id)；timeoutMs 超时；id 接上一个已经在跑的任务（不再提交）。
   */
  async function aiRun(req, opts = {}) {
    const sub = opts.id ? { ok: true, id: opts.id } : await send({ type: 'cc-ai-run', req })
    if (!sub.ok) throw new Error(sub.error || '提交失败')
    opts.onSubmit?.(sub.id)
    const deadline = opts.timeoutMs ? Date.now() + opts.timeoutMs : Infinity
    const cancel = () => send({ type: 'cc-ai-cancel', id: sub.id })
    for (;;) {
      await new Promise((r) => setTimeout(r, opts.interval || 500))
      if (opts.stop?.()) {
        if (!opts.keepOnStop) cancel()
        throw new Error('已取消')
      }
      if (Date.now() > deadline) {
        cancel()
        throw new Error('等太久了，已取消')
      }
      const j = await send({ type: 'cc-ai-job', id: sub.id })
      if (!j.ok) throw new Error(j.error || '查询失败')
      opts.onProgress?.(j)
      if (j.state === 'done') return j.result || {}
      if (j.state === 'error') throw new Error(j.error || '失败了')
      if (j.state === 'cancelled' || j.state === 'canceled') throw new Error('已取消')
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
      const fn = (ch, area) => {
        if (area === 'local' && (ch.siteRules || ch.features)) cb()
      }
      api.storage.onChanged.addListener(fn)
      life.add(() => api.storage.onChanged.removeListener(fn))
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

  // ---------- 吞掉的错误留痕、存储写失败提示 ----------

  const logCounts = {}
  /** 记一条调试日志：不打扰用户，但“静默吞掉”的错误至少留个痕迹（页面控制台里筛选 [cc]）。同一处最多记 20 次。 */
  function log(where, e) {
    logCounts[where] = (logCounts[where] || 0) + 1
    if (logCounts[where] > 20) return
    try {
      console.debug('[cc]', where, e?.message || e)
    } catch {
      // 控制台不可用
    }
  }

  /** 执行 fn，出错记日志并返回 fallback。fn 可以是同步或异步函数。 */
  async function safe(fn, fallback, where = 'safe') {
    try {
      return await fn()
    } catch (e) {
      log(where, e)
      return fallback
    }
  }

  const isQuota = (e) => /quota|QUOTA_BYTES|storage.*(full|exceed)/i.test(String(e?.message || e))
  let lastSaveToast = 0
  /** 存储写失败：记日志，并提示用户（同一页面 30 秒内只提示一次，免得连环弹）。ui 是 mount() 的结果。 */
  function saveFailed(ui, where, e) {
    log(where, e)
    if (!ui) return false // 没有界面可提示（后台式的写入）：只记日志
    const now = Date.now()
    if (now - lastSaveToast < 30_000) return false
    lastSaveToast = now
    try {
      toast(ui, isQuota(e) ? '保存失败：扩展的存储空间满了。请在“视频摘要 → 更多”里清除缓存或删除不要的笔记' : '保存失败：这次的改动没有存下来，刷新页面后可能丢失', true, 5200)
    } catch {
      // 容器已经关了
    }
    return true
  }

  /**
   * 写 storage.local：失败时（多半是 10MB 配额满了）先清掉可以再生成的缓存（cacheKeys）重试一次，再失败就 saveFailed 提示。
   * 返回是否写成功。
   */
  async function storeSet(ui, obj, cacheKeys = ['vsumCache', 'transCache']) {
    try {
      await api.storage.local.set(obj)
      return true
    } catch (e) {
      let err = e
      if (isQuota(e)) {
        try {
          await api.storage.local.remove(cacheKeys.filter((k) => !(k in obj)))
          await api.storage.local.set(obj)
          return true
        } catch (e2) {
          err = e2
        }
      }
      saveFailed(ui, `storage.set ${Object.keys(obj).join(',')}`, err)
      return false
    }
  }

  globalThis.ccUI = { api, life, mount, h, send, aiRun, siteRules, hostOff, onSettings, toast, listed, log, safe, saveFailed, storeSet, isQuota }
})()
