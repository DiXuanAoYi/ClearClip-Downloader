<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { api, errorText } from '../api'
import type { PlaylistEntry } from '../types'
import { formatDuration } from '../utils/format'
import VirtualList from './VirtualList.vue'

const props = defineProps<{
  entries: PlaylistEntry[]
  selected: Set<string>
  /** 之前已经下载过的条目 */
  downloaded: Set<string>
}>()
const emit = defineEmits<{
  'update:selected': [ids: Set<string>]
  /** 展开过、并且只勾了其中一部分的列表：列表项 id → 勾选的视频 id */
  'update:only': [only: Record<string, string[]>]
  'update:total': [t: { videos: number; lists: number; approx: boolean }]
}>()

interface Kid {
  status: 'loading' | 'ok' | 'error'
  error: string
  entries: PlaylistEntry[]
  sel: Set<string>
  open: boolean
  done: Set<string>
}

const query = ref('')
const hideDone = ref(false)
const from = ref<number | null>(null)
const to = ref<number | null>(null)
const kids = ref<Record<string, Kid>>({})

type Row = { kind: 'entry'; e: PlaylistEntry } | { kind: 'kid'; e: PlaylistEntry; parent: PlaylistEntry } | { kind: 'state'; e: PlaylistEntry; text: string; retry: boolean }

const q = computed(() => query.value.trim().toLowerCase())
const match = (e: PlaylistEntry) => !q.value || e.title.toLowerCase().includes(q.value)

const rows = computed<Row[]>(() => {
  const out: Row[] = []
  for (const e of props.entries) {
    const k = kids.value[e.id]
    const kidMatches = e.isList && k?.status === 'ok' ? k.entries.filter((c) => match(c) && (!hideDone.value || !k.done.has(c.id))) : []
    if (!(match(e) || kidMatches.length) || (hideDone.value && props.downloaded.has(e.id))) continue
    out.push({ kind: 'entry', e })
    if (e.isList && k?.open) {
      if (k.status === 'loading') out.push({ kind: 'state', e, text: '正在读取里面的视频…', retry: false })
      else if (k.status === 'error') out.push({ kind: 'state', e, text: `读取失败：${k.error}`, retry: true })
      else for (const c of q.value || hideDone.value ? kidMatches : k.entries) out.push({ kind: 'kid', e: c, parent: e })
    }
  }
  return out
})

const topVisible = computed(() => rows.value.filter((r): r is Extract<Row, { kind: 'entry' }> => r.kind === 'entry').map((r) => r.e))
const doneCount = computed(() => props.entries.filter((e) => props.downloaded.has(e.id)).length)
const hasLists = computed(() => props.entries.some((e) => e.isList))
const selectedVisible = computed(() => topVisible.value.filter((e) => props.selected.has(e.id)).length)

function set(ids: Iterable<string>) {
  emit('update:selected', new Set(ids))
}

function listState(e: PlaylistEntry): 'none' | 'all' | 'some' {
  if (!props.selected.has(e.id)) return 'none'
  const k = kids.value[e.id]
  if (k?.status === 'ok' && k.sel.size < k.entries.length) return 'some'
  return 'all'
}

/** 列表勾选 / 取消时，里面已读出来的视频跟着全选 / 全不选。 */
function syncKids(e: PlaylistEntry, on: boolean) {
  const k = kids.value[e.id]
  if (e.isList && k?.status === 'ok') k.sel = new Set(on ? k.entries.map((c) => c.id) : [])
}

function toggle(e: PlaylistEntry) {
  const next = new Set(props.selected)
  const on = !next.has(e.id)
  if (on) next.add(e.id)
  else next.delete(e.id)
  syncKids(e, on)
  set(next)
}

function toggleKid(parent: PlaylistEntry, c: PlaylistEntry) {
  const k = kids.value[parent.id]
  if (!k) return
  const sel = new Set(k.sel)
  if (sel.has(c.id)) sel.delete(c.id)
  else sel.add(c.id)
  k.sel = sel
  const next = new Set(props.selected)
  // 里面一个都没勾，整个列表就不选；勾了任何一个，列表就算选中（只下载勾的那些）
  if (sel.size) next.add(parent.id)
  else next.delete(parent.id)
  set(next)
}

async function expand(e: PlaylistEntry) {
  const cur = kids.value[e.id]
  if (cur && cur.status !== 'error') {
    cur.open = !cur.open
    return
  }
  kids.value[e.id] = { status: 'loading', error: '', entries: [], sel: new Set(), open: true, done: new Set() }
  try {
    const info = await api.expandList(e.url)
    const entries = info.entries
    let done = new Set<string>()
    try {
      done = new Set(await api.downloadedEntries(info.platform, entries))
    } catch {
      /* 查不到不影响下载 */
    }
    // 列表已勾选就全选里面的；没勾就都不选
    kids.value[e.id] = { status: 'ok', error: '', entries, sel: new Set(props.selected.has(e.id) ? entries.map((c) => c.id) : []), open: true, done }
  } catch (err) {
    kids.value[e.id] = { status: 'error', error: errorText(err), entries: [], sel: new Set(), open: true, done: new Set() }
  }
}

function selectVisible(on: boolean) {
  const next = new Set(props.selected)
  for (const e of topVisible.value) {
    if (on) next.add(e.id)
    else next.delete(e.id)
    syncKids(e, on)
  }
  set(next)
}

