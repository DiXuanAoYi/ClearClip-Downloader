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
    <div class="bar">
      <span class="count">已选 {{ selected.size }} / {{ entries.length }} 条<template v-if="doneCount"> · 其中 {{ doneCount }} 条之前已下载</template></span>
      <span class="ops">
        <el-button link size="small" type="primary" @click="selectVisible(selectedVisible < visible.length)">{{ selectedVisible < visible.length ? '全选' : '全不选' }}</el-button>
        <el-button link size="small" @click="invert">反选</el-button>
        <el-button v-if="doneCount" link size="small" type="primary" @click="onlyNew">只选没下载过的</el-button>
      </span>
    </div>
    <div class="filters">
      <el-input v-model="query" size="small" clearable placeholder="在列表中搜索标题" class="q" />
      <span class="range">
        <span class="mute">第</span>
        <el-input-number v-model="from" :min="1" :max="entries.length" size="small" controls-position="right" :value-on-clear="null" class="n" />
        <span class="mute">到</span>
        <el-input-number v-model="to" :min="1" :max="entries.length" size="small" controls-position="right" :value-on-clear="null" class="n" />
        <el-button size="small" @click="applyRange">选中这个范围</el-button>
      </span>
      <el-checkbox v-if="doneCount" v-model="hideDone" size="small">隐藏已下载的</el-checkbox>
    </div>
    <div v-if="!visible.length" class="empty mute">没有匹配的条目。</div>
    <VirtualList v-else :items="visible" :item-height="54" :item-key="(e) => e.id" class="list">
      <template #default="{ item: e }">
        <label class="entry" :class="{ on: selected.has(e.id) }">
          <el-checkbox :model-value="selected.has(e.id)" @change="toggle(e.id)" />
          <span class="mono mute idx">{{ e.index }}</span>
          <img v-if="e.thumbnail" :src="e.thumbnail" class="thumb" referrerpolicy="no-referrer" loading="lazy" alt="" />
          <span v-else class="thumb" />
          <span class="title ellipsis" :title="e.title">{{ e.title }}</span>
          <span v-if="downloaded.has(e.id)" class="tag">已下载</span>
          <small v-if="e.durationMs" class="mono mute">{{ formatDuration(e.durationMs) }}</small>
        </label>
      </template>
    </VirtualList>
  </div>
</template>

<style scoped>
.picker {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
  color: var(--cc-mute);
  flex-wrap: wrap;
  gap: 4px;
}
.ops :deep(.el-button) {
  margin-left: 8px;
}
.filters {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.q {
  width: 200px;
}
.range {
  display: flex;
  gap: 6px;
  align-items: center;
  font-size: 12px;
}
.n {
  width: 86px;
}
.list {
  height: 330px;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
}
.entry {
  height: 54px;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border-top: 1px dashed var(--cc-line);
  cursor: pointer;
  font-size: 12.5px;
}
.entry.on {
  background: var(--cc-acc-soft);
}
.idx {
  width: 30px;
  text-align: right;
  flex: none;
}
.thumb {
  width: 64px;
  height: 36px;
  border-radius: 4px;
  object-fit: cover;
  background: var(--cc-line);
  flex: none;
}
.title {
  flex: 1;
  min-width: 0;
}
.tag {
  flex: none;
  font-size: 10.5px;
  padding: 0 6px;
  border-radius: 4px;
  border: 1px solid var(--cc-ok);
  color: var(--cc-ok);
  line-height: 18px;
}
.empty {
  padding: 24px;
  text-align: center;
}
</style>
