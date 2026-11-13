<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../api'
import type { BgMode, BlockEdit, PdfBlock, PdfLayout, RGB3, StylePatch, TextAlign } from '../types'
import { gradientCss, rgbCss, rgbToHex } from '../utils/color'
import ColorPicker from './ColorPicker.vue'

/**
 * PDF 版面画布：原页图 + 每块的译文（盖上原文所在处的背景，放回原来的位置）。
 * 可以切换看原文 / 译文 / 左右对比 / 导出效果（真的写进 PDF 再渲染），滚轮缩放（Ctrl/⌘+滚轮 = 以鼠标为中心缩放，
 * 普通滚轮 = 上下平移）、拖动平移，点一块译文直接改；Ctrl / Shift 点选多块，右边检查器改文字色、背景、字号、对齐，
 * 单选多选都行，也可以把一种颜色在本页 / 整份文件里换成另一种。
 */
const props = defineProps<{ path: string; target: string; ocr: string; jobStatus?: string; running?: boolean; page: number }>()
const emit = defineEmits<{
  (e: 'edited', viewPage: number, index: number, text: string | null): void
  (e: 'update:page', page: number): void
  (e: 'continue'): void
}>()

type Mode = 'orig' | 'trans' | 'both' | 'export'
const mode = ref<Mode>('trans')
const page = computed({ get: () => props.page, set: (v: number) => emit('update:page', v) })
const lay = ref<PdfLayout | null>(null)
const exp = ref<{ orig: string; trans: string; items: number } | null>(null)
const busy = ref(false)
const err = ref('')
let token = 0

// 视图变换：s 是相对“适应宽度”的倍数，(x, y) 是平移（像素）
const tf = ref({ s: 1, x: 0, y: 0 })
const vp = ref<HTMLElement | null>(null)
const vw = ref(800)
let ro: ResizeObserver | null = null

const panes = computed<('orig' | 'trans')[]>(() => (mode.value === 'both' || mode.value === 'export' ? ['orig', 'trans'] : [mode.value === 'orig' ? 'orig' : 'trans']))
const baseW = computed(() => Math.max(200, (vw.value - 8 - (panes.value.length - 1) * 10) / panes.value.length - 8))
const k = computed(() => (lay.value ? baseW.value / lay.value.w : 1)) // 1 个 PDF 点 = k 个界面像素（适应宽度时）
const stageH = computed(() => (lay.value ? (baseW.value * lay.value.h) / lay.value.w : 0))
const stageStyle = computed(() => ({
  width: `${baseW.value}px`,
  height: `${stageH.value}px`,
  transform: `translate(${tf.value.x}px, ${tf.value.y}px) scale(${tf.value.s})`,
}))

/** 这一页还没译的块（不含按设置不翻译的） */
const todoCount = computed(() => (lay.value?.blocks ?? []).filter((b) => !b.skipped && !b.trans.trim()).length)
const shown = computed(() => (lay.value?.blocks ?? []).filter((b) => !b.skipped))
const banner = computed(() => {
  const l = lay.value
  if (!l || mode.value === 'export') return ''
  if (l.note) return l.note
  if (!todoCount.value) return ''
  const done = shown.value.length - todoCount.value
  if (!l.done && !done) return props.running ? '这一页还没译到，译到后会自动出现。' : '这一页还没有译文。'
  return `这一页译好了 ${done} 段，还有 ${todoCount.value} 段没译（虚线框）。没译的先显示原文，不会整页空白；点虚线框可以自己写译文。`
})

// ---------- 一块的外观：外扩的遮盖 + 保持原字号 + 往下长 ----------
/** 外扩（点）：手动设的，没有就是字号的 18%，至少 2 点（和导出一致） */
const padOf = (b: PdfBlock) => b.pad ?? Math.max(0.18 * b.size, 2)
function bgCss(b: PdfBlock): string {
  if (!b.trans.trim()) return 'rgba(251, 234, 219, 0.5)'
  if (b.bgMode === 'none') return 'transparent'
  if ((b.bgMode === 'hgrad' || b.bgMode === 'vgrad') && b.bgStops.length >= 2) return gradientCss(b.bgStops, b.bgMode)
  return rgbCss(b.bg)
}
function blkStyle(b: PdfBlock) {
  const l = lay.value!
  const [bl, bb, br, bt] = b.rect
  const pad = padOf(b)
  const just = b.align === 'center' ? 'center' : b.align === 'right' ? 'flex-end' : 'flex-start'
  return {
    left: `${((bl - pad) / l.w) * 100}%`,
    top: `${((l.h - bt - pad) / l.h) * 100}%`,
    width: `${((br - bl + 2 * pad) / l.w) * 100}%`,
    minHeight: `${(bt - bb + 2 * pad) * k.value}px`,
    padding: `${pad * k.value}px`,
    background: bgCss(b),
    color: rgbCss(b.rgb),
    fontSize: `${Math.max(3, b.size * k.value)}px`,
    textAlign: b.align,
    justifyContent: just,
  }
}
function selStyle(b: PdfBlock) {
  const l = lay.value!
  const [bl, bb, br, bt] = b.rect
  return { left: `${(bl / l.w) * 100}%`, top: `${((l.h - bt) / l.h) * 100}%`, width: `${((br - bl) / l.w) * 100}%`, height: `${((bt - bb) / l.h) * 100}%` }
}

