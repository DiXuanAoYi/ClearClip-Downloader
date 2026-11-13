<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { api, errorText } from './api'

/** 截图翻译（桌面端）：快捷键截下整屏并冻结，框选一块，交给视觉模型识字、翻译，译文盖回原位。 */
type Block = { text: string; translation: string; rect: [number, number, number, number] | null }
type Rect = { x: number; y: number; w: number; h: number }

const phase = ref<'loading' | 'select' | 'busy' | 'done' | 'error'>('loading')
const shotUrl = ref('')
const sel = ref<Rect | null>(null)
const blocks = ref<Block[]>([])
const message = ref('')
const cover = ref(true)
const toastText = ref('')
const histAt = ref(0)
const saved = ref(false)
const cmp = ref<{ name: string; text: string; err?: boolean }[]>([])
const cmpBusy = ref(false)
// 识别方式：视觉模型一步完成（默认），或本机识字（屏幕翻译里选的那种）+ 翻译模型，位置更准、不发图片给模型
const readOcr = () => {
  try {
    return localStorage.getItem('shotOcr') === 'local' ? 'local' : 'vision'
  } catch {
    return 'vision'
  }
}
const ocrMode = ref<'vision' | 'local'>(readOcr())
function toggleOcr() {
  ocrMode.value = ocrMode.value === 'local' ? 'vision' : 'local'
  try {
    localStorage.setItem('shotOcr', ocrMode.value)
  } catch {
    /* 记不住也能用 */
  }
  if (sel.value) void run(sel.value)
}
let langName = '简体中文'
const out = ref<HTMLCanvasElement | null>(null)
let img: HTMLImageElement | null = null
let base: HTMLCanvasElement | null = null
let start: { x: number; y: number } | null = null
let jobId = 0
let alive = true

const MAX_CHARS = 1_800_000

const loadImage = (src: string) =>
  new Promise<HTMLImageElement>((ok, bad) => {
    const im = new Image()
    im.onload = () => ok(im)
    im.onerror = () => bad(new Error('截图读取失败'))
    im.src = src
  })

onMounted(async () => {
  window.addEventListener('keydown', onKey, true)
  const info = await api.shotTake().catch(() => null)
  if (!info) return api.shotClose()
  shotUrl.value = info.dataUrl
  try {
    img = await loadImage(info.dataUrl)
    phase.value = 'select'
  } catch (e) {
    message.value = errorText(e)
    phase.value = 'error'
  }
})
onBeforeUnmount(() => {
  alive = false
  window.removeEventListener('keydown', onKey, true)
})

function close() {
  if (jobId) void api.writeCancel(jobId)
  void api.shotClose()
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    close()
  } else if (e.key === 'Enter' && sel.value && (phase.value === 'done' || phase.value === 'error')) {
    e.preventDefault()
    void run(sel.value)
  }
}
async function pin() {
  const cv = out.value
  const r = sel.value
  if (!cv || !r) return
  try {
    await api.shotPin(cv.toDataURL('image/png'), r.x, r.y, r.w, r.h)
    close()
  } catch (e) {
    tip(errorText(e))
  }
}
async function saveNote() {
  if (!histAt.value) return tip('历史记录已在设置里关闭')
  saved.value = !saved.value
  await api.shotHistSave(histAt.value, saved.value).catch(() => {})
  tip(saved.value ? '已存为笔记（设置 → 剪贴板里查看）' : '已取消笔记')
}
async function compare() {
  if (cmpBusy.value) return
  cmpBusy.value = true
  const src = blocks.value.map((b) => b.text).join('\n')
  cmp.value = [{ name: '清影大模型', text: blocks.value.map((b) => b.translation).join('\n') }]
  try {
    cmp.value.push({ name: '通用翻译', text: await api.shotCompare(src, langName) })
  } catch (e) {
    cmp.value.push({ name: '通用翻译', text: errorText(e), err: true })
  } finally {
    cmpBusy.value = false
  }
}
function speak() {
  const t = blocks.value.map((b) => b.translation).join('。')
  if (!t || !('speechSynthesis' in window)) return tip('这台电脑没有朗读功能')
  speechSynthesis.cancel()
  speechSynthesis.speak(new SpeechSynthesisUtterance(t))
}

