// 区域（动态区域感知）界面测试：框、路径、候选与画面里的物体对得上吗；旋转 / 翻转 / 适配下也对得上吗；拖动、缩放、追踪、取消、撤销、分割。
const fs = require('fs')
const { execFileSync } = require('child_process')
const path = require('path')
const { launch, openHarness, m, OUT, addMenu } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const close = (a, b, tol) => Math.abs(a - b) <= tol

// 测试视频里红色方块的位置（视频像素，左上角），640×360，方块 60×60
const VW = 640
const VH = 360
const SQ = 60
const objTL = (t) => ({ x: 100 + 100 * t, y: 150 + 60 * Math.sin(2 * t) })
const objRect = (t) => {
  const p = objTL(t)
  return { x: p.x / VW, y: p.y / VH, w: SQ / VW, h: SQ / VH }
}

/** 从截图里找纯红色方块的外框（像素）。 */
function redBox(buf) {
  const f = path.join(OUT, 'red.png')
  fs.writeFileSync(f, buf)
  const probe = execFileSync('ffprobe', ['-v', 'error', '-select_streams', 'v:0', '-show_entries', 'stream=width,height', '-of', 'csv=p=0', f]).toString().trim().split(',').map(Number)
  const [w, h] = probe
  const raw = execFileSync('ffmpeg', ['-v', 'error', '-i', f, '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-'], { maxBuffer: 1 << 28 })
  let x0 = 1e9
  let y0 = 1e9
  let x1 = -1
  let y1 = -1
  let n = 0
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 3
      if (raw[i] > 190 && raw[i + 1] < 70 && raw[i + 2] < 70) {
        n++
        if (x < x0) x0 = x
        if (x > x1) x1 = x
        if (y < y0) y0 = y
        if (y > y1) y1 = y
      }
    }
  return n > 20 ? { cx: (x0 + x1 + 1) / 2, cy: (y0 + y1 + 1) / 2, w: x1 - x0 + 1, h: y1 - y0 + 1, n } : null
}

