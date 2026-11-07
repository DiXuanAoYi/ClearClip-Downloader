// 视频摘要面板：评论页（读取、分析、问答、在页面里定位）。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.comments = ({
    S, V, actsBar, addNote, copy, ensureReady, h, localKey, outLang, render, saveFile, savePrefs, seek, sel, store,
    submit, toast, ui
  }) => {
    // ---------- 评论分析 ----------

    const CM_KEEP = 6

    async function cmLoadSaved() {
      const k = localKey()
      if (S.cmLoading) return
      S.cmLoading = true
      try {
        const all = await store.get('vsumComments', {})
        const old = S.cm
        S.cm = { key: k, list: all[k]?.list || [], res: all[k]?.res || null, sort: old?.sort || 'hot', count: old?.count || 300, loading: false, n: 0, err: '', ask: [], stop: false, asking: false }
      } finally {
        S.cmLoading = false
      }
      render()
    }

    async function cmSave() {
      const c = S.cm
      const all = await store.get('vsumComments', {})
      delete all[c.key]
      all[c.key] = { list: c.list, res: c.res, ts: Date.now() }
      const keys = Object.keys(all)
      for (const k of keys.slice(0, Math.max(0, keys.length - CM_KEEP))) delete all[k]
      await store.set('vsumComments', all)
    }

    const CM_GAP = 90 * 1000 // 同站点两次读取至少隔 90 秒
    const CM_COOL = 15 * 60 * 1000 // 被限流后冷却 15 分钟
    const cmSite = () => V.videoIdOf(S.data?.url || location.href)?.site || ''

    /** 返回还需等待的毫秒数（0 = 可以读）。 */
    async function cmWait() {
      const m = (await store.get('vsumCmGate', {}))[cmSite()] || {}
      return Math.max(0, (m.until || 0) - Date.now(), (m.last || 0) + CM_GAP - Date.now())
    }

    async function cmGate(patch) {
      const all = await store.get('vsumCmGate', {})
      all[cmSite()] = { ...(all[cmSite()] || {}), ...patch }
      await store.set('vsumCmGate', all)
    }

    async function cmRead() {
      const c = S.cm
      const wait = await cmWait()
      if (wait > 0) {
        c.err = `读取太频繁，为避免被站点限制，请 ${Math.ceil(wait / 1000) >= 120 ? `${Math.ceil(wait / 60000)} 分钟` : `${Math.ceil(wait / 1000)} 秒`}后再试。已读取的评论可以直接分析。`
        render()
        return
      }
      await cmGate({ last: Date.now() })
      c.loading = true
      c.err = ''
      c.n = 0
      c.stop = false
      render()
      try {
        c.list = await globalThis.ccVsumSource.loadComments({ count: c.count, sort: c.sort }, (n) => ((c.n = n), !S.closed && S.tab === 'comments' && cmProgress()), () => c.stop || S.closed)
        c.res = null
        c.ask = []
        await cmSave()
      } catch (e) {
        c.err = e.block ? `${e.message}。已暂停 15 分钟，已读取的评论仍可分析。` : e.message
        if (e.block) await cmGate({ until: Date.now() + CM_COOL })
      } finally {
        c.loading = false
        if (!S.closed) render()
      }
    }

    function cmProgress() {
      const el = ui.root.getElementById('cmprog')
      if (el) el.textContent = `已读 ${S.cm.n} / ${S.cm.count}`
    }

    async function cmAnalyze() {
      const c = S.cm
      if (S.busy || !c.list.length) return
      S.busy = true
      render()
      try {
        await ensureReady()
        const comments = c.list.map((x) => ({ author: x.author, text: x.text, likes: x.likes, t: x.t, replies: x.replies }))
        c.res = await submit({ kind: 'comments', title: S.data?.title || document.title, lang: outLang(), comments }, 'comments')
        await cmSave()
      } catch (e) {
        if (!S.closed) toast(ui, e.message, true, 5200)
      } finally {
        S.busy = false
        if (!S.closed) render()
      }
    }

    async function cmAsk(q) {
      const c = S.cm
      q = String(q || '').trim()
      if (!q || S.busy || !c.list.length) return
      S.busy = true
      c.ask.push({ q, a: '' })
      render()
      try {
        await ensureReady()
        const comments = c.list.map((x) => ({ author: x.author, text: x.text, likes: x.likes, t: x.t, replies: x.replies }))
        const r = await submit({ kind: 'askcomments', title: S.data?.title || document.title, lang: outLang(), question: q, comments }, '')
        c.ask[c.ask.length - 1].a = r.answer
      } catch (e) {
        c.ask.pop()
        if (!S.closed) toast(ui, e.message, true, 5200)
      } finally {
        S.busy = false
        if (!S.closed) render()
      }
    }

    // ---------- 评论定位：在页面的评论区里找到这一条，滚过去并高亮 ----------

    /** 递归收集页面里（含 shadow DOM）所有匹配的元素。 */
    function deepAll(root, selector, out = []) {
      for (const el of root.querySelectorAll('*')) {
        if (el.matches(selector)) out.push(el)
        if (el.shadowRoot) deepAll(el.shadowRoot, selector, out)
      }
      return out
    }
    /** 页面上现有的评论节点：{ el: 用来高亮的外层元素, text, id }。 */
    function pageComments() {
      const site = V.videoIdOf(location.href)?.site
      const out = []
      if (site === 'youtube') {
        for (const th of document.querySelectorAll('ytd-comment-thread-renderer, ytd-comment-view-model, ytd-comment-renderer')) {
          const body = th.querySelector('#content-text')
          if (!body) continue
          const link = th.querySelector('a[href*="lc="]')
          const id = link ? new URL(link.href, location.href).searchParams.get('lc') || '' : ''
          out.push({ el: th, text: body.textContent || '', id: id.split('.')[0] })
        }
      } else if (site === 'bilibili') {
        for (const rt of deepAll(document, 'bili-rich-text')) {
          const text = rt.shadowRoot?.querySelector('#contents')?.textContent || rt.textContent || ''
          let host = rt
          for (let i = 0; i < 6 && host; i++) {
            const root = host.getRootNode()
            host = root instanceof ShadowRoot ? root.host : host.parentElement
            if (host?.tagName === 'BILI-COMMENT-RENDERER' || host?.tagName === 'BILI-COMMENT-REPLY-RENDERER') break
          }
          out.push({ el: host || rt, text, id: '' })
        }
      }
      return out
    }
    function flash(el) {
      const old = { outline: el.style.outline, bg: el.style.background, tr: el.style.transition }
      el.style.transition = 'background .4s, outline-color .4s'
      el.style.outline = '3px solid #e0a82e'
      el.style.background = '#fff4d6'
      setTimeout(() => {
        el.style.outline = old.outline
        el.style.background = old.bg
        el.style.transition = old.tr
      }, 2400)
    }
    let locating = 0
    /** 找这一条评论：已在页面上就滚过去；没有就一边向下滚动加载一边找（最多 20 轮，可取消）。 */
    async function locateComment(q) {
      const c = V.findComment(S.cm?.list, q) || q
      const key = V.cmKey(c.text)
      const head = key.slice(0, 20)
      if (!head) return toast(ui, '这条评论没有文字，没法定位', true)
      const token = ++locating
      for (let round = 0; round < 20 && token === locating && !S.closed; round++) {
        const hit = pageComments().find((p) => (c.id && p.id === c.id) || V.cmKey(p.text).startsWith(head) || (head.length >= 12 && V.cmKey(p.text).includes(head)))
        if (hit) {
          hit.el.scrollIntoView({ block: 'center', behavior: 'smooth' })
          flash(hit.el)
          return toast(ui, '已定位到这条评论')
        }
        if (round === 0 && !pageComments().length) {
          // 评论区还没出现：先滚到页面下方触发懒加载
          document.querySelector('#comments, ytd-comments, bili-comments')?.scrollIntoView({ block: 'start' })
        } else window.scrollBy({ top: Math.round(innerHeight * 0.9), behavior: 'auto' })
        toast(ui, `正在向下查找这条评论（第 ${round + 1} 次）…`, false, 900)
        await new Promise((r) => setTimeout(r, 800))
      }
      if (token !== locating) return
      navigator.clipboard.writeText(c.text).catch(() => {})
      const link = V.commentLink(S.data?.url || location.href, c)
      toast(ui, link ? '页面里没找到，可能被折叠或删除了。文字已复制，也可以用“新标签页打开”' : '页面里没找到，可能被折叠或删除了。评论文字已复制，可以用页内搜索', true, 4500)
    }

    const cmLocate = (q) => {
      const c = V.findComment(S.cm?.list, q) || q
      const link = V.commentLink(S.data?.url || location.href, c)
      return [
        h('button', { class: 'sec', text: '定位 ↗', title: '在页面的评论区里找到这条评论并高亮', onclick: () => locateComment(q) }),
        link && h('button', { class: 'sec', text: '新标签打开', title: 'YouTube 的评论直达链接', onclick: () => window.open(link, '_blank', 'noopener') }),
      ]
    }

    const cmQuote = (q) =>
      q &&
      h('div', { class: 'cq' }, q.text,
        h('div', { class: 'm' },
          h('span', { text: [q.author && `@${q.author}`, q.likes ? `赞 ${q.likes}` : ''].filter(Boolean).join(' · ') }),
          q.t >= 0 && h('button', { class: 'sec', text: V.fmtClock(q.t), title: '跳到视频里的这个时间', onclick: () => seek(q.t) }),
          ...cmLocate(q)))

    function tabComments() {
      if (S.cm?.key !== localKey()) {
        cmLoadSaved()
        return h('div', { class: 'mute', text: '…' })
      }
      const c = S.cm
      const r = c.res
      const supported = ['youtube', 'bilibili'].includes(V.videoIdOf(S.data?.url || location.href)?.site)
      const head = h('div', { class: 'card' },
        h('div', { class: 'tools' },
          h('b', { text: c.list.length ? `已读取 ${c.list.length} 条评论` : '读取评论' }),
          h('span', { class: 'mute', style: 'flex:1' }),
          sel([['100', '100 条'], ['300', '300 条'], ['500', '500 条'], ['1000', '1000 条']], String(c.count), (v) => ((c.count = Number(v)), savePrefs()), '读取多少条'),
          sel([['hot', '热门'], ['new', '最新']], c.sort, (v) => (c.sort = v), '排序')),
        !c.list.length && h('div', { class: 'mute', text: supported ? '读取这个视频的评论后，可以分析整体情绪、主要观点、观众提问和纠错补充。评论只在本机的清影里处理，不会上传到别处。' : '这个站点暂不支持直接读取评论（目前支持 YouTube 和哔哩哔哩）。' }),
        c.err && h('div', { class: 'warn', text: c.err }),
        c.loading
          ? h('div', { class: 'tools' }, h('span', { class: 'mute', id: 'cmprog', text: `已读 ${c.n} / ${c.count}` }), h('button', { class: 'sec', text: '停止', onclick: () => (c.stop = true) }))
          : h('div', { class: 'tools' },
              ...(!c.list.length
                ? [h('button', { text: '读取并分析', disabled: !supported || S.busy, onclick: async () => (await cmRead(), S.cm.list.length && cmAnalyze()) }), h('button', { class: 'sec', text: '只读取', disabled: !supported || S.busy, onclick: cmRead })]
                : [h('button', { text: r ? '重新分析' : '开始分析', disabled: S.busy, onclick: cmAnalyze }), h('button', { class: 'sec', text: '重新读取', disabled: !supported || S.busy, onclick: cmRead })])))
      const out = [head]
      if (r) {
        const sm = r.sentiment
        out.push(
          h('div', { class: 'card' },
            h('div', { class: 'tools' }, h('b', { text: '整体情绪' }), h('span', { class: 'mute', text: `基于 ${r.count} 条评论` })),
            h('div', { class: 'cmbar' }, h('i', { class: 'p', style: `width:${sm.pos}%` }), h('i', { class: 'u', style: `width:${sm.neu}%` }), h('i', { class: 'n', style: `width:${sm.neg}%` })),
            h('div', { class: 'cmleg' }, h('span', { text: `正面 ${sm.pos}%` }), h('span', { text: `中性 ${sm.neu}%` }), h('span', { text: `负面 ${sm.neg}%` })),
            r.verdict && h('div', { style: 'margin-top:6px', text: r.verdict })))
        if (r.themes?.length) {
          const max = Math.max(1, ...r.themes.map((t) => t.count))
          out.push(h('div', { class: 'card' }, h('b', { text: '主要观点' }),
            ...r.themes.map((t) => h('div', { class: 'cmth' },
              h('div', { class: 'tools' }, h('b', { text: t.name, style: 'flex:1' }), h('span', { class: 'mute', text: `${t.count} 人` })),
              h('div', { class: 'cmb' }, h('i', { style: `width:${Math.round((t.count / max) * 100)}%` })),
              ...(t.quotes || []).map(cmQuote)))))
        }
        if (r.disputes?.length) {
          out.push(h('div', { class: 'card' }, h('b', { text: '争议点' }),
            ...r.disputes.map((d) => h('div', { class: 'cmth' }, h('div', {}, h('span', { class: 'tg no', text: '分歧' }), d.topic, h('span', { class: 'mute', text: `  支持 ${d.pro} · 质疑 ${d.con}` })), ...(d.quotes || []).map(cmQuote)))))
        }
        if (r.questions?.length) {
          const open = r.questions.filter((x) => !x.answered).length
          out.push(h('div', { class: 'card' }, h('div', { class: 'tools' }, h('b', { text: '观众提问' }), h('span', { class: 'mute', text: `${r.questions.length} 个 · ${open} 个未解答` })),
            ...r.questions.map((x) => h('div', { class: 'cmq' }, h('span', { class: `tg ${x.answered ? 'ok' : 'no'}`, text: x.answered ? '已回答' : '未解答' }), h('span', { text: x.q })))))
        }
        if (r.corrections?.length) {
          out.push(h('div', { class: 'card' }, h('b', { text: '纠错与补充' }),
            ...r.corrections.map((x) => h('div', { class: 'cmth' }, h('div', { text: x.text }), cmQuote(x.quote)))))
        }
        if (r.keywords?.length) out.push(h('div', { class: 'card' }, h('b', { text: '热词' }), h('div', { class: 'chips' }, ...r.keywords.map((k) => h('span', { class: 'chip', text: k })))))
      }
      if (c.list.length) {
        const shown = c.showAll ? c.list.slice(0, 80) : []
        out.push(h('div', { class: 'card' },
          h('div', { class: 'tools' }, h('b', { text: '全部评论' }), h('span', { class: 'mute', style: 'flex:1', text: `按点赞排序，共 ${c.list.length} 条` }), h('button', { class: 'sec', text: c.showAll ? '收起' : '展开', onclick: () => ((c.showAll = !c.showAll), render()) })),
          ...shown.map((x) => h('div', { class: 'cq' }, x.text, h('div', { class: 'm' }, h('span', { text: [x.author && `@${x.author}`, x.likes ? `赞 ${x.likes}` : ''].filter(Boolean).join(' · ') }), ...cmLocate(x))))))
      }
      if (c.list.length) {
        const inp = h('input', { type: 'text', placeholder: '问评论：例如“大家最希望作者下期讲什么？”', style: 'flex:1', onkeydown: (e) => e.key === 'Enter' && !e.isComposing && (cmAsk(inp.value), (inp.value = '')) })
        out.push(h('div', { class: 'card' }, h('b', { text: '问评论' }),
          ...c.ask.map((m) => h('div', { class: 'cmth' }, h('div', { class: 'mute', text: m.q }), h('div', { text: m.a || '正在想…' }))),
          h('div', { class: 'tools', style: 'margin-top:6px' }, inp, h('button', { text: '发送', disabled: S.busy, onclick: () => (cmAsk(inp.value), (inp.value = '')) }))))
      }
      if (r) {
        out.push(actsBar(
          h('button', { class: 'sec', text: '导出 Markdown', onclick: () => saveFile(V.fileName(`${S.data.title} 评论分析`, 'md'), V.commentsToMarkdown(S.data.title, r), 'text/markdown') }),
          h('button', { class: 'sec', text: '存为笔记', onclick: () => addNote({ t: -1, text: `评论分析：${r.verdict || ''}\n${(r.themes || []).map((t) => `- ${t.name}（${t.count} 人）`).join('\n')}`.trim(), kind: 'note' }) }),
          h('button', { class: 'sec', text: '复制', onclick: () => copy(V.commentsToMarkdown(S.data.title, r), '评论分析') })))
      }
      return h('div', {}, ...out)
    }

    return { tabComments }
  }
})()
