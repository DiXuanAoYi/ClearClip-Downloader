<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { AiExpert, FileTransView, FtOpts, FtRecent, JobSnap, OcrStatus, PdfxStatus } from '../types'
import { registerDrop } from '../utils/dropHub'
import { TARGET_LANGS } from '../utils/langs'
import FtReader from '../components/filetrans/FtReader.vue'
import FtTasks from '../components/filetrans/FtTasks.vue'
import FtStartDialog from '../components/filetrans/FtStartDialog.vue'
import { baseName, countView, etaSeconds, etaText, filterTasks, mergeTasks, taskKey, type Task, type TaskFilter } from '../utils/ft'

/** 文件翻译：拖入文件 →（开始前确认）→ 一页一页翻 → 左原文右译文 / 版面阅读 → 导出。任务能停、能续、重启后还在。 */
const app = useAppStore()
const EXTS = ['png', 'jpg', 'jpeg', 'webp', 'pdf', 'epub', 'txt', 'md', 'markdown', 'srt', 'vtt', 'ass', 'ssa', 'docx', 'pptx', 'xlsx']
const FORMATS = ['PDF', 'Word', 'PPT', 'Excel', 'EPUB', 'TXT', 'MD', 'SRT / ASS', '图片']
const LS = 'cc-ft-opts'

const jobs = ref<JobSnap[]>([])
const recents = ref<FtRecent[]>([])
const cur = ref('') // 当前查看的源文件
const view = ref<FileTransView | null>(null)
const filter = ref<TaskFilter>('all')
const over = ref(false)
const experts = ref<AiExpert[]>([])
const advanced = ref(false)
const confirmStart = ref(true)

// 设置：常用的放前面，高级收起来；记在本机
const o = ref<FtOpts>({ bilingual: true, target: app.settings?.ai.targetLang || '简体中文', pdfMode: 'inplace', ocr: 'vision', preview: true, pages: '', skipHeaders: true, skipCode: true, skipRefs: false, scanAuto: true })
function restoreOpts() {
  try {
    const s = localStorage.getItem(LS)
    if (s) Object.assign(o.value, JSON.parse(s), { pages: '' })
    confirmStart.value = localStorage.getItem('cc-ft-confirm') !== '0'
  } catch {
    /* 读不到就用默认 */
  }
}
watch(
  o,
  (v) => {
    try {
      localStorage.setItem(LS, JSON.stringify({ ...v, pages: '' }))
    } catch {
      /* 记不住也不影响 */
    }
  },
  { deep: true },
)
watch(confirmStart, (v) => {
  try {
    localStorage.setItem('cc-ft-confirm', v ? '1' : '0')
  } catch {
    /* 记不住也不影响 */
  }
})

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

// ---------- 任务 ----------
const counts = computed(() => (cur.value ? countView(view.value) : null))
const tasks = computed<Task[]>(() => mergeTasks(jobs.value, recents.value, o.value.target, cur.value && counts.value ? { key: taskKey(cur.value, o.value.target), counts: counts.value } : null))
const curTask = computed(() => tasks.value.find((t) => t.path === cur.value && t.target === o.value.target) ?? tasks.value.find((t) => t.path === cur.value) ?? null)

// 剩余时间：记下每个任务进度开始动的时刻
const marks = new Map<string, { t: number; p: number }>()
const eta = ref<Record<string, string>>({})
function updateEta() {
  const next: Record<string, string> = {}
  for (const t of tasks.value) {
    if (t.state !== 'running') {
      marks.delete(t.key)
      continue
    }
    const m = marks.get(t.key)
    if (!m) marks.set(t.key, { t: Date.now(), p: t.percent })
    else next[t.key] = etaText(etaSeconds(m.t, m.p, Date.now(), t.percent))
  }
  eta.value = next
}

