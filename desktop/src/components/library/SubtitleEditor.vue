<script setup lang="ts">
// 字幕校对编辑器：表格改字 / 改时间、拆分 / 合并 / 插入 / 删除、查找替换、整体平移、撤销重做、问题提示；有视频时可点行跳转、循环播放当前行。
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import type { SubIssue } from '../../types'
import { formatClock } from '../../utils/time'
import * as S from '../../utils/subedit'
import type { ECue } from '../../utils/subedit'

const props = defineProps<{ modelValue: boolean; path: string; video?: string }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; saved: [path: string] }>()

const cues = ref<ECue[]>([])
const original = ref('')
const issues = ref<SubIssue[]>([])
const loading = ref(false)
const saving = ref(false)
const hist = new S.History()
const histTick = ref(0)
const sel = ref(-1)
const find = ref('')
const repl = ref('')
const opt = ref({ regex: false, matchCase: false })
const shiftMs = ref(0)
const view = ref<S.View>('both')
const videoEl = ref<HTMLVideoElement | null>(null)
const mediaUrl = ref('')
const now = ref(0)
const loop = ref(false)
const listEl = ref<HTMLElement | null>(null)

const dirty = computed(() => JSON.stringify(cues.value) !== original.value)
const matches = computed(() => S.findCues(cues.value, find.value, opt.value))
const issueMap = computed(() => {
  const m = new Map<number, SubIssue[]>()
  for (const i of issues.value) m.set(i.index, [...(m.get(i.index) ?? []), i])
  return m
})
const bilingual = computed(() => cues.value.filter((c) => c.lines.length > 1).length * 2 > cues.value.length)
const canUndo = computed(() => (histTick.value, hist.canUndo))
const canRedo = computed(() => (histTick.value, hist.canRedo))
const current = computed(() => S.cueAt(cues.value, now.value * 1000))

