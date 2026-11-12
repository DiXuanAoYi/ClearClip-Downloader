<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useAppStore } from '../../stores/app'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import type { FileTransView, FtOpts } from '../../types'
import PdfCanvas from '../PdfCanvas.vue'
import { baseName, countView, hitPages, pageState, pageStates, primaryAction, qaTotal, railPages, segIndexes, splitHits, stripMarks, type PageState, type Task } from '../../utils/ft'

/** 文件翻译的阅读器：左边页栏（状态点）、原文 / 对照 / 译文 / 版面、搜索、只看未译 / 失败、直接改译文、底栏的主操作。 */
const props = defineProps<{ path: string; target: string; view: FileTransView | null; task: Task | null; opts: Partial<FtOpts> }>()
const emit = defineEmits<{
  (e: 'act', what: 'resume' | 'retry' | 'generate' | 'open' | 'stop'): void
  (e: 'export', kind: 'file' | 'reveal' | 'txt' | 'md' | 'srt' | 'srt-trans' | 'copy'): void
}>()

const app = useAppStore()
const show = ref<'src' | 'both' | 'trans' | 'layout'>('both')
// 在版面里调整时，主导航和左栏自动收起
watch(
  () => show.value === 'layout' && !!props.path,
  (v) => (app.layoutWork = v),
  { immediate: true },
)
onBeforeUnmount(() => (app.layoutWork = false))
const page = ref(0) // 阅读器里的页序号
const pdfPage = ref(0) // 版面里的 PDF 页号（从 0 起）
const only = ref<'all' | 'todo' | 'fail'>('all')
const q = ref('')

const isPdf = computed(() => /\.pdf$/i.test(props.path))
const isSub = computed(() => /\.(srt|vtt|ass|ssa)$/i.test(props.path))
const running = computed(() => props.task?.state === 'running')
const pages = computed(() => props.view?.pages ?? [])
const pg = computed(() => pages.value[page.value])
const counts = computed(() => countView(props.view))
const states = computed(() => pageStates(props.view, running.value))
const hits = computed(() => new Set(hitPages(props.view, q.value)))
const qaCount = computed(() => qaTotal(props.view))
const primary = computed(() => (props.task ? primaryAction(props.task) : 'none'))

// 页栏：版面里按 PDF 页（含没有文字的页），其他按阅读器里的页
interface RailItem {
  key: number
  label: number
  state: PageState | 'none'
  hit: boolean
  cur: boolean
}
const rail = computed<RailItem[]>(() => {
  const v = props.view
  if (show.value === 'layout' && isPdf.value && v?.layout) {
    const total = v.totalPages || pages.value.length
    return Array.from({ length: total }, (_, i) => {
      const vi = pages.value.findIndex((p) => p.page === i)
      return { key: i, label: i + 1, state: vi >= 0 ? states.value[vi] : 'none', hit: vi >= 0 && hits.value.has(vi), cur: i === pdfPage.value }
    })
  }
  return railPages(v, only.value).map((i) => ({ key: i, label: (pages.value[i].page ?? i) + 1, state: states.value[i], hit: hits.value.has(i), cur: i === page.value }))
})
const STATE_TITLE: Record<string, string> = { done: '已译', fail: '有没译出来的段', run: '正在译', todo: '还没译', none: '没有文字的页' }
const DOT: Record<string, string> = { done: '#2f8a57', fail: '#c2453b', run: '#d9762b', todo: '#b7beb9', none: 'transparent' }

function goRail(it: RailItem) {
  if (show.value === 'layout' && isPdf.value && props.view?.layout) pdfPage.value = it.key
  else {
    page.value = it.key
    const p = pages.value[it.key]?.page
    if (p != null) pdfPage.value = p
  }
}
watch(show, (m, old) => {
  if (m === 'layout') pdfPage.value = pg.value?.page ?? pdfPage.value
  else if (old === 'layout') {
    const vi = pages.value.findIndex((p) => p.page === pdfPage.value)
    if (vi >= 0) page.value = vi
  }
})
watch(
  () => props.path,
  () => {
    page.value = 0
    pdfPage.value = 0
    only.value = 'all'
    q.value = ''
    editing.value = null
  },
)
watch(pages, (p) => {
  if (page.value >= p.length) page.value = 0
})
watch(page, async () => {
  await nextTick()
  document.querySelector('.ftr .body')?.scrollTo({ top: 0 })
})

// 版面不适用（没装排版组件 / 不是文字版）时回到对照
watch(isPdf, (v) => {
  if (!v && show.value === 'layout') show.value = 'both'
})

// ---------- 搜索 ----------
function nextHit() {
  const list = [...hits.value].sort((a, b) => a - b)
  if (!list.length) return
  const at = list.find((i) => i > page.value) ?? list[0]
  page.value = at
  const p = pages.value[at]?.page
  if (p != null) pdfPage.value = p
}

