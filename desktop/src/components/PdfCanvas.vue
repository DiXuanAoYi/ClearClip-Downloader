<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { PdfLayout } from '../types'

/**
 * PDF 版面画布：原页图 + 每块的译文（盖上原文所在处的背景色，放回原来的位置）。
 * 可以切换看原文 / 译文 / 左右对比 / 导出效果（真的写进 PDF 再渲染），滚轮缩放（Ctrl/⌘+滚轮 = 以鼠标为中心缩放，
 * 普通滚轮 = 上下平移）、拖动平移，点一块译文直接改。
 */
const props = defineProps<{ path: string; target: string; ocr: string; jobStatus?: string }>()
const emit = defineEmits<{ (e: 'edited', viewPage: number, index: number, text: string | null): void }>()

type Mode = 'orig' | 'trans' | 'both' | 'export'
const mode = ref<Mode>('trans')
const page = ref(0)
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

const rgb = (c: number[]) => `rgb(${c[0]},${c[1]},${c[2]})`
function blkStyle(b: PdfLayout['blocks'][number]) {
  const l = lay.value!
  const [bl, bb, br, bt] = b.rect
  return {
    left: `${(bl / l.w) * 100}%`,
    top: `${((l.h - bt) / l.h) * 100}%`,
    width: `${((br - bl) / l.w) * 100}%`,
    height: `${((bt - bb) / l.h) * 100}%`,
    background: rgb(b.bg),
    color: rgb(b.rgb),
    fontSize: `${Math.max(3, b.size * k.value)}px`,
  }
}

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
      if (!lay.value) lay.value = { pages: r.pages, viewPage: 0, w: 1, h: 1, image: '', blocks: [] }
      else lay.value.pages = r.pages
    } else {
      const r = await api.filetrPageLayout(props.path, props.target, props.ocr, page.value)
      if (tok !== token) return
      lay.value = r
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
  if ((e.target as HTMLElement).closest('.blk')) return
  drag = { x: e.clientX, y: e.clientY, ox: tf.value.x, oy: tf.value.y }
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
}
function move(e: PointerEvent) {
  if (!drag) return
  tf.value = { ...tf.value, x: drag.ox + e.clientX - drag.x, y: drag.oy + e.clientY - drag.y }
}
function up() {
  drag = null
}

// 译文放进框里：放不下就缩小字号，直到放得下（和导出时的做法一致）
function fitAll() {
  nextTick(() => {
    document.querySelectorAll<HTMLElement>('.pdfcv .blk').forEach((el) => {
      const base = Number(el.dataset.fs) || parseFloat(getComputedStyle(el).fontSize)
      let fs = base
      el.style.fontSize = `${fs}px`
      let guard = 40
      while (el.scrollHeight > el.clientHeight * 1.06 + 1 && fs > Math.max(3, base * 0.4) && guard-- > 0) {
        fs *= 0.93
        el.style.fontSize = `${fs}px`
      }
    })
  })
}

async function save(b: PdfLayout['blocks'][number], ev: FocusEvent) {
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
async function retranslate(b: PdfLayout['blocks'][number]) {
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
                v-for="b in lay.blocks.filter((x) => x.trans.trim())"
                :key="b.index"
                class="blk"
                :data-fs="Math.max(3, b.size * k)"
                :style="blkStyle(b)"
                :title="'点一下直接改；' + (b.src.length > 60 ? b.src.slice(0, 60) + '…' : b.src)"
                contenteditable="plaintext-only"
                spellcheck="false"
                @blur="save(b, $event)"
                @keydown.esc.prevent="($event.target as HTMLElement).blur()"
                @dblclick.stop="retranslate(b)"
              >{{ b.trans }}</div>
            </template>
          </div>
        </div>
      </template>
    </div>
    <div class="tip">
      Ctrl（⌘）+ 滚轮以鼠标为中心缩放，滚轮上下平移，按住拖动平移；点一块译文直接改（改完点别处保存），双击这一块可以重译。
      <template v-if="lay && !lay.blocks.some((b) => b.trans.trim()) && mode !== 'export'">这一页没有译文块。</template>
      <template v-if="busyIdx >= 0"> 重译中…</template>
    </div>
  </div>
</template>

<style scoped>
.pdfcv { flex: 1; min-height: 0; display: flex; flex-direction: column }
.cbar { display: flex; gap: 6px; align-items: center; padding: 8px 12px; flex-wrap: wrap; border-bottom: 1px solid var(--el-border-color) }
.cbar .sp { flex: 1 }
.zt { min-width: 44px; text-align: center }
.seg3 { display: flex; background: var(--el-fill-color-blank); border: 1px solid var(--el-border-color); border-radius: 8px; padding: 2px; gap: 2px }
.seg3 span { padding: 3px 12px; border-radius: 6px; font-size: 13px; cursor: pointer; color: var(--el-text-color-secondary); white-space: nowrap }
.seg3 .on { background: var(--el-color-primary); color: #fff; font-weight: 700 }
.vp { flex: 1; min-height: 0; display: flex; gap: 10px; overflow: hidden; background: var(--el-fill-color-light); padding: 4px; position: relative }
.vp > figure { flex: 1; margin: 0; display: flex; flex-direction: column; gap: 6px; overflow: auto; min-width: 0 }
.vp figcaption { font-size: 12.5px; font-weight: 700; color: var(--el-text-color-secondary) }
.vp figure img { width: 100%; height: auto; border: 1px solid var(--el-border-color); background: #fff }
.pane { position: relative; overflow: hidden; cursor: grab; touch-action: none; flex: none }
.pane:active { cursor: grabbing }
.lbl { position: absolute; left: 6px; top: 4px; z-index: 3; font-size: 12px; font-weight: 700; padding: 1px 8px; border-radius: 10px; background: rgba(0, 0, 0, 0.55); color: #fff; pointer-events: none }
.stage { position: relative; transform-origin: 0 0; margin: 4px; box-shadow: 0 1px 8px rgba(0, 0, 0, 0.25); background: #fff }
.stage img { width: 100%; height: 100%; display: block; user-select: none; -webkit-user-drag: none }
.blk { position: absolute; box-sizing: border-box; overflow: hidden; line-height: 1.25; padding: 0 1px; cursor: text; white-space: pre-wrap; word-break: break-word; outline: 0 }
.blk:hover { outline: 1px dashed var(--el-color-primary) }
.blk:focus { outline: 2px solid var(--el-color-primary); overflow: visible; z-index: 2 }
.empty { margin: auto }
.tip { font-size: 12px; color: var(--el-text-color-secondary); padding: 4px 12px }
</style>
