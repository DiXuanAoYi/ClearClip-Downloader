import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { parseSample, scrub, kindOf, fixtureName, importSample } from '../../scripts/import-sample.mjs'

test('parseSample 读出地址、时间和正文', () => {
  const s = parseSample('URL: https://a.com/x\nTIME: 2026-01-01T00:00:00Z\n\n{"a":1}')
  assert.deepEqual(s, { url: 'https://a.com/x', time: '2026-01-01T00:00:00Z', body: '{"a":1}' })
  assert.throws(() => parseSample('{"a":1}'), /录制样本/)
})

test('scrub 替换手机号、邮箱、内网地址，不动长数字 ID', () => {
  const { text, counts } = scrub('{"tel":"13912345678","m":"a.b@qq.com","ip":"192.168.1.5","id":7123456789012345678,"v":"1.2.3.4"}')
  assert.ok(!/13912345678|a\.b@qq|192\.168/.test(text))
  assert.ok(text.includes('7123456789012345678') && text.includes('1.2.3.4'))
  assert.deepEqual(counts, { phone: 1, email: 1, ip: 1 })
})

test('kindOf / fixtureName', () => {
  assert.equal(kindOf('{"a":1}'), 'json')
  assert.equal(kindOf('<html></html>'), 'html')
  assert.equal(fixtureName('bilibili', '/x/bilibili/20260101-120000.123-space-list.txt'), 'bilibili-space-list')
  assert.equal(fixtureName('bilibili', '/x/a.txt', '合集 v2'), 'bilibili-v2')
  assert.throws(() => fixtureName('x', '/x/a.txt', '合集'), /名字/)
})

test('importSample 写出样本和说明文件', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'imp-'))
  const src = path.join(tmp, 'bilibili')
  fs.mkdirSync(src)
  const f = path.join(src, '20260101-120000.123-dynamic.txt')
  fs.writeFileSync(f, 'URL: https://api.bilibili.com/x\nTIME: t\n\n{"code":0,"p":"13912345678"}')
  const out = path.join(tmp, 'out')
  const { target } = importSample(f, undefined, out)
  assert.equal(path.basename(target), 'bilibili-dynamic.json')
  assert.ok(!fs.readFileSync(target, 'utf8').includes('13912345678'))
  const meta = JSON.parse(fs.readFileSync(path.join(out, 'bilibili-dynamic.meta.json'), 'utf8'))
  assert.equal(meta.url, 'https://api.bilibili.com/x')
  assert.equal(meta.scrubbed.phone, 1)
  fs.rmSync(tmp, { recursive: true })
})
