// 网页翻译 / 视频翻译共用的纯逻辑：语言表、判断“已经是目标语言”、分批、设置整理、免费引擎的请求与解析。
// 不碰网页和浏览器接口，可以单元测试。内容脚本、后台、弹窗里都挂在 globalThis.ccTrans 上。
;(() => {
  if (globalThis.ccTrans) return

  /** code 用 BCP-47 风格；name 给人看；en 写进提示词（大模型最熟悉的写法）；g 是谷歌翻译的代码。 */
  const LANGS = [
    { code: 'zh-CN', name: '简体中文', en: 'Simplified Chinese', g: 'zh-CN' },
    { code: 'zh-TW', name: '繁體中文', en: 'Traditional Chinese', g: 'zh-TW' },
    { code: 'en', name: 'English', en: 'English', g: 'en' },
    { code: 'ja', name: '日本語', en: 'Japanese', g: 'ja' },
    { code: 'ko', name: '한국어', en: 'Korean', g: 'ko' },
    { code: 'fr', name: 'Français', en: 'French', g: 'fr' },
    { code: 'de', name: 'Deutsch', en: 'German', g: 'de' },
    { code: 'es', name: 'Español', en: 'Spanish', g: 'es' },
    { code: 'pt', name: 'Português', en: 'Portuguese', g: 'pt' },
    { code: 'it', name: 'Italiano', en: 'Italian', g: 'it' },
    { code: 'ru', name: 'Русский', en: 'Russian', g: 'ru' },
    { code: 'uk', name: 'Українська', en: 'Ukrainian', g: 'uk' },
    { code: 'pl', name: 'Polski', en: 'Polish', g: 'pl' },
    { code: 'nl', name: 'Nederlands', en: 'Dutch', g: 'nl' },
    { code: 'sv', name: 'Svenska', en: 'Swedish', g: 'sv' },
    { code: 'da', name: 'Dansk', en: 'Danish', g: 'da' },
    { code: 'fi', name: 'Suomi', en: 'Finnish', g: 'fi' },
    { code: 'no', name: 'Norsk', en: 'Norwegian', g: 'no' },
    { code: 'cs', name: 'Čeština', en: 'Czech', g: 'cs' },
    { code: 'hu', name: 'Magyar', en: 'Hungarian', g: 'hu' },
    { code: 'ro', name: 'Română', en: 'Romanian', g: 'ro' },
    { code: 'el', name: 'Ελληνικά', en: 'Greek', g: 'el' },
    { code: 'tr', name: 'Türkçe', en: 'Turkish', g: 'tr' },
    { code: 'ar', name: 'العربية', en: 'Arabic', g: 'ar' },
    { code: 'he', name: 'עברית', en: 'Hebrew', g: 'iw' },
    { code: 'fa', name: 'فارسی', en: 'Persian', g: 'fa' },
    { code: 'hi', name: 'हिन्दी', en: 'Hindi', g: 'hi' },
    { code: 'bn', name: 'বাংলা', en: 'Bengali', g: 'bn' },
    { code: 'ta', name: 'தமிழ்', en: 'Tamil', g: 'ta' },
    { code: 'th', name: 'ไทย', en: 'Thai', g: 'th' },
    { code: 'vi', name: 'Tiếng Việt', en: 'Vietnamese', g: 'vi' },
    { code: 'id', name: 'Bahasa Indonesia', en: 'Indonesian', g: 'id' },
    { code: 'ms', name: 'Bahasa Melayu', en: 'Malay', g: 'ms' },
    { code: 'tl', name: 'Filipino', en: 'Filipino', g: 'tl' },
    { code: 'sw', name: 'Kiswahili', en: 'Swahili', g: 'sw' },
    { code: 'la', name: 'Latina', en: 'Latin', g: 'la' },
  ]

  /** 语言名称的界面语言版本（英文用上面的 en 字段）。 */
  const NAMES = {
    zh: { 'zh-CN': '简体中文', 'zh-TW': '繁体中文', 'en': '英语', 'ja': '日语', 'ko': '韩语', 'fr': '法语', 'de': '德语', 'es': '西班牙语', 'pt': '葡萄牙语', 'it': '意大利语', 'ru': '俄语', 'uk': '乌克兰语', 'pl': '波兰语', 'nl': '荷兰语', 'sv': '瑞典语', 'da': '丹麦语', 'fi': '芬兰语', 'no': '挪威语', 'cs': '捷克语', 'hu': '匈牙利语', 'ro': '罗马尼亚语', 'el': '希腊语', 'tr': '土耳其语', 'ar': '阿拉伯语', 'he': '希伯来语', 'fa': '波斯语', 'hi': '印地语', 'bn': '孟加拉语', 'ta': '泰米尔语', 'th': '泰语', 'vi': '越南语', 'id': '印度尼西亚语', 'ms': '马来语', 'tl': '菲律宾语', 'sw': '斯瓦希里语', 'la': '拉丁语' },
    ja: { 'zh-CN': '中国語（簡体字）', 'zh-TW': '中国語（繁体字）', 'en': '英語', 'ja': '日本語', 'ko': '韓国語', 'fr': 'フランス語', 'de': 'ドイツ語', 'es': 'スペイン語', 'pt': 'ポルトガル語', 'it': 'イタリア語', 'ru': 'ロシア語', 'uk': 'ウクライナ語', 'pl': 'ポーランド語', 'nl': 'オランダ語', 'sv': 'スウェーデン語', 'da': 'デンマーク語', 'fi': 'フィンランド語', 'no': 'ノルウェー語', 'cs': 'チェコ語', 'hu': 'ハンガリー語', 'ro': 'ルーマニア語', 'el': 'ギリシャ語', 'tr': 'トルコ語', 'ar': 'アラビア語', 'he': 'ヘブライ語', 'fa': 'ペルシア語', 'hi': 'ヒンディー語', 'bn': 'ベンガル語', 'ta': 'タミル語', 'th': 'タイ語', 'vi': 'ベトナム語', 'id': 'インドネシア語', 'ms': 'マレー語', 'tl': 'フィリピン語', 'sw': 'スワヒリ語', 'la': 'ラテン語' },
  }
  /** 界面语言：清影主程序的语言（zh / en / ja），认不出来就看浏览器语言。 */
  function uiLang(v) {
    const s = String(v || '').toLowerCase()
    if (s === 'zh' || s === 'en' || s === 'ja') return s
    const n = String((globalThis.navigator && navigator.language) || '').toLowerCase()
    return n.startsWith('zh') ? 'zh' : n.startsWith('ja') ? 'ja' : 'en'
  }
  /** 下拉框里的一项：左边是界面语言写的名字，右边是这种语言自己的写法，如 “日语 · 日本語” / “Japanese · 日本語”；两边一样就只写一个。 */
  function label(code, ui) {
    const l = LANGS.find((x) => x.code === code)
    if (!l) return String(code || '')
    const u = uiLang(ui)
    const left = u === 'en' ? l.en : NAMES[u]?.[l.code] || l.en
    return left === l.name ? left : `${left} · ${l.name}`
  }

  const byCode = (code) => LANGS.find((l) => l.code === code) || null
  /** 写进提示词的语言名：认识的代码 / 名称换成英文名，其他（用户自己填的）原样用。 */
  function promptName(v) {
    const s = String(v || '').trim()
    const hit = LANGS.find((l) => l.code === s || l.name === s || l.en.toLowerCase() === s.toLowerCase())
    return hit ? hit.en : s
  }
  const nameOf = (code) => byCode(code)?.name || String(code || '')

  // ---------- 文字属于哪种文字系统 ----------

  const RANGES = [
    ['kana', 0x3040, 0x30ff],
    ['hangul', 0xac00, 0xd7af],
    ['hangul', 0x1100, 0x11ff],
    ['han', 0x4e00, 0x9fff],
    ['han', 0x3400, 0x4dbf],
    ['cyrillic', 0x0400, 0x04ff],
    ['arabic', 0x0600, 0x06ff],
    ['arabic', 0x0750, 0x077f],
    ['thai', 0x0e00, 0x0e7f],
    ['devanagari', 0x0900, 0x097f],
    ['bengali', 0x0980, 0x09ff],
    ['tamil', 0x0b80, 0x0bff],
    ['hebrew', 0x0590, 0x05ff],
    ['greek', 0x0370, 0x03ff],
    ['latin', 0x0041, 0x005a],
    ['latin', 0x0061, 0x007a],
    ['latin', 0x00c0, 0x024f],
    ['latin', 0x1e00, 0x1eff],
  ]

  /** 各文字系统的字数。 */
  function scriptCounts(text) {
    const out = {}
    for (const ch of String(text || '')) {
      const c = ch.codePointAt(0)
      for (const [name, a, b] of RANGES) {
        if (c >= a && c <= b) {
          out[name] = (out[name] || 0) + 1
          break
        }
      }
    }
    return out
  }

  const EN_WORDS = /\b(the|and|of|to|in|is|that|for|with|on|are|this|it|as|was|be|by|you|not)\b/gi

  /** 这段文字是不是已经是目标语言（只在能确定时返回 true：文字系统特别的语言，以及英文）。 */
  function inTarget(text, code) {
    const c = scriptCounts(text)
    const total = Object.values(c).reduce((a, b) => a + b, 0)
    if (!total) return false
    const share = (...names) => names.reduce((a, n) => a + (c[n] || 0), 0) / total
    switch (code) {
      case 'zh-CN':
      case 'zh-TW':
        return share('han') > 0.6 && !(c.kana > 0)
      case 'ja':
        return c.kana > 0 && share('han', 'kana') > 0.6
      case 'ko':
        return share('hangul') > 0.6
      case 'ru':
      case 'uk':
        return share('cyrillic') > 0.7
      case 'ar':
      case 'fa':
        return share('arabic') > 0.7
      case 'th':
        return share('thai') > 0.7
      case 'hi':
        return share('devanagari') > 0.7
      case 'bn':
        return share('bengali') > 0.7
      case 'ta':
        return share('tamil') > 0.7
      case 'he':
        return share('hebrew') > 0.7
      case 'el':
        return share('greek') > 0.7
      case 'en': {
        if (share('latin') < 0.8) return false
        const words = (String(text).match(EN_WORDS) || []).length
        return words >= 2 || (words >= 1 && total < 40)
      }
      default:
        return false
    }
  }

  const URLISH = /^(https?:\/\/|www\.)\S+$/i
  const EMAILISH = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  const TEXTLESS = /^[\d\s.,:;!?()[\]{}<>/\\|+\-*=_~^%$#@&'"`·•—–…，。！？、；：（）【】《》“”‘’¥€£©®™°]*$/

  /** 这段文字值不值得翻译：太短、纯数字符号、网址邮箱、已经是目标语言的都不翻。 */
  function worthTranslating(text, targetCode) {
    const t = String(text || '').replace(/\s+/g, ' ').trim()
    if (t.length < 2 || t.length > 6000) return false
    if (TEXTLESS.test(t) || URLISH.test(t) || EMAILISH.test(t)) return false
    if (inTarget(t, targetCode)) return false
    return true
  }

  /** 整个页面抽样出来的文字大部分已经是目标语言吗（自动翻译时用来跳过本来就是目标语言的页面）。 */
  function pageInTarget(sample, code) {
    return inTarget(String(sample || '').slice(0, 1500), code)
  }

  // ---------- 分批 ----------

  /** 把 [{ id, text }] 按字数和条数切成几批。单条超长就独占一批。 */
  function batchPlan(items, { maxChars = 2400, maxItems = 16 } = {}) {
    const out = []
    let cur = []
    let size = 0
    for (const it of items) {
      const n = String(it.text).length
      if (cur.length && (size + n > maxChars || cur.length >= maxItems)) {
        out.push(cur)
        cur = []
        size = 0
      }
      cur.push(it)
      size += n
    }
    if (cur.length) out.push(cur)
    return out
  }

  // ---------- 设置 ----------

  const MODES = ['bilingual', 'replace']
  const STYLES = ['card', 'dim', 'underline', 'highlight', 'plain']
  const ENGINES = ['clearclip', 'google']
  const HOVER_KEYS = ['off', 'Control', 'Alt', 'Shift']

  const DEFAULT_CFG = {
    target: 'zh-CN', // 网页翻译的目标语言
    mode: 'bilingual', // bilingual 双语对照 / replace 只显示译文
    style: 'card', // 译文样式（card 淡蓝底色 + 左侧竖线）
    engine: 'clearclip', // clearclip 用清影里设置的大模型 / google 免费的谷歌翻译（页面文字会发给谷歌）
    hover: 'Control', // 单击一下这个键，翻译鼠标指着的段落；off 关闭
    select: true, // 选中文字后出现“译”按钮
    input: true, // 输入框里连按三次空格，翻译成下面的语言
    inputLang: 'en',
    auto: [], // 这些网站打开就自动翻译
    never: [], // 这些网站永远不翻译
    sites: {}, // 各网站单独的设置：{ 域名: { target?, mode?, style? } }，覆盖上面的全局值
    lazy: true, // 只翻译看得见（和快要看见）的段落
    skipTarget: true, // 本来就是目标语言的段落 / 页面不翻译
    domain: true, // 翻译前先判断网页领域，按领域选词（只对“清影（大模型）”有效）
  }

  const hostList = (v) => [...new Set((Array.isArray(v) ? v : String(v || '').split(/[\s,;]+/)).map((x) => String(x || '').trim().toLowerCase().replace(/^\*?\.?/, '')).filter(Boolean))]

  /** 各网站单独设置：只留认得出的字段；域名统一小写、去掉 `*.`。 */
  function normalizeSites(raw) {
    const out = {}
    if (!raw || typeof raw !== 'object') return out
    for (const [k, v] of Object.entries(raw)) {
      const host = hostList([k])[0]
      if (!host || !v || typeof v !== 'object') continue
      const e = {}
      if (typeof v.target === 'string' && v.target.trim()) e.target = v.target.trim()
      if (MODES.includes(v.mode)) e.mode = v.mode
      if (STYLES.includes(v.style)) e.style = v.style
      if (Object.keys(e).length) out[host] = e
    }
    return out
  }

  /** 文本框 ↔ 对象。每行：`域名 语言代码 [bilingual|replace]`，如 `github.com ja replace`；语言写 `-` 表示沿用全局。 */
  function parseSites(text) {
    const o = {}
    for (const line of String(text || '').split(/\n/)) {
      const [host, lang, mode] = line.trim().split(/\s+/)
      if (!host) continue
      const e = {}
      if (lang && lang !== '-') e.target = lang
      if (MODES.includes(mode)) e.mode = mode
      o[host] = e
    }
    return normalizeSites(o)
  }
  const formatSites = (sites) =>
    Object.entries(sites || {})
      .map(([h, e]) => [h, e.target || '-', e.mode].filter(Boolean).join(' '))
      .join('\n')

  /** 套用当前网站的单独设置（最长匹配的域名赢）；没有就原样返回。 */
  function effectiveCfg(cfg, host) {
    const h = String(host || '').toLowerCase()
    const key = Object.keys(cfg.sites || {})
      .filter((s) => h === s || h.endsWith('.' + s))
      .sort((a, b) => b.length - a.length)[0]
    return key ? { ...cfg, ...cfg.sites[key] } : cfg
  }

  function normalizeCfg(raw) {
    const r = raw && typeof raw === 'object' ? raw : {}
    const lang = (v, d) => (typeof v === 'string' && v.trim() ? v.trim() : d)
    return {
      target: lang(r.target, DEFAULT_CFG.target),
      mode: MODES.includes(r.mode) ? r.mode : DEFAULT_CFG.mode,
      style: STYLES.includes(r.style) ? r.style : DEFAULT_CFG.style,
      engine: ENGINES.includes(r.engine) ? r.engine : DEFAULT_CFG.engine,
      hover: HOVER_KEYS.includes(r.hover) ? r.hover : DEFAULT_CFG.hover,
      select: r.select !== false,
      input: r.input !== false,
      inputLang: lang(r.inputLang, DEFAULT_CFG.inputLang),
      auto: hostList(r.auto),
      never: hostList(r.never),
      sites: normalizeSites(r.sites),
      lazy: r.lazy !== false,
      skipTarget: r.skipTarget !== false,
      domain: r.domain !== false,
    }
  }

  const listed = (list, host) => {
    const h = String(host || '').toLowerCase()
    return (list || []).some((s) => h === s || h.endsWith('.' + s))
  }

  /** 打开这个网站要不要自动翻译：在 auto 名单里、不在 never 名单里。 */
  const shouldAuto = (cfg, host) => listed(cfg.auto, host) && !listed(cfg.never, host)

  /** 这些网站要常驻脚本才能自动翻译：返回要匹配的网址模式（没有名单就是空）。 */
  function autoMatches(cfg) {
    const out = []
    for (const h of cfg.auto) {
      if (!/^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$/.test(h)) continue
      out.push(`*://${h}/*`, `*://*.${h}/*`)
    }
    return out
  }

  // ---------- 免费的谷歌翻译（gtx）----------

  const googleUrl = (text, targetCode) => {
    const t = byCode(targetCode)?.g || targetCode || 'zh-CN'
    return `https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=${encodeURIComponent(t)}&dt=t&q=${encodeURIComponent(text)}`
  }

  /** 解析 gtx 的返回：[[["译文","原文",...],...],...]。读不出来返回 null。 */
  function parseGoogle(json) {
    const segs = Array.isArray(json) && Array.isArray(json[0]) ? json[0] : null
    if (!segs) return null
    const out = segs.map((s) => (Array.isArray(s) && typeof s[0] === 'string' ? s[0] : '')).join('')
    return out || null
  }

  // ---------- 缓存 ----------

  const cacheKey = (engine, target, text) => `${engine}|${target}|${String(text).replace(/\s+/g, ' ').trim()}`

  /** 缓存对象 { key: [译文, 时间] } 超过条数 / 字数时丢掉最旧的。 */
  function pruneCache(map, { maxEntries = 3000, maxChars = 1_500_000 } = {}) {
    const entries = Object.entries(map)
    let chars = entries.reduce((a, [k, v]) => a + k.length + String(v[0]).length, 0)
    if (entries.length <= maxEntries && chars <= maxChars) return map
    entries.sort((a, b) => b[1][1] - a[1][1])
    const out = {}
    chars = 0
    let n = 0
    for (const [k, v] of entries) {
      chars += k.length + String(v[0]).length
      if (n >= maxEntries || chars > maxChars) break
      out[k] = v
      n++
    }
    return out
  }

  globalThis.ccTrans = {
    LANGS, NAMES, uiLang, label, byCode, promptName, nameOf, scriptCounts, inTarget, worthTranslating, pageInTarget, batchPlan,
    DEFAULT_CFG, MODES, STYLES, ENGINES, HOVER_KEYS, normalizeCfg, normalizeSites, parseSites, formatSites, effectiveCfg, hostList, shouldAuto, autoMatches, listed,
    googleUrl, parseGoogle, cacheKey, pruneCache,
  }
})()
