<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { AiExpert, FileTransView, JobSnap, OcrStatus, PdfxStatus } from '../types'
import { registerDrop } from '../utils/dropHub'
import { TARGET_LANGS } from '../utils/langs'
import PdfCanvas from '../components/PdfCanvas.vue'

/** 文件翻译：拖入文件 → 一页一页翻 → 左原文右译文对照阅读 → 导出。设计图 ④。 */
const app = useAppStore()
const EXTS = ['png', 'jpg', 'jpeg', 'webp', 'pdf', 'epub', 'txt', 'md', 'markdown', 'srt', 'vtt', 'ass', 'ssa', 'docx']
const FORMATS = ['PDF', 'EPUB', 'DOCX', 'TXT', 'MD', 'SRT / ASS', '图片']

const jobs = ref<JobSnap[]>([])
// 任务编号 → 源文件路径：从任务快照里来（后端记了输入），页面切走再回来也认得出
const files = computed<Record<number, string>>(() => Object.fromEntries(jobs.value.filter((j) => j.kind === 'filetr' && j.input).map((j) => [j.id, j.input as string])))
const cur = ref<string>('') // 当前查看的源文件路径
const view = ref<FileTransView | null>(null)
const page = ref(0)
const show = ref<'src' | 'both' | 'trans' | 'layout'>('both')
const target = ref(app.settings?.ai.targetLang || '简体中文')
const bilingual = ref(true)
// PDF：译文写回原处 / 原页与译文页交替 / 只输出 TXT
const pdfMode = ref<'inplace' | 'interleave' | 'txt'>('inplace')
// 图片和扫描版 PDF 的识字方式：视觉模型 / 本机增强识字（PaddleOCR，开源）/ 在线识字接口
const ocrEngine = ref<'vision' | 'enhanced' | 'system' | 'api'>('vision')
const ocrSt = ref<OcrStatus | null>(null)
const ocrBusy = ref(false)
async function ocrRefresh() {
  ocrSt.value = await api.ocrStatus().catch(() => null)
}
async function ocrInstall() {
  ocrBusy.value = true
  try {
    ocrSt.value = await api.ocrInstall()
    ElMessage.success('增强识字已就绪')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    ocrBusy.value = false
  }
}
const API_NAMES: Record<string, string> = { ocrspace: 'OCR.space', baidu: '百度高精度', google: 'Google Cloud Vision' }
const apiProvider = computed(() => API_NAMES[app.settings?.live.ocrApi.provider ?? ''] ?? '')
const apiReady = computed(() => !!app.settings?.live.ocrApi.key)
// 先预览再生成：翻译完先在右边检查、改译文，满意后再生成文件
const previewFirst = ref(true)
// 任务编号 → 这个任务只翻译、没有生成文件（同样来自任务快照的参数）
const previews = computed<Record<number, boolean>>(() => Object.fromEntries(jobs.value.filter((j) => j.kind === 'filetr').map((j) => [j.id, !!j.params?.preview])))
const pdfx = ref<PdfxStatus | null>(null)
const pdfxBusy = ref('')
async function pdfxRefresh() {
  pdfx.value = await api.pdfxStatus().catch(() => null)
}
async function pdfxDo(what: 'install' | 'font' | 'remove') {
  pdfxBusy.value = what
  try {
    pdfx.value = await (what === 'install' ? api.pdfxInstall() : what === 'font' ? api.pdfxInstallFont() : api.pdfxRemove())
    ElMessage.success(what === 'remove' ? '已卸载 PDF 排版组件' : '已就绪')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    pdfxBusy.value = ''
  }
}
// 别的页面（媒体库里字幕的“翻译字幕”、工具箱）交过来的文件
async function takeRequest() {
  const paths = app.filetrRequest
  if (!paths?.length) return
  app.filetrRequest = null
  await addFiles(paths)
}
watch(() => app.filetrRequest, () => void takeRequest())
onMounted(() => {
  void takeRequest()
  void pdfxRefresh()
  void ocrRefresh()
  try {
    previewFirst.value = localStorage.getItem('cc-ft-preview') !== '0'
    const e = localStorage.getItem('cc-ft-ocr')
    if (e === 'enhanced' || e === 'system' || e === 'api' || e === 'vision') ocrEngine.value = e
  } catch {
    /* 读不到就用默认 */
  }
})
watch(previewFirst, (v) => {
  try {
    localStorage.setItem('cc-ft-preview', v ? '1' : '0')
  } catch {
    /* 记不住也不影响 */
  }
})
watch(ocrEngine, (v) => {
  try {
    localStorage.setItem('cc-ft-ocr', v)
  } catch {
    /* 记不住也不影响 */
  }
})
const experts = ref<AiExpert[]>([])
const over = ref(false)
let offs: UnlistenFn[] = []
let release: (() => void) | null = null