function invert() {
  const next = new Set(props.selected)
  for (const e of topVisible.value) {
    const on = !next.has(e.id)
    if (on) next.add(e.id)
    else next.delete(e.id)
    syncKids(e, on)
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

/** 展开过、只勾了一部分的列表。 */
const only = computed(() => {
  const out: Record<string, string[]> = {}
  for (const e of props.entries) {
    const k = kids.value[e.id]
    if (e.isList && props.selected.has(e.id) && k?.status === 'ok' && k.sel.size < k.entries.length) out[e.id] = [...k.sel]
  }
  return out
})

const total = computed(() => {
  let videos = 0
  let lists = 0
  let approx = false
  for (const e of props.entries) {
    if (!props.selected.has(e.id)) continue
    if (!e.isList) {
      videos++
      continue
    }
    lists++
    const k = kids.value[e.id]
    if (k?.status === 'ok') videos += k.sel.size
    else if (e.count != null) videos += e.count
    else approx = true
  }
  return { videos, lists, approx }
})
watch(only, (v) => emit('update:only', v), { immediate: true })
watch(total, (v) => emit('update:total', v), { immediate: true })

// 列表换了（切到别的分栏）就清空筛选和展开状态
watch(
  () => props.entries,
  () => {
    query.value = ''
    hideDone.value = false
    from.value = null
    to.value = null
    kids.value = {}
  },
)

const rowKey = (r: Row) => (r.kind === 'entry' ? `e:${r.e.id}` : r.kind === 'kid' ? `k:${r.parent.id}:${r.e.id}` : `s:${r.e.id}`)
</script>

<template>
  <div class="picker">
    <div class="pz-sh">
      <span class="pz-st">{{ hasLists ? '列表' : '视频' }}<small>已选 {{ selected.size }} / {{ entries.length }}</small></span>
      <span class="pz-sa">
        <button type="button" class="pz-link" @click="selectVisible(selectedVisible < topVisible.length)">{{ selectedVisible < topVisible.length ? '全选' : '全不选' }}</button>
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
    <div v-if="!rows.length" class="empty pz-note">没有匹配的条目。</div>
    <VirtualList v-else :items="rows" :item-height="48" :item-key="rowKey" class="list">
      <template #default="{ item: r }">
        <label v-if="r.kind === 'entry'" class="entry" :class="{ on: selected.has(r.e.id), done: downloaded.has(r.e.id), list: r.e.isList }">
          <el-checkbox :model-value="selected.has(r.e.id)" :indeterminate="r.e.isList && listState(r.e) === 'some'" @change="toggle(r.e)" />
          <span class="mono idx">{{ r.e.index }}</span>
          <img v-if="r.e.thumbnail" :src="r.e.thumbnail" class="thumb" referrerpolicy="no-referrer" loading="lazy" alt="" />
          <span v-else class="thumb" />
          <span class="title ellipsis" :title="r.e.title">{{ r.e.title }}</span>
          <span class="tags">
            <span v-if="r.e.isList" class="pz-tag acc">列表{{ r.e.count != null ? ` · ${r.e.count} 个视频` : '' }}</span>
            <span v-if="downloaded.has(r.e.id)" class="pz-tag ok">已下载</span>
          </span>
          <button v-if="r.e.isList" type="button" class="pz-link open" :aria-expanded="!!kids[r.e.id]?.open" @click.prevent="expand(r.e)">
            {{ kids[r.e.id]?.status === 'loading' ? '读取中…' : kids[r.e.id]?.open ? '收起 ▴' : '展开 ▾' }}
          </button>
          <span v-else class="mono dur">{{ r.e.durationMs ? formatDuration(r.e.durationMs) : '' }}</span>
        </label>
        <label v-else-if="r.kind === 'kid'" class="entry kid" :class="{ on: kids[r.parent.id]?.sel.has(r.e.id), done: kids[r.parent.id]?.done.has(r.e.id) }">
          <el-checkbox :model-value="kids[r.parent.id]?.sel.has(r.e.id)" @change="toggleKid(r.parent, r.e)" />
          <span class="mono idx">{{ r.e.index }}</span>
          <img v-if="r.e.thumbnail" :src="r.e.thumbnail" class="thumb" referrerpolicy="no-referrer" loading="lazy" alt="" />
          <span v-else class="thumb" />
          <span class="title ellipsis" :title="r.e.title">{{ r.e.title }}</span>
          <span class="tags"><span v-if="kids[r.parent.id]?.done.has(r.e.id)" class="pz-tag ok">之前下载过</span><span v-else class="pz-tag">没下载过</span></span>
          <span class="mono dur">{{ r.e.durationMs ? formatDuration(r.e.durationMs) : '' }}</span>
        </label>
        <div v-else class="entry stateRow">
          <span />
          <span />
          <span />
          <span class="title ellipsis">{{ r.text }}</span>
          <span />
          <button v-if="r.retry" type="button" class="pz-link" @click="expand(r.e)">重试</button>
        </div>
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
  grid-template-columns: 18px 28px 64px minmax(0, 1fr) auto 64px;
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
.entry.list .title {
  font-weight: 600;
}
.entry.kid {
  padding-left: 40px;
  background: var(--cc-hover);
}
.entry.stateRow {
  padding-left: 40px;
  color: var(--cc-mute);
  cursor: default;
}
.entry.done .title {
  color: var(--cc-mute);
}
.tags {
  display: flex;
  gap: 6px;
}
.open {
  justify-self: end;
  white-space: nowrap;
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
