// 截图翻译：框选页面上的一块（图片、视频画面、漫画、PDF 预览……），交给视觉模型识字并翻译，
// 把译文盖回原位置，同时给一份对照列表。按需注入，用完移除。
;(() => {
  globalThis.__ccShotStop?.()
  const { mount, h, send, toast } = globalThis.ccUI
  const ui = mount(`
    .shade { inset: 0; cursor: crosshair; background: #0000; }
    .box { border: 2px solid var(--acc); box-shadow: 0 0 0 9999px var(--shade); pointer-events: none; }
    .tip { left: 50%; top: 16px; transform: translateX(-50%); background: var(--fg); color: var(--bg); border-radius: 8px; padding: 6px 14px; }
    .card { background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 10px; box-shadow: 0 8px 30px var(--shade); font: 13px/1.5 system-ui, sans-serif; overflow: hidden; display: flex; flex-direction: column; max-height: 86vh; }
    .bar { display: flex; gap: 6px; align-items: center; padding: 6px 8px; background: var(--soft); flex-wrap: wrap; }
    .bar button { border: 1px solid var(--line); background: var(--card); color: var(--fg); border-radius: 6px; padding: 3px 9px; cursor: pointer; font: inherit; }
    .bar button.on { background: var(--acc); border-color: var(--acc); color: var(--onacc); }
    .bar .sp { flex: 1; }
    .stage { position: relative; overflow: hidden; background: #000; flex: none; }
    .stage canvas { display: block; width: 100%; height: auto; }
    .tb { position: absolute; display: flex; align-items: center; justify-content: center; text-align: center; overflow: hidden; border-radius: 3px; line-height: 1.15; font-weight: 600; word-break: break-word; }
    .busy { padding: 10px 14px; color: var(--mute); }
    .list { overflow: auto; padding: 4px 10px 10px; }
    .row { padding: 5px 0; border-top: 1px solid var(--line); cursor: pointer; }
    .row:first-child { border-top: 0; }
    .row .o { color: var(--mute); font-size: 12px; }
    .row .t { font-weight: 600; }
    .row .o[contenteditable] { outline: none; border-radius: 4px; }
    .row .o[contenteditable]:focus { background: var(--soft); color: var(--fg); }
    .row button { margin-top: 3px; border: 1px solid var(--line); background: var(--card); color: var(--fg); border-radius: 6px; padding: 1px 8px; cursor: pointer; font: inherit; font-size: 12px; }
    .err { padding: 10px 14px; color: var(--bad); }
  `)

  const stop = () => {
    document.removeEventListener('keydown', onKey, true)
    ui.remove()
    globalThis.__ccShotStop = null
  }
  globalThis.__ccShotStop = stop
  let jobId = ''
  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      if (jobId) send({ type: 'cc-ai-cancel', id: jobId })
      stop()
    }
  }
  document.addEventListener('keydown', onKey, true)

  const shade = h('div', { class: 'layer shade' })
  const tip = h('div', { class: 'layer tip', text: '拖动框选要翻译的图片文字区域，Esc 取消' })
  ui.add(shade)
  ui.add(tip)
  let box = null
  let x0 = 0
  let y0 = 0
  const rectOf = (e) => ({ x: Math.min(x0, e.clientX), y: Math.min(y0, e.clientY), w: Math.abs(e.clientX - x0), h: Math.abs(e.clientY - y0) })
  const paint = (r) => (box.style.cssText = `left:${r.x}px;top:${r.y}px;width:${r.w}px;height:${r.h}px`)

  shade.addEventListener('mousedown', (e) => {
    e.preventDefault()
    x0 = e.clientX
    y0 = e.clientY
    tip.remove()
    box = h('div', { class: 'layer box' })
    ui.add(box)
    paint({ x: x0, y: y0, w: 0, h: 0 })
    const move = (ev) => paint(rectOf(ev))
    const up = (ev) => {
      shade.removeEventListener('mousemove', move)
      shade.removeEventListener('mouseup', up)
      const r = rectOf(ev)
      shade.remove()
      box.remove()
      if (r.w < 8 || r.h < 8) return stop()
      begin(r)
    }
    shade.addEventListener('mousemove', move)
    shade.addEventListener('mouseup', up)
  })

  const wait = (ms) => new Promise((ok) => setTimeout(ok, ms))
  const loadImg = (src) =>
    new Promise((ok, bad) => {
      const im = new Image()
      im.onload = () => ok(im)
      im.onerror = () => bad(new Error('截图读取失败'))
      im.src = src
    })

  // 取区域里的平均色当“遮盖底色”，再按亮度挑黑/白字
  function sample(ctx, x, y, w, hh) {
    try {
      const sx = Math.max(0, Math.floor(x))
      const sy = Math.max(0, Math.floor(y))
      const d = ctx.getImageData(sx, sy, Math.max(1, Math.floor(w)), Math.max(1, Math.floor(hh))).data
      let r = 0
      let g = 0
      let b = 0
      let n = 0
      const step = Math.max(1, Math.floor(d.length / 4 / 400)) * 4
      for (let i = 0; i < d.length; i += step) {
        r += d[i]
        g += d[i + 1]
        b += d[i + 2]
        n++
      }
      r = Math.round(r / n)
      g = Math.round(g / n)
      b = Math.round(b / n)
      const lum = 0.299 * r + 0.587 * g + 0.114 * b
      return { bg: `rgb(${r},${g},${b})`, fg: lum > 140 ? '#111' : '#fff' }
    } catch {
      return { bg: '#fff', fg: '#111' }
    }
  }

  // 找一个能放得下译文的最大字号（二分）
  function fit(el, w, hh, max) {
    let lo = 8
    let hi = Math.max(8, Math.min(max, hh))
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2)
      el.style.fontSize = `${mid}px`
      if (el.scrollHeight <= hh + 1 && el.scrollWidth <= w + 1) lo = mid
      else hi = mid - 1
    }
    el.style.fontSize = `${lo}px`
  }

  async function begin(r) {
    await wait(140) // 等遮罩真的从画面上消失再截
    const cap = await send({ type: 'cc-shot', rect: r, vw: innerWidth })
    if (!cap.ok) {
      toast(ui, cap.error || '截图没有完成', true, 3200)
      return setTimeout(stop, 3300)
    }
    const img = await loadImg(cap.dataUrl).catch(() => null)
    if (!img) {
      toast(ui, '截图读取失败', true, 3000)
      return setTimeout(stop, 3100)
    }
    const cw = Math.min(Math.max(r.w, 280), innerWidth - 24)
    const left = Math.max(12, Math.min(r.x, innerWidth - cw - 12))
    const top = Math.max(12, Math.min(r.y, innerHeight - 160))
    const base = document.createElement('canvas')
    base.width = cap.w
    base.height = cap.h
    const bctx = base.getContext('2d', { willReadFrequently: true })
    bctx.drawImage(img, 0, 0, cap.w, cap.h)
    const out = document.createElement('canvas')
    out.width = cap.w
    out.height = cap.h
    const stage = h('div', { class: 'stage' }, out)
    const status = h('div', { class: 'busy', text: '正在识别并翻译图片里的文字……' })
    const list = h('div', { class: 'list' })
    const bCover = h('button', { class: 'on', text: '译图' })
    const bOrig = h('button', { text: '原图' })
    const bCopy = h('button', { text: '复制译文' })
    const bCopyO = h('button', { text: '复制原文' })
    const bNote = h('button', { text: '存为笔记' })
    const bEng = h('button', { text: '识别：视觉模型 ▾', title: '切换识别与翻译方式' })
    const bSave = h('button', { text: '下载译图' })
    const bClose = h('button', { text: '关闭' })
    const bar = h('div', { class: 'bar' }, bCover, bOrig, h('span', { class: 'sp' }), bCopy, bCopyO, bNote, bEng, bSave, bClose)
    const card = h('div', { class: 'layer card' }, bar, stage, status, list)
    card.style.cssText = `left:${left}px;top:${top}px;width:${cw}px`
    ui.add(card)

    let blocks = []
    let showCover = true
    const draw = () => {
      const c = out.getContext('2d')
      c.drawImage(base, 0, 0)
      if (!showCover) return
      for (const b of blocks) {
        if (!b.rect) continue
        const [rx, ry, rw, rh] = b.rect
        const px = rx * cap.w
        const py = ry * cap.h
        const pw = rw * cap.w
        const ph = rh * cap.h
        const col = sample(bctx, px, py, pw, ph)
        c.fillStyle = col.bg
        c.fillRect(px, py, pw, ph)
        // 用一个隐藏元素量字号，再在画布上逐行画
        const probe = h('div', { class: 'tb' })
        probe.style.cssText = `position:absolute;visibility:hidden;width:${pw}px;height:auto;left:0;top:0`
        probe.textContent = b.translation
        stage.appendChild(probe)
        fit(probe, pw, ph, Math.max(10, ph * 0.9))
        const size = parseFloat(probe.style.fontSize) || 12
        probe.remove()
        c.font = `600 ${size}px system-ui, sans-serif`
        c.fillStyle = col.fg
        c.textAlign = 'center'
        c.textBaseline = 'top'
        const lines = wrapLines(c, b.translation, pw)
        const lh = size * 1.15
        let y = py + Math.max(0, (ph - lines.length * lh) / 2)
        for (const ln of lines) {
          c.fillText(ln, px + pw / 2, y, pw)
          y += lh
        }
      }
    }
    // 逐字折行（中日韩没有空格，所以按字符量宽度；英文按词）
    function wrapLines(c, text, maxW) {
      const toks = text.match(/[　-鿿＀-￯]|\S+\s*|\s+/g) || [text]
      const lines = []
      let cur = ''
      for (const t of toks) {
        if (c.measureText(cur + t).width > maxW && cur) {
          lines.push(cur.trimEnd())
          cur = t.trimStart()
        } else cur += t
      }
      if (cur) lines.push(cur.trimEnd())
      return lines
    }
    draw()

    const setMode = (cover) => {
      showCover = cover
      bCover.classList.toggle('on', cover)
      bOrig.classList.toggle('on', !cover)
      draw()
    }
    bCover.onclick = () => setMode(true)
    bOrig.onclick = () => setMode(false)
    bClose.onclick = () => {
      if (jobId && !blocks.length) send({ type: 'cc-ai-cancel', id: jobId })
      stop()
    }
    const copyText = async (t) => {
      try {
        await navigator.clipboard.writeText(t)
        toast(ui, '已复制', false, 1200)
      } catch {
        toast(ui, '复制失败', true, 1800)
      }
    }
    bCopy.onclick = () => blocks.length && copyText(blocks.map((b) => b.translation).join('\n'))
    bCopyO.onclick = () => blocks.length && copyText(blocks.map((b) => b.text).join('\n'))
    // ---- 识别方式：一步（视觉模型直接给译文） / 两步（视觉模型只管识字，译文交给翻译模型，用上术语库和专家） ----
    const store = (globalThis.browser || globalThis.chrome).storage.local
    let engine = 'one'
    try {
      const got = await store.get('shotEngine')
      if (got && got.shotEngine === 'two') engine = 'two'
    } catch {
      /* 读不到就用默认 */
    }
    const engLabel = () => (engine === 'two' ? '识别：视觉识字 + 翻译模型 ▾' : '识别：视觉模型 ▾')
    bEng.textContent = engLabel()
    // 提交一个后台任务并等结果
    const runAi = async (req) => {
      const sub = await send({ type: 'cc-ai-run', req })
      if (!sub.ok) throw new Error(sub.error || '提交失败')
      for (let n = 0; n < 240; n++) {
        await wait(500)
        if (!globalThis.__ccShotStop) throw new Error('已取消')
        const j = await send({ type: 'cc-ai-job', id: sub.id })
        if (!j.ok) throw new Error(j.error || '任务失败')
        if (j.state === 'error') throw new Error(j.error || '翻译失败')
        if (j.state === 'done') return j.result || {}
      }
      throw new Error('翻译超时')
    }
    const retrans = async (texts) => {
      const res = await runAi({ kind: 'webtrans', lang: cap.lang, title: document.title, context: location.hostname, lines: texts.map((t, i) => ({ t: i, text: t })) })
      const out = new Map((res.lines || []).map((l) => [l.t, l.text]))
      return texts.map((t, i) => out.get(i) || '')
    }
    const renderList = () => {
      list.replaceChildren()
      blocks.forEach((b, i) => {
        const o = h('div', { class: 'o', text: b.text || '' })
        o.contentEditable = 'true'
        o.title = '识别不准？直接改这里的字，再点“重译”'
        const t = h('div', { class: 't', text: b.translation })
        const re = h('button', { text: '重译这段' })
        re.onclick = async (ev) => {
          ev.stopPropagation()
          const src = o.textContent.trim()
          if (!src) return
          re.disabled = true
          re.textContent = '翻译中…'
          try {
            const [tr] = await retrans([src])
            if (tr) {
              b.text = src
              b.translation = tr
              t.textContent = tr
              draw()
            } else toast(ui, '没有得到译文', true, 1800)
          } catch (e) {
            toast(ui, e.message || '重译失败', true, 2400)
          }
          re.disabled = false
          re.textContent = '重译这段'
        }
        const row = h('div', { class: 'row' }, o, t, re)
        row.onclick = (ev) => {
          if (ev.target === o || ev.target === re) return
          copyText(b.translation)
        }
        list.appendChild(row)
      })
    }
    bEng.onclick = async () => {
      engine = engine === 'two' ? 'one' : 'two'
      bEng.textContent = engLabel()
      try {
        await store.set({ shotEngine: engine })
      } catch {
        /* 记不住也能用 */
      }
      if (engine === 'two' && blocks.length) {
        bEng.disabled = true
        try {
          const trs = await retrans(blocks.map((b) => b.text))
          blocks.forEach((b, i) => trs[i] && (b.translation = trs[i]))
          renderList()
          draw()
        } catch (e) {
          toast(ui, e.message || '重新翻译失败', true, 2400)
        }
        bEng.disabled = false
      } else toast(ui, '下一次框选起按这个方式识别', false, 1800)
    }
    bNote.onclick = async () => {
      if (!blocks.length) return
      const text = blocks.map((b) => `${b.text}\n→ ${b.translation}`).join('\n\n')
      const note = { id: `n${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`, key: `shot:${location.href}`, url: location.href, title: document.title.slice(0, 200), owner: '截图翻译', t: -1, text: text.slice(0, 6000), kind: 'quote', created: Date.now() }
      try {
        const got = await store.get('vsumNotes')
        const cur = Array.isArray(got && got.vsumNotes) ? got.vsumNotes : []
        await store.set({ vsumNotes: [...cur, note].slice(-2000) })
        bNote.textContent = '已存笔记'
        toast(ui, '已存进笔记（视频摘要面板 → 笔记 里能看到）', false, 2200)
      } catch {
        toast(ui, '保存失败', true, 1800)
      }
    }
    bSave.onclick = () => {
      out.toBlob((bl) => {
        if (!bl) return
        const a = document.createElement('a')
        a.href = URL.createObjectURL(bl)
        a.download = `截图翻译-${Date.now()}.jpg`
        a.click()
        setTimeout(() => URL.revokeObjectURL(a.href), 4000)
      }, 'image/jpeg', 0.92)
    }

    const sub = await send({ type: 'cc-ai-run', req: { kind: 'imgtrans', image: cap.dataUrl, lang: cap.lang, title: document.title } })
    if (!sub.ok) return fail(sub.error || '提交失败')
    jobId = sub.id
    for (let n = 0; n < 240; n++) {
      await wait(500)
      if (!globalThis.__ccShotStop) return
      const j = await send({ type: 'cc-ai-job', id: jobId })
      if (!j.ok) return fail(j.error)
      if (j.state === 'error') return fail(j.error || '翻译失败')
      if (j.state === 'done') {
        blocks = (j.result?.blocks || []).filter((b) => b && b.translation)
        jobId = ''
        if (!blocks.length) return fail('没有识别到文字')
        if (engine === 'two') {
          status.textContent = '正在用翻译模型重新翻译……'
          try {
            const trs = await retrans(blocks.map((b) => b.text))
            blocks.forEach((b, i) => trs[i] && (b.translation = trs[i]))
          } catch {
            /* 失败就保留视觉模型给的译文 */
          }
        }
        status.remove()
        draw()
        renderList()
        return
      }
    }
    fail('翻译超时')

    function fail(msg) {
      jobId = ''
      status.className = 'err'
      status.textContent = msg
    }
  }
})()
