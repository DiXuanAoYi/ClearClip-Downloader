<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { TrashItem } from '../../types'
import { formatBytes, formatDateTime } from '../../utils/format'

const emit = defineEmits<{ changed: [] }>()
const app = useAppStore()
const items = ref<TrashItem[]>([])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    items.value = await api.trashList()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}
onMounted(load)

async function restore(t: TrashItem) {
  try {
    const p = await api.trashRestore(t.id)
    ElMessage.success(`已还原到 ${p}`)
    emit('changed')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
  await load()
}

async function purge(t: TrashItem) {
  try {
    await ElMessageBox.confirm(`永久删除“${t.title}”？此操作无法撤销。`, '永久删除', { type: 'warning', confirmButtonText: '永久删除', cancelButtonText: '取消' })
    await api.trashPurge(t.id)
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error(errorText(e))
  }
  await load()
}

async function empty() {
  try {
    await ElMessageBox.confirm(`永久删除回收站里的 ${items.value.length} 个文件？此操作无法撤销。`, '清空回收站', { type: 'warning', confirmButtonText: '清空', cancelButtonText: '取消' })
    const n = await api.trashEmpty()
    ElMessage.success(`已永久删除 ${n} 个文件`)
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error(errorText(e))
  }
  await load()
}
</script>

<template>
  <div v-loading="loading" class="trash">
    <div class="bar">
      <span class="mute">从媒体库删除文件时会先放到这里（下载目录里的隐藏文件夹），<template v-if="app.settings?.library.trashKeepDays">保留 {{ app.settings.library.trashKeepDays }} 天后自动清空</template><template v-else>不会自动清空</template>；可在“设置 → 通用”里调整。</span>
      <el-button size="small" :disabled="!items.length" @click="empty">清空回收站</el-button>
    </div>
    <div v-if="!items.length" class="mute empty">回收站是空的。</div>
    <div v-for="t in items" :key="t.id" class="item">
      <div class="info">
        <div class="ellipsis" :title="t.title">{{ t.title }}</div>
        <small class="mono mute ellipsis" :title="t.originalPath">{{ formatBytes(t.size) }} · 删除于 {{ formatDateTime(t.deletedAt) }} · {{ t.originalPath }}</small>
      </div>
      <el-button link type="primary" size="small" @click="restore(t)">还原</el-button>
      <el-button link size="small" @click="purge(t)">永久删除</el-button>
    </div>
  </div>
</template>

<style scoped>
.trash {
  min-height: 160px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  font-size: 12px;
}
.item {
  display: grid;
  grid-template-columns: 1fr auto auto;
  gap: 8px;
  align-items: center;
  padding: 6px 0;
  border-bottom: 1px solid var(--cc-line);
}
.info {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.info small {
  font-size: 10.5px;
}
.empty {
  padding: 30px 0;
  text-align: center;
}
</style>
