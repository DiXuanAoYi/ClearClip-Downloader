// 分层术语编辑：树 ↔ 行、加同级 / 下级、Tab / Shift+Tab、上下移动、删除、拖动、搜索。
import assert from 'node:assert/strict'
import * as T from '../../src/utils/termtree.ts'

const tree = () => [
  { source: '界面', target: 'Interface', note: '', children: [
    { source: '文件', target: 'File', note: '菜单名', children: [{ source: '打开文件', target: 'Open File', note: '' }, { source: '退出', target: 'Exit', note: '' }] },
    { source: '编辑', target: 'Edit', note: '', children: [{ source: '复制', target: 'Copy', note: '' }] },
  ] },
  { source: '角色', target: '', note: '', group: true, children: [{ source: '旅行者', target: 'Traveler', note: '' }] },
]
const shape = (rows) => rows.map((r) => '  '.repeat(r.level) + r.source).join('\n')

// 树 ↔ 行
{
  const rows = T.toRows(tree())
  assert.deepEqual(rows.map((r) => [r.level, r.source]), [[0, '界面'], [1, '文件'], [2, '打开文件'], [2, '退出'], [1, '编辑'], [2, '复制'], [0, '角色'], [1, '旅行者']])
  assert.equal(rows[6].group, true)
  assert.deepEqual(T.toTree(rows), tree(), '原样还原')
  assert.equal(T.depthOf(rows), 3)
  assert.equal(T.countTerms(tree()), 8)
  assert.deepEqual(T.pathOf(rows, 2).map((i) => rows[i].source), ['界面', '文件', '打开文件'])
}

// 加同级（放在下级后面）、加下级
{
  const rows = T.toRows(tree())
  const i = T.addSibling(rows, 1)
  assert.equal(i, 4, '“文件”的同级加在它的下级后面')
  assert.equal(rows[4].level, 1)
  const c = T.addChild(rows, 0)
  assert.equal(rows[c].level, 1)
  assert.equal(c, 7, '下级加在“界面”的下级最后')
  assert.equal(T.addSibling(rows, -1), rows.length - 1, '没有选中的行：加在最后')
}

// Tab / Shift+Tab
{
  const rows = T.toRows(tree())
  assert.equal(T.indent(rows, 4), true, '“编辑”变成“文件”的下级')
  assert.equal(shape(rows), '界面\n  文件\n    打开文件\n    退出\n    编辑\n      复制\n角色\n  旅行者')
  assert.equal(T.indent(rows, 0), false, '第一行不能再往里')
  const at = T.outdent(rows, 2)
  assert.equal(shape(rows), '界面\n  文件\n    退出\n    编辑\n      复制\n  打开文件\n角色\n  旅行者', '升一层：移到原来上级的后面')
  assert.equal(at, 5)
  assert.equal(T.outdent(rows, 0), -1)
}

// 最多 6 层
{
  const rows = [0, 1, 2, 3, 4, 5].map((l) => T.newRow(l, { source: 'L' + l }))
  assert.equal(T.addChild(rows, 5), -1)
  rows.push(T.newRow(5, { source: 'x' }))
  assert.equal(T.indent(rows, 6), false)
}

// 上下移动（连同下级）
{
  const rows = T.toRows(tree())
  assert.equal(T.move(rows, 4, -1), 1)
  assert.equal(shape(rows), '界面\n  编辑\n    复制\n  文件\n    打开文件\n    退出\n角色\n  旅行者')
  assert.equal(T.move(rows, 1, -1), -1, '已经是第一个')
  assert.equal(T.move(rows, 0, 1), 2, '“界面”和“角色”对调')
  assert.equal(shape(rows), '角色\n  旅行者\n界面\n  编辑\n    复制\n  文件\n    打开文件\n    退出')
}

// 删除
{
  let rows = T.toRows(tree())
  assert.equal(T.remove(rows, 1, false), 1)
  assert.equal(shape(rows), '界面\n  打开文件\n  退出\n  编辑\n    复制\n角色\n  旅行者', '只删“文件”：下级升一层')
  rows = T.toRows(tree())
  assert.equal(T.remove(rows, 1, true), 3)
  assert.equal(shape(rows), '界面\n  编辑\n    复制\n角色\n  旅行者')
}

// 拖动
{
  let rows = T.toRows(tree())
  assert.equal(T.drop(rows, 3, 4, 'inside'), 5)
  assert.equal(shape(rows), '界面\n  文件\n    打开文件\n  编辑\n    复制\n    退出\n角色\n  旅行者', '“退出”放进“编辑”')
  rows = T.toRows(tree())
  T.drop(rows, 7, 1, 'before')
  assert.equal(shape(rows), '界面\n  旅行者\n  文件\n    打开文件\n    退出\n  编辑\n    复制\n角色')
  rows = T.toRows(tree())
  assert.equal(T.drop(rows, 1, 2, 'inside'), -1, '不能放进自己的下级')
}

// 搜索：命中的和它们的上级
{
  const rows = T.toRows(tree())
  const { hit, shown } = T.searchRows(rows, 'copy')
  assert.deepEqual([...hit], [5])
  assert.deepEqual([...shown].sort(), [0, 4, 5])
  assert.equal(T.searchRows(rows, '  ').shown.size, 0)
}

console.log('termtree: ok')
