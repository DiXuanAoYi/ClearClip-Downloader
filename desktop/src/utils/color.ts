/**
 * 颜色计算：色彩空间互转、WCAG 对比度、色盲模拟、BT.709 安全色域、配色规则、中文颜色名、
 * 从图片里提取一组颜色和渐变。全是纯函数，不碰界面。
 *
 * 约定：RGB 是 0–255 的整数；HSV / HSL 的 H 是 0–360，S、V、L 是 0–100；Lab 是 D65。
 */

export type RGB = [number, number, number]

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v))
const r8 = (v: number) => clamp(Math.round(v), 0, 255)

// ---------- 十六进制 ----------

/** `#abc` / `abc` / `#AABBCC` / `AABBCC` → RGB；写法不对返回 null。 */
export function hexToRgb(hex: string): RGB | null {
  let h = hex.trim().replace(/^#/, '')
  if (/^[0-9a-fA-F]{3}$/.test(h)) h = h.split('').map((c) => c + c).join('')
  if (!/^[0-9a-fA-F]{6}$/.test(h)) return null
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)]
}

export function rgbToHex(c: readonly number[]): string {
  return '#' + [c[0], c[1], c[2]].map((v) => r8(v).toString(16).padStart(2, '0')).join('').toUpperCase()
}

/** “RGB(212, 165, 165)”、“212,165,165”、“rgb(212 165 165)” → RGB。 */
export function parseRgbText(text: string): RGB | null {
  const m = text.replace(/rgb/i, '').match(/-?\d+(\.\d+)?/g)
  if (!m || m.length !== 3) return null
  const v = m.map(Number)
  if (v.some((x) => !Number.isFinite(x) || x < 0 || x > 255)) return null
  return [r8(v[0]), r8(v[1]), r8(v[2])]
}

