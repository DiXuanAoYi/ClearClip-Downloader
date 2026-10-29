const fs = require('fs')
const { launch, openHarness, m, pixel, OUT, urlOf, addMenu } = require('./lib.cjs')

let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const close = (a, b, tol) => Math.abs(a - b) <= tol

;(async () => {
  const { srv, browser, page, errors } = await launch()
  page.on('response', (r) => r.status() >= 400 && console.log('  [http ' + r.status() + '] ' + r.url().slice(0, 140)))
  await openHarness(page, errors)

  // ---- 工具 ----
  const parseT = (s) => {
    const mm = /^(\d+):(\d{2})(?:\.(\d+))?$/.exec(s.trim())
    return mm ? (+mm[1] * 60 + +mm[2]) * 1000 + (mm[3] ? Math.round(+('0.' + mm[3]) * 1000) : 0) : NaN
  }
  const clips = () =>
    page.$$eval('.blk.clip', (els) =>
      els.map((e) => {
        const r = e.getBoundingClientRect()
        return { name: e.querySelector('.cap')?.textContent.trim(), x: r.x, w: r.width, y: r.y, h: r.height, sel: e.classList.contains('sel'), bad: e.classList.contains('bad'), xf: e.querySelector('.xf')?.getBoundingClientRect().width ?? 0 }
      }),
    )
  const total = async () => parseT(await page.locator('.info .mono').first().textContent())
  const playhead = async () => parseT((await page.locator('.time').textContent()).split('/')[0])
  const pps = async () => {
    const t = await page.$$eval('.tick', (els) => els.slice(0, 3).map((e) => ({ left: parseFloat(e.style.left), label: e.textContent.trim() })))
    const secs = parseT(t[1].label.includes('.') ? t[1].label : t[1].label + '.0')
    return t[1].left / (secs / 1000)
  }
  const rulerX = async (ms) => {
    const r = await page.locator('.ruler').boundingBox()
    return r.x + (ms / 1000) * (await pps())
  }
  const seekTo = async (ms) => {
    const r = await page.locator('.ruler').boundingBox()
    await page.mouse.click(await rulerX(ms), r.y + 10)
  }
  const field = (label) => page.locator('.insp .f', { has: page.locator(`.n:text-is("${label}")`) })
  const setTime = async (label, text) => {
    const i = field(label).locator('input').first()
    await i.fill(text)
    await i.press('Enter')
    await page.waitForTimeout(80)
  }
  const exportInfo = () => page.locator('.info').textContent()
  const dialogOpen = (v) => page.evaluate((a) => (window.__dialogOpen = [a]), v)
  const stage = () => page.locator('.stage')
  const stagePixel = async (fx, fy) => {
    const b = await stage().boundingBox()
    const buf = await page.screenshot({ clip: b })
    return pixel(buf, Math.round(b.width * fx), Math.round(b.height * fy))
  }
  const dominant = ([r, g, b]) => (r > 150 && g < 90 && b < 90 ? 'red' : g > 90 && r < 90 && b < 90 ? 'green' : b > 150 && r < 90 && g < 90 ? 'blue' : `rgb(${r},${g},${b})`)

  // ---- 1 空白状态 ----
  console.log('# 空白状态')
  ok('监视器提示可见', await page.locator('.stage .hint').isVisible())
  ok('导出按钮不可用', await page.getByRole('button', { name: '导出视频' }).isDisabled())
  ok('主轨提示可见', await page.locator('.main-row .empty').isVisible())

  // ---- 2 添加片段 ----
  console.log('# 添加片段')
  await dialogOpen([m('red.webm'), m('green.webm')])
  await addMenu(page, '添加到主轨…')
  await page.waitForSelector('.blk.clip')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 2)
  let cs = await clips()
  ok('两个片段按顺序排列', cs[0].name === 'red.webm' && cs[1].name === 'green.webm', JSON.stringify(cs.map((c) => c.name)))
  const t0 = await total()
  ok('总长 = 红 + 绿 ≈ 11 秒', close(t0, 11000, 150), String(t0))
  ok('最后加入的片段被选中', cs[1].sel)
  ok('监视器显示红色', dominant(await stagePixel(0.5, 0.5)) === 'red', dominant(await stagePixel(0.5, 0.5)))
  ok('输出尺寸跟随第一个片段 640×360', (await exportInfo()).includes('640×360'))
  const p0 = await pps()
  ok('缩放：适合窗口后 11 秒占满大半个时间线', p0 > 40 && p0 < 200, String(p0))
  await page.screenshot({ path: OUT + '/a1-added.png' })

  // ---- 3 选择、裁剪（输入框）----
  console.log('# 选择与裁剪')
  await page.mouse.click(cs[0].x + cs[0].w / 2, cs[0].y + cs[0].h / 2)
  await page.waitForTimeout(100)
  cs = await clips()
  ok('点击片段选中', cs[0].sel && !cs[1].sel)
  ok('点击片段移动了播放位置', close(await playhead(), 3000, 400), String(await playhead()))
  ok('检查器显示文件名', (await page.locator('.insp .title').textContent()).includes('red.webm'))
  await setTime('出点', '4')
  cs = await clips()
  ok('出点改成 4 秒后总长 ≈ 9 秒', close(await total(), 9000, 150), String(await total()))
  ok('片段变短', cs[0].w < 4.2 * p0 && cs[0].w > 3.8 * p0, String(cs[0].w))
  await setTime('出点', 'abc')
  ok('写错的时间会恢复原值', (await field('出点').locator('input').first().inputValue()) === '0:04.000')
  await setTime('出点', '99')
  ok('超出素材长度的时间被拒绝', (await field('出点').locator('input').first().inputValue()) === '0:04.000')

  // ---- 4 分割 ----
  console.log('# 分割')
  await seekTo(1500)
  ok('点标尺定位', close(await playhead(), 1500, 150), String(await playhead()))
  await page.keyboard.press('s')
  await page.waitForTimeout(150)
  cs = await clips()
  ok('按 S 分割成三个片段', cs.length === 3, String(cs.length))
  ok('分割后总长不变', close(await total(), 9000, 150), String(await total()))
  ok('分割后两段宽度之和 ≈ 4 秒', close(cs[0].w + cs[1].w, 4 * p0, 6), String(cs[0].w + cs[1].w))
  ok('分割出的后一段被选中', cs[1].sel)
  ok('分割位置约 1.5 秒', close(cs[0].w, 1.5 * p0, 12), String(cs[0].w / p0))

  // ---- 5 拖动排序 + 撤销 ----
  console.log('# 拖动排序、撤销')
  cs = await clips()
  const g = cs[2]
  await page.mouse.move(g.x + g.w / 2, g.y + g.h / 2)
  await page.mouse.down()
  await page.mouse.move(g.x + g.w / 2 - 40, g.y + g.h / 2, { steps: 4 })
  ok('拖动时出现落点指示', await page.locator('.drop').isVisible())
  await page.mouse.move(cs[0].x + 8, g.y + g.h / 2, { steps: 12 })
  await page.mouse.up()
  await page.waitForTimeout(150)
  cs = await clips()
  ok('绿色片段拖到最前面', cs[0].name === 'green.webm' && cs[1].name === 'red.webm', JSON.stringify(cs.map((c) => c.name)))
  ok('监视器：回到开头显示绿色', (await seekTo(100), await page.waitForTimeout(300), dominant(await stagePixel(0.5, 0.5))) === 'green')
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(150)
  cs = await clips()
  ok('Ctrl+Z 撤销排序', cs[0].name === 'red.webm' && cs[2].name === 'green.webm', JSON.stringify(cs.map((c) => c.name)))
  await page.keyboard.press('Control+Shift+z')
  await page.waitForTimeout(150)
  cs = await clips()
  ok('Ctrl+Shift+Z 重做', cs[0].name === 'green.webm', JSON.stringify(cs.map((c) => c.name)))
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(150)

  // ---- 6 拖边缘裁剪（一次拖动 = 一步撤销）----
  console.log('# 拖边缘裁剪')
  cs = await clips()
  const last = cs[2]
  const wBefore = last.w
  await page.mouse.click(last.x + last.w / 2, last.y + last.h / 2)
  await page.mouse.move(last.x + last.w - 3, last.y + last.h / 2)
  await page.mouse.down()
  await page.mouse.move(last.x + last.w - 3 - 30, last.y + last.h / 2, { steps: 6 })
  await page.mouse.move(last.x + last.w - 3 - 60, last.y + last.h / 2, { steps: 6 })
  await page.mouse.up()
  await page.waitForTimeout(150)
  cs = await clips()
  ok('右边缘向左拖 60 像素，片段缩短约 60 像素', close(wBefore - cs[2].w, 60, 12), String(wBefore - cs[2].w))
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(150)
  cs = await clips()
  ok('整个拖动只算一步撤销', close(cs[2].w, wBefore, 4), `${cs[2].w} vs ${wBefore}`)

  // ---- 7 转场 ----
  console.log('# 转场')
  cs = await clips()
  await page.mouse.click(cs[1].x + cs[1].w / 2, cs[1].y + cs[1].h / 2)
  const tBefore = await total()
  await field('转场').locator('.el-select').click()
  await page.locator('.el-select-dropdown__item:visible', { hasText: '淡入淡出' }).click()
  await page.waitForTimeout(200)
  cs = await clips()
  ok('转场使后一个片段和前一个重叠', cs[1].xf > 0.4 * p0 && cs[1].xf < 0.6 * p0, String(cs[1].xf / p0))
  ok('总长缩短 0.5 秒', close(tBefore - (await total()), 500, 60), String(tBefore - (await total())))
  ok('第一个片段没有转场选项', (await (async () => {
    await page.mouse.click(cs[0].x + cs[0].w / 2, cs[0].y + cs[0].h / 2)
    await page.waitForTimeout(100)
    return await field('转场').count()
  })()) === 0)
  await page.mouse.click(cs[1].x + cs[1].w / 2, cs[1].y + cs[1].h / 2)
  await page.waitForTimeout(100)
  ok('选了转场后出现转场时长', (await field('转场时长').count()) === 1)
  await page.screenshot({ path: OUT + '/a2-transition.png' })

  // ---- 8 文字 ----
  console.log('# 文字')
  await seekTo(500)
  await page.getByRole('button', { name: '添加文字' }).click()
  await page.waitForTimeout(150)
  ok('时间线上出现文字块', (await page.locator('.blk.txt').count()) === 1)
  await page.locator('.insp textarea').fill('你好 Hello')
  await page.waitForTimeout(150)
  ok('监视器显示文字', (await page.locator('.stage .txt').textContent()).trim() === '你好 Hello')
  const tb0 = await page.locator('.stage .txt').boundingBox()
  const sb = await stage().boundingBox()
  ok('文字默认在画面下方居中', tb0.y + tb0.height / 2 > sb.y + sb.height * 0.75 && close(tb0.x + tb0.width / 2, sb.x + sb.width / 2, 8), JSON.stringify([tb0, sb]))
  // 在监视器里拖动文字
  await page.mouse.move(tb0.x + tb0.width / 2, tb0.y + tb0.height / 2)
  await page.mouse.down()
  await page.mouse.move(sb.x + sb.width * 0.3, sb.y + sb.height * 0.25, { steps: 8 })
  await page.mouse.up()
  await page.waitForTimeout(150)
  const tb1 = await page.locator('.stage .txt').boundingBox()
  ok('拖动文字改变了位置', tb1.y < tb0.y - 40 && tb1.x < tb0.x - 40, JSON.stringify([tb0, tb1]))
  const xv = await field('左右位置').locator('.val').textContent()
  ok('检查器里的位置同步更新', parseInt(xv) < 45, xv)
  await seekTo(5000)
  await page.waitForTimeout(200)
  ok('超出文字时间范围后监视器不显示文字', (await page.locator('.stage .txt').count()) === 0)
  await page.screenshot({ path: OUT + '/a3-text.png' })

  // ---- 9 配乐 ----
  console.log('# 配乐')
  await dialogOpen([m('music.ogg')])
  await addMenu(page, '添加配乐…')
  await page.waitForSelector('.blk.aud')
  ok('音频行出现配乐块', (await page.locator('.blk.aud').count()) === 1)
  ok('检查器切到配乐', (await page.locator('.insp .title').textContent()).includes('music.ogg'))
  const aw = (await page.locator('.blk.aud').boundingBox()).width
  ok('配乐块只到视频结束（从 5 秒开始，长度 = 总长 - 5 秒）', close(aw, ((await total()) - 5000) / 1000 * (await pps()), 14), String(aw / (await pps())))
  await field('').locator('input[type=checkbox]').first().check().catch(() => undefined)

  // ---- 10 播放 ----
  console.log('# 播放')
  await seekTo(0)
  await page.waitForTimeout(300)
  await page.getByRole('button', { name: '播放', exact: true }).click()
  await page.waitForTimeout(1500)
  const ph1 = await playhead()
  ok('播放 1.5 秒后播放位置前进', ph1 > 1000 && ph1 < 2300, String(ph1))
  const playingState = await page.evaluate(() => {
    const vs = [...document.querySelectorAll('.stage .lay:not(.off) video')]
    return vs.map((v) => ({ t: v.currentTime, paused: v.paused, muted: v.muted, rate: v.playbackRate }))
  })
  ok('可见的视频元素都在播放（转场重叠区会有两个）', playingState.length >= 1 && playingState.length <= 2 && playingState.every((v) => !v.paused) && playingState.some((v) => v.t > 0.8), JSON.stringify(playingState))
  ok('监视器播放中显示红色', dominant(await stagePixel(0.5, 0.3)) === 'red', dominant(await stagePixel(0.5, 0.3)))
  ok('播放按钮变成暂停', (await page.locator('.transport .play').textContent()).trim() === '暂停')
  await page.keyboard.press('Space')
  await page.waitForTimeout(200)
  ok('空格暂停', (await page.locator('.transport .play').textContent()).trim() === '播放')
  const frozen = await playhead()
  await page.waitForTimeout(400)
  ok('暂停后位置不再前进', (await playhead()) === frozen)

  // 配乐从 5 秒开始：跳到 5.3 秒播放 1.2 秒，配乐应该在播
  await seekTo(5300)
  await page.waitForTimeout(200)
  await page.getByRole('button', { name: '播放', exact: true }).click()
  await page.waitForTimeout(1300)
  const musicState = await page.evaluate(() => [...document.querySelectorAll('audio')].map((a) => ({ paused: a.paused, t: a.currentTime })))
  ok('配乐跟着播放', musicState.length === 1 && !musicState[0].paused && musicState[0].t > 0.8 && musicState[0].t < 2.2, JSON.stringify(musicState))
  await page.keyboard.press('Space')

  // 跨片段连续播放：从红色片段结尾前 0.5 秒开始，应该切换到绿色
  const cs2 = await clips()
  const redEnd = (cs2[0].w + cs2[1].w) / (await pps()) * 1000 // 前两段（红 1.5 + 红 2.5 = 4 秒）
  await seekTo(redEnd - 600)
  await page.waitForTimeout(200)
  await page.getByRole('button', { name: '播放', exact: true }).click()
  await page.waitForTimeout(2200)
  const cross = await playhead()
  ok('越过片段边界继续播放', cross > redEnd + 600, `${cross} / ${redEnd}`)
  const px2 = dominant(await stagePixel(0.5, 0.3))
  ok('越过红色片段后监视器显示绿色', px2 === 'green', px2)
  await page.keyboard.press('Space')

  // 播放到结尾自动停止
  await seekTo((await total()) - 700)
  await page.getByRole('button', { name: '播放', exact: true }).click()
  await page.waitForTimeout(1500)
  ok('播放到结尾自动停止', (await page.locator('.transport .play').textContent()).trim() === '播放' && close(await playhead(), await total(), 60), `${await playhead()} / ${await total()}`)

  // ---- 11 预览成片 ----
  console.log('# 预览成片')
  const previewFile = m('preview.webm')
  await page.evaluate((u) => (window.__previewUrl = u), urlOf(previewFile))
  await page.locator('.transport .el-radio-button', { hasText: '精确预览' }).click()
  await page.waitForTimeout(200)
  const started = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'edit_preview_start').length)
  ok('发出了生成预览的请求', started === 1)
  await page.evaluate(() => window.__emit('media://jobs', [{ id: 77, title: 'x', op: '剪辑预览', status: 'running', percent: 40, output: null, error: null, note: null, finishedAt: null }]))
  await page.waitForTimeout(150)
  ok('显示生成进度', (await page.locator('.pvbar .el-progress').count()) === 1)
  await page.evaluate((f) => window.__emit('media://jobs', [{ id: 77, title: 'x', op: '剪辑预览', status: 'done', percent: 100, output: f, error: null, note: null, finishedAt: 1 }]), previewFile)
  await page.waitForTimeout(500)
  ok('生成完成后监视器切到预览成片', (await page.locator('.transport .el-radio-button.is-active').textContent()).includes('精确预览'))
  ok('预览是最新的时不显示过期提示', (await page.locator('.pvbar').count()) === 0)
  await seekTo(500)
  await page.getByRole('button', { name: '播放', exact: true }).click()
  await page.waitForTimeout(1200)
  const pv = await page.evaluate(() => { const v = document.querySelector('video.pv'); return { t: v.currentTime, paused: v.paused, src: v.src } })
  ok('预览成片在播放', !pv.paused && pv.t > 0.8 && pv.src.includes('preview.webm'), JSON.stringify(pv))
  ok('播放位置跟着预览视频走', close(await playhead(), pv.t * 1000, 400), `${await playhead()} vs ${pv.t}`)
  await page.screenshot({ path: OUT + '/a4-preview.png' })
  await page.keyboard.press('Space')
  // 改动工程 → 预览过期，监视器回到实时画面
  cs = await clips()
  await page.mouse.click(cs[0].x + cs[0].w / 2, cs[0].y + cs[0].h / 2)
  await field('音量').locator('.el-slider__button-wrapper').focus()
  await page.keyboard.press('ArrowRight')
  await page.waitForTimeout(250)
  ok('改动工程后预览标记为过期', (await page.locator('.pvbar').textContent()).includes('预览已过期'))
  ok('监视器自动回到实时画面', (await page.locator('.transport .el-radio-button.is-active').textContent()).includes('实时画面'))
  const startsBefore = await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'edit_preview_start').length)
  await page.locator('.transport .el-radio-button', { hasText: '精确预览' }).click()
  await page.waitForTimeout(200)
  ok('过期后再点“精确预览”会重新生成', (await page.evaluate(() => window.__calls.filter((c) => c.cmd === 'edit_preview_start').length)) === startsBefore + 1)
  ok('重新生成期间仍停在实时画面', (await page.locator('.transport .el-radio-button.is-active').textContent()).includes('实时画面'))
  await page.evaluate(() => window.__emit('media://jobs', [{ id: 77, title: 'x', op: '剪辑预览', status: 'canceled', percent: 10, output: null, error: null, note: null, finishedAt: 1 }]))

  // ---- 12 导出 ----
  console.log('# 导出')
  await page.getByRole('button', { name: '导出视频' }).click()
  await page.waitForTimeout(300)
  const exp = await page.evaluate(() => window.__exported)
  ok('发出了导出请求（op=edit）', exp && exp.op === 'edit' && Array.isArray(exp.inputs) && exp.inputs.length === 0, JSON.stringify(exp)?.slice(0, 200))
  ok('工程里有 3 个片段、1 条文字、1 条配乐', exp.project.clips.length === 3 && exp.project.texts.length === 1 && exp.project.audio.length === 1)
  ok('转场写入了第二个片段', exp.project.clips[1].transition && exp.project.clips[1].transition.kind === 'fade' && exp.project.clips[1].transition.durationMs === 500, JSON.stringify(exp.project.clips[1].transition))
  fs.writeFileSync(OUT + '/exported-job.json', JSON.stringify(exp, null, 2))

  // ---- 13 保存 / 新建 / 打开 ----
  console.log('# 工程文件')
  ok('有未保存的修改时显示提示', (await exportInfo()).includes('工程还没有保存'))
  await page.evaluate(() => (window.__dialogSave = ['/tmp/demo.ccedit']))
  await page.getByRole('button', { name: '保存', exact: true }).click()
  await page.waitForTimeout(300)
  ok('保存后显示文件名', (await exportInfo()).includes('demo.ccedit') && !(await exportInfo()).includes('未保存'), await exportInfo())
  const saved = await page.evaluate(() => window.__files['/tmp/demo.ccedit'])
  ok('保存的工程内容完整', saved && saved.clips.length === 3 && saved.out.fit === 'contain')
  await page.keyboard.press('Control+z')
  await page.waitForTimeout(100)
  ok('保存后再改动显示“有未保存的修改”', (await exportInfo()).includes('有未保存的修改'))
  await page.getByRole('button', { name: '新建' }).click()
  await page.waitForSelector('.el-message-box')
  ok('未保存时新建会询问', await page.locator('.el-message-box').isVisible())
  await page.getByRole('button', { name: '放弃修改' }).click()
  await page.waitForTimeout(300)
  ok('新建后时间线为空', (await page.locator('.blk.clip').count()) === 0)
  await page.evaluate(() => (window.__dialogOpen = ['/tmp/demo.ccedit']))
  await page.getByRole('button', { name: '打开…', exact: true }).click()
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 3, null, { timeout: 8000 })
  cs = await clips()
  ok('打开工程后恢复片段', cs.length === 3 && cs[0].name === 'red.webm')
  ok('打开后缩略图和时长已重新读取', (await total()) > 8000)

  // ---- 14 素材丢失与重新指定 ----
  console.log('# 素材丢失')
  const gone = await page.evaluate(() => {
    const p = JSON.parse(JSON.stringify(window.__files['/tmp/demo.ccedit']))
    p.clips[2].path = '/nonexistent/lost.webm'
    window.__files['/tmp/lost.ccedit'] = p
    return true
  })
  await page.evaluate(() => (window.__dialogOpen = ['/tmp/lost.ccedit']))
  await page.getByRole('button', { name: '打开…', exact: true }).click()
  await page.waitForTimeout(1500)
  cs = await clips()
  ok('找不到的素材标红', cs.length === 3 && cs[2].bad && !cs[0].bad, JSON.stringify(cs.map((c) => c.bad)))
  await page.mouse.click(cs[2].x + cs[2].w / 2, cs[2].y + cs[2].h / 2)
  ok('检查器提示重新指定', (await page.getByRole('button', { name: '重新指定文件…' }).count()) === 1)
  ok('素材丢失时导出被拦住', (await (async () => {
    await page.evaluate(() => (window.__exported = null))
    await page.getByRole('button', { name: '导出视频' }).click()
    await page.waitForTimeout(300)
    return await page.evaluate(() => window.__exported)
  })()) === null)
  await dialogOpen(m('green.webm'))
  await page.getByRole('button', { name: '重新指定文件…' }).click()
  await page.waitForTimeout(600)
  cs = await clips()
  ok('重新指定后恢复正常', cs.every((c) => !c.bad) && cs[2].name === 'green.webm', JSON.stringify(cs.map((c) => [c.name, c.bad])))
  await page.evaluate(() => document.querySelectorAll('.el-message').forEach((e) => e.remove()))

  // ---- 15 竖屏、图片、旋转 ----
  console.log('# 图片、旋转、输出设置')
  await page.getByRole('button', { name: '新建' }).click()
  await page.getByRole('button', { name: '放弃修改' }).click().catch(() => undefined)
  await page.waitForTimeout(300)
  await dialogOpen([m('portrait.webm'), m('yellow.png')])
  await addMenu(page, '添加到主轨…')
  await page.waitForFunction(() => document.querySelectorAll('.blk.clip').length === 2)
  ok('竖屏素材：输出尺寸 360×640', (await exportInfo()).includes('360×640'), await exportInfo())
  cs = await clips()
  ok('图片片段默认 5 秒', close(cs[1].w / (await pps()), 5, 0.3), String(cs[1].w / (await pps())))
  ok('图片的检查器没有速度滑块', (await field('速度').count()) === 0)
  await page.mouse.click(cs[1].x + cs[1].w / 2, cs[1].y + cs[1].h / 2)
  await page.locator('.insp .el-radio-button', { hasText: '右转 90°' }).click()
  await page.waitForTimeout(200)
  const imgCss = await page.evaluate(() => { const i = document.querySelector('.stage .lay:not(.off) img'); return i ? i.style.transform : null })
  ok('图片旋转 90° 后监视器里也旋转', imgCss && imgCss.includes('rotate(90deg)'), String(imgCss))
  await page.locator('.insp .seg .el-radio-button', { hasText: '输出' }).click()
  await page.waitForTimeout(150)
  await field('画面尺寸').locator('.el-select').click()
  await page.locator('.el-select-dropdown__item:visible', { hasText: '1920×1080 横屏 1080p' }).click()
  await page.waitForTimeout(200)
  ok('选了输出尺寸后监视器比例变横', (await stage().boundingBox()).width > (await stage().boundingBox()).height)
  ok('信息栏显示 1920×1080', (await exportInfo()).includes('1920×1080'))
  await page.screenshot({ path: OUT + '/a5-output.png' })

  // ---- 16 英文界面 ----
  console.log('# 英文界面')
  await page.locator('.insp .seg .el-radio-button', { hasText: '属性' }).click().catch(() => undefined)
  await page.evaluate(() => window.__setLang('en'))
  await page.waitForTimeout(500)
  const leftover = await page.evaluate(() => {
    const out = []
    const w = document.createTreeWalker(document.querySelector('.ed'), NodeFilter.SHOW_TEXT)
    for (let n = w.nextNode(); n; n = w.nextNode()) if (/[一-鿿]/.test(n.nodeValue) && !n.parentElement.closest('[data-no-i18n]')) out.push(n.nodeValue.trim())
    return out
  })
  ok('英文模式下编辑器里没有遗漏的中文', leftover.length === 0, JSON.stringify(leftover))
  await page.screenshot({ path: OUT + '/a6-english.png' })
  await page.evaluate(() => window.__setLang('zh'))

  console.log('\n页面错误 / 控制台:', errors.filter((e) => !/404/.test(e)))
  console.log(`\n通过 ${pass}，失败 ${fail}`)
  await browser.close()
  srv.close()
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
