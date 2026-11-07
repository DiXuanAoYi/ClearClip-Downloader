<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { PlaylistEntry } from '../types'
import { formatDuration } from '../utils/format'
import VirtualList from './VirtualList.vue'

const props = defineProps<{
  entries: PlaylistEntry[]
  selected: Set<string>
  /** 之前已经下载过的条目 */
  downloaded: Set<string>
}>()
const emit = defineEmits<{ 'update:selected': [ids: Set<string>] }>()

const query = ref('')
const hideDone = ref(false)
const from = ref<number | null>(null)
const to = ref<number | null>(null)

const visible = computed(() => {
  const q = query.value.trim().toLowerCase()
  return props.entries.filter((e) => (!q || e.title.toLowerCase().includes(q)) && (!hideDone.value || !props.downloaded.has(e.id)))
})
const doneCount = computed(() => props.entries.filter((e) => props.downloaded.has(e.id)).length)
const selectedVisible = computed(() => visible.value.filter((e) => props.selected.has(e.id)).length)

function set(ids: Iterable<string>) {
  emit('update:selected', new Set(ids))
}

function toggle(id: string) {
  const next = new Set(props.selected)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  set(next)
}

/** 对当前显示的条目操作，不影响被筛选掉的。 */
function selectVisible(on: boolean) {
  const next = new Set(props.selected)
  for (const e of visible.value) {
    if (on) next.add(e.id)
    else next.delete(e.id)
  }
  set(next)
}

function invert() {
  const next = new Set(props.selected)
  for (const e of visible.value) {
    if (next.has(e.id)) next.delete(e.id)
    else next.add(e.id)
  }
  set(next)
}

function onlyNew() {
  set(props.entries.filter((e) => !props.downloaded.has(e.id)).map((e) => e.id))
}

function applyRange() {
  const a = from.value ?? 1
  const b = to.value ?? props.entries.length
  const lo = Math.min(a, b)
  const hi = Math.max(a, b)
  set(props.entries.filter((e) => e.index >= lo && e.index <= hi).map((e) => e.id))
}

// 列表换了就清空筛选
watch(
  () => props.entries,
  () => {
    query.value = ''
    hideDone.value = false
    from.value = null
    to.value = null
  },
)
</script>

<template>
  <div class="picker">
    <div class="pz-sh">
      <span class="pz-st">视频<small>已选 {{ selected.size }} / {{ entries.length }}</small></span>
      <span class="pz-sa">
        <button type="button" class="pz-link" @click="selectVisible(selectedVisible < visible.length)">{{ selectedVisible < visible.length ? '全选' : '全不选' }}</button>
        <button type="button" class="pz-link" @click="invert">反选</button>
        <button v-if="doneCount" type="button" class="pz-link" @click="onlyNew">只选没下载过的</button>
      </span>
    </div>
    <div class="filters">
      <el-input v-model="query" clearable placeholder="搜索标题" class="q">
        <template #prefix>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></svg>
        </template>
      </el-input>
      <span class="range">
        <span>第</span>
        <el-input-number v-model="from" :min="1" :max="entries.length" :controls="false" :value-on-clear="null" class="n" aria-label="从第几个" />
        <span>到</span>
        <el-input-number v-model="to" :min="1" :max="entries.length" :controls="false" :value-on-clear="null" class="n" aria-label="到第几个" />
        <el-button @click="applyRange">选这些</el-button>
      </span>
      <el-checkbox v-if="doneCount" v-model="hideDone">隐藏已下载的</el-checkbox>
    </div>
    <div v-if="!visible.length" class="empty pz-note">没有匹配的条目。</div>
    <VirtualList v-else :items="visible" :item-height="48" :item-key="(e) => e.id" class="list">
      <template #default="{ item: e }">
        <label class="entry" :class="{ on: selected.has(e.id), done: downloaded.has(e.id) }">
          <el-checkbox :model-value="selected.has(e.id)" @change="toggle(e.id)" />
          <span class="mono idx">{{ e.index }}</span>
          <img v-if="e.thumbnail" :src="e.thumbnail" class="thumb" referrerpolicy="no-referrer" loading="lazy" alt="" />
          <span v-else class="thumb" />
          <span class="title ellipsis" :title="e.title">{{ e.title }}</span>
          <span><span v-if="downloaded.has(e.id)" class="pz-tag ok">已下载</span></span>
          <span class="mono dur">{{ e.durationMs ? formatDuration(e.durationMs) : '' }}</span>
        </label>
      </template>
    </VirtualList>
  </div>
</template>

<style scoped>
.picker {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
}
.q {
  flex: 1 1 220px;
}
.range {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--cc-fg2);
}
.n {
  width: 56px;
}
.list {
  max-height: 392px;
  border: 1px solid var(--cc-line-soft);
  border-radius: 8px;
}
.entry {
  display: grid;
  grid-template-columns: 18px 28px 64px minmax(0, 1fr) auto 56px;
  align-items: center;
  gap: 12px;
  height: 48px;
  padding: 0 14px;
  border-bottom: 1px solid var(--cc-line-soft);
  cursor: pointer;
  box-sizing: border-box;
}
.entry:hover {
  background: var(--cc-hover);
}
.entry.done .title {
  color: var(--cc-mute);
}
.idx {
  text-align: right;
  font-size: 12px;
  color: var(--cc-mute);
}
.thumb {
  width: 64px;
  height: 36px;
  border-radius: 4px;
}
.dur {
  text-align: right;
  font-size: 12px;
  color: var(--cc-mute);
}
.empty {
  padding: 16px 0;
}
</style>