const rectOf = (e: MouseEvent): Rect => {
  const s = start ?? { x: e.clientX, y: e.clientY }
  return { x: Math.min(s.x, e.clientX), y: Math.min(s.y, e.clientY), w: Math.abs(e.clientX - s.x), h: Math.abs(e.clientY - s.y) }
}
function down(e: MouseEvent) {
  if (phase.value !== 'select' || e.button !== 0) return
  start = { x: e.clientX, y: e.clientY }
  sel.value = { x: e.clientX, y: e.clientY, w: 0, h: 0 }
}
function move(e: MouseEvent) {
  if (start) sel.value = rectOf(e)
}
function up(e: MouseEvent) {
  if (!start) return
  const r = rectOf(e)
  start = null
  if (r.w < 12 || r.h < 12) {
    sel.value = null
    return
  }
  sel.value = r
  void run(r)
}

/** 把选区裁出来，缩到 1400 宽以内，压成 JPEG；太大就继续缩，直到放得进请求。 */
function crop(r: Rect): { url: string; w: number; h: number; c: HTMLCanvasElement } | null {
  if (!img) return null
  const kx = img.naturalWidth / innerWidth
  const ky = img.naturalHeight / innerHeight
  const sx = Math.round(r.x * kx)
  const sy = Math.round(r.y * ky)
  const sw = Math.max(1, Math.round(r.w * kx))
  const sh = Math.max(1, Math.round(r.h * ky))
  let scale = Math.min(1, 1400 / sw)
  for (let i = 0; i < 6; i++) {
    const c = document.createElement('canvas')
    c.width = Math.max(1, Math.round(sw * scale))
    c.height = Math.max(1, Math.round(sh * scale))
    c.getContext('2d')?.drawImage(img, sx, sy, sw, sh, 0, 0, c.width, c.height)
    const url = c.toDataURL('image/jpeg', 0.85)
    if (url.length <= MAX_CHARS) return { url, w: c.width, h: c.height, c }
    scale *= 0.75
  }
  return null
}

async function run(r: Rect) {
  const c = crop(r)
  if (!c) {
    message.value = '选区太大，请框小一点'
    phase.value = 'error'
    return
  }
  base = c.c
  phase.value = 'busy'
  message.value = '正在识别并翻译……'
  try {
    const settings = await api.getSettings()
    langName = settings.ai.targetLang || '简体中文'
    cmp.value = []
    saved.value = false
    jobId = await api.shotRun({ kind: 'imgtrans', image: c.url, lang: langName, ocr: ocrMode.value })
    for (let n = 0; n < 240 && alive; n++) {
      await new Promise((ok) => setTimeout(ok, 500))
      const j = await api.writeJob(jobId)
      if (!j) throw new Error('任务丢失')
      if (j.state === 'error') throw new Error(j.error || '翻译失败')
      if (j.state === 'done') {
        const list = ((j.result as { blocks?: Block[] } | null)?.blocks ?? []).filter((b) => b && b.translation)
        jobId = 0
        if (!list.length) throw new Error('没有识别到文字')
        blocks.value = list
        phase.value = 'done'
        await nextTick()
        draw()
        histAt.value = await api.shotHistAdd(list.map((b) => b.text).join('\n'), list.map((b) => b.translation).join('\n')).catch(() => 0)
        if (settings.shotAutoCopy) void copyText(list.map((b) => b.translation).join('\n'))
        return
      }
    }
    throw new Error('翻译超时')
  } catch (e) {
    jobId = 0
    message.value = errorText(e)
    phase.value = 'error'
  }
}

function sample(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number) {
  try {
    const d = ctx.getImageData(Math.max(0, Math.floor(x)), Math.max(0, Math.floor(y)), Math.max(1, Math.floor(w)), Math.max(1, Math.floor(h))).data
    let r = 0
    let g = 0
    let b = 0
    let n = 0
    const step = Math.max(1, Math.floor(d.length / 4 / 400)) * 4
    for (let i = 0; i < d.length; i += step) {
      r += d[i]
      g += d[i + 1]
      b += d[i + 2]
      n++
    }
    r = Math.round(r / n)
    g = Math.round(g / n)
    b = Math.round(b / n)
    return { bg: `rgb(${r},${g},${b})`, fg: 0.299 * r + 0.587 * g + 0.114 * b > 140 ? '#111' : '#fff' }
  } catch {
    return { bg: '#fff', fg: '#111' }
  }
}

