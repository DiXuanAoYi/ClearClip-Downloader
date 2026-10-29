<script setup lang="ts">
// 剪辑：把多个视频 / 图片在时间线上排好，裁剪、分割、加转场、文字和配乐，还能在主轨上面叠加多条素材（画中画、贴纸、GIF），导出成一个视频。
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { open, save } from '@tauri-apps/plugin-dialog'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../../api'
import type { EditProject, JobSnap } from '../../types'
import { useEditProject, type AddReport } from '../../composables/useEditProject'
import { useRegions } from '../../composables/useRegions'
import { registerDrop } from '../../utils/dropHub'
import { EDIT_AUDIO_EXTS, EDIT_IMAGE_EXTS, EDIT_VIDEO_EXTS, MAX_AUDIO, MAX_CLIPS, MAX_OVERLAYS, MAX_TEXTS, clamp, editMediaKind, msText, outputSize } from '../../utils/edit'
import FileInput from './FileInput.vue'
import EditInspector from './edit/EditInspector.vue'
import EditMonitor from './edit/EditMonitor.vue'
import EditTimeline from './edit/EditTimeline.vue'

const props = defineProps<{ preset?: string[] }>()
const emit = defineEmits<{ consumed: [] }>()

const ed = useEditProject()
const rg = useRegions(ed)
const root = ref<HTMLElement | null>(null)
const tlBox = ref<HTMLElement | null>(null)
const timeline = ref<InstanceType<typeof EditTimeline> | null>(null)

const VIDEO_EXTS = EDIT_VIDEO_EXTS
const IMAGE_EXTS = EDIT_IMAGE_EXTS
const AUDIO_EXTS = EDIT_AUDIO_EXTS

const pps = ref(50)
const outDir = ref('')
const monitorMode = ref<'live' | 'preview'>('live')

const SNAP_KEY = 'clearclip.edit.snap'
const snapOn = ref(true)
try {
  snapOn.value = localStorage.getItem(SNAP_KEY) !== '0'
} catch {
  /* 读不到就保持默认：吸附开着 */
}
watch(snapOn, (v) => {
  try {
    localStorage.setItem(SNAP_KEY, v ? '1' : '0')
  } catch {
    /* 记不住不影响使用 */
  }
})

// ---------- 添加素材 ----------

const pickedClips = ref<string[]>([])
const pickedOverlays = ref<string[]>([])
const pickedMusic = ref<string[]>([])
/** 三个文件选择器藏在“添加素材”菜单后面，菜单项调用它们的选择文件 / 媒体库 */
const mainIn = ref<InstanceType<typeof FileInput> | null>(null)
const overlayIn = ref<InstanceType<typeof FileInput> | null>(null)
const musicIn = ref<InstanceType<typeof FileInput> | null>(null)

