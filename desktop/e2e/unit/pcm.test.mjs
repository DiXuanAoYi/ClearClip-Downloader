import assert from 'node:assert/strict'
import * as P from '../../src/utils/pcm.ts'

// 重采样：48k → 16k 样本数是三分之一，直流分量不变；16k 原样返回
{
  const a = new Float32Array(4800).fill(0.5)
  const r = P.resampleTo16k(a, 48000)
  assert.equal(r.length, 1600)
  assert.ok(r.every((x) => Math.abs(x - 0.5) < 1e-6))
  assert.equal(P.resampleTo16k(a, 16000), a)
  assert.equal(P.resampleTo16k(new Float32Array(0), 44100).length, 0)
  // 44.1k：长度按比例，斜坡信号保持单调
  const ramp = Float32Array.from({ length: 4410 }, (_, i) => i / 4410)
  const rr = P.resampleTo16k(ramp, 44100)
  assert.equal(rr.length, 1600)
  for (let i = 1; i < rr.length; i++) assert.ok(rr[i] >= rr[i - 1])
}

// 浮点 → 16 位：满幅、零、削平
{
  const i = P.floatToInt16(Float32Array.of(0, 1, -1, 0.5, -0.5, 2, -3))
  assert.deepEqual(Array.from(i), [0, 32767, -32768, 16384, -16384, 32767, -32768])
}

// base64：小端，能还原（和后端 pcm_from_b64 对得上）
{
  const s = Int16Array.of(0, 1, -1, 256, -32768, 32767)
  const b64 = P.int16ToBase64(s)
  const bytes = Buffer.from(b64, 'base64')
  assert.equal(bytes.length, 12)
  assert.deepEqual(Array.from({ length: 6 }, (_, k) => bytes.readInt16LE(k * 2)), Array.from(s))
  // 很长的一段不会因为参数太多出错
  assert.equal(Buffer.from(P.int16ToBase64(new Int16Array(100000)), 'base64').length, 200000)
}

// 音量：静音 0，满幅正弦接近上限
{
  assert.equal(P.level(new Float32Array(100)), 0)
  assert.equal(P.level(new Float32Array(0)), 0)
  assert.ok(P.level(new Float32Array(100).fill(0.5)) > 0.9)
}
console.log('pcm ok')

import * as D from '../../src/utils/dictation.ts'
assert.equal(D.joinFinals(['你好', '', '世界']), '你好世界')
assert.equal(D.joinFinals(['Hello', 'world']), 'Hello world')
assert.equal(D.joinFinals(['这是 OK', 'fine']), '这是 OK fine')
assert.equal(D.joinFinals(['今天', 'meeting', '很长']), '今天meeting很长', '中英文交界不加空格（和后端一致）')
assert.equal(D.joinFinals([]), '')
console.log('dictation ok')