export const rgbCss = (c: readonly number[]) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`

// ---------- HSV / HSL ----------

export function rgbToHsv(c: readonly number[]): [number, number, number] {
  const [r, g, b] = [c[0] / 255, c[1] / 255, c[2] / 255]
  const mx = Math.max(r, g, b)
  const mn = Math.min(r, g, b)
  const d = mx - mn
  let h = 0
  if (d > 0) {
    if (mx === r) h = ((g - b) / d) % 6
    else if (mx === g) h = (b - r) / d + 2
    else h = (r - g) / d + 4
    h *= 60
    if (h < 0) h += 360
  }
  return [h, mx === 0 ? 0 : (d / mx) * 100, mx * 100]
}

export function hsvToRgb(h: number, s: number, v: number): RGB {
  const hh = ((h % 360) + 360) % 360
  const [ss, vv] = [clamp(s, 0, 100) / 100, clamp(v, 0, 100) / 100]
  const ch = vv * ss
  const x = ch * (1 - Math.abs(((hh / 60) % 2) - 1))
  const m = vv - ch
  const [r, g, b] = hh < 60 ? [ch, x, 0] : hh < 120 ? [x, ch, 0] : hh < 180 ? [0, ch, x] : hh < 240 ? [0, x, ch] : hh < 300 ? [x, 0, ch] : [ch, 0, x]
  return [r8((r + m) * 255), r8((g + m) * 255), r8((b + m) * 255)]
}

export function rgbToHsl(c: readonly number[]): [number, number, number] {
  const [r, g, b] = [c[0] / 255, c[1] / 255, c[2] / 255]
  const mx = Math.max(r, g, b)
  const mn = Math.min(r, g, b)
  const l = (mx + mn) / 2
  const d = mx - mn
  if (d === 0) return [0, 0, l * 100]
  const s = d / (1 - Math.abs(2 * l - 1))
  const [h] = rgbToHsv(c)
  return [h, s * 100, l * 100]
}

export function hslToRgb(h: number, s: number, l: number): RGB {
  const hh = ((h % 360) + 360) % 360
  const [ss, ll] = [clamp(s, 0, 100) / 100, clamp(l, 0, 100) / 100]
  const ch = (1 - Math.abs(2 * ll - 1)) * ss
  const x = ch * (1 - Math.abs(((hh / 60) % 2) - 1))
  const m = ll - ch / 2
  const [r, g, b] = hh < 60 ? [ch, x, 0] : hh < 120 ? [x, ch, 0] : hh < 180 ? [0, ch, x] : hh < 240 ? [0, x, ch] : hh < 300 ? [x, 0, ch] : [ch, 0, x]
  return [r8((r + m) * 255), r8((g + m) * 255), r8((b + m) * 255)]
}

// ---------- 线性光、Lab ----------

const toLin = (v: number) => {
  const c = v / 255
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}
const fromLin = (c: number) => r8((c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055) * 255)

export function rgbToLab(c: readonly number[]): [number, number, number] {
  const [r, g, b] = [toLin(c[0]), toLin(c[1]), toLin(c[2])]
  const x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047
  const y = 0.2126729 * r + 0.7151522 * g + 0.072175 * b
  const z = (0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883
  const f = (t: number) => (t > 216 / 24389 ? Math.cbrt(t) : (24389 / 27 * t + 16) / 116)
  const [fx, fy, fz] = [f(x), f(y), f(z)]
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)]
}

export function labToRgb(l: number, a: number, b: number): RGB {
  const fy = (l + 16) / 116
  const [fx, fz] = [fy + a / 500, fy - b / 200]
  const inv = (t: number) => (t ** 3 > 216 / 24389 ? t ** 3 : (116 * t - 16) / (24389 / 27))
  const [x, y, z] = [inv(fx) * 0.95047, l > 8 ? fy ** 3 : l / (24389 / 27), inv(fz) * 1.08883]
  const r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z
  const g = -0.969266 * x + 1.8760108 * y + 0.041556 * z
  const bb = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z
  return [fromLin(r), fromLin(g), fromLin(bb)]
}

/** 两个颜色的差（CIE76 ΔE）：> 2.3 大致是肉眼能分出来的。 */
export function deltaE(a: readonly number[], b: readonly number[]): number {
  const [l1, a1, b1] = rgbToLab(a)
  const [l2, a2, b2] = rgbToLab(b)
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2)
}

// ---------- BT.709 安全色域 ----------

/** 全范围 RGB → YCbCr（BT.709，0–255）。 */
export function rgbToYcbcr709(c: readonly number[]): [number, number, number] {
  const [r, g, b] = c
  const y = 0.2126 * r + 0.7152 * g + 0.0722 * b
  return [y, 128 + (b - y) / 1.8556, 128 + (r - y) / 1.5748]
}

export function ycbcr709ToRgb(y: number, cb: number, cr: number): RGB {
  const r = y + 1.5748 * (cr - 128)
  const b = y + 1.8556 * (cb - 128)
  const g = (y - 0.2126 * r - 0.0722 * b) / 0.7152
  return [r8(r), r8(g), r8(b)]
}

export interface SafeRange {
  yMin: number
  yMax: number
  cMin: number
  cMax: number
}
/** 广播安全范围：Y 16–235，CbCr 16–240。 */
export const SAFE_DEFAULT: SafeRange = { yMin: 16, yMax: 235, cMin: 16, cMax: 240 }

/** 把颜色限制在安全范围里；返回新颜色和有没有被改。 */
export function clampSafe(c: readonly number[], r: SafeRange = SAFE_DEFAULT): { rgb: RGB; clamped: boolean; ycc: [number, number, number] } {
  const [y, cb, cr] = rgbToYcbcr709(c)
  const [y2, cb2, cr2] = [clamp(y, r.yMin, r.yMax), clamp(cb, r.cMin, r.cMax), clamp(cr, r.cMin, r.cMax)]
  const changed = Math.abs(y - y2) > 0.5 || Math.abs(cb - cb2) > 0.5 || Math.abs(cr - cr2) > 0.5
  const rgb = changed ? ycbcr709ToRgb(y2, cb2, cr2) : ([r8(c[0]), r8(c[1]), r8(c[2])] as RGB)
  return { rgb, clamped: changed, ycc: rgbToYcbcr709(rgb) }
}

// ---------- 对比度 ----------

export function relativeLuminance(c: readonly number[]): number {
  return 0.2126 * toLin(c[0]) + 0.7152 * toLin(c[1]) + 0.0722 * toLin(c[2])
}

export function contrastRatio(a: readonly number[], b: readonly number[]): number {
  const [la, lb] = [relativeLuminance(a), relativeLuminance(b)]
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

export interface Wcag {
  /** 普通文字 AA（4.5）/ AAA（7） */
  aa: boolean
  aaa: boolean
  /** 大号文字（≥ 18 pt 或 14 pt 粗体）AA（3）/ AAA（4.5） */
  aaLarge: boolean
  aaaLarge: boolean
  /** 图形和界面元素（3） */
  graphics: boolean
}

export function wcag(ratio: number): Wcag {
  return { aa: ratio >= 4.5, aaa: ratio >= 7, aaLarge: ratio >= 3, aaaLarge: ratio >= 4.5, graphics: ratio >= 3 }
}

/** “5.0 : 1 · AA” 这样的一行字。 */
export function ratioLabel(ratio: number): string {
  const t = `${ratio.toFixed(1)} : 1`
  if (ratio >= 7) return `${t} · AAA`
  if (ratio >= 4.5) return `${t} · AA`
  if (ratio >= 3) return `${t} · 仅大字`
  return `${t} · 不够`
}

/**
 * 保持色相和饱和度，只调明度，让 `fg` 对 `bg` 达到 `target`。往更亮、更暗两个方向都试，取离原来最近的；
 * 都达不到返回 null（比如目标比黑白之间能做到的还高）。
 */
export function adjustForContrast(fg: readonly number[], bg: readonly number[], target: number): RGB | null {
  if (contrastRatio(fg, bg) >= target) return [fg[0], fg[1], fg[2]]
  const [h, s, l] = rgbToHsl(fg)
  let best: RGB | null = null
  let bestD = Infinity
  for (const dir of [1, -1]) {
    for (let step = 1; step <= 100; step++) {
      const ll = l + dir * step
      if (ll < 0 || ll > 100) break
      const c = hslToRgb(h, s, ll)
      if (contrastRatio(c, bg) >= target) {
        if (step < bestD) {
          best = c
          bestD = step
        }
        break
      }
    }
  }
  return best
}

// ---------- 色盲模拟（Machado 2009，线性 RGB，严重程度 100%） ----------

export type ColorBlind = 'protanopia' | 'deuteranopia' | 'tritanopia'

const CB: Record<ColorBlind, number[][]> = {
  protanopia: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deuteranopia: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritanopia: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
}
export const COLORBLIND_NAMES: Record<ColorBlind, string> = { protanopia: '红盲', deuteranopia: '绿盲', tritanopia: '蓝盲' }

export function simulateColorblind(c: readonly number[], kind: ColorBlind): RGB {
  const lin = [toLin(c[0]), toLin(c[1]), toLin(c[2])]
  const m = CB[kind]
  const out = m.map((row) => clamp(row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2], 0, 1))
  return [fromLin(out[0]), fromLin(out[1]), fromLin(out[2])]
}

// ---------- 配色规则 ----------

export const HARMONY_SCHEMES: Record<string, number[]> = {
  互补: [180],
  类似: [-30, 30],
  分裂互补: [150, 210],
  三角: [120, 240],
  四方: [90, 180, 270],
}

/** 按色相偏移得到一组配色（保持饱和度和明度）。 */
export function harmony(c: readonly number[], offsets: number[]): RGB[] {
  const [h, s, v] = rgbToHsv(c)
  return offsets.map((d) => hsvToRgb(h + d, s, v))
}

/** 色轮上的一组颜色：以 `base` 为中心，按规则排出 5 个（Adobe Color 色轮那样一组点联动）。 */
export function wheelPalette(base: readonly number[], scheme: string): RGB[] {
  const [h, s, v] = rgbToHsv(base)
  const mk = (dh: number, ds = 0, dv = 0) => hsvToRgb(h + dh, clamp(s + ds, 0, 100), clamp(v + dv, 0, 100))
  switch (scheme) {
    case '单色':
      return [mk(0, 0, -40), mk(0, 0, -20), mk(0), mk(0, -25, 10), mk(0, -50, 20)]
    case '三色':
      return [mk(-120), mk(-60, 0, -10), mk(0), mk(120), mk(60, 0, -10)]
    case '互补':
      return [mk(0, 0, -25), mk(0, -20, 0), mk(0), mk(180, -20, 0), mk(180)]
    case '分裂互补':
      return [mk(150), mk(150, -15, -10), mk(0), mk(210, -15, -10), mk(210)]
    case '四方':
      return [mk(0), mk(90), mk(180), mk(270), mk(45, -20, 5)]
    default:
      return [-60, -30, 0, 30, 60].map((d) => mk(d))
  }
}

// ---------- 中文颜色名 ----------

const NAMES: [string, RGB][] = [
  ['黑色', [0, 0, 0]], ['深灰', [64, 64, 64]], ['灰色', [128, 128, 128]], ['银灰', [192, 192, 192]], ['白色', [255, 255, 255]],
  ['红色', [255, 0, 0]], ['深红', [139, 0, 0]], ['酒红', [128, 0, 32]], ['橙红', [255, 69, 0]], ['橙色', [255, 140, 0]],
  ['金色', [255, 215, 0]], ['黄色', [255, 255, 0]], ['芥末黄', [244, 169, 0]], ['米黄', [245, 245, 220]], ['杏色', [255, 218, 185]],
  ['黄绿', [173, 255, 47]], ['绿色', [0, 160, 0]], ['草绿', [124, 252, 0]], ['墨绿', [0, 100, 0]], ['橄榄绿', [107, 142, 35]],
  ['青色', [0, 255, 255]], ['青绿', [0, 206, 209]], ['天蓝', [135, 206, 250]], ['蓝色', [0, 0, 255]], ['宝蓝', [30, 144, 255]],
  ['深蓝', [0, 0, 128]], ['藏青', [25, 25, 112]], ['靛蓝', [75, 0, 130]], ['紫色', [138, 43, 226]], ['紫罗兰', [148, 0, 211]],
  ['粉色', [255, 192, 203]], ['玫红', [255, 20, 147]], ['品红', [255, 0, 255]], ['玫瑰粉', [212, 165, 165]], ['陶土色', [193, 102, 107]],
  ['棕色', [139, 69, 19]], ['巧克力棕', [74, 64, 58]], ['赭石', [160, 82, 45]], ['驼色', [205, 133, 63]], ['沙色', [232, 213, 196]],
  ['勃艮第红', [93, 46, 70]], ['珊瑚', [255, 127, 80]], ['鲑鱼粉', [255, 160, 122]], ['薄荷绿', [152, 251, 152]], ['浅蓝', [173, 216, 230]],
]

export function colorName(c: readonly number[]): string {
  let best = NAMES[0]
  let bd = Infinity
  for (const n of NAMES) {
    const d = deltaE(c, n[1])
    if (d < bd) {
      bd = d
      best = n
    }
  }
  return best[0]
}

/** 名字 → 颜色（先按中文名，再按十六进制 / 常见英文名）；认不出来返回 null。 */
export function nameToRgb(name: string): RGB | null {
  const t = name.trim()
  const hit = NAMES.find((n) => n[0] === t)
  if (hit) return [...hit[1]] as RGB
  const hex = hexToRgb(t)
  if (hex) return hex
  const en: Record<string, RGB> = { red: [255, 0, 0], green: [0, 128, 0], blue: [0, 0, 255], black: [0, 0, 0], white: [255, 255, 255], yellow: [255, 255, 0], orange: [255, 165, 0], purple: [128, 0, 128], pink: [255, 192, 203], gray: [128, 128, 128], grey: [128, 128, 128], brown: [165, 42, 42] }
  return en[t.toLowerCase()] ? ([...en[t.toLowerCase()]] as RGB) : null
}

// ---------- 颜色库 ----------

/** 基本颜色 48 格（8 × 6）。 */
export const BASIC_COLORS = [
  '#FFFFFF', '#C0C0C0', '#808080', '#404040', '#000000', '#2F4F4F', '#556B2F', '#8B4513',
  '#FF0000', '#FF4500', '#FF8C00', '#FFD700', '#FFFF00', '#ADFF2F', '#00FF00', '#00FA9A',
  '#00FFFF', '#00CED1', '#1E90FF', '#0000FF', '#8A2BE2', '#9400D3', '#FF00FF', '#FF1493',
  '#FF6347', '#FF7F50', '#FFA07A', '#FFDAB9', '#FFFACD', '#98FB98', '#AFEEEE', '#87CEFA',
  '#CD853F', '#D2691E', '#BC8F8F', '#F4A460', '#DEB887', '#DAA520', '#BDB76B', '#6B8E23',
  '#800000', '#8B0000', '#A0522D', '#4B0082', '#483D8B', '#000080', '#006400', '#191970',
]

// ---------- 从图片提取 ----------

export interface PixelImage {
  data: ArrayLike<number>
  width: number
  height: number
}

export interface Extracted {
  rgb: RGB
  /** 占取样范围的比例 0–1 */
  share: number
}

export type Mood = 'auto' | 'colorful' | 'bright' | 'muted' | 'deep' | 'dark'
export const MOODS: { id: Mood; name: string }[] = [
  { id: 'auto', name: '自动' },
  { id: 'colorful', name: '多彩' },
  { id: 'bright', name: '明亮' },
  { id: 'muted', name: '柔和' },
  { id: 'deep', name: '深色' },
  { id: 'dark', name: '暗' },
]

/** 取样范围：像素框（左、上、右、下），不给 = 整张图。 */
export type Box = [number, number, number, number]

/** 一个点附近 (2r+1)² 个像素的平均色；r = 0 就是这一个像素。 */
export function pixelAverage(img: PixelImage, x: number, y: number, radius = 0): RGB {
  const [cx, cy] = [clamp(Math.round(x), 0, img.width - 1), clamp(Math.round(y), 0, img.height - 1)]
  let n = 0
  const s = [0, 0, 0]
  for (let yy = Math.max(0, cy - radius); yy <= Math.min(img.height - 1, cy + radius); yy++) {
    for (let xx = Math.max(0, cx - radius); xx <= Math.min(img.width - 1, cx + radius); xx++) {
      const i = (yy * img.width + xx) * 4
      s[0] += img.data[i]
      s[1] += img.data[i + 1]
      s[2] += img.data[i + 2]
      n++
    }
  }
  return n ? [r8(s[0] / n), r8(s[1] / n), r8(s[2] / n)] : [0, 0, 0]
}

/** 取出现最多的 `n` 个颜色（中位切分），按 `mood` 偏向某种风格；颜色之间至少差 ΔE 12，免得取到一堆相近色。 */
export function extractPalette(img: PixelImage, n = 5, mood: Mood = 'auto', box?: Box): Extracted[] {
  const [x0, y0, x1, y1] = box ?? [0, 0, img.width, img.height]
  const area = Math.max(1, (x1 - x0) * (y1 - y0))
  const step = Math.max(1, Math.floor(Math.sqrt(area / 8000)))
  const px: RGB[] = []
  for (let y = Math.max(0, y0); y < Math.min(img.height, y1); y += step) {
    for (let x = Math.max(0, x0); x < Math.min(img.width, x1); x += step) {
      const i = (y * img.width + x) * 4
      if ((img.data[i + 3] ?? 255) < 128) continue
      px.push([img.data[i], img.data[i + 1], img.data[i + 2]])
    }
  }
  if (!px.length) return []
  // 先量化到 5 位再数，相近色合并
  const hist = new Map<number, { n: number; s: [number, number, number] }>()
  for (const p of px) {
    const k = ((p[0] >> 3) << 10) | ((p[1] >> 3) << 5) | (p[2] >> 3)
    const e = hist.get(k) ?? { n: 0, s: [0, 0, 0] }
    e.n++
    e.s[0] += p[0]
    e.s[1] += p[1]
    e.s[2] += p[2]
    hist.set(k, e)
  }
  type Bin = { rgb: RGB; n: number }
  let bins: Bin[] = [...hist.values()].map((e) => ({ rgb: [e.s[0] / e.n, e.s[1] / e.n, e.s[2] / e.n] as RGB, n: e.n }))
  // 中位切分到 16 组
  const boxes: Bin[][] = [bins]
  while (boxes.length < 16) {
    let bi = -1
    let bs = 0
    boxes.forEach((b, i) => {
      if (b.length < 2) return
      const size = b.reduce((a, x) => a + x.n, 0)
      if (size > bs) {
        bs = size
        bi = i
      }
    })
    if (bi < 0) break
    const b = boxes[bi]
    const range = [0, 1, 2].map((k) => Math.max(...b.map((x) => x.rgb[k])) - Math.min(...b.map((x) => x.rgb[k])))
    const k = range.indexOf(Math.max(...range))
    b.sort((p, q) => p.rgb[k] - q.rgb[k])
    const total = b.reduce((a, x) => a + x.n, 0)
    let acc = 0
    let cut = 1
    for (let i = 0; i < b.length; i++) {
      acc += b[i].n
      if (acc >= total / 2) {
        cut = Math.min(b.length - 1, Math.max(1, i + 1))
        break
      }
    }
    boxes.splice(bi, 1, b.slice(0, cut), b.slice(cut))
  }
  bins = boxes.map((b) => {
    const total = b.reduce((a, x) => a + x.n, 0)
    const rgb = [0, 1, 2].map((k) => b.reduce((a, x) => a + x.rgb[k] * x.n, 0) / total)
    return { rgb: [r8(rgb[0]), r8(rgb[1]), r8(rgb[2])] as RGB, n: total }
  })
  const total = bins.reduce((a, b) => a + b.n, 0)
  const score = (b: Bin) => {
    const [, s, v] = rgbToHsv(b.rgb)
    const share = b.n / total
    const pref =
      mood === 'colorful' ? s / 100
      : mood === 'bright' ? v / 100
      : mood === 'muted' ? 1 - Math.abs(s - 30) / 100
      : mood === 'deep' ? (s / 100) * (1 - v / 100)
      : mood === 'dark' ? 1 - v / 100
      : 0
    return mood === 'auto' ? share : share ** 0.5 * (0.2 + pref)
  }
  const sorted = [...bins].sort((a, b) => score(b) - score(a))
  const out: Extracted[] = []
  for (const b of sorted) {
    if (out.length >= n) break
    if (out.every((o) => deltaE(o.rgb, b.rgb) >= 12)) out.push({ rgb: b.rgb, share: b.n / total })
  }
  // 差异太小凑不够 n 个：放宽
  for (const b of sorted) {
    if (out.length >= n) break
    if (!out.some((o) => o.rgb === b.rgb) && out.every((o) => deltaE(o.rgb, b.rgb) >= 4)) out.push({ rgb: b.rgb, share: b.n / total })
  }
  return out
}

/** 沿一条线（像素坐标）等距取 `n` 个色标，每个取周围 3×3 的平均。 */
export function sampleGradient(img: PixelImage, from: [number, number], to: [number, number], n = 4): RGB[] {
  const out: RGB[] = []
  for (let i = 0; i < n; i++) {
    const t = n === 1 ? 0 : i / (n - 1)
    out.push(pixelAverage(img, from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, 1))
  }
  return out
}

/** 色标 → CSS 渐变（`dir`：to right / to bottom）。 */
export function gradientCss(stops: readonly RGB[], dir: 'hgrad' | 'vgrad' = 'hgrad'): string {
  const list = stops.length >= 2 ? stops : [stops[0] ?? ([255, 255, 255] as RGB), stops[0] ?? ([255, 255, 255] as RGB)]
  return `linear-gradient(${dir === 'hgrad' ? 'to right' : 'to bottom'}, ${list.map((c, i) => `${rgbCss(c)} ${Math.round((i / (list.length - 1)) * 100)}%`).join(', ')})`
}
