<script setup lang="ts">
// “文稿”页：把所选片段的语音转成带时间的文字稿。在文稿里选中词删除，对应的画面和声音就被剪掉；
// 一键找出口头禅；按词生成字幕（样式模板、逐字高亮）。识别用“设置 → AI”里配置的语音转文字。
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { ElMessage } from 'element-plus'
import { api, errorText, events } from '../../../api'
import type { EditApi } from '../../../composables/useEditProject'
import type { EditText, SttStatus, Transcript, TranscriptWord } from '../../../types'
import { CAPTION_STYLES, DEFAULT_FILLERS, captionsFromWords, clamp, findFillers, msText, outputSize, parseFillers, sentencesOf, wordSpans, wordsInClip } from '../../../utils/edit'

const props = defineProps<{ ed: EditApi }>()
const emit = defineEmits<{ goto: [ms: number] }>()
const ed = props.ed

const clip = computed(() => (ed.selectedClip.value?.kind === 'video' ? ed.selectedClip.value : undefined))
const placed = computed(() => {
  const i = clip.value ? ed.project.value.clips.findIndex((c) => c.id === clip.value?.id) : -1
  return i >= 0 ? ed.placed.value[i] : undefined
})
const src = computed(() => (clip.value ? ed.sources[clip.value.path] : undefined))
const hasAudio = computed(() => !!src.value?.hasAudio)

// ---------- 状态与识别 ----------

const stt = ref<SttStatus | null>(null)
/** 每个素材已经识别过的结果（素材时间）。片段分割、裁剪之后仍然能用 */
const store = reactive<Record<string, Transcript[]>>({})
const job = reactive({ running: false, id: 0, percent: 0 })
const lang = ref('default')
const LANGS: { v: string; t: string }[] = [
  { v: 'default', t: '按设置' },
  { v: 'auto', t: '自动识别' },
  { v: 'zh', t: '中文' },
  { v: 'en', t: 'English' },
  { v: 'ja', t: '日本語' },
]
let seq = (Math.floor(Date.now() / 1000) % 1_000_000) * 1000 + 700
let unlisten: UnlistenFn | undefined
onMounted(async () => {
  stt.value = await api.editSttStatus().catch(() => null)
  unlisten = await events.onTrackProgress((p) => {
    if (job.running && p.id === job.id) job.percent = clamp(p.percent, 0, 100)
  })
})
onBeforeUnmount(() => {
  unlisten?.()
  if (job.running) api.trackCancel(job.id).catch(() => undefined)
})

/** 能覆盖当前片段的识别结果 */
const transcript = computed<Transcript | undefined>(() => {
  const c = clip.value
  if (!c) return undefined
  return (store[c.path] ?? []).find((t) => t.fromMs <= c.inMs + 50 && t.toMs >= c.outMs - 50)
})
const words = computed<TranscriptWord[]>(() => transcript.value?.words ?? [])
/** 属于当前片段的词（下标指向 words） */
const inClip = computed(() => (clip.value ? wordsInClip(words.value, clip.value) : []))
const sentences = computed(() => {
  const idx = inClip.value
  if (!idx.length) return []
  return sentencesOf(idx.map((i) => words.value[i])).map((s) => ({ ...s, from: idx[s.from], to: idx[s.to] }))
})

watch(
  () => clip.value?.id,
  () => {
    picked.value = new Set()
    anchor = -1
  },
)

async function transcribe(force = false) {
  const c = clip.value
  if (!c || job.running) return
  if (!stt.value?.ready) return ElMessage.warning(stt.value?.reason || '还没有配置语音转文字。')
  job.id = ++seq
  job.percent = 0
  job.running = true
  try {
    const t = await api.editTranscribe({ id: job.id, path: c.path, fromMs: c.inMs, toMs: c.outMs, language: lang.value === 'default' ? null : lang.value === 'auto' ? '' : lang.value, force })
    store[c.path] = [t, ...(store[c.path] ?? []).filter((x) => !(x.fromMs <= t.toMs && x.toMs >= t.fromMs))]
    picked.value = new Set()
    ElMessage.success(`识别出 ${t.words.length} 个词。`)
  } catch (e) {
    const msg = errorText(e)
    if (msg === 'canceled') ElMessage.info('已取消。')
    else ElMessage.error(msg)
  } finally {
    job.running = false
  }
}
async function cancel() {
  if (job.running) await api.trackCancel(job.id).catch(() => undefined)
}

