// 视频摘要的纯逻辑：字幕解析与整理、章节、网址、导图布局、导出、缓存。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ URL, URLSearchParams, console, navigator: { language: 'zh-CN' } })
ctx.globalThis = ctx
vm.runInContext(fs.readFileSync(path.join(dir, 'vsum-logic.js'), 'utf8'), ctx, { filename: 'vsum-logic.js' })
const V = vm.runInContext('ccVsum', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// ---- 时间
assert.equal(V.fmtClock(0), '0:00')
assert.equal(V.fmtClock(65_900), '1:05')
assert.equal(V.fmtClock(3_723_000), '1:02:03')
assert.equal(V.parseClock('1:05'), 65_000)
assert.equal(V.parseClock('01:02:03'), 3_723_000)
assert.equal(V.parseClock('83'), 83_000)
assert.equal(V.parseClock('00:01,500'), 1500)
assert.equal(V.parseClock('abc'), null)
assert.equal(V.parseClock(''), null)

// ---- 各种字幕格式
assert.deepEqual(
  plain(V.parseJson3({ events: [{ tStartMs: 0, dDurationMs: 1000 }, { tStartMs: 1000, dDurationMs: 2000, segs: [{ utf8: 'Hello' }, { utf8: ' world' }] }, { tStartMs: 3000, segs: [{ utf8: '\n' }] }] })),
  [{ t: 1000, end: 3000, text: 'Hello world' }],
)
assert.deepEqual(plain(V.parseBilibili({ body: [{ from: 1.5, to: 3, content: '你好\n世界' }, { from: 4, to: 5, content: '  ' }] })), [{ t: 1500, end: 3000, text: '你好 世界' }])
const vtt = 'WEBVTT\n\nNOTE x\n\n1\n00:00:01.000 --> 00:00:02.500 align:start\n<c.colorE5E5E5>Hello</c> &amp; welcome\n\n00:03.000 --> 00:04.000\nsecond\nline\n'
assert.deepEqual(plain(V.parseSubtitleText(vtt)), [{ t: 1000, end: 2500, text: 'Hello & welcome' }, { t: 3000, end: 4000, text: 'second line' }])
const srt = '﻿1\r\n00:00:01,000 --> 00:00:02,000\r\n你好\r\n\r\n2\r\n01:00:00,500 --> 01:00:02,000\r\n一小时后\r\n'
assert.deepEqual(plain(V.parseSubtitleText(srt)), [{ t: 1000, end: 2000, text: '你好' }, { t: 3_600_500, end: 3_602_000, text: '一小时后' }])
assert.deepEqual(plain(V.parseSubtitleText('不是字幕')), [])
assert.deepEqual(plain(V.fromTextTrackCues([{ startTime: 1.2, endTime: 2, text: '<b>A</b>\nB' }, { startTime: 3, endTime: 4, text: '' }])), [{ t: 1200, end: 2000, text: 'A B' }])

// ---- 整理成句：去滚动重复，合并碎片
{
  const rolling = [
    { t: 0, end: 1500, text: 'so today we' },
    { t: 1000, end: 3000, text: 'so today we are going to' },
    { t: 2500, end: 4000, text: 'so today we are going to talk about rust.' },
    { t: 5000, end: 6000, text: 'It is fast.' },
    { t: 5000, end: 6000, text: 'It is fast.' },
  ]
  const lines = plain(V.normalizeLines(rolling))
  assert.deepEqual(lines, [{ t: 0, text: 'so today we are going to talk about rust.' }, { t: 5000, text: 'It is fast.' }])
  // 中文不加空格；碎片合到一句话结束；超过上限就另起一行
  const zh = plain(V.normalizeLines([{ t: 0, text: '今天我们' }, { t: 1000, text: '来聊聊视频摘要。' }, { t: 3000, text: '第二句' }]))
  assert.deepEqual(zh, [{ t: 0, text: '今天我们来聊聊视频摘要。' }, { t: 3000, text: '第二句' }])
  const long = Array.from({ length: 30 }, (_, i) => ({ t: i * 1000, text: `word${i} word${i}b word${i}c` }))
  const split = plain(V.normalizeLines(long))
  assert.ok(split.length > 1 && split.every((l) => l.text.length <= 160), split.map((l) => l.text.length).join())
  assert.ok(split.every((l, i) => i === 0 || l.t > split[i - 1].t))
  assert.deepEqual(plain(V.normalizeLines([])), [])
  // 乱序输入先排序
  assert.deepEqual(plain(V.normalizeLines([{ t: 5000, text: 'b.' }, { t: 0, text: 'a.' }])).map((l) => l.text), ['a.', 'b.'])
}

// ---- 简介里的章节
{
  const ok = '精彩视频\n0:00 开场\n01:30 - 第一部分\n• 5:20 | 第二部分\n1:02:03 结尾\n谢谢观看'
  assert.deepEqual(plain(V.parseDescriptionChapters(ok)).map((c) => [c.startMs, c.title]), [[0, '开场'], [90_000, '第一部分'], [320_000, '第二部分'], [3_723_000, '结尾']])
  assert.deepEqual(plain(V.parseDescriptionChapters('开场 0:00\n中间 2:00\n结尾 5:00')).map((c) => c.title), ['开场', '中间', '结尾'])
  assert.deepEqual(plain(V.parseDescriptionChapters('0:00 a\n0:30 b')), [], '少于 3 章不算')
  assert.deepEqual(plain(V.parseDescriptionChapters('0:10 a\n0:30 b\n1:00 c')), [], '必须从 0:00 开始')
  assert.deepEqual(plain(V.parseDescriptionChapters('0:00 a\n2:00 b\n1:00 c')), [], '时间必须递增')
}

// ---- 网址
{
  const id = (u) => plain(V.videoIdOf(u))
  assert.deepEqual(id('https://www.youtube.com/watch?v=abc123XYZ_-&t=5'), { site: 'youtube', id: 'abc123XYZ_-' })
  assert.deepEqual(id('https://youtu.be/abc123XYZ_-?si=1'), { site: 'youtube', id: 'abc123XYZ_-' })
  assert.deepEqual(id('https://m.youtube.com/shorts/abcdefghijk'), { site: 'youtube', id: 'abcdefghijk' })
  assert.deepEqual(id('https://www.bilibili.com/video/BV1xx411c7mD/?p=3&spm=1'), { site: 'bilibili', id: 'BV1xx411c7mD', part: 3 })
  assert.equal(id('https://www.youtube.com/'), null)
  assert.equal(id('https://example.com/video/BV1'), null)
  assert.equal(id('not a url'), null)
  assert.equal(V.timeLink('https://www.youtube.com/watch?v=abc123XYZ_-&list=x&t=5', 83_900), 'https://www.youtube.com/watch?v=abc123XYZ_-&t=83s')
  assert.equal(V.timeLink('https://youtu.be/abc123XYZ_-', 0), 'https://www.youtube.com/watch?v=abc123XYZ_-')
  assert.equal(V.timeLink('https://www.bilibili.com/video/BV1xx411c7mD/?p=2', 61_000), 'https://www.bilibili.com/video/BV1xx411c7mD/?p=2&t=61')
  assert.equal(V.timeLink('https://example.com/a#x', 5000), 'https://example.com/a')
}

// ---- YouTube 页面数据
{
  const tracks = [
    { baseUrl: 'https://www.youtube.com/api/timedtext?v=1\\u0026lang=en', languageCode: 'en', name: { simpleText: 'English' } },
    { baseUrl: 'https://www.youtube.com/api/timedtext?v=1&lang=zh&kind=asr', languageCode: 'zh-Hans', kind: 'asr', name: { runs: [{ text: '中文（自动生成）' }] } },
  ]
  const html = `<script>var x={"foo":"}]"};"captionTracks":${JSON.stringify(tracks)},"audioTracks":[]; "title":"A \\"quoted\\" 标题","shortDescription":"0:00 开场\\n1:00 中间\\n2:00 结尾","lengthSeconds":"245"</script>`
  const p = plain(V.parseYouTubePage(html))
  assert.equal(p.tracks.length, 2)
  assert.equal(p.tracks[0].url, 'https://www.youtube.com/api/timedtext?v=1&lang=en')
  assert.deepEqual([p.tracks[1].auto, p.tracks[1].name, p.tracks[1].lang], [true, '中文（自动生成）', 'zh-Hans'])
  assert.equal(p.title, 'A "quoted" 标题')
  assert.equal(p.description, '0:00 开场\n1:00 中间\n2:00 结尾')
  assert.equal(p.lengthSec, 245)
  assert.deepEqual(plain(V.parseYouTubePage('<html>没有字幕</html>')), { tracks: [], title: '', description: '', lengthSec: 0 })
  assert.deepEqual(plain(V.parseYouTubePage('"captionTracks":[broken')).tracks, [])
  assert.equal(V.sliceJson('x {"a":"}"} y', 2), '{"a":"}"}')
  assert.equal(V.sliceJson('abc', 0), null)
  // 选轨：想要的语言里人工优先，没有就退而求其次
  const t = plain(p.tracks)
  assert.equal(V.pickTrack(t, ['en']).lang, 'en')
  assert.equal(V.pickTrack(t, ['zh-CN']).lang, 'zh-Hans')
  assert.equal(V.pickTrack(t, ['ja']).lang, 'zh-Hans', '其次浏览器语言（中文）')
  assert.equal(V.pickTrack([{ lang: 'fr', auto: true }, { lang: 'de', auto: false }], ['ja']).lang, 'de', '没有想要的语言就拿人工字幕')
  assert.equal(V.pickTrack([], ['en']), null)
}

// ---- 导图
{
  const md = '# 主题\n- 分支一\n  - 细节甲\n  - 细节乙\n    - 更深\n- 分支二\n'
  const tree = plain(V.outlineToTree(md))
  assert.equal(tree.text, '主题')
  assert.deepEqual(tree.children.map((c) => c.text), ['分支一', '分支二'])
  assert.deepEqual(tree.children[0].children.map((c) => c.text), ['细节甲', '细节乙'])
  assert.equal(tree.children[0].children[1].children[0].text, '更深')
  assert.equal(plain(V.outlineToTree('- a\n- b', '备用标题')).text, '备用标题')
  const lay = plain(V.layoutMindmap(V.outlineToTree(md)))
  assert.equal(lay.nodes.length, 6)
  assert.equal(lay.links.length, 5)
  const root = lay.nodes[0]
  assert.equal(root.depth, 0)
  // 子节点在父节点右边；同一列的节点不重叠
  for (const l of lay.links) assert.ok(l.x2 > l.x1)
  for (const d of [1, 2]) {
    const col = lay.nodes.filter((n) => n.depth === d).sort((a, b) => a.y - b.y)
    for (let i = 1; i < col.length; i++) assert.ok(col[i].y >= col[i - 1].y + col[i - 1].h, `depth ${d} overlap`)
  }
  assert.ok(lay.nodes.every((n) => n.x >= 0 && n.y >= 0 && n.x + n.w <= lay.width + 0.5 && n.y + n.h <= lay.height + 0.5))
  const svg = V.mindmapSvg(V.layoutMindmap(V.outlineToTree('# <a&b>\n- "q"\n- r')))
  assert.ok(svg.startsWith('<svg') && svg.endsWith('</svg>'))
  assert.ok(svg.includes('&lt;a&amp;b&gt;') && svg.includes('&quot;q&quot;') && !svg.includes('<a&b>'))
  // 长文字会折行，宽度有上限
  const wide = plain(V.layoutMindmap(V.outlineToTree('# 根\n- ' + '很长的一句话'.repeat(20))))
  assert.ok(wide.nodes[1].lines.length > 1 && wide.nodes[1].w < 260)
}

// ---- 搜索与定位
{
  const lines = [{ t: 0, text: 'Hello World' }, { t: 5000, text: '第二行 世界' }, { t: 9000, text: 'hello again' }]
  assert.deepEqual(plain(V.searchLines(lines, 'hello')), [0, 2])
  assert.deepEqual(plain(V.searchLines(lines, 'HELLO again')), [2])
  assert.deepEqual(plain(V.searchLines(lines, '   ')), [])
  assert.equal(V.lineAt(lines, -1), -1)
  assert.equal(V.lineAt(lines, 0), 0)
  assert.equal(V.lineAt(lines, 8999), 1)
  assert.equal(V.lineAt(lines, 99999), 2)
  assert.equal(V.lineAt([], 5), -1)
  const ch = [{ startMs: 0 }, { startMs: 60_000 }, { startMs: 120_000 }]
  assert.deepEqual([0, 59_999, 60_000, 500_000].map((m) => V.chapterAt(ch, m)), [0, 0, 1, 2])
  assert.equal(V.chapterAt([], 5), -1)
}

// ---- 导出
{
  const d = {
    title: '我的 / 视频',
    url: 'https://www.youtube.com/watch?v=abc123XYZ_-',
    result: { summary: '概述', points: ['要点一', '要点二'], keywords: ['AI 工具', '效率'], chapters: [{ title: '开场', startMs: 0 }, { title: '正文', startMs: 90_000 }] },
    quotes: [{ text: '好句子', t: 30_000 }],
    cards: [{ q: '什么是 X？', a: '是 Y', t: 10_000 }],
    outline: '# 主题\n- 一\n  - 二',
    lines: [{ t: 0, text: '第一句' }, { t: 4000, text: '第二句' }],
  }
  const md = V.toMarkdown(d)
  assert.ok(md.startsWith('# 我的 / 视频\n'))
  assert.ok(md.includes('#AI_工具 #效率'))
  assert.ok(md.includes('- [1:30](https://www.youtube.com/watch?v=abc123XYZ_-&t=90s) 正文'))
  assert.ok(md.includes('> 好句子 — [0:30](https://www.youtube.com/watch?v=abc123XYZ_-&t=30s)'))
  assert.ok(md.includes('1. **什么是 X？**') && md.includes('## 导图大纲\n\n- 一\n  - 二'))
  assert.ok(!md.includes('## 文稿'), '默认不带文稿')
  const full = V.toMarkdown({ ...d, includeTranscript: true })
  assert.ok(full.includes('[0:04](https://www.youtube.com/watch?v=abc123XYZ_-&t=4s) 第二句'))
  assert.ok(!/\n{3,}/.test(full))
  const bare = V.toMarkdown({ title: 't', result: { summary: 's', chapters: [{ title: 'c', startMs: 61_000 }] } })
  assert.ok(bare.includes('- 1:01 c') && !bare.includes('来源'))
  assert.equal(V.toMarkdown({}).trim(), '# 视频笔记')
  assert.equal(V.toSrt(d.lines), '1\n00:00:00,000 --> 00:00:04,000\n第一句\n\n2\n00:00:04,000 --> 00:00:08,000\n第二句\n')
  assert.equal(V.toTxt(d.lines), '[0:00] 第一句\n[0:04] 第二句\n')
  assert.equal(V.cardsToCsv([{ q: 'a\tb', a: 'x\ny', t: 65_000 }, { q: 'q2', a: 'a2' }]), 'a b\tx y\t1:05\nq2\ta2\t\n')
  assert.equal(V.fileName('a/b:c*?"<>|d', 'md'), 'a b c d.md')
  assert.equal(V.fileName('', 'md'), '视频笔记.md')
  assert.ok(V.fileName('长'.repeat(200), 'md').length <= 83)
}

// ---- 缓存
{
  const a = [{ t: 0, text: 'x' }]
  const k = V.cacheKey(a, 'summary', 'brief', '中文')
  assert.equal(k, V.cacheKey([{ t: 0, text: 'x' }], 'summary', 'brief', '中文'))
  for (const other of [V.cacheKey([{ t: 0, text: 'y' }], 'summary', 'brief', '中文'), V.cacheKey(a, 'mindmap', 'brief', '中文'), V.cacheKey(a, 'summary', 'detailed', '中文'), V.cacheKey(a, 'summary', 'brief', 'English')]) assert.notEqual(k, other)
  const entries = {}
  for (let i = 0; i < 60; i++) entries['k' + i] = { ts: i, data: 'x'.repeat(10) }
  const kept = plain(V.pruneCache(entries, 40))
  assert.equal(Object.keys(kept).length, 40)
  assert.ok('k59' in kept && !('k0' in kept), '留下最新的')
  const big = { a: { ts: 3, data: 'x'.repeat(600) }, b: { ts: 2, data: 'x'.repeat(600) }, c: { ts: 1, data: 'x'.repeat(100) } }
  assert.deepEqual(Object.keys(plain(V.pruneCache(big, 40, 800))), ['a', 'c'], '太大时跳过放不下的')
  assert.deepEqual(plain(V.pruneCache(undefined)), {})
}

// ---- timedtext XML、视频键
{
  const a = V.parseTimedTextXml('<?xml version="1.0"?><transcript><text start="1.5" dur="2">Hi &amp; bye</text><text start="4" dur="1">&#39;ok&#39;</text></transcript>')
  assert.deepEqual(plain(a), [{ t: 1500, end: 3500, text: 'Hi & bye' }, { t: 4000, end: 5000, text: "'ok'" }])
  const b = V.parseTimedTextXml('<timedtext><body><p t="1200" d="3000">你好<s>世界</s></p></body></timedtext>')
  assert.deepEqual(plain(b), [{ t: 1200, end: 4200, text: '你好世界' }])
  assert.deepEqual(plain(V.parseTimedTextXml('')), [])
  assert.equal(V.videoKey('https://www.youtube.com/watch?v=abc123&t=5s#x'), 'youtube:abc123')
  assert.equal(V.videoKey('https://youtu.be/abc123?si=1'), 'youtube:abc123')
  assert.equal(V.videoKey('https://www.bilibili.com/video/BV1xx?p=3'), 'bilibili:BV1xx:p3')
  assert.equal(V.videoKey('https://www.bilibili.com/video/BV1xx'), 'bilibili:BV1xx')
  assert.equal(V.videoKey('https://site.test/play/1?a=b#frag'), 'url:https://site.test/play/1?a=b')
}

// ---- 笔记
{
  const n1 = V.makeNote({ key: 'youtube:a', url: 'https://www.youtube.com/watch?v=a', title: 'T', t: 65_900, text: ' 第一句 ' })
  assert.equal(n1.text, '第一句')
  assert.equal(n1.t, 65_900)
  assert.equal(V.makeNote({ text: '  ' }), null)
  let list = V.addNote([], n1)
  list = V.addNote(list, V.makeNote({ key: 'youtube:a', url: n1.url, title: 'T', t: 65_900, text: '第一句' }))
  assert.equal(list.length, 1, '同一时刻同样文字不重复')
  list = V.addNote(list, V.makeNote({ key: 'youtube:a', url: n1.url, title: 'T', t: 5000, text: '更早的一句', kind: 'note' }))
  assert.deepEqual([...V.sortNotes(list).map((n) => n.text)], ['更早的一句', '第一句'])
  assert.equal(V.searchNotes(list, '早').length, 1)
  const md = V.notesToMarkdown(list, { title: '汇总' })
  assert.ok(md.includes('# 汇总') && md.includes('## T') && md.includes('- [0:05](https://www.youtube.com/watch?v=a&t=5s) 更早的一句') && md.includes('> 第一句'), md)
  const doc = V.studyDoc({ title: 'T', url: n1.url, result: { summary: '概要' }, lines: [] }, list)
  assert.ok(doc.startsWith('---\ntitle: "T"') && doc.includes('## 我的笔记') && doc.includes('概要'), doc)
  const lines = [{ t: 0, text: 'a' }, { t: 5000, text: 'b' }, { t: 9000, text: 'c' }]
  const c = V.contextAround(lines, 5200, 1)
  assert.equal(c.target, 'b')
  assert.ok(c.before.includes('a') && c.after.includes('c'))
  const vl = (...a) => [...V.viewLine(...a)]
  assert.deepEqual(vl('x', 'y', 'both'), ['x', 'y'])
  assert.deepEqual(vl('x', 'y', 'tr'), ['y', ''])
  assert.deepEqual(vl('x', 'y', 'orig'), ['x', ''])
  assert.deepEqual(vl('x', null, 'both'), ['x', ''])
}

console.log('extension-vsum: ok')

// ---- 搜索高亮 / 回答引用校验
{
  assert.deepEqual(plain(V.splitMarks('Hello hello World', 'hello')), [{ s: 'Hello', hit: true }, { s: ' ', hit: false }, { s: 'hello', hit: true }, { s: ' World', hit: false }])
  assert.deepEqual(plain(V.splitMarks('abc', '  ')), [{ s: 'abc', hit: false }])

  const lines = [{ t: 0, text: 'intro' }, { t: 60000, text: 'the key point' }, { t: 120000, text: 'wrap up' }]
  const c = V.citations('结论在 [1:00]，另见 [1:00-1:10] 和 [99:00]。', lines)
  assert.equal(c.refs.length, 1)
  assert.equal(c.refs[0].text, 'the key point')
  assert.equal(c.bad, 1)
  assert.equal(c.unsupported, false)
  assert.equal(V.citations('随便说说', lines).unsupported, true)
  assert.equal(V.citations('字幕中未找到相关内容', lines).unsupported, false)
}