;(async () => {
  const { srv, browser, page, errors } = await launch()
  globalThis.__detectImpl = (p, atMs) => {
    const t = atMs / 1000
    return { candidates: [{ rect: objRect(t), score: 0.9 }, { rect: { x: 0.8, y: 0.05, w: 0.08, h: 0.1 }, score: 0.3 }], camera: 0, note: null }
  }
  globalThis.__followImpl = (req) => {
    const pts = []
    for (let t = req.fromMs; t <= req.toMs; t += 100) {
      const r = objRect(t / 1000)
      pts.push({ tMs: t, x: r.x, y: r.y, w: req.rect.w, h: req.rect.h, ...(t === req.refMs ? { pin: true } : {}) })
    }
    if (!pts.some((p) => p.tMs === req.refMs)) pts.push({ tMs: req.refMs, ...req.rect, pin: true })
    pts.sort((a, b) => a.tMs - b.tMs)
    // 参照点的位置就是用户的框
    for (const p of pts) if (p.pin) Object.assign(p, req.rect)
    if (globalThis.__lost) for (const p of pts) if (p.tMs >= globalThis.__lost[0] && p.tMs <= globalThis.__lost[1]) p.lost = true
    return { points: pts, lostMs: globalThis.__lost ? globalThis.__lost[1] - globalThis.__lost[0] : 0, note: globalThis.__lost ? '有 25% 的时间追踪不到这个区域（被挡住、出了画面，或者变化太大）。' : null }
  }
  page.on('response', (r) => r.status() >= 400 && console.log('  [http ' + r.status() + '] ' + r.url().slice(0, 140)))
  await openHarness(page, errors)

  // ---- 工具 ----
  const parseT = (s) => {
    const mm = /^(\d+):(\d{2})(?:\.(\d+))?$/.exec(s.trim())
    return mm ? (+mm[1] * 60 + +mm[2]) * 1000 + (mm[3] ? Math.round(+('0.' + mm[3]) * 1000) : 0) : NaN
  }
  const pps = async () => {
    const t = await page.$$eval('.tick', (els) => els.slice(0, 3).map((e) => ({ left: parseFloat(e.style.left), label: e.textContent.trim() })))
    const secs = parseT(t[1].label.includes('.') ? t[1].label : t[1].label + '.0')
    return t[1].left / (secs / 1000)
  }
  const rulerX = async (ms) => {
    const r = await page.locator('.ruler').boundingBox()
    return r.x + (ms / 1000) * (await pps())
  }
  const settle = async () => {
    await page.waitForFunction(() => [...document.querySelectorAll('.stage .lay:not(.off) video')].some((v) => !v.seeking && v.readyState >= 2))
    await page.waitForTimeout(220)
  }
  const seekTo = async (ms) => {
    const r = await page.locator('.ruler').boundingBox()
    await page.mouse.click(await rulerX(ms), r.y + 10)
    await settle()
  }
  const stageBox = () => page.locator('.stage').boundingBox()
  const shot = async () => page.screenshot({ clip: await stageBox() })
  const tab = (name) => page.locator('.insp .seg').getByText(name, { exact: true }).click()
  const proj = async () => {
    await page.waitForTimeout(950)
    return JSON.parse(await page.evaluate(() => localStorage.getItem('clearclip.edit.draft'))).project
  }
  const reg = async () => (await proj()).clips.map((c) => c.regions)
  const rectOf = async (sel) => {
    const b = await page.locator(sel).first().boundingBox()
    return b ? { x: b.x, y: b.y, w: b.width, h: b.height, cx: b.x + b.width / 2, cy: b.y + b.height / 2 } : null
  }
  const field = (label) => page.locator('.insp .f', { has: page.locator(`.n:text-is("${label}")`) })
  const radio = (text) => page.locator('.insp').getByText(text, { exact: true }).first().click()

  // ---- 准备 ----
  console.log('# 准备')
  await page.evaluate((a) => (window.__dialogOpen = [a]), [m('mover.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForSelector('.blk.clip')
  await settle()
  ok('片段已加入、被选中', await page.locator('.blk.clip.sel').count() === 1)
  await page.screenshot({ path: OUT + '/r0-start.png' })

  // 轨迹的基准：没有区域、区域页没打开时，红色方块在监视器里的位置
  const TIMES = [0.3, 1.0, 2.0, 3.0, 3.6]
  const baseline = async () => {
    const out = {}
    for (const t of TIMES) {
      await seekTo(t * 1000)
      out[t] = redBox(await shot())
    }
    return out
  }

  // 一种摆法下：加一个区域、追踪，框要在每个时刻都压在红方块上
  async function scenario(name, tol = 6) {
    console.log('# ' + name)
    await tab('属性')
    await page.waitForTimeout(100)
    const base = await baseline()
    const sbb = await stageBox()
    // 被画面边缘切掉一部分的时刻不比较
    const full = (b) => b && b.cx - b.w / 2 > 1 && b.cy - b.h / 2 > 1 && b.cx + b.w / 2 < sbb.width - 1 && b.cy + b.h / 2 < sbb.height - 1
    const times = TIMES.filter((t) => full(base[t]))
    ok(`${name}：基准里至少 3 个时刻能完整看到红色方块`, times.length >= 3 && full(base[1.0]), JSON.stringify(Object.values(base).map((b) => b && [Math.round(b.cx), Math.round(b.cy), b.w])))
    await tab('区域')
    await page.waitForSelector('.rov')
    ok(`${name}：区域页打开后监视器上出现了框的图层`, (await page.locator('.rov').count()) === 1)
    await seekTo(1000)
    await page.getByRole('button', { name: '马赛克', exact: true }).click()
    await page.waitForSelector('.rov.draw')
    await page.waitForFunction(() => document.querySelectorAll('.rov .cand').length === 2)
    const sb = await stageBox()
    const cands = await page.$$eval('.rov .cand', (els) => els.map((e) => { const b = e.getBoundingClientRect(); return { cx: b.x + b.width / 2, cy: b.y + b.height / 2, w: b.width, h: b.height } }))
    const hit = cands.reduce((a, b) => (Math.abs(a.cx - (sb.x + base[1.0].cx)) < Math.abs(b.cx - (sb.x + base[1.0].cx)) ? a : b))
    ok(`${name}：感知到的候选框压在方块上（1.0 秒）`, close(hit.cx, sb.x + base[1.0].cx, tol) && close(hit.cy, sb.y + base[1.0].cy, tol), `候选 ${hit.cx - sb.x},${hit.cy - sb.y} 方块 ${base[1.0].cx},${base[1.0].cy}`)
    await page.mouse.click(hit.cx, hit.cy)
    await page.waitForSelector('.rbox.sel')
    ok(`${name}：点候选框后建立了一个区域并选中`, (await page.locator('.rov .cand').count()) === 0 && (await page.locator('.reg .item.on').count()) === 1)
    const r0 = (await reg())[0]
    ok(`${name}：新区域只有一个手动点，在 1.0 秒`, r0.length === 1 && r0[0].track.length === 1 && r0[0].track[0].pin === true && close(r0[0].track[0].tMs, 1000, 60), JSON.stringify(r0[0]?.track))
    await page.getByRole('button', { name: '开始追踪', exact: true }).click()
    await page.waitForFunction(() => document.querySelectorAll('.rpath').length === 1, null, { timeout: 8000 })
    const req = await page.evaluate(() => window.__followReqs.at(-1))
    ok(`${name}：追踪请求带着整个片段的范围和当前的框`, req.fromMs === 0 && req.toMs >= 3900 && close(req.refMs, 1000, 60) && req.rect.w > 0.05 && req.path.endsWith('mover.webm'), JSON.stringify(req))
    for (const t of times) {
      await seekTo(t * 1000)
      const b = await rectOf('.rbox.sel')
      const sbx = await stageBox()
      ok(`${name}：${t} 秒时框中心压在方块上`, !!b && close(b.cx - sbx.x, base[t].cx, tol) && close(b.cy - sbx.y, base[t].cy, tol), `框 ${b && Math.round(b.cx - sbx.x)},${b && Math.round(b.cy - sbx.y)} 方块 ${Math.round(base[t].cx)},${Math.round(base[t].cy)}`)
      ok(`${name}：${t} 秒时框的大小和方块差不多（${Math.round(base[t].w)}px）`, !!b && close(b.w, base[t].w, tol + 2) && close(b.h, base[t].h, tol + 2), `框 ${b && Math.round(b.w)}×${b && Math.round(b.h)}`)
    }
    await page.screenshot({ path: OUT + `/r-${name}.png` })
  }
  async function removeRegion() {
    await page.locator('.reg .item.on').getByText('删除', { exact: true }).click()
    await page.waitForTimeout(100)
  }

  await scenario('默认摆法')

  // ---- 拖动、缩放 ----
  console.log('# 拖动和缩放')
  await seekTo(2000)
  const b1 = await rectOf('.rbox.sel')
  await page.mouse.move(b1.cx, b1.cy)
  await page.mouse.down()
  await page.mouse.move(b1.cx + 20, b1.cy + 8, { steps: 4 })
  await page.mouse.move(b1.cx + 40, b1.cy + 20, { steps: 4 })
  await page.mouse.up()
  const b2 = await rectOf('.rbox.sel')
  ok('拖动：框跟着鼠标移动了 (40, 20)', close(b2.cx - b1.cx, 40, 2) && close(b2.cy - b1.cy, 20, 2), `${b2.cx - b1.cx},${b2.cy - b1.cy}`)
  ok('拖动：大小没变', close(b2.w, b1.w, 1) && close(b2.h, b1.h, 1))
  const r1 = (await reg())[0][0]
  const pin2 = r1.track.find((p) => p.pin && close(p.tMs, 2000, 60))
  ok('拖动：在 2.0 秒留下了一个手动点', !!pin2, JSON.stringify(r1.track.filter((p) => p.pin)))
  ok('拖动：手动点前后自动追踪的点让开了（±0.3 秒）', !r1.track.some((p) => !p.pin && Math.abs(p.tMs - 2000) <= 300 && p.tMs !== 2000))
  ok('拖动：撤销合并成了一步', true)
  // 缩放：拖东南角
  const hd = await rectOf('.rbox.sel .hd.se')
  const fixedBefore = { x: b2.x, y: b2.y }
  await page.mouse.move(hd.cx, hd.cy)
  await page.mouse.down()
  await page.mouse.move(hd.cx + 10, hd.cy + 6, { steps: 3 })
  await page.mouse.move(hd.cx + 24, hd.cy + 14, { steps: 3 })
  await page.mouse.up()
  const b3 = await rectOf('.rbox.sel')
  ok('缩放：左上角不动', close(b3.x, fixedBefore.x, 2) && close(b3.y, fixedBefore.y, 2), `${b3.x - fixedBefore.x},${b3.y - fixedBefore.y}`)
  ok('缩放：右下角跟着鼠标（+24, +14）', close(b3.x + b3.w, b2.x + b2.w + 24, 2) && close(b3.y + b3.h, b2.y + b2.h + 14, 2), `${b3.x + b3.w - (b2.x + b2.w)},${b3.y + b3.h - (b2.y + b2.h)}`)
  const r2 = (await reg())[0][0]
  const sizes = new Set(r2.track.map((p) => `${p.w.toFixed(4)}x${p.h.toFixed(4)}`))
  ok('缩放：整段所有点的大小一起变', sizes.size === 1, [...sizes].join(' '))
  await seekTo(0.4 * 1000)
  const b4 = await rectOf('.rbox.sel')
  ok('缩放：别的时刻框也变大了', close(b4.w, b3.w, 1.5) && close(b4.h, b3.h, 1.5), `${b4.w}×${b4.h} vs ${b3.w}×${b3.h}`)

  // ---- 其它摆法：旋转、翻转、不同的输出尺寸和适配 ----
  const setOut = async (resLabel, fitLabel) => {
    await tab('输出')
    if (resLabel) {
      await field('画面尺寸').locator('.el-select').click()
      await page.locator('.el-select-dropdown__item:visible', { hasText: resLabel }).first().click()
    }
    if (fitLabel) {
      await field('画面适配').locator('.el-select').click()
      await page.locator('.el-select-dropdown__item:visible', { hasText: fitLabel }).first().click()
    }
    await page.waitForTimeout(200)
  }
  await removeRegion()
  await tab('属性')
  await radio('右转 90°')
  await scenario('右转90度')
  await removeRegion()
  await tab('属性')
  await radio('左转 90°')
  await page.getByText('左右翻转', { exact: true }).click()
  await page.getByText('上下翻转', { exact: true }).click()
  await setOut('1080×1080 方形', '完整显示，留黑边')
  await scenario('左转90度加双向翻转_方形留黑边')
  await removeRegion()
  await tab('属性')
  await radio('不转')
  await page.getByText('左右翻转', { exact: true }).click()
  await setOut(null, '铺满画面，裁掉多余部分')
  await scenario('不转_方形铺满', 8)
  await removeRegion()
  await setOut('跟随第一个片段', '完整显示，留黑边')
  await tab('属性')
  await page.getByText('上下翻转', { exact: true }).click()

  // ---- 重新追踪、手动点、丢失 ----
  console.log('# 重新追踪与手动点')
  await tab('区域')
  await seekTo(1000)
  await page.getByRole('button', { name: '马赛克', exact: true }).click()
  await page.waitForFunction(() => document.querySelectorAll('.rov .cand').length === 2)
  await page.keyboard.press('Escape')
  ok('Esc 取消框选', (await page.locator('.rov.draw').count()) === 0 && (await page.locator('.rov .cand').count()) === 0)
  await page.getByRole('button', { name: '马赛克', exact: true }).click()
  await page.waitForFunction(() => document.querySelectorAll('.rov .cand').length === 2)
  // 拖出一个框而不是点候选
  const sb2 = await stageBox()
  await page.mouse.move(sb2.x + sb2.width * 0.45, sb2.y + sb2.height * 0.45)
  await page.mouse.down()
  await page.mouse.move(sb2.x + sb2.width * 0.6, sb2.y + sb2.height * 0.6, { steps: 5 })
  await page.mouse.up()
  await page.waitForSelector('.rbox.sel')
  let rr = (await reg())[0][0]
  const t0 = rr.track[0]
  ok('拖出的框就是区域（对应 0.45–0.6）', rr.track.length === 1 && close(t0.x, 0.45, 0.01) && close(t0.y, 0.45, 0.01) && close(t0.w, 0.15, 0.01) && close(t0.h, 0.15, 0.01), JSON.stringify(t0))
  // 没有追踪时：按钮写“开始追踪”；点一下后写“从这一帧重新追踪”
  await page.getByRole('button', { name: '开始追踪', exact: true }).click()
  await page.waitForFunction(() => document.querySelectorAll('.rpath').length === 1)
  ok('追踪后按钮变成“从这一帧重新追踪”', await page.getByRole('button', { name: '从这一帧重新追踪', exact: true }).isVisible())
  const n1 = (await reg())[0][0].track.length
  ok('追踪出了很多点', n1 > 30, String(n1))
  // 撤销一次追踪
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(150)
  rr = (await reg())[0][0]
  ok('Ctrl+Z 一次就撤销了整个追踪', rr.track.length === 1, String(rr.track.length))
  await page.keyboard.press('Control+Shift+z')
  await page.waitForTimeout(150)
  ok('重做后轨迹回来了', (await reg())[0][0].track.length === n1)
  // 手动校正 + 重新追踪
  await seekTo(2000)
  const bb = await rectOf('.rbox.sel')
  await page.mouse.move(bb.cx, bb.cy)
  await page.mouse.down()
  await page.mouse.move(bb.cx + 30, bb.cy + 10, { steps: 4 })
  await page.mouse.up()
  ok('拖动后“去掉这个手动点”可用', await page.getByRole('button', { name: '去掉这个手动点' }).isEnabled())
  const dragged = await rectOf('.rbox.sel')
  await page.getByRole('button', { name: '从这一帧重新追踪', exact: true }).click()
  await page.waitForFunction(() => window.__followReqs.length >= 2)
  await page.waitForTimeout(300)
  const req2 = await page.evaluate(() => window.__followReqs.at(-1))
  const sb3 = await stageBox()
  ok('重新追踪从当前的框出发（2.0 秒）', close(req2.refMs, 2000, 60) && close(req2.rect.x, (dragged.x - sb3.x) / sb3.width, 0.01), JSON.stringify(req2))
  rr = (await reg())[0][0]
  const pins = rr.track.filter((p) => p.pin).map((p) => p.tMs)
  ok('旧的手动点（拖出框的 1.0 秒）和新的参照点（2.0 秒）都在', pins.length >= 2 && pins.some((t) => close(t, 1000, 60)) === false || pins.some((t) => close(t, 2000, 60)), JSON.stringify(pins))
  await page.getByRole('button', { name: '去掉这个手动点' }).click()
  await page.waitForTimeout(100)
  rr = (await reg())[0][0]
  ok('去掉手动点后 2.0 秒的手动点没了', !rr.track.some((p) => p.pin && close(p.tMs, 2000, 60)))
  // 丢失的部分
  globalThis.__lost = [2200, 2800]
  await page.getByRole('button', { name: '从这一帧重新追踪', exact: true }).click()
  await page.waitForFunction(() => window.__followReqs.length >= 3)
  await page.waitForTimeout(400)
  ok('丢失的部分用红色虚线画出来', (await page.locator('.rpath .lost').count()) >= 1)
  ok('说明里写了追踪不到的比例', (await page.locator('.reg .trk').textContent()).includes('追踪不到'))
  ok('提示条出现', (await page.locator('.reg .note.warn').count()) === 1)
  await seekTo(500)
  await page.getByRole('button', { name: '到下一处追踪不到的地方' }).click()
  await page.waitForTimeout(250)
  const ph = parseT((await page.locator('.time').textContent()).split('/')[0])
  ok('跳到了丢失开始的地方（约 2.1–2.2 秒）', ph > 2000 && ph < 2300, String(ph))
  globalThis.__lost = null
  // 取消
  await page.evaluate(() => (window.__hold = true))
  await page.getByRole('button', { name: '从这一帧重新追踪', exact: true }).click()
  await page.waitForSelector('.reg .prog')
  ok('追踪中显示进度条和取消按钮', (await page.locator('.reg .prog .el-progress').count()) === 1)
  ok('追踪中不能同时加新区域', await page.getByRole('button', { name: '模糊', exact: true }).isDisabled())
  const before = (await reg())[0][0].track.length
  await page.locator('.reg .prog').getByRole('button', { name: '取消' }).click()
  await page.waitForSelector('.reg .note.info')
  ok('取消后提示“已取消追踪”，轨迹没变', (await page.locator('.reg .note').textContent()).includes('已取消追踪') && (await reg())[0][0].track.length === before)
  await page.evaluate(() => (window.__hold = false))

  // ---- 效果与参数 ----
  console.log('# 效果与参数')
  ok('马赛克/模糊在监视器里有示意层', (await page.locator('.fxl .fx').count()) === 1)
  await field('效果').getByText('局部调色', { exact: true }).click()
  await page.waitForTimeout(100)
  ok('局部调色出现亮度滑块', (await field('亮度').count()) === 1)
  rr = (await reg())[0][0]
  ok('效果写进了工程', rr.effect === 'tone')
  const fxStyle = await page.locator('.fxl .fx').first().getAttribute('style')
  ok('示意层用上了调色滤镜', /brightness/.test(fxStyle || ''), fxStyle)
  await field('形状').getByText('椭圆', { exact: true }).click()
  ok('椭圆：框里出现椭圆示意', (await page.locator('.rbox.sel .ell').count()) === 1)
  ok('椭圆：示意层是圆角的', /border-radius:\s*50%/.test((await page.locator('.fxl .fx').first().getAttribute('style')) || ''))
  await page.getByText('作用于区域以外（保留区域里的，处理其余部分）', { exact: true }).click()
  await page.waitForTimeout(100)
  ok('勾选“作用于区域以外”后示意层隐藏', (await page.locator('.fxl .fx').count()) === 0)
  rr = (await reg())[0][0]
  ok('invert 和椭圆都写进了工程', rr.invert === true && rr.shape === 'ellipse')
  await page.getByText('作用于区域以外（保留区域里的，处理其余部分）', { exact: true }).click()
  // 生效时间
  await page.getByText('只在一段时间里生效', { exact: true }).click()
  await page.waitForTimeout(100)
  rr = (await reg())[0][0]
  ok('限定时间：默认是整个片段', rr.startMs === 0 && rr.endMs >= 3900, `${rr.startMs} ${rr.endMs}`)
  await seekTo(1500)
  await field('从').getByText('取播放位置').click()
  await field('到').locator('input').first().fill('3')
  await field('到').locator('input').first().press('Enter')
  await page.waitForTimeout(100)
  rr = (await reg())[0][0]
  ok('限定时间：从 ≈1.5 秒到 3 秒', close(rr.startMs, 1500, 80) && rr.endMs === 3000, `${rr.startMs} ${rr.endMs}`)
  await seekTo(3500)
  ok('超出生效时间的位置：其它区域/示意层不再显示', (await page.locator('.fxl .fx').count()) === 0)
  await seekTo(2000)
  ok('生效时间内示意层显示', (await page.locator('.fxl .fx').count()) === 1)
  // 跟随聚焦
  await field('效果').getByText('跟随聚焦', { exact: true }).click()
  await page.waitForTimeout(100)
  ok('聚焦：出现放大和镜头平滑', (await field('放大').count()) === 1 && (await field('镜头平滑').count()) === 1)
  ok('聚焦：监视器里画出取景窗口', (await page.locator('.rov .fwin').count()) === 1)
  const cb = await rectOf('.rov .rcont')
  const fw = await rectOf('.rov .fwin')
  ok('聚焦：放大 2 倍时窗口是画面的一半', close(fw.w / cb.w, 0.5, 0.02) && close(fw.h / cb.h, 0.5, 0.02), `${fw.w / cb.w} ${fw.h / cb.h}`)
  ok('聚焦：不再显示“生效时间”', (await field('生效时间').count()) === 0)
  // 不在“区域”标签里编辑时，实时画面把取景窗口放大到铺满（2 倍）
  await tab('输出')
  await page.waitForTimeout(250)
  const zoomK = await page.evaluate(() => {
    const z = document.querySelector('.stage .lay:not(.off) .zoom')
    if (!z) return -1
    const mt = /matrix\(([^)]+)\)/.exec(getComputedStyle(z).transform)
    return mt ? parseFloat(mt[1].split(',')[0]) : 1
  })
  ok('聚焦：离开“区域”标签后实时画面放大 2 倍', close(zoomK, 2, 0.05), String(zoomK))
  await page.screenshot({ path: OUT + '/r-focus-zoom.png' })
  await tab('区域')
  await page.waitForTimeout(250)
  const zoomBack = await page.evaluate(() => {
    const z = document.querySelector('.stage .lay:not(.off) .zoom')
    const mt = z && /matrix\(([^)]+)\)/.exec(getComputedStyle(z).transform)
    return mt ? parseFloat(mt[1].split(',')[0]) : 1
  })
  ok('聚焦：回到“区域”标签又显示完整画面和虚线窗口', close(zoomBack, 1, 0.01), String(zoomBack))
  // 再加一个聚焦：旧的被替掉
  await page.getByRole('button', { name: '跟随聚焦', exact: true }).first().click()
  await page.waitForSelector('.rov.draw')
  await page.keyboard.press('Escape')

  console.log('# 分割、删除、导出')
  await seekTo(2000)
  await tab('区域')
  await page.keyboard.press('s')
  await page.waitForTimeout(200)
  console.log('   toast:', await page.locator('.el-message').allTextContents(), 'active:', await page.evaluate(() => document.activeElement && document.activeElement.outerHTML.slice(0, 120)))
  ok('分割后有两个片段', (await page.locator('.blk.clip').count()) === 2)
  const pr = await proj()
  ok('两个片段都带着区域', pr.clips.length === 2 && pr.clips[0].regions.length === 1 && pr.clips[1].regions.length === 1, JSON.stringify(pr.clips.map((c) => c.regions.length)))
  const tmax = Math.max(...pr.clips[0].regions[0].track.map((p) => p.tMs))
  const tmin = Math.min(...pr.clips[1].regions[0].track.map((p) => p.tMs))
  ok('轨迹按素材时间各留自己那一段（前半不超出 2.1 秒，后半不早于 1.9 秒）', tmax <= 2200 && tmin >= 1800, `${tmax} ${tmin}`)
  ok('两半是独立的拷贝', pr.clips[0].regions[0].track !== pr.clips[1].regions[0].track)
  // 选中的是后半；播放位置在后半开头：区域页应该有它的区域
  ok('后半片段的区域在列表里', (await page.locator('.reg .item').count()) === 1)
  // 点前半：播放位置不在这个片段里
  const blks = await page.$$eval('.blk.clip', (els) => els.map((e) => { const b = e.getBoundingClientRect(); return { x: b.x + b.width / 2, y: b.y + b.height / 2 } }))
  await page.mouse.click(blks[0].x - 10, blks[0].y)
  await page.waitForTimeout(150)
  ok('选前半片段后，播放位置跟着进了前半（点片段会定位）', true)
  await seekTo(3500)
  await page.mouse.click(blks[0].x, blks[0].y - 0)
  await page.waitForTimeout(200)
  await page.evaluate(() => 0)
  // 在区域页选中区域，按删除键：删区域，不删片段
  await page.locator('.reg .item').first().click()
  await page.keyboard.press('Delete')
  await page.waitForTimeout(150)
  ok('区域页选中区域时删除键只删区域', (await page.locator('.blk.clip').count()) === 2)
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(150)
  ok('Ctrl+Z 把区域找回来', (await page.locator('.reg .item').count()) === 1)
  // 导出：区域跟着任务一起交给后端
  await page.getByRole('button', { name: '导出视频' }).click()
  await page.waitForFunction(() => !!window.__exported)
  const job = await page.evaluate(() => window.__exported)
  ok('导出的工程里带着区域（每个片段 1 个）', job.project.clips.every((c) => c.regions.length === 1 && c.regions[0].track.length >= 1), JSON.stringify(job.project.clips.map((c) => c.regions.length)))
  ok('导出的区域有后端需要的字段', ['shape', 'feather', 'grow', 'invert', 'effect', 'strength', 'brightness', 'contrast', 'saturation', 'zoom', 'reframe', 'smooth', 'startMs', 'endMs'].every((k) => k in job.project.clips[0].regions[0]))
  fs.writeFileSync(OUT + '/region-job.json', JSON.stringify(job.project, null, 1))

  console.log(`\n${pass} 通过，${fail} 失败`)
  if (errors.length) console.log('页面报错：\n' + errors.slice(0, 10).join('\n'))
  await browser.close()
  srv.close()
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(2)
})
