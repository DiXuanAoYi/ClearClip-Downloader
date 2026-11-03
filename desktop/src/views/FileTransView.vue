<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { AiExpert, FileTransView, JobSnap } from '../types'
import { registerDrop } from '../utils/dropHub'
import { TARGET_LANGS } from '../utils/langs'

/** 文件翻译：拖入文件 → 一页一页翻 → 左原文右译文对照阅读 → 导出。设计图 ④。 */
const app = useAppStore()
const EXTS = ['pdf', 'epub', 'txt', 'md', 'markdown', 'srt', 'vtt', 'ass', 'ssa', 'docx']
const FORMATS = ['PDF', 'EPUB', 'DOCX', 'TXT', 'MD', 'SRT / ASS']

const jobs = ref<JobSnap[]>([])
const files = ref<Record<number, string>>({}) // 任务编号 → 源文件路径
const cur = ref<string>('') // 当前查看的源文件路径
const view = ref<FileTransView | null>(null)
const page = ref(0)
const show = ref<'src' | 'both' | 'trans'>('both')
const target = ref(app.settings?.ai.targetLang || '简体中文')
const bilingual = ref(true)
const experts = ref<AiExpert[]>([])
const over = ref(false)
let offs: UnlistenFn[] = []
let release: (() => void) | null = null

const mine = computed(() => jobs.value.filter((j) => j.op === '文件翻译').sort((a, b) => b.id - a.id))
const curJob = computed(() => mine.value.find((j) => files.value[j.id] === cur.value))
const pages = computed(() => view.value?.pages ?? [])
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
  if (!ok.length) return ElMessage.warning('支持 PDF、EPUB、DOCX、TXT、Markdown 和 SRT / VTT / ASS 字幕。图片请用“截图翻译”。')
  for (const p of ok) await start(p)
}

async function start(path: string) {
  try {
    const id = await api.filetrStart(path, { bilingual: bilingual.value, target: target.value })
    files.value = { ...files.value, [id]: path }
    cur.value = path
    page.value = 0
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
async function cancel(j: JobSnap) {
  await api.mediaJobCancel(j.id).catch(() => {})
}
async function openOut() {
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
      <p>PDF、EPUB、DOCX、TXT、Markdown、字幕，译成双语对照，术语和专家沿用你的设置</p>
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
              <el-button v-if="j.status === 'running' || j.status === 'queued'" size="small" text @click.stop="cancel(j)">取消</el-button>
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
          <div class="fld">
            <span>输出</span>
            <div class="seg">
              <span :class="{ on: bilingual }" @click="bilingual = true">双语对照</span>
              <span :class="{ on: !bilingual }" @click="bilingual = false">仅译文</span>
            </div>
          </div>
          <div class="sw on"><i />保留排版：公式、表格、图片位置原样不动（只替换文字）</div>
          <small class="mute">PDF 只能取出文字，输出为 TXT；扫描版 PDF（没有文字层）暂不能翻译，请先用文字识别软件转成可复制文字。</small>
        </div>
      </aside>

      <section class="rd">
        <div class="tb">
          <span class="t">{{ cur ? baseName(cur) : '阅读' }}</span>
          <template v-if="pages.length">
            <span class="mute">第 {{ page + 1 }} / {{ pages.length }} 页</span>
            <el-button size="small" :disabled="page <= 0" @click="page--">上一页</el-button>
            <el-button size="small" :disabled="page >= pages.length - 1" @click="page++">下一页</el-button>
          </template>
          <span class="sp" />
          <div class="seg w210">
            <span :class="{ on: show === 'src' }" @click="show = 'src'">原文</span>
            <span :class="{ on: show === 'both' }" @click="show = 'both'">对照</span>
            <span :class="{ on: show === 'trans' }" @click="show = 'trans'">译文</span>
          </div>
          <el-dropdown trigger="click" @command="exportAs">
            <el-button size="small">导出 ▾</el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="file" :disabled="curJob?.status !== 'done'">在文件夹里显示译好的文件</el-dropdown-item>
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
        <div v-else class="pg" :class="show">
          <div v-if="pg && show !== 'trans'" class="pane">
            <div class="lab">原文</div>
            <p v-for="(s, i) in pg.src" :key="i" :class="{ hl: !pg.trans[i] }">{{ s }}</p>
          </div>
          <div v-if="pg && show !== 'src'" class="pane">
            <div class="lab">译文</div>
            <p v-for="(_s, i) in pg.src" :key="i" :class="{ miss: !pg.trans[i] }">{{ pg.trans[i] || (curJob && curJob.status !== 'done' && curJob.status !== 'failed' ? '翻译中…' : '（没有译文，已保留原文）') }}</p>
          </div>
          <div v-if="!pg" class="empty"><span class="mute">读取中…</span></div>
        </div>

        <div class="st">
          <span><b>已译 {{ donePages }} / {{ pages.length }} 页</b></span>
          <span>{{ doneSegs }} / {{ totalSegs }} 段</span>
          <span v-if="curJob?.note" class="mute">{{ curJob.note }}</span>
          <span class="sp" />
          <el-button size="small" :disabled="!cur || !failedSegs || curJob?.status === 'running'" @click="retry">重试失败段落{{ failedSegs && curJob?.status !== 'running' ? `（${failedSegs}）` : '' }}</el-button>
          <el-button size="small" type="primary" :disabled="curJob?.status !== 'done'" @click="openOut">导出双语文件</el-button>
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
.pg { flex: 1; display: grid; grid-template-columns: 1fr 1fr; min-height: 0; overflow-y: auto }
.pg.src, .pg.trans { grid-template-columns: 1fr }
.pane { padding: 18px 22px; display: flex; flex-direction: column; gap: 12px }
.pane + .pane { border-left: 1px solid var(--el-border-color); background: var(--el-fill-color-lighter) }
.pg.src .pane + .pane, .pg.trans .pane + .pane { border-left: 0 }
.lab { font-size: 11px; letter-spacing: .06em; color: var(--el-text-color-secondary); font-weight: 700 }
.pane p { margin: 0; line-height: 1.75; font-size: 13.5px; white-space: pre-wrap; word-break: break-word }
.pane p.hl { background: var(--el-color-primary-light-9); border-radius: 6px; padding: 6px 8px; margin: -6px -8px }
.pane p.miss { color: var(--el-text-color-secondary) }
.empty { flex: 1; display: flex; flex-direction: column; gap: 6px; align-items: center; justify-content: center; padding: 24px; text-align: center }
.st { display: flex; gap: 16px; align-items: center; padding: 9px 14px; border-top: 1px solid var(--el-border-color); color: var(--el-text-color-secondary); font-size: 12.5px; flex-wrap: wrap }
.st b { color: var(--el-text-color-primary) }
</style>