// ---------- 改译文 ----------
const editing = ref<{ page: number; i: number } | null>(null)
const draft = ref('')
const busySeg = ref('')
const canEdit = computed(() => !running.value && !!pg.value)
function beginEdit(i: number) {
  if (!canEdit.value || !pg.value) return
  editing.value = { page: page.value, i }
  draft.value = stripMarks(pg.value.trans[i])
}
async function saveEdit() {
  const e = editing.value
  if (!e) return
  editing.value = null
  const p = pages.value[e.page]
  if (!p || stripMarks(p.trans[e.i]) === draft.value.trim()) return
  try {
    await api.filetrEdit(props.path, props.target, e.page, e.i, draft.value)
    p.trans[e.i] = draft.value.trim() || null
    if (p.qa) delete p.qa[String(e.i)]
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
async function clearSeg(i: number) {
  try {
    await api.filetrEdit(props.path, props.target, page.value, i, '')
    if (pg.value) pg.value.trans[i] = null
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
async function retranslateSeg(i: number) {
  if (!pg.value) return
  busySeg.value = `${page.value}:${i}`
  try {
    pg.value.trans[i] = await api.filetrRetranslate(props.path, props.target, page.value, i)
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    busySeg.value = ''
  }
}
function onCanvasEdited(viewPage: number, index: number, text: string | null) {
  const p = pages.value[viewPage]
  if (p && index < p.trans.length) p.trans[index] = text
}

const segs = computed(() => (pg.value ? segIndexes(pg.value, only.value) : []))
const skipSet = computed(() => new Set(pg.value?.skip ?? []))
const curState = computed(() => (pg.value ? pageState(pg.value, running.value, states.value[page.value] === 'run') : 'todo'))
const emptyHint = computed(() => {
  const t = props.task
  if (t && (t.state === 'running' || t.state === 'queued')) return '翻译中…'
  return '（没有译文，已保留原文；点一下可以自己写）'
})
function missText(i: number): string {
  if (!pg.value) return ''
  if (skipSet.value.has(i)) return '（按设置不翻译，保留原文）'
  return pg.value.done || !running.value ? emptyHint.value : '（还没译到这一页）'
}

const PRIMARY_LABEL: Record<string, string> = { resume: '继续翻译', retry: '重试失败的', generate: '生成文件', open: '打开成品', stop: '停止' }
const primaryText = computed(() => PRIMARY_LABEL[primary.value] ?? '')
const retryText = computed(() => `重试失败（${counts.value.failedSegs}）`)
</script>

<template>
  <section class="ftr rd">
    <div class="tb">
      <span class="t" :title="path">{{ path ? baseName(path) : '阅读' }}</span>
      <span v-if="pg && show !== 'layout'" class="mute">第 {{ (pg.page ?? page) + 1 }} / {{ view?.totalPages || pages.length }} 页</span>
      <span class="sp" />
      <el-input v-if="path && show !== 'layout'" v-model="q" size="small" class="search" clearable placeholder="搜索原文 / 译文" @keydown.enter="nextHit" />
      <span v-if="q.trim() && show !== 'layout'" class="mute">命中 {{ hits.size }} 页</span>
      <div class="seg" :class="isPdf ? 'w260' : 'w210'">
        <span :class="{ on: show === 'src' }" @click="show = 'src'">原文</span>
        <span :class="{ on: show === 'both' }" @click="show = 'both'">对照</span>
        <span :class="{ on: show === 'trans' }" @click="show = 'trans'">译文</span>
        <span v-if="isPdf" :class="{ on: show === 'layout' }" title="把译文画到原来的页面上看位置" @click="show = 'layout'">版面</span>
      </div>
      <el-dropdown trigger="click" @command="(c: string) => emit('export', c as never)">
        <el-button size="small">导出 ▾</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="file" :disabled="!path">{{ task?.state === 'generated' ? '打开生成的文件' : '生成双语 / 译文文件' }}</el-dropdown-item>
            <el-dropdown-item command="reveal" :disabled="task?.state !== 'generated'">在文件夹里显示生成的文件</el-dropdown-item>
            <el-dropdown-item command="txt" divided :disabled="!counts.doneSegs">译文 TXT…</el-dropdown-item>
            <el-dropdown-item command="md" :disabled="!counts.doneSegs">双语 Markdown…</el-dropdown-item>
            <template v-if="isSub">
              <el-dropdown-item command="srt" :disabled="!counts.doneSegs">双语 SRT 字幕…</el-dropdown-item>
              <el-dropdown-item command="srt-trans" :disabled="!counts.doneSegs">仅译文 SRT 字幕…</el-dropdown-item>
            </template>
            <el-dropdown-item command="copy" divided :disabled="!counts.doneSegs">复制全文</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>

    <div v-if="!path" class="empty">
      <b>还没有打开文件</b>
      <span class="mute">拖入文件后，这里左边是原文、右边是译文，翻译的同时就能边译边看。</span>
    </div>
    <div v-else class="mid">
      <div v-if="rail.length" class="rail" aria-label="页">
        <div v-if="show !== 'layout'" class="only">
          <span :class="{ on: only === 'all' }" @click="only = 'all'">全部</span>
          <span :class="{ on: only === 'todo' }" title="还没译的页" @click="only = 'todo'">未译</span>
          <span :class="{ on: only === 'fail' }" title="有没译出来的段" @click="only = 'fail'">失败</span>
        </div>
        <button
          v-for="it in rail"
          :key="it.key"
          class="thumb"
          :class="{ on: it.cur, hit: it.hit, none: it.state === 'none' }"
          :title="`第 ${it.label} 页 · ${STATE_TITLE[it.state]}`"
          :disabled="it.state === 'none' && show !== 'layout'"
          @click="goRail(it)"
        >
          {{ it.label }}<i :style="{ background: DOT[it.state] }" />
        </button>
        <div class="legend">绿 已译<br />橙 译一半<br />灰 未译<br />红 失败</div>
      </div>

      <PdfCanvas
        v-if="show === 'layout' && isPdf"
        v-model:page="pdfPage"
        :path="path"
        :target="target"
        :ocr="opts.ocr || 'vision'"
        :job-status="task?.state === 'running' ? 'running' : task?.state === 'generated' || task?.state === 'ready' ? 'done' : ''"
        :running="running"
        @edited="onCanvasEdited"
        @continue="emit('act', 'resume')"
      />
      <div v-else class="body pg" :class="show">
        <div v-if="pg && curState === 'fail'" class="note warn">这一页有 {{ pg.trans.filter((t, i) => !t && !skipSet.has(i)).length }} 段没译出来，可以点“重译”，或到底栏点“重试失败”。</div>
        <div class="cols">
        <div v-if="pg && show !== 'trans'" class="pane">
          <div class="lab">原文</div>
          <p v-for="i in segs" :key="i" :class="{ hl: !pg.trans[i] && !skipSet.has(i) }">
            <template v-for="(s, k) in splitHits(stripMarks(pg.src[i]), q)" :key="k"><mark v-if="s.m">{{ s.t }}</mark><template v-else>{{ s.t }}</template></template>
          </p>
        </div>
        <div v-if="pg && show !== 'src'" class="pane">
          <div class="lab">译文</div>
          <div v-for="i in segs" :key="i" class="seg1">
            <el-input
              v-if="editing && editing.page === page && editing.i === i"
              v-model="draft"
              type="textarea"
              :autosize="{ minRows: 2, maxRows: 14 }"
              autofocus
              @blur="saveEdit"
              @keydown.esc.prevent="editing = null"
              @keydown.ctrl.enter.prevent="saveEdit"
            />
            <template v-else>
              <p :class="{ miss: !pg.trans[i], ed: canEdit }" :title="canEdit ? '点一下可以直接改' : ''" @click="beginEdit(i)">
                <template v-if="pg.trans[i]">
                  <template v-for="(s, k) in splitHits(stripMarks(pg.trans[i]), q)" :key="k"><mark v-if="s.m">{{ s.t }}</mark><template v-else>{{ s.t }}</template></template>
                </template>
                <template v-else>{{ missText(i) }}</template>
              </p>
              <span v-if="pg.qa?.[String(i)]" class="qa" :title="pg.qa[String(i)].join('\n')">⚠ {{ pg.qa[String(i)][0] }}<template v-if="pg.qa[String(i)].length > 1">（等 {{ pg.qa[String(i)].length }} 项）</template></span>
              <span v-if="canEdit && !skipSet.has(i)" class="acts">
                <el-button size="small" text :loading="busySeg === `${page}:${i}`" @click.stop="retranslateSeg(i)">重译</el-button>
                <el-button v-if="pg.trans[i]" size="small" text @click.stop="clearSeg(i)">去掉</el-button>
              </span>
            </template>
          </div>
          <div v-if="!segs.length" class="mute">这一页没有符合筛选的内容。</div>
        </div>
        <div v-if="!pg" class="empty"><span class="mute">读取中…</span></div>
        </div>
      </div>
    </div>

    <div class="st">
      <span><b>已译 {{ counts.donePages + counts.failPages }} / {{ counts.pages }} 页</b></span>
      <span>{{ counts.doneSegs }} / {{ counts.segs }} 段</span>
      <span v-if="qaCount" class="qa" title="译后检查：数字、术语、格式标记、漏译等，只提示，不影响生成">⚠ {{ qaCount }} 段需要检查</span>
      <span v-if="task?.note && running" class="mute">{{ task.note }}</span>
      <span class="sp" />
      <el-button v-if="counts.failedSegs && !running" size="small" @click="emit('act', 'retry')">{{ retryText }}</el-button>
      <el-button v-if="primary === 'stop'" size="small" @click="emit('act', 'stop')">停止</el-button>
      <el-button v-else-if="primaryText && primary !== 'retry' && primary !== 'cancel'" size="small" type="primary" @click="emit('act', primary as never)">{{ primaryText }}</el-button>
    </div>
  </section>
</template>

<style scoped>
.rd { flex: 1; min-width: 0; border: 1px solid var(--cc-line); background: var(--cc-card); border-radius: 12px; display: flex; flex-direction: column; overflow: hidden }
.tb { display: flex; align-items: center; gap: 8px; padding: 9px 14px; border-bottom: 1px solid var(--cc-line-soft); flex-wrap: wrap }
.tb .t { font-weight: 700; font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 320px }
.sp { flex: 1 }
.mute { color: var(--cc-mute); font-size: 12.5px }
.search { width: 160px }
.seg { display: flex; background: var(--cc-inset); border: 1px solid var(--cc-line); border-radius: 8px; padding: 2px; gap: 2px }
.seg span { padding: 3px 12px; border-radius: 6px; font-size: 13px; cursor: pointer; color: var(--cc-mute); white-space: nowrap }
.seg .on { background: var(--cc-amber); color: #fff; font-weight: 700 }
.mid { flex: 1; min-height: 0; display: flex }
.rail { width: 76px; flex: none; border-right: 1px solid var(--cc-line-soft); padding: 8px 0; display: flex; flex-direction: column; align-items: center; gap: 10px; background: var(--cc-hover); overflow-y: auto }
.rail .only { display: flex; flex-direction: column; width: 56px; border: 1px solid var(--cc-line); border-radius: 8px; overflow: hidden; background: var(--cc-card) }
.rail .only span { text-align: center; font-size: 11.5px; padding: 2px 0; cursor: pointer; color: var(--cc-mute) }
.rail .only .on { background: var(--cc-acc-soft); color: var(--cc-amber); font-weight: 700 }
.thumb { flex: none; width: 44px; height: 56px; border-radius: 5px; border: 1px solid var(--cc-line); background: var(--cc-card); position: relative; display: grid; place-items: center; font-size: 11.5px; color: var(--cc-mute); cursor: pointer; padding: 0 }
.thumb.on { border-color: var(--cc-amber); box-shadow: 0 0 0 2px var(--cc-acc-line); color: var(--cc-fg) }
.thumb.hit { outline: 2px solid #e6a23c; outline-offset: 1px }
.thumb.none { opacity: 0.45 }
.thumb:disabled { cursor: default }
.thumb i { position: absolute; right: -3px; top: -3px; width: 9px; height: 9px; border-radius: 50%; box-shadow: 0 0 0 2px var(--cc-hover) }
.legend { font-size: 10.5px; line-height: 1.5; color: var(--cc-mute); text-align: center; margin-top: 4px }
.body { flex: 1; min-width: 0; overflow: auto; padding: 14px 18px; display: flex; flex-direction: column; gap: 10px }
.cols { display: flex; gap: 22px; align-items: flex-start; width: 100% }
.body.both .pane { flex: 1; min-width: 0 }
.body.src .pane, .body.trans .pane { max-width: 720px; margin: 0 auto; flex: 1 }
.pane .lab { font-size: 12px; font-weight: 700; color: var(--cc-mute); margin-bottom: 8px }
.pane p { margin: 0 0 10px; line-height: 1.75; font-size: 13.5px; white-space: pre-wrap }
.pane p.hl { border-left: 3px solid var(--cc-amber); padding-left: 8px }
.pane p.miss { color: var(--cc-faint); border: 1px dashed var(--cc-line); border-radius: 8px; padding: 4px 10px }
.pane p.ed { cursor: text }
.pane p.ed:hover { background: var(--cc-hover); border-radius: 6px }
.seg1 { position: relative }
.qa { display: block; font-size: 12px; color: var(--cc-amber); margin: -4px 0 8px }
.st .qa { display: inline; margin: 0 }
.seg1 .acts { display: none; position: absolute; right: 0; top: -2px }
.seg1:hover .acts { display: block }
mark { background: #ffe58a; color: inherit; border-radius: 2px }
.note { width: 100%; font-size: 12.5px; padding: 6px 10px; border-radius: 8px }
.note.warn { background: var(--cc-acc-soft); color: var(--cc-amber); margin-bottom: 4px }
.empty { margin: auto; text-align: center; display: flex; flex-direction: column; gap: 6px; padding: 30px }
.st { display: flex; align-items: center; gap: 12px; padding: 9px 14px; border-top: 1px solid var(--cc-line-soft); background: var(--cc-fill); font-size: 12.5px }
</style>