// ---------- 选词与删除 ----------

const picked = ref<Set<number>>(new Set())
let anchor = -1
const pickedCount = computed(() => picked.value.size)

function timelineAt(ms: number): number {
  const c = clip.value
  const pl = placed.value
  if (!c || !pl) return 0
  return clamp(pl.startMs + (ms - c.inMs) / Math.max(0.01, c.speed), pl.startMs, pl.endMs)
}

function clickWord(i: number, e: MouseEvent) {
  if (e.shiftKey && anchor >= 0) {
    const [a, b] = anchor < i ? [anchor, i] : [i, anchor]
    const next = new Set(picked.value)
    for (const k of inClip.value) if (k >= a && k <= b) next.add(k)
    picked.value = next
  } else if (e.ctrlKey || e.metaKey) {
    const next = new Set(picked.value)
    if (!next.delete(i)) next.add(i)
    picked.value = next
    anchor = i
  } else {
    emit('goto', timelineAt(words.value[i].startMs))
    anchor = i
  }
}
function pickSentence(s: { from: number; to: number }) {
  const next = new Set(picked.value)
  const all = inClip.value.filter((k) => k >= s.from && k <= s.to)
  const every = all.every((k) => next.has(k))
  for (const k of all) every ? next.delete(k) : next.add(k)
  picked.value = next
}

const picking = computed(() => {
  const spans = wordSpans(words.value, picked.value)
  return { spans, ms: spans.reduce((n, s) => n + (s.endMs - s.startMs), 0) }
})

function deletePicked() {
  const c = clip.value
  if (!c || !picked.value.size) return
  const removed = ed.removeClipSilences(c.id, picking.value.spans, 100)
  if (removed > 0) {
    ElMessage.success(`已剪掉 ${msText(removed)}，后面的内容已前移。`)
    picked.value = new Set()
  } else {
    ElMessage.warning('没有可剪的内容（选中的词可能已经不在这个片段里，或整段都被选中了）。')
  }
}

// ---------- 口头禅 ----------

const FILLER_KEY = 'cc.edit.fillers'
function loadFillers(): string {
  try {
    return localStorage.getItem(FILLER_KEY) || DEFAULT_FILLERS
  } catch {
    return DEFAULT_FILLERS
  }
}
const fillers = ref(loadFillers())
watch(fillers, (v) => {
  try {
    localStorage.setItem(FILLER_KEY, v)
  } catch {
    /* 存不了就算了 */
  }
})
const fillerNote = ref('')
function markFillers() {
  const hit = findFillers(words.value, parseFillers(fillers.value)).filter((i) => inClip.value.includes(i))
  picked.value = new Set(hit)
  fillerNote.value = hit.length ? `找到 ${hit.length} 个词。检查一下，不想剪的按住 Ctrl（Mac 用 ⌘）点一下取消，然后点“剪掉选中的词”。` : '没有找到口头禅。'
}

// ---------- 当前播放的词 ----------

const activeIdx = computed(() => {
  const c = clip.value
  const pl = placed.value
  if (!c || !pl) return -1
  const t = ed.playhead.value
  if (t < pl.startMs || t >= pl.endMs) return -1
  const srcMs = c.inMs + (t - pl.startMs) * c.speed
  return inClip.value.find((i) => srcMs >= words.value[i].startMs && srcMs < words.value[i].endMs) ?? -1
})

// ---------- 字幕 ----------

