<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../../api'
import { useAppStore } from '../../stores/app'
import type { DupGroup, LibraryItem } from '../../types'
import { formatBytes, formatDateTime, formatDuration } from '../../utils/format'

const emit = defineEmits<{ changed: [] }>()
const groups = ref<DupGroup[]>([])
const mode = ref<'exact' | 'similar' | null>(null)
const busy = ref(false)
const prog = ref<{ done: number; total: number } | null>(null)
/** 勾选要删除的记录 ID */
const marked = ref<Set<number>>(new Set())
let un: UnlistenFn | undefined

onMounted(async () => {
  un = await events.onLibraryProgress((p) => {
    if (p.task === 'similar') prog.value = p.done >= p.total ? null : { done: p.done, total: p.total }
  })
})
onBeforeUnmount(() => un?.())

/** 每组默认保留：画面更清楚（文件更大）的，其次是最早下载的；其余默认勾选删除。 */
function markDefaults(gs: DupGroup[]) {
  const next = new Set<number>()
  for (const g of gs) {
    const keep = [...g.items].sort((a, b) => b.size - a.size || a.finishedAt - b.finishedAt || a.id - b.id)[0]
    for (const i of g.items) if (i.id !== keep.id) next.add(i.id)
  }
  marked.value = next
}

async function scan(kind: 'exact' | 'similar') {
  busy.value = true
  mode.value = kind
  groups.value = []
  try {
    groups.value = kind === 'exact' ? await api.duplicatesExact() : await api.duplicatesSimilar()
    markDefaults(groups.value)
    if (!groups.value.length) ElMessage.success('没有发现重复的文件')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
    prog.value = null
  }
}

function toggle(i: LibraryItem) {
  const next = new Set(marked.value)
  if (next.has(i.id)) next.delete(i.id)
  else next.add(i.id)
  marked.value = next
}

const markedItems = computed(() => groups.value.flatMap((g) => g.items).filter((i) => marked.value.has(i.id)))
const savings = computed(() => markedItems.value.reduce((s, i) => s + i.size, 0))

function groupFull(g: DupGroup) {
  // 不允许一组全部删除
  return g.items.every((i) => marked.value.has(i.id))
}

async function removeMarked() {
  if (groups.value.some(groupFull)) {
    ElMessage.warning('有一组的文件全部被勾选了，每组至少要保留一个。')
    return
  }
  const trash = useTrash.value
  try {
    await ElMessageBox.confirm(`${trash ? '移到回收站' : '永久删除'} ${markedItems.value.length} 个文件，可释放 ${formatBytes(savings.value)}？`, '删除重复文件', {
      type: 'warning',
      confirmButtonText: trash ? '移到回收站' : '永久删除',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }
  try {
    const r = await api.libraryDelete(
      markedItems.value.map((i) => i.id),
      true,
    )
    if (r.failed.length) ElMessage.warning(`${r.failed.length} 个文件删除失败：${r.failed[0]}`)
    else ElMessage.success(`已处理 ${r.removed} 个文件`)
    emit('changed')
    if (mode.value) await scan(mode.value)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

const app = useAppStore()
const useTrash = computed(() => app.settings?.library.useTrash ?? true)

function cover(i: LibraryItem) {
  return i.coverPath ? convertFileSrc(i.coverPath) : i.cover
}
</script>

<template>
  <div class="dupes">
    <div class="bar">
      <el-button size="small" type="primary" :loading="busy && mode === 'exact'" :disabled="busy" @click="scan('exact')">查找内容完全相同的文件</el-button>
      <el-button size="small" :loading="busy && mode === 'similar'" :disabled="busy" @click="scan('similar')">查找画面相似的视频</el-button>
      <span class="mute hint">“相似”会在每个视频的 3 个位置各取一帧比较，能找出重新编码、不同清晰度的同一个视频；第一次扫描较慢，结果会缓存。</span>
    </div>
    <el-progress v-if="prog" :percentage="Math.round((prog.done / Math.max(prog.total, 1)) * 100)" :stroke-width="6" :format="() => `${prog!.done} / ${prog!.total}`" />
    <div v-if="mode && !busy && !groups.length" class="mute empty">没有发现重复的文件。</div>
    <div v-for="(g, gi) in groups" :key="gi" class="group card" :class="{ warn: groupFull(g) }">
      <div class="gh mute">{{ g.kind === 'exact' ? '内容完全相同' : '画面相似' }} · {{ g.items.length }} 个</div>
      <label v-for="i in g.items" :key="i.id" class="it">
        <el-checkbox :model-value="marked.has(i.id)" @change="toggle(i)" />
        <img v-if="cover(i)" :src="cover(i)!" class="thumb" referrerpolicy="no-referrer" alt="" />
        <div v-else class="thumb" />
        <div class="info">
          <div class="ellipsis" :title="i.title">{{ i.title }}</div>
          <small class="mono mute ellipsis" :title="i.path">{{ formatBytes(i.size) }}<template v-if="i.durationMs"> · {{ formatDuration(i.durationMs) }}</template> · {{ formatDateTime(i.finishedAt) }} · {{ i.path }}</small>
        </div>
        <el-button link size="small" @click.prevent="api.revealFile(i.path)">位置</el-button>
      </label>
    </div>
    <div v-if="groups.length" class="foot">
      <span>已勾选 {{ markedItems.length }} 个，可释放 <b class="mono">{{ formatBytes(savings) }}</b>（默认保留每组里文件最大的一个）</span>
      <el-button type="primary" size="small" :disabled="!markedItems.length" @click="removeMarked">删除勾选的文件</el-button>
    </div>
  </div>
</template>

<style scoped>
.dupes {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.bar {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.hint {
  font-size: 11.5px;
  flex-basis: 100%;
}
.group {
  padding: 8px 10px;
}
.group.warn {
  border-color: var(--cc-err);
}
.gh {
  font-size: 11.5px;
  margin-bottom: 4px;
}
.it {
  display: grid;
  grid-template-columns: 24px 40px 1fr auto;
  gap: 10px;
  align-items: center;
  padding: 4px 0;
  cursor: pointer;
}
.it .thumb {
  width: 40px;
  height: 28px;
}
.info {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.info small {
  font-size: 10.5px;
}
.foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  position: sticky;
  bottom: 0;
  background: var(--cc-bg);
  padding: 8px 0;
}
.empty {
  padding: 30px 0;
  text-align: center;
}
</style>