// ---------- 选择：单击一块；Ctrl / ⌘ 点选加减，Shift 点选连续；列表里也一样 ----------
const selSet = ref<Set<number>>(new Set())
const sel = ref(-1) // 最后点的那一块
const selBlocks = computed(() => (lay.value?.blocks ?? []).filter((b) => selSet.value.has(b.index) && !b.skipped))
const selBlock = computed(() => (selBlocks.value.length === 1 ? selBlocks.value[0] : null))
const multi = computed(() => selBlocks.value.length > 1)
function pick(i: number, ev?: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }) {
  const s = new Set(selSet.value)
  if (ev?.shiftKey && sel.value >= 0) {
    const ids = shown.value.map((b) => b.index)
    const [a, z] = [ids.indexOf(sel.value), ids.indexOf(i)]
    if (a >= 0 && z >= 0) for (const id of ids.slice(Math.min(a, z), Math.max(a, z) + 1)) s.add(id)
  } else if (ev?.ctrlKey || ev?.metaKey) {
    if (s.has(i)) s.delete(i)
    else s.add(i)
  } else {
    s.clear()
    s.add(i)
  }
  selSet.value = s
  sel.value = s.has(i) ? i : [...s].pop() ?? -1
}
function selectAll() {
  selSet.value = new Set(shown.value.map((b) => b.index))
  sel.value = shown.value[0]?.index ?? -1
}
function selectNone() {
  selSet.value = new Set()
  sel.value = -1
}
function focusBlock(i: number, ev?: MouseEvent) {
  pick(i, ev)
  if (ev?.ctrlKey || ev?.metaKey || ev?.shiftKey) return
  const el = document.querySelector<HTMLElement>(`.pdfcv .blk[data-idx="${i}"]`)
  el?.focus()
  el?.scrollIntoView({ block: 'center', inline: 'center' })
}
/** 点在块上：带修饰键只改选择，不进入编辑 */
function blkDown(e: PointerEvent, b: PdfBlock) {
  if (e.ctrlKey || e.metaKey || e.shiftKey) {
    e.preventDefault()
    pick(b.index, e)
  }
}
function blkFocus(b: PdfBlock) {
  if (!selSet.value.has(b.index)) pick(b.index)
  else sel.value = b.index
}

// ---------- 改样式：先改界面，再存进缓存（预览和生成文件都按它来） ----------
const DEF_ALIGN: TextAlign = 'left'
function localApply(b: PdfBlock, p: StylePatch) {
  for (const key of p.clear ?? []) {
    if (key === 'rect' || key === 'size') {
      /* 位置和字号要重新取，这里只标记 */
    }
    if (key === 'fg') b.rgb = [...b.origRgb]
    if (key === 'bg') b.bg = [...b.origBg]
    if (key === 'bgMode') b.bgMode = 'solid'
    if (key === 'bgStops') b.bgStops = []
    if (key === 'align') b.align = DEF_ALIGN
    if (key === 'pad') b.pad = null
    if (key === 'size') b.size = b.origSize
  }
  if (p.rect) {
    b.rect = [...p.rect]
    b.adjusted = true
  }
  if (p.size !== undefined) {
    b.size = p.size
    b.adjusted = true
  }
  if (p.fg) b.rgb = [...p.fg]
  if (p.bg) {
    b.bg = [...p.bg]
    if (b.bgMode === 'none') b.bgMode = 'solid'
  }
  if (p.bgMode) b.bgMode = p.bgMode
  if (p.bgStops) b.bgStops = p.bgStops.map((c) => [...c] as RGB3)
  if (p.align) b.align = p.align
  if (p.pad !== undefined) b.pad = p.pad
  if (!p.clear?.length || p.fg || p.bg || p.bgMode || p.bgStops || p.align || p.pad !== undefined) b.styled = true
}
const saving = ref(false)
async function style(edits: { b: PdfBlock; patch: StylePatch }[]) {
  if (!edits.length || !lay.value) return
  for (const e of edits) localApply(e.b, e.patch)
  fitAll()
  const list: BlockEdit[] = edits.map((e) => ({ index: e.b.index, patch: e.patch }))
  saving.value = true
  try {
    await api.filetrBlockStyle(props.path, props.target, lay.value.viewPage, list)
  } catch (e) {
    ElMessage.error(errorText(e))
    void load()
  } finally {
    saving.value = false
  }
}
/** 给选中的每一块算一份改动 */
const styleSel = (mk: (b: PdfBlock) => StylePatch) => style(selBlocks.value.map((b) => ({ b, patch: mk(b) })))