const cap = reactive({ style: 'hl', clear: false })
const bi = reactive({ on: false, lang: 'English' })
const BI_LANGS = ['English', '中文', '日本語', '한국어', 'Français', 'Deutsch', 'Español']
const styleOf = computed(() => CAPTION_STYLES.find((s) => s.id === cap.style) ?? CAPTION_STYLES[0])
const making = ref(false)
async function makeCaptions() {
  const c = clip.value
  const pl = placed.value
  if (!c || !pl || !words.value.length || making.value) return
  const frame = outputSize(ed.project.value, ed.sources)
  const proto = captionsFromWords(words.value, c, pl, styleOf.value, frame, () => 0)
  if (!proto.length) return ElMessage.warning('没有生成字幕（这个片段里没有识别出的词）。')
  // 双语：把每条字幕交给“设置 → AI”里的对话模型翻译，译文作为小一号的第二行放在下面
  let tr: (string | null)[] = []
  if (bi.on) {
    making.value = true
    try {
      tr = await api.editTranslateLines(proto.map((t) => t.text), bi.lang)
    } catch (e) {
      making.value = false
      return ElMessage.error(errorText(e))
    }
    making.value = false
  }
  const replace = cap.clear ? ed.project.value.texts.filter((t) => t.startMs < pl.endMs && t.endMs > pl.startMs).map((t) => t.id) : []
  const n = ed.addCaptions((ids) => {
    const out: EditText[] = []
    proto.forEach((t, i) => {
      out.push({ ...t, id: ids() })
      const line = tr[i]
      if (line?.trim()) {
        const size = Math.max(2.5, t.size * 0.7)
        out.push({ ...t, id: ids(), text: line.trim(), size, y: Math.min(0.97, t.y + (t.size / 100) * 1.25), words: [], highlight: '', color: '#FFFFFF' })
      }
    })
    return out
  }, replace)
  if (n) ElMessage.success(`已生成 ${n} 条字幕，可以在“文字”页逐条调整。`)
  else ElMessage.warning('没有生成字幕（文字数量已到上限）。')
}
</script>

<template>
  <div class="tr">
    <div v-if="!clip" class="mute small">先在时间线上选中一个视频片段。</div>
    <div v-else-if="!hasAudio" class="mute small">这个素材没有声音，没法生成文稿。</div>
    <template v-else>
      <div class="mute small">{{ src?.name ?? '' }}。生成文稿后，在文稿里选词删除就能剪视频；也可以直接按词生成字幕。</div>
      <div v-if="stt && !stt.ready" class="note small">{{ stt.reason }}</div>

      <div class="row">
        <el-select v-model="lang" size="small" class="lang" :disabled="job.running">
          <el-option v-for="l in LANGS" :key="l.v" :label="l.t" :value="l.v" />
        </el-select>
        <el-button v-if="!job.running" size="small" type="primary" :disabled="!!stt && !stt.ready" @click="transcribe(false)">{{ words.length ? '重新生成' : '生成文稿' }}</el-button>
        <template v-else>
          <el-progress :percentage="job.percent" :stroke-width="6" class="prog" />
          <el-button size="small" @click="cancel">取消</el-button>
        </template>
        <span v-if="transcript" class="mute small">{{ transcript.engine }} · {{ inClip.length }} 个词</span>
      </div>
      <div v-if="transcript && !transcript.wordLevel" class="note small">这次识别只给了整句的时间，词的位置是按字数估计的，剪出来的边界可能不准。想要准确的词级时间，请用 whisper.cpp 或支持词级时间戳的接口。</div>

      <template v-if="inClip.length">
        <h4>文稿</h4>
        <div class="mute small">点一个词跳到那里；Shift + 点选一段，Ctrl / ⌘ + 点增减单个词；“选句”一次选一整句。选好后点“剪掉选中的词”。</div>
        <div class="doc" data-no-i18n>
          <div v-for="s in sentences" :key="s.from" class="sent">
            <span class="time" @click="emit('goto', timelineAt(s.startMs))">{{ msText(timelineAt(s.startMs) - (placed?.startMs ?? 0)) }}</span>
            <span class="ws">
              <span
                v-for="i in inClip.filter((k) => k >= s.from && k <= s.to)"
                :key="i"
                class="w"
                :class="{ on: picked.has(i), now: activeIdx === i }"
                @click="clickWord(i, $event)"
                >{{ words[i].t }}</span
              >
            </span>
            <a class="pick" @click="pickSentence(s)">选句</a>
          </div>
        </div>
        <div class="row">
          <el-button size="small" type="danger" :disabled="!pickedCount" @click="deletePicked">剪掉选中的词（{{ pickedCount }} 个，{{ msText(picking.ms) }}）</el-button>
          <el-button size="small" :disabled="!pickedCount" @click="picked = new Set()">取消选择</el-button>
        </div>

        <h4>口头禅</h4>
        <el-input v-model="fillers" type="textarea" :rows="2" size="small" placeholder="用逗号分隔，例如：嗯,呃,那个,就是,um,uh" />
        <div class="row">
          <el-button size="small" @click="markFillers">找出口头禅</el-button>
          <el-button size="small" text @click="fillers = DEFAULT_FILLERS">恢复默认</el-button>
        </div>
        <div v-if="fillerNote" class="mute small">{{ fillerNote }}</div>
        <div class="mute small">中文每个字是一个词，“那个”就是相邻的两个词。词表是你自己的，“就是”“然后”这类词有时是正常表达，剪之前检查一遍。</div>

        <h4>生成字幕</h4>
        <div class="styles">
          <div v-for="s in CAPTION_STYLES" :key="s.id" class="style" :class="{ on: cap.style === s.id }" @click="cap.style = s.id">
            <b>{{ s.label }}</b>
            <span class="mute small">{{ s.hint }}</span>
          </div>
        </div>
        <el-checkbox v-model="cap.clear">先清掉这个片段范围内已有的文字</el-checkbox>
        <div class="row">
          <el-checkbox v-model="bi.on">同时生成译文（双语）</el-checkbox>
          <el-select v-if="bi.on" v-model="bi.lang" size="small" class="lang">
            <el-option v-for="l in BI_LANGS" :key="l" :label="l" :value="l" />
          </el-select>
        </div>
        <div v-if="bi.on" class="note small">字幕文字会发给“设置 → AI”里配置的对话模型去翻译；译文放在原文下面，字号小一号。</div>
        <div class="row">
          <el-button size="small" type="primary" :loading="making" @click="makeCaptions">按词生成字幕</el-button>
        </div>
        <div class="mute small">字幕就是普通的文字条，生成后可以在“文字”页改字、改样式、挪位置。逐字高亮和淡入淡出的字幕需要 ffmpeg 带 ass 滤镜，导出时才能看到完整效果；实时预览里是近似的。改过字的字幕会按普通文字显示。</div>
      </template>
      <div v-else-if="clip && !job.running" class="mute small">还没有文稿。第一次识别会比较慢；识别过的结果会缓存，同一段素材再次生成很快。</div>
    </template>
  </div>