/** 英文按词、中日韩按字折行 */
function wrap(c: CanvasRenderingContext2D, text: string, maxW: number): string[] {
  const toks = text.match(/[　-鿿＀-￯]|\S+\s*|\s+/g) ?? [text]
  const lines: string[] = []
  let cur = ''
  for (const t of toks) {
    if (c.measureText(cur + t).width > maxW && cur) {
      lines.push(cur.trimEnd())
      cur = t.trimStart()
    } else cur += t
  }
  if (cur) lines.push(cur.trimEnd())
  return lines
}

function draw() {
  const cv = out.value
  if (!cv || !base) return
  cv.width = base.width
  cv.height = base.height
  const c = cv.getContext('2d')
  const bc = base.getContext('2d')
  if (!c || !bc) return
  c.drawImage(base, 0, 0)
  if (!cover.value) return
  for (const b of blocks.value) {
    if (!b.rect) continue
    const [rx, ry, rw, rh] = b.rect
    const px = rx * base.width
    const py = ry * base.height
    const pw = rw * base.width
    const ph = rh * base.height
    const col = sample(bc, px, py, pw, ph)
    c.fillStyle = col.bg
    c.fillRect(px, py, pw, ph)
    c.fillStyle = col.fg
    c.textAlign = 'center'
    c.textBaseline = 'top'
    let size = Math.max(10, Math.min(ph * 0.9, 48))
    let lines: string[] = []
    for (; size >= 8; size -= 1) {
      c.font = `600 ${size}px system-ui, "Microsoft YaHei", sans-serif`
      lines = wrap(c, b.translation, pw)
      if (lines.length * size * 1.15 <= ph + 1) break
    }
    const lh = size * 1.15
    let y = py + Math.max(0, (ph - lines.length * lh) / 2)
    for (const ln of lines) {
      c.fillText(ln, px + pw / 2, y, pw)
      y += lh
    }
  }
}

function setCover(v: boolean) {
  cover.value = v
  draw()
}

function tip(t: string) {
  toastText.value = t
  setTimeout(() => (toastText.value = ''), 1400)
}
async function copyText(t: string) {
  try {
    await navigator.clipboard.writeText(t)
    tip('已复制')
  } catch {
    tip('复制失败')
  }
}
async function copyImage() {
  const cv = out.value
  if (!cv) return
  try {
    const blob = await new Promise<Blob | null>((ok) => cv.toBlob(ok, 'image/png'))
    if (!blob) throw new Error('x')
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })])
    tip('图片已复制')
  } catch {
    tip('复制图片失败')
  }
}

