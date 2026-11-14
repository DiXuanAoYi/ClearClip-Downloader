// 视频摘要纯逻辑的拆分模块（由 vsum-logic.js 汇总成对外的 ccVsum 对象）：不碰网页和浏览器接口；后面的模块直接取前面模块放进 C 的函数。
// 搜索高亮与回答引用校验、评论解析、面板收到右边时小标签的位置和状态。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsumLogic ||= {})
  P.comments = (C) => {
    const { fmtClock, lineAt, parseClock, videoIdOf } = C

    // ---------- 搜索高亮 / 回答引用校验（参考 youtube-digest、bilibili-digest，MIT） ----------

    /** 把文字按搜索词切成 [{s, hit}]，界面里命中的部分用 <mark> 高亮。 */
    function splitMarks(text, query) {
      const terms = [...new Set(String(query || '').toLowerCase().split(/\s+/).filter(Boolean))]
      const src = String(text || '')
      if (!terms.length) return [{ s: src, hit: false }]
      const low = src.toLowerCase()
      const mask = new Array(src.length).fill(false)
      for (const t of terms) {
        for (let i = low.indexOf(t); i >= 0; i = low.indexOf(t, i + t.length)) for (let k = i; k < i + t.length; k++) mask[k] = true
      }
      const out = []
      for (let i = 0; i < src.length; ) {
        let j = i
        while (j < src.length && mask[j] === mask[i]) j++
        out.push({ s: src.slice(i, j), hit: mask[i] })
        i = j
      }
      return out
    }

    /**
     * 回答里的 [m:ss] / [m:ss-m:ss] 引用：只认落在文稿时间范围内的，并从文稿里取出那一句当“依据”
     * （由本地取，模型编不出来）。一个有效引用都没有、又没说“未找到”的回答，标成 unsupported 提醒用户别全信。
     */
    function citations(answer, lines) {
      const ls = lines || []
      const first = ls.length ? ls[0].t : 0
      const last = ls.length ? ls[ls.length - 1].t : 0
      const refs = []
      const seen = new Set()
      let bad = 0
      for (const m of String(answer || '').matchAll(/\[(\d{1,2}(?::\d{2}){1,2})(?:\s*[-–~]\s*(\d{1,2}(?::\d{2}){1,2}))?\]/g)) {
        const ms = parseClock(m[1])
        if (!(ms >= first - 1000 && ms <= last + 30000)) {
          bad++
          continue
        }
        const i = lineAt(ls, ms)
        if (i < 0 || seen.has(i)) continue
        seen.add(i)
        refs.push({ ms: ls[i].t, label: fmtClock(ls[i].t), text: ls[i].text })
      }
      const notFound = /未找到|没有找到|找不到|not found|no (?:relevant|supporting)/i.test(String(answer || ''))
      return { refs, bad, notFound, unsupported: !refs.length && !notFound }
    }

    const STYLES = [
      ['standard', '标准'],
      ['brief', '简短'],
      ['detailed', '详细'],
      ['plain', '通俗'],
      ['study', '学习笔记'],
    ]
    const REWRITES = [
      ['xhs', '小红书文案'],
      ['thread', '推文串'],
      ['blog', '博客文章'],
      ['email', '分享邮件'],
      ['notes', '会议纪要式笔记'],
    ]
    const SUGGESTIONS = ['这个视频的核心观点是什么？', '有哪些可以直接照做的建议？', '作者提到了哪些数据或例子？', '有哪些我可能不同意的地方？']


    // ---------- 评论 ----------

    /** “1.2万”“3.4K”“1,234”“2M”这类点赞数写法转成整数；读不出来是 0。 */
    function parseCount(v) {
      if (typeof v === 'number') return Math.max(0, Math.round(v))
      const m = String(v ?? '').replace(/[,\s]/g, '').match(/^([\d.]+)(万|亿|[kKmMbB])?/)
      if (!m) return 0
      const n = parseFloat(m[1])
      if (!Number.isFinite(n)) return 0
      const mul = { 万: 1e4, 亿: 1e8, k: 1e3, m: 1e6, b: 1e9 }[(m[2] || '').toLowerCase()] || 1
      return Math.round(n * mul)
    }

    /** 评论里写的视频时间（如 05:21、1:02:03），取第一个，返回毫秒；没有是 -1。 */
    function commentTime(text) {
      const m = String(text || '').match(/(?:^|[^\d:])((?:\d{1,2}:)?\d{1,2}:\d{2})(?![\d:])/)
      const t = m ? parseClock(m[1]) : null
      return t == null ? -1 : t
    }

    const cmt = (author, text, likes, replies, id = '') => {
      const t = String(text || '').replace(/\s+/g, ' ').trim()
      return { author: String(author || '').replace(/^@/, '').slice(0, 40), text: t.slice(0, 600), likes: parseCount(likes), replies: parseCount(replies), t: commentTime(t), id: String(id || '').slice(0, 80) }
    }

    /** 把一段文字压成可比较的形式：去掉空白和标点，转小写。 */
    const cmKey = (s) => String(s || '').replace(/[\s\p{P}\p{S}]+/gu, '').toLowerCase()

    /** 在读取到的评论里找出“这一条”：先按编号，再按作者 + 文字，最后按文字开头。找不到返回 null。 */
    function findComment(list, q) {
      if (!q) return null
      const arr = list || []
      if (q.id) {
        const m = arr.find((c) => c.id === q.id)
        if (m) return m
      }
      const k = cmKey(q.text)
      if (!k) return null
      const same = (c) => cmKey(c.text) === k
      const byAuthor = q.author ? arr.find((c) => same(c) && c.author === q.author) : null
      if (byAuthor) return byAuthor
      const exact = arr.find(same)
      if (exact) return exact
      const head = k.slice(0, 24)
      return arr.find((c) => {
        const ck = cmKey(c.text)
        return ck.startsWith(head) || (head.length >= 12 && ck.includes(head)) || (ck.length >= 8 && k.startsWith(ck))
      }) || null
    }

    /** YouTube 评论的直达链接（官方的 lc 参数）；没有编号返回空串。 */
    function commentLink(url, c) {
      const v = videoIdOf(url)
      if (v?.site === 'youtube' && c?.id) return `https://www.youtube.com/watch?v=${v.id}&lc=${encodeURIComponent(c.id)}`
      return ''
    }

    /**
     * 解析 YouTube `youtubei/v1/next` 的响应：
     * { items: [{author,text,likes,replies,t}], next: 下一页令牌, sorts: [{title, token}] }
     * 新旧两种返回格式都认（commentThreadRenderer / commentEntityPayload）。
     */
    function parseYtComments(json) {
      const out = { items: [], next: '', sorts: [] }
      const acts = [...(json?.onResponseReceivedEndpoints || []), ...(json?.onResponseReceivedActions || [])]
      const entities = {}
      for (const m of json?.frameworkUpdates?.entityBatchUpdate?.mutations || []) {
        const p = m?.payload?.commentEntityPayload
        if (p?.key || m?.entityKey) entities[p?.key || m.entityKey] = p
      }
      const text = (c) => (c?.runs ? c.runs.map((r) => r.text).join('') : c?.simpleText || '')
      for (const a of acts) {
        const list = a?.reloadContinuationItemsCommand?.continuationItems || a?.appendContinuationItemsAction?.continuationItems || []
        if (a?.reloadContinuationItemsCommand?.slot === 'RELOAD_CONTINUATION_SLOT_HEADER') {
          for (const it of list[0]?.commentsHeaderRenderer?.sortMenu?.sortFilterSubMenuRenderer?.subMenuItems || []) {
            const token = it?.serviceEndpoint?.continuationCommand?.token
            if (token) out.sorts.push({ title: it.title || '', token })
          }
        }
        for (const it of list) {
          const tok = it?.continuationItemRenderer?.continuationEndpoint?.continuationCommand?.token
          if (tok) out.next = tok
          const th = it?.commentThreadRenderer
          if (!th) continue
          const old = th.comment?.commentRenderer
          if (old) {
            out.items.push(cmt(text(old.authorText), text(old.contentText), old.voteCount?.simpleText || old.likeCount, th.replies?.commentRepliesRenderer?.moreText ? text(th.replies.commentRepliesRenderer.moreText).replace(/\D/g, '') : old.replyCount, old.commentId))
            continue
          }
          const vm = th.commentViewModel?.commentViewModel || th.commentViewModel
          const p = entities[vm?.commentKey]
          if (p) out.items.push(cmt(p.author?.displayName, p.properties?.content?.content, p.toolbar?.likeCountNotliked ?? p.toolbar?.likeCountA11y, p.toolbar?.replyCount, p.properties?.commentId))
        }
      }
      return out
    }

    /** 解析 B 站 `x/v2/reply/main` 的响应：{ items, next, end }。 */
    function parseBiliComments(json) {
      const d = json?.data || {}
      const all = [...(d.top_replies || []), ...(d.replies || [])]
      return {
        items: all.map((r) => cmt(r?.member?.uname, r?.content?.message, r?.like, r?.rcount, r?.rpid_str || r?.rpid)),
        next: d.cursor?.next ?? 0,
        end: !!d.cursor?.is_end || !all.length,
      }
    }

    /** 去掉重复和过短的评论，按点赞数从高到低。 */
    function cleanComments(list) {
      const seen = new Set()
      return [...(list || [])]
        .sort((a, b) => b.likes - a.likes)
        .filter((c) => {
          const k = c.text
          if (!k || k.length < 2 || seen.has(k)) return false
          seen.add(k)
          return true
        })
    }

    /** 评论分析结果转成 Markdown。 */
    function commentsToMarkdown(title, r) {
      const q = (x) => (x ? `\n  > ${x.text}${x.author ? `（@${x.author}${x.likes ? ` · 赞 ${x.likes}` : ''}）` : ''}` : '')
      const L = [`# ${title || '视频'} · 评论分析`, '', `共分析 ${r.count} 条评论。`, '', '## 整体情绪', `正面 ${r.sentiment.pos}% · 中性 ${r.sentiment.neu}% · 负面 ${r.sentiment.neg}%`, '']
      if (r.verdict) L.push(r.verdict, '')
      if (r.themes?.length) L.push('## 主要观点', ...r.themes.map((t) => `- **${t.name}**（${t.count} 人）${(t.quotes || []).map(q).join('')}`), '')
      if (r.disputes?.length) L.push('## 争议点', ...r.disputes.map((d) => `- ${d.topic}（支持 ${d.pro} · 质疑 ${d.con}）`), '')
      if (r.questions?.length) L.push('## 观众提问', ...r.questions.map((x) => `- [${x.answered ? '已回答' : '未解答'}] ${x.q}`), '')
      if (r.corrections?.length) L.push('## 纠错与补充', ...r.corrections.map((x) => `- ${x.text}${q(x.quote)}`), '')
      if (r.keywords?.length) L.push('## 热词', r.keywords.join('、'), '')
      return L.join('\n')
    }

    // ---------- 面板收到右边：小标签的位置、三种状态的切换 ----------

    const DOCK_TAB_H = 84
    const DOCK_TAB_W = 30
    /** 小标签离窗口顶部多少像素：frac 是中心在窗口高度的几分之几；贴着上下边留 8 像素。 */
    function dockTabTop(frac, vh) {
      const f = Number.isFinite(frac) ? Math.min(1, Math.max(0, frac)) : 0.38
      const max = Math.max(8, vh - DOCK_TAB_H - 8)
      return Math.round(Math.min(max, Math.max(8, f * vh - DOCK_TAB_H / 2)))
    }
    /** 拖到 y（鼠标在窗口里的纵坐标）后记下的 frac。 */
    function dockFrac(y, vh) {
      return vh > 0 ? Math.round(Math.min(0.95, Math.max(0.05, y / vh)) * 1000) / 1000 : 0.38
    }
    /**
     * 面板的状态：open 展开 / hidden 收起 / peek 预览。
     * 动作：toggle（快捷键）、collapse（⇥）、click（点小标签）、hover（在标签上停够了）、leave（移开够久了）、pin（固定 / 在面板里点了）。
     */
    function dockNext(state, action) {
      switch (action) {
        case 'toggle':
          return state === 'open' ? 'hidden' : 'open'
        case 'collapse':
          return 'hidden'
        case 'click':
          return state === 'hidden' ? 'open' : 'hidden'
        case 'hover':
          return state === 'hidden' ? 'peek' : state
        case 'leave':
          return state === 'peek' ? 'hidden' : state
        case 'pin':
          return 'open'
        default:
          return state
      }
    }

    Object.assign(C, { dockTabTop, dockFrac, dockNext, parseCount, commentTime, findComment, commentLink, cmKey, parseYtComments, parseBiliComments, cleanComments, commentsToMarkdown, splitMarks, citations, STYLES, REWRITES, SUGGESTIONS })
  }
})()
