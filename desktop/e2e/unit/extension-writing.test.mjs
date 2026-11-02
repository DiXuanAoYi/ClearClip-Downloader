// 写作浮窗的纯逻辑：设置整理、请求合成、输入框判断、文字差异与逐处取舍、语法建议合并。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ Uint16Array, Set, Math, Number, Object, Array, String, JSON })
vm.runInContext(fs.readFileSync(path.join(dir, 'writing-logic.js'), 'utf8'), ctx, { filename: 'writing-logic.js' })
const W = vm.runInContext('globalThis.ccWrite', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// ---- 设置
{
  const d = plain(W.normalizeCfg(undefined))
  assert.equal(d.trigger, 'badge')
  assert.equal(d.route, 'local_first')
  assert.equal(d.candidates, 1)
  assert.equal(d.last.action, 'rewrite')
  const c = plain(W.normalizeCfg({ trigger: 'x', route: 'cloud_only', candidates: 9, offHosts: [' A.com ', 'a.com', ''], last: { action: 'bogus', length: 'long', format: 'custom', customFormat: '小红书风', tone: 'nope' } }))
  assert.equal(c.trigger, 'badge')
  assert.equal(c.route, 'cloud_only')
  assert.equal(c.candidates, 3)
  assert.deepEqual(plain(c.offHosts), ['a.com'])
  assert.equal(c.last.action, 'rewrite')
  assert.equal(c.last.length, 'long')
  assert.equal(c.last.format, 'custom')
  assert.equal(c.last.customFormat, '小红书风')
  assert.equal(c.last.tone, 'auto')
  assert.equal(W.hostOffNow(c, 'www.a.com'), true)
  assert.equal(W.hostOffNow(c, 'b.com'), false)
}

// ---- 所有要求的选项都在
{
  const names = [...W.BASIC, ...W.STYLES].map((a) => a.name)
  for (const n of ['改写它', '语法检查', '翻译', '加入统计数据', '添加细节', '增加幽默', '使其礼貌', '去除专业术语', '更多信息', '更引人入胜', '更有细节', '更有说服力', '更有歉意', '使其真诚', '使其简洁', '继续写作', '缩短它', '优化它', '总结']) {
    assert.ok(names.includes(n), `缺少动作：${n}`)
  }
  const m = (k) => plain(W.MODS[k].items.map((i) => i[1]))
  assert.deepEqual(m('length'), ['自动', '短', '中等', '长'])
  assert.deepEqual(m('formality'), ['自动', '随意', '中立', '正式'])
  assert.deepEqual(m('format'), ['自动', '电子邮件', '消息', '评论', '段落', '文章', '博客文章', '想法', '大纲', '推特', 'Reddit', '脸书', '领英'])
  assert.deepEqual(m('tone'), ['自动', '热情的', '幽默的', '关心的', '谦逊的', '乐观的', '有同理心的', '坦率的', '真诚的'])
  assert.equal(W.MODS.format.custom, true)
  assert.equal(W.MODS.tone.custom, true)
}

// ---- 请求
{
  const sel = { action: 'concise', length: 'short', formality: 'formal', format: 'custom', customFormat: '小红书风', tone: 'warm', extra: '对方是客户', target: 'English' }
  const r = plain(W.buildRequest(sel, '原文', { candidates: 2, route: 'local_only', page: 'mail' }))
  assert.equal(r.kind, 'write')
  assert.equal(r.write.action, 'concise')
  assert.equal(r.write.custom_format, '小红书风')
  assert.equal(r.write.custom_tone, '')
  assert.equal(r.write.target, '', '只有翻译才带目标语言')
  assert.equal(r.write.candidates, 2)
  assert.equal(r.write.route, 'local_only')
  assert.equal(W.buildRequest({ action: 'grammar' }, 'x').kind, 'grammar')
  assert.equal(W.buildRequest({ action: 'translate', target: 'English' }, 'x').write.target, 'English')
  // 统计数据分两步
  assert.equal(W.buildRequest({ action: 'stats' }, 'x').kind, 'stats')
  const w = plain(W.buildRequest({ action: 'stats' }, 'x', { facts: [{ text: 't' }] }))
  assert.equal(w.kind, 'write')
  assert.equal(w.write.facts.length, 1)
}

// ---- 输入框
{
  const ok = { tag: 'textarea', width: 400, height: 80 }
  assert.equal(W.fieldVerdict(ok).ok, true)
  assert.equal(W.fieldVerdict({ tag: 'input', type: 'password', width: 400, height: 40 }).why, 'type')
  assert.equal(W.fieldVerdict({ tag: 'input', type: 'text', name: 'cardNumber', width: 400, height: 40 }).why, 'sensitive')
  assert.equal(W.fieldVerdict({ tag: 'input', type: 'text', placeholder: '请输入验证码', width: 400, height: 40 }).why, 'sensitive')
  assert.equal(W.fieldVerdict({ tag: 'input', type: 'text', autocomplete: 'one-time-code', width: 400, height: 40 }).why, 'sensitive')
  assert.equal(W.fieldVerdict({ tag: 'input', type: 'text', width: 150, height: 30 }).why, 'small')
  assert.equal(W.fieldVerdict({ tag: 'input', type: 'search', width: 300, height: 30 }).ok, true)
  assert.equal(W.fieldVerdict({ tag: 'textarea', readOnly: true, width: 400, height: 80 }).why, 'readonly')
  assert.equal(W.fieldVerdict({ tag: 'div', editable: true, width: 400, height: 80 }).ok, true)
  assert.equal(W.fieldVerdict({ tag: 'div', editable: false, width: 400, height: 80 }).why, 'not-editable')
}

// ---- 字数
{
  assert.equal(W.countText('王经理你好'), 5)
  assert.equal(W.countText('hello world 你好'), 4)
  assert.equal(W.countText(''), 0)
}

// ---- 差异
{
  const a = '王经理你好，下周的交付可能要推迟两天。'
  const b = '王经理，您好：下周的交付可能要延后两天。'
  const ops = plain(W.diffOps(a, b))
  assert.equal(ops.filter((o) => o.t !== 'add').map((o) => o.s).join(''), a, '删除 + 相同 拼起来是原文')
  assert.equal(ops.filter((o) => o.t !== 'del').map((o) => o.s).join(''), b, '新增 + 相同 拼起来是新文')
  const parts = plain(W.hunks(ops))
  const hs = parts.filter((p) => p.id != null)
  assert.ok(hs.length >= 2 && hs.length <= 5, `改动处数合理：${hs.length}`)
  assert.equal(W.compose(parts), b, '全部采用 = 新文')
  assert.equal(W.compose(parts, new Set(hs.map((h) => h.id))), a, '全部放弃 = 原文')
  // 放弃其中一处
  const one = W.compose(parts, new Set([hs[hs.length - 1].id]))
  assert.ok(one !== a && one !== b)
  // 相同文字没有改动
  assert.equal(plain(W.hunks(W.diffOps('一样', '一样'))).filter((p) => p.id != null).length, 0)
  // 英文按词比较
  const e = plain(W.diffOps('I like red apples', 'I like green apples'))
  assert.deepEqual(e.filter((o) => o.t !== 'eq').map((o) => `${o.t}:${o.s}`), ['del:red', 'add:green'])
  // 超长文字不卡死
  const big = '字'.repeat(3000)
  const t0 = Date.now()
  W.diffOps(big, big.replace(/字/g, (m, i) => (i % 7 === 0 ? '词' : m)))
  assert.ok(Date.now() - t0 < 5000)
}

// ---- 语法建议
{
  const text = '我们将于下周发步新产品，，欢迎关注'
  const issues = [
    { from: '发步', to: '发布', start: 6, end: 8 },
    { from: '，，', to: '，', start: 11, end: 13 },
  ]
  assert.equal(W.applyIssues(text, issues), '我们将于下周发布新产品，欢迎关注')
  assert.equal(W.applyIssues(text, issues, new Set([0])), '我们将于下周发步新产品，欢迎关注')
}

// ---- 历史
{
  let h = []
  h = plain(W.pushHistory(h, { src: 'a', out: 'b', ts: 1 }))
  h = plain(W.pushHistory(h, { src: 'a', out: 'b', ts: 2 }))
  assert.equal(h.length, 1)
  for (let i = 0; i < 60; i++) h = plain(W.pushHistory(h, { src: 's' + i, out: 'o', ts: i }))
  assert.equal(h.length, 50)
  assert.equal(h[0].src, 's59')
}

console.log('extension-writing: ok')
