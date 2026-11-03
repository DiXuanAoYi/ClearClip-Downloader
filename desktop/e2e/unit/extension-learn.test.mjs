import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ console })
ctx.globalThis = ctx
vm.runInContext(fs.readFileSync(path.join(dir, 'learn-logic.js'), 'utf8'), ctx, { filename: 'learn-logic.js' })
const L = ctx.ccLearn
const plain = (v) => JSON.parse(JSON.stringify(v))

test('segment splits words and keeps punctuation so the pieces rebuild the sentence', () => {
  const s = "Don't stop, it's a well-known fact."
  const parts = L.segment(s)
  assert.equal(parts.map((p) => p.text).join(''), s)
  assert.deepEqual(plain(parts.filter((p) => p.word).map((p) => p.text)), ["Don't", 'stop', "it's", 'a', 'well-known', 'fact'])
  assert.deepEqual(plain(L.segment('你好 world').filter((p) => p.word).map((p) => p.text)), ['world'])
})

test('norm lowercases and trims quotes', () => {
  assert.equal(L.norm("'Hello’"), 'hello')
  assert.equal(L.norm('It’s'), "it's")
})

test('uniqueWords skips short words and duplicates', () => {
  const w = L.uniqueWords([{ t: 0, text: 'The cat and the Cat sat' }, { t: 1, text: 'A big cat' }])
  assert.deepEqual(plain(w), ['the', 'cat', 'and', 'sat', 'big'])
})

test('levels map to rank thresholds', () => {
  assert.equal(L.levelRank(0), 0)
  assert.equal(L.levelRank(3), 2000)
  assert.equal(L.levelRank(99), 0)
})

test('span ends at the next line but never later than 8 seconds', () => {
  const lines = [{ t: 1000, text: 'a' }, { t: 3000, text: 'b' }, { t: 60000, text: 'c' }]
  assert.deepEqual(plain(L.span(lines, 0)), { start: 1000, end: 3000 })
  assert.equal(L.span(lines, 1).end, 11000)
  assert.equal(L.span(lines, 2).end, 64000)
  assert.equal(L.span(lines, 0, 500).start, 1500)
  assert.equal(L.span(lines, 9), null)
})

test('dictation diff marks missed words and scores', () => {
  const d = L.dictationDiff('The quick brown fox jumps.', 'the quick fox jump')
  assert.deepEqual(plain(d.missed), ['brown', 'jumps'])
  assert.equal(d.score, 60)
  assert.equal(d.parts.map((p) => p.text).join(''), 'The quick brown fox jumps.')
  assert.equal(d.parts.find((p) => p.text === 'brown').ok, false)
  assert.equal(L.dictationDiff('Hello there', 'hello there').score, 100)
  assert.equal(L.dictationDiff('Hello there', '').score, 0)
})

test('listenStep loops then stops', () => {
  const sp = { start: 0, end: 3000 }
  assert.equal(L.listenStep(1000, sp, 0, 3), 'play')
  assert.equal(L.listenStep(2990, sp, 0, 3), 'again')
  assert.equal(L.listenStep(2990, sp, 2, 3), 'stop')
  assert.equal(L.listenStep(2990, sp, 50, 0), 'again')
  assert.equal(L.listenStep(2990, sp, 0, 1), 'stop')
})
