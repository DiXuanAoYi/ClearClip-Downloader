// 网页翻译的拆分模块（由 content/webtrans.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 悬停翻译：单击一下修饰键，翻译鼠标下面的那一段。
;(() => {
  const P = ((globalThis.ccParts ||= {}).trans ||= {})
  P.hover = (C) => {
    const { INLINE, S, toast } = C
    const blockOf = (...a) => C.blockOf(...a)
    const ensureStyle = (...a) => C.ensureStyle(...a)
    const run = (...a) => C.run(...a)
    const skippable = (...a) => C.skippable(...a)
    const textOf = (...a) => C.textOf(...a)
    const uiRoot = (...a) => C.uiRoot(...a)
    const unhide = (...a) => C.unhide(...a)

    // ---------- 悬停翻译：单击一下修饰键 ----------

    let tapKey = null
    let sweepTimer = 0
    document.addEventListener(
      'mousemove',
      (e) => {
        const el = e.target instanceof Element ? e.target : null
        S.lastBlock = el
        S.lastNode = null
        // 按住修饰键已经进入“扫描”状态：鼠标停在哪一段超过一小会儿，就翻译那一段（已翻译的不动）
        if (held) {
          clearTimeout(sweepTimer)
          sweepTimer = setTimeout(() => held && hoverTranslate(true), 140)
        }
      },
      { passive: true, capture: true },
    )
    // 按住：超过 holdMs 就翻译（不用松手）；松手早于这个时间算单击。其他键或鼠标按下都取消。
    let holdTimer = 0
    let held = false
    const clearHold = () => {
      clearTimeout(holdTimer)
      holdTimer = 0
    }
    // 切到别的窗口时收不到松开键的事件，不重置的话回来就一直处在“扫描”状态
    addEventListener('blur', () => {
      clearHold()
      clearTimeout(sweepTimer)
      tapKey = null
      held = false
    })
    document.addEventListener('keydown', (e) => {
      if (e.repeat && e.key === S.cfg.hover) return
      tapKey = S.cfg.hover !== 'off' && e.key === S.cfg.hover ? e.key : null
      if (e.key !== S.cfg.hover) {
        tapKey = null
        clearHold()
        return
      }
      held = false
      clearHold()
      if (tapKey && S.cfg.holdMs > 0) {
        holdTimer = setTimeout(() => {
          holdTimer = 0
          if (!tapKey) return
          held = true
          // 按住只负责“翻译”，已经翻过的段落不动（还原用单击）
          hoverTranslate(true)
        }, S.cfg.holdMs)
      }
    }, true)
    document.addEventListener('mousedown', () => ((tapKey = null), clearHold()), true)
    document.addEventListener('keyup', (e) => {
      clearHold()
      clearTimeout(sweepTimer)
      if (tapKey && e.key === tapKey && !held) hoverTranslate()
      tapKey = null
      held = false
    }, true)

    function blockUnder(el) {
      let e = el
      while (e && e !== document.body) {
        if (!skippable(e)) {
          const own = [...e.childNodes].some((n) => n.nodeType === 3 && n.nodeValue.trim().length >= 2)
          if (own && !INLINE.test(getComputedStyle(e).display)) return e
          if (own) {
            // 行内元素：用它所在的段落
            const t = [...e.childNodes].find((n) => n.nodeType === 3 && n.nodeValue.trim())
            return blockOf(t)
          }
        }
        e = e.parentElement
      }
      return null
    }

    let hoverDepth = 0
    let hoverWasOn = false
    async function hoverTranslate(onlyTranslate = false) {
      const b = S.lastBlock && blockUnder(S.lastBlock)
      if (!b) return
      ensureStyle()
      const rec = S.blocks.get(b)
      if (rec && !rec.node && rec.state === 'busy') return // 这一段正在翻
      if (rec?.node) {
        if (onlyTranslate) return
        // 再点一次 = 还原这一段
        rec.node.remove()
        unhide(b)
        S.blocks.delete(b)
        return
      }
      const { text, last } = textOf(b)
      if (!text) return
      S.blocks.set(b, { text, last, state: 'busy', node: null })
      // 扫过好几段时会同时有几个在翻：只有最先开始的记下“整页翻译开没开”，最后一个结束时才还原
      if (hoverDepth++ === 0) hoverWasOn = S.on
      S.on = true // 让 run() 在没开整页时也能工作
      try {
        await run([{ id: b, text }])
      } finally {
        if (--hoverDepth === 0) {
          S.on = hoverWasOn
          if (!hoverWasOn) {
            C.pill?.remove()
            C.pill = null
          }
        }
      }
      if (!hoverWasOn && S.blocks.get(b)?.state === 'fail') toast(uiRoot(), S.err || '翻译失败了', true, 4000)
    }

  }
})()
