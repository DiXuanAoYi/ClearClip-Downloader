// 后台消息的入口检查（bg-guard.js）：发送者必须是本扩展、needTab 的消息必须来自内容脚本、字段白名单与长度上限。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({})
vm.runInContext(fs.readFileSync(path.join(dir, 'bg-guard.js'), 'utf8'), ctx, { filename: 'bg-guard.js' })
const plain = (v) => JSON.parse(JSON.stringify(v))
const G = vm.runInContext('ccGuard', ctx)
const check = (msg, sender, id = 'self-id') => plain(G.check(msg, sender, id))
const tab = { id: 1, url: 'https://example.com/' }
const content = { id: 'self-id', tab }
const page = { id: 'self-id' } // 扩展页面（弹窗、总览）：没有 tab
const LIM = plain(G.LIMITS)

test('发送者不是本扩展：统一返回 forbidden', () => {
  for (const sender of [{ id: 'other-ext', tab }, { tab }, undefined, null]) {
    const r = check({ type: 'cc-engines' }, sender)
    assert.equal(r.ok, false)
    assert.equal(r.code, 'forbidden')
    assert.match(r.error, /本扩展/)
  }
  assert.equal(check({ type: 'cc-engines' }, content, '').code, 'forbidden', '拿不到本扩展 id 时也拒绝')
})

test('needTab：内容脚本发的才行，扩展页面发的返回 no_tab', () => {
  for (const type of ['cc-sniffed', 'cc-send-page', 'cc-engines', 'cc-ai-status', 'cc-capture']) {
    assert.equal(check({ type }, content).ok, true, type)
    const r = check({ type }, page)
    assert.equal(r.code, 'no_tab', type)
  }
  // 不需要标签页的（扩展页面和内容脚本都能发）
  assert.equal(check({ type: 'cc-app-lang' }, page).ok, true)
  assert.equal(check({ type: 'cc-app-lang' }, content).ok, true)
})

test('needTab 的集合与原来的正则一致（没有悄悄放宽或收紧）', () => {
  const needTab = Object.entries(plain(G.SPEC)).filter(([, s]) => s.needTab).map(([t]) => t.replace(/^cc-/, ''))
  const old = 'sniffed|send-page|send-media|send-batch|engines|save|capture|area|shot|ai-status|ai-run|ai-job|ai-cancel|note|tr|stub-load'.split('|')
  assert.deepEqual(needTab.sort(), old.sort())
})

test('没登记的消息类型：返回 unknown，不放行', () => {
  assert.equal(check({ type: 'cc-nope' }, content).code, 'unknown')
  assert.equal(check({ type: 'toString' }, content).code, 'unknown', '不能被原型上的名字绕过')
  assert.equal(check(null, content).code, 'unknown')
  assert.equal(check({}, content).code, 'unknown')
})

test('cc-ai-run：只留白名单字段，丢掉多余的（terms / guide 等由桌面端自己填）', () => {
  const r = check({ type: 'cc-ai-run', req: { kind: 'summary', title: 'T', lines: [{ t: 0, text: 'hi', extra: 1 }], terms: [{ x: 1 }], guide: 'evil', __proto__x: 1 } }, content)
  assert.equal(r.ok, true)
  assert.deepEqual(r.msg.req, { kind: 'summary', title: 'T', lines: [{ t: 0, text: 'hi' }] })
})

test('cc-ai-run：缺 kind、类型不对、req 不是对象都拒绝', () => {
  for (const req of [{}, { title: 'x' }, { kind: 5 }, { kind: 'summary', lines: 'abc' }, { kind: 'summary', lines: [null] }, 'str', [1], 7]) {
    const r = check({ type: 'cc-ai-run', req }, content)
    assert.equal(r.ok, false, JSON.stringify(req))
    assert.equal(r.code, 'bad_request')
    assert.match(r.error, /^请求无效：/)
  }
  assert.equal(check({ type: 'cc-ai-run' }, content).code, 'bad_request', '缺 req')
})

test('cc-ai-run：长度上限——总字数与后端 MAX_CHARS（60 万）一致，自定义提示词 2 万，图片 190 万', () => {
  assert.equal(LIM.AI_MAX_CHARS, 600_000)
  assert.equal(LIM.AI_SYSTEM, 20_000)
  assert.equal(LIM.AI_IMAGE, 1_900_000)
  const line = 'x'.repeat(92) // 每行算 92 + 8 = 100 字
  const ok = Array.from({ length: 6000 }, (_, i) => ({ t: i, text: line })) // 刚好 60 万
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'summary', lines: ok } }, content).ok, true)
  const over = [...ok, { t: 7000, text: 'y' }]
  const r = check({ type: 'cc-ai-run', req: { kind: 'summary', lines: over } }, content)
  assert.equal(r.ok, false)
  assert.match(r.error, /内容太多/)
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'translate', text: 'a'.repeat(600_001) } }, content).ok, false)
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'summary', system: 'a'.repeat(20_001) } }, content).ok, false)
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'summary', system: 'a'.repeat(20_000) } }, content).ok, true)
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'imgtrans', image: 'a'.repeat(1_900_001) } }, content).ok, false)
  // 评论也计入总字数
  const cm = Array.from({ length: 5000 }, () => ({ author: 'a', text: 'x'.repeat(200), likes: 1, t: -1, replies: 0 }))
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'comments', comments: cm } }, content).ok, false)
  // write 选项只限大小
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'write', text: 'hi', write: { action: 'rewrite', tone: 'warm' } } }, content).ok, true)
  assert.equal(check({ type: 'cc-ai-run', req: { kind: 'write', write: { extra: 'a'.repeat(LIM.OBJ_JSON) } } }, content).ok, false)
})

