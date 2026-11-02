// 写作浮窗的纯逻辑：动作和修饰选项、设置整理、输入框是否该显示浮标、文字差异（逐处采用 / 放弃）。
// 不碰网页和浏览器接口，可以单元测试。内容脚本里挂在 globalThis.ccWrite。
;(() => {
  if (globalThis.ccWrite) return

  /** 动作：key 要和应用端 writing.rs 的 ACTIONS 一致。kind 决定走哪条接口。 */
  const BASIC = [
    { key: 'rewrite', name: '改写它' },
    { key: 'grammar', name: '语法检查', kind: 'grammar' },
    { key: 'translate', name: '翻译' },
    { key: 'summarize', name: '总结' },
    { key: 'optimize', name: '优化它' },
    { key: 'continue', name: '继续写作', insert: true },
    { key: 'shorten', name: '缩短它' },
  ]
  const STYLES = [
    { key: 'stats', name: '加入统计数据', kind: 'stats' },
    { key: 'detail', name: '添加细节' },
    { key: 'humor', name: '增加幽默' },
    { key: 'polite', name: '使其礼貌' },
    { key: 'concise', name: '使其简洁' },
    { key: 'plain', name: '去除专业术语' },
    { key: 'more_info', name: '更多信息' },
    { key: 'engaging', name: '更引人入胜' },
    { key: 'more_detail', name: '更有细节' },
    { key: 'persuasive', name: '更有说服力' },
    { key: 'apologetic', name: '更有歉意' },
    { key: 'sincere', name: '使其真诚' },
  ]
  const MODS = {
    length: { name: '长度', items: [['auto', '自动'], ['short', '短'], ['medium', '中等'], ['long', '长']] },
    formality: { name: '正式程度', items: [['auto', '自动'], ['casual', '随意'], ['neutral', '中立'], ['formal', '正式']] },
    format: {
      name: '格式',
      custom: true,
      items: [['auto', '自动'], ['email', '电子邮件'], ['message', '消息'], ['comment', '评论'], ['paragraph', '段落'], ['article', '文章'], ['blog', '博客文章'], ['idea', '想法'], ['outline', '大纲'], ['twitter', '推特'], ['reddit', 'Reddit'], ['facebook', '脸书'], ['linkedin', '领英']],
    },
    tone: {
      name: '语气',
      custom: true,
      items: [['auto', '自动'], ['warm', '热情的'], ['humorous', '幽默的'], ['caring', '关心的'], ['humble', '谦逊的'], ['optimistic', '乐观的'], ['empathetic', '有同理心的'], ['frank', '坦率的'], ['sincere', '真诚的']],
    },
  }
  const TARGETS = [['', '中英互译'], ['简体中文', '简体中文'], ['English', 'English'], ['日本語', '日本語'], ['한국어', '한국어'], ['Français', 'Français'], ['Deutsch', 'Deutsch'], ['Español', 'Español'], ['Русский', 'Русский']]

  const allActions = () => [...BASIC, ...STYLES]
  const actionOf = (key) => allActions().find((a) => a.key === key) || null

  const DEFAULT_CFG = {
    trigger: 'badge', // badge = 小浮标；hotkey = 只用快捷键
    route: 'local_first', // local_first / local_only / cloud_only
    candidates: 1,
    showEmpty: true,
    runOnClick: false, // 选了动作就立刻生成
    offHosts: [],
    minWidth: 160,
    last: { action: 'rewrite', length: 'auto', formality: 'auto', format: 'auto', tone: 'auto', target: '' },
    customFormats: [],
    customTones: [],
  }

  const pick = (v, list, d) => (list.includes(v) ? v : d)

  function normalizeCfg(raw) {
    const r = raw && typeof raw === 'object' ? raw : {}
    const l = r.last && typeof r.last === 'object' ? r.last : {}
    const keys = (m) => m.items.map((i) => i[0])
    const strs = (v) => [...new Set((Array.isArray(v) ? v : []).map((x) => String(x || '').trim().slice(0, 40)).filter(Boolean))].slice(0, 20)
    const hosts = [...new Set((Array.isArray(r.offHosts) ? r.offHosts : []).map((x) => String(x || '').trim().toLowerCase()).filter(Boolean))]
    const fmtOk = (v) => (v === 'custom' ? 'custom' : pick(v, keys(MODS.format), 'auto'))
    const toneOk = (v) => (v === 'custom' ? 'custom' : pick(v, keys(MODS.tone), 'auto'))
    return {
      trigger: pick(r.trigger, ['badge', 'hotkey'], DEFAULT_CFG.trigger),
      route: pick(r.route, ['local_first', 'local_only', 'cloud_only'], DEFAULT_CFG.route),
      candidates: Math.max(1, Math.min(3, Math.round(Number(r.candidates)) || 1)),
      showEmpty: r.showEmpty !== false,
      runOnClick: r.runOnClick === true,
      offHosts: hosts,
      minWidth: Math.max(60, Math.min(600, Math.round(Number(r.minWidth)) || DEFAULT_CFG.minWidth)),
      last: {
        action: allActions().some((a) => a.key === l.action) ? l.action : 'rewrite',
        length: pick(l.length, keys(MODS.length), 'auto'),
        formality: pick(l.formality, keys(MODS.formality), 'auto'),
        format: fmtOk(l.format),
        tone: toneOk(l.tone),
        target: TARGETS.some((t) => t[0] === l.target) ? l.target : '',
        customFormat: typeof l.customFormat === 'string' ? l.customFormat.slice(0, 40) : '',
        customTone: typeof l.customTone === 'string' ? l.customTone.slice(0, 40) : '',
      },
      customFormats: strs(r.customFormats),
      customTones: strs(r.customTones),
    }
  }

  /** 把面板上的选择变成发给应用端的请求。 */
  function buildRequest(sel, text, opts = {}) {
    const a = actionOf(sel.action)
    const kind = a?.kind || 'write'
    const write = {
      action: sel.action,
      length: sel.length || 'auto',
      formality: sel.formality || 'auto',
      format: sel.format || 'auto',
      custom_format: sel.format === 'custom' ? sel.customFormat || '' : '',
      tone: sel.tone || 'auto',
      custom_tone: sel.tone === 'custom' ? sel.customTone || '' : '',
      extra: (sel.extra || '').slice(0, 200),
      target: sel.action === 'translate' ? sel.target || '' : '',
      candidates: Math.max(1, Math.min(3, opts.candidates || 1)),
      route: opts.route || 'local_first',
      page: (opts.page || '').slice(0, 200),
    }
    if (kind === 'grammar') return { kind: 'grammar', text, write }
    // 加入统计数据分两步：先检索（kind=stats），用户勾选后带着数据再写（kind=write）
    if (sel.action === 'stats') {
      if (!opts.facts || !opts.facts.length) return { kind: 'stats', text, write }
      write.facts = opts.facts
    }
    return { kind: 'write', text, write }
  }

  // ---------- 输入框 ----------

  const SKIP_TYPES = ['password', 'hidden', 'number', 'tel', 'date', 'datetime-local', 'month', 'time', 'week', 'color', 'range', 'file', 'checkbox', 'radio', 'submit', 'button', 'image', 'reset']
  const SENSITIVE_AC = /(^|\s)(cc-|one-time-code|current-password|new-password|username)/i
  const SENSITIVE_NAME = /pass(word|wd)?|pwd|card|cvv|cvc|otp|captcha|verif|iban|ssn|pin\b|密码|口令|验证码|银行卡|卡号|身份证|支付/i

  /** 描述一个输入框的信息，判断要不要显示浮标。info 是从元素里读出来的普通对象，所以能单元测试。 */
  function fieldVerdict(info, cfg = DEFAULT_CFG) {
    const tag = String(info.tag || '').toLowerCase()
    const type = String(info.type || 'text').toLowerCase()
    if (info.disabled || info.readOnly) return { ok: false, why: 'readonly' }
    if (tag === 'input' && (SKIP_TYPES.includes(type) || !['text', 'search', 'email', 'url', ''].includes(type))) return { ok: false, why: 'type' }
    if (tag !== 'input' && tag !== 'textarea' && !info.editable) return { ok: false, why: 'not-editable' }
    if (SENSITIVE_AC.test(info.autocomplete || '')) return { ok: false, why: 'sensitive' }
    if (SENSITIVE_NAME.test(`${info.name || ''} ${info.id || ''} ${info.placeholder || ''} ${info.ariaLabel || ''}`)) return { ok: false, why: 'sensitive' }
    if (info.width < (cfg.minWidth || 160) || info.height < 24) return { ok: false, why: 'small' }
    if (tag === 'input' && info.width < 220) return { ok: false, why: 'small' }
    if (info.hidden) return { ok: false, why: 'hidden' }
    return { ok: true, why: '' }
  }

  /** 网站是否被用户关掉。 */
  function hostOffNow(cfg, host) {
    const h = String(host || '').toLowerCase()
    return (cfg.offHosts || []).some((s) => h === s || h.endsWith('.' + s))
  }

  function countText(s) {
    const t = String(s || '')
    const cjk = (t.match(/[㐀-鿿豈-﫿]/g) || []).length
    const words = (t.replace(/[㐀-鿿豈-﫿]/g, ' ').match(/[A-Za-z0-9À-ɏ]+(?:['’-][A-Za-z0-9]+)*/g) || []).length
    return cjk + words
  }

  // ---------- 文字差异 ----------

  /** 切成最小单位：每个汉字、每个英文单词（含数字）、每个空白串、每个标点各一个。 */
  function tokenize(s) {
    return String(s || '').match(/[㐀-鿿豈-﫿]|[A-Za-z0-9À-ɏ]+(?:['’][A-Za-z]+)?|\s+|[^\s]/gu) || []
  }

  /** LCS 差异：返回 [{t:'eq'|'del'|'add', s}]，相邻同类合并。太长时退回“公共前后缀 + 中间整段替换”。 */
  function diffOps(a, b) {
    const A = tokenize(a)
    const B = tokenize(b)
    let pre = 0
    while (pre < A.length && pre < B.length && A[pre] === B[pre]) pre++
    let suf = 0
    while (suf < A.length - pre && suf < B.length - pre && A[A.length - 1 - suf] === B[B.length - 1 - suf]) suf++
    const am = A.slice(pre, A.length - suf)
    const bm = B.slice(pre, B.length - suf)
    const ops = []
    const push = (t, s) => {
      if (!s) return
      const last = ops[ops.length - 1]
      if (last && last.t === t) last.s += s
      else ops.push({ t, s })
    }
    push('eq', A.slice(0, pre).join(''))
    if (am.length * bm.length > 4_000_000) {
      push('del', am.join(''))
      push('add', bm.join(''))
    } else {
      const n = am.length
      const m = bm.length
      const w = m + 1
      const dp = new Uint16Array((n + 1) * w)
      for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
          dp[i * w + j] = am[i] === bm[j] ? dp[(i + 1) * w + j + 1] + 1 : Math.max(dp[(i + 1) * w + j], dp[i * w + j + 1])
        }
      }
      let i = 0
      let j = 0
      while (i < n && j < m) {
        if (am[i] === bm[j]) {
          push('eq', am[i])
          i++
          j++
        } else if (dp[(i + 1) * w + j] >= dp[i * w + j + 1]) push('del', am[i++])
        else push('add', bm[j++])
      }
      while (i < n) push('del', am[i++])
      while (j < m) push('add', bm[j++])
    }
    push('eq', A.slice(A.length - suf).join(''))
    return ops
  }

  /** 把差异整理成“段”：相等的文字，和连续的改动（一处改动 = 删掉的 + 新增的）。给每处改动编号。 */
  function hunks(ops) {
    const out = []
    let id = 0
    for (let k = 0; k < ops.length; k++) {
      const o = ops[k]
      if (o.t === 'eq') {
        out.push({ eq: o.s })
        continue
      }
      const h = { id: id++, del: '', add: '' }
      while (k < ops.length && ops[k].t !== 'eq') {
        if (ops[k].t === 'del') h.del += ops[k].s
        else h.add += ops[k].s
        k++
      }
      k--
      out.push(h)
    }
    return out
  }

  /** 按用户的取舍合成最终文字：rejected 里的改动保留原文，其余采用新文。 */
  function compose(parts, rejected = new Set()) {
    return parts.map((p) => (p.eq != null ? p.eq : rejected.has(p.id) ? p.del : p.add)).join('')
  }

  /** 语法建议里被采用的合并回原文（位置是字符位置，从后往前换）。 */
  function applyIssues(text, issues, skipped = new Set()) {
    const chars = Array.from(String(text || ''))
    const list = issues.map((it, i) => ({ it, i })).filter((x) => !skipped.has(x.i)).sort((a, b) => b.it.start - a.it.start)
    for (const { it } of list) {
      if (it.end <= chars.length && it.start <= it.end) chars.splice(it.start, it.end - it.start, ...Array.from(it.to))
    }
    return chars.join('')
  }

  /** 历史记录：只留最近 50 条，同一原文 + 结果不重复。 */
  function pushHistory(list, rec) {
    const base = (Array.isArray(list) ? list : []).filter((x) => !(x.src === rec.src && x.out === rec.out))
    return [rec, ...base].slice(0, 50)
  }

  globalThis.ccWrite = { BASIC, STYLES, MODS, TARGETS, DEFAULT_CFG, actionOf, normalizeCfg, buildRequest, fieldVerdict, hostOffNow, countText, tokenize, diffOps, hunks, compose, applyIssues, pushHistory }
})()
