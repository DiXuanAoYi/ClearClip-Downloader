import assert from 'node:assert/strict'
import * as S from '../../src/utils/subedit.ts'

const c = (s, e, ...lines) => ({ startMs: s, endMs: e, lines })

// 拆分：按字符比例分配时间，也可以指定时间点
{
  const out = S.splitCue([c(0, 4000, '你好世界再见朋友')], 0, 4)
  assert.equal(out.length, 2)
  assert.deepEqual(out.map((x) => x.lines[0]), ['你好世界', '再见朋友'])
  assert.deepEqual([out[0].endMs, out[1].startMs, out[1].endMs], [2000, 2000, 4000])
  const at = S.splitCue([c(0, 4000, 'hello world')], 0, 5, 1500)
  assert.equal(at[0].endMs, 1500)
  assert.equal(at[1].lines[0], 'world')
  // 两边必须都有字
  assert.equal(S.splitCue([c(0, 1000, 'a')], 0, 1).length, 1)
  // 不改动原数组
  const src = [c(0, 4000, 'abcdef')]
  S.splitCue(src, 0, 3)
  assert.equal(src.length, 1)
}

// 合并：中文直接连，英文加空格，双语逐行合并
{
  assert.equal(S.mergeNext([c(0, 1000, '你好'), c(1000, 2000, '世界')], 0)[0].lines[0], '你好世界')
  assert.equal(S.mergeNext([c(0, 1000, 'Hello'), c(1000, 2000, 'world')], 0)[0].lines[0], 'Hello world')
  const bi = S.mergeNext([c(0, 1000, '你好', 'Hello'), c(1000, 3000, '世界', 'world')], 0)
  assert.deepEqual(bi, [c(0, 3000, '你好世界', 'Hello world')])
  assert.equal(S.mergeNext([c(0, 1000, 'x')], 0).length, 1)
}

// 插入 / 删除
{
  const a = S.insertAfter([c(0, 1000, 'a'), c(5000, 6000, 'b')], 0)
  assert.deepEqual(a[1], c(1000, 3000, ''))
  assert.equal(S.removeCue(a, 1).length, 2)
}

// 查找替换：普通 / 忽略大小写 / 正则 / 限定范围 / 非法正则
{
  const cues = [c(0, 1, 'Foo foo'), c(1, 2, 'bar'), c(2, 3, 'FOO.')]
  assert.deepEqual(S.findCues(cues, 'foo'), [0, 2])
  assert.deepEqual(S.findCues(cues, 'foo', { matchCase: true }), [0])
  const r = S.replaceAll(cues, 'foo', 'X')
  assert.equal(r.count, 3)
  assert.equal(r.cues[0].lines[0], 'X X')
  assert.equal(cues[0].lines[0], 'Foo foo')
  assert.equal(S.replaceAll(cues, 'foo', 'X', {}, [2]).count, 1)
  assert.equal(S.replaceAll(cues, 'o+', '0', { regex: true }).cues[0].lines[0], 'F0 f0')
  assert.equal(S.replaceAll(cues, '.', '!').count, 1) // 普通模式下“.”只匹配句点
  assert.equal(S.replaceAll(cues, '(', 'x', { regex: true }).count, 0)
  assert.deepEqual(S.findCues(cues, ''), [])
}

// 平移 / 修重叠
{
  assert.deepEqual(S.shiftAll([c(500, 1500, 'a')], -1000), [c(0, 500, 'a')])
  const f = S.fixOverlaps([c(0, 3000, 'a'), c(2000, 4000, 'b')])
  assert.equal(f[0].endMs, 2000)
}

// 二分查找当前条
{
  const cs = [c(0, 1000, 'a'), c(2000, 3000, 'b'), c(3000, 4000, 'c')]
  assert.equal(S.cueAt(cs, 500), 0)
  assert.equal(S.cueAt(cs, 1500), -1)
  assert.equal(S.cueAt(cs, 3000), 2)
  assert.equal(S.cueAt(cs, 9000), -1)
  assert.equal(S.cueAt([], 0), -1)
}

// 双语显示方式
{
  assert.equal(S.viewLines('你好\nHello', 'both'), '你好\nHello')
  assert.equal(S.viewLines('你好\nHello', 'first'), '你好')
  assert.equal(S.viewLines('你好\nHello', 'second'), 'Hello')
  assert.equal(S.viewLines('你好\nHello', 'swap'), 'Hello\n你好')
  assert.equal(S.viewLines('只有一行', 'second'), '只有一行')
}

// 撤销重做
{
  const h = new S.History()
  let st = [c(0, 1, 'a')]
  h.push(st)
  st = [c(0, 1, 'b')]
  h.push(st)
  st = [c(0, 1, 'c')]
  const u1 = h.undo(st)
  assert.equal(u1[0].lines[0], 'b')
  const u2 = h.undo(u1)
  assert.equal(u2[0].lines[0], 'a')
  assert.equal(h.undo(u2), null)
  assert.equal(h.redo(u2)[0].lines[0], 'b')
  assert.equal(h.canRedo, true)
  h.push(u2) // 新操作清掉重做栈
  assert.equal(h.canRedo, false)
}
console.log('subedit ops ok')