async function importSubtitles() {
  try {
    const r = await open({ multiple: false, filters: [{ name: '字幕文件', extensions: ['srt', 'vtt', 'ass', 'ssa'] }] })
    if (!r || Array.isArray(r)) return
    if (!hasClips.value) return void ElMessage.info('先把视频放到主轨上，再导入字幕。')
    const { added, skipped } = await ed.importSubtitles(r)
    if (!added && !skipped) ElMessage.warning('这个字幕文件里没有读到字幕。')
    else ElMessage({ type: skipped ? 'warning' : 'success', message: skipped ? `已导入 ${added} 条字幕，还有 ${skipped} 条放不下（文字最多 ${MAX_TEXTS} 条）。` : `已导入 ${added} 条字幕，可以在时间线的文字轨上调整。` })
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function addMenu(cmd: string) {
  if (cmd === 'subtitle') return void importSubtitles()
  const [target, via] = cmd.split(':')
  const picker = target === 'main' ? mainIn.value : target === 'overlay' ? overlayIn.value : musicIn.value
  if (via === 'library') picker?.openPicker()
  else picker?.browse()
}

function report(r: AddReport) {
  if (r.errors.length) ElMessage({ type: r.added ? 'warning' : 'error', message: r.errors.slice(0, 3).join('\n') + (r.errors.length > 3 ? `\n……还有 ${r.errors.length - 3} 个` : ''), duration: 6000, showClose: true })
}
async function addClips(paths: string[], at?: number) {
  if (!paths.length) return
  const before = ed.total.value
  report(await ed.addClips(paths, at))
  if (!before) fit()
}
/** 添加到叠加轨。主轨还是空的时候叠加素材没有地方出现，就先放到主轨上。 */
async function addOverlays(paths: string[], at?: { track?: number; startMs?: number }) {
  if (!paths.length) return
  if (!hasClips.value) {
    ElMessage.info('主轨上还没有视频，先把它放到主轨上；之后再添加的素材可以叠加在它上面。')
    return addClips(paths.slice(0, 1))
  }
  report(await ed.addOverlays(paths, at))
}
async function addMusic(paths: string[], startMs?: number) {
  if (!paths.length) return
  report(await ed.addMusic(paths, startMs))
}
watch(pickedClips, (v) => {
  if (!v.length) return
  pickedClips.value = []
  addClips(v)
})
watch(pickedOverlays, (v) => {
  if (!v.length) return
  pickedOverlays.value = []
  addOverlays(v)
})
watch(pickedMusic, (v) => {
  if (!v.length) return
  pickedMusic.value = []
  addMusic(v)
})
watch(
  () => props.preset,
  (v) => {
    if (!v?.length) return
    addClips([...v])
    emit('consumed')
  },
  { immediate: true },
)

function addText() {
  if (!ed.addText()) ElMessage.warning(`文字最多 ${MAX_TEXTS} 条。`)
}

// ---------- 播放 ----------

const hasClips = computed(() => ed.project.value.clips.length > 0)
const timeText = computed(() => `${msText(ed.playhead.value).replace(/\.\d+$/, (m) => m.slice(0, 3))} / ${msText(ed.total.value).replace(/\.\d+$/, (m) => m.slice(0, 3))}`)

function togglePlay() {
  if (!hasClips.value) return
  ed.playing.value = !ed.playing.value
}
function goto(ms: number) {
  ed.playhead.value = clamp(Math.round(ms), 0, ed.total.value)
  nextTick(() => timeline.value?.reveal())
}
function step(ms: number) {
  ed.playing.value = false
  goto(ed.playhead.value + ms)
}
/** 跳到上一个 / 下一个剪辑点 */
function jump(dir: -1 | 1) {
  ed.playing.value = false
  const t = ed.cutPointFrom(dir)
  if (t !== null) goto(t)
}
/** 前进 / 后退一帧 */
function frame(dir: -1 | 1) {
  ed.playing.value = false
  goto(ed.frameFrom(dir))
}

watch(
  () => ed.total.value,
  (t) => {
    if (ed.playhead.value > t) ed.playhead.value = t
  },
)

// ---------- 缩放 ----------

function setZoom(v: number) {
  pps.value = clamp(Math.round(v), 5, 400)
}
function fit() {
  nextTick(() => {
    const w = (tlBox.value?.clientWidth ?? 800) - 64 - 48
    const secs = Math.max(1, (ed.extent.value || 10000) / 1000)
    pps.value = Math.round(clamp(w / secs, 5, 400))
  })
}

// ---------- 编辑操作 ----------

function split() {
  if (!ed.splitAtPlayhead()) ElMessage.info('请先把播放位置移到片段中间（离片段两端至少 0.1 秒），再分割。')
}
function duplicate() {
  if (!ed.duplicateSelected()) ElMessage.warning('没有选中的内容，或已经到了数量上限。')
}
/** 向左 / 向右裁剪：把选中的（或播放位置下的）片段在播放位置裁掉前面 / 后面 */
function trimHere(side: 'in' | 'out') {
  if (!ed.trimAtPlayhead(side)) ElMessage.info('请先把播放位置移到片段中间（离片段两端至少 0.1 秒），再裁剪。')
}
function rippleDelete() {
  const n = ed.rippleDeleteSelected()
  if (n === null) return ed.removeSelected()
  if (n) ElMessage.info(`后面的 ${n} 个叠加素材 / 文字 / 配乐也一起前移了。`)
}
function detachAudio() {
  const why = ed.detachSelectedAudio()
  if (why) ElMessage.info(why)
  else ElMessage.success('声音已经分离到配乐轨，原来的画面静音了。')
}
const canDetach = computed(() => {
  const s = ed.sel.value
  return (s?.kind === 'clip' && ed.selectedClip.value?.kind === 'video') || (s?.kind === 'overlay' && ed.selectedOverlay.value?.kind === 'video')
})
const selectedLabel = computed(() => (ed.sel.value?.kind === 'clip' ? '片段' : ed.sel.value?.kind === 'overlay' ? '叠加素材' : ed.sel.value?.kind === 'text' ? '文字' : ed.sel.value?.kind === 'audio' ? '配乐' : ''))

// ---------- 工程文件 ----------

const FILTERS = [{ name: '清影剪辑工程', extensions: ['ccedit'] }]
const hasContent = computed(() => hasClips.value || ed.project.value.overlays.length > 0 || ed.project.value.texts.length > 0 || ed.project.value.audio.length > 0)

async function discardOk(): Promise<boolean> {
  if (!ed.dirty.value || !hasContent.value) return true
  try {
    await ElMessageBox.confirm('当前的工程还没有保存，继续会丢掉这些修改。', '放弃未保存的修改？', { type: 'warning', confirmButtonText: '放弃修改', cancelButtonText: '取消' })
    return true
  } catch {
    return false
  }
}

async function newProject() {
  if (!(await discardOk())) return
  ed.newProject()
  ed.clearDraft()
}

function reportMissing(missing: string[]) {
  if (missing.length) ElMessage({ type: 'warning', message: `有 ${missing.length} 个素材文件找不到，已在时间线上标红。选中后可以重新指定。`, duration: 6000, showClose: true })
}

async function openProject() {
  if (!(await discardOk())) return
  try {
    const r = await open({ multiple: false, filters: FILTERS })
    if (!r || Array.isArray(r)) return
    const missing = await ed.openProject(r)
    fit()
    reportMissing(missing)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

/** “打开”旁边的菜单：最近的工程 */
async function openRecent(cmd: string) {
  if (cmd === '*clear') return ed.forgetRecent()
  if (!(await discardOk())) return
  try {
    const missing = await ed.openProject(cmd)
    fit()
    reportMissing(missing)
  } catch (e) {
    ed.forgetRecent(cmd)
    ElMessage.error(`打不开这个工程，已从最近的列表里去掉：${errorText(e)}`)
  }
}
const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p

async function saveAs(): Promise<boolean> {
  try {
    const name = (ed.project.value.title.trim() || '未命名') + '.ccedit'
    let p = await save({ filters: FILTERS, defaultPath: ed.filePath.value || name })
    if (!p) return false
    if (!/\.ccedit$/i.test(p)) p += '.ccedit'
    await ed.saveProject(p)
    ElMessage.success('已保存')
    return true
  } catch (e) {
    ElMessage.error(errorText(e))
    return false
  }
}
async function saveProject() {
  if (!ed.filePath.value) return void (await saveAs())
  try {
    await ed.saveProject(ed.filePath.value)
    ElMessage.success('已保存')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

// ---------- 草稿 ----------

const draft = ref<{ project: EditProject; filePath: string } | null>(null)
onMounted(() => {
  if (!props.preset?.length) draft.value = ed.readDraft()
})
async function resumeDraft() {
  const d = draft.value
  draft.value = null
  if (!d) return
  try {
    await ed.restoreDraft(d)
    fit()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
function dropDraft() {
  draft.value = null
  ed.clearDraft()
}
watch(hasContent, (v) => {
  if (v) draft.value = null
})

// ---------- 精确预览（可选：和导出同一套流程渲染，实时画面是近似效果） ----------

const projectKey = computed(() => JSON.stringify(ed.project.value))
const preview = ref<{ url: string; key: string } | null>(null)
const previewJob = ref<JobSnap | null>(null)
const previewId = ref<number | null>(null)
const previewError = ref('')
const building = computed(() => previewJob.value !== null && (previewJob.value.status === 'queued' || previewJob.value.status === 'running'))
const stale = computed(() => !!preview.value && preview.value.key !== projectKey.value)

let unlisten: UnlistenFn | undefined
onMounted(async () => {
  unlisten = await events.onMediaJobs(async (list) => {
    const id = previewId.value
    if (id === null) return
    const j = list.find((x) => x.id === id)
    if (!j) return
    previewJob.value = j
    if (j.status === 'done' && j.output) {
      previewId.value = null
      try {
        const url = await api.editPreviewUrl(j.output)
        preview.value = { url, key: buildKey }
        monitorMode.value = 'preview'
        ed.playing.value = false
      } catch (e) {
        previewError.value = errorText(e)
      }
    } else if (j.status === 'failed') {
      previewId.value = null
      previewError.value = j.error ?? '生成预览失败。'
    } else if (j.status === 'canceled') {
      previewId.value = null
    }
  })
})
onBeforeUnmount(() => unlisten?.())

let buildKey = ''
async function buildPreview() {
  if (!hasClips.value || building.value) return
  if (!checkMedia()) return
  previewError.value = ''
  buildKey = projectKey.value
  try {
    ed.playing.value = false
    previewId.value = await api.editPreviewStart(JSON.parse(buildKey))
    previewJob.value = null
  } catch (e) {
    previewError.value = errorText(e)
  }
}
async function cancelPreview() {
  if (previewId.value !== null) await api.mediaJobCancel(previewId.value).catch(() => undefined)
}
/** 点“精确预览”：已经是最新的就直接切过去，否则先生成，生成好了自动切过去 */
function setMode(v: string | number | boolean | undefined) {
  if (v === 'preview' && (!preview.value || stale.value)) return void buildPreview()
  monitorMode.value = v === 'preview' ? 'preview' : 'live'
  if (monitorMode.value === 'preview') ed.playing.value = false
}
watch(stale, (s) => {
  if (s && monitorMode.value === 'preview') {
    ed.playing.value = false
    monitorMode.value = 'live'
  }
})

// ---------- 导出 ----------

const exporting = ref(false)

/** 有素材找不到时不能导出 / 预览，提示是哪几个。 */
function checkMedia(): boolean {
  const used = new Set([...ed.project.value.clips.map((c) => c.path), ...ed.project.value.overlays.map((o) => o.path), ...ed.project.value.audio.map((a) => a.path)])
  const bad = [...used].filter((p) => p in ed.broken)
  if (!bad.length) return true
  ElMessage({ type: 'error', message: `有 ${bad.length} 个素材找不到或读不出来（在时间线上标红）：${bad.slice(0, 2).map((p) => p.split(/[\\/]/).pop()).join('、')}${bad.length > 2 ? '…' : ''}。请重新指定或删除这些片段。`, duration: 7000, showClose: true })
  return false
}

async function exportNow() {
  if (!hasClips.value || exporting.value) return
  if (!checkMedia()) return
  exporting.value = true
  try {
    await api.mediaJobStart({ op: 'edit', project: JSON.parse(projectKey.value), inputs: [], outputDir: outDir.value || null })
    ElMessage.success('已开始导出，进度见下方的“处理进度”。')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    exporting.value = false
  }
}

// ---------- 键盘 ----------

const SHORTCUTS: [string, string][] = [
  ['空格', '播放 / 暂停'],
  ['S', '在播放位置分割'],
  ['Q / W', '向左 / 向右裁剪到播放位置'],
  ['Delete', '删除选中的'],
  ['Shift+Delete', '波纹删除（后面的叠加素材、文字、配乐一起前移）'],
  ['Ctrl+D', '复制选中的'],
  ['Ctrl+Z / Ctrl+Y', '撤销 / 重做'],
  ['Ctrl+S', '保存工程'],
  ['← →', '前后 1 秒（加 Shift 为 0.1 秒）'],
  [', .', '后退 / 前进一帧'],
  ['[ ]', '上一个 / 下一个剪辑点'],
  ['Home / End', '回到开头 / 跳到结尾'],
  ['Ctrl+滚轮', '缩放时间线'],
  ['拖文件', '把文件拖到时间线的某一行，就放进那一行'],
]

function visible(): boolean {
  return !!root.value && root.value.offsetParent !== null
}
function onKey(e: KeyboardEvent) {
  if (!visible()) return
  const t = e.target as HTMLElement | null
  if (t?.closest('textarea, select, [contenteditable="true"], .el-overlay, .el-popper')) return
  const input = t?.closest('input') as HTMLInputElement | null
  if (input) {
    // 点过单选 / 复选按钮（比如检查器的标签页）后焦点会留在上面：快捷键照常生效，空格、方向键和回车留给它们自己用
    if (input.type !== 'radio' && input.type !== 'checkbox') return
    if (e.key === ' ' || e.key === 'Enter' || e.key.startsWith('Arrow')) return
  }
  const mod = e.ctrlKey || e.metaKey
  const key = e.key.toLowerCase()
  if (mod && key === 'd') {
    e.preventDefault()
    duplicate()
  } else if (mod && key === 'z') {
    e.preventDefault()
    e.shiftKey ? ed.redo() : ed.undo()
  } else if (mod && key === 'y') {
    e.preventDefault()
    ed.redo()
  } else if (mod && key === 's') {
    e.preventDefault()
    saveProject()
  } else if (mod || e.altKey) {
    return
  } else if (e.key === ' ' && t?.tagName !== 'BUTTON') {
    e.preventDefault()
    togglePlay()
  } else if (key === 's') {
    split()
  } else if (key === 'q') {
    trimHere('in')
  } else if (key === 'w') {
    trimHere('out')
  } else if (e.key === '[') {
    jump(-1)
  } else if (e.key === ']') {
    jump(1)
  } else if (e.key === ',') {
    frame(-1)
  } else if (e.key === '.') {
    frame(1)
  } else if (e.key === 'Escape' && rg.drawing.value) {
    rg.cancelDraw()
  } else if ((e.key === 'Delete' || e.key === 'Backspace') && rg.tabActive.value && rg.region.value) {
    // 在“区域”页选中了区域时，删除键删的是区域，不是整个片段
    e.preventDefault()
    rg.remove(rg.region.value.id)
  } else if ((e.key === 'Delete' || e.key === 'Backspace') && e.shiftKey) {
    e.preventDefault()
    rippleDelete()
  } else if (e.key === 'Delete' || e.key === 'Backspace') {
    e.preventDefault()
    ed.removeSelected()
  } else if (e.key === 'ArrowLeft') {
    e.preventDefault()
    step(e.shiftKey ? -100 : -1000)
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    step(e.shiftKey ? 100 : 1000)
  } else if (e.key === 'Home') {
    goto(0)
  } else if (e.key === 'End') {
    goto(ed.total.value)
  }
}
onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))

// ---------- 信息 ----------

const outInfo = computed(() => {
  const s = outputSize(ed.project.value, ed.sources)
  return `${s.w}×${s.h}${ed.project.value.out.fps ? ` · ${ed.project.value.out.fps} 帧/秒` : ''}`
})
const clipCount = computed(() => ed.project.value.clips.length)
const overlayCount = computed(() => ed.project.value.overlays.length)
const sizeWarn = computed(() =>
  ed.project.value.clips.length >= MAX_CLIPS
    ? `片段已达上限 ${MAX_CLIPS} 个`
    : ed.project.value.overlays.length >= MAX_OVERLAYS
      ? `叠加素材已达上限 ${MAX_OVERLAYS} 个`
      : ed.project.value.audio.length >= MAX_AUDIO
        ? `配乐已达上限 ${MAX_AUDIO} 条`
        : '',
)

// ---------- 把文件拖进来 ----------
// 拖到时间线的哪一行就放进哪一行：主轨按位置插入，叠加轨从落点的时间开始，音频行当配乐；拖到别处放到主轨末尾。

const dropHint = ref('')

function dropText(t: { lane: string; track?: number; ms: number } | null): string {
  if (!t) return '松开，添加到主轨末尾'
  const at = msText(t.ms).replace(/\.\d+$/, (m) => m.slice(0, 2))
  if (t.lane === 'main') return '松开，插入到主轨这里'
  if (t.lane === 'audio') return `松开，作为配乐，从 ${at} 开始`
  return `松开，放到叠加 ${t.lane === 'overlay' ? t.track : 1}，从 ${at} 开始`
}

async function dropPaths(paths: string[], x: number, y: number) {
  const project = paths.filter((p) => /\.ccedit$/i.test(p))
  if (project.length) {
    if (!(await discardOk())) return
    try {
      const missing = await ed.openProject(project[0])
      fit()
      reportMissing(missing)
    } catch (e) {
      ElMessage.error(errorText(e))
    }
    return
  }
  const media = paths.filter((p) => editMediaKind(p) !== null)
  const skipped = paths.length - media.length
  if (skipped > 0) ElMessage({ type: 'warning', message: `有 ${skipped} 个文件不是剪辑能用的视频、图片或音频（文件夹不能直接拖入，请把里面的文件选中后拖进来）。`, duration: 5000, showClose: true })
  if (!media.length) return
  const t = timeline.value?.targetAt(x, y) ?? null
  if (!t || t.lane === 'main') return addClips(media, t?.index)
  if (t.lane === 'audio') {
    const audio = media.filter((p) => editMediaKind(p) !== 'image')
    if (audio.length < media.length) ElMessage.warning('图片不能当配乐，已忽略。')
    return addMusic(audio, t.ms)
  }
  return addOverlays(media, { track: t.lane === 'overlay' ? t.track : 1, startMs: t.ms })
}

let releaseDrop: (() => void) | undefined
onMounted(() => {
  releaseDrop = registerDrop({
    accepts(x, y) {
      const r = root.value
      if (!r || r.offsetParent === null) return false
      const b = r.getBoundingClientRect()
      return x >= b.left && x <= b.right && y >= b.top && y <= b.bottom
    },
    over(x, y) {
      timeline.value?.hover(x, y)
      dropHint.value = dropText(timeline.value?.targetAt(x, y) ?? null)
    },
    leave() {
      timeline.value?.hover(null)
      dropHint.value = ''
    },
    drop(paths, x, y) {
      dropHint.value = ''
      timeline.value?.hover(null)
      dropPaths(paths, x, y)
    },
  })
})
onBeforeUnmount(() => releaseDrop?.())
</script>

<template>
  <div ref="root" class="ed">
    <el-alert v-if="draft" type="info" :closable="false" show-icon class="draft">
      <template #title>
        <span>找到上次没保存的剪辑（{{ draft.project.clips.length }} 个片段{{ draft.project.overlays.length ? `，${draft.project.overlays.length} 个叠加素材` : '' }}）。</span>
        <el-button size="small" type="primary" class="gap" @click="resumeDraft">继续编辑</el-button>
        <el-button size="small" @click="dropDraft">不要了</el-button>
      </template>
    </el-alert>

    <!-- 工具栏 -->
    <div class="bar">
      <div class="grp">
        <el-button size="small" @click="newProject">新建</el-button>
        <el-dropdown split-button size="small" trigger="click" @click="openProject" @command="openRecent">
          打开…
          <template #dropdown>
            <el-dropdown-menu>
              <template v-if="ed.recent.value.length">
                <el-dropdown-item v-for="p in ed.recent.value" :key="p" :command="p" :title="p"><span data-no-i18n>{{ baseName(p) }}</span></el-dropdown-item>
                <el-dropdown-item divided command="*clear">清空最近的记录</el-dropdown-item>
              </template>
              <el-dropdown-item v-else disabled>还没有最近打开的工程</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
        <el-button size="small" :disabled="!hasContent" @click="saveProject">保存</el-button>
        <el-button size="small" :disabled="!hasContent" @click="saveAs">另存为…</el-button>
      </div>
      <div class="grp">
        <!-- 点按钮本身是添加到主轨；箭头里是叠加素材、配乐和从媒体库添加 -->
        <el-dropdown split-button type="primary" size="small" trigger="click" @click="addMenu('main')" @command="addMenu">
          添加素材
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="main">添加到主轨…</el-dropdown-item>
              <el-dropdown-item command="main:library">从媒体库添加…</el-dropdown-item>
              <el-dropdown-item divided command="overlay">添加叠加素材…</el-dropdown-item>
              <el-dropdown-item command="overlay:library">从媒体库叠加…</el-dropdown-item>
              <el-dropdown-item divided command="music">添加配乐…</el-dropdown-item>
              <el-dropdown-item command="music:library">从媒体库添加配乐…</el-dropdown-item>
              <el-dropdown-item divided command="subtitle">导入字幕文件（SRT / ASS）…</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
        <el-button size="small" @click="addText">添加文字</el-button>
      </div>
      <div class="grp end">
        <el-button size="small" :disabled="!ed.canUndo.value" @click="ed.undo">撤回</el-button>
        <el-button size="small" :disabled="!ed.canRedo.value" @click="ed.redo">重做</el-button>
      </div>
      <!-- 文件选择器：界面上只有“添加素材”菜单，选择文件和媒体库对话框由它们负责 -->
      <div class="pickers">
        <FileInput ref="mainIn" v-model="pickedClips" multiple compact plain :extensions="[...VIDEO_EXTS, ...IMAGE_EXTS, ...AUDIO_EXTS]" :kinds="['video', 'image']" label="视频或图片" />
        <FileInput ref="overlayIn" v-model="pickedOverlays" multiple compact plain :extensions="[...VIDEO_EXTS, ...IMAGE_EXTS]" :kinds="['video', 'image']" label="叠加的视频、图片或 GIF" />
        <FileInput ref="musicIn" v-model="pickedMusic" multiple compact plain :extensions="[...AUDIO_EXTS, ...VIDEO_EXTS]" :kinds="['audio']" label="音频" />
      </div>
    </div>

    <div v-if="dropHint" class="drophint">{{ dropHint }}</div>

    <div class="main">
      <!-- 监视器 -->
      <div class="left">
        <EditMonitor :ed="ed" :rg="rg" :mode="monitorMode" :preview-url="preview?.url ?? null" />
        <div class="transport">
          <el-button-group size="small">
            <el-button :disabled="!hasClips" title="上一个剪辑点（[）" @click="jump(-1)">«</el-button>
            <el-button :disabled="!hasClips" title="后退一帧（,）" @click="frame(-1)">‹</el-button>
          </el-button-group>
          <el-button size="small" type="primary" :disabled="!hasClips" class="play" @click="togglePlay">{{ ed.playing.value ? '暂停' : '播放' }}</el-button>
          <el-button-group size="small">
            <el-button :disabled="!hasClips" title="前进一帧（.）" @click="frame(1)">›</el-button>
            <el-button :disabled="!hasClips" title="下一个剪辑点（]）" @click="jump(1)">»</el-button>
          </el-button-group>
          <span class="mono time">{{ timeText }}</span>
          <span class="sp" />
          <el-radio-group :model-value="monitorMode" size="small" @update:model-value="setMode">
            <el-radio-button value="live" title="随改随看的近似效果（转场的形状、变速、降噪等细节以导出为准）">实时画面</el-radio-button>
            <el-radio-button value="preview" :disabled="!hasClips || building" title="和导出同一套流程渲染一遍再播放，用来逐帧核对；改过工程后需要重新生成">精确预览</el-radio-button>
          </el-radio-group>
        </div>
        <div v-if="building || stale" class="pvbar">
          <template v-if="building">
            <span class="mute small">正在生成精确预览</span>
            <el-progress :percentage="Math.round(previewJob?.percent ?? 0)" :stroke-width="6" class="pg" />
            <el-button size="small" link @click="cancelPreview">取消</el-button>
          </template>
          <span v-else class="mute small">精确预览已过期（工程改过了），点“精确预览”重新生成</span>
        </div>
        <div v-if="previewError" class="err selectable">{{ previewError }}</div>
      </div>

      <!-- 检查器 -->
      <div class="right">
        <EditInspector :ed="ed" :rg="rg" v-model:out-dir="outDir" @goto="goto" />
      </div>
    </div>

    <!-- 时间线 -->
    <div ref="tlBox" class="tlbox" tabindex="-1">
      <div class="tlbar">
        <el-button size="small" :disabled="!hasClips" title="在播放位置分割（S）" @click="split">分割</el-button>
        <el-button size="small" :disabled="!hasClips" title="去掉播放位置左边的部分（Q）" @click="trimHere('in')">向左裁剪</el-button>
        <el-button size="small" :disabled="!hasClips" title="去掉播放位置右边的部分（W）" @click="trimHere('out')">向右裁剪</el-button>
        <el-button size="small" :disabled="!canDetach" title="把这段素材的声音单独放到配乐轨，原来的画面静音" @click="detachAudio">分离音频</el-button>
        <el-button size="small" :disabled="!ed.sel.value" title="（Ctrl+D）" @click="duplicate">复制{{ selectedLabel }}</el-button>
        <el-button size="small" :disabled="!ed.sel.value" title="（Delete）" @click="ed.removeSelected">删除{{ selectedLabel }}</el-button>
        <el-button v-if="ed.sel.value?.kind === 'clip'" size="small" title="删除片段，并把它后面才开始的叠加素材、文字、配乐一起前移（Shift+Delete）" @click="rippleDelete">波纹删除</el-button>
        <span class="sp" />
        <el-checkbox v-model="snapOn" size="small" title="拖动叠加素材、文字、配乐时，自动靠到附近的片段边界和播放位置上">吸附</el-checkbox>
        <span class="mute small">缩放</span>
        <el-slider v-model="pps" :min="5" :max="400" :show-tooltip="false" size="small" class="zoom" />
        <el-button size="small" link @click="fit">适合窗口</el-button>
        <el-popover placement="top-end" :width="340" trigger="click">
          <template #reference>
            <el-button size="small" link>快捷键</el-button>
          </template>
          <div class="keys">
            <template v-for="k in SHORTCUTS" :key="k[0]">
              <kbd>{{ k[0] }}</kbd>
              <span>{{ k[1] }}</span>
            </template>
          </div>
        </el-popover>
      </div>
      <EditTimeline ref="timeline" :ed="ed" :pps="pps" :snap-on="snapOn" @zoom="setZoom" />
    </div>

    <!-- 导出 -->
    <div class="export card">
      <div class="info">
        <span>共 {{ clipCount }} 个片段</span>
        <span v-if="overlayCount">{{ overlayCount }} 个叠加素材</span>
        <span class="mono">{{ msText(ed.total.value) }}</span>
        <span class="mono">{{ outInfo }}</span>
        <span v-if="ed.filePath.value" class="mute ellipsis" :title="ed.filePath.value">{{ ed.filePath.value.split(/[\\/]/).pop() }}{{ ed.dirty.value ? '（有未保存的修改）' : '' }}</span>
        <span v-else-if="hasContent" class="mute">（工程还没有保存）</span>
        <span v-if="sizeWarn" class="warn">{{ sizeWarn }}</span>
      </div>
      <el-button type="primary" :disabled="!hasClips" :loading="exporting" @click="exportNow">导出视频</el-button>
    </div>
  </div>
</template>

<style scoped>
.ed {
  display: flex;
  flex-direction: column;
  gap: 10px;
  outline: none;
}
.draft .gap {
  margin-left: 12px;
}
.drophint {
  position: sticky;
  top: 0;
  z-index: 30;
  padding: 6px 12px;
  border-radius: 6px;
  background: var(--cc-acc);
  color: #fff;
  font-size: 12.5px;
  text-align: center;
}
.bar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
  align-items: center;
}
.grp {
  display: flex;
  gap: 8px;
  align-items: center;
}
.grp.end {
  margin-left: auto;
}
.main {
  display: grid;
  grid-template-columns: minmax(360px, 1fr) 380px;
  gap: 16px;
  align-items: start;
}
.left {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.right {
  min-width: 0;
  max-height: 470px;
  overflow: auto;
  padding-right: 4px;
}
.transport {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.play {
  min-width: 64px;
}
.time {
  font-size: 12.5px;
}
.sp {
  flex: 1;
}
.pvbar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.pg {
  width: 200px;
}
.small {
  font-size: 11.5px;
}
.ok {
  color: var(--cc-ok);
}
.err {
  color: var(--cc-err);
  font-size: 12px;
  word-break: break-all;
}
.tlbox {
  display: flex;
  flex-direction: column;
  gap: 6px;
  outline: none;
}
.tlbar {
  display: flex;
  align-items: center;
  gap: 8px;
}
.zoom {
  width: 150px;
  margin: 0 8px;
}
.keys {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 5px 12px;
  align-items: baseline;
  font-size: 12.5px;
}
.keys kbd {
  justify-self: start;
  padding: 1px 6px;
  border: 1px solid var(--cc-line);
  border-radius: 4px;
  background: var(--cc-card);
  font-family: var(--cc-mono);
  font-size: 11.5px;
  white-space: nowrap;
}
.pickers {
  display: none;
}
.export {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 14px;
}
.info {
  display: flex;
  gap: 16px;
  align-items: center;
  flex-wrap: wrap;
  font-size: 12.5px;
  min-width: 0;
}
.warn {
  color: var(--cc-err);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 320px;
}
</style>
