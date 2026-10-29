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

const open = ref(false)
const startText = ref('')
const endText = ref('')
const precise = ref(false)
const error = ref('')

const total = computed(() => props.durationMs ?? 0)

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
watch(open, (on) => {
  if (on) apply()
  else {
    error.value = ''
    emit('update:clip', null)
  }
})
// 外部清空（换了一个作品）时同步
watch(
  () => props.clip,
  (c) => {
    if (c === null && !open.value) {
      startText.value = ''
      endText.value = ''
    }
  },
)

function useChapter(c: Chapter) {
  open.value = true
  startText.value = formatClock(c.startMs)
  endText.value = formatClock(c.endMs)
}

const summary = computed(() => {
  if (!props.clip) return ''
  const end = props.clip.endMs ?? total.value
  const len = end ? end - props.clip.startMs : 0
  return len > 0 ? `将保留 ${formatClock(props.clip.startMs)} – ${props.clip.endMs ? formatClock(props.clip.endMs) : '结尾'}，约 ${formatDuration(len)}` : ''
})
</script>

<template>
  <div class="clip">
    <div class="line">
      <el-checkbox v-model="open">只下载一段</el-checkbox>
      <template v-if="open">
        <el-input v-model="startText" size="small" class="t" placeholder="开始 0:00" />
        <span class="mute">至</span>
        <el-input v-model="endText" size="small" class="t" placeholder="结束（留空到结尾）" />
        <el-checkbox v-model="precise" size="small">精确剪切（重新编码，较慢）</el-checkbox>
      </template>
    </div>
    <small v-if="open && (error || summary)" :class="error ? 'err' : 'mute'">{{ error || summary }}</small>
    <small v-if="open && !precise && !error" class="mute">默认不重新编码，速度快，开头会落在前一个关键帧上，可能比设置的时间早几秒。</small>

    <template v-if="chapters.length">
      <div class="chead">
        <span class="mute">章节（{{ chapters.length }}）</span>
        <el-checkbox :model-value="split" size="small" @update:model-value="(v: unknown) => emit('update:split', !!v)">同时把每个章节另存为单独的文件</el-checkbox>
      </div>
      <div class="chapters">
        <div v-for="(c, i) in chapters" :key="i" class="chap">
          <span class="mono mute">{{ formatClock(c.startMs) }}</span>
          <span class="ellipsis grow" :title="c.title">{{ c.title || `第 ${i + 1} 章` }}</span>
          <small class="mono mute">{{ formatDuration(c.endMs - c.startMs) }}</small>
          <el-button link size="small" type="primary" @click="useChapter(c)">只下载这一章</el-button>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.clip {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.line {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.t {
  width: 150px;
}
.err {
  color: var(--cc-err);
}
small {
  font-size: 11.5px;
}
.chead {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: 4px;
}
.chapters {
  max-height: 180px;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
}
.chap {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 10px;
  border-top: 1px dashed var(--cc-line);
  font-size: 12.5px;
}
.chap:first-child {
  border-top: 0;
}
.grow {
  flex: 1;
  min-width: 0;
}
</style>
