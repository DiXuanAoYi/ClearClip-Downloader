import test from 'node:test'
import assert from 'node:assert/strict'
import { countColors, isExempt, compare } from '../../scripts/lint-colors.mjs'

test('统计色值并忽略注释', () => {
  assert.equal(countColors('a{color:#fff;background:#1a2b3c;border:1px solid rgba(0,0,0,.2)}'), 3)
  assert.equal(countColors('/* #fff */ a{color:var(--cc-fg)}'), 0)
  assert.equal(countColors('// #abc\nconst x = 1'), 0)
  assert.equal(countColors('a{color:#12345678}'), 1)
})

test('豁免目录', () => {
  assert.ok(isExempt('src/styles/theme.css'))
  assert.ok(isExempt('src/components/ui/CcButton.vue'))
  assert.ok(!isExempt('src/components/Other.vue'))
})

test('只许减不许增', () => {
  assert.deepEqual(compare({ a: 3 }, { a: 3 }), [])
  assert.deepEqual(compare({ a: 2 }, { a: 3 }), [])
  assert.equal(compare({ a: 4 }, { a: 3 }).length, 1)
  assert.equal(compare({ b: 1 }, {}).length, 1)
})
