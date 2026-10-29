<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import ErrorAlert from './ErrorAlert.vue'
import { useAppStore, useParseStore } from '../stores/app'
import type { InboxFilter, InboxItem, InboxSource, InboxStatus } from '../types'
import { formatDateTime } from '../utils/format'
import { guideFor, siteName } from '../utils/siteGuides'

const props = defineProps<{ query: string }>()
const app = useAppStore()
const parse = useParseStore()

const items = ref<InboxItem[]>([])
const loading = ref(false)
const fSource = ref<InboxSource | ''>('')
const fStatus = ref<NonNullable<InboxFilter['status']>>('all')
const expanded = ref<number | null>(null)

const SOURCE: Record<InboxSource, string> = { clipboard: '剪贴板', phone: '手机', extension: '浏览器扩展', manual: '手动粘贴', api: '接口', cli: '命令行' }
const STATUS: Record<InboxStatus, string> = {
  pending_pair: '等待配对',
  rejected: '已拒绝配对',
  confirm: '待确认',
  playlist: '需要选择条目',
  resolving: '解析中',
  queued: '排队中',
  downloading: '下载中',
  done: '已完成',
  failed: '失败',
  ignored: '已忽略',
}
const STATUS_CLASS: Partial<Record<InboxStatus, string>> = { done: 'ok', failed: 'bad', rejected: 'bad', confirm: 'warn', playlist: 'warn', ignored: 'mute' }

const failedCount = computed(() => items.value.filter((i) => i.status === 'failed').length)