const same = <T,>(f: (b: PdfBlock) => T, eq: (a: T, b: T) => boolean = (a, b) => a === b): T | null => {
  const l = selBlocks.value
  if (!l.length) return null
  const v = f(l[0])
  return l.every((b) => eq(f(b), v)) ? v : null
}
const eqRgb = (a: RGB3, b: RGB3) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2]
const fgNow = computed(() => same((b) => b.rgb, eqRgb))
const bgNow = computed(() => same((b) => b.bg, eqRgb))
const modeNow = computed(() => same((b) => b.bgMode))
const alignNow = computed(() => same((b) => b.align))
const sizeNow = computed(() => same((b) => Math.round(b.size * 10) / 10))
const padNow = computed(() => same((b) => (b.pad === null ? -1 : Math.round(b.pad * 10) / 10)))
const fgChanged = computed(() => selBlocks.value.some((b) => !eqRgb(b.rgb, b.origRgb)))
const bgChanged = computed(() => selBlocks.value.some((b) => !eqRgb(b.bg, b.origBg) || b.bgMode !== 'solid'))
const sizeChanged = computed(() => selBlocks.value.some((b) => Math.abs(b.size - b.origSize) > 0.05))
const anyStyled = computed(() => selBlocks.value.some((b) => b.styled || b.adjusted))
const stopsNow = computed(() => selBlocks.value.find((b) => b.bgStops.length >= 2)?.bgStops ?? null)
const sizeNote = computed(() => {
  const b = selBlock.value
  if (!b) return ''
  return Math.abs(b.size - b.origSize) > 0.05 ? `原 ${b.origSize.toFixed(1)} → 现 ${b.size.toFixed(1)} 磅` : `和原文一样（${b.origSize.toFixed(1)} 磅）`
})
const setSize = (v: number | undefined) => {
  if (v === undefined || !Number.isFinite(v)) return
  const n = Math.min(200, Math.max(3, Math.round(v * 10) / 10))
  void styleSel(() => ({ size: n }))
}
const bumpSize = (d: number) => styleSel((b) => ({ size: Math.min(200, Math.max(3, Math.round((b.size + d) * 10) / 10)) }))
const setMode = (m: BgMode) =>
  styleSel((b) => {
    const p: StylePatch = { bgMode: m }
    // 第一次切到渐变：用当前背景色和它稍微变暗的颜色做起点，不让它变成一片空白
    if ((m === 'hgrad' || m === 'vgrad') && b.bgStops.length < 2) {
      const dk = b.bg.map((v) => Math.max(0, Math.round(v * 0.85))) as RGB3
      p.bgStops = [[...b.bg], dk]
    }
    return p
  })
const setAlign = (a: TextAlign) => styleSel(() => ({ align: a }))
const setPad = (v: number | undefined) => {
  if (v === undefined || !Number.isFinite(v)) return
  void styleSel(() => ({ pad: Math.min(40, Math.max(0, v)) }))
}
const clearKeys = (keys: string[]) => styleSel(() => ({ clear: keys }))
async function resetSel() {
  await clearKeys(['rect', 'size', 'fg', 'bg', 'bgMode', 'bgStops', 'align', 'pad'])
  await load()
}

// ---------- 取色器 ----------
type Role = 'fg' | 'bg' | 'from' | 'to'
const picker = ref<{ role: Role } | null>(null)
const pickerOpen = computed({ get: () => picker.value !== null, set: (v: boolean) => !v && (picker.value = null) })
const rep = ref<{ from: RGB3; to: RGB3; scope: 'page' | 'file'; tol: number; count: number | null; busy: boolean }>({ from: [255, 255, 255], to: [255, 255, 255], scope: 'page', tol: 24, count: null, busy: false })
const repOpen = ref(false)
const pickerColor = computed<RGB3>(() => {
  const r = picker.value?.role
  if (r === 'from') return rep.value.from
  if (r === 'to') return rep.value.to
  if (r === 'bg') return bgNow.value ?? selBlocks.value[0]?.bg ?? [255, 255, 255]
  return fgNow.value ?? selBlocks.value[0]?.rgb ?? [0, 0, 0]
})
const pickerTitle = computed(() => {
  const n = selBlocks.value.length
  const who = n > 1 ? `选中的 ${n} 块` : '选中的这一块'
  const r = picker.value?.role
  return r === 'bg' ? `${who}的背景色` : r === 'fg' ? `${who}的文字色` : r === 'from' ? '要换掉的颜色' : '换成的颜色'
})
const pickerAgainst = computed<RGB3 | undefined>(() => {
  const r = picker.value?.role
  const b = selBlocks.value[0]
  if (!b) return undefined
  return r === 'fg' ? (bgNow.value ?? b.bg) : r === 'bg' ? (fgNow.value ?? b.rgb) : undefined
})
const pickerFocus = computed<[number, number, number, number] | null>(() => {
  const b = selBlocks.value[0]
  const l = lay.value
  if (!b || !l || picker.value?.role === 'from' || picker.value?.role === 'to') return null
  return [b.rect[0] / l.w, (l.h - b.rect[3]) / l.h, b.rect[2] / l.w, (l.h - b.rect[1]) / l.h]
})
function openPicker(role: Role) {
  picker.value = { role }
}
function onPicked(c: RGB3) {
  const r = picker.value?.role
  if (r === 'fg') void styleSel(() => ({ fg: c }))
  else if (r === 'bg') void styleSel(() => ({ bg: c }))
  else if (r === 'from') rep.value = { ...rep.value, from: c, count: null }
  else if (r === 'to') rep.value = { ...rep.value, to: c, count: null }
}
function onGradient(stops: RGB3[], dir: 'hgrad' | 'vgrad') {
  void styleSel(() => ({ bgMode: dir, bgStops: stops, bg: stops[0] }))
}

