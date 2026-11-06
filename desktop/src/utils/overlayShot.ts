// 屏幕翻译贴字层的排版：实时显示和“保存带译文的截图”共用同一套样式，两边看到的一模一样。
// 贴字层窗口不进截图（否则会被自己再识别一遍），所以存图时要把译文单独画到一张干净的画面上：
// 这里把每块译文拼成一段 SVG（里面是同样样式的 HTML），再由页面画到画布上。
import type { ScreenBlock, ScreenPayload } from '../types'

/** 窗口里的 CSS 像素和屏幕物理像素的换算（CSS px = 物理 px × 比例）。按窗口实际大小除以取景区大小算，不依赖系统报的缩放比例，多个显示器缩放不同也不会错位。 */
export interface View {
  sx: number
  sy: number
}

export const FONT_STACK = "-apple-system, 'Microsoft YaHei', 'PingFang SC', 'Noto Sans CJK SC', sans-serif"

/** 窗口大小 / 取景区大小；取景区不详时退回系统缩放比例。 */
export function viewOf(inner: [number, number], rect: number[] | undefined, dpr: number): View {
  const fallback = 1 / (dpr > 0 ? dpr : 1)
  const ok = !!rect && rect[2] > 0 && rect[3] > 0 && inner[0] > 0 && inner[1] > 0
  return ok ? { sx: inner[0] / rect![2], sy: inner[1] / rect![3] } : { sx: fallback, sy: fallback }
}

/** 一块译文起始字号（CSS 像素），不低于最小字号。 */
export function startFont(b: ScreenBlock, p: ScreenPayload, v: View): number {
  return Math.max(p.fontMin, b.font * v.sy)
}

/** 译文框比原文的框四周多出来的边（CSS 像素）：OCR 的框贴着字，译文的字体比原文大一点或行高不同就会被裁、偏下，所以框往外扩一圈，底色也能把原文盖严。 */
export function padOf(b: ScreenBlock, p: ScreenPayload, v: View): number {
  return Math.min(7, Math.max(2, startFont(b, p, v) * 0.18))
}

/** 一块译文的全部样式。`font` 是按框缩过之后的字号（CSS 像素）。 */
export function blockStyle(b: ScreenBlock, p: ScreenPayload, v: View, font: number): Record<string, string> {
  const pad = padOf(b, p, v)
  const o: Record<string, string> = {
    position: 'absolute',
    'box-sizing': 'border-box',
    overflow: 'visible',
    padding: b.vertical ? '3px 1px' : '1px 3px',
    'border-radius': '3px',
    'font-family': p.fontFamily ? `${p.fontFamily}` : FONT_STACK,
    'font-weight': '600',
    'line-height': b.vertical ? '1.15' : '1.2',
    'word-break': 'break-word',
    'overflow-wrap': 'anywhere',
    display: 'flex',
    left: `${b.x * v.sx - pad}px`,
    top: `${b.y * v.sy - pad}px`,
    width: `${b.w * v.sx + pad * 2}px`,
    // 用 min-height：放不下时框自己长高，不再把字裁掉
    'min-height': `${b.h * v.sy + pad * 2}px`,
    color: b.fg,
    background: b.bg,
    opacity: String(Math.max(0.3, p.opacity / 100)),
    'font-size': `${font}px`,
  }
  if (b.vertical) {
    o['writing-mode'] = 'vertical-rl'
    o['flex-direction'] = 'row-reverse'
    o['justify-content'] = 'flex-start'
    return o
  }
  o['flex-direction'] = 'column'
  // 字幕盖在原位：上下左右都居中；其余按原文的对齐方式摆
  const align = b.center ? 1 : (b.align ?? 0)
  // 单行（按钮、菜单项、标题）上下居中，原文在框里本来就是居中的；多行段落从上往下排
  o['justify-content'] = b.center || (b.lines ?? 1) <= 1 ? 'center' : 'flex-start'
  o['align-items'] = align === 1 ? 'center' : align === 2 ? 'flex-end' : 'flex-start'
  o['text-align'] = align === 1 ? 'center' : align === 2 ? 'right' : 'left'
  return o
}

export const ORIG_STYLE = { 'font-size': '0.62em', 'font-weight': '400', opacity: '0.72' }

/** 译文那一行的样式：竖排时中日韩用正立排版。 */
export function trStyle(b: ScreenBlock, p: ScreenPayload): Record<string, string> {
  return b.vertical ? { 'text-orientation': p.cjk ? 'upright' : 'mixed' } : {}
}

export function cssText(o: Record<string, string>): string {
  return Object.entries(o)
    .map(([k, v]) => `${k}:${v}`)
    .join(';')
}

export function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, '')
}

/** 把所有译文拼成一张 SVG（宽高是窗口的 CSS 像素）。`fonts[i]` 是第 i 块缩好的字号。 */
export function overlaySvg(p: ScreenPayload, v: View, fonts: number[], w: number, h: number): string {
  const blocks = p.blocks
    .map((b, i) => {
      const font = fonts[i] ?? startFont(b, p, v)
      const orig = p.bilingual && b.orig ? `<span style="${escapeXml(cssText(ORIG_STYLE))}">${escapeXml(b.orig)}</span>` : ''
      return `<div style="${escapeXml(cssText(blockStyle(b, p, v, font)))}">${orig}<span style="${escapeXml(cssText(trStyle(b, p)))}">${escapeXml(b.text)}</span></div>`
    })
    .join('')
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
    `<foreignObject x="0" y="0" width="${w}" height="${h}"><div xmlns="http://www.w3.org/1999/xhtml" style="position:relative;width:${w}px;height:${h}px;overflow:hidden">${blocks}</div></foreignObject></svg>`
  )
}
