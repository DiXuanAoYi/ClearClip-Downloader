// 写作浮窗的拆分模块（由 content/writing.js 调用）：状态和工具通过参数 C 传入，导出的函数放回 C，模块之间不直接引用。
// 面板的选项视图、生成请求和进行中视图。
;(() => {
  const P = ((globalThis.ccParts ||= {}).write ||= {})
  P.options = (C) => {
    const { W, h, send } = C
    const asArray = (...a) => C.asArray(...a)
    const buildParts = (...a) => C.buildParts(...a)
    const chip = (...a) => C.chip(...a)
    const header = (...a) => C.header(...a)
    const render = (...a) => C.render(...a)
    const saveCfg = (...a) => C.saveCfg(...a)
    const savePref = (...a) => C.savePref(...a)
    const scopeText = (...a) => C.scopeText(...a)
    const showHistory = (...a) => C.showHistory(...a)

    // ---- 选项视图
    function viewOptions() {
      const s = C.st.sel
      const a = W.actionOf(s.action)
      const kids = []
      const scopeChips = h('div', { class: 'acts' }, h('span', { class: 'mut', text: '处理范围：' }))
      if (C.st.snap.sel.trim()) scopeChips.append(chip(`选中 ${W.countText(C.st.snap.sel)} 字`, C.st.scope === 'sel', () => ((C.st.scope = 'sel'), render())))
      scopeChips.append(chip(`全文 ${W.countText(C.st.snap.full)} 字`, C.st.scope === 'all', () => ((C.st.scope = 'all'), render())))
      kids.push(header('清影写作', scopeChips))
      const bd = h('div', { class: 'bd' })
      if (C.st.info && !C.st.info.ok) bd.append(h('div', { class: 'warn', text: `连不上清影桌面应用：${C.st.info.error || '请确认清影正在运行，并在扩展设置里填好访问令牌。'}` }))
      else if (C.st.info && !C.st.info.ready) bd.append(h('div', { class: 'warn', text: '清影里还没有配置 AI 模型：请在清影的“设置 → AI”里填写，或安装本地模型。' }))
      if (C.st.note) {
        bd.append(h('div', { class: 'warn', text: C.st.note }))
        if (C.st.noteErr) bd.append(h('div', { class: 'acts' }, h('button', { class: 'sec', text: '重试', onclick: () => run() })))
      }

      const pickAction = (key) => {
        s.action = key
        if (C.cfg.runOnClick) run()
        else render()
      }
      bd.append(
        h('div', {}, h('div', { class: 'sec-t', text: '基础' }), h('div', { class: 'chips' }, ...W.BASIC.map((x) => chip(x.name, s.action === x.key, () => pickAction(x.key))))),
        h('div', {}, h('div', { class: 'sec-t', text: '改写风格' }), h('div', { class: 'chips' }, ...W.STYLES.map((x) => chip(x.name, s.action === x.key, () => pickAction(x.key))))),
      )

      const sep = h('div', { class: 'sep' })
      for (const key of ['length', 'formality']) sep.append(modRow(key))
      sep.append(modRow('format'), modRow('tone'))
      if (s.action === 'translate') {
        const sel = h('select', { class: 'grow', onchange: (e) => (s.target = e.target.value) })
        for (const [v, n] of W.TARGETS) sel.append(h('option', { value: v, text: n, selected: s.target === v }))
        sep.append(h('div', { class: 'row' }, h('span', { class: 'lbl', style: 'padding-top:8px', text: '翻译成' }), sel))
      }
      bd.append(sep)

      const extra = h('input', { type: 'text', class: 'grow', placeholder: '还有特别要求？例如“对方是客户，别太生硬”', value: s.extra || '', maxlength: '200', oninput: (e) => (s.extra = e.target.value) })
      extra.addEventListener('keydown', (e) => {
        // 输入法选字时按回车是“确认拼音”，不能当成运行
        if (e.key === 'Enter' && !e.isComposing && e.keyCode !== 229) run()
        e.stopPropagation()
      })
      const recent = C.pref.extras.length
        ? h('div', { class: 'chips' }, ...C.pref.extras.slice(0, 5).map((t) => chip(t.length > 12 ? t.slice(0, 12) + '…' : t, false, () => ((s.extra = t), (extra.value = t)), 'mut')))
        : null
      const cand = h('select', { title: '一次生成几个候选', onchange: (e) => ((C.cfg.candidates = Number(e.target.value)), saveCfg()) })
      for (const n of [1, 2, 3]) cand.append(h('option', { value: String(n), text: `${n} 个候选`, selected: C.cfg.candidates === n }))
      // Element.append(null) 会把 null 当成文字塞进页面（面板底下那行“null”），所以先滤掉空的
      bd.append(
        ...[
        h(
          'div',
          { class: 'acts', style: 'border-top:1px solid var(--line);padding-top:10px' },
          extra,
          a?.kind === 'grammar' || a?.kind === 'stats' ? null : cand,
          h('button', { text: a?.kind === 'stats' ? '查找数据' : a?.kind === 'grammar' ? '检查' : '生成', onclick: run, disabled: !scopeText().trim() }),
        ),
        recent,
        h('div', { class: 'acts mut' }, h('span', { text: 'Ctrl+Enter 生成 · Esc 关闭' }), h('a', { href: '#', text: '历史记录', onclick: (e) => (e.preventDefault(), showHistory()) })),
        ].filter(Boolean),
      )
      kids.push(bd)
      return kids
    }

    function modRow(key) {
      const m = W.MODS[key]
      const s = C.st.sel
      const cur = s[key]
      const chips = h('div', { class: 'chips' })
      for (const [v, n] of m.items) chips.append(chip(n, cur === v, () => ((s[key] = v), render())))
      if (m.custom) {
        const saved = key === 'format' ? C.cfg.customFormats : C.cfg.customTones
        const field = key === 'format' ? 'customFormat' : 'customTone'
        for (const name of saved) chips.append(chip(name, cur === 'custom' && s[field] === name, () => ((s[key] = 'custom'), (s[field] = name), render())))
        chips.append(
          chip(key === 'format' ? '+ 添加格式' : '+ 添加语气', false, () => {
            const input = h('input', { type: 'text', placeholder: key === 'format' ? '如：小红书风' : '如：像老朋友', maxlength: '40', style: 'height:26px;width:140px' })
            const ok = () => {
              const v = input.value.trim()
              if (!v) return render()
              const list = key === 'format' ? C.cfg.customFormats : C.cfg.customTones
              if (!list.includes(v)) list.push(v)
              s[key] = 'custom'
              s[field] = v
              saveCfg()
              render()
            }
            input.addEventListener('keydown', (e) => {
              e.stopPropagation()
              if (e.isComposing || e.keyCode === 229) return
              if (e.key === 'Enter') ok()
              if (e.key === 'Escape') render()
            })
            const holder = h('span', { class: 'acts' }, input, h('button', { class: 'chip x', text: '添加', onclick: ok }))
            chips.replaceChildren(holder)
            setTimeout(() => input.focus(), 0)
          }, 'dash'),
        )
      }
      return h('div', { class: 'row' }, h('span', { class: 'lbl', text: m.name }), chips)
    }

    // ---- 运行
    async function job(req) {
      let id = 0
      try {
        return await globalThis.ccUI.aiRun(req, { interval: 600, onSubmit: (x) => ((id = x), C.st && (C.st.job = x)), stop: () => !C.st || C.st.job !== id })
      } finally {
        if (C.st && C.st.job === id) C.st.job = 0
      }
    }

    async function run(extraOpts = {}) {
      if (!C.st) return
      const text = scopeText()
      if (!text.trim()) return
      const s = C.st.sel
      Object.assign(C.cfg.last, { action: s.action, length: s.length, formality: s.formality, format: s.format, tone: s.tone, target: s.target, customFormat: s.customFormat || '', customTone: s.customTone || '' })
      saveCfg()
      const ex = (s.extra || '').trim()
      if (ex) {
        C.pref.extras = [ex, ...C.pref.extras.filter((x) => x !== ex)].slice(0, 8)
        savePref()
      }
      const req = W.buildRequest(s, text, { candidates: C.cfg.candidates, route: C.cfg.route, page: `${document.title} ${location.hostname}`, ...extraOpts })
      C.st.view = 'running'
      C.st.reqKind = req.kind
      render()
      try {
        const out = await job(req)
        if (!C.st) return
        if (req.kind === 'grammar') {
          C.st.res = { issues: asArray(out.issues), skipped: new Set() }
          C.st.view = 'issues'
        } else if (req.kind === 'stats') {
          C.st.facts = { list: asArray(out.facts), picked: new Set(asArray(out.facts).map((_, i) => i)), note: out.note || '' }
          C.st.view = 'facts'
        } else {
          const cands = asArray(out.candidates)
          if (!cands.length) throw new Error('模型没有返回内容')
          C.st.res = { src: text, cands, idx: 0, mode: ['translate', 'summarize', 'continue'].includes(s.action) ? 'new' : 'diff', rejected: new Set(), parts: null }
          buildParts()
          C.st.view = 'result'
        }
      } catch (e) {
        if (!C.st) return
        C.st.view = 'options'
        C.st.note = e.message === '已取消' ? '' : e.message
        C.st.noteErr = !!C.st.note
      }
      render()
    }

    function viewRunning() {
      const label = C.st.reqKind === 'stats' ? '正在开放数据库里查找数据…' : C.st.reqKind === 'grammar' ? '正在检查…' : '正在生成…'
      return [
        header('清影写作'),
        h(
          'div',
          { class: 'bd' },
          h('div', { class: 'acts' }, h('span', { class: 'spin' }), h('span', { text: label })),
          h('div', { class: 'acts' }, h('button', { class: 'sec', text: '取消', onclick: () => {
            if (C.st.job) send({ type: 'cc-ai-cancel', id: C.st.job })
            C.st.job = 0
            C.st.view = 'options'
            render()
          } })),
        ),
      ]
    }


    Object.assign(C, { run, viewOptions, viewRunning })
  }
})()
