<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../../api'
import type { JobSnap } from '../../types'

const jobs = ref<JobSnap[]>([])
let un: UnlistenFn | undefined

onMounted(async () => {
  jobs.value = await api.mediaJobs().catch(() => [])
  un = await events.onMediaJobs((l) => (jobs.value = l))
})
onBeforeUnmount(() => un?.())

const STATUS: Record<string, string> = { queued: '等待中', running: '处理中', done: '完成', failed: '失败', canceled: '已取消' }

async function run(fn: () => Promise<unknown>) {
  try {
    await fn()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

const openOut = (j: JobSnap) => run(() => api.openFile(j.output ?? ''))
const revealOut = (j: JobSnap) => run(() => api.revealFile(j.output ?? ''))
const cancel = (j: JobSnap) => run(() => api.mediaJobCancel(j.id))
const clear = () => run(() => api.mediaJobsClear())
</script>

<template>
  <div class="jobs">
    <div class="head">
      <h4>处理进度</h4>
      <el-button v-if="jobs.some((j) => j.status !== 'running' && j.status !== 'queued')" link size="small" @click="clear">清除已结束的</el-button>
    </div>
    <div v-if="!jobs.length" class="mute empty">还没有处理任务。</div>
    <div v-for="j in [...jobs].reverse()" :key="j.id" class="job card" :class="j.status">
      <div class="top">
        <span class="chip">{{ j.op }}</span>
        <span class="ellipsis ttl" :title="j.title">{{ j.title }}</span>
        <span class="st" :class="j.status">{{ STATUS[j.status] }}</span>
      </div>
      <el-progress v-if="j.status === 'running' || j.status === 'queued'" :percentage="Math.round(j.percent)" :stroke-width="6" />
      <div v-if="j.note" class="note mute">{{ j.note }}</div>
      <div v-if="j.error" class="err selectable">{{ j.error }}</div>
      <div class="acts">
        <template v-if="j.status === 'done' && j.output">
          <el-button link size="small" type="primary" @click="openOut(j)">打开</el-button>
          <el-button link size="small" @click="revealOut(j)">所在文件夹</el-button>
        </template>
        <el-button v-if="j.status === 'running' || j.status === 'queued'" link size="small" @click="cancel(j)">取消</el-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.jobs {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
h4 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-mute);
  font-weight: 500;
}
.job {
  padding: 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.top {
  display: grid;
  grid-template-columns: auto 1fr auto;
  gap: 8px;
  align-items: center;
}
.st {
  font-size: 11.5px;
  color: var(--cc-mute);
}
.st.done {
  color: var(--cc-ok);
}
.st.failed {
  color: var(--cc-err);
}
.note {
  font-size: 11.5px;
}
.err {
  font-size: 12px;
  color: var(--cc-err);
  word-break: break-all;
}
.acts {
  display: flex;
  gap: 4px;
}
.acts :deep(.el-button) {
  margin-left: 0;
  margin-right: 8px;
}
.empty {
  text-align: center;
  padding: 20px 0;
}
</style>