// ---------- 替换颜色：本页 / 整份文件 ----------
function openRep() {
  repOpen.value = !repOpen.value
  if (repOpen.value) {
    const b = selBlocks.value[0]
    rep.value = { ...rep.value, from: b ? [...b.rgb] : rep.value.from, to: b ? [...b.rgb] : rep.value.to, count: null }
  }
}
const repPage = () => (rep.value.scope === 'page' ? lay.value!.viewPage : null)
async function repCount() {
  rep.value.busy = true
  try {
    rep.value.count = await api.filetrReplaceColor(props.path, props.target, repPage(), [...rep.value.from], [...rep.value.to], rep.value.tol, true)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    rep.value.busy = false
  }
}
async function repDo() {
  if (rep.value.count === null) await repCount()
  const n = rep.value.count ?? 0
  if (!n) {
    ElMessage.info('没有找到这个颜色')
    return
  }
  const where = rep.value.scope === 'page' ? '这一页' : '整份文件'
  try {
    await ElMessageBox.confirm(`${where}会有 ${n} 处文字 / 背景从 ${rgbToHex(rep.value.from)} 换成 ${rgbToHex(rep.value.to)}。`, '替换颜色', { confirmButtonText: `替换 ${n} 处`, cancelButtonText: '取消' })
  } catch {
    return
  }
  rep.value.busy = true
  try {
    await api.filetrReplaceColor(props.path, props.target, repPage(), [...rep.value.from], [...rep.value.to], rep.value.tol, false)
    ElMessage.success(`已替换 ${n} 处`)
    rep.value.count = null
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    rep.value.busy = false
  }
}

// ---------- 拖动移动、拖右下角改大小（只在选中一块时） ----------
async function persist(b: PdfBlock) {
  await style([{ b, patch: { rect: [...b.rect] as [number, number, number, number], size: b.size } }])
}
let gesture: { kind: 'move' | 'size'; x: number; y: number; rect: [number, number, number, number]; b: PdfBlock } | null = null
function startGesture(e: PointerEvent, b: PdfBlock, kind: 'move' | 'size') {
  gesture = { kind, x: e.clientX, y: e.clientY, rect: [...b.rect], b }
  window.addEventListener('pointermove', onGesture)
  window.addEventListener('pointerup', endGesture, { once: true })
}
function onGesture(e: PointerEvent) {
  const g = gesture
  if (!g) return
  const pp = k.value * tf.value.s // 1 个 PDF 点 = pp 个屏幕像素
  const dx = (e.clientX - g.x) / pp
  const dy = -(e.clientY - g.y) / pp
  const [l, bt, r, t] = g.rect
  if (g.kind === 'move') g.b.rect = [l + dx, bt + dy, r + dx, t + dy]
  else g.b.rect = [l, Math.min(bt + dy, t - 6), Math.max(r + dx, l + 8), t]
  fitAll()
}
async function endGesture() {
  window.removeEventListener('pointermove', onGesture)
  const g = gesture
  gesture = null
  if (g && g.rect.some((v, i) => Math.abs(v - g.b.rect[i]) > 0.01)) await persist(g.b)
}
watch([page, () => props.path], selectNone)
onBeforeUnmount(() => window.removeEventListener('pointermove', onGesture))

async function load() {
  if (!props.path) return
  const tok = ++token
  busy.value = true
  err.value = ''
  try {
    if (mode.value === 'export') {
      const r = await api.filetrPagePreview(props.path, props.target, props.ocr, page.value)
      if (tok !== token) return
      exp.value = r
      if (!lay.value) lay.value = { pages: r.pages, viewPage: 0, w: 1, h: 1, image: '', blocks: [], done: true, note: '' }
      else lay.value.pages = r.pages
    } else {
      const r = await api.filetrPageLayout(props.path, props.target, props.ocr, page.value)
      if (tok !== token) return
      lay.value = r
      // 重新取过之后，选中的块只留还存在的
      const ids = new Set(r.blocks.map((b) => b.index))
      selSet.value = new Set([...selSet.value].filter((i) => ids.has(i)))
      await nextTick()
      fitAll()
    }
  } catch (e) {
    if (tok !== token) return
    err.value = errorText(e)
  } finally {
    if (tok === token) busy.value = false
  }
}

function resetView() {
  tf.value = { s: 1, x: 0, y: 0 }
}
function zoomAt(cx: number, cy: number, f: number) {
  const t = tf.value
  const s = Math.min(8, Math.max(0.3, t.s * f))
  const r = s / t.s
  tf.value = { s, x: cx - (cx - t.x) * r, y: cy - (cy - t.y) * r }
}
function zoomBtn(f: number) {
  const el = vp.value
  zoomAt((el?.clientWidth ?? 600) / panes.value.length / 2, (el?.clientHeight ?? 400) / 2, f)
}
function actual() {
  // 100%：一个 PDF 点 = 约 1.33 个屏幕像素（96dpi）
  const l = lay.value
  if (!l) return
  const target = (l.w * 96) / 72
  tf.value = { s: target / baseW.value, x: 0, y: 0 }
}
function onWheel(e: WheelEvent, ev: HTMLElement) {
  const r = ev.getBoundingClientRect()
  if (e.ctrlKey || e.metaKey) zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-e.deltaY * 0.0015))
  else tf.value = { ...tf.value, x: tf.value.x - e.deltaX, y: tf.value.y - e.deltaY }
}
let drag: { x: number; y: number; ox: number; oy: number } | null = null
function down(e: PointerEvent) {
  if ((e.target as HTMLElement).closest('.blk, .sel')) return
  drag = { x: e.clientX, y: e.clientY, ox: tf.value.x, oy: tf.value.y }
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
}
function move(e: PointerEvent) {
  if (!drag) return
  tf.value = { ...tf.value, x: drag.ox + e.clientX - drag.x, y: drag.oy + e.clientY - drag.y }
}
function up(e: PointerEvent) {
  // 没拖动、点在空白处：取消选择
  if (drag && Math.abs(e.clientX - drag.x) < 3 && Math.abs(e.clientY - drag.y) < 3 && !e.ctrlKey && !e.shiftKey && !e.metaKey) selectNone()
  drag = null
}