test('其他消息：缺字段、类型不对、超长、列表太多', () => {
  assert.equal(check({ type: 'cc-send-media' }, content).code, 'bad_request')
  assert.equal(check({ type: 'cc-send-media', url: 5 }, content).code, 'bad_request')
  assert.equal(check({ type: 'cc-send-media', url: 'https://a.com/' + 'x'.repeat(LIM.URL) }, content).code, 'bad_request')
  assert.equal(check({ type: 'cc-send-media', url: 'https://a.com/v.mp4', junk: 1 }, content).msg.junk, undefined, '多余字段被丢掉')
  assert.equal(check({ type: 'cc-ai-job', id: 'abc' }, content).ok, true)
  assert.equal(check({ type: 'cc-ai-job', id: 12 }, content).ok, true)
  assert.equal(check({ type: 'cc-ai-job', id: { a: 1 } }, content).ok, false)
  assert.equal(check({ type: 'cc-ai-cancel' }, content).ok, false)
  assert.equal(check({ type: 'cc-tr', texts: Array(LIM.TR_ITEMS + 1).fill('a') }, content).ok, false)
  assert.equal(check({ type: 'cc-tr', texts: ['a'.repeat(LIM.TR_TEXT + 1)] }, content).ok, false)
  assert.equal(check({ type: 'cc-tr', texts: ['hello', 'world'], engine: 'google', target: 'zh-CN', page: { host: 'a.com', title: 't', sample: 's', zz: 1 } }, content).msg.page.zz, undefined)
  assert.equal(check({ type: 'cc-send-batch', items: Array(LIM.BATCH_ITEMS + 1).fill({ kind: 'image', url: 'https://a.com/1.png' }) }, content).ok, false)
  assert.equal(check({ type: 'cc-send-batch', items: [{ kind: 'image', url: 'https://a.com/1.png' }, 'bad'] }, content).ok, false, '列表里有坏元素：整条拒绝，不悄悄丢掉以免错位')
  assert.equal(check({ type: 'cc-run', tabId: '3', action: 'vsum' }, page).ok, false, 'tabId 必须是数字')
  assert.equal(check({ type: 'cc-run', tabId: 3, action: 'vsum' }, page).ok, true)
  assert.equal(check({ type: 'cc-note', text: 'a'.repeat(LIM.NOTE + 1) }, content).ok, false)
  assert.equal(check({ type: 'cc-shot', vw: 100 }, content).code, 'bad_request', '缺 rect')
  assert.equal(check({ type: 'cc-shot', rect: { x: 1, y: 2, w: 30, h: 40 }, vw: 1200 }, content).ok, true)
  assert.equal(check({ type: 'cc-ai-name', items: Array(LIM.BATCH_ITEMS + 1).fill({ kind: 'image' }) }, page).ok, false)
})

test('选中文字 / 翻译窗文字：超长的截断（处理函数本来就只取前 5000 字），不拒绝', () => {
  const r = check({ type: 'cc-wt-sel', text: 'a'.repeat(50_000), rect: { left: 1, right: 2, top: 3, bottom: 4 } }, content)
  assert.equal(r.ok, true)
  assert.equal(r.msg.text.length, 5000)
  assert.equal(check({ type: 'cc-trbox', text: 'b'.repeat(9999), engines: ['google'] }, page).msg.text.length, 5000)
})

test('cc-vocab：单词是文字或对象；cc-videocache 负载限大小', () => {
  assert.equal(check({ type: 'cc-vocab', op: 'clip', word: 'hello', data: 'x' }, content).ok, true)
  assert.equal(check({ type: 'cc-vocab', op: 'add', word: { word: 'a', sentence: 's' } }, content).ok, true)
  assert.equal(check({ type: 'cc-vocab', op: 'add', word: 5 }, content).ok, false)
  assert.equal(check({ type: 'cc-vocab', op: 'unfamiliar', words: Array(401).fill('a') }, content).ok, false)
  assert.equal(check({ type: 'cc-videocache', op: 'put', payload: { summary: { key: 'k', data: {} } } }, content).ok, true)
  assert.equal(check({ type: 'cc-videocache', op: 'put', payload: { big: 'x'.repeat(LIM.VCACHE_JSON) } }, content).ok, false)
})

test('cc-bypass 这类只要确认发送者的消息：fromSelf', () => {
  assert.equal(G.fromSelf({ id: 'self-id' }, 'self-id'), true)
  assert.equal(G.fromSelf({ id: 'x' }, 'self-id'), false)
  assert.equal(G.fromSelf(undefined, 'self-id'), false)
})
