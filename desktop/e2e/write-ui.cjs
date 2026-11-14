// 写作窗：四个标签（帮我回 / 改写润色 / 检查 / 翻译 · 更多），帮我回的对方是谁 × 我的意思 × 怎么说、
// 看懂了、问清楚的问题清单、三版回复和微调、记住对方是谁。假后端按命令名和请求回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = Number(process.env.E2E_PORT) || 5199
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}
const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))

const INIT = (pending, extra = {}) => `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1; let jobId = 1
  const listeners = {}
  const jobs = {}
  window.__calls = []
  window.__reqs = []
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__settings = ${JSON.stringify({ ...settings, theme: extra.theme ?? 'light' })}
  window.__ws = ${JSON.stringify({ hotkey: 'Ctrl+Alt+J', badge: true, pausedUntil: 0, offApps: [], route: 'local_first', candidates: 1, style: '', pin: true, autoClose: false, win: [0, 0, 0, 0], lastAction: 'polite', extras: [], relations: extra.relations ?? {} })}
  window.__pending = ${JSON.stringify(pending)}
  const answer = (req) => {
    if (req.kind === 'replyread') {
      return /方案/.test(req.text)
        ? { ask: '催你交方案', facts: '但没说是哪一份、几点前、要多完整', unclear: ['指的是哪一份方案', '“明天之前”最晚几点', '要完整版，还是先看大纲'], relation: 'superior', who: '张总' }
        : { ask: '请你周六加班', facts: '时间已给出（9:00–15:00），语气客气、有点急', unclear: [], relation: 'superior', who: '王经理' }
    }
    if (req.kind === 'write' && req.write.action === 'reply') {
      return { candidates: [
        { text: '王经理好，周六我可以过来帮忙接待，9 点准时到。想跟您申请一下：下周一调休一天，您看可以吗？', label: '稳妥', guard: { missing: [], ratio: 1, warn: false, added: [] } },
        { text: '没问题王经理！周六 9 点我准时到。另外想跟您商量下，下周一能否调休一天？', label: '热情', guard: { missing: [], ratio: 1, warn: false, added: [] } },
        { text: '好的王经理，周六 8 点到。下周一我想调休一天，您看行吗？', label: '简短', guard: { missing: [], ratio: 1, warn: true, added: ['8'] } },
      ] }
    }
    if (req.kind === 'write' && req.write.action === 'tweak') return { candidates: [{ text: '（更软）' + req.text, guard: { missing: [], ratio: 1, warn: false, added: [] } }] }
    if (req.kind === 'write') return { candidates: [{ text: '改好了：' + req.text, guard: { missing: [], ratio: 1, warn: false } }] }
    return {}
  }
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'write' }, currentWebview: { windowLabel: 'write', label: 'write' } },
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': (listeners[args.event] ||= []).push(args.handler); return evId++
        case 'plugin:event|unlisten': return null
        case 'get_settings': return window.__settings
        case 'write_settings': return window.__ws
        case 'write_take': return window.__pending
        case 'write_run': { const id = jobId++; window.__reqs.push(args.req); jobs[id] = answer(args.req); return id }
        case 'write_job': return { state: 'done', result: jobs[args.id] }
        case 'write_personas': return ['恋爱军师|reply','回怼王|reply','淬毒之嘴|create','情话撩人|create','催进度|create','道歉修复|create','超能豆包|create','网恋助手|reply','营销高手|create','神评论|create','祝福大师|create','甜心聊宝|reply','搞笑担当|reply','情绪掌控大师|reply'].map((x, i) => ({ id: 'p' + i, name: x.split('|')[0], desc: x.split('|')[0] + '的说明', mode: x.split('|')[1], hint: '输入提示' + i }))
        case 'write_relation': window.__rel = args; return null
        case 'write_prefs': window.__prefs = args; return null
        case 'write_apply': window.__applied = args; return args.replace ? '已替换' : '已复制'
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const open = async (pending, extra) => {
    const page = await browser.newPage({ viewport: { width: 520, height: 1100 } })
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript(INIT(pending, extra))
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#write`)
    await page.waitForTimeout(1500)
    return page
  }
  const reqs = (page) => page.evaluate(() => window.__reqs)
  const chip = (page, group, name) => page.locator('.grp', { hasText: group }).locator('.pk', { hasText: name })

  // 1. 从微信取到的消息：直接进“帮我回”，先看懂
  {
    const page = await open({ text: '小李，这周六公司有客户来访，需要人手帮忙接待，你方便过来加个班吗？大概上午 9 点到下午 3 点。', fromSelection: true, app: 'WeChat.exe', canReplace: true, note: '' })
    const tabs = await page.locator('.tabs .tab').allInnerTexts()
    ok('五个标签放得下（不横向溢出）', await page.locator('.tabs').evaluate((e) => e.scrollWidth <= e.clientWidth + 1))
    ok('五个标签：帮我回 / 改写润色 / 检查 / 翻译 · 更多 / 人设', tabs.join('|') === '帮我回|改写润色|检查|翻译 · 更多|人设', tabs.join('|'))
    ok('从微信取的文字直接停在“帮我回”', (await page.locator('.tab.on').innerText()) === '帮我回')
    ok('有“收到的消息”和“＋ 贴上前面的聊天”', (await page.locator('.recv').innerText()).includes('收到的消息') && (await page.locator('.recv .lnk').innerText()).includes('贴上前面的聊天'))
    await page.waitForFunction(() => document.querySelector('.look b'), null, { timeout: 4000 }).catch(() => {})
    ok('看懂了：对方要什么（加粗）+ 细节和语气', (await page.locator('.look').innerText()).replace(/\s/g, '') === '看懂了：请你周六加班，时间已给出（9:00–15:00），语气客气、有点急。', await page.locator('.look').innerText())
    ok('顶部写来源和对方称呼', (await page.locator('.from').innerText()) === '微信 · 王经理', await page.locator('.from').innerText())
    ok('按看懂的结果先选上“上级”', await chip(page, '对方是', '上级').evaluate((e) => e.classList.contains('on')))
    const groups = await page.locator('.grp .lbl').allInnerTexts()
    ok('三组：对方是 / 我的意思 / 怎么说', groups.join('|') === '对方是|我的意思|怎么说', groups.join('|'))
    ok('六种对方、五种意思、五种说法', (await page.locator('.grp').nth(0).locator('.pk').count()) === 6 && (await page.locator('.grp').nth(1).locator('.pk').count()) === 5 && (await page.locator('.grp').nth(2).locator('.pk').count()) === 5)
    ok('默认“高情商”、长度“正常”', (await chip(page, '怎么说', '高情商').evaluate((e) => e.classList.contains('on'))) && (await page.locator('.lenrow .seg3 .on').innerText()) === '正常')

    // 有条件地答应：条件必填
    await chip(page, '我的意思', '有条件地答应').click()
    ok('选“有条件地答应”出现“条件”框', (await page.locator('.fld span').innerText()) === '条件')
    ok('没写条件时不能生成，并说还差什么', (await page.locator('.go').isDisabled()) && (await page.locator('.runrow').innerText()).includes('还差：写上条件'))
    await page.locator('.fld input').fill('下周一调休一天')
    await page.locator('.say').fill('9 点准时到')
    ok('写上条件后可以生成，按钮叫“生成回复”', !(await page.locator('.go').isDisabled()) && (await page.locator('.go').innerText()).startsWith('生成回复'))
    await page.locator('.go').click()
    await page.waitForFunction(() => document.querySelectorAll('.vt').length === 3, null, { timeout: 4000 }).catch(() => {})
    const req = (await reqs(page)).find((r) => r.kind === 'write' && r.write.action === 'reply')
    ok('请求：上级 × 有条件（条件）× 高情商 × 正常 + 还想说', req && req.write.reply.relation === 'superior' && req.write.reply.intent === 'conditional' && req.write.reply.detail === '下周一调休一天' && req.write.tone === 'eq' && req.write.length === 'medium' && req.write.extra === '9 点准时到', JSON.stringify(req?.write))
    ok('三版：稳妥 / 热情 / 简短', (await page.locator('.vt').allInnerTexts()).join('|') === '稳妥|热情|简短')
    ok('结果标题是“回复”，显示第一版', (await page.locator('.reshd b').innerText()) === '回复' && (await page.locator('.out').innerText()).startsWith('王经理好'))
    ok('写回按钮叫“替换到微信”', (await page.locator('.res .go.sm').innerText()) === '替换到微信')
    ok('有一句“不乱说”的说明', (await page.locator('.res .fine').innerText()).includes('不替你编理由'))
    ok('五个微调', (await page.locator('.tweaks .tw').allInnerTexts()).join('|') === '更软一点|更硬一点|再短点|去掉客套|加个表情')
    await page.locator('.vt', { hasText: '简短' }).click()
    ok('凭空多出来的数字标黄提醒', (await page.locator('.res .warn').innerText()).includes('「8」'), await page.locator('.res .warn').innerText().catch(() => ''))
    await page.locator('.vt', { hasText: '稳妥' }).click()
    await page.locator('.tweaks .tw', { hasText: '更软一点' }).click()
    await page.waitForFunction(() => /（更软）/.test(document.querySelector('.out')?.textContent || ''), null, { timeout: 4000 }).catch(() => {})
    const tw = (await reqs(page)).find((r) => r.write?.action === 'tweak')
    ok('微调在当前这一版上改，带着收到的消息', tw && tw.write.reply.tweak === 'softer' && tw.text.startsWith('王经理好') && tw.write.reply.received.startsWith('小李'), JSON.stringify(tw))
    ok('微调后替换这一版，标签不变', (await page.locator('.out').innerText()).startsWith('（更软）王经理好') && (await page.locator('.vt.on').innerText()) === '稳妥')
    await page.locator('.res .go.sm').click()
    await page.waitForTimeout(200)
    ok('替换到微信：写回当前这一版', (await page.evaluate(() => window.__applied))?.replace === true)

    // 拒绝默认配高情商（体面拒绝）；幽默接话对上级提醒
    await chip(page, '怎么说', '正式得体').click()
    await chip(page, '我的意思', '拒绝').click()
    ok('选“拒绝”自动配“高情商”（体面拒绝）', await chip(page, '怎么说', '高情商').evaluate((e) => e.classList.contains('on')))
    ok('拒绝：理由（可不说）+ 给个替代', (await page.locator('.fld span').innerText()) === '理由' && (await page.locator('.mini .seg3 button').allInnerTexts()).join('|') === '不给|改天|推荐别人')
    await chip(page, '怎么说', '幽默接话').click()
    ok('对上级用幽默接话时提醒', (await page.locator('.grp .warn').innerText()).includes('可能不合适'))
    await chip(page, '我的意思', '再想想').click()
    ok('再想想：答复时间 + 今晚 / 明天 / 这周内', (await page.locator('.fld span').innerText()) === '答复时间' && (await page.locator('.mini .tw').allInnerTexts()).join('|') === '今晚|明天|这周内')
    await page.locator('.mini .tw', { hasText: '明天' }).click()
    ok('点“明天”填进答复时间', (await page.locator('.fld input').inputValue()) === '明天')

    // 记住对方是谁
    await chip(page, '对方是', '同事').click()
    await page.waitForTimeout(100)
    const rel = await page.evaluate(() => window.__rel)
    ok('改了对方是谁就记住（程序 + 称呼）', rel && rel.key === 'wechat.exe|王经理' && rel.relation === 'colleague', JSON.stringify(rel))

    // 贴上前面的聊天：只用来理解，会重新看懂
    await page.locator('.recv .lnk').click()
    await page.locator('.recv .ctx').fill('王经理：上周辛苦了')
    await page.waitForFunction(() => window.__reqs.filter((r) => r.kind === 'replyread').length >= 2, null, { timeout: 4000 }).catch(() => {})
    const rr = (await reqs(page)).filter((r) => r.kind === 'replyread')
    ok('贴上前面的聊天后重新看懂一次，带着聊天', rr.length >= 2 && rr.at(-1).write.reply.context === '王经理：上周辛苦了', JSON.stringify(rr.at(-1)))
    await page.close()
  }

  // 2. 问清楚：自动列出没说清的地方，最要紧的两条默认勾上；暗色
  {
    const page = await open({ text: '那个方案你明天之前能给我吗？', fromSelection: true, app: 'Feishu.exe', canReplace: true, note: '' }, { theme: 'dark', relations: { 'feishu.exe|张总': 'client' } })
    await page.waitForFunction(() => document.querySelector('.look b'), null, { timeout: 4000 }).catch(() => {})
    ok('暗色主题', await page.evaluate(() => document.documentElement.classList.contains('dark')))
    ok('记住过“张总是客户”：自动选上客户（不用看懂的猜测）', await chip(page, '对方是', '客户').evaluate((e) => e.classList.contains('on')))
    await chip(page, '我的意思', '问清楚').click()
    const qs = await page.locator('.qs .q').evaluateAll((els) => els.map((e) => [e.querySelector('span').textContent, e.querySelector('input').checked]))
    ok('问清楚：列出三条没说清的，前两条勾上', JSON.stringify(qs) === JSON.stringify([['指的是哪一份方案', true], ['“明天之前”最晚几点', true], ['要完整版，还是先看大纲', false]]), JSON.stringify(qs))
    await page.locator('.qadd input').fill('发到哪个群')
    await page.locator('.qadd input').press('Enter')
    await page.locator('.qs .q').nth(1).locator('input').uncheck()
    await page.locator('.go').click()
    await page.waitForTimeout(800)
    const req = (await reqs(page)).find((r) => r.kind === 'write' && r.write.action === 'reply')
    ok('只问勾上的（含自己加的）', JSON.stringify(req?.write.reply.questions) === JSON.stringify(['指的是哪一份方案', '发到哪个群']), JSON.stringify(req?.write.reply))
    ok('写回按钮叫“替换到飞书”', (await page.locator('.res .go.sm').innerText()) === '替换到飞书')
    const goBg = await page.locator('.res .go.sm').evaluate((e) => getComputedStyle(e).backgroundColor + '|' + getComputedStyle(e).color)
    ok('暗色主按钮：琥珀底、深色字', goBg === 'rgb(255, 181, 71)|rgb(26, 17, 0)', goBg)
    await page.close()
  }

  // 3. 别的程序：停在上次用的标签；改写润色共用“怎么说”；检查；翻译 · 更多
  {
    const page = await open({ text: '这个功能下周上线，大家注意一下。', fromSelection: true, app: 'winword.exe', canReplace: true, note: '' })
    ok('Word 里取的文字停在上次用的“改写润色”', (await page.locator('.tab.on').innerText()) === '改写润色')
    ok('改写润色：改写 / 润色 / 更简洁 / 缩短 / 更礼貌 / 更有说服力', (await page.locator('.seg .seg-item').allInnerTexts()).join('|') === '改写|润色|更简洁|缩短|更礼貌|更有说服力')
    ok('上次的“更礼貌”还选着', (await page.locator('.seg .seg-item.on').innerText()) === '更礼貌')
    await page.locator('.grp .pk', { hasText: '温暖真诚' }).click()
    await page.locator('.go').click()
    await page.waitForTimeout(800)
    const req = (await reqs(page)).at(-1)
    ok('改写带上“怎么说”', req.write.action === 'polite' && req.write.tone === 'heartfelt', JSON.stringify(req.write))
    ok('不是聊天程序：写回按钮叫“替换原文”', (await page.locator('.res .go.sm').innerText()) === '替换原文')
    await page.locator('.tab', { hasText: '检查' }).click()
    ok('检查：按钮是“语法检查”', (await page.locator('.go').innerText()).startsWith('语法检查'))
    await page.locator('.tab', { hasText: '翻译' }).click()
    ok('翻译 · 更多：翻译 / 总结 / 续写 + 目标语言', (await page.locator('.seg .seg-item').allInnerTexts()).join('|') === '翻译|总结|续写' && (await page.locator('.tg').count()) === 1)
    const sel = await page.locator('.selrow').innerText()
    ok('划词工具和摘录、收藏在这里', ['释义', '朗读', 'AI 搜索', '存入摘录', '摘录列表', '我的收藏'].every((x) => sel.includes(x)), sel)
    await page.locator('.selrow .chip', { hasText: '我的收藏' }).click()
    ok('我的收藏：空的时候有说明', (await page.locator('.favs').innerText()).includes('还没有收藏'))
    const prefs = await page.evaluate(() => window.__prefs)
    ok('切标签记住上次的动作', prefs?.lastAction === 'translate', JSON.stringify(prefs))
    await page.close()
  }

  // 4. 人设：14 个，选一个才能生成，请求里带人设 id；写回时带上来源程序（写作记忆用）
  {
    const page = await open({ text: '你这人怎么老是迟到', fromSelection: true, app: 'WeChat.exe', canReplace: true, note: '' })
    await page.locator('.tab', { hasText: '人设' }).click()
    await page.waitForTimeout(300)
    const names = await page.locator('.pcard b').allInnerTexts()
    ok('人设标签里有 14 个人设', names.length === 14 && ['恋爱军师', '回怼王', '淬毒之嘴', '情话撩人', '催进度', '道歉修复', '超能豆包', '网恋助手', '营销高手', '神评论', '祝福大师', '甜心聊宝', '搞笑担当', '情绪掌控大师'].every((n) => names.includes(n)), names.join(','))
    ok('没选人设时不能生成', await page.locator('.go').first().isDisabled())
    await page.locator('.pcard', { hasText: '回怼王' }).click()
    ok('选了人设显示它的输入提示', (await page.locator('.hint').first().innerText()).includes('输入提示1'))
    await page.locator('.go').first().click()
    await page.waitForTimeout(500)
    const req = await page.evaluate(() => window.__reqs.at(-1))
    ok('请求里带人设：action=persona + 人设 id', req && req.kind === 'write' && req.write.action === 'persona' && req.write.persona === 'p1', JSON.stringify(req))
    await page.locator('.res .go.sm').first().click()
    await page.waitForTimeout(300)
    const ap = await page.evaluate(() => window.__applied)
    ok('写回时带上来源程序', ap && ap.sourceApp === 'WeChat.exe', JSON.stringify(ap))
    await page.close()
  }

  ok('没有页面错误', !errors.length, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
