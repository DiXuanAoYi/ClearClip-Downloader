// 后台消息的入口检查：内容脚本和扩展页面通过 runtime.sendMessage 发来的请求，先过这里再交给处理函数。
// 做三件事：
// 1) 发送者必须是本扩展自己（sender.id === runtime.id）；
// 2) 需要标签页的消息（needTab）必须来自有标签页的发送者（内容脚本），扩展页面不行；
// 3) 请求字段按白名单整理：只留声明过的字段、类型要对、长度有上限，超出的整条拒绝，并返回统一的错误。
// 本文件不依赖浏览器接口，可以单元测试。

const ccGuard = (() => {
  // ---- 长度上限（字符数）。有后端对应的以后端为准，其余是“正常使用远远碰不到、滥用才会碰到”的数。
  const LIMITS = {
    // 一次 AI 请求的总字数：与清影 vsum.rs 的 MAX_CHARS 一致（字幕行 + 正文 + 评论，每条另加 8）
    AI_MAX_CHARS: 600_000,
    // 自定义提示词：清影 vsum.rs 的 MAX_SYSTEM
    AI_SYSTEM: 20_000,
    // 图片 data URL：清影 imgtrans.rs 的 MAX_IMAGE
    AI_IMAGE: 1_900_000,
    // 网址：浏览器地址栏实际能放的上限量级
    URL: 8192,
    // 标题 / 名字 / 语言代码之类的短文本
    TITLE: 2000,
    SHORT: 200,
    // 选中文字 / 翻译窗文字：处理函数本来就只取前 5000 字，这里超出的直接截掉（clip）
    SELECTION: 5000,
    // 摘录 / 笔记正文：设置里“不限制”是默认，但导出整份文稿也不会超过这个数
    NOTE: 2_000_000,
    // 一批网页翻译：处理函数最多取 60 条，这里放宽到 200 条再拒绝；单条 20000 字足够一个段落
    TR_ITEMS: 200,
    TR_TEXT: 20_000,
    // 批量发送 / 起名的条数：sendBatch 最多处理 200 条，起名最多取 60 条（多出来的由处理函数截掉）
    BATCH_ITEMS: 500,
    // 视频缓存同步的负载（摘要会话 + 双语字幕）：与 vsumSessions 的 2.5MB 上限同量级
    VCACHE_JSON: 8_000_000,
    // 其他自由格式的小对象（写作选项、生词、页面信息）序列化后的上限
    OBJ_JSON: 200_000,
  }

  // ---- 规则构造。soft：类型不对时丢掉这个字段而不是整条拒绝（用于页面里抓来的、格式不一定整齐的数据）
  const S = (max, o = {}) => ({ t: 's', max, ...o })
  const N = (o = {}) => ({ t: 'n', ...o })
  const B = (o = {}) => ({ t: 'b', ...o })
  const SN = (max, o = {}) => ({ t: 'sn', max, ...o }) // 字符串或数字（任务编号、引擎序号）
  const A = (of, max, o = {}) => ({ t: 'a', of, max, ...o })
  const O = (fields, o = {}) => ({ t: 'o', fields, ...o })
  const J = (maxJson, o = {}) => ({ t: 'j', maxJson, ...o }) // 自由格式对象，只限制大小
  const req = (r) => ({ ...r, required: true })

  const ITEM = O({ kind: S(32), url: S(LIMITS.URL), name: S(LIMITS.TITLE), ext: S(16), mime: S(100) }, { soft: true })
  const RECT = O({ x: N(), y: N(), w: N(), h: N() })

  // AI 请求：字段与清影的 vsum::Req 对应（terms / guide 由桌面端自己填，扩展不传）
  const AI_REQ = O(
    {
      kind: req(S(32)),
      title: S(LIMITS.TITLE),
      lang: S(LIMITS.SHORT),
      style: S(64),
      format: S(64),
      lines: A(O({ t: N(), text: S(LIMITS.AI_MAX_CHARS) }), 200_000),
      text: S(LIMITS.AI_MAX_CHARS),
      question: S(20_000),
      history: A(O({ role: S(32), text: S(50_000) }), 200),
      url: S(LIMITS.URL),
      context: S(100_000),
      trans: S(20_000),
      track: S(LIMITS.SHORT),
      system: S(LIMITS.AI_SYSTEM),
      write: J(LIMITS.OBJ_JSON),
      comments: A(O({ author: S(LIMITS.SHORT, { soft: true }), text: S(20_000, { soft: true }), likes: N({ soft: true }), t: N({ soft: true }), replies: N({ soft: true }) }), 5000),
      image: S(LIMITS.AI_IMAGE),
      ocr: S(32),
    },
    { total: aiChars },
  )

  /** 与清影 vsum.rs 的估算一致：每行字幕 / 每条评论字数 + 8，再加正文。 */
  function aiChars(r) {
    const len = (s) => String(s ?? '').length
    return (r.lines || []).reduce((a, l) => a + len(l.text) + 8, 0) + len(r.text) + (r.comments || []).reduce((a, c) => a + len(c.text) + 8, 0)
  }

  // ---- 每种消息的规则。needTab：只接受内容脚本（有标签页的发送者）。fields：允许的字段（其余丢掉）。
  // 新增消息类型必须在这里登记（单元测试会核对 HANDLERS 与这张表一一对应）。
  const SPEC = {
    'cc-sniffed': { needTab: true },
    'cc-stub-load': { needTab: true },
    'cc-send-page': { needTab: true },
    'cc-send-media': { needTab: true, fields: { url: req(S(LIMITS.URL)) } },
    'cc-send-batch': { needTab: true, fields: { items: req(A(ITEM, LIMITS.BATCH_ITEMS)) } },
    'cc-run': { fields: { tabId: req(N()), action: req(S(32)), mode: S(32) } },
    'cc-send-items': { fields: { items: req(A(ITEM, LIMITS.BATCH_ITEMS)), page: S(LIMITS.URL), title: S(LIMITS.TITLE) } },
    'cc-open-audio': {},
    'cc-engines': { needTab: true },
    'cc-save': { needTab: true, fields: { url: req(S(LIMITS.URL)), referrer: S(LIMITS.URL) } },
    'cc-probe-media': { fields: { url: req(S(LIMITS.URL)), referrer: S(LIMITS.URL) } },
    'cc-search': { fields: { url: req(S(LIMITS.URL)), engine: SN(16) } },
    'cc-page-save': { fields: { action: req(S(32)), tabId: req(N()) } },
    'cc-shot': { needTab: true, fields: { rect: req(RECT), vw: N() } },
    'cc-area': { needTab: true, fields: { rect: req(RECT), vw: N() } },
    'cc-unlock-now': { fields: { tabId: req(N()) } },
    'cc-ai-status': { needTab: true },
    'cc-fetch-sub': { fields: { url: req(S(2048)) } },
    'cc-app-lang': {},
    'cc-open-highlights': {},
    'cc-videocache': { fields: { op: req(S(16)), url: S(LIMITS.URL), payload: J(LIMITS.VCACHE_JSON) } },
    'cc-vocab': {
      fields: {
        op: req(S(16)),
        q: S(LIMITS.SHORT),
        state: S(32),
        word: { t: 'sj', max: LIMITS.SHORT, maxJson: LIMITS.OBJ_JSON }, // 查词的单词（字符串）或加入生词本的对象
        id: N(),
        words: A(S(100), 400),
        minRank: N(),
        format: S(16),
        data: S(4_000_000),
      },
    },
    'cc-ai-name': {
      fields: {
        items: A(O({ kind: S(32), alt: S(1000), cap: S(1000), text: S(1000), orig: S(500), near: S(1000), url: S(LIMITS.URL) }, { soft: true }), LIMITS.BATCH_ITEMS),
        ocr: B(),
        page: S(LIMITS.URL),
        title: S(LIMITS.TITLE),
        excerpt: S(20_000),
        lang: S(LIMITS.SHORT),
      },
    },
    'cc-ai-run': { needTab: true, fields: { req: req(AI_REQ) } },
    'cc-ai-job': { needTab: true, fields: { id: req(SN(100)) } },
    'cc-ai-cancel': { needTab: true, fields: { id: req(SN(100)) } },
    'cc-tr': {
      needTab: true,
      fields: {
        texts: req(A(S(LIMITS.TR_TEXT), LIMITS.TR_ITEMS)),
        engine: S(32),
        target: S(32),
        page: O({ host: S(100), title: S(LIMITS.TITLE), sample: S(LIMITS.TITLE * 5) }, { soft: true }),
      },
    },
    'cc-trbox': { fields: { text: S(LIMITS.SELECTION, { clip: true }), target: S(32), engines: A(S(32), 16) } },
    'cc-wt-sel': { fields: { text: S(LIMITS.SELECTION, { clip: true }), rect: O({ left: N(), right: N(), top: N(), bottom: N() }, { soft: true }) } },
    'cc-open-transbox': { fields: { text: S(LIMITS.SELECTION, { clip: true }) } },
    'cc-sublayer-open': {},
    'cc-vsum-open': {},
    'cc-tr-clear': {},
    'cc-note': { needTab: true, fields: { text: S(LIMITS.NOTE) } },
    'cc-capture': { needTab: true },
  }

  class Bad extends Error {}
  const bad = (path, why) => new Bad(`${path} ${why}`)

  /** 按规则整理一个值。返回整理后的值（undefined = 没有）；不合规抛 Bad。 */
  function clean(rule, v, path) {
    if (v === undefined || v === null) {
      if (rule.required) throw bad(path, '缺少')
      return undefined
    }
    const fail = (why) => {
      if (rule.soft) return undefined
      throw bad(path, why)
    }
    switch (rule.t) {
      case 's': {
        if (typeof v !== 'string') return fail('必须是文字')
        if (v.length > rule.max) {
          if (rule.clip) return v.slice(0, rule.max)
          return fail(`太长（上限 ${rule.max} 字）`)
        }
        return v
      }
      case 'n':
        return typeof v === 'number' && Number.isFinite(v) ? v : fail('必须是数字')
      case 'b':
        return typeof v === 'boolean' ? v : fail('必须是真假值')
      case 'sn':
        if (typeof v === 'number' && Number.isFinite(v)) return v
        if (typeof v === 'string' && v.length <= rule.max) return v
        return fail('必须是文字或数字，且不能太长')
      case 'a': {
        if (!Array.isArray(v)) return fail('必须是列表')
        if (v.length > rule.max) return fail(`太多（上限 ${rule.max} 条）`)
        const out = []
        for (let i = 0; i < v.length; i++) {
          // 列表里的元素不能悄悄丢掉：处理函数按下标对应结果（起名、翻译），丢一个就错位
          const x = clean({ ...rule.of, required: false }, v[i], `${path}[${i}]`)
          if (x === undefined) throw bad(`${path}[${i}]`, '格式不对')
          out.push(x)
        }
        return out
      }
      case 'o': {
        if (typeof v !== 'object' || Array.isArray(v)) return fail('必须是对象')
        const out = {}
        for (const [k, r] of Object.entries(rule.fields)) {
          const x = clean(rule.soft ? { ...r, soft: true } : r, v[k], `${path}.${k}`)
          if (x !== undefined) out[k] = x
        }
        if (rule.total && rule.total(out) > LIMITS.AI_MAX_CHARS) return fail(`内容太多（总共上限 ${LIMITS.AI_MAX_CHARS} 字）`)
        return out
      }
      case 'j': {
        if (typeof v !== 'object' || Array.isArray(v)) return fail('必须是对象')
        return jsonWithin(v, rule.maxJson) ? v : fail('太大')
      }
      case 'sj': {
        if (typeof v === 'string') return v.length <= rule.max ? v : fail('太长')
        if (typeof v === 'object' && !Array.isArray(v)) return jsonWithin(v, rule.maxJson) ? v : fail('太大')
        return fail('必须是文字或对象')
      }
      default:
        throw new Error(`未知规则 ${rule.t}`)
    }
  }

  function jsonWithin(v, max) {
    try {
      return JSON.stringify(v).length <= max
    } catch {
      return false
    }
  }

  /**
   * 检查并整理一条消息。selfId：本扩展的 id（runtime.id）。
   * 返回 { ok: true, msg } —— msg 只含声明过的字段；或 { ok: false, code, error } —— 统一的错误：
   *   forbidden（发送者不是本扩展）/ no_tab（需要标签页）/ bad_request（字段缺失、类型不对、超长）。
   * 没有登记的消息类型返回 { ok: false, code: 'unknown' }（调用方应当直接忽略，让别的监听器处理）。
   */
  function check(msg, sender, selfId) {
    const spec = msg && typeof msg.type === 'string' && Object.prototype.hasOwnProperty.call(SPEC, msg.type) ? SPEC[msg.type] : null
    if (!spec) return { ok: false, code: 'unknown', error: '不支持的请求' }
    if (!sender || !selfId || sender.id !== selfId) return { ok: false, code: 'forbidden', error: '请求被拒绝：发送者不是本扩展' }
    if (spec.needTab && !sender.tab) return { ok: false, code: 'no_tab', error: '请求被拒绝：这个请求只能来自网页里的内容脚本' }
    const out = { type: msg.type }
    try {
      for (const [k, r] of Object.entries(spec.fields || {})) {
        const x = clean(r, msg[k], k)
        if (x !== undefined) out[k] = x
      }
    } catch (e) {
      if (!(e instanceof Bad)) throw e
      return { ok: false, code: 'bad_request', error: `请求无效：${e.message}` }
    }
    return { ok: true, msg: out }
  }

  /** 只检查发送者是本扩展（没有字段要整理的简单消息，如 cc-bypass）。 */
  function fromSelf(sender, selfId) {
    return !!sender && !!selfId && sender.id === selfId
  }

  return { check, fromSelf, SPEC, LIMITS }
})()