const frame = computed(() => {
  const r = sel.value
  return r ? { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` } : {}
})
const barStyle = computed(() => {
  const r = sel.value
  if (!r) return {}
  const below = r.y + r.h + 56 < innerHeight
  const left = Math.max(8, Math.min(r.x, innerWidth - 700))
  return { left: `${left}px`, top: below ? `${r.y + r.h + 8}px` : `${Math.max(8, r.y - 46)}px` }
})
const listStyle = computed(() => {
  const r = sel.value
  if (!r) return {}
  const right = r.x + r.w + 12 + 320 < innerWidth
  return right ? { left: `${r.x + r.w + 12}px`, top: `${r.y}px` } : { left: `${Math.max(8, r.x)}px`, top: `${Math.min(innerHeight - 240, r.y + r.h + 60)}px` }
})
</script>

<template>
  <div class="root" @mousedown="down" @mousemove="move" @mouseup="up" @contextmenu.prevent="close">
    <img v-if="shotUrl" class="bg" :src="shotUrl" draggable="false" />
    <div v-if="phase === 'select' && !sel" class="tip">拖动框选要翻译的区域 · Esc / 右键 退出</div>
    <div v-if="sel && phase !== 'done'" class="frame" :class="{ busy: phase === 'busy' }" :style="frame" />
    <canvas v-show="phase === 'done'" ref="out" class="result" :style="frame" />
    <div v-if="phase === 'busy' || phase === 'error'" class="bar" :style="barStyle" @mousedown.stop>
      <span :class="{ err: phase === 'error' }">{{ message }}</span>
      <button v-if="phase === 'error'" @click="close">关闭</button>
      <button v-else @click="close">取消</button>
    </div>
    <template v-if="phase === 'done'">
      <div class="bar" :style="barStyle" @mousedown.stop>
        <button :class="{ on: cover }" @click="setCover(true)">译文覆盖</button>
        <button :class="{ on: !cover }" @click="setCover(false)">原图</button>
        <button @click="copyText(blocks.map((b) => b.translation).join('\n'))">复制译文</button>
        <button @click="copyText(blocks.map((b) => b.text).join('\n'))">复制原文</button>
        <button @click="speak">朗读</button>
        <button :disabled="cmpBusy" @click="compare">对比服务</button>
        <button :title="ocrMode === 'local' ? '现在：本机识字 + 翻译模型（点一下换成视觉模型重新识别）' : '现在：视觉模型（点一下换成本机识字重新识别）'" @click="toggleOcr">{{ ocrMode === 'local' ? '识别：本机' : '识别：视觉模型' }}</button>
        <button @click="pin">钉在桌面</button>
        <button :class="{ on: saved }" @click="saveNote">{{ saved ? '已存笔记' : '存为笔记' }}</button>
        <button @click="copyImage">复制图片</button>
        <button @click="close">关闭</button>
      </div>
      <div class="list" :style="listStyle" @mousedown.stop>
        <div v-if="cmp.length" class="cmp">
          <div v-for="c in cmp" :key="c.name" class="ci" :class="{ err: c.err }" @click="!c.err && copyText(c.text)">
            <b>{{ c.name }}</b>
            <div>{{ c.text }}</div>
          </div>
        </div>
        <div v-for="(b, i) in blocks" :key="i" class="row" @click="copyText(b.translation)">
          <div class="o">{{ b.text }}</div>
          <div class="t">{{ b.translation }}</div>
        </div>
      </div>
    </template>
    <div v-if="phase === 'done'" class="keys">Esc 关闭 · Enter 重新翻译 · 点译文行复制该段</div>
    <div v-if="toastText" class="toast">{{ toastText }}</div>
  </div>
</template>

<style scoped>
.root { position: fixed; inset: 0; overflow: hidden; cursor: crosshair; user-select: none; background: #000; }
.bg { position: absolute; inset: 0; width: 100%; height: 100%; filter: brightness(0.6); pointer-events: none; }
.tip { position: absolute; left: 50%; top: 24px; transform: translateX(-50%); background: #0b1424; color: #fff; border-radius: 8px; padding: 6px 14px; font-size: 13px; pointer-events: none; }
.frame { position: absolute; border: 2px solid #ffb547; box-shadow: 0 0 0 9999px #0000; background: transparent; pointer-events: none; overflow: hidden; }
.result { position: absolute; cursor: default; box-shadow: 0 0 0 2px #ffb547, 0 8px 30px #0008; }
.bar { position: absolute; display: flex; gap: 6px; align-items: center; background: #fff; color: #0b1424; border-radius: 8px; padding: 6px 8px; box-shadow: 0 6px 24px #0006; font-size: 13px; cursor: default; }
.bar button { border: 1px solid #aebfd8; background: #fff; border-radius: 6px; padding: 3px 9px; cursor: pointer; font: inherit; color: inherit; }
.bar button.on { background: #ffb547; border-color: #ffb547; color: #fff; }
.err { color: #c4283d; }
.list { position: absolute; width: 320px; max-height: 60vh; overflow: auto; background: #fff; color: #0b1424; border-radius: 10px; padding: 6px 12px; box-shadow: 0 6px 24px #0006; font-size: 13px; line-height: 1.5; cursor: default; }
.row { padding: 6px 0; border-top: 1px solid #eee; cursor: pointer; }
.row:first-child { border-top: 0; }
.o { color: #777; font-size: 12px; }
.t { font-weight: 600; }
.cmp { border-bottom: 2px solid #ffb547; margin-bottom: 4px; }
.ci { padding: 6px 0; border-top: 1px solid #eee; cursor: pointer; }
.ci:first-child { border-top: 0; }
.ci b { font-size: 12px; color: #b45f00; }
.keys { position: absolute; left: 50%; bottom: 12px; transform: translateX(-50%); background: #1d2420cc; color: #fff; padding: 4px 12px; border-radius: 999px; font-size: 12px; pointer-events: none; }
.toast { position: absolute; left: 50%; bottom: 40px; transform: translateX(-50%); background: #0b1424; color: #fff; padding: 6px 14px; border-radius: 8px; font-size: 13px; }
</style>