/**
 * 译文放进框里：默认保持原字号、框往下长。只有往下长会压到下面的块时，才缩小字号，最低到原来的 85%。
 */
const MIN_KEEP = 0.85
function fitAll() {
  nextTick(() => {
    const els = [...document.querySelectorAll<HTMLElement>('.pdfcv .blk')]
    for (const el of els) el.style.fontSize = `${Number(el.dataset.fs) || parseFloat(getComputedStyle(el).fontSize)}px`
    const rects = els.map((el) => el.getBoundingClientRect())
    els.forEach((el, i) => {
      const base = Number(el.dataset.fs) || parseFloat(getComputedStyle(el).fontSize)
      const hit = () => {
        const r = el.getBoundingClientRect()
        return els.some((_o, j) => {
          if (j === i) return false
          const q = rects[j]
          const below = q.top >= rects[i].top - 1 // 只看下面的块
          const ox = Math.min(r.right, q.right) - Math.max(r.left, q.left)
          return below && ox > Math.min(r.width, q.width) * 0.3 && r.bottom > q.top + 1.5 && r.top < q.top
        })
      }
      let fs = base
      let guard = 12
      while (hit() && fs > base * MIN_KEEP && guard-- > 0) {
        fs = Math.max(base * MIN_KEEP, fs * 0.97)
        el.style.fontSize = `${fs}px`
      }
    })
  })
}

async function save(b: PdfBlock, ev: FocusEvent) {
  const el = ev.target as HTMLElement
  const text = (el.innerText || '').replace(/\n{3,}/g, '\n\n').trim()
  if (text === b.trans.trim()) return
  try {
    await api.filetrEdit(props.path, props.target, lay.value!.viewPage, b.index, text)
    b.trans = text
    emit('edited', lay.value!.viewPage, b.index, text || null)
    fitAll()
  } catch (e) {
    ElMessage.error(errorText(e))
    el.innerText = b.trans
  }
}
const busyIdx = ref(-1)
async function retranslate(b: PdfBlock) {
  busyIdx.value = b.index
  try {
    b.trans = await api.filetrRetranslate(props.path, props.target, lay.value!.viewPage, b.index)
    emit('edited', lay.value!.viewPage, b.index, b.trans || null)
    fitAll()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busyIdx.value = -1
  }
}

watch([page, () => props.path, () => props.target], () => void load())
// 在 原文 / 译文 / 对比 之间切换不用重新取页面；进出“导出效果”才要
watch(mode, (m, old) => {
  if ((m === 'export') !== (old === 'export') || !lay.value?.image) void load()
})
watch(
  () => props.jobStatus,
  (st) => {
    if (st === 'done') void load()
  },
)
watch([baseW, () => lay.value?.blocks.length], () => fitAll())
watch(mode, (m, old) => {
  if (m === 'both' || old === 'both') nextTick(resetView)
})

onMounted(() => {
  void load()
  if (vp.value) {
    vw.value = vp.value.clientWidth
    ro = new ResizeObserver(() => {
      vw.value = vp.value?.clientWidth ?? vw.value
    })
    ro.observe(vp.value)
  }
})
onBeforeUnmount(() => ro?.disconnect())
</script>

