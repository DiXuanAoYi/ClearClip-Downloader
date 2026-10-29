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
  if (a.id.startsWith('auto-')) return a.label.includes('自动翻译') ? '机器翻译' : '机器生成'
  if (a.quality?.endsWith('.ai')) return 'AI 生成'
  return a.ext.toUpperCase()
}

const modeHint = computed(() => {
  if (!picked.value.length) return ''
  if (props.mode === 'file') return '字幕保存在视频旁边，文件名与视频相同，播放器会自动加载。'
  if (!props.hasVideo) return '内嵌 / 烧录需要同时选择一个视频格式，否则字幕会单独保存为文件。'
  if (props.mode === 'soft') return `字幕写进视频文件里，播放器中可以切换${hasDanmaku.value ? '（MP4 不能内嵌 ASS 弹幕，弹幕会被跳过，MKV 可以）' : ''}。不重新编码，很快。`
  return '字幕直接画进画面，任何播放器都能看到。需要重新编码视频，耗时较长；多种语言会叠在一起，所以只烧录选中的第一条文字字幕（弹幕可以同时烧录）。'
})
</script>

<template>
  <div class="subs">
    <div class="head">
      <span class="label">字幕与弹幕（{{ assets.length }}）</span>
      <span class="ops">
        <el-button v-if="preferred.length" link size="small" type="primary" @click="emit('pick', preferred)">按偏好语言选择</el-button>
        <el-button link size="small" @click="emit('pick', picked.length ? [] : texts.map((a) => a.id))">{{ picked.length ? '全不选' : '全选字幕' }}</el-button>
      </span>
    </div>
    <div class="opts">
      <button
        v-for="a in texts"
        :key="a.id"
        type="button"
        class="opt"
        :class="{ on: selected.has(a.id) }"
        :aria-pressed="selected.has(a.id)"
        @click="emit('toggle', a.id)"
      >
        <span class="ellipsis" :title="title(a)">{{ title(a) }}</span>
        <small class="mono">{{ note(a) }}</small>
      </button>
      <button v-for="a in danmaku" :key="a.id" type="button" class="opt" :class="{ on: selected.has(a.id) }" :aria-pressed="selected.has(a.id)" @click="emit('toggle', a.id)">
        <span>{{ title(a) }}</span>
        <small class="mono">{{ note(a) }}</small>
      </button>
    </div>
    <div v-if="picked.length" class="mode">
      <span class="mute">已选 {{ picked.length }} 项，处理方式</span>
      <el-radio-group :model-value="mode" size="small" @update:model-value="(v: unknown) => emit('update:mode', v as SubMode)">
        <el-radio-button value="file">单独保存为文件</el-radio-button>
        <el-radio-button value="soft">内嵌到视频</el-radio-button>
        <el-radio-button value="burn">烧录进画面</el-radio-button>
      </el-radio-group>
      <small class="mute">{{ modeHint }}</small>
    </div>
    <small v-if="pickedText === 0 && hasDanmaku && mode !== 'file'" class="mute">只选了弹幕：烧录后弹幕直接出现在画面里。</small>
  </div>
</template>

<style scoped>
.subs {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.label {
  font-size: 11.5px;
  color: var(--cc-mute);
}
.ops :deep(.el-button) {
  margin-left: 8px;
}
.opts {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
  max-height: 168px;
  overflow: auto;
  padding: 2px;
}
.opt {
  all: unset;
  cursor: pointer;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 6px 10px;
  display: flex;
  flex-direction: column;
  font-size: 12.5px;
  min-width: 0;
}
.opt small {
  color: var(--cc-mute);
  font-size: 10.5px;
}
.opt.on {
  border-color: var(--cc-acc);
  background: var(--cc-acc-soft);
}
.opt:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.mode {
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: flex-start;
  margin-top: 2px;
}
.mode small {
  font-size: 11.5px;
}
</style>
