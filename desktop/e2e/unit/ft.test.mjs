// 文件翻译页的纯逻辑：页范围、页状态、任务合并 / 筛选 / 主按钮、搜索、剩余时间。
import assert from 'node:assert/strict'
import * as F from '../../src/utils/ft.ts'

// 页范围（和后端一致）
assert.deepEqual(F.parseRanges('3-12, 15'), [[3, 12], [15, 15]])
assert.deepEqual(F.parseRanges('5-'), [[5, Number.MAX_SAFE_INTEGER]])
assert.deepEqual(F.parseRanges('2～4；7'), [[2, 4], [7, 7]])
assert.equal(F.parseRanges(''), null)
assert.equal(F.parseRanges('abc, 0, 9-3'), null)
assert.ok(F.rangesValid('') && F.rangesValid('1-3') && !F.rangesValid('xx'))

// 页状态：译到一半、停止、失败
const P = (src, trans, done, skip = []) => ({ src, trans, done, skip })
const view = { input: 'a.pdf', target: 'x', bilingual: true, failed: 1, layout: true, pages: [P(['a', 'b'], ['甲', '乙'], true), P(['c', 'd'], ['丙', null], true), P(['e'], [null], false), P(['f'], [null], false)] }
assert.deepEqual(F.pageStates(view, true), ['done', 'fail', 'run', 'todo'])
assert.deepEqual(F.pageStates(view, false), ['done', 'fail', 'todo', 'todo'], '没在跑：没有“正在译”')
const c = F.countView(view)
assert.deepEqual(c, { pages: 4, donePages: 1, failPages: 1, segs: 6, doneSegs: 3, failedSegs: 1, todoSegs: 2 })
// 按设置不翻译的片段不算失败
const sk = { ...view, pages: [P(['H', 'body'], [null, '正文'], true, [0])] }
assert.equal(F.pageStates(sk, false)[0], 'done')
assert.deepEqual(F.countView(sk), { pages: 1, donePages: 1, failPages: 0, segs: 1, doneSegs: 1, failedSegs: 0, todoSegs: 0 })
// 只看未译 / 失败
assert.deepEqual(F.segIndexes(view.pages[1], 'fail'), [1])
assert.deepEqual(F.segIndexes(view.pages[2], 'fail'), [], '还没轮到的页没有“失败”')
assert.deepEqual(F.segIndexes(view.pages[2], 'todo'), [0])
assert.deepEqual(F.railPages(view, 'todo'), [1, 2, 3])
assert.deepEqual(F.railPages(view, 'fail'), [1])
assert.deepEqual(F.railPages(view, 'all'), [0, 1, 2, 3])

// 搜索
assert.deepEqual(F.hitPages(view, '丙'), [1])
assert.deepEqual(F.hitPages(view, 'E'), [2], '不分大小写')
assert.deepEqual(F.hitPages(view, '  '), [])
assert.deepEqual(F.splitHits('Hello hello', 'LL'), [{ t: 'He', m: false }, { t: 'll', m: true }, { t: 'o he', m: false }, { t: 'll', m: true }, { t: 'o', m: false }])
assert.deepEqual(F.splitHits('abc', ''), [{ t: 'abc', m: false }])

// 任务：进行中 + 记录合成一张列表
const job = (id, over) => ({ id, title: 'x.pdf', op: '文件翻译', status: 'running', percent: 38, output: null, error: null, note: '译第 7 页', finishedAt: null, kind: 'filetr', lane: 'ai', input: '/d/x.pdf', params: { target: '', preview: true }, ...over })
const rec = (path, state, over = {}) => ({ path, title: path.split('/').pop(), target: '简体中文', opts: { preview: true }, at: 100, output: null, last: '', error: '', state, pagesTotal: 10, pagesDone: 4, failed: 0, missing: false, ...over })
let t = F.mergeTasks([job(5, {})], [rec('/d/x.pdf', 'stopped'), rec('/d/y.pdf', 'stopped', { at: 90 }), rec('/d/z.pdf', 'generated', { at: 120, output: '/d/z.译文.pdf' })], '简体中文')
assert.deepEqual(t.map((x) => [x.title, x.state]), [['x.pdf', 'running'], ['y.pdf', 'stopped'], ['z.pdf', 'generated']], '进行中的在前，然后停下的，已生成的最后')
assert.equal(t[0].jobId, 5)
assert.equal(t[0].pagesDone, 4, '进行中的任务从记录里补页数')
// 同一个文件只留一条；快照里没写语言时按设置里的目标语言
assert.equal(t.filter((x) => x.path === '/d/x.pdf').length, 1)
// 状态映射
assert.equal(F.stateOfJob({ status: 'canceled', params: null }), 'stopped')
assert.equal(F.stateOfJob({ status: 'done', params: { preview: true } }), 'ready')
assert.equal(F.stateOfJob({ status: 'done', params: { preview: false } }), 'generated')
assert.equal(F.stateOfJob({ status: 'failed', params: null }), 'failed')
// 当前打开的文件：页数以阅读器为准
t = F.mergeTasks([], [rec('/d/x.pdf', 'stopped')], '简体中文', { key: F.taskKey('/d/x.pdf', '简体中文'), counts: F.countView(view) })
assert.deepEqual([t[0].pagesDone, t[0].pagesTotal, t[0].failed], [2, 4, 1])
// 筛选和数量
const all = F.mergeTasks([job(1, { status: 'failed', error: '模型没有返回', params: { target: '', preview: false }, input: '/d/f.pdf', title: 'f.pdf' }), job(2, { status: 'queued', input: '/d/q.pdf', title: 'q.pdf' })], [rec('/d/g.pdf', 'generated'), rec('/d/r.pdf', 'ready')], '简体中文')
assert.deepEqual(F.taskCounts(all), { all: 4, active: 1, done: 2, failed: 1 })
assert.deepEqual(F.filterTasks(all, 'failed').map((x) => x.title), ['f.pdf'])
// 每种状态的主按钮
const act = (state, o = {}) => F.primaryAction({ state, missing: false, failed: 0, ...o })
assert.deepEqual(['queued', 'running', 'stopped', 'failed', 'ready', 'generated'].map((s) => act(s)), ['cancel', 'stop', 'resume', 'retry', 'generate', 'open'])
assert.equal(act('ready', { failed: 2 }), 'retry', '待生成但有失败的段：先重试')
assert.equal(act('stopped', { missing: true }), 'none', '源文件不在了：没法继续')
assert.equal(act('running', { missing: true }), 'stop')

// 剩余时间
assert.equal(F.etaSeconds(0, 10, 60_000, 12), null, '进度动得太少不估')
assert.equal(F.etaSeconds(0, 0, 60_000, 50), 60)
assert.equal(F.etaText(30), '约剩不到 1 分钟')
assert.equal(F.etaText(240), '约剩 4 分钟')
assert.equal(F.etaText(5400), '约剩 1 小时 30 分钟')
assert.equal(F.etaText(null), '')
assert.equal(F.sizeText(18, 340, 31000), '18 页 · 约 340 段 · 约 3.1 万字')
assert.equal(F.baseName('C:\\a\\b.pdf'), 'b.pdf')

console.log('ft: ok')
