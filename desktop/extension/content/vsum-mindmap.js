// 视频摘要面板：导图页（全屏查看、导出 PNG）。
// 由 vsum.js 调用工厂函数：用到的面板状态和工具从参数传入，返回面板要用的函数。
;(() => {
  const T = (globalThis.ccVsumTabs ||= {})
  T.mindmap = ({
    S, V, actsBar, copy, h, isDark, isVideo, onKey, render, saveFile, seek, task, toast, ui
  }) => {
      async function savePng(svg, name) {
      const w = Number(svg.match(/width="([\d.]+)"/)?.[1]) || 800
      const hh = Number(svg.match(/height="([\d.]+)"/)?.[1]) || 600
      const img = new Image()
      img.src = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }))
      await img.decode().catch(() => {})
      const c = document.createElement('canvas')
      c.width = w * 2
      c.height = hh * 2
      const g = c.getContext('2d')
      g.scale(2, 2)
      g.drawImage(img, 0, 0, w, hh)
      URL.revokeObjectURL(img.src)
      c.toBlob((b) => {
        if (!b) return toast(ui, '图片太大，导出失败，请改用 SVG', true)
        const a = document.createElement('a')
        a.href = URL.createObjectURL(b)
        a.download = name
        a.click()
        setTimeout(() => URL.revokeObjectURL(a.href), 1000)
      }, 'image/png')
    }

    /** 全屏查看导图：滚轮缩放（以鼠标为中心）、拖动平移、双击适应窗口、+ / − / 0 键。 */
    function openMapViewer(svgText, title = '思维导图') {
      const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml').documentElement
      const W = Number(doc.getAttribute('width')) || 800
      const H = Number(doc.getAttribute('height')) || 600
      doc.removeAttribute('style')
      const stage = h('div', { class: 'stage' })
      stage.append(doc)
      doc.setAttribute('width', W)
      doc.setAttribute('height', H)
      doc.style.width = `${W}px`
      doc.style.height = `${H}px`
      // 流程图：点节点跳到视频里说到它的那一刻（拖动平移时不算点击）
      let moved = false
      // 舞台按下时抓了指针，点击的目标会变成舞台本身：按位置找到下面的节点
      stage.addEventListener('click', (e) => {
        const root = stage.getRootNode()
        const el = (root.elementFromPoint ? root.elementFromPoint(e.clientX, e.clientY) : null) || e.target
        const ms = Number(el?.closest?.('.fn')?.getAttribute('data-ms'))
        if (!moved && Number.isFinite(ms) && ms >= 0 && isVideo?.()) seek(ms)
      })
      let k = 1
      let x = 0
      let y = 0
      const apply = () => (doc.style.transform = `translate(${x}px, ${y}px) scale(${k})`)
      const fit = () => {
        const r = stage.getBoundingClientRect()
        k = Math.min(r.width / W, r.height / H, 1.5) * 0.96
        x = (r.width - W * k) / 2
        y = (r.height - H * k) / 2
        apply()
      }
      const zoomAt = (f, cx, cy) => {
        const nk = Math.min(6, Math.max(0.1, k * f))
        x = cx - ((cx - x) / k) * nk
        y = cy - ((cy - y) / k) * nk
        k = nk
        apply()
      }
      const center = (f) => {
        const r = stage.getBoundingClientRect()
        zoomAt(f, r.width / 2, r.height / 2)
      }
      stage.addEventListener('wheel', (e) => {
        e.preventDefault()
        const r = stage.getBoundingClientRect()
        zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - r.left, e.clientY - r.top)
      }, { passive: false })
      const pts = new Map()
      let pinch = 0
      stage.addEventListener('pointerdown', (e) => {
        moved = false
        stage.setPointerCapture(e.pointerId)
        pts.set(e.pointerId, { x: e.clientX, y: e.clientY })
        stage.classList.add('drag')
        pinch = 0
      })
      stage.addEventListener('pointermove', (e) => {
        const p = pts.get(e.pointerId)
        if (!p) return
        const dx = e.clientX - p.x
        const dy = e.clientY - p.y
        if (Math.abs(dx) + Math.abs(dy) > 2) moved = true
        p.x = e.clientX
        p.y = e.clientY
        if (pts.size === 2) {
          const [a, b] = [...pts.values()]
          const d = Math.hypot(a.x - b.x, a.y - b.y)
          if (pinch) {
            const r = stage.getBoundingClientRect()
            zoomAt(d / pinch, (a.x + b.x) / 2 - r.left, (a.y + b.y) / 2 - r.top)
          }
          pinch = d
        } else {
          x += dx
          y += dy
          apply()
        }
      })
      const up = (e) => {
        pts.delete(e.pointerId)
        pinch = 0
        if (!pts.size) stage.classList.remove('drag')
      }
      stage.addEventListener('pointerup', up)
      stage.addEventListener('pointercancel', up)
      stage.addEventListener('dblclick', fit)
      const view = h('div', { class: 'layer mapview', role: 'dialog', 'aria-label': '思维导图' },
        h('div', { class: 'bar2' },
          h('b', { text: S.data?.title || title }),
          h('button', { class: 'sec', text: '−', title: '缩小', onclick: () => center(1 / 1.25) }),
          h('button', { class: 'sec', text: '+', title: '放大', onclick: () => center(1.25) }),
          h('button', { class: 'sec', text: '适应窗口', onclick: fit }),
          h('button', { class: 'sec', text: '100%', onclick: () => { k = 1; x = 8; y = 8; apply() } }),
          h('button', { class: 'sec', text: '保存 SVG', onclick: () => saveFile(V.fileName(S.data.title, 'svg'), svgText, 'image/svg+xml') }),
          h('button', { class: 'sec', text: '保存 PNG', onclick: () => savePng(svgText, V.fileName(S.data.title, 'png')) }),
          h('button', { class: 'sec', text: '✕', title: '关闭（Esc）', 'aria-label': '关闭', onclick: close }),
        ),
        stage,
        h('div', { class: 'hint', text: `滚轮缩放 · 拖动平移 · 双击适应窗口 · 键盘 + − 0 · Esc 关闭${title === '流程图' ? ' · 点节点跳到视频' : ''}` }),
      )
      function onKey(e) {
        if (e.key === 'Escape') close()
        else if (e.key === '+' || e.key === '=') center(1.25)
        else if (e.key === '-') center(1 / 1.25)
        else if (e.key === '0') fit()
        else return
        e.stopPropagation()
      }
      function close() {
        document.removeEventListener('keydown', onKey, true)
        view.remove()
      }
      document.addEventListener('keydown', onKey, true)
      ui.add(view)
      requestAnimationFrame(fit)
    }

    /** 流程 / 因果图：模型把视频里的步骤、原因、观点、证据、结果连成一张从左到右的图，点节点跳到视频那一刻。 */
    function tabFlow() {
      const f = S.res.flow
      const gen = h('button', { class: f ? 'sec' : '', text: f ? '重新生成' : '生成流程图', disabled: S.busy, onclick: () => task('flowgraph', {}, (x) => (S.res.flow = x.flow)) })
      const lay = f ? V.layoutFlow(f) : null
      if (!lay) return h('div', { class: 'sm' }, h('div', { class: 'empty' }, h('b', { text: f ? '这张图没法画' : '还没有流程图' }), h('p', { class: 'mute', text: '把视频里的步骤、原因、观点、证据、结果连成从左到右的流程 / 因果图，每个节点带视频时间，点一下跳过去。适合教程、讲解、论证。' }), gen))
      const dark = isDark()
      const mk = (d) => V.flowSvg(lay, d)
      const view = h('div', { class: 'svgbox', title: '点击全屏查看，可缩放、拖动；点节点跳到视频', onclick: () => openMapViewer(mk(dark), '流程图') })
      view.append(new DOMParser().parseFromString(mk(dark), 'image/svg+xml').documentElement, h('div', { class: 'zoomhint', text: '点击缩略图全屏查看' }))
      const kinds = Object.entries(V.FLOW_KINDS).map(([k, n]) => `${n}`).join(' · ')
      return h('div', { class: 'sm' },
        h('div', { class: 'crow' }, h('span', { class: 'mute', text: `流程图（${lay.nodes.length} 个节点）` }), h('span', { class: 'grow' }), gen),
        view,
        h('p', { class: 'mute', text: `节点类型：${kinds}。${lay.dropped ? `有 ${lay.dropped} 条连线形成了回环，已忽略。` : ''}` }),
        actsBar(
          h('button', { text: '全屏查看 / 缩放', onclick: () => openMapViewer(mk(dark), '流程图') }),
          h('button', { class: 'sec', text: '保存 SVG', onclick: () => saveFile(V.fileName(S.data.title, 'svg').replace(/\.svg$/, '-流程图.svg'), mk(false), 'image/svg+xml') }),
          h('button', { class: 'sec', text: '保存 PNG', onclick: () => savePng(mk(false), V.fileName(S.data.title, 'png').replace(/\.png$/, '-流程图.png')) })),
      )
    }

    function tabMindmap() {
      // 导图页里两种图：层级导图 / 流程图
      const sw = h('div', { class: 'crow mapsw' },
        ...[['mind', '思维导图'], ['flow', '流程图']].map(([id, name]) => h('button', { class: (S.mapView || 'mind') === id ? '' : 'sec', text: name, onclick: () => ((S.mapView = id), render()) })))
      const body = (S.mapView || 'mind') === 'flow' ? tabFlow() : tabMindmapOnly()
      return h('div', { class: 'sm' }, sw, body)
    }

    function tabMindmapOnly() {
      const o = S.res.outline
      const gen = h('button', { class: o ? 'sec' : '', text: o ? '重新生成' : '生成思维导图', disabled: S.busy, onclick: () => task('mindmap', {}, (x) => (S.res.outline = x.outline)) })
      if (!o) return h('div', { class: 'sm' }, h('div', { class: 'empty' }, h('b', { text: '还没有思维导图' }), h('p', { class: 'mute', text: '生成层级大纲（最多 4 层，带具体细节），并画成可缩放、可导出的导图。' }), gen))
      const dark = isDark()
      const mk = (d) => V.mindmapSvg(V.layoutMindmap(V.outlineToTree(o, S.data.title)), d)
      const view = h('div', { class: 'svgbox', title: '点击全屏查看，可缩放、拖动', onclick: () => openMapViewer(mk(dark)) })
      view.append(new DOMParser().parseFromString(mk(dark), 'image/svg+xml').documentElement, h('div', { class: 'zoomhint', text: '点击缩略图全屏查看' }))
      return h('div', { class: 'sm' },
        h('div', { class: 'crow' }, h('span', { class: 'mute', text: '思维导图' }), h('span', { class: 'grow' }), gen),
        view,
        h('details', { class: 'acc', open: true }, h('summary', { text: '大纲文本' }), h('pre', { text: o })),
        actsBar(
          h('button', { text: '全屏查看 / 缩放', onclick: () => openMapViewer(mk(dark)) }),
          h('button', { class: 'sec', text: '保存 SVG', onclick: () => saveFile(V.fileName(S.data.title, 'svg'), mk(false), 'image/svg+xml') }),
          h('button', { class: 'sec', text: '保存 PNG', onclick: () => savePng(mk(false), V.fileName(S.data.title, 'png')) }),
          h('button', { class: 'sec', text: '复制大纲', onclick: () => copy(o, '大纲') })),
      )
    }

    return { tabMindmap }
  }
})()