async function load() {
  loading.value = true
  try {
    items.value = await api.inboxList({ source: fSource.value || null, status: fStatus.value, query: props.query })
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}

let timer: number | undefined
function reloadSoon() {
  window.clearTimeout(timer)
  timer = window.setTimeout(load, 250)
}
watch(() => props.query, reloadSoon)
watch([fSource, fStatus], load)

let unlisten: UnlistenFn | undefined
let poll: number | undefined
onMounted(async () => {
  await load()
  unlisten = await events.onInbox(reloadSoon)
  // 下载进度在任务完成前不会触发事件，进行中时定期刷新
  poll = window.setInterval(() => {
    if (items.value.some((i) => i.status === 'downloading' || i.status === 'queued' || i.status === 'resolving')) load()
  }, 3000)
})
onUnmounted(() => {
  unlisten?.()
  window.clearInterval(poll)
  window.clearTimeout(timer)
})

async function run<T>(fn: () => Promise<T>, ok?: string | ((r: T) => string)) {
  try {
    const r = await fn()
    if (ok) ElMessage.success(typeof ok === 'function' ? ok(r) : ok)
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function openInParse(i: InboxItem) {
  app.view = 'parse'
  parse.parse(i.text)
}

function displayTitle(i: InboxItem) {
  return i.title || i.url
}

function sourceText(i: InboxItem) {
  const s = SOURCE[i.source] ?? i.source
  return i.deviceName ? `${s} · ${i.deviceName}` : s
}

function needsLogin(i: InboxItem) {
  return i.status === 'failed' && (i.errorKind === 'need_login' || i.errorKind === 'rate_limited') && !!i.site
}

function loginName(i: InboxItem) {
  return siteName(i.site, app.info?.providers)
}

function retryOne(i: InboxItem, ok: string) {
  return run(() => api.inboxRetry([i.id]), ok)
}
function retryFailed() {
  return run(api.inboxRetryFailed, (n) => `已重试 ${n} 条`)
}
function ignoreOne(i: InboxItem) {
  return run(() => api.inboxIgnore([i.id]))
}
function deleteOne(i: InboxItem) {
  return run(() => api.inboxDelete([i.id]))
}
function copyUrl(i: InboxItem) {
  return run(() => api.copyText(i.url), '已复制链接')
}

async function clearDone() {
  await run(() => api.inboxClear('done'), (n) => `已清除 ${n} 条`)
}

async function clearAll() {
  try {
    await ElMessageBox.confirm('清空全部收到的链接记录？进行中的下载不受影响。', '清空记录', { type: 'warning', confirmButtonText: '清空', cancelButtonText: '取消' })
  } catch {
    return
  }
  await run(() => api.inboxClear('all'))
}
</script>

<template>
  <div class="inbox">
    <div class="filters">
      <el-select v-model="fSource" size="small" clearable placeholder="全部来源" class="fsel">
        <el-option v-for="(name, id) in SOURCE" :key="id" :value="id" :label="name" />
      </el-select>
      <el-radio-group v-model="fStatus" size="small">
        <el-radio-button value="all">全部</el-radio-button>
        <el-radio-button value="unhandled">待处理</el-radio-button>
        <el-radio-button value="active">进行中</el-radio-button>
        <el-radio-button value="failed">失败</el-radio-button>
        <el-radio-button value="done">已完成</el-radio-button>
        <el-radio-button value="ignored">已忽略</el-radio-button>
      </el-radio-group>
      <span class="spacer" />
      <el-button v-if="failedCount" size="small" type="primary" plain @click="retryFailed">重试全部失败的</el-button>
      <el-dropdown trigger="click" size="small">
        <el-button size="small">清理</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item @click="clearDone">清除已完成和已忽略的</el-dropdown-item>
            <el-dropdown-item @click="clearAll">清空全部</el-dropdown-item>
            <el-dropdown-item divided @click="app.goSettings('parse')">保留期限设置…</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>

    <div v-loading="loading" class="rows">
      <div v-if="!items.length" class="empty mute">
        <template v-if="query || fSource || fStatus !== 'all'">没有匹配的记录。</template>
        <template v-else>
          还没有收到链接。剪贴板识别到的链接、手机和浏览器扩展发来的链接、手动解析失败的链接都会记录在这里。
        </template>
      </div>
      <div v-for="i in items" :key="i.id" class="item card" :class="{ dim: i.status === 'ignored' }">
        <div class="main" @click="expanded = expanded === i.id ? null : i.id">
          <div class="line1">
            <span class="chip src">{{ SOURCE[i.source] ?? i.source }}</span>
            <span class="title ellipsis" :title="displayTitle(i)">{{ displayTitle(i) }}</span>
            <span class="chip st" :class="STATUS_CLASS[i.status]">{{ STATUS[i.status] ?? i.status }}</span>
          </div>
          <small class="mute ellipsis">
            {{ formatDateTime(i.createdAt) }} · {{ sourceText(i) }}
            <template v-if="i.title"> · <span class="mono">{{ i.url }}</span></template>
            <template v-if="i.seenBefore > 0"> · 之前收到过 {{ i.seenBefore }} 次</template>
            <template v-if="i.message && i.status !== 'failed'"> · {{ i.message }}</template>
          </small>
        </div>
        <div class="ops">
          <template v-if="i.status === 'confirm'">
            <el-button link size="small" type="primary" @click="retryOne(i, '已开始解析并下载')">下载</el-button>
            <el-button link size="small" @click="openInParse(i)">选择清晰度…</el-button>
          </template>
          <el-button v-if="i.status === 'playlist'" link size="small" type="primary" @click="openInParse(i)">选择条目…</el-button>
          <el-button v-if="needsLogin(i)" link size="small" type="primary" @click="app.goLogin(i.site)">登录{{ loginName(i) }}</el-button>
          <el-button v-if="i.status === 'failed' || i.status === 'ignored'" link size="small" :type="needsLogin(i) ? undefined : 'primary'" @click="retryOne(i, '已重试')">重试</el-button>
          <el-button v-if="i.status === 'queued' || i.status === 'downloading' || i.status === 'done'" link size="small" @click="app.view = 'queue'">查看队列</el-button>
          <el-button link size="small" @click="copyUrl(i)">复制</el-button>
          <el-button v-if="['confirm', 'playlist', 'failed'].includes(i.status)" link size="small" @click="ignoreOne(i)">忽略</el-button>
          <el-button link size="small" @click="deleteOne(i)">删除</el-button>
        </div>
        <div v-if="i.status === 'failed' && i.message" class="err">
          <ErrorAlert :message="i.message" :kind="i.errorKind" compact />
          <small v-if="needsLogin(i)" class="mute">
            {{ guideFor(i.site).needLogin }}登录后会自动重试这条链接。
          </small>
        </div>
        <div v-if="expanded === i.id" class="detail selectable mono">{{ i.text }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.inbox {
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex: 1;
  min-height: 0;
}
.filters {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.filters :deep(.el-button) {
  margin-left: 0;
}
.fsel {
  width: 130px;
}
.spacer {
  flex: 1;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
  min-height: 120px;
  flex: 1;
}
.empty {
  padding: 40px 16px;
  text-align: center;
  line-height: 1.8;
}
.item {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 4px 12px;
  align-items: center;
  padding: 8px 12px;
  flex: none;
}
.item.dim {
  opacity: 0.6;
}
.main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
}
.line1 {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.title {
  min-width: 0;
  flex: 1;
}
.main small {
  font-size: 11px;
}
.chip {
  flex: none;
  font-size: 10.5px;
  padding: 0 6px;
  border-radius: 4px;
  border: 1px solid var(--cc-line);
  color: var(--cc-mute);
  line-height: 18px;
}
.chip.ok {
  color: var(--cc-ok);
  border-color: var(--cc-ok);
}
.chip.bad {
  color: var(--cc-err);
  border-color: var(--cc-err);
}
.chip.warn {
  color: #d9a441;
  border-color: #d9a441;
}
.ops {
  display: flex;
  gap: 2px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.ops :deep(.el-button) {
  margin-left: 6px;
}
.err,
.detail {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.err small {
  font-size: 11.5px;
}
.detail {
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-all;
  background: var(--cc-side);
  border-radius: 6px;
  padding: 6px 8px;
}
</style>
