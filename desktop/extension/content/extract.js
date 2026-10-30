// 保存为 Markdown：找到页面正文（有选区就只用选区），转成 Markdown。尽力而为：不同网站的结构差别很大，所以结果会先给用户预览、修改。
// 按需注入；最后一个表达式的值就是返回结果。
(() => {
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'SVG', 'CANVAS', 'IFRAME', 'NAV', 'ASIDE', 'FOOTER', 'FORM', 'BUTTON', 'SELECT', 'TEXTAREA', 'INPUT', 'TEMPLATE', 'AUDIO', 'VIDEO', 'OBJECT', 'EMBED'])
  const abs = (u) => {
    try {
      return new URL(u, document.baseURI).href
    } catch {
      return ''
    }
  }
  const text = (n) => (n.textContent || '').replace(/\s+/g, ' ').trim()

  function textLen(el) {
    return text(el).length
  }

  /** 正文候选：有语义标签的优先；否则按“段落文字量 − 链接文字量”给容器打分。 */
  function findMain() {
    const semantic = [...document.querySelectorAll('article, main, [role=main], .post-content, .article-content, .entry-content, .post-body, #content, .content')].filter((e) => textLen(e) > 200)
    if (semantic.length) {
      const articles = semantic.filter((e) => e.tagName === 'ARTICLE')
      const pool = articles.length === 1 ? articles : semantic
      return pool.sort((a, b) => textLen(b) - textLen(a))[0]
    }
    let best = null
    let bestScore = 0
    for (const el of document.querySelectorAll('div, section')) {
      let p = 0
      for (const c of el.children) if (c.tagName === 'P') p += textLen(c)
      const links = [...el.querySelectorAll('a')].reduce((n, a) => n + textLen(a), 0)
      const score = p - links * 0.5
      if (score > bestScore) {
        bestScore = score
        best = el
      }
    }
    return bestScore > 200 ? best : document.body
  }

  const state = { list: 0 }

  /** 行内标记：标记要贴着文字，文字两头的空白留在标记外面。 */
  function wrap(mark, t) {
    const core = t.trim()
    if (!core) return t
    return `${/^\s/.test(t) ? ' ' : ''}${mark}${core}${mark}${/\s$/.test(t) ? ' ' : ''}`
  }

  function inline(node, ctx) {
    return [...node.childNodes].map((c) => conv(c, ctx)).join('')
  }

  function conv(n, ctx = {}) {
    if (n.nodeType === 3) return ctx.pre ? n.nodeValue : n.nodeValue.replace(/\s+/g, ' ')
    if (n.nodeType !== 1) return ''
    const tag = n.tagName
    if (SKIP.has(tag) || n.hidden || n.getAttribute('aria-hidden') === 'true') return ''
    const kids = () => inline(n, ctx)
    switch (tag) {
      case 'H1': case 'H2': case 'H3': case 'H4': case 'H5': case 'H6': {
        const t = kids().trim()
        return t ? `\n\n${'#'.repeat(Number(tag[1]))} ${t}\n\n` : ''
      }
      case 'P': case 'FIGURE': case 'FIGCAPTION': case 'SECTION': case 'ARTICLE': case 'MAIN': case 'DIV': case 'HEADER': case 'DETAILS': case 'SUMMARY': case 'DL': case 'DD': case 'DT': {
        const t = kids()
        return t.trim() ? `\n\n${t.trim()}\n\n` : t
      }
      case 'BR': return '  \n'
      case 'HR': return '\n\n---\n\n'
      case 'STRONG': case 'B': return wrap('**', kids())
      case 'EM': case 'I': return wrap('*', kids())
      case 'DEL': case 'S': return wrap('~~', kids())
      case 'CODE': {
        if (ctx.pre) return n.textContent
        return wrap('`', n.textContent.replace(/`/g, "'"))
      }
      case 'PRE': {
        const code = n.querySelector('code')
        const lang = ((code || n).className.match(/(?:language|lang)-([\w+#-]+)/) || [])[1] || ''
        return `\n\n\`\`\`${lang}\n${(code || n).textContent.replace(/\n$/, '')}\n\`\`\`\n\n`
      }
      case 'A': {
        const t = kids().trim()
        const href = n.getAttribute('href') || ''
        if (!t) return ''
        if (!href || href.startsWith('#') || /^javascript:/i.test(href)) return t
        return `[${t}](${abs(href)})`
      }
      case 'IMG': {
        const src = n.currentSrc || n.getAttribute('src') || n.getAttribute('data-src') || n.getAttribute('data-original') || ''
        const u = abs(src)
        return /^https?:/i.test(u) ? `![${(n.getAttribute('alt') || '').replace(/[\[\]]/g, '')}](${u})` : ''
      }
      case 'BLOCKQUOTE': {
        const t = kids().trim()
        return t ? `\n\n${t.split('\n').map((l) => `> ${l}`).join('\n')}\n\n` : ''
      }
      case 'UL': case 'OL': {
        const depth = state.list++
        const items = [...n.children].filter((c) => c.tagName === 'LI').map((li, i) => {
          const mark = tag === 'OL' ? `${i + 1}.` : '-'
          const body = inline(li, ctx).trim().replace(/\n{3,}/g, '\n\n').replace(/\n/g, '\n  ')
          return `${'  '.repeat(depth)}${mark} ${body}`
        })
        state.list--
        return depth ? `\n${items.join('\n')}\n` : `\n\n${items.join('\n')}\n\n`
      }
      case 'TABLE': {
        const rows = [...n.querySelectorAll('tr')].map((tr) => [...tr.children].map((c) => text(c).replace(/\|/g, '\\|')))
        if (!rows.length) return ''
        const w = Math.max(...rows.map((r) => r.length))
        const line = (r) => `| ${Array.from({ length: w }, (_, i) => r[i] || '').join(' | ')} |`
        return `\n\n${line(rows[0])}\n| ${Array(w).fill('---').join(' | ')} |\n${rows.slice(1).map(line).join('\n')}\n\n`
      }
      default:
        return kids()
    }
  }

  const sel = getSelection()
  const hasSel = !!sel && !sel.isCollapsed && sel.toString().trim().length > 0
  let root
  if (hasSel) {
    root = document.createElement('div')
    for (let i = 0; i < sel.rangeCount; i++) root.append(sel.getRangeAt(i).cloneContents())
  } else {
    root = findMain()
  }
  let body = conv(root)
    .replace(/[ \t]+\n/g, (m) => (m.startsWith('  ') && m.length === 3 ? m : '\n'))
    .replace(/\n{3,}/g, '\n\n')
    .replace(/(\S) {2,}(?=\S)/g, '$1 ')
    .trim()
  const title = (document.title || '').trim()
  const md = `# ${title || location.hostname}\n\n> 来源：${location.href}\n\n${body}\n`
  return { title, url: location.href, markdown: md, selection: hasSel, chars: body.length }
})()