</template>

<style scoped>
.tr {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
h4 {
  margin: 10px 0 0;
  font-size: 13px;
  padding-top: 8px;
  border-top: 1px solid var(--cc-line, rgba(127, 127, 127, 0.25));
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
.lang {
  width: 104px;
}
.prog {
  flex: 1;
  min-width: 120px;
}
.note {
  color: var(--cc-warn, #b7791f);
}
.doc {
  max-height: 280px;
  overflow: auto;
  border: 1px solid var(--cc-line, rgba(127, 127, 127, 0.25));
  border-radius: 6px;
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sent {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  line-height: 1.7;
}
.time {
  flex: none;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  opacity: 0.55;
  cursor: pointer;
  padding-top: 1px;
}
.ws {
  flex: 1;
  min-width: 0;
  white-space: pre-wrap;
  word-break: break-all;
}
.w {
  cursor: pointer;
  border-radius: 3px;
}
.w:hover {
  background: rgba(127, 127, 127, 0.2);
}
.w.on {
  background: rgba(245, 108, 108, 0.28);
  text-decoration: line-through;
}
.w.now {
  background: rgba(64, 158, 255, 0.3);
}
.pick {
  flex: none;
  font-size: 11px;
  opacity: 0.6;
  cursor: pointer;
}
.pick:hover {
  opacity: 1;
}
.styles {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
.style {
  border: 1px solid var(--cc-line, rgba(127, 127, 127, 0.3));
  border-radius: 6px;
  padding: 6px 8px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.style.on {
  border-color: var(--el-color-primary);
  background: rgba(64, 158, 255, 0.1);
}
.tr :deep(.el-checkbox) {
  height: auto;
  min-height: 24px;
  white-space: normal;
}
</style>
