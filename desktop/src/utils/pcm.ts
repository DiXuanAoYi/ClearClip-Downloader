// 实时听写的声音处理：麦克风给的是任意采样率的浮点样本，后端要 16 kHz 单声道 16 位小端 PCM（base64）。

/** 线性插值重采样到 16 kHz。采样率本来就是 16 kHz 时原样返回。 */
export function resampleTo16k(input: Float32Array, srcRate: number): Float32Array {
  if (!(srcRate > 0) || srcRate === 16000 || input.length === 0) return input
  const ratio = srcRate / 16000
  const n = Math.floor(input.length / ratio)
  const out = new Float32Array(n)
  for (let i = 0; i < n; i++) {
    const pos = i * ratio
    const a = Math.floor(pos)
    const b = Math.min(a + 1, input.length - 1)
    const f = pos - a
    out[i] = input[a] * (1 - f) + input[b] * f
  }
  return out
}

/** -1..1 的浮点样本 → 16 位整数（超出范围的削平）。 */
export function floatToInt16(input: Float32Array): Int16Array {
  const out = new Int16Array(input.length)
  for (let i = 0; i < input.length; i++) {
    const s = Math.max(-1, Math.min(1, input[i]))
    out[i] = s < 0 ? Math.round(s * 32768) : Math.round(s * 32767)
  }
  return out
}

/** 16 位整数样本 → 小端字节的 base64（分段拼，避免一次展开太大的参数）。 */
export function int16ToBase64(samples: Int16Array): string {
  const bytes = new Uint8Array(samples.length * 2)
  const view = new DataView(bytes.buffer)
  samples.forEach((s, i) => view.setInt16(i * 2, s, true))
  let bin = ''
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return btoa(bin)
}

/** 一段样本的音量（均方根，0..1），界面做音量条用。 */
export function level(input: Float32Array): number {
  if (!input.length) return 0
  let sum = 0
  for (const x of input) sum += x * x
  return Math.min(1, Math.sqrt(sum / input.length) * 4)
}
