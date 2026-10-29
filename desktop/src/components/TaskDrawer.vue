<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { TaskDetail } from '../types'
import { fileNameOf, formatBytes, formatDateTime } from '../utils/format'
import ErrorAlert from './ErrorAlert.vue'

/** 任务 ID；为 null 时关闭 */
const id = defineModel<number | null>({ required: true })
const detail = ref<TaskDetail | null>(null)
const error = ref('')
let timer: number | undefined

const STATUS: Record<string, string> = { queued: '等待', running: '下载中', paused: '已暂停', done: '已完成', failed: '失败', canceled: '已取消' }
const open = computed({ get: () => id.value !== null, set: (v) => !v && (id.value = null) })

async function load() {
  if (id.value === null) return
  try {
    detail.value = await api.taskDetail(id.value)
    error.value = ''
  } catch (e) {
    error.value = errorText(e)
    detail.value = null
  }
}

watch(
  id,
  (v) => {
    window.clearInterval(timer)
    detail.value = null
    if (v !== null) {
      load()
      // 进行中的任务每 1.5 秒刷新一次过程记录
      timer = window.setInterval(load, 1500)
    }
  },
  { immediate: true },
)
onBeforeUnmount(() => window.clearInterval(timer))

function time(ms: number) {
  const d = new Date(ms)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`
}

/** 整理成一段文字，方便复制给别人排查问题（链接和令牌已打码）。 */
const report = computed(() => {
  const d = detail.value
  if (!d) return ''
  const t = d.task
  const lines = [
    `任务：${fileNameOf(t.filePath) || t.title}（${t.assetLabel}）`,
    `作品：${t.title}`,
    `状态：${STATUS[t.status]}${t.error ? `，${t.error}` : ''}`,
    `来源：${d.sourceUrl || '-'}`,
    ...d.requests.flatMap((r, i) => [`请求 ${i + 1}：${r.label} · ${r.protocol} · ${r.route}`, `  ${r.url}`, ...r.headers.map(([k, v]) => `  ${k}: ${v}`)]),
    '过程记录：',
    ...d.log.map((l) => `  ${time(l.at)}  ${l.text}`),
  ]
  return lines.join('\n')
})

async function copy() {
  try {
    await navigator.clipboard.writeText(report.value)
    ElMessage.success('已复制（链接和令牌已打码）')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <el-drawer v-model="open" :title="detail ? fileNameOf(detail.task.filePath) || detail.task.title : '任务详情'" size="560px" append-to-body>
    <div v-if="error" class="mute">{{ error }}</div>
    <div v-else-if="detail" class="body">
      <ErrorAlert v-if="detail.task.status === 'failed' && detail.task.error" :message="detail.task.error" :kind="detail.task.errorKind" :site="detail.task.site" />

      <section>
        <h4>基本信息</h4>
        <div class="kv"><span>状态</span><span>{{ STATUS[detail.task.status] }}<template v-if="detail.task.total"> · {{ formatBytes(detail.task.received) }} / {{ formatBytes(detail.task.total) }}</template></span></div>
        <div class="kv"><span>作品</span><span class="selectable wrap">{{ detail.task.title }}</span></div>
        <div class="kv"><span>资源</span><span>{{ detail.task.assetLabel }}</span></div>
        <div class="kv"><span>来源链接</span><span class="mono selectable wrap">{{ detail.sourceUrl || '-' }}</span></div>
        <div class="kv"><span>保存到</span><span class="mono selectable wrap">{{ detail.task.filePath }}</span></div>
        <div class="kv"><span>创建时间</span><span>{{ formatDateTime(detail.task.createdAt) }}</span></div>
        <div v-if="detail.task.finishedAt" class="kv"><span>结束时间</span><span>{{ formatDateTime(detail.task.finishedAt) }}</span></div>
        <div v-if="detail.post.length" class="kv"><span>后处理</span><span>{{ detail.post.join('；') }}</span></div>
        <div v-if="detail.partFiles.length" class="kv"><span>临时文件</span><span class="mono selectable wrap">{{ detail.partFiles.join('\n') }}</span></div>
      </section>

      <section>
        <h4>原始请求</h4>
        <div v-for="(r, i) in detail.requests" :key="i" class="req">
          <div class="rh">{{ i + 1 }}. {{ r.label }} <span class="chip">{{ r.protocol }}</span> <span class="chip">{{ r.route }}</span></div>
          <div class="mono selectable wrap url">{{ r.url }}</div>
          <div v-for="[k, v] in r.headers" :key="k" class="mono selectable wrap hdr"><b>{{ k }}</b>: {{ v }}</div>
          <small v-if="!r.headers.length" class="mute">没有额外的请求头{{ r.protocol === 'yt-dlp' ? '（由 yt-dlp 自己发送请求）' : '' }}</small>
        </div>
        <small class="mute">Cookie 和令牌只显示长度，链接里的令牌参数已缩短，可以放心复制给别人排查问题。</small>
      </section>

      <section>
        <h4>过程记录 <el-button link size="small" @click="copy">复制全部</el-button></h4>
        <div class="log mono selectable">
          <div v-for="(l, i) in detail.log" :key="i"><span class="t">{{ time(l.at) }}</span> {{ l.text }}</div>
          <div v-if="!detail.log.length" class="mute">还没有记录。</div>
        </div>
        <small class="mute">过程记录只保存在内存里，退出清影后会清空。</small>
      </section>
    </div>
  </el-drawer>
</template>

<style scoped>
.body {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
h4 {
  margin: 0 0 8px;
  font-size: 12px;
  color: var(--cc-mute);
  font-weight: 500;
  letter-spacing: 0.08em;
}
.kv {
  display: flex;
  gap: 12px;
  font-size: 13px;
  padding: 3px 0;
}
.kv > span:first-child {
  flex: 0 0 72px;
  color: var(--cc-mute);
}
.wrap {
  word-break: break-all;
  white-space: pre-wrap;
  font-size: 12px;
}
.req {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 8px 10px;
  margin-bottom: 8px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.rh {
  font-size: 13px;
}
.url {
  color: var(--cc-acc);
}
.hdr {
  color: var(--cc-mute);
}
.log {
  max-height: 320px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.7;
  background: var(--cc-side);
  border-radius: 8px;
  padding: 8px 10px;
}
.t {
  color: var(--cc-mute);
  margin-right: 6px;
}
</style>
