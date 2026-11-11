import test from 'node:test'
import assert from 'node:assert/strict'
import {
  hexToRgb, rgbToHex, parseRgbText, rgbToHsv, hsvToRgb, rgbToHsl, hslToRgb, rgbToLab, labToRgb,
  contrastRatio, wcag, adjustForContrast, simulateColorblind, clampSafe, rgbToYcbcr709,
  colorName, extractPalette, sampleGradient, gradientCss, harmony, deltaE,
} from '../../src/utils/color.ts'

test('hex 往返与简写', () => {
  assert.deepEqual(hexToRgb('#abc'), [170, 187, 204])
  assert.equal(rgbToHex([212, 165, 165]), '#D4A5A5')
  assert.equal(hexToRgb('zzz'), null)
  assert.deepEqual(parseRgbText('RGB(212, 165, 165)'), [212, 165, 165])
  assert.equal(parseRgbText('1,2'), null)
})

test('HSV / HSL / Lab 往返误差不超过 1', () => {
  for (const c of [[0, 0, 0], [255, 255, 255], [212, 165, 165], [91, 47, 72], [244, 169, 0], [10, 200, 240]]) {
    const a = hsvToRgb(...rgbToHsv(c))
    const b = hslToRgb(...rgbToHsl(c))
    const l = labToRgb(...rgbToLab(c))
    for (let i = 0; i < 3; i++) {
      assert.ok(Math.abs(a[i] - c[i]) <= 1, `hsv ${c}`)
      assert.ok(Math.abs(b[i] - c[i]) <= 1, `hsl ${c}`)
      assert.ok(Math.abs(l[i] - c[i]) <= 1, `lab ${c}`)
    }
  }
})

test('对比度：黑白 21，同色 1', () => {
  assert.ok(Math.abs(contrastRatio([0, 0, 0], [255, 255, 255]) - 21) < 0.01)
  assert.equal(contrastRatio([9, 9, 9], [9, 9, 9]), 1)
  assert.equal(wcag(4.5).aa, true)
  assert.equal(wcag(4.4).aa, false)
  assert.equal(wcag(3).aaLarge, true)
})

test('adjustForContrast 达标且尽量不动色相', () => {
  const bg = [91, 47, 72]
  const out = adjustForContrast([120, 80, 100], bg, 4.5)
  assert.ok(out && contrastRatio(out, bg) >= 4.5)
  assert.equal(adjustForContrast([255, 255, 255], [255, 255, 255], 30), null)
})

test('色盲：灰色不变，纯红在红盲下变暗', () => {
  assert.ok(deltaE(simulateColorblind([128, 128, 128], 'deuteranopia'), [128, 128, 128]) < 2)
  const r = simulateColorblind([255, 0, 0], 'protanopia')
  assert.ok(r[0] < 120)
})

test('BT.709 安全色域：纯白、纯黑被拉回，中灰不动', () => {
  assert.equal(clampSafe([128, 128, 128]).clamped, false)
  const w = clampSafe([255, 255, 255])
  assert.equal(w.clamped, true)
  assert.ok(rgbToYcbcr709(w.rgb)[0] <= 235.6)
  assert.equal(clampSafe([0, 0, 0]).clamped, true)
})

test('颜色名与配色', () => {
  assert.equal(colorName([255, 0, 0]), '红色')
  assert.equal(colorName([0, 0, 0]), '黑色')
  const h = harmony([255, 0, 0], [180])
  assert.deepEqual(h[0], [0, 255, 255])
})

function img(w, h, f) {
  const data = new Uint8ClampedArray(w * h * 4)
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const c = f(x, y)
    data.set([c[0], c[1], c[2], 255], (y * w + x) * 4)
  }
  return { data, width: w, height: h }
}

test('提取：红蓝两半，红蓝各一，占比约一半', () => {
  const im = img(40, 40, (x) => (x < 20 ? [200, 30, 30] : [30, 30, 200]))
  const p = extractPalette(im, 2)
  assert.equal(p.length, 2)
  const red = p.find((e) => e.rgb[0] > 150)
  const blue = p.find((e) => e.rgb[2] > 150)
  assert.ok(red && blue)
  assert.ok(Math.abs(red.share - 0.5) < 0.1)
})

test('提取：框选范围只看框内', () => {
  const im = img(40, 40, (x) => (x < 20 ? [200, 30, 30] : [30, 30, 200]))
  const p = extractPalette(im, 3, 'auto', [0, 0, 10, 40])
  assert.ok(p.every((e) => e.rgb[0] > 150))
})

test('渐变取样与 CSS', () => {
  const im = img(100, 10, (x) => [x * 2, 0, 0])
  const s = sampleGradient(im, [1, 5], [98, 5], 3)
  assert.ok(s[0][0] < s[1][0] && s[1][0] < s[2][0])
  assert.match(gradientCss(s, 'vgrad'), /^linear-gradient\(to bottom, rgb\(/)
})
