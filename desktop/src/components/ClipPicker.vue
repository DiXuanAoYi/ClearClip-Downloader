<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { Chapter, Clip } from '../types'
import { formatDuration } from '../utils/format'
import { formatClock, parseClock } from '../utils/time'

const props = defineProps<{
  durationMs: number | null
  chapters: Chapter[]
  /** 当前裁剪（null 表示下载完整视频） */
  clip: Clip | null
  split: boolean
}>()
const emit = defineEmits<{
  'update:clip': [clip: Clip | null]
  'update:split': [on: boolean]
}>()

/** 完整 / 只下载一段 / 选一章 */
const mode = ref<'full' | 'range' | 'chapter'>('full')
const startText = ref('')
const endText = ref('')
const precise = ref(false)
const error = ref('')
const chapterAt = ref(-1)

const total = computed(() => props.durationMs ?? 0)
const open = computed(() => mode.value !== 'full')

function apply() {
  const start = startText.value.trim() ? parseClock(startText.value) : 0
  const end = endText.value.trim() ? parseClock(endText.value) : null
  if (start === null || (endText.value.trim() && end === null)) {
    error.value = '时间格式不对，可以写 83、1:23 或 01:02:03。'
    emit('update:clip', null)
    return
  }
  if (end !== null && end <= start) {
    error.value = '结束时间要大于开始时间。'
    emit('update:clip', null)
    return
  }
  if (total.value && start >= total.value) {
    error.value = `开始时间超过了视频长度（${formatDuration(total.value)}）。`
    emit('update:clip', null)
    return
  }
  error.value = ''
  if (start === 0 && end === null) emit('update:clip', null)
  else emit('update:clip', { startMs: start, endMs: end, precise: precise.value })
}

watch([startText, endText, precise], () => open.value && apply())
watch(mode, (m) => {
  if (m === 'full') {
    error.value = ''
    chapterAt.value = -1
    emit('update:clip', null)
  } else if (m === 'chapter') {
    if (chapterAt.value < 0 && props.chapters.length) useChapter(props.chapters[0], 0)
  } else {
    chapterAt.value = -1
    apply()
  }
})
// 外部清空（换了一个作品）时同步
watch(
  () => props.clip,
  (c) => {
    if (c === null && mode.value === 'full') {
      startText.value = ''
      endText.value = ''
    }
  },
)
watch(
  () => props.chapters,
  () => {
    mode.value = 'full'
    chapterAt.value = -1
  },
)

function useChapter(c: Chapter, i: number) {
  chapterAt.value = i
  if (mode.value !== 'chapter') mode.value = 'chapter'
  startText.value = formatClock(c.startMs)
  endText.value = formatClock(c.endMs)
}

/** 章节色条：每段按时长占宽度 */
const bars = computed(() => {
  const all = props.chapters.reduce((n, c) => n + Math.max(0, c.endMs - c.startMs), 0) || 1
  return props.chapters.map((c, i) => ({ c, i, grow: Math.max(0.5, ((c.endMs - c.startMs) / all) * 100) }))
})

const summary = computed(() => {
  if (!props.clip) return ''
  const end = props.clip.endMs ?? total.value
  const len = end ? end - props.clip.startMs : 0
  return len > 0 ? `将保留 ${formatClock(props.clip.startMs)} – ${props.clip.endMs ? formatClock(props.clip.endMs) : '结尾'}，约 ${formatDuration(len)}` : ''
})
</script>

