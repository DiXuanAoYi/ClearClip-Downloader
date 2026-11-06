import assert from 'node:assert/strict'
import * as O from '../../src/utils/overlayShot.ts'

const blk = (o = {}) => ({ x: 100, y: 50, w: 200, h: 40, text: '开始游戏', orig: 'Start Game', fg: '#ffffff', bg: '#202020', font: 20, vertical: false, lines: 1, center: false, align: 0, ...o })
const pay = (o = {}) => ({ seq: 1, mode: 'standard', rect: [0, 0, 1000, 500], blocks: [blk()], bilingual: false, opacity: 90, fontMin: 12, cjk: true, erase: false, fontFamily: '', ...o })

// 换算：按窗口实际大小除以取景区大小，不依赖系统缩放比例
{
  assert.deepEqual(O.viewOf([1000, 500], [0, 0, 1000, 500], 1.5), { sx: 1, sy: 1 }, '窗口和取景区一样大 = 1:1，哪怕系统说缩放 150%')
  assert.deepEqual(O.viewOf([800, 400], [0, 0, 1200, 600], 1), { sx: 800 / 1200, sy: 400 / 600 })
  assert.deepEqual(O.viewOf([800, 400], undefined, 2), { sx: 0.5, sy: 0.5 }, '没有取景区时退回系统缩放比例')
  assert.deepEqual(O.viewOf([0, 0], [0, 0, 100, 100], 1.25), { sx: 0.8, sy: 0.8 })
}

// 位置、大小按比例换算；字号不低于最小字号
{
  const v = { sx: 0.5, sy: 0.5 }
  const s = O.blockStyle(blk(), pay(), v, 14)
  const pad = O.padOf(blk(), pay(), v)
  assert.ok(pad >= 2 && pad <= 7)
  // 框往外扩一圈（不再贴着原文裁字），高度是最小高度、放不下会自己长
  assert.equal(s.left, `${50 - pad}px`)
  assert.equal(s.top, `${25 - pad}px`)
  assert.equal(s.width, `${100 + pad * 2}px`)
  assert.equal(s['min-height'], `${20 + pad * 2}px`)
  assert.equal(s.height, undefined)
  assert.equal(s.overflow, 'visible')
  assert.equal(s['justify-content'], 'center', '单行上下居中')
  assert.equal(O.blockStyle(blk({ lines: 3 }), pay(), v, 14)['justify-content'], 'flex-start', '多行从上往下排')
  assert.equal(s['font-size'], '14px')
  assert.equal(O.startFont(blk({ font: 10 }), pay(), v), 12, '起始字号不低于最小字号 12')
  assert.equal(O.startFont(blk({ font: 40 }), pay(), v), 20)
}

// 对齐：靠左 / 居中 / 靠右；字幕盖在原位居中；竖排用竖排写法
{
  const v = { sx: 1, sy: 1 }
  const al = (o) => O.blockStyle(blk(o), pay(), v, 16)
  assert.equal(al({ align: 0 })['text-align'], 'left')
  assert.equal(al({ align: 1 })['text-align'], 'center')
  assert.equal(al({ align: 1 })['align-items'], 'center')
  assert.equal(al({ align: 2 })['text-align'], 'right')
  assert.equal(al({ align: 2 })['align-items'], 'flex-end')
  assert.equal(al({ center: true })['justify-content'], 'center')
  assert.equal(al({ align: undefined })['text-align'], 'left', '旧数据没有 align 按靠左')
  const vert = al({ vertical: true, align: 1 })
  assert.equal(vert['writing-mode'], 'vertical-rl')
  assert.equal(vert['text-align'], undefined)
  assert.equal(O.trStyle(blk({ vertical: true }), pay())['text-orientation'], 'upright')
  assert.equal(O.trStyle(blk({ vertical: true }), pay({ cjk: false }))['text-orientation'], 'mixed')
  assert.deepEqual(O.trStyle(blk(), pay()), {})
}

// 存图用的 SVG：每块译文都在，特殊字符被转义，双语时带原文
{
  const p = pay({ bilingual: true, blocks: [blk({ text: 'A & <B> "C"', orig: 'x<y' }), blk({ x: 0, text: '第二块', orig: '' })] })
  const svg = O.overlaySvg(p, { sx: 1, sy: 1 }, [18, 16], 1000, 500)
  assert.ok(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="500"'))
  assert.ok(svg.includes('A &amp; &lt;B&gt; &quot;C&quot;'))
  assert.ok(svg.includes('x&lt;y'))
  assert.ok(svg.includes('第二块'))
  assert.equal(svg.match(/font-size:18px/g)?.length, 1, '第一块用缩好的 18px')
  assert.ok(svg.includes('font-size:16px'))
  assert.ok(!/[\u0000-\u0008]/.test(O.escapeXml('a\u0001b')), '控制字符会让 XML 解析失败，要去掉')
  // 没有缩好的字号时用起始字号
  assert.ok(O.overlaySvg(pay(), { sx: 1, sy: 1 }, [], 100, 100).includes('font-size:20px'))
}
console.log('overlay shot ok')
