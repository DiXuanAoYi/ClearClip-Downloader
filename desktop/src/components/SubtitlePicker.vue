<script setup lang="ts">
import { computed } from 'vue'
import { useAppStore } from '../stores/app'
import type { Asset } from '../types'

export type SubMode = 'file' | 'soft' | 'burn'

const props = defineProps<{
  assets: Asset[]
  selected: Set<string>
  mode: SubMode
  /** 已选了视频格式（内嵌 / 烧录需要同时下载视频） */
  hasVideo: boolean
  /** 按偏好语言应选中的字幕 */
  preferred: string[]
  /** 不能选（例如“只要音频”时），附带原因 */
  disabled?: string
}>()
const emit = defineEmits<{
  toggle: [id: string]
  'update:mode': [mode: SubMode]
  pick: [ids: string[]]
}>()

const app = useAppStore()
const texts = computed(() => props.assets.filter((a) => a.ext !== 'xml'))
const danmaku = computed(() => props.assets.filter((a) => a.ext === 'xml'))
const picked = computed(() => props.assets.filter((a) => props.selected.has(a.id)))
const pickedText = computed(() => texts.value.filter((a) => props.selected.has(a.id)).length)
const hasDanmaku = computed(() => danmaku.value.some((a) => props.selected.has(a.id)))

function title(a: Asset) {
  return a.label.replace(/^字幕 · /, '')
}

function note(a: Asset) {
  if (a.ext === 'xml') return app.settings?.danmakuAss === false ? 'XML 弹幕' : '弹幕 · 转为 ASS'
  if (a.id.startsWith('auto-')) return a.label.includes('自动翻译') ? '自动翻译' : '自动生成'
  if (a.quality?.endsWith('.ai')) return 'AI 生成'
  return a.ext.toUpperCase()
}
const MODES: { v: SubMode; label: string }[] = [
  { v: 'file', label: '单独文件' },
  { v: 'soft', label: '内嵌到视频' },
  { v: 'burn', label: '烧录进画面' },
]

const modeHint = computed(() => {
  if (!picked.value.length) return ''
  if (props.mode === 'file') return '字幕保存在视频旁边，文件名与视频相同，播放器会自动加载。'
  if (!props.hasVideo) return '内嵌 / 烧录需要同时选择一个视频格式，否则字幕会单独保存为文件。'
  if (props.mode === 'soft') return `字幕写进视频文件里，播放器中可以切换${hasDanmaku.value ? '（MP4 不能内嵌 ASS 弹幕，弹幕会被跳过，MKV 可以）' : ''}。不重新编码，很快。`
  return '字幕直接画进画面，任何播放器都能看到。需要重新编码视频，耗时较长；多种语言会叠在一起，所以只烧录选中的第一条文字字幕（弹幕可以同时烧录）。'
})
</script>

<template>
  <div class="subs" :class="{ off: !!disabled }">
    <div class="pz-sh">
      <span class="pz-st">{{ danmaku.length && texts.length ? '字幕与弹幕' : danmaku.length ? '弹幕' : '字幕' }}<small>已选 {{ picked.length }} / {{ assets.length }}</small></span>
      <span v-if="!disabled" class="pz-sa">
        <button v-if="preferred.length" type="button" class="pz-link" @click="emit('pick', preferred)">按偏好语言选</button>
        <button type="button" class="pz-link" @click="emit('pick', picked.length ? [] : texts.map((a) => a.id))">{{ picked.length ? '全不选' : '全选字幕' }}</button>
      </span>
    </div>
    <p v-if="disabled" class="pz-note">{{ disabled }}</p>
    <div class="chips">
      <button
        v-for="a in [...texts, ...danmaku]"
        :key="a.id"
        type="button"
        class="pz-chip"
        :class="{ on: selected.has(a.id) }"
        :aria-pressed="selected.has(a.id)"
        :disabled="!!disabled"
        :title="title(a)"
        @click="emit('toggle', a.id)"
      >
        <span class="pz-box" />
        <span class="ellipsis">{{ title(a) }}</span>
        <span class="pz-tag">{{ note(a) }}</span>
      </button>
    </div>
    <div v-if="picked.length && !disabled" class="mode">
      <span class="ml">处理方式</span>
      <div class="pz-seg" role="radiogroup" aria-label="字幕处理方式">
        <button v-for="m in MODES" :key="m.v" type="button" role="radio" :aria-checked="mode === m.v" :class="{ on: mode === m.v }" @click="emit('update:mode', m.v)">{{ m.label }}</button>
      </div>
      <span class="pz-note">{{ modeHint }}</span>
    </div>
    <small v-if="pickedText === 0 && hasDanmaku && mode !== 'file' && !disabled" class="pz-note">只选了弹幕：烧录后弹幕直接出现在画面里。</small>
  </div>
</template>

<style scoped>
.subs {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.chips .pz-chip {
  max-width: 320px;
}
.mode {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
}
.ml {
  width: 56px;
  font-size: 12px;
  color: var(--cc-fg2);
}
.mode .pz-note {
  flex: 1 1 240px;
}
p.pz-note {
  margin: 0;
}
</style>