<template>
  <div class="clip">
    <div class="pz-seg" role="radiogroup" aria-label="下载范围">
      <button type="button" role="radio" :aria-checked="mode === 'full'" :class="{ on: mode === 'full' }" @click="mode = 'full'">完整视频</button>
      <button type="button" role="radio" :aria-checked="mode === 'range'" :class="{ on: mode === 'range' }" @click="mode = 'range'">只下载一段</button>
      <button v-if="chapters.length" type="button" role="radio" :aria-checked="mode === 'chapter'" :class="{ on: mode === 'chapter' }" @click="mode = 'chapter'">选章节</button>
    </div>

    <div v-if="mode === 'range'" class="line">
      <label class="t"><span class="sr">开始</span><el-input v-model="startText" placeholder="开始 0:00" /></label>
      <span class="mute">至</span>
      <label class="t"><span class="sr">结束</span><el-input v-model="endText" placeholder="结束（留空到结尾）" /></label>
      <el-checkbox v-model="precise">精确剪切（重新编码，较慢）</el-checkbox>
    </div>

    <div v-if="chapters.length" class="bar" :aria-label="`章节（${chapters.length}）`">
      <button
        v-for="b in bars"
        :key="b.i"
        type="button"
        class="seg"
        :class="{ on: mode === 'chapter' && chapterAt === b.i, alt: b.i % 2 === 1 }"
        :style="{ flexGrow: b.grow }"
        :title="`${b.c.title || `第 ${b.i + 1} 章`} · ${formatClock(b.c.startMs)} – ${formatClock(b.c.endMs)}（点一下只下载这一章）`"
        @click="useChapter(b.c, b.i)"
      >
        <span class="ellipsis">{{ b.c.title || `第 ${b.i + 1} 章` }}<template v-if="b.grow > 14"> · {{ formatDuration(b.c.endMs - b.c.startMs) }}</template></span>
      </button>
    </div>
    <div v-if="mode === 'chapter'" class="chapters" role="radiogroup" aria-label="选一章">
      <button v-for="(c, i) in chapters" :key="i" type="button" role="radio" :aria-checked="chapterAt === i" class="chap" :class="{ on: chapterAt === i }" @click="useChapter(c, i)">
        <span class="dot" />
        <span class="mono mute">{{ formatClock(c.startMs) }}</span>
        <span class="ellipsis grow" :title="c.title">{{ c.title || `第 ${i + 1} 章` }}</span>
        <span class="mono mute">{{ formatDuration(c.endMs - c.startMs) }}</span>
      </button>
    </div>

    <small v-if="open && (error || summary)" :class="error ? 'err' : 'pz-note'">{{ error || summary }}</small>
    <small v-if="mode === 'range' && !precise && !error" class="pz-note">默认不重新编码，速度快，开头会落在前一个关键帧上，可能比设置的时间早几秒。</small>
    <el-checkbox v-if="chapters.length" :model-value="split" @update:model-value="(v: unknown) => emit('update:split', !!v)">同时把每个章节另存为单独的文件</el-checkbox>
  </div>
</template>

<style scoped>
.clip {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
}
.line {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 10px;
}
.t {
  width: 150px;
}
.sr {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
}
.bar {
  display: flex;
  gap: 2px;
  width: 100%;
  height: 28px;
}
.seg {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  align-items: center;
  padding: 0 8px;
  border: 0;
  background: var(--cc-fill);
  color: var(--cc-fg2);
  font: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.seg.alt {
  background: var(--cc-line-soft);
}
.seg:first-child {
  border-radius: 4px 0 0 4px;
}
.seg:last-child {
  border-radius: 0 4px 4px 0;
}
.seg:hover {
  color: var(--cc-fg);
}
.seg.on {
  background: var(--cc-acc-soft);
  color: var(--cc-acc-text);
  box-shadow: inset 0 0 0 1px var(--cc-acc);
}
.chapters {
  width: 100%;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--cc-line-soft);
  border-radius: 8px;
  max-height: 240px;
  overflow: auto;
}
.chap {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 40px;
  padding: 0 14px;
  border: 0;
  border-bottom: 1px solid var(--cc-line-soft);
  background: transparent;
  font: inherit;
  color: inherit;
  text-align: left;
  cursor: pointer;
}
.chap:last-child {
  border-bottom: 0;
}
.chap:hover {
  background: var(--cc-hover);
}
.chap.on {
  background: var(--cc-acc-soft);
}
.dot {
  flex: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1.5px solid var(--cc-dot);
  box-sizing: border-box;
}
.chap.on .dot {
  border: 4px solid var(--cc-acc-strong);
}
.grow {
  flex: 1;
}
.err {
  color: var(--cc-err);
  font-size: 12px;
}
</style>
