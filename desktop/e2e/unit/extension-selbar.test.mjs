// 划词工具条的纯逻辑：朗读语言、查词还是解释、高亮的定位、导出与整理。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ Math, Number, Object, Array, String, JSON, Date, URL, encodeURIComponent })
vm.runInContext(fs.readFileSync(path.join(dir, 'selbar-logic.js'), 'utf8'), ctx, { filename: 'selbar-logic.js' })
const S = vm.runInContext('globalThis.ccSel', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// ---- 朗读语言与查词判断
{
  assert.equal(S.speechLang('Hello world'), 'en-US')
  assert.equal(S.speechLang('今天天气不错'), 'zh-CN')
  assert.equal(S.speechLang('今日はいい天気ですね'), 'ja-JP')
  assert.equal(S.speechLang('안녕하세요 반갑습니다'), 'ko-KR')
  assert.equal(S.speechLang('Привет мир'), 'ru-RU')
  assert.equal(S.speechLang('GPU 加速训练模型很快'), 'zh-CN', '中英混排以汉字为主')
  assert.equal(S.isWord('ephemeral'), true)
  assert.equal(S.isWord('machine learning'), true)
  assert.equal(S.isWord('Self-attention lets every position look at every other position.'), false)
  assert.equal(S.isWord('注意力'), true)
  assert.equal(S.isWord('注意力机制是如何工作的呢？'), false)
  assert.equal(S.searchUrl('a b&c'), 'https://www.bing.com/search?q=a%20b%26c')
  assert.equal(S.pageKey('https://example.com/a?x=1#frag'), 'https://example.com/a?x=1')
}

// ---- 高亮：记下前后文，再次打开页面时找回
{
  const page = '前言。Self-attention lets every position look at every other position. The scaling factor matters. The scaling factor also appears later.'
  const start = page.indexOf('The scaling factor also')
  const hl = S.makeHl({ full: page, start, end: start + 'The scaling factor'.length, color: 'g', url: 'https://x.test/p#h', title: 'T' })
  assert.equal(hl.text, 'The scaling factor')
  assert.equal(hl.color, 'g')
  assert.equal(hl.url, 'https://x.test/p')
  assert.ok(hl.before.endsWith('matters. ') || hl.before.length > 0)
  // 同样的文字出现两次，靠前后文找回第二处
  const at = S.locate(page, hl)
  assert.deepEqual(plain(at), [start, start + hl.text.length])
  // 页面略有变化（后文没了）也能退一步找到
  const changed = page.replace('also appears later.', '')
  assert.ok(S.locate(changed, hl))
  // 文字不在页面里
  assert.equal(S.locate('完全不同的页面内容', hl), null)
  assert.equal(S.makeHl({ full: page, start: 0, end: 2, color: 'zzz' }).color, 'y', '不认识的颜色退回默认')
}

// ---- 导出与整理
{
  const a = { id: '1', color: 'y', text: '第一条\n换行', created: 3, title: '页面 A', url: 'https://a.test/' }
  const b = { id: '2', color: 'r', text: '第二条', created: 5, note: '要核实', title: '页面 A', url: 'https://a.test/' }
  const md = S.toMarkdown([a, b], { title: '页面 A', url: 'https://a.test/' })
  assert.match(md, /^# 页面 A\n\n来源：https:\/\/a.test\/\n\n- 重点：第一条 换行\n- 存疑：第二条\n  - 笔记：要核实\n$/)
  const g = S.groupByPage({ 'https://a.test/': [a, b], 'https://b.test/': [{ id: '3', color: 'b', text: 'x', created: 9, title: 'B' }], 'https://empty.test/': [] })
  assert.deepEqual(plain(g.map((x) => x.url)), ['https://b.test/', 'https://a.test/'], '最近的在前，空的丢掉')
  const many = { 'https://a.test/': Array.from({ length: 250 }, (_, i) => ({ id: String(i), text: 't', created: i })) }
  assert.equal(S.prune(many)['https://a.test/'].length, 200)
}
console.log('extension-selbar: ok')
