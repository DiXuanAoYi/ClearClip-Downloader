<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { open, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import { useAppStore, useQueueStore } from '../stores/app'
import type { BatchView } from '../types'

const app = useAppStore()
const queue = useQueueStore()
const batches = ref<BatchView[]>([])
let unBatch: (() => void) | null = null
let timer: number | undefined

async function load() {
  batches.value = await api.batchList().catch(() => [])
}
function loadSoon() {
  window.clearTimeout(timer)
  timer = window.setTimeout(load, 400)
}
onMounted(async () => {
  unBatch = await events.onBatchChanged(loadSoon)
  await load()
})
onBeforeUnmount(() => {
  unBatch?.()
  window.clearTimeout(timer)
})
// 任务状态变化时顺带刷新进度
watch(() => queue.tasks.map((t) => t.status).join(''), loadSoon)

const pct = (b: BatchView) => (b.total ? Math.round(((b.done + b.failed + b.failedLinks.length) / b.total) * 100) : 0)
const finished = (b: BatchView) => b.total > 0 && b.done + b.failed + b.failedLinks.length >= b.total && !b.running && !b.queued && !b.pending

async function run<A extends unknown[]>(fn: (...args: A) => Promise<unknown>, ...args: A) {
  try {
    await fn(...args)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function act(b: BatchView, what: string) {
  if (what === 'delete' || what === 'cancel') {
    try {
      await ElMessageBox.confirm(what === 'delete' ? `删除批次“${b.name}”及其所有任务记录？已下载的文件不会删除。` : `取消批次“${b.name}”里所有未完成的任务？`, '确认', { type: 'warning', confirmButtonText: '确定', cancelButtonText: '返回' })
    } catch {
      return
    }
  }
  await run(api.batchAction, b.id, what)
  await load()
}

async function exportFailed(b: BatchView) {
  try {
    const text = await api.batchExportFailed(b.id)
    const path = await saveDialog({ defaultPath: `${b.name}-失败清单.txt`, filters: [{ name: '文本', extensions: ['txt'] }] })
    if (!path) return
    await api.batchSaveText(path, text)
    ElMessage.success('失败清单已导出，可以直接拖回清影重新批量添加')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function importFile() {
  const p = await open({ multiple: false, filters: [{ name: '链接清单', extensions: ['txt', 'csv', 'md', 'html', 'htm'] }] })
  if (!p || typeof p !== 'string') return
  try {
    const [list, cut] = await api.batchReadFile(p)
    if (!list.length) {
      ElMessage.info('文件里没有找到链接')
      return
    }
    app.openBatch(list, '导入文件', cut)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div class="page">
    <div class="head">
      <h2>批量下载</h2>
      <div class="acts">
        <el-button type="primary" @click="app.openBatch([], '手动添加')">新建批量下载</el-button>
        <el-button @click="importFile">从文件导入…</el-button>
      </div>
    </div>
    <p class="mute small hint">一次加入几十上百条链接：先预览（去重、标出已下载过的），再统一设置并发、限速、保存位置和完成后动作。也可以直接把含链接的文本文件拖进窗口，或在解析页点“批量添加…”。</p>

    <div v-if="!batches.length" class="empty card">
      <b>还没有批次</b>
      <span class="mute">点“新建批量下载”粘贴链接，或把 .txt / .csv 文件拖进来。</span>
    </div>

    <div v-else class="list">
      <div v-for="b in batches" :key="b.id" class="bx card">
        <div class="bh">
          <b class="ellipsis" :title="b.name">{{ b.name }}</b>
          <span class="mute small">{{ b.source }} · {{ b.done }}/{{ b.total }} 完成</span>
          <span v-if="finished(b)" class="pl ok">已结束</span>
          <span v-if="b.running" class="pl run">下载中 {{ b.running }}</span>
          <span v-if="b.queued" class="pl">排队 {{ b.queued }}</span>
          <span v-if="b.pending" class="pl">待解析 {{ b.pending }}</span>
          <span v-if="b.paused" class="pl">暂停 {{ b.paused }}</span>
          <span v-if="b.failed + b.failedLinks.length" class="pl bad">失败 {{ b.failed + b.failedLinks.length }}</span>
        </div>
        <div class="bar"><i :style="{ width: pct(b) + '%' }" /></div>
        <div class="bb">
          <el-button link size="small" type="primary" @click="app.goQueueBatch(b.id)">查看任务</el-button>
          <el-button link size="small" :disabled="!b.running && !b.queued" @click="act(b, 'pause')">暂停批次</el-button>
          <el-button link size="small" :disabled="!b.paused" @click="act(b, 'resume')">继续批次</el-button>
          <el-button link size="small" type="primary" :disabled="!b.failed && !b.failedLinks.length" @click="act(b, 'retry')">只重试失败项</el-button>
          <el-button link size="small" :disabled="!b.failed && !b.failedLinks.length" @click="exportFailed(b)">导出失败清单</el-button>
          <el-button link size="small" @click="run(api.revealFile, b.baseDir)">打开文件夹</el-button>
          <el-button link size="small" @click="act(b, 'clear')">清理已完成</el-button>
          <el-button link size="small" @click="act(b, 'cancel')">取消</el-button>
          <el-button link size="small" @click="act(b, 'delete')">删除批次</el-button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.acts {
  display: flex;
  gap: 8px;
}
.hint {
  margin: 0;
}
.empty {
  padding: 28px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: center;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
}
.bx {
  padding: 10px 14px;
}
.bh {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.bx .bar {
  height: 4px;
  border-radius: 2px;
  background: var(--cc-line);
  margin: 7px 0 4px;
  overflow: hidden;
}
.bx .bar i {
  display: block;
  height: 100%;
  background: var(--cc-acc);
}
.bb {
  display: flex;
  flex-wrap: wrap;
  gap: 2px 10px;
}
.bb :deep(.el-button) {
  margin-left: 0;
}
.pl {
  font-size: 11px;
  border: 1px solid var(--cc-line);
  border-radius: 10px;
  padding: 0 7px;
  color: var(--cc-mute);
}
.pl.run,
.pl.ok {
  color: var(--cc-acc);
  border-color: var(--cc-acc);
}
.pl.bad {
  color: var(--cc-err);
  border-color: var(--cc-err);
}
</style>