const mine = computed(() => jobs.value.filter((j) => j.kind === 'filetr').sort((a, b) => b.id - a.id))
const curJob = computed(() => mine.value.find((j) => files.value[j.id] === cur.value))
const pages = computed(() => view.value?.pages ?? [])
// 版面画布：PDF 才有，译文放回原来的位置，可切换原文 / 译文 / 对比 / 导出效果、缩放平移、直接改译文
const isPdf = computed(() => /\.pdf$/i.test(cur.value))
function onCanvasEdited(viewPage: number, index: number, text: string | null) {
  const p = view.value?.pages[viewPage]
  if (p && index < p.trans.length) p.trans[index] = text
}
const pg = computed(() => pages.value[page.value])
const doneSegs = computed(() => pages.value.reduce((n, p) => n + p.trans.filter(Boolean).length, 0))
const totalSegs = computed(() => pages.value.reduce((n, p) => n + p.src.length, 0))
const donePages = computed(() => pages.value.filter((p) => p.trans.length && p.trans.every(Boolean)).length)
const failedSegs = computed(() => totalSegs.value - doneSegs.value)
const hasTrans = computed(() => doneSegs.value > 0)
const baseName = (p: string) => p.split(/[\\/]/).pop() || p
const expertId = computed({
  get: () => (app.settings?.ai.expertAuto ? '__auto' : app.settings?.ai.expert || ''),
  set: (v: string) => {
    if (!app.settings) return
    void app.patch({ ai: { ...app.settings.ai, expertAuto: v === '__auto', expert: v === '__auto' ? app.settings.ai.expert : v } })
  },
})
const glossaryOn = computed({
  get: () => !!app.settings?.ai.glossaryOn,
  set: (v: boolean) => app.settings && void app.patch({ ai: { ...app.settings.ai, glossaryOn: v } }),
})

onMounted(async () => {
  jobs.value = await api.mediaJobs().catch(() => [])
  // 从别的页面切回来：接着看最近一个文件翻译任务
  const last = mine.value.find((j) => j.input)
  if (!cur.value && last?.input) {
    cur.value = last.input
    void loadView()
  }
  experts.value = (await api.expertsState().catch(() => null))?.experts ?? []
  offs = [
    await events.onMediaJobs((l) => {
      const before = curJob.value?.status
      jobs.value = l
      // 当前文件译完 / 译了一页，刷新阅读器
      if (curJob.value && (curJob.value.status !== before || curJob.value.status === 'running')) void loadView()
    }),
  ]
  release = registerDrop({
    accepts: (x, y) => {
      void x
      void y
      return true
    },
    over: () => (over.value = true),
    leave: () => (over.value = false),
    drop: (paths) => {
      over.value = false
      void addFiles(paths)
    },
  })
})
onBeforeUnmount(() => {
  offs.forEach((f) => f())
  release?.()
})
watch(page, () => {
  const el = document.querySelector('.ft .pg')
  if (el) el.scrollTop = 0
})
watch(target, () => void loadView())

async function loadView() {
  if (!cur.value) return
  try {
    const v = await api.filetrView(cur.value, target.value)
    const nv = v ?? (await api.filetrPreview(cur.value))
    view.value = nv
    if (page.value >= nv.pages.length) page.value = 0
  } catch (e) {
    view.value = null
    ElMessage.error(errorText(e))
  }
}

async function choose() {
  const f = await open({ multiple: true, title: '选择要翻译的文件', filters: [{ name: '文档、电子书、字幕', extensions: EXTS }] })
  if (Array.isArray(f)) await addFiles(f)
  else if (typeof f === 'string') await addFiles([f])
}

