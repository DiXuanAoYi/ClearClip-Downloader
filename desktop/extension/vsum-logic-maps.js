// 视频摘要纯逻辑的拆分模块（由 vsum-logic.js 汇总成对外的 ccVsum 对象）：不碰网页和浏览器接口；后面的模块直接取前面模块放进 C 的函数。
// 导图与流程 / 因果图的布局和 SVG。
;(() => {
  const P = ((globalThis.ccParts ||= {}).vsumLogic ||= {})
  P.maps = (C) => {
    const { fmtClock, parseClock } = C

    // ---------- 导图 ----------

    /** Markdown 大纲 → 树。第一行 `# 标题` 是根；缺了就用传进来的标题。 */
    function outlineToTree(md, fallbackTitle = '') {
      const root = { text: fallbackTitle || '视频', children: [] }
      const stack = [{ depth: -1, node: root }]
      for (const raw of String(md || '').split('\n')) {
        const line = raw.replace(/\t/g, '  ').replace(/\s+$/, '')
        const head = line.match(/^#\s+(.+)$/)
        if (head && root.children.length === 0 && stack.length === 1) {
          root.text = head[1].trim()
          continue
        }
        const m = line.match(/^(\s*)[-*]\s+(.+)$/)
        if (!m) continue
        const depth = Math.floor(m[1].length / 2)
        const node = { text: m[2].trim(), children: [] }
        while (stack.length > 1 && stack[stack.length - 1].depth >= depth) stack.pop()
        stack[stack.length - 1].node.children.push(node)
        stack.push({ depth, node })
      }
      return root
    }

    const textWidth = (s) => [...String(s)].reduce((w, c) => w + (/[⺀-鿿＀-￯]/.test(c) ? 14 : 7.2), 0)

    /** 把长文字折成多行，每行不超过 maxW。 */
    function wrapText(s, maxW = 190) {
      const lines = []
      let cur = ''
      let w = 0
      for (const ch of String(s)) {
        const cw = textWidth(ch)
        if (w + cw > maxW && cur) {
          lines.push(cur)
          cur = ''
          w = 0
        }
        cur += ch
        w += cw
      }
      if (cur) lines.push(cur)
      return lines.length ? lines : ['']
    }

    /** 向右展开的树形布局：返回每个节点的位置和连线，宽高都算好。 */
    function layoutMindmap(tree, opt = {}) {
      const gapX = opt.gapX ?? 36
      const gapY = opt.gapY ?? 10
      const pad = opt.pad ?? 16
      const maxW = opt.maxW ?? 190
      const nodes = []
      const links = []
      const measure = (n, depth) => {
        const lines = wrapText(n.text, depth === 0 ? maxW + 40 : maxW)
        const w = Math.min(Math.max(...lines.map(textWidth)), maxW + 40) + 20
        const h = lines.length * 18 + 12
        const kids = n.children.map((c) => measure(c, depth + 1))
        const span = kids.length ? Math.max(h, kids.reduce((a, k) => a + k.span, 0) + gapY * (kids.length - 1)) : h
        return { n, depth, lines, w, h, kids, span }
      }
      const root = measure(tree, 0)
      const colW = []
      const widths = (m) => {
        colW[m.depth] = Math.max(colW[m.depth] || 0, m.w)
        m.kids.forEach(widths)
      }
      widths(root)
      const colX = []
      colW.forEach((w, i) => (colX[i] = i === 0 ? pad : colX[i - 1] + colW[i - 1] + gapX))
      let branches = 0
      const place = (m, top, parent) => {
        const cy = top + m.span / 2
        // 颜色跟着一级分支走：子孙节点和连线与它所在的一级分支同色
        const branch = m.depth === 0 ? -1 : m.depth === 1 ? branches++ : parent.branch
        const node = { x: colX[m.depth], y: cy - m.h / 2, w: m.w, h: m.h, lines: m.lines, depth: m.depth, text: m.n.text, branch }
        nodes.push(node)
        if (parent) links.push({ x1: parent.x + parent.w, y1: parent.y + parent.h / 2, x2: node.x, y2: cy, branch })
        let y = top + (m.span - (m.kids.reduce((a, k) => a + k.span, 0) + gapY * Math.max(0, m.kids.length - 1))) / 2
        for (const k of m.kids) {
          place(k, y, node)
          y += k.span + gapY
        }
      }
      place(root, pad, null)
      return { nodes, links, width: colX[colX.length - 1] + colW[colW.length - 1] + pad, height: root.span + pad * 2 }
    }

    const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c])
    const BRANCH = ['#b4561a', '#2a7f62', '#3b6ea8', '#8e4ba0', '#b08a1a', '#b23a48']

    /** 布局 → 独立的 SVG 字符串（可以直接下载，也能画成 PNG）。 */
    function mindmapSvg(layout, dark = false) {
      const bg = dark ? '#0b1424' : '#ffffff'
      const fg = dark ? '#e8f0fd' : '#0b1424'
      const colorOf = (n) => (n.branch >= 0 ? BRANCH[n.branch % BRANCH.length] : '#0b1424')
      const parts = [`<svg xmlns="http://www.w3.org/2000/svg" width="${Math.ceil(layout.width)}" height="${Math.ceil(layout.height)}" viewBox="0 0 ${Math.ceil(layout.width)} ${Math.ceil(layout.height)}" font-family="-apple-system,'PingFang SC','Microsoft YaHei',sans-serif" font-size="13">`, `<rect width="100%" height="100%" fill="${bg}"/>`]
      for (const l of layout.links) {
        const mx = (l.x1 + l.x2) / 2
        parts.push(`<path d="M${l.x1.toFixed(1)} ${l.y1.toFixed(1)} C${mx.toFixed(1)} ${l.y1.toFixed(1)} ${mx.toFixed(1)} ${l.y2.toFixed(1)} ${l.x2.toFixed(1)} ${l.y2.toFixed(1)}" fill="none" stroke="${colorOf(l)}" stroke-width="1.5"/>`)
      }
      for (const n of layout.nodes) {
        const c = colorOf(n)
        const filled = n.depth <= 1
        parts.push(`<rect x="${n.x.toFixed(1)}" y="${n.y.toFixed(1)}" width="${n.w.toFixed(1)}" height="${n.h.toFixed(1)}" rx="8" fill="${filled ? c : bg}" stroke="${c}" stroke-width="1.5"/>`)
        n.lines.forEach((ln, i) => parts.push(`<text x="${(n.x + 10).toFixed(1)}" y="${(n.y + 6 + 14 + i * 18).toFixed(1)}" fill="${filled ? '#fff' : fg}">${esc(ln)}</text>`))
      }
      parts.push('</svg>')
      return parts.join('')
    }

    // ---------- 第二种图：流程 / 因果图 ----------
    // 节点是视频里的“步骤 / 原因 / 观点 / 证据 / 结果”，边是先后或因果。按最长路径分层从左到右排（天然是时间顺序），
    // 有环的边（模型画反了）直接丢掉，不会卡死。节点带视频时间，点一下跳到那里。

    const FLOW_KINDS = { step: '步骤', cause: '原因', claim: '观点', evidence: '证据', result: '结果' }
    const FLOW_COLORS = { step: '#3b6ea8', cause: '#b08a1a', claim: '#8e4ba0', evidence: '#2a7f62', result: '#b4561a' }
    const FLOW = { colW: 210, gapX: 70, gapY: 22, pad: 24, textW: 178 }

    /** 整理模型给的图：去重、丢掉指向不存在节点和自己指自己的边、限制数量；少于 2 个节点返回 null。 */
    function normalizeFlow(raw) {
      const nodes = []
      const seen = new Set()
      for (const n of (raw && raw.nodes) || []) {
        const id = String(n?.id ?? '').trim()
        const title = String(n?.title ?? '').trim()
        if (!id || !title || seen.has(id)) continue
        seen.add(id)
        const time = String(n.time || '')
        const ms = time ? parseClock(time) : null
        nodes.push({ id, title: title.slice(0, 60), detail: String(n.detail || '').trim().slice(0, 120), kind: FLOW_KINDS[n.kind] ? n.kind : 'step', ms: ms == null ? -1 : ms })
        if (nodes.length >= 40) break
      }
      const edges = []
      const key = new Set()
      for (const e of (raw && raw.edges) || []) {
        const from = String(e?.from ?? '').trim()
        const to = String(e?.to ?? '').trim()
        const k = `${from}>${to}`
        if (!seen.has(from) || !seen.has(to) || from === to || key.has(k)) continue
        key.add(k)
        edges.push({ from, to, label: String(e.label || '').trim().slice(0, 12) })
        if (edges.length >= 80) break
      }
      return nodes.length >= 2 ? { nodes, edges } : null
    }

    /** 布局：返回 { width, height, nodes: [{ ..., x, y, w, h, lines, dlines }], edges: [{ d, label, lx, ly }] }。 */
    function layoutFlow(graph) {
      const g = normalizeFlow(graph)
      if (!g) return null
      const idx = new Map(g.nodes.map((n, i) => [n.id, i]))
      // 去掉形成环的边（深度优先，遇到指向栈里节点的边就丢）
      const out = g.nodes.map(() => [])
      for (const e of g.edges) out[idx.get(e.from)].push(idx.get(e.to))
      const state = g.nodes.map(() => 0)
      const back = new Set()
      const dfs = (u) => {
        state[u] = 1
        for (const v of out[u]) {
          if (state[v] === 1) back.add(`${u}>${v}`)
          else if (state[v] === 0) dfs(v)
        }
        state[u] = 2
      }
      for (let i = 0; i < g.nodes.length; i++) if (state[i] === 0) dfs(i)
      const edges = g.edges.filter((e) => !back.has(`${idx.get(e.from)}>${idx.get(e.to)}`))
      // 最长路径分层
      const indeg = g.nodes.map(() => 0)
      const adj = g.nodes.map(() => [])
      for (const e of edges) {
        adj[idx.get(e.from)].push(idx.get(e.to))
        indeg[idx.get(e.to)]++
      }
      const rank = g.nodes.map(() => 0)
      const q = indeg.map((d, i) => (d === 0 ? i : -1)).filter((i) => i >= 0)
      while (q.length) {
        const u = q.shift()
        for (const v of adj[u]) {
          rank[v] = Math.max(rank[v], rank[u] + 1)
          if (--indeg[v] === 0) q.push(v)
        }
      }
      // 每层里按“前驱的平均位置”排（减少连线交叉），两遍
      const cols = []
      g.nodes.forEach((n, i) => (cols[rank[i]] ||= []).push(i))
      const preds = g.nodes.map(() => [])
      for (const e of edges) preds[idx.get(e.to)].push(idx.get(e.from))
      const pos = g.nodes.map(() => 0)
      cols.forEach((c) => c.forEach((i, k) => (pos[i] = k)))
      for (let pass = 0; pass < 2; pass++) {
        for (let r = 1; r < cols.length; r++) {
          const bary = (i) => (preds[i].length ? preds[i].reduce((s, p) => s + pos[p], 0) / preds[i].length : pos[i])
          cols[r].sort((a, b) => bary(a) - bary(b) || a - b)
          cols[r].forEach((i, k) => (pos[i] = k))
        }
      }
      // 坐标
      const nodes = []
      const box = new Map()
      let maxH = 0
      const colH = cols.map((c) => {
        let h = 0
        for (const i of c) {
          const n = g.nodes[i]
          const lines = wrapText(n.title, FLOW.textW).slice(0, 3)
          const dlines = n.detail ? wrapText(n.detail, FLOW.textW).slice(0, 2) : []
          const hh = 14 + lines.length * 17 + (dlines.length ? 4 + dlines.length * 15 : 0) + 10
          box.set(i, { lines, dlines, h: hh })
          h += hh + FLOW.gapY
        }
        h -= FLOW.gapY
        maxH = Math.max(maxH, h)
        return h
      })
      cols.forEach((c, r) => {
        let y = FLOW.pad + (maxH - colH[r]) / 2
        for (const i of c) {
          const n = g.nodes[i]
          const b = box.get(i)
          const x = FLOW.pad + r * (FLOW.colW + FLOW.gapX)
          nodes[i] = { ...n, x, y, w: FLOW.colW, h: b.h, lines: b.lines, dlines: b.dlines, rank: r }
          y += b.h + FLOW.gapY
        }
      })
      const links = edges.map((e) => {
        const a = nodes[idx.get(e.from)]
        const b = nodes[idx.get(e.to)]
        const x1 = a.x + a.w
        const y1 = a.y + a.h / 2
        const x2 = b.x
        const y2 = b.y + b.h / 2
        const mx = (x1 + x2) / 2
        return { d: `M${x1.toFixed(1)} ${y1.toFixed(1)} C${mx.toFixed(1)} ${y1.toFixed(1)} ${mx.toFixed(1)} ${y2.toFixed(1)} ${(x2 - 2).toFixed(1)} ${y2.toFixed(1)}`, label: e.label, lx: mx, ly: (y1 + y2) / 2 - 4 }
      })
      return { width: FLOW.pad * 2 + cols.length * FLOW.colW + (cols.length - 1) * FLOW.gapX, height: maxH + FLOW.pad * 2, nodes, edges: links, dropped: back.size }
    }

    /** 流程图 → 独立的 SVG 字符串（可下载、可转 PNG）。节点带 data-ms，页面里点一下可以跳到视频那一刻。 */
    function flowSvg(layout, dark = false) {
      const bg = dark ? '#0b1424' : '#ffffff'
      const fg = dark ? '#e8f0fd' : '#0b1424'
      const mute = dark ? '#a9b3ad' : '#66716c'
      const W = Math.ceil(layout.width)
      const H = Math.ceil(layout.height)
      const parts = [`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" font-family="-apple-system,'PingFang SC','Microsoft YaHei',sans-serif" font-size="13">`, `<defs><marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="${mute}"/></marker></defs>`, `<rect width="100%" height="100%" fill="${bg}"/>`]
      for (const l of layout.edges) {
        parts.push(`<path d="${l.d}" fill="none" stroke="${mute}" stroke-width="1.5" marker-end="url(#ah)"/>`)
        if (l.label) parts.push(`<text x="${l.lx.toFixed(1)}" y="${l.ly.toFixed(1)}" text-anchor="middle" font-size="11" fill="${mute}" stroke="${bg}" stroke-width="3" paint-order="stroke">${esc(l.label)}</text>`)
      }
      for (const n of layout.nodes) {
        const c = FLOW_COLORS[n.kind] || FLOW_COLORS.step
        parts.push(`<g class="fn"${n.ms >= 0 ? ` data-ms="${n.ms}" style="cursor:pointer"` : ''}>`)
        parts.push(`<rect x="${n.x.toFixed(1)}" y="${n.y.toFixed(1)}" width="${n.w}" height="${n.h.toFixed(1)}" rx="9" fill="${bg}" stroke="${c}" stroke-width="1.6"/>`)
        parts.push(`<rect x="${n.x.toFixed(1)}" y="${n.y.toFixed(1)}" width="5" height="${n.h.toFixed(1)}" rx="2.5" fill="${c}"/>`)
        n.lines.forEach((ln, i) => parts.push(`<text x="${(n.x + 14).toFixed(1)}" y="${(n.y + 20 + i * 17).toFixed(1)}" font-weight="700" fill="${fg}">${esc(ln)}</text>`))
        n.dlines.forEach((ln, i) => parts.push(`<text x="${(n.x + 14).toFixed(1)}" y="${(n.y + 20 + n.lines.length * 17 + 4 + 12 + i * 15).toFixed(1)}" font-size="11.5" fill="${mute}">${esc(ln)}</text>`))
        if (n.ms >= 0) parts.push(`<text x="${(n.x + n.w - 8).toFixed(1)}" y="${(n.y + n.h - 7).toFixed(1)}" text-anchor="end" font-size="10.5" fill="${c}">${fmtClock(n.ms)}</text>`)
        parts.push('</g>')
      }
      parts.push('</svg>')
      return parts.join('')
    }


    Object.assign(C, { outlineToTree, wrapText, layoutMindmap, mindmapSvg, normalizeFlow, layoutFlow, flowSvg, FLOW_KINDS })
  }
})()
