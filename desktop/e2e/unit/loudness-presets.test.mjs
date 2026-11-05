import assert from 'node:assert/strict'
import * as L from '../../src/utils/loudness.ts'

// 预设表：编号不重复，三个数都在后端接受的范围内（后端会把超出范围的夹回去，这里保证预设本身不被夹）
{
  const ids = L.LOUDNESS_PRESETS.map((p) => p.id)
  assert.equal(new Set(ids).size, ids.length)
  assert.ok(!ids.includes(L.CUSTOM_LOUDNESS))
  for (const p of L.LOUDNESS_PRESETS) {
    assert.ok(p.lufs >= -30 && p.lufs <= -5, `${p.id} 响度`)
    assert.ok(p.tp >= -9 && p.tp <= 0, `${p.id} 真峰值`)
    assert.ok(p.lra >= 1 && p.lra <= 20, `${p.id} 响度范围`)
    assert.ok(p.name && p.note)
  }
}

// 常见行业标准都在，且数值正确
{
  const g = (id) => L.findLoudnessPreset(id)
  assert.deepEqual([g('web').lufs, g('web').tp, g('web').lra], [-16, -1.5, 11])
  assert.equal(g('stream').lufs, -14)
  assert.equal(g('ebu').lufs, -23)
  assert.equal(g('atsc').lufs, -24)
  assert.equal(g('atsc').tp, -2)
}

// 三个数正好等于某个预设就显示预设，改了任何一个就是自定义
{
  assert.equal(L.loudnessPresetId(-23, -1, 15), 'ebu')
  assert.equal(L.loudnessPresetId(-16, -1.5, 11), 'web')
  assert.equal(L.loudnessPresetId(-16, -1.5, 10), L.CUSTOM_LOUDNESS)
  assert.equal(L.loudnessPresetId(-17, -1.5, 11), L.CUSTOM_LOUDNESS)
}
console.log('loudness presets ok')