<template>
  <div class="pdfcv">
    <div class="cbar">
      <el-button size="small" :disabled="page <= 0" @click="page--">上一页</el-button>
      <span class="mute">第 {{ page + 1 }} / {{ lay?.pages || '…' }} 页</span>
      <el-button size="small" :disabled="!lay || page >= lay.pages - 1" @click="page++">下一页</el-button>
      <span class="sp" />
      <div class="seg3">
        <span :class="{ on: mode === 'orig' }" @click="mode = 'orig'">原文</span>
        <span :class="{ on: mode === 'trans' }" @click="mode = 'trans'">译文</span>
        <span :class="{ on: mode === 'both' }" @click="mode = 'both'">对比</span>
        <span :class="{ on: mode === 'export' }" title="把译文真的写进 PDF 再渲染，和生成的文件一样" @click="mode = 'export'">导出效果</span>
      </div>
      <template v-if="mode !== 'export'">
        <el-button size="small" @click="zoomBtn(1 / 1.25)">－</el-button>
        <span class="mute zt">{{ Math.round(tf.s * 100) }}%</span>
        <el-button size="small" @click="zoomBtn(1.25)">＋</el-button>
        <el-button size="small" @click="resetView">适应宽度</el-button>
        <el-button size="small" @click="actual">实际大小</el-button>
      </template>
      <el-button size="small" :loading="busy" @click="load">刷新</el-button>
    </div>

    <div v-if="banner" class="banner">
      <span>{{ banner }}</span>
      <el-button v-if="todoCount && !running" size="small" type="primary" @click="emit('continue')">继续翻译</el-button>
    </div>
    <div class="vrow">
    <div ref="vp" class="vp">
      <div v-if="err" class="empty"><span class="mute">{{ err }}</span></div>
      <div v-else-if="!lay || (mode !== 'export' && !lay.image)" class="empty"><span class="mute">正在渲染页面…</span></div>
      <template v-else-if="mode === 'export'">
        <div v-if="!exp" class="empty"><span class="mute">正在把译文写进页面…</span></div>
        <template v-else>
          <figure>
            <figcaption>原页</figcaption>
            <img :src="exp.orig" alt="原页" />
          </figure>
          <figure>
            <figcaption>导出效果（{{ exp.items }} 处译文）</figcaption>
            <img :src="exp.trans" alt="导出效果" />
          </figure>
        </template>
      </template>
      <template v-else>
        <div
          v-for="p in panes"
          :key="p"
          class="pane"
          :style="{ width: baseW + 8 + 'px' }"
          @wheel.prevent="(e) => onWheel(e, e.currentTarget as HTMLElement)"
          @pointerdown="down"
          @pointermove="move"
          @pointerup="up"
          @pointercancel="up"
        >
          <div class="lbl">{{ p === 'orig' ? '原文' : '译文' }}</div>
          <div class="stage" :style="stageStyle">
            <img :src="lay.image" draggable="false" alt="" />
            <template v-if="p === 'trans'">
              <div
                v-for="b in shown"
                :key="b.index"
                class="blk"
                :class="{ todo: !b.trans.trim(), on: selSet.has(b.index) }"
                :data-idx="b.index"
                :data-fs="Math.max(3, b.size * k)"
                :style="blkStyle(b)"
                :title="(b.trans.trim() ? '点一下直接改；Ctrl / Shift + 点击多选；' : '未译 · 点一下可以自己写；') + (b.src.length > 60 ? b.src.slice(0, 60) + '…' : b.src)"
                contenteditable="plaintext-only"
                spellcheck="false"
                @pointerdown="blkDown($event, b)"
                @focus="blkFocus(b)"
                @blur="save(b, $event)"
                @keydown.esc.prevent="($event.target as HTMLElement).blur()"
                @dblclick.stop="retranslate(b)"
              >{{ b.trans }}</div>
              <div v-if="selBlock" class="sel" :style="selStyle(selBlock)">
                <span class="grip" title="按住拖动，移动这一块" @pointerdown.stop.prevent="startGesture($event, selBlock, 'move')">✥</span>
                <span class="rsz" title="按住拖动，改这一块的大小" @pointerdown.stop.prevent="startGesture($event, selBlock, 'size')" />
              </div>
            </template>
          </div>
        </div>
      </template>
    </div>

    <aside v-if="lay && lay.blocks.length && mode !== 'export'" class="side" aria-label="这一页的段落和样式">
      <section class="insp" aria-label="样式">
        <div class="ih">
          <b>{{ multi ? `已选 ${selBlocks.length} 块` : selBlock ? '选中的这一块' : '样式' }}</b>
          <span class="sp" />
          <el-button size="small" text @click="selectAll">全选本页</el-button>
          <el-button v-if="selBlocks.length" size="small" text @click="selectNone">清除</el-button>
        </div>
        <template v-if="selBlocks.length">
          <div class="ir">
            <span class="il">文字色</span>
            <button class="swb" :title="fgNow ? rgbToHex(fgNow) : '几块颜色不一样'" @click="openPicker('fg')">
              <i :style="{ background: fgNow ? rgbCss(fgNow) : 'repeating-linear-gradient(45deg,#ccc 0 4px,#fff 4px 8px)' }" />{{ fgNow ? rgbToHex(fgNow) : '混合' }}
            </button>
            <span v-if="!fgChanged" class="chip">和原文一致</span>
            <el-button v-else size="small" text @click="clearKeys(['fg'])">还原</el-button>
          </div>
          <div class="ir">
            <span class="il">背景</span>
            <button class="swb" :title="bgNow ? rgbToHex(bgNow) : '几块颜色不一样'" @click="openPicker('bg')">
              <i :style="{ background: modeNow === 'hgrad' || modeNow === 'vgrad' ? gradientCss(stopsNow ?? [], modeNow) : bgNow ? rgbCss(bgNow) : 'repeating-linear-gradient(45deg,#ccc 0 4px,#fff 4px 8px)' }" />{{ modeNow === 'hgrad' || modeNow === 'vgrad' ? '渐变' : modeNow === 'none' ? '无' : bgNow ? rgbToHex(bgNow) : '混合' }}
            </button>
            <span v-if="!bgChanged" class="chip">取自原页</span>
            <el-button v-else size="small" text @click="clearKeys(['bg', 'bgMode', 'bgStops'])">还原</el-button>
          </div>
          <div class="ir">
            <span class="il" />
            <div class="seg">
              <span :class="{ on: modeNow === 'solid' }" @click="setMode('solid')">纯色</span>
              <span :class="{ on: modeNow === 'hgrad' }" title="左 → 右渐变" @click="setMode('hgrad')">横渐变</span>
              <span :class="{ on: modeNow === 'vgrad' }" title="上 → 下渐变" @click="setMode('vgrad')">竖渐变</span>
              <span :class="{ on: modeNow === 'none' }" title="不盖原文，只放文字" @click="setMode('none')">无</span>
            </div>
          </div>
          <div class="ir">
            <span class="il">字号</span>
            <el-button size="small" aria-label="字号减小" @click="bumpSize(-0.5)">－</el-button>
            <el-input-number :model-value="sizeNow ?? undefined" :min="3" :max="200" :step="0.5" :precision="1" size="small" :controls="false" :placeholder="sizeNow === null ? '混合' : ''" class="szin" @change="setSize" />
            <el-button size="small" aria-label="字号增大" @click="bumpSize(0.5)">＋</el-button>
            <span class="mute">磅</span>
          </div>
          <div v-if="selBlock" class="ir"><span class="il" /><span class="chip" :class="{ warn: sizeChanged }">{{ sizeNote }}</span><el-button v-if="sizeChanged" size="small" text @click="clearKeys(['size'])">还原字号</el-button></div>
          <div class="ir">
            <span class="il">对齐</span>
            <div class="seg">
              <span :class="{ on: alignNow === 'left' }" @click="setAlign('left')">靠左</span>
              <span :class="{ on: alignNow === 'center' }" @click="setAlign('center')">居中</span>
              <span :class="{ on: alignNow === 'right' }" @click="setAlign('right')">靠右</span>
            </div>
          </div>
          <div class="ir">
            <span class="il">外扩</span>
            <el-input-number :model-value="padNow !== null && padNow >= 0 ? padNow : undefined" :min="0" :max="40" :step="0.5" :precision="1" size="small" :controls="false" :placeholder="padNow === -1 ? '自动' : '混合'" class="szin" @change="setPad" />
            <span class="mute">磅</span>
            <el-button v-if="padNow !== -1" size="small" text @click="clearKeys(['pad'])">自动</el-button>
          </div>
          <div class="ir"><el-button size="small" :disabled="!anyStyled" @click="resetSel">还原位置、大小和样式</el-button></div>
          <small v-if="selBlock" class="mute">拖 ✥ 移动，拖右下角改大小；默认保持原字号，往下长，压到下面的块才缩小（最低 85%）。</small>
        </template>
        <small v-else class="mute">点一块译文看它的样式；Ctrl / Shift + 点击可以多选，一起改。</small>

        <div class="rp">
          <div class="adv" role="button" tabindex="0" @click="openRep" @keydown.enter="openRep">{{ repOpen ? '▾' : '▸' }} 替换颜色（本页 / 整份文件）</div>
          <template v-if="repOpen">
            <div class="ir">
              <button class="swb" @click="openPicker('from')"><i :style="{ background: rgbCss(rep.from) }" />{{ rgbToHex(rep.from) }}</button>
              <span>→</span>
              <button class="swb" @click="openPicker('to')"><i :style="{ background: rgbCss(rep.to) }" />{{ rgbToHex(rep.to) }}</button>
            </div>
            <div class="ir">
              <span class="il">范围</span>
              <div class="seg"><span :class="{ on: rep.scope === 'page' }" @click="((rep.scope = 'page'), (rep.count = null))">本页</span><span :class="{ on: rep.scope === 'file' }" @click="((rep.scope = 'file'), (rep.count = null))">整份文件</span></div>
            </div>
            <div class="ir"><span class="il">相近度</span><el-slider v-model="rep.tol" :min="0" :max="60" size="small" class="grow" @change="rep.count = null" /></div>
            <div class="ir">
              <el-button size="small" :loading="rep.busy" @click="repCount">查看有几处</el-button>
              <span v-if="rep.count !== null" class="chip">{{ rep.count }} 处</span>
              <span class="sp" />
              <el-button size="small" type="primary" :disabled="rep.busy || rep.count === 0" @click="repDo">替换…</el-button>
            </div>
          </template>
        </div>
      </section>

      <section class="lst" aria-label="这一页的段落">
        <b>这一页的段落</b>
        <button v-for="b in lay.blocks" :key="b.index" class="row" :class="{ on: selSet.has(b.index) }" :title="b.src" @click="focusBlock(b.index, $event)">
          <i :class="b.skipped ? 'sk' : b.trans.trim() ? 'ok' : 'td'" />
          <span class="tx">{{ b.src.length > 28 ? b.src.slice(0, 28) + '…' : b.src }}</span>
          <em v-if="b.skipped">不译</em>
          <em v-else-if="!b.trans.trim()" class="td">未译</em>
          <em v-else-if="b.styled || b.adjusted" class="ed" title="手动调过">已调</em>
        </button>
      </section>
    </aside>
    </div>
    <div class="tip">
      Ctrl（⌘）+ 滚轮以鼠标为中心缩放，滚轮上下平移，按住拖动平移；点一块译文直接改（改完点别处保存），双击这一块可以重译；Ctrl / Shift + 点击多选。
      <template v-if="lay && !lay.blocks.some((b) => b.trans.trim()) && mode !== 'export'">这一页没有译文块。</template>
      <template v-if="busyIdx >= 0"> 重译中…</template>
      <template v-if="saving"> 保存中…</template>
    </div>

    <ColorPicker
      v-model="pickerOpen"
      :title="pickerTitle"
      :color="pickerColor"
      :image="lay?.image"
      :against="pickerAgainst"
      :against-label="picker?.role === 'bg' ? '文字' : '背景'"
      :focus="pickerFocus"
      :gradient="picker?.role === 'bg'"
      :stops="stopsNow ?? undefined"
      @apply="onPicked"
      @apply-gradient="onGradient"
    />
  </div>
