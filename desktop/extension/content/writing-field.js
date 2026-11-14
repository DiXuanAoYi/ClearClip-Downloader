// 写作浮窗的拆分模块（由 content/writing.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 输入框：找到有焦点的可编辑元素，读取它的文字、选区和位置。
;(() => {
  const P = ((globalThis.ccParts ||= {}).write ||= {})
  P.field = (C) => {

    function deepActive() {
      let a = document.activeElement
      while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement
      return a
    }

    function editableRoot(el) {
      if (!el || el.nodeType !== 1) return null
      if (el.matches?.('textarea, input')) return el
      if (el.isContentEditable) {
        let r = el
        while (r.parentElement && r.parentElement.isContentEditable) r = r.parentElement
        return r
      }
      return null
    }

    function infoOf(el) {
      const r = el.getBoundingClientRect()
      const cs = getComputedStyle(el)
      return {
        tag: el.tagName,
        type: el.type,
        editable: el.isContentEditable,
        disabled: el.disabled,
        readOnly: el.readOnly || el.getAttribute('aria-readonly') === 'true',
        autocomplete: el.getAttribute('autocomplete') || '',
        name: el.getAttribute('name') || '',
        id: el.id || '',
        placeholder: el.getAttribute('placeholder') || '',
        ariaLabel: el.getAttribute('aria-label') || '',
        width: r.width,
        height: r.height,
        hidden: cs.visibility === 'hidden' || cs.display === 'none' || r.width === 0,
      }
    }

    const isField = (el) => el.matches?.('textarea, input')
    const readAll = (el) => (isField(el) ? el.value : el.innerText || '')

    function snapshot(el) {
      if (!el) return null
      const full = readAll(el)
      if (isField(el)) {
        let s = 0
        let e = 0
        try {
          s = el.selectionStart ?? 0
          e = el.selectionEnd ?? 0
        } catch {
          // 有些输入类型不支持读取选区
        }
        return { el, full, start: s, end: e, sel: full.slice(s, e) }
      }
      const sel = getSelection()
      let range = null
      let selText = ''
      if (sel && sel.rangeCount && el.contains(sel.anchorNode)) {
        range = sel.getRangeAt(0).cloneRange()
        selText = sel.toString()
      }
      return { el, full, range, sel: selText }
    }

    Object.assign(C, { readAll, editableRoot, infoOf, deepActive, snapshot, isField })
  }
})()