async function addFiles(paths: string[]) {
  const ok = paths.filter((p) => EXTS.includes((p.split('.').pop() || '').toLowerCase()))
  if (!ok.length) return ElMessage.warning('支持 PDF、EPUB、DOCX、TXT、Markdown、SRT / VTT / ASS 字幕，以及 PNG / JPG / WebP 图片。')
  for (const p of ok) await start(p)
}

async function start(path: string, preview = previewFirst.value) {
  try {
    await api.filetrStart(path, { bilingual: bilingual.value, target: target.value, pdfMode: pdfMode.value, ocr: ocrEngine.value, preview })
    cur.value = path
    page.value = 0
    jobs.value = await api.mediaJobs().catch(() => jobs.value)
    await loadView()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function pick(j: JobSnap) {
  const p = files.value[j.id]
  if (!p) return ElMessage.info('这个任务是上次打开程序时开始的，源文件位置没有保留。重新拖入文件即可继续（已译的段落有缓存）。')
  cur.value = p
  page.value = 0
  await loadView()
}

async function retry() {
  if (cur.value) await start(cur.value) // 已译好的段落走缓存，只重翻失败的
}
/** 按阅读器里现在的译文（含你改过的）生成文件：已译的段落走缓存，很快。 */
async function generate() {
  if (cur.value) await start(cur.value, false)
}
const isPreviewJob = computed(() => !!curJob.value && !!previews.value[curJob.value.id])

// ---------- 在阅读器里改译文 ----------
const editing = ref<{ page: number; i: number } | null>(null)
const draft = ref('')
const busySeg = ref('')
function beginEdit(i: number) {
  if (!pg.value || curJob.value?.status === 'running') return
  editing.value = { page: page.value, i }
  draft.value = pg.value.trans[i] ?? ''
}
async function saveEdit() {
  const e = editing.value
  if (!e || !cur.value) return
  editing.value = null
  const p = pages.value[e.page]
  if (!p || (p.trans[e.i] ?? '') === draft.value.trim()) return
  try {
    await api.filetrEdit(cur.value, target.value, e.page, e.i, draft.value)
    p.trans[e.i] = draft.value.trim() || null
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
function cancelEdit() {
  editing.value = null
}
async function clearSeg(i: number) {
  if (!cur.value) return
  try {
    await api.filetrEdit(cur.value, target.value, page.value, i, '')
    if (pg.value) pg.value.trans[i] = null
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
async function retranslateSeg(i: number) {
  if (!cur.value || !pg.value) return
  busySeg.value = `${page.value}:${i}`
  try {
    pg.value.trans[i] = await api.filetrRetranslate(cur.value, target.value, page.value, i)
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    busySeg.value = ''
  }
}
async function cancel(j: JobSnap) {
  await api.mediaJobCancel(j.id).catch(() => {})
}
async function openOut() {
  if (isPreviewJob.value) return
  const o = curJob.value?.output
  if (o) await api.revealFile(o).catch((e) => ElMessage.error(errorText(e)))
}

function exportText(): string {
  return pages.value
    .map((p) => p.src.map((s, i) => (bilingual.value ? `${s}\n${p.trans[i] ?? ''}` : (p.trans[i] ?? s))).join('\n\n'))
    .join('\n\n')
}
async function exportAs(kind: 'file' | 'txt' | 'md') {
  if (kind === 'file') return openOut()
  const text = kind === 'md' ? exportText().replace(/\n\n/g, '\n\n') : exportText()
  try {
    await api.copyText(text)
    ElMessage.success('已复制全文，粘贴到任意编辑器保存即可')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function status(j: JobSnap): string {
  return j.status === 'done' ? '已完成' : j.status === 'failed' ? '失败' : j.status === 'canceled' ? '已取消' : j.status === 'queued' ? '排队中' : `${Math.round(j.percent)}%`
}
</script>

<template>
  <div class="ft">
    <header class="h1">
      <h1>文件翻译</h1>
      <p>PDF（含扫描版）、EPUB、DOCX、TXT、Markdown、字幕、图片，译成双语对照，术语和专家沿用你的设置</p>
    </header>
    <div class="cols">
      <aside class="lc">
        <div class="drop" :class="{ over }" @click="choose">
          <b>把文件拖到这里，或点击选择</b>
          <div class="fmt"><span v-for="f in FORMATS" :key="f">{{ f }}</span></div>
          <small class="mute">图片里的文字请用“截图翻译”（Ctrl+Alt+T）</small>
        </div>

        <div class="card">
          <h3>任务</h3>
          <div v-if="!mine.length" class="mute">还没有任务。拖入文件就会开始翻译，原文件不会被修改。</div>
          <div v-for="j in mine" :key="j.id" class="job" :class="{ cur: files[j.id] === cur }" @click="pick(j)">
            <div class="n">
              <b :title="j.title">{{ j.title }}</b>
              <span class="s" :class="{ ok: j.status === 'done', bad: j.status === 'failed' }">{{ status(j) }}</span>
              <el-button v-if="j.status === 'running' || j.status === 'queued'" size="small" type="danger" plain class="cancel" @click.stop="cancel(j)">取消</el-button>
            </div>
            <div class="bar" :class="{ ok: j.status === 'done' }"><i :style="{ width: (j.status === 'done' ? 100 : j.percent) + '%' }" /></div>
            <div v-if="j.note || j.error" class="s">{{ j.error || j.note }}</div>
          </div>
        </div>

        <div class="card">
          <h3>翻译设置</h3>
          <div class="fld">
            <span>语言</span>
            <el-select v-model="target" size="small" filterable allow-create default-first-option class="grow">
              <el-option v-for="l in TARGET_LANGS" :key="l" :value="l" :label="`自动检测 → ${l}`" />
            </el-select>
          </div>
          <div class="fld"><span>翻译服务</span><div class="sel">AI 模型池 · 在“设置 → AI”里换</div></div>
          <div class="fld">
            <span>专家</span>
            <el-select v-model="expertId" size="small" class="grow" placeholder="不使用">
              <el-option value="" label="不使用" />
              <el-option value="__auto" label="自动选择" />
              <el-option v-for="e in experts" :key="e.id" :value="e.id" :label="e.name" />
            </el-select>
          </div>
          <div class="fld"><span>术语库</span><el-switch v-model="glossaryOn" size="small" /><small class="mute">按内容自动带入相关术语</small></div>
          <div class="fld"><span>预览</span><el-switch v-model="previewFirst" size="small" /><small class="mute">先译完在右边检查、改译文，再点“生成文件”</small></div>
          <div class="fld">
            <span>识字</span>
            <el-select v-model="ocrEngine" size="small" class="grow">
              <el-option value="vision" label="视觉模型（默认）" />
              <el-option value="enhanced" label="本机增强识字（开源 PaddleOCR）" />
              <el-option value="system" label="Windows 自带识字（不用下载，只在 Windows 上可用）" />
              <el-option value="api" label="在线识字接口（高精度）" />
            </el-select>
          </div>
          <div v-if="ocrEngine !== 'vision'" class="pdfx">
            <template v-if="ocrEngine === 'enhanced'">
              <div class="row1"><b>增强识字</b><span :class="ocrSt?.installed ? 'okc' : 'mute'">{{ ocrSt?.installed ? '已安装' : ocrSt?.supported === false ? '这台电脑暂不支持' : '还没安装' }}</span></div>
              <div class="row2"><el-button v-if="ocrSt?.supported !== false && !ocrSt?.installed" size="small" type="primary" :loading="ocrBusy" @click="ocrInstall">下载并安装（约 15 MB）</el-button></div>
              <small class="mute">PaddleOCR 开源模型，中英日韩等常见语言，全程在本机运行，不上传页面。比视觉模型更稳、更便宜，位置也更准；手写、艺术字、特别复杂的版面仍可能出错。</small>
            </template>
            <template v-else-if="ocrEngine === 'system'">
              <div class="row1"><b>Windows 自带识字</b></div>
              <small class="mute">用 Windows 10/11 自带的识字，不用下载、不上传页面；识别的语言取决于系统里装了哪些语言包（“屏幕翻译 → 识字”里可以选）。小字会先放大再识别。</small>
            </template>
            <template v-else>
              <div class="row1"><b>在线识字接口</b><span :class="apiReady ? 'okc' : 'mute'">{{ apiReady ? apiProvider : '还没填密钥' }}</span></div>
              <small class="mute">用“屏幕翻译 → 识字”里配好的接口（OCR.space、百度高精度、Google）。页面图片会发给这家服务；密钥没填的话先去那里填。</small>
              <div class="row2"><el-button v-if="!apiReady" size="small" @click="app.view = 'screen'">去填密钥</el-button></div>
            </template>
          </div>
          <div class="fld">
            <span>输出</span>
            <div class="seg">
              <span :class="{ on: bilingual }" @click="bilingual = true">双语对照</span>
              <span :class="{ on: !bilingual }" @click="bilingual = false">仅译文</span>
            </div>
          </div>
          <div class="fld">
            <span>PDF</span>
            <div class="seg">
              <span :class="{ on: pdfMode === 'inplace' }" title="译文写回原来的位置，版面、图片、页码不动" @click="pdfMode = 'inplace'">原处替换</span>
              <span :class="{ on: pdfMode === 'interleave' }" title="每页原文后面跟一页译文" @click="pdfMode = 'interleave'">原页+译文页</span>
              <span :class="{ on: pdfMode === 'txt' }" title="只取出文字，输出 TXT" @click="pdfMode = 'txt'">只要文字</span>
            </div>
          </div>
          <div v-if="pdfMode !== 'txt'" class="pdfx">
            <div class="row1"><b>PDF 排版组件</b><span :class="pdfx?.installed ? 'okc' : 'mute'">{{ pdfx?.note ?? '…' }}</span></div>
            <div class="row2">
              <el-button v-if="pdfx?.supported && !pdfx.installed" size="small" type="primary" :loading="pdfxBusy === 'install'" @click="pdfxDo('install')">安装 PDF 排版组件</el-button>
              <el-button v-if="pdfx?.installed && !pdfx.fontOk" size="small" :loading="pdfxBusy === 'font'" @click="pdfxDo('font')">下载译文字体（约 10 MB）</el-button>
              <el-button v-if="pdfx?.installed" size="small" text :loading="pdfxBusy === 'remove'" @click="pdfxDo('remove')">卸载</el-button>
            </div>
            <small class="mute">没装组件时 PDF 只输出 TXT。文字版 PDF 把译文放回原来的位置（放不下会缩小字号，最小 6 磅）；扫描版和图片先交给视觉模型识字并给出位置（需要支持看图的模型，一次最多 60 页）。公式、竖排、重叠的文字可能错位，输出后请抽查。</small>
          </div>
          <small v-else class="mute">只取出 PDF 里的文字，输出 TXT。</small>
        </div>
      </aside>

      <section class="rd">
        <div class="tb">
          <span class="t">{{ cur ? baseName(cur) : '阅读' }}</span>
          <template v-if="pages.length && show !== 'layout'">
            <span class="mute">第 {{ page + 1 }} / {{ pages.length }} 页</span>
            <el-button size="small" :disabled="page <= 0" @click="page--">上一页</el-button>
            <el-button size="small" :disabled="page >= pages.length - 1" @click="page++">下一页</el-button>
          </template>
          <span class="sp" />
          <div class="seg" :class="isPdf ? 'w260' : 'w210'">
            <span :class="{ on: show === 'src' }" @click="show = 'src'">原文</span>
            <span :class="{ on: show === 'both' }" @click="show = 'both'">对照</span>
            <span :class="{ on: show === 'trans' }" @click="show = 'trans'">译文</span>
            <span v-if="isPdf" :class="{ on: show === 'layout' }" title="把译文画到原来的页面上看位置" @click="show = 'layout'">版面</span>
          </div>
          <el-dropdown trigger="click" @command="exportAs">
            <el-button size="small">导出 ▾</el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="file" :disabled="curJob?.status !== 'done' || isPreviewJob">在文件夹里显示译好的文件</el-dropdown-item>
                <el-dropdown-item command="txt" :disabled="!hasTrans">复制全文（纯文本）</el-dropdown-item>
                <el-dropdown-item command="md" :disabled="!hasTrans">复制全文（Markdown）</el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>

        <div v-if="!cur" class="empty">
          <b>还没有打开文件</b>
          <span class="mute">拖入文件后，这里左边是原文、右边是译文，翻译的同时就能边译边看。</span>
        </div>
        <PdfCanvas v-else-if="show === 'layout' && isPdf" :path="cur" :target="target" :ocr="ocrEngine" :job-status="curJob?.status" @edited="onCanvasEdited" />
        <div v-else class="pg" :class="show">
          <div v-if="pg && show !== 'trans'" class="pane">
            <div class="lab">原文</div>
            <p v-for="(s, i) in pg.src" :key="i" :class="{ hl: !pg.trans[i] }">{{ s }}</p>
          </div>
          <div v-if="pg && show !== 'src'" class="pane">
            <div class="lab">译文</div>
            <div v-for="(_s, i) in pg.src" :key="i" class="seg1">
              <el-input
                v-if="editing && editing.page === page && editing.i === i"
                v-model="draft"
                type="textarea"
                :autosize="{ minRows: 2, maxRows: 14 }"
                autofocus
                @blur="saveEdit"
                @keydown.esc.prevent="cancelEdit"
                @keydown.ctrl.enter.prevent="saveEdit"
              />
              <template v-else>
                <p :class="{ miss: !pg.trans[i], ed: curJob?.status !== 'running' }" :title="curJob?.status === 'running' ? '' : '点一下可以直接改'" @click="beginEdit(i)">{{ pg.trans[i] || (curJob && curJob.status !== 'done' && curJob.status !== 'failed' ? '翻译中…' : '（没有译文，已保留原文；点一下可以自己写）') }}</p>
                <span v-if="curJob?.status !== 'running'" class="acts">
                  <el-button size="small" text :loading="busySeg === `${page}:${i}`" @click.stop="retranslateSeg(i)">重译</el-button>
                  <el-button v-if="pg.trans[i]" size="small" text @click.stop="clearSeg(i)">去掉</el-button>
                </span>
              </template>
            </div>
          </div>
          <div v-if="!pg" class="empty"><span class="mute">读取中…</span></div>
        </div>

        <div class="st">
          <span><b>已译 {{ donePages }} / {{ pages.length }} 页</b></span>
          <span>{{ doneSegs }} / {{ totalSegs }} 段</span>
          <span v-if="curJob?.note" class="mute">{{ curJob.note }}</span>
          <span class="sp" />
          <el-button size="small" :disabled="!cur || !failedSegs || curJob?.status === 'running'" @click="retry">重试失败段落{{ failedSegs && curJob?.status !== 'running' ? `（${failedSegs}）` : '' }}</el-button>
          <el-button v-if="isPreviewJob" size="small" type="primary" :disabled="curJob?.status !== 'done'" @click="generate">生成文件</el-button>
          <el-button v-else size="small" type="primary" :disabled="curJob?.status !== 'done'" @click="openOut">导出双语文件</el-button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.ft { display: flex; flex-direction: column; gap: 16px; padding: 24px 28px; height: 100%; box-sizing: border-box; min-height: 0 }
.h1 { display: flex; align-items: flex-end; gap: 14px; flex-wrap: wrap }
.h1 h1 { margin: 0; font-size: 26px }
.h1 p { margin: 0; color: var(--el-text-color-secondary) }
.cols { flex: 1; display: flex; gap: 18px; min-height: 0 }
.lc { width: 330px; flex: none; display: flex; flex-direction: column; gap: 12px; overflow-y: auto }
.mute { color: var(--el-text-color-secondary); font-size: 12.5px }
.drop { border: 2px dashed var(--el-border-color); border-radius: 12px; padding: 18px 14px; text-align: center; display: flex; flex-direction: column; gap: 8px; align-items: center; background: var(--el-bg-color); cursor: pointer }
.drop.over, .drop:hover { border-color: var(--el-color-primary) }
.drop b { font-size: 15px }
.fmt { display: flex; gap: 6px; flex-wrap: wrap; justify-content: center }
.fmt span { height: 22px; padding: 0 8px; border-radius: 11px; background: var(--el-color-primary-light-9); color: var(--el-color-primary); font-size: 12px; font-weight: 700; display: flex; align-items: center }
.card { border: 1px solid var(--el-border-color); background: var(--el-bg-color); border-radius: 12px; padding: 12px 14px; display: flex; flex-direction: column; gap: 10px }
.card h3 { margin: 0; font-size: 13px; color: var(--el-text-color-secondary); font-weight: 600 }
.job { display: flex; flex-direction: column; gap: 5px; padding: 8px 10px; border-radius: 9px; border: 1px solid var(--el-border-color); cursor: pointer }
.job.cur { border-color: var(--el-color-primary); background: var(--el-color-primary-light-9) }
.job .n { display: flex; gap: 8px; align-items: center }
.job .n b { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px }
.job .s { color: var(--el-text-color-secondary); font-size: 12px }
.s.ok { color: var(--el-color-success) }
.s.bad { color: var(--el-color-danger) }
.bar { height: 5px; border-radius: 3px; background: var(--el-border-color); overflow: hidden }
.bar i { display: block; height: 5px; background: var(--el-color-primary); transition: width .3s }
.bar.ok i { background: var(--el-color-success) }
.fld { display: flex; align-items: center; gap: 8px }
.fld > span { width: 58px; color: var(--el-text-color-secondary); font-size: 12.5px; flex: none }
.grow { flex: 1 }
.sel { flex: 1; min-height: 30px; border: 1px solid var(--el-border-color); border-radius: 8px; display: flex; align-items: center; padding: 0 10px; font-size: 12.5px; color: var(--el-text-color-secondary) }
.sw { display: flex; align-items: center; gap: 8px; font-size: 13px }
.sw i { width: 30px; height: 17px; border-radius: 9px; background: var(--el-border-color); position: relative; flex: none }
.sw i::after { content: ''; position: absolute; top: 2px; left: 2px; width: 13px; height: 13px; border-radius: 50%; background: var(--el-bg-color) }
.sw.on i { background: var(--el-color-primary) }
.sw.on i::after { left: 15px }
.seg { display: flex; background: var(--el-fill-color-blank); border: 1px solid var(--el-border-color); border-radius: 8px; padding: 2px; gap: 2px; flex: 1 }
.seg span { flex: 1; text-align: center; padding: 3px 0; border-radius: 6px; color: var(--el-text-color-secondary); font-size: 13px; cursor: pointer }
.seg .on { background: var(--el-color-primary); color: #fff; font-weight: 700 }
.w210 { flex: none; width: 210px }
.rd { flex: 1; min-width: 0; border: 1px solid var(--el-border-color); background: var(--el-bg-color); border-radius: 12px; display: flex; flex-direction: column; overflow: hidden }
.tb { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-bottom: 1px solid var(--el-border-color); flex-wrap: wrap }
.tb .t { font-weight: 700; max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.sp { flex: 1 }
.job .cancel { font-weight: 700; margin-left: auto }
.seg.w260 { flex: none; width: 260px }
.pg { flex: 1; display: grid; grid-template-columns: 1fr 1fr; min-height: 0; overflow-y: auto }
.pg.src, .pg.trans { grid-template-columns: 1fr }
.pane { padding: 18px 22px; display: flex; flex-direction: column; gap: 12px }
.pane + .pane { border-left: 1px solid var(--el-border-color); background: var(--el-fill-color-lighter) }
.pg.src .pane + .pane, .pg.trans .pane + .pane { border-left: 0 }
.lab { font-size: 11px; letter-spacing: .06em; color: var(--el-text-color-secondary); font-weight: 700 }
.pane p { margin: 0; line-height: 1.75; font-size: 13.5px; white-space: pre-wrap; word-break: break-word }
.pane p.hl { background: var(--el-color-primary-light-9); border-radius: 6px; padding: 6px 8px; margin: -6px -8px }
.pane p.miss { color: var(--el-text-color-secondary) }
.seg1 { position: relative; display: flex; flex-direction: column; gap: 2px }
.pane p.ed { cursor: text; border-radius: 6px; padding: 2px 6px; margin: -2px -6px }
.pane p.ed:hover { background: var(--el-fill-color) }
.seg1 .acts { display: none; position: absolute; right: -6px; top: -14px; background: var(--el-bg-color); border: 1px solid var(--el-border-color); border-radius: 6px; padding: 0 2px }
.seg1:hover .acts { display: flex }
.empty { flex: 1; display: flex; flex-direction: column; gap: 6px; align-items: center; justify-content: center; padding: 24px; text-align: center }
.st { display: flex; gap: 16px; align-items: center; padding: 9px 14px; border-top: 1px solid var(--el-border-color); color: var(--el-text-color-secondary); font-size: 12.5px; flex-wrap: wrap }
.st b { color: var(--el-text-color-primary) }
.pdfx { border: 1px solid var(--el-border-color); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 6px; background: var(--el-fill-color-lighter) }
.pdfx .row1 { display: flex; justify-content: space-between; gap: 8px; align-items: baseline }
.pdfx .row2 { display: flex; gap: 6px; flex-wrap: wrap }
.pdfx .okc { color: var(--el-color-success); font-size: 12.5px }
</style>