</template>

<style scoped>
.pdfcv { flex: 1; min-height: 0; display: flex; flex-direction: column }
.cbar { display: flex; gap: 6px; align-items: center; padding: 8px 12px; flex-wrap: wrap; border-bottom: 1px solid var(--cc-line) }
.cbar .sp, .ih .sp { flex: 1 }
.zt { min-width: 44px; text-align: center }
.seg3 { display: flex; background: var(--cc-inset); border: 1px solid var(--cc-line); border-radius: 8px; padding: 2px; gap: 2px }
.seg3 span { padding: 3px 12px; border-radius: 6px; font-size: 13px; cursor: pointer; color: var(--cc-mute); white-space: nowrap }
.seg3 .on { background: var(--cc-amber); color: var(--cc-on-acc); font-weight: 700 }
.vrow { flex: 1; min-height: 0; display: flex }
.banner { display: flex; align-items: center; gap: 10px; padding: 7px 14px; background: var(--cc-acc-soft); border-bottom: 1px solid var(--cc-acc-line); color: var(--cc-amber); font-size: 12.5px }
.banner span { flex: 1 }
.side { width: 292px; flex: none; border-left: 1px solid var(--cc-line-soft); display: flex; flex-direction: column; overflow: hidden; font-size: 12px; background: var(--cc-fill) }
.insp { padding: 10px; display: flex; flex-direction: column; gap: 7px; border-bottom: 1px solid var(--cc-line-soft); overflow-y: auto; max-height: 62% }
.ih { display: flex; align-items: center; gap: 2px }
.ir { display: flex; align-items: center; gap: 6px; flex-wrap: wrap }
.il { width: 44px; flex: none; color: var(--cc-mute) }
.swb { display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--cc-line); background: var(--cc-card); border-radius: 6px; padding: 2px 8px 2px 3px; cursor: pointer; font: 12px ui-monospace, Menlo, Consolas, monospace; color: var(--cc-fg); min-width: 96px }
.swb:hover { border-color: var(--cc-amber) }
.swb i { width: 22px; height: 18px; border-radius: 4px; border: 1px solid rgba(0, 0, 0, .25); flex: none }
.chip { font-size: 11.5px; padding: 1px 8px; border-radius: 10px; background: var(--cc-fill); color: var(--cc-mute) }
.chip.warn { background: var(--cc-acc-soft); color: var(--cc-amber) }
.seg { display: flex; background: var(--cc-inset); border: 1px solid var(--cc-line); border-radius: 8px; padding: 2px; gap: 2px }
.seg span { padding: 2px 8px; border-radius: 6px; cursor: pointer; color: var(--cc-mute); white-space: nowrap }
.seg .on { background: var(--cc-amber); color: var(--cc-on-acc); font-weight: 700 }
.szin { width: 62px }
.grow { flex: 1; margin: 0 6px }
.adv { color: var(--cc-amber); cursor: pointer; user-select: none; margin-top: 2px }
.rp { display: flex; flex-direction: column; gap: 7px; border-top: 1px dashed var(--cc-line); padding-top: 7px }
.lst { flex: 1; min-height: 0; overflow-y: auto; padding: 10px; display: flex; flex-direction: column; gap: 3px }
.side .row { display: flex; gap: 6px; align-items: center; border: 0; background: none; text-align: left; padding: 3px 4px; border-radius: 6px; cursor: pointer; color: inherit; font: inherit }
.side .row:hover { background: var(--cc-fill) }
.side .row.on { background: var(--cc-acc-soft) }
.side i { width: 8px; height: 8px; border-radius: 50%; flex: none; background: #b7beb9 }
.side i.ok { background: #2f8a57 }
.side i.td { background: #d9762b }
.side .tx { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.side em { font-style: normal; color: var(--cc-mute) }
.side em.td { color: #b4561a; font-weight: 700 }
.side em.ed { color: var(--cc-amber) }
.mute { color: var(--cc-mute); font-size: 12px }
.sel { position: absolute; pointer-events: none; z-index: 5; border: 1.5px solid var(--cc-amber) }
.sel .grip { position: absolute; left: -1px; top: -20px; height: 18px; min-width: 22px; padding: 0 4px; background: var(--cc-amber); color: var(--cc-on-acc); border-radius: 4px 4px 0 0; font-size: 12px; line-height: 18px; text-align: center; cursor: move; pointer-events: auto; user-select: none }
.sel .rsz { position: absolute; right: -6px; bottom: -6px; width: 12px; height: 12px; background: var(--cc-amber); border: 2px solid #fff; border-radius: 3px; cursor: nwse-resize; pointer-events: auto }
.blk.todo { border: 1.5px dashed #d9762b; min-height: 6px }
.blk.on { outline: 1.5px solid var(--cc-amber); outline-offset: 0 }
.vp { flex: 1; min-height: 0; display: flex; gap: 10px; overflow: hidden; background: var(--cc-hover); padding: 4px; position: relative }
.vp > figure { flex: 1; margin: 0; display: flex; flex-direction: column; gap: 6px; overflow: auto; min-width: 0 }
.vp figcaption { font-size: 12.5px; font-weight: 700; color: var(--cc-mute) }
.vp figure img { width: 100%; height: auto; border: 1px solid var(--cc-line); background: #fff }
.pane { position: relative; overflow: hidden; cursor: grab; touch-action: none; flex: none }
.pane:active { cursor: grabbing }
.lbl { position: absolute; left: 6px; top: 4px; z-index: 3; font-size: 12px; font-weight: 700; padding: 1px 8px; border-radius: 10px; background: rgba(0, 0, 0, 0.55); color: #fff; pointer-events: none }
.stage { position: relative; transform-origin: 0 0; margin: 4px; box-shadow: 0 1px 8px rgba(0, 0, 0, 0.25); background: #fff }
.stage img { width: 100%; height: 100%; display: block; user-select: none; -webkit-user-drag: none }
.blk { position: absolute; box-sizing: border-box; display: flex; align-items: center; line-height: 1.25; cursor: text; white-space: pre-wrap; word-break: break-word; outline: 0 }
.blk:hover { outline: 1px dashed var(--cc-amber) }
.blk:focus { outline: 2px solid var(--cc-amber); z-index: 2 }
.empty { margin: auto }
.tip { font-size: 12px; color: var(--cc-mute); padding: 4px 12px }
</style>
