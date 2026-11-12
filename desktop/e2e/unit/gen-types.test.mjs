import { test } from 'node:test'
import assert from 'node:assert/strict'
import { normalize } from '../../scripts/gen-types.mjs'

test('normalize 把 bigint 改成 number，不动别的词，末尾只留一个换行', () => {
  assert.equal(normalize('a: bigint | null, b: bigintLike\n\n\n'), 'a: number | null, b: bigintLike\n')
  assert.equal(normalize('x'), 'x\n')
})