watch(
  () => [props.modelValue, props.path] as const,
  async ([open]) => {
    if (!open) return
    loading.value = true
    try {
      cues.value = await api.subeditLoad(props.path)
      original.value = JSON.stringify(cues.value)
      hist.past = []
      hist.future = []
      histTick.value++
      sel.value = -1
      await revalidate()
      mediaUrl.value = ''
      if (props.video) mediaUrl.value = (await api.playerSource(props.video)).url
    } catch (e) {
      ElMessage.error(errorText(e))
      emit('update:modelValue', false)
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)

let timer = 0
function revalidateSoon() {
  clearTimeout(timer)
  timer = window.setTimeout(revalidate, 300)
}
async function revalidate() {
  try {
    issues.value = await api.subeditValidate(cues.value)
  } catch {
    issues.value = []
  }
}
onBeforeUnmount(() => clearTimeout(timer))

/** 所有改动都走这里：先记撤销点再应用 */
function apply(next: ECue[]) {
  hist.push(cues.value)
  cues.value = next
  histTick.value++
  revalidateSoon()
}
function undo() {
  const p = hist.undo(cues.value)
  if (p) (cues.value = p, histTick.value++, revalidateSoon())
}
function redo() {
  const p = hist.redo(cues.value)
  if (p) (cues.value = p, histTick.value++, revalidateSoon())
}

// 文字 / 时间输入框：一次编辑（失焦）记一个撤销点
let pending = false
function beforeEdit() {
  if (!pending) {
    hist.push(cues.value)
    histTick.value++
    pending = true
  }
}
function afterEdit() {
  pending = false
  revalidateSoon()
}
function setLine(i: number, k: number, v: string) {
  beforeEdit()
  cues.value[i].lines[k] = v
}
function setTime(i: number, key: 'startMs' | 'endMs', v: string) {
  const ms = parseTime(v)
  if (ms === null) return ElMessage.warning('时间写法不对，例如 1:23.450')
  beforeEdit()
  cues.value[i][key] = ms
  afterEdit()
}
function parseTime(s: string): number | null {
  const m = /^(?:(\d+):)?(\d{1,2}):(\d{1,2})(?:[.,](\d{1,3}))?$/.exec(s.trim()) ?? /^(\d{1,2}):(\d{1,2})(?:[.,](\d{1,3}))?$/.exec(`0:${s.trim()}`)
  if (!m) return null
  const [h, mi, se, fr] = m.length === 5 ? [m[1] ?? '0', m[2], m[3], m[4]] : ['0', m[1], m[2], m[3]]
  return ((+h * 60 + +mi) * 60 + +se) * 1000 + (fr ? Math.round(+`0.${fr}` * 1000) : 0)
}
const fmt = (ms: number) => {
  const t = formatClock(ms)
  return `${t}.${String(ms % 1000).padStart(3, '0')}`
}

function doSplit(i: number) {
  const c = cues.value[i]
  const t = videoEl.value && now.value * 1000 > c.startMs && now.value * 1000 < c.endMs ? Math.round(now.value * 1000) : undefined
  const len = c.lines.join(' ').length
  const next = S.splitCue(cues.value, i, Math.round(len / 2), t)
  if (next === cues.value) return ElMessage.warning('这一条太短，没法再拆。')
  apply(next)
}
function doMerge(i: number) {
  if (i >= cues.value.length - 1) return
  apply(S.mergeNext(cues.value, i))
}
function doReplace(onlySelected = false) {
  const r = S.replaceAll(cues.value, find.value, repl.value, opt.value, onlySelected && sel.value >= 0 ? [sel.value] : undefined)
  if (!r.count) return ElMessage.info('没有可替换的内容。')
  apply(r.cues)
  ElMessage.success(`已替换 ${r.count} 处`)
}
function doShift() {
  if (!shiftMs.value) return
  apply(S.shiftAll(cues.value, Math.round(shiftMs.value * 1000)))
  shiftMs.value = 0
}
function nextMatch() {
  if (!matches.value.length) return
  const n = matches.value.find((i) => i > sel.value) ?? matches.value[0]
  select(n)
}

function select(i: number) {
  sel.value = i
  const v = videoEl.value
  if (v) v.currentTime = cues.value[i].startMs / 1000
  nextTick(() => listEl.value?.querySelector<HTMLElement>(`[data-i="${i}"]`)?.scrollIntoView({ block: 'nearest' }))
}
function onTime() {
  const v = videoEl.value
  if (!v) return
  now.value = v.currentTime
  if (loop.value && sel.value >= 0) {
    const c = cues.value[sel.value]
    if (c && v.currentTime * 1000 >= c.endMs) v.currentTime = c.startMs / 1000
  }
}

async function save(overwrite: boolean) {
  const errs = issues.value.filter((i) => i.kind === 'reversed' || i.kind === 'empty')
  if (errs.length) return ElMessage.warning(`有 ${errs.length} 条时间颠倒或没有文字，先改掉再保存。`)
  saving.value = true
  try {
    const r = await api.subeditSave(props.path, cues.value, overwrite)
    original.value = JSON.stringify(cues.value)
    ElMessage.success(overwrite ? `已保存${r.backup ? '（原文件备份为 .bak）' : ''}` : `已另存为 ${r.path.split(/[\\/]/).pop()}`)
    emit('saved', r.path)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    saving.value = false
  }
}
function close() {
  if (dirty.value && !confirm('有未保存的修改，确定关闭吗？')) return
  emit('update:modelValue', false)
}
function onKey(e: KeyboardEvent) {
  if (!props.modelValue) return
  const k = e.key.toLowerCase()
  if ((e.ctrlKey || e.metaKey) && k === 'z') (e.preventDefault(), e.shiftKey ? redo() : undo())
  else if ((e.ctrlKey || e.metaKey) && k === 'y') (e.preventDefault(), redo())
  else if ((e.ctrlKey || e.metaKey) && k === 's') (e.preventDefault(), save(false))
}
window.addEventListener('keydown', onKey)
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
const shown = (c: ECue) => S.viewLines(S.textOf(c), view.value)
</script>

<template>
  <el-dialog :model-value="modelValue" title="字幕校对" width="min(1080px, 96vw)" top="3vh" :close-on-click-modal="false" @update:model-value="(v: boolean) => (v ? null : close())" @close="close">
    <div v-loading="loading" class="ed">
      <div class="bar">
        <el-button size="small" :disabled="!canUndo" @click="undo">撤销</el-button>
        <el-button size="small" :disabled="!canRedo" @click="redo">重做</el-button>
        <span class="sep" />
        <el-input v-model="find" size="small" class="fi" placeholder="查找" clearable @keydown.enter="nextMatch" />
        <el-input v-model="repl" size="small" class="fi" placeholder="替换为" clearable />
        <el-checkbox v-model="opt.matchCase" size="small" label="区分大小写" />
        <el-checkbox v-model="opt.regex" size="small" label="正则" />
        <el-button size="small" :disabled="!matches.length" @click="nextMatch">下一个（{{ matches.length }}）</el-button>
        <el-button size="small" :disabled="!matches.length" @click="doReplace(true)">替换选中条</el-button>
        <el-button size="small" :disabled="!matches.length" @click="doReplace()">全部替换</el-button>
      </div>
      <div class="bar">
        <span class="mute">整体平移（秒）</span>
        <el-input-number v-model="shiftMs" size="small" :step="0.1" :precision="2" controls-position="right" />
        <el-button size="small" :disabled="!shiftMs" @click="doShift">应用</el-button>
        <el-button size="small" :disabled="!issues.some((i) => i.kind === 'overlap')" @click="apply(S.fixOverlaps(cues))">修复重叠</el-button>
        <template v-if="bilingual">
          <span class="sep" />
          <span class="mute">显示</span>
          <el-radio-group v-model="view" size="small">
            <el-radio-button value="both">双语</el-radio-button>
            <el-radio-button value="first">第一行</el-radio-button>
            <el-radio-button value="second">第二行</el-radio-button>
          </el-radio-group>
        </template>
        <span class="grow" />
        <span class="mute">共 {{ cues.length }} 条，{{ issues.length }} 处提示</span>
      </div>
      <div class="body">
        <div v-if="mediaUrl" class="pv">
          <video ref="videoEl" :src="mediaUrl" controls preload="metadata" @timeupdate="onTime" />
          <div v-if="current >= 0" class="live">{{ shown(cues[current]) }}</div>
          <el-checkbox v-model="loop" size="small" label="循环播放选中的这一条" />
        </div>
        <div ref="listEl" class="list" role="table" aria-label="字幕条目">
          <div v-for="(c, i) in cues" :key="i" :data-i="i" class="row" :class="{ sel: i === sel, now: i === current, bad: issueMap.has(i), hit: matches.includes(i) && find }" @click="select(i)">
            <span class="no mono">{{ i + 1 }}</span>
            <div class="times">
              <el-input :model-value="fmt(c.startMs)" size="small" class="mono tm" @change="(v: string) => setTime(i, 'startMs', v)" />
              <el-input :model-value="fmt(c.endMs)" size="small" class="mono tm" @change="(v: string) => setTime(i, 'endMs', v)" />
            </div>
            <div class="txt">
              <el-input
                v-for="(l, k) in c.lines"
                :key="k"
                :model-value="l"
                size="small"
                :class="{ second: k > 0, dim: view === 'first' && k > 0 || view === 'second' && k === 0 }"
                @update:model-value="(v: string) => setLine(i, k, v)"
                @blur="afterEdit"
                @focus="sel = i"
              />
              <div v-if="issueMap.has(i)" class="iss">{{ issueMap.get(i)!.map((x) => x.message).join('；') }}</div>
            </div>
            <div class="ops">
              <el-button size="small" text @click.stop="doSplit(i)">拆分</el-button>
              <el-button size="small" text :disabled="i === cues.length - 1" @click.stop="doMerge(i)">并下一条</el-button>
              <el-button size="small" text @click.stop="apply(S.insertAfter(cues, i))">插入</el-button>
              <el-button size="small" text type="danger" @click.stop="apply(S.removeCue(cues, i))">删除</el-button>
            </div>
          </div>
        </div>
      </div>
    </div>
    <template #footer>
      <span class="mute foot">{{ dirty ? '有未保存的修改' : '没有修改' }} · Ctrl+Z 撤销 · Ctrl+S 另存</span>
      <el-button @click="close">关闭</el-button>
      <el-button :loading="saving" :disabled="!dirty" @click="save(false)">另存为新文件</el-button>
      <el-button type="primary" :loading="saving" :disabled="!dirty" @click="save(true)">保存（覆盖，留 .bak）</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.ed {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 420px;
}
.bar {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.fi {
  width: 150px;
}
.sep {
  width: 1px;
  height: 18px;
  background: var(--cc-line);
}
.grow {
  flex: 1;
}
.body {
  display: flex;
  gap: 10px;
  min-height: 0;
}
.pv {
  width: 340px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.pv video {
  width: 100%;
  background: #000;
  border-radius: 6px;
  max-height: 220px;
}
.live {
  white-space: pre-line;
  text-align: center;
  padding: 6px;
  background: var(--cc-line);
  border-radius: 6px;
  font-size: 13px;
}
.list {
  flex: 1;
  min-width: 0;
  max-height: 58vh;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
}
.row {
  display: grid;
  grid-template-columns: 34px 112px minmax(0, 1fr) auto;
  gap: 8px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--cc-line);
  align-items: start;
}
.row.sel {
  background: color-mix(in srgb, var(--el-color-primary) 10%, transparent);
}
.row.now {
  box-shadow: inset 3px 0 0 var(--el-color-primary);
}
.row.hit {
  background: color-mix(in srgb, var(--el-color-warning) 14%, transparent);
}
.row.bad .no {
  color: var(--cc-err);
}
.no {
  font-size: 12px;
  padding-top: 5px;
  color: var(--cc-mute);
}
.times,
.txt {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.tm {
  font-size: 11px;
}
.second :deep(.el-input__inner) {
  color: var(--cc-mute);
}
.dim {
  opacity: 0.45;
}
.iss {
  font-size: 11px;
  color: var(--cc-err);
}
.ops {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
}
.ops .el-button + .el-button {
  margin-left: 0;
}
.foot {
  margin-right: 12px;
  font-size: 12px;
}
@media (max-width: 860px) {
  .body {
    flex-direction: column;
  }
  .pv {
    width: 100%;
  }
}
</style>
