<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../../api'
import type { Bucket, LibraryStats } from '../../types'
import { formatBytes } from '../../utils/format'

const emit = defineEmits<{ changed: [] }>()
const stats = ref<LibraryStats | null>(null)
const loading = ref(false)

const KIND_NAME: Record<string, string> = { video: '视频', image: '图片', audio: '音频', cover: '封面', subtitle: '字幕 / 弹幕' }

async function load() {
  loading.value = true
  try {
    stats.value = await api.libraryStats()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}
onMounted(load)

const sections = computed<{ title: string; rows: Bucket[] }[]>(() => {
  const s = stats.value
  if (!s) return []
  return [
    { title: '按类型', rows: s.byKind.map((b) => ({ ...b, name: KIND_NAME[b.key] ?? b.name })) },
    { title: '按平台', rows: s.byPlatform },
    { title: '按作者（占空间最多的 15 位）', rows: s.byAuthor },
    { title: '按月份（最近 12 个月）', rows: s.byMonth },
  ].filter((x) => x.rows.length)
})

function pct(b: Bucket, rows: Bucket[]) {
  const max = Math.max(...rows.map((r) => r.size), 1)
  return Math.max(2, Math.round((b.size / max) * 100))
}

async function removeMissing() {
  try {
    await ElMessageBox.confirm(`清理 ${stats.value?.missing} 条文件已经不在磁盘上的记录？（不会删除任何文件）`, '清理失效记录', { type: 'warning', confirmButtonText: '清理', cancelButtonText: '取消' })
  } catch {
    return
  }
  try {
    const n = await api.libraryRemoveMissing()
    ElMessage.success(`已清理 ${n} 条记录`)
    emit('changed')
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function trash(id: number) {
  try {
    const r = await api.libraryDelete([id], true)
    ElMessage.success(r.trashed ? '已移到回收站' : '已删除')
    emit('changed')
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div v-loading="loading" class="stats">
    <template v-if="stats">
      <div class="top">
        <div class="num"><b class="mono">{{ stats.count }}</b><span>个文件</span></div>
        <div class="num"><b class="mono">{{ formatBytes(stats.totalSize) }}</b><span>占用空间</span></div>
        <div class="num" :class="{ warn: stats.missing > 0 }">
          <b class="mono">{{ stats.missing }}</b><span>个文件已丢失</span>
          <el-button v-if="stats.missing > 0" link type="primary" size="small" @click="removeMissing">清理记录</el-button>
        </div>
      </div>
      <div v-if="!stats.count" class="mute empty">媒体库还是空的。</div>
      <div class="cols">
        <section v-for="s in sections" :key="s.title">
          <h4>{{ s.title }}</h4>
          <div v-for="b in s.rows" :key="b.key" class="row">
            <span class="name ellipsis" :title="b.name">{{ b.name }}</span>
            <span class="bar"><i :style="{ width: pct(b, s.rows) + '%' }" /></span>
            <span class="val mono">{{ formatBytes(b.size) }} · {{ b.count }}</span>
          </div>
        </section>
      </div>
      <section v-if="stats.largest.length">
        <h4>最大的文件</h4>
        <div v-for="f in stats.largest" :key="f.id" class="big">
          <span class="ellipsis" :title="f.path">{{ f.title }}</span>
          <span class="mono mute">{{ formatBytes(f.size) }}</span>
          <el-button link size="small" :disabled="!f.exists" @click="api.revealFile(f.path)">位置</el-button>
          <el-button link size="small" :disabled="!f.exists" @click="trash(f.id)">删除</el-button>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.stats {
  min-height: 160px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.top {
  display: flex;
  gap: 28px;
  flex-wrap: wrap;
}
.num {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.num b {
  font-size: 22px;
}
.num span {
  color: var(--cc-mute);
}
.num.warn b {
  color: var(--cc-err);
}
.cols {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: 18px;
}
h4 {
  margin: 0 0 6px;
  font-size: 12px;
  color: var(--cc-mute);
  font-weight: 500;
}
.row {
  display: grid;
  grid-template-columns: 110px 1fr 120px;
  gap: 8px;
  align-items: center;
  font-size: 12px;
  height: 22px;
}
.bar {
  background: var(--cc-line);
  height: 6px;
  border-radius: 3px;
  overflow: hidden;
}
.bar i {
  display: block;
  height: 100%;
  background: var(--cc-acc);
}
.val {
  text-align: right;
  color: var(--cc-mute);
  font-size: 11px;
}
.big {
  display: grid;
  grid-template-columns: 1fr 80px auto auto;
  gap: 8px;
  align-items: center;
  font-size: 12px;
  height: 26px;
}
.empty {
  padding: 24px 0;
  text-align: center;
}
</style>
