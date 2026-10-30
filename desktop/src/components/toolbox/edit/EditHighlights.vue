<script setup lang="ts">
// “精彩”页：从一段长视频里找出最值得剪出来的几个片段（口播 / 访谈 / 直播回放 / 会议录像都行）。
// 默认只在本机用声音大小、语速变化、镜头切换的节奏打分；有文稿时片段会对齐到完整的句子。
// 可选“让 AI 挑”：把文稿的文字发给“设置 → AI”里的对话模型，每次都先确认。
import { computed, reactive, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../../../api'
import type { EditApi } from '../../../composables/useEditProject'
import type { EditClip, HighlightCandidate, HighlightSentence } from '../../../types'
import { clipDuration, msText, nextId, sentencesOf } from '../../../utils/edit'
import EditField from './EditField.vue'

const props = defineProps<{ ed: EditApi }>()
const emit = defineEmits<{ goto: [ms: number] }>()
const ed = props.ed

const clip = computed<EditClip | undefined>(() => (ed.selectedClip.value?.kind === 'video' ? ed.selectedClip.value : undefined))
const usable = computed(() => !!clip.value && !clip.value.freeze && !clip.value.reverse)

const opts = reactive({ count: 5, minSecs: 15, maxSecs: 60, useText: false, portrait: false })
const cands = ref<HighlightCandidate[]>([])
const busy = ref('')
const note = ref('')
const viaLlm = ref(false)

watch(
  () => clip.value?.id,
  () => {
    cands.value = []
    note.value = ''
  },
)

/** 取（或读缓存的）文稿，转成带文字的句子（素材时间） */
async function sentences(): Promise<HighlightSentence[]> {
  const c = clip.value!
  const t = await api.editTranscribe({ id: Math.floor(Date.now() / 1000) % 1_000_000_000, path: c.path, fromMs: c.inMs, toMs: c.outMs })
  const ws = t.words.filter((w) => w.endMs > c.inMs && w.startMs < c.outMs)
  return sentencesOf(ws).map((s) => ({
    startMs: s.startMs,
    endMs: s.endMs,
    text: ws
      .slice(s.from, s.to + 1)
      .map((w) => w.t)
      .join(t.wordLevel && /^[\x20-\x7e]/.test(ws[s.from]?.t ?? '') ? ' ' : ''),
  }))
}

async function find(llm: boolean) {
  const c = clip.value
  if (!c || !usable.value || busy.value) return
  if (opts.minSecs > opts.maxSecs) opts.maxSecs = opts.minSecs
  if (llm) {
    try {
      await ElMessageBox.confirm('会把这段视频的文稿文字（只有文字，没有画面和声音）发给“设置 → AI”里配置的对话模型，由它挑出精彩片段。要继续吗？', '让 AI 挑选', { confirmButtonText: '发送并挑选', cancelButtonText: '取消', type: 'warning' })
    } catch {
      return
    }
  }
  busy.value = llm ? 'llm' : 'local'
  note.value = ''
  try {
    const sent = llm || opts.useText ? await sentences() : []
    const req = { path: c.path, fromMs: c.inMs, toMs: c.outMs, sentences: sent, count: opts.count, minSecs: opts.minSecs, maxSecs: opts.maxSecs }
    cands.value = llm ? await api.editHighlightsLlm(req) : await api.editHighlights(req)
    viaLlm.value = llm
    if (!cands.value.length) note.value = '没有找到合适的片段。可以把最短时长调短，或者换一段更长的素材。'
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = ''
  }
}

/** 候选的素材时间 → 时间线位置（用来跳转预览） */
function at(ms: number): number {
  const c = clip.value
  const i = c ? ed.project.value.clips.findIndex((x) => x.id === c.id) : -1
  const pl = i >= 0 ? ed.placed.value[i] : undefined
  if (!c || !pl) return 0
  return Math.round(pl.startMs + (ms - c.inMs) / Math.max(0.01, c.speed))
}

function portraitOut() {
  if (!opts.portrait) return
  ed.mutate((p) => {
    p.out.width = 1080
    p.out.height = 1920
  })
}

/** 把候选作为一个新片段加到主轨末尾，原片段不动 */
function append(k: HighlightCandidate) {
  const c = clip.value
  if (!c) return
  const id = nextId(ed.project.value)
  const copy: EditClip = { ...c, id, inMs: k.startMs, outMs: k.endMs, transition: null, fadeInMs: 0, fadeOutMs: 0, regions: [] }
  ed.mutate((p) => {
    p.clips.push(copy)
  })
  portraitOut()
  ed.sel.value = { kind: 'clip', id }
  ElMessage.success(`已把“${k.title}”（${msText(clipDuration(copy))}）加到主轨末尾。`)
}

/** 把当前片段裁成只剩这一段 */
function keepOnly(k: HighlightCandidate) {
  const c = clip.value
  if (!c) return
  ed.mutate((p) => {
    const t = p.clips.find((x) => x.id === c.id)
    if (t) {
      t.inMs = k.startMs
      t.outMs = k.endMs
      t.regions = []
    }
  })
  portraitOut()
  ElMessage.success('已裁成只剩这一段。')
}

function appendAll() {
  for (const k of cands.value) append(k)
}
</script>

<template>
  <div class="hl">
    <div v-if="!clip" class="mute small">先在时间线上选中一个视频片段。</div>
    <div v-else-if="!usable" class="mute small">倒放 / 定格的片段不能分析，请选别的片段。</div>
    <template v-else>
      <div class="mute small">从这段视频里找出最值得剪出来的几个片段。默认全部在本机算，不联网。</div>
      <EditField label="找几段">
        <el-input-number v-model="opts.count" :min="1" :max="12" size="small" controls-position="right" />
      </EditField>
      <EditField label="每段长度">
        <el-input-number v-model="opts.minSecs" :min="3" :max="600" size="small" controls-position="right" />
        <span>～</span>
        <el-input-number v-model="opts.maxSecs" :min="3" :max="600" size="small" controls-position="right" />
        <span>秒</span>
      </EditField>
      <el-checkbox v-model="opts.useText" size="small">结合文稿（片段会对齐到完整的句子；没有文稿会先识别）</el-checkbox>
      <el-checkbox v-model="opts.portrait" size="small">加入时间线时把输出改成竖屏 9:16</el-checkbox>
      <div class="row">
        <el-button type="primary" size="small" :loading="busy === 'local'" :disabled="!!busy" @click="find(false)">找精彩片段</el-button>
        <el-button size="small" :loading="busy === 'llm'" :disabled="!!busy" title="把文稿文字发给配置的对话模型" @click="find(true)">让 AI 挑选…</el-button>
      </div>
      <div v-if="note" class="mute small">{{ note }}</div>

      <template v-if="cands.length">
        <div class="mute small">{{ viaLlm ? '由 AI 根据文稿挑选：' : '按声音、语速、镜头节奏打分：' }}点一行预览，再决定怎么用。</div>
        <div v-for="(k, i) in cands" :key="k.startMs" class="cand" @click="emit('goto', at(k.startMs))">
          <div class="head">
            <b>{{ i + 1 }}. {{ k.title }}</b>
            <span class="score">{{ Math.round(k.score) }} 分</span>
          </div>
          <div class="mute small">{{ msText(k.startMs - (clip?.inMs ?? 0)) }} – {{ msText(k.endMs - (clip?.inMs ?? 0)) }}（{{ msText(k.endMs - k.startMs) }}）</div>
          <div v-if="k.reasons.length" class="mute small">{{ k.reasons.join('、') }}</div>
          <div class="row" @click.stop>
            <el-button size="small" @click="append(k)">加到时间线末尾</el-button>
            <el-button size="small" @click="keepOnly(k)">只留这段</el-button>
          </div>
        </div>
        <el-button size="small" @click="appendAll">全部加到时间线末尾</el-button>
      </template>
    </template>
  </div>
</template>

<style scoped>
.hl {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.small {
  font-size: 11.5px;
}
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}
.row :deep(.el-button + .el-button) {
  margin-left: 0;
}
.cand {
  border: 1px solid var(--cc-line, rgba(127, 127, 127, 0.25));
  border-radius: 6px;
  padding: 6px 8px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.cand:hover {
  border-color: var(--el-color-primary);
}
.head {
  display: flex;
  justify-content: space-between;
  gap: 8px;
}
.score {
  color: var(--el-color-primary);
  white-space: nowrap;
}
</style>