async function refreshRecents() {
  recents.value = await api.filetrRecent().catch(() => recents.value)
}
async function refreshJobs() {
  jobs.value = await api.mediaJobs().catch(() => jobs.value)
}
async function loadView() {
  if (!cur.value) return
  try {
    const v = await api.filetrView(cur.value, o.value.target)
    view.value = v ?? (await api.filetrPreview(cur.value))
  } catch (e) {
    view.value = null
    ElMessage.error(errorText(e))
  }
}

let offs: UnlistenFn[] = []
let release: (() => void) | null = null
onMounted(async () => {
  restoreOpts()
  void pdfxRefresh()
  void ocrRefresh()
  void takeRequest()
  await Promise.all([refreshJobs(), refreshRecents()])
  const last = tasks.value[0]
  if (!cur.value && last) {
    cur.value = last.path
    void loadView()
  }
  experts.value = (await api.expertsState().catch(() => null))?.experts ?? []
  offs = [
    await events.onMediaJobs((l) => {
      const before = new Map(jobs.value.map((j) => [j.id, j.status]))
      jobs.value = l
      updateEta()
      const mine = l.find((j) => j.kind === 'filetr' && j.input === cur.value)
      const ended = l.some((j) => j.kind === 'filetr' && before.get(j.id) !== j.status && j.status !== 'running' && j.status !== 'queued')
      if (ended) void refreshRecents()
      if (mine && (mine.status === 'running' || before.get(mine.id) !== mine.status)) void loadView()
    }),
  ]
  release = registerDrop({
    accepts: () => true,
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
watch(() => o.value.target, () => void loadView())
async function takeRequest() {
  const paths = app.filetrRequest
  if (!paths?.length) return
  app.filetrRequest = null
  await addFiles(paths)
}
watch(() => app.filetrRequest, () => void takeRequest())

// ---------- 开始 ----------
const dlg = ref<{ paths: string[]; opts: FtOpts; title: string; ok: string } | null>(null)
const dlgOpen = computed({ get: () => !!dlg.value, set: (v: boolean) => !v && (dlg.value = null) })

async function choose() {
  const f = await open({ multiple: true, title: '选择要翻译的文件', filters: [{ name: '文档、电子书、字幕', extensions: EXTS }] })
  if (Array.isArray(f)) await addFiles(f)
  else if (typeof f === 'string') await addFiles([f])
}
async function addFiles(paths: string[]) {
  const ok = paths.filter((p) => EXTS.includes((p.split('.').pop() || '').toLowerCase()))
  if (!ok.length) return ElMessage.warning('支持 PDF、Word、PPT、Excel、EPUB、TXT、Markdown、SRT / VTT / ASS 字幕，以及 PNG / JPG / WebP 图片。')
  if (confirmStart.value) dlg.value = { paths: ok, opts: { ...o.value, pages: '' }, title: '开始前确认', ok: '开始翻译' }
  else for (const p of ok) await start(p, { ...o.value, pages: '' })
}
async function onConfirm(opts: FtOpts, remember: boolean) {
  const d = dlg.value
  dlg.value = null
  if (!d) return
  confirmStart.value = remember
  o.value = { ...o.value, ...opts, pages: '' }
  for (const p of d.paths) await start(p, opts)
}
async function start(path: string, opts: Partial<FtOpts>) {
  try {
    await api.filetrStart(path, opts)
    cur.value = path
    if (opts.target && opts.target !== o.value.target) o.value.target = opts.target
    await refreshJobs()
    await refreshRecents()
    updateEta()
    await loadView()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

// ---------- 任务上的操作 ----------
async function pick(t: Task) {
  cur.value = t.path
  if (t.target !== o.value.target) o.value.target = t.target
  else await loadView()
}
const taskOpts = (t: Task, patch: Partial<FtOpts> = {}): Partial<FtOpts> => ({ ...o.value, ...t.opts, target: t.target, ...patch })
async function ask(msg: string, ok: string): Promise<boolean> {
  try {
    await ElMessageBox.confirm(msg, '请确认', { confirmButtonText: ok, cancelButtonText: '取消', type: 'warning' })
    return true
  } catch {
    return false
  }
}
async function removeTask(t: Task) {
  if (t.jobId != null) await api.mediaJobRemove(t.jobId).catch(() => {})
  await api.filetrRecentRemove(t.path, t.target).catch(() => {})
}
async function act(what: string, t: Task) {
  try {
    switch (what) {
      case 'cancel':
      case 'stop':
        if (t.jobId != null) await api.mediaJobCancel(t.jobId)
        break
      case 'resume':
      case 'retry':
        await start(t.path, taskOpts(t))
        break
      case 'generate':
        await start(t.path, taskOpts(t, { preview: false }))
        break
      case 'open':
        if (t.output) await api.openFile(t.output)
        break
      case 'reveal':
        if (t.output) await api.revealFile(t.output)
        break
      case 'source':
        await api.revealFile(t.path)
        break
      case 'read':
        await pick(t)
        break
      case 'repick':
        await choose()
        break
      case 'regen':
        dlg.value = { paths: [t.path], opts: { ...(taskOpts(t, { preview: false }) as FtOpts), pages: '' }, title: '换设置重新生成', ok: '重新生成' }
        break
      case 'retranslate':
        if (await ask('会清掉这份文件已有的译文缓存（包括你改过的译文），再从头翻译。继续吗？', '清掉并重翻')) {
          await api.filetrCacheDelete(t.path, t.target)
          await start(t.path, taskOpts(t))
        }
        break
      case 'copyerr':
        await api.copyText(t.error)
        ElMessage.success('已复制出错信息')
        break
      case 'remove':
        await removeTask(t)
        await Promise.all([refreshJobs(), refreshRecents()])
        break
      case 'delcache':
        if (await ask('会删掉这份文件的译文缓存，包括你改过的译文，不能恢复。原文件不受影响。继续吗？', '删除')) {
          await api.filetrCacheDelete(t.path, t.target)
          await removeTask(t)
          await Promise.all([refreshJobs(), refreshRecents()])
          if (t.path === cur.value) await loadView()
        }
        break
    }
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function bulk(what: 'clear' | 'stopAll' | 'resumeAll') {
  if (what === 'clear') {
    for (const t of tasks.value) if (t.state !== 'running' && t.state !== 'queued') await removeTask(t)
    await Promise.all([refreshJobs(), refreshRecents()])
    ElMessage.success('已清除已结束的任务（译文缓存还在）')
  } else if (what === 'stopAll') {
    for (const t of tasks.value) if ((t.state === 'running' || t.state === 'queued') && t.jobId != null) await api.mediaJobCancel(t.jobId).catch(() => {})
  } else {
    for (const t of filterTasks(tasks.value, 'all').filter((x) => x.state === 'stopped' && !x.missing)) await start(t.path, taskOpts(t))
  }
}

// 阅读器里的操作
async function onReaderAct(what: 'resume' | 'retry' | 'generate' | 'open' | 'stop') {
  const t = curTask.value
  if (t) return act(what, t)
  if (cur.value) await start(cur.value, { ...o.value, pages: '' })
}
async function onExport(kind: 'file' | 'reveal' | 'txt' | 'md' | 'srt' | 'srt-trans' | 'copy') {
  if (!cur.value) return
  const t = curTask.value
  try {
    if (kind === 'file') {
      if (t?.state === 'generated') return act('open', t)
      return start(cur.value, t ? taskOpts(t, { preview: false }) : { ...o.value, preview: false, pages: '' })
    }
    if (kind === 'reveal') return t && act('reveal', t)
    if (kind === 'copy') {
      const text = (view.value?.pages ?? []).map((p) => p.src.map((s, i) => (o.value.bilingual ? `${s}\n${p.trans[i] ?? ''}` : (p.trans[i] ?? s))).join('\n\n')).join('\n\n')
      await api.copyText(text)
      return ElMessage.success('已复制全文')
    }
    const ext = kind === 'txt' ? 'txt' : kind === 'md' ? 'md' : 'srt'
    const stem = baseName(cur.value).replace(/\.[^.]+$/, '')
    const dest = await save({ defaultPath: `${stem}.${kind === 'srt-trans' ? 'translated.' : ''}${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] })
    if (!dest) return
    await api.filetrExport(cur.value, o.value.target, kind, dest)
    ElMessage.success('已保存')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div class="ft">
    <header class="h1">
      <h1>文件翻译</h1>
      <p>PDF（含扫描版）、Word、PPT、Excel、EPUB、TXT、Markdown、字幕、图片，译完版面和格式不变，可双语对照，术语和专家沿用你的设置</p>
    </header>
    <div class="cols">
      <aside class="lc">
        <div class="drop" :class="{ over }" @click="choose">
          <b>把文件拖到这里，或点击选择</b>
          <div class="fmt"><span v-for="f in FORMATS" :key="f">{{ f }}</span></div>
          <small class="mute">图片里的文字请用“截图翻译”（Ctrl+Alt+T）</small>
        </div>

        <FtTasks v-model:filter="filter" :tasks="tasks" :cur="cur" :eta="eta" @pick="pick" @act="act" @bulk="bulk" />

        <div class="card">
          <h3>翻译设置</h3>
          <div class="fld">
            <span>语言</span>
            <el-select v-model="o.target" size="small" filterable allow-create default-first-option class="grow">
              <el-option v-for="l in TARGET_LANGS" :key="l" :value="l" :label="`自动检测 → ${l}`" />
            </el-select>
          </div>
          <div class="fld">
            <span>输出</span>
            <div class="seg">
              <span :class="{ on: o.bilingual }" @click="o.bilingual = true">双语对照</span>
              <span :class="{ on: !o.bilingual }" @click="o.bilingual = false">仅译文</span>
            </div>
          </div>
          <div class="fld">
            <span>PDF</span>
            <div class="seg">
              <span :class="{ on: o.pdfMode === 'inplace' }" title="译文写回原来的位置，版面、图片、页码不动" @click="o.pdfMode = 'inplace'">原处替换</span>
              <span :class="{ on: o.pdfMode === 'interleave' }" title="每页原文后面跟一页译文" @click="o.pdfMode = 'interleave'">原页+译文页</span>
              <span :class="{ on: o.pdfMode === 'txt' }" title="只取出文字，输出 TXT" @click="o.pdfMode = 'txt'">只要文字</span>
            </div>
          </div>
          <div class="fld"><span>开始前</span><el-switch v-model="confirmStart" size="small" /><small class="mute">先确认页范围、跳过哪些内容和请求数</small></div>
          <div class="adv" role="button" tabindex="0" @click="advanced = !advanced" @keydown.enter="advanced = !advanced">{{ advanced ? '▾' : '▸' }} 高级：专家、术语库、先预览、识字</div>
          <template v-if="advanced">
            <div class="fld">
              <span>专家</span>
              <el-select v-model="expertId" size="small" class="grow" placeholder="不使用">
                <el-option value="" label="不使用" />
                <el-option value="__auto" label="自动选择" />
                <el-option v-for="e in experts" :key="e.id" :value="e.id" :label="e.name" />
              </el-select>
            </div>
            <div class="fld"><span>术语库</span><el-switch v-model="glossaryOn" size="small" /><small class="mute">按内容自动带入相关术语</small></div>
            <div class="fld"><span>预览</span><el-switch v-model="o.preview" size="small" /><small class="mute">先译完在右边检查、改译文，再点“生成文件”</small></div>
            <div class="fld"><span>排除</span><div class="chk"><el-checkbox v-model="o.skipHeaders" size="small">页眉页脚页码</el-checkbox><el-checkbox v-model="o.skipCode" size="small">公式代码网址</el-checkbox><el-checkbox v-model="o.skipRefs" size="small">参考文献</el-checkbox></div></div>
            <div class="fld"><span>扫描页</span><div class="chk"><el-checkbox v-model="o.scanAuto" size="small" title="有文字层的 PDF 里夹着扫描页时，自动识字并一起翻译">自动识别扫描页</el-checkbox></div></div>
<div class="fld">
            <span>识字</span>
            <el-select v-model="o.ocr" size="small" class="grow">
              <el-option value="vision" label="视觉模型（默认）" />
              <el-option value="enhanced" label="本机增强识字（开源 PaddleOCR）" />
              <el-option value="system" label="Windows 自带识字（不用下载，只在 Windows 上可用）" />
              <el-option value="api" label="在线识字接口（高精度）" />
            </el-select>
          </div>
          <div v-if="o.ocr !== 'vision'" class="pdfx">
            <template v-if="o.ocr === 'enhanced'">
              <div class="row1"><b>增强识字</b><span :class="ocrSt?.installed ? 'okc' : 'mute'">{{ ocrSt?.installed ? '已安装' : ocrSt?.supported === false ? '这台电脑暂不支持' : '还没安装' }}</span></div>
              <div class="row2"><el-button v-if="ocrSt?.supported !== false && !ocrSt?.installed" size="small" type="primary" :loading="ocrBusy" @click="ocrInstall">下载并安装（约 15 MB）</el-button></div>
              <small class="mute">PaddleOCR 开源模型，中英日韩等常见语言，全程在本机运行，不上传页面。比视觉模型更稳、更便宜，位置也更准；手写、艺术字、特别复杂的版面仍可能出错。</small>
            </template>
            <template v-else-if="o.ocr === 'system'">
              <div class="row1"><b>Windows 自带识字</b></div>
              <small class="mute">用 Windows 10/11 自带的识字，不用下载、不上传页面；识别的语言取决于系统里装了哪些语言包（“屏幕翻译 → 识字”里可以选）。小字会先放大再识别。</small>
            </template>
            <template v-else>
              <div class="row1"><b>在线识字接口</b><span :class="apiReady ? 'okc' : 'mute'">{{ apiReady ? apiProvider : '还没填密钥' }}</span></div>
              <small class="mute">用“屏幕翻译 → 识字”里配好的接口（OCR.space、百度高精度、Google）。页面图片会发给这家服务；密钥没填的话先去那里填。</small>
              <div class="row2"><el-button v-if="!apiReady" size="small" @click="app.view = 'screen'">去填密钥</el-button></div>
            </template>
          </div>

          <div v-if="o.pdfMode !== 'txt'" class="pdfx">
            <div class="row1"><b>PDF 排版组件</b><span :class="pdfx?.installed ? 'okc' : 'mute'">{{ pdfx?.note ?? '…' }}</span></div>
            <div class="row2">
              <el-button v-if="pdfx?.supported && !pdfx.installed" size="small" type="primary" :loading="pdfxBusy === 'install'" @click="pdfxDo('install')">安装 PDF 排版组件</el-button>
              <el-button v-if="pdfx?.installed && !pdfx.fontOk" size="small" :loading="pdfxBusy === 'font'" @click="pdfxDo('font')">下载译文字体（约 10 MB）</el-button>
              <el-button v-if="pdfx?.installed" size="small" text :loading="pdfxBusy === 'remove'" @click="pdfxDo('remove')">卸载</el-button>
            </div>
            <small class="mute">没装组件时 PDF 只输出 TXT。文字版 PDF 把译文放回原来的位置（放不下会缩小字号，最小 6 磅）；扫描版和图片先交给视觉模型识字并给出位置（需要支持看图的模型，一次最多 60 页）。公式、竖排、重叠的文字可能错位，输出后请抽查。</small>
          </div>
          <small v-else class="mute">只取出 PDF 里的文字，输出 TXT。</small>

          </template>
        </div>
      </aside>

      <FtReader :path="cur" :target="o.target" :view="view" :task="curTask" :opts="o" @act="onReaderAct" @export="onExport" />
    </div>
    <FtStartDialog v-model="dlgOpen" :paths="dlg?.paths ?? []" :opts="dlg?.opts ?? o" :title="dlg?.title" :confirm-text="dlg?.ok" @confirm="onConfirm" />
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
.adv { font-size: 12.5px; color: var(--el-color-primary); cursor: pointer; user-select: none }
.chk { display: flex; flex-direction: column }
</style>
