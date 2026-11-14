<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { siteName } from '../utils/siteGuides'
import VirtualList from '../components/VirtualList.vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText, events } from '../api'
import { useAppStore, useQueueStore } from '../stores/app'
import type { BatchView, OrphanPart, TaskSnapshot } from '../types'
import { fileNameOf, formatBytes, formatDateTime, formatEta, formatSpeed } from '../utils/format'
import TaskDrawer from '../components/TaskDrawer.vue'
import CcButton from '../components/ui/CcButton.vue'
import CcSegmented from '../components/ui/CcSegmented.vue'
import CcSelect from '../components/ui/CcSelect.vue'
import CcDialog from '../components/ui/CcDialog.vue'
import { useTauriEvents } from '../composables/useTauriEvent'
const ev = useTauriEvents()

const queue = useQueueStore()
const app = useAppStore()
const orphans = ref<OrphanPart[]>([])
const orphanSize = computed(() => orphans.value.reduce((s, o) => s + o.size, 0))

const afterAll = ref('none')
const detailId = ref<number | null>(null)
const scheduling = ref<TaskSnapshot | null>(null)
const scheduleAt = ref<Date | null>(null)

const batches = ref<BatchView[]>([])
const batchFilter = computed({ get: () => app.queueBatch, set: (v: string | null) => (app.queueBatch = v) })
const tab = ref<'all' | 'running' | 'queued' | 'failed' | 'done'>('all')
let unBatch: (() => void) | null = null
let refreshTimer: number | undefined

async function loadBatches() {
  batches.value = await api.batchList().catch(() => []) // 忽略：可选数据，取不到就按空处理
  if (batchFilter.value && !batches.value.some((b) => b.id === batchFilter.value)) batchFilter.value = null
}
function refreshSoon() {
  window.clearTimeout(refreshTimer)
  refreshTimer = window.setTimeout(loadBatches, 400)
}
watch(
  () => queue.tasks.map((t) => t.status).join(''),
  refreshSoon,
)
onBeforeUnmount(() => {
  unBatch?.()
  window.clearTimeout(refreshTimer)
})

const batchName = (id: string | null | undefined) => batches.value.find((b) => b.id === id)?.name
// 从列表（播放列表、合集）展开下载的任务按列表筛选
const listFilter = ref<string>('')
const listOptions = computed(() => [{ value: '', label: '列表：全部' }, ...lists.value.map((l) => ({ value: l.name, label: `${l.name}（${l.count}）` }))])
const tabOptions = [
  { value: 'all', label: '全部' },
  { value: 'running', label: '进行中' },
  { value: 'queued', label: '排队/暂停' },
  { value: 'failed', label: '失败' },
  { value: 'done', label: '已完成' },
] as const
const afterOptions = [
  { value: 'none', label: '不操作' },
  { value: 'sleep', label: '睡眠' },
  { value: 'shutdown', label: '关机' },
]
const lists = computed(() => {
  const m = new Map<string, number>()
  for (const t of queue.tasks) if (t.listName) m.set(t.listName, (m.get(t.listName) ?? 0) + 1)
  return [...m].map(([name, count]) => ({ name, count }))
})
watch(lists, (v) => {
  if (listFilter.value && !v.some((l) => l.name === listFilter.value)) listFilter.value = ''
})

onMounted(async () => {
  unBatch = await ev.on(events.onBatchChanged(refreshSoon))
  await loadBatches()
  try {
    orphans.value = await api.listOrphanParts()
  } catch {
    orphans.value = []
  }
  afterAll.value = await api.getAfterAllDone().catch(() => 'none')
})

async function setAfterAll(v: string) {
  await run(api.setAfterAllDone, v)
  if (v === 'shutdown') ElMessage.info('全部下载完成后将在 60 秒倒计时后关机，倒计时期间可以取消。')
}

function openSchedule(t: TaskSnapshot) {
  scheduling.value = t
  scheduleAt.value = t.startAt ? new Date(t.startAt * 1000) : new Date(Date.now() + 3600_000)
}

async function confirmSchedule() {
  if (!scheduling.value || !scheduleAt.value) return
  await run(api.scheduleTask, scheduling.value.id, Math.floor(scheduleAt.value.getTime() / 1000))
  scheduling.value = null
}

async function onMore(t: TaskSnapshot, cmd: string) {
  switch (cmd) {
    case 'schedule':
      return openSchedule(t)
    case 'now':
      return run(api.scheduleTask, t.id, null)
    case 'detail':
      detailId.value = t.id
      return
    case 'subtitle':
      return app.goToolbox([t.filePath], undefined, { tab: 'ai', aiMode: 'transcribe' })
    case 'remove':
      return run(api.removeTask, t.id)
    case 'cancel':
      try {
        await ElMessageBox.confirm(`取消“${t.title}”的下载？已下载的部分会保留，可以之后重试。`, '取消下载', { type: 'warning', confirmButtonText: '取消下载', cancelButtonText: '先不取消' })
      } catch {
        return
      }
      return run(api.cancelTask, t.id)
    default:
      return run(api.moveTask, t.id, cmd as 'top' | 'up' | 'down' | 'bottom')
  }
}

/** 单个音视频文件、已知大小、正在下载或暂停的任务才能边下边播（m3u8 / 站点解析的流要等合并后才有完整文件）。 */
const canPreview = (t: TaskSnapshot) =>
  (t.status === 'running' || t.status === 'paused') && t.inputs === 1 && (t.assetKind === 'video' || t.assetKind === 'audio') && !!t.total && t.received > 0 && !/\.(m3u8|mpd)$/i.test(t.filePath)
const isWaiting = (t: TaskSnapshot) => t.status === 'queued' || t.status === 'paused' || t.status === 'failed'

async function cleanOrphans() {
  try {
    await ElMessageBox.confirm(
      `删除 ${orphans.value.length} 个不属于任何任务的未完成文件（共 ${formatBytes(orphanSize.value)}）？这些通常是旧版本或异常退出留下的。`,
      '清理残留文件',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  const n = await api.deleteOrphanParts(orphans.value.map((o) => o.path))
  ElMessage.success(`已删除 ${n} 个残留文件`)
  orphans.value = await api.listOrphanParts()
}
/** 进行中 → 等待（按优先级）→ 已暂停 / 失败 → 已完成（最近的在前）。 */
const tasks = computed(() => {
  const inTab = (t: TaskSnapshot) => {
    switch (tab.value) {
      case 'running':
        return t.status === 'running'
      case 'queued':
        return t.status === 'queued' || t.status === 'paused'
      case 'failed':
        return t.status === 'failed'
      case 'done':
        return t.status === 'done' || t.status === 'canceled'
      default:
        return true
    }
  }
  const group = (t: TaskSnapshot) => ({ running: 0, queued: 1, paused: 2, failed: 2, done: 3, canceled: 3 })[t.status]
  return queue.tasks.filter((t) => inTab(t) && (!batchFilter.value || t.batchId === batchFilter.value) && (!listFilter.value || t.listName === listFilter.value)).sort((a, b) => {
    const g = group(a) - group(b)
    if (g !== 0) return g
    if (group(a) === 3) return (b.finishedAt ?? b.id) - (a.finishedAt ?? a.id) || b.id - a.id
    return b.priority - a.priority || a.id - b.id
  })
})

const MEDIA_EXT = /\.(mp4|mkv|webm|mov|m4v|flv|ts|avi|mp3|m4a|aac|flac|wav|ogg|opus)$/i
const isMedia = (p?: string | null) => !!p && MEDIA_EXT.test(p)
const statusText: Record<TaskSnapshot['status'], string> = {
  queued: '等待',
  running: '下载中',
  paused: '已暂停',
  done: '完成',
  failed: '失败',
  canceled: '已取消',
}

function percent(t: TaskSnapshot) {
  if (t.status === 'done') return 100
  if (!t.total) return 0
  return Math.min(100, Math.floor((t.received / t.total) * 100))
}

/** 现在生效的限速：限速计划里命中的时段优先，没有命中就用全局限速（KB/s，0 = 不限） */
const speedPlan = computed(() => {
  const st = app.settings
  if (!st) return null
  const wins = st.speedSchedule ?? []
  if (!wins.length && !st.speedLimitKbps) return null
  const now = new Date()
  const day = now.getDay() === 0 ? 7 : now.getDay()
  const mins = now.getHours() * 60 + now.getMinutes()
  const toMin = (t: string) => {
    const [h, m] = t.split(':').map(Number)
    return (h || 0) * 60 + (m || 0)
  }
  const hit = wins.find((w) => {
    if (w.days.length && !w.days.includes(day)) return false
    const a = toMin(w.start)
    const b = toMin(w.end)
    return a <= b ? mins >= a && mins < b : mins >= a || mins < b
  })
  const kbps = hit ? hit.limitKbps : st.speedLimitKbps
  const fmt = (k: number) => (k >= 1024 ? `${(k / 1024).toFixed(k % 1024 ? 1 : 0)} MB/s` : `${k} KB/s`)
  const prefix = wins.length ? '限速计划：' : '限速：'
  return {
    limited: kbps > 0,
    text: `${prefix}${kbps > 0 ? fmt(kbps) : '不限速'}`,
    title: hit ? `现在在时段 ${hit.start}–${hit.end} 内` : '没有命中任何时段，使用全局限速',
  }
})

function detail(t: TaskSnapshot) {
  if (t.note) return t.note
  switch (t.status) {
    case 'running':
      return [t.total ? `${formatBytes(t.received)} / ${formatBytes(t.total)}` : formatBytes(t.received), formatSpeed(t.speed), formatEta(t.received, t.total, t.speed)]
        .filter(Boolean)
        .join(' · ')
    case 'queued':
      return t.startAt && t.startAt * 1000 > Date.now() ? `定时 ${formatDateTime(t.startAt)} 开始` : '排队中'
    case 'paused':
      return t.received ? `已下载 ${formatBytes(t.received)}，继续时从断点开始` : '已暂停'
    case 'failed':
      return t.error ?? '下载失败'
    case 'done':
      return `${formatBytes(t.total ?? t.received)} · ${t.filePath}`
    case 'canceled':
      return '已取消'
  }
}

async function run<A extends unknown[]>(fn: (...args: A) => Promise<unknown>, ...args: A) {
  try {
    await fn(...args)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div class="dpage page">
    <div class="dtop">
      <h1>下载队列</h1>
      <span class="dspacer" />
      <span v-if="speedPlan" class="chip" :class="speedPlan.limited ? 'acc' : 'cyan'" :title="speedPlan.title"><i class="ddot" :class="{ amber: speedPlan.limited }" />{{ speedPlan.text }}</span>
      <span v-if="queue.counts.running" class="chip cyan"><i class="ddot" />{{ queue.counts.running }} 个下载中</span>
    </div>
    <div class="dstats">
      <div class="dstat" :class="{ on: queue.counts.running > 0 }"><b>{{ queue.counts.running }}</b><span>进行中</span></div>
      <div class="dstat"><b>{{ queue.counts.queued }}</b><span>等待</span></div>
      <div class="dstat"><b>{{ queue.counts.paused }}</b><span>暂停</span></div>
      <div class="dstat"><b>{{ queue.counts.done }}</b><span>完成</span></div>
      <div class="dstat"><b>{{ queue.counts.failed }}</b><span>失败</span></div>
      <div class="dstat"><b class="mono speed">{{ formatSpeed(queue.totalSpeed) }}</b><span>总速度</span></div>
    </div>

    <section class="dpanel qpanel">
      <div class="dtoolbar qbar">
        <CcSegmented v-model="tab" :options="[...tabOptions]" size="s" aria-label="状态" />
        <CcSelect v-if="lists.length" v-model="listFilter" :options="listOptions" size="s" width="m" aria-label="按列表筛选" data-testid="list-filter" />
        <span v-if="batchFilter" class="mute small">只看批次“{{ batchName(batchFilter) }}” <CcButton kind="text" size="s" @click="batchFilter = null">显示全部</CcButton></span>
        <span class="dspacer" />
        <CcButton size="s" :disabled="queue.active === 0" disabled-reason="没有进行中或排队的任务" @click="run(api.pauseAll)">全部暂停</CcButton>
        <CcButton size="s" :disabled="queue.counts.paused + queue.counts.failed === 0" disabled-reason="没有已暂停或失败的任务" @click="run(api.resumeAll)">全部继续 / 重试</CcButton>
        <CcButton size="s" :disabled="!queue.tasks.some((t) => t.status === 'done' || t.status === 'canceled')" disabled-reason="还没有已完成的任务" @click="run(api.clearFinished)">清除已完成</CcButton>
      </div>

      <el-alert v-if="orphans.length" type="info" show-icon :closable="false">
        <template #title>下载目录里有 {{ orphans.length }} 个不属于任何任务的未完成文件（共 {{ formatBytes(orphanSize) }}）</template>
        <CcButton size="s" @click="cleanOrphans">清理</CcButton>
      </el-alert>

      <div v-if="tasks.length === 0" class="dempty mute">队列是空的。在“解析”页选择内容并点击“下载所选”后，任务会出现在这里。</div>

      <div v-if="tasks.length" class="qlist"><VirtualList :items="tasks" :item-height="92" :gap="10" :item-key="(t) => t.id" class="list">
        <template #default="{ item: t }">
          <div class="task ditem" :class="t.status">
            <img v-if="t.cover" :src="t.cover" class="dthumb tcover" referrerpolicy="no-referrer" alt="" />
            <div v-else class="dthumb tcover" />
            <div class="info grow">
              <div class="name ellipsis" :title="`${fileNameOf(t.filePath) || t.title}（作品：${t.title}）`"><span v-if="t.batchSeq" class="seq mono">#{{ t.batchSeq }}</span> {{ fileNameOf(t.filePath) || t.title }} <span class="mute">· {{ t.assetLabel }}</span> <span v-if="t.listName" class="chip" :title="`来自列表：${t.listName}`">{{ t.listName }}</span></div>
              <div class="dprog" :class="t.status === 'failed' ? 'red' : t.status === 'paused' || t.status === 'queued' || t.status === 'canceled' ? 'amber' : ''"><i :style="{ width: (t.status === 'failed' ? 100 : percent(t)) + '%' }" /></div>
              <div class="meta">
                <small class="mono detail ellipsis selectable" :title="detail(t)">{{ detail(t) }}</small>
                <div class="tags">
                  <span v-if="t.resumable === false && t.status !== 'done'" class="chip">服务器不支持续传</span>
                  <span v-else-if="t.resumable && (t.status === 'paused' || t.status === 'running')" class="chip">可续传</span>
                  <CcButton v-if="t.status === 'failed' && (t.errorKind === 'need_login' || t.errorKind === 'rate_limited')" kind="text" size="s" @click="app.goLogin(t.site)">{{ t.site ? `登录${siteName(t.site, app.info?.providers)}后自动重试` : '添加 Cookie 后重试' }}</CcButton>
                </div>
              </div>
              <small v-if="t.warning" class="warn-line selectable" :title="t.warning"><svg class="wic" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4 2.5 20h19zM12 10v5M12 18v.5" /></svg><span class="cc-sr">警告：</span>{{ t.warning }}</small>
              
            </div>
            <div class="st">
              <span class="state chip" :class="{ cyan: t.status === 'running' || t.status === 'done', acc: t.status === 'paused' || t.status === 'queued' }">{{ statusText[t.status] }}<template v-if="t.status === 'running' && t.total"> {{ percent(t) }}%</template></span>
              <div class="actions">
                <CcButton v-if="t.status === 'running' || t.status === 'queued'" icon-only size="s" kind="text" aria-label="暂停" @click="run(api.pauseTask, t.id)">
                  <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M9 5v14M15 5v14" /></svg></template>
                </CcButton>
                <CcButton v-if="canPreview(t)" icon-only size="s" kind="text" aria-label="边下边播" @click="app.previewTask(t.id)">
                  <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M3 6h18v12H3zM10 9.5v5l4-2.5z" /></svg></template>
                </CcButton>
                <CcButton v-if="t.status === 'paused'" icon-only size="s" kind="text" aria-label="继续" @click="run(api.resumeTask, t.id)">
                  <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M8 5l11 7-11 7z" /></svg></template>
                </CcButton>
                <CcButton v-if="t.status === 'failed' || t.status === 'canceled'" kind="primary" size="s" @click="run(api.resumeTask, t.id)">重试</CcButton>
                <CcButton v-if="isWaiting(t) && t.status !== 'failed'" icon-only size="s" kind="text" aria-label="优先" title="排到最前并马上开始；下载位已满时，让优先级最低的任务暂时让位（它会自动回到队列，不丢进度）" @click="run(api.prioritizeTask, t.id)">
                  <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M12 19V5M6 11l6-6 6 6" /></svg></template>
                </CcButton>
                <CcButton v-if="t.status === 'done'" icon-only size="s" kind="text" aria-label="打开文件" @click="run(api.openFile, t.filePath)">
                  <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M8 5l11 7-11 7z" /></svg></template>
                </CcButton>
                <CcButton v-if="t.status === 'done'" icon-only size="s" kind="text" aria-label="打开所在文件夹" @click="run(api.revealFile, t.filePath)">
                  <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M3 6h7l2 2h9v11H3z" /></svg></template>
                </CcButton>
                <el-dropdown trigger="click" @command="(c: string) => onMore(t, c)">
                  <CcButton icon-only size="s" kind="text" aria-label="更多操作" data-testid="task-more">
                    <template #icon><svg class="qi" viewBox="0 0 24 24"><path d="M5 12h.01M12 12h.01M19 12h.01" stroke-width="3" /></svg></template>
                  </CcButton>
                  <template #dropdown>
                    <el-dropdown-menu>
                      <template v-if="isWaiting(t) && t.status !== 'failed'">
                        <el-dropdown-item command="top">置顶</el-dropdown-item>
                        <el-dropdown-item command="up">上移</el-dropdown-item>
                        <el-dropdown-item command="down">下移</el-dropdown-item>
                        <el-dropdown-item command="bottom">置底</el-dropdown-item>
                        <el-dropdown-item command="schedule" divided>定时开始…</el-dropdown-item>
                        <el-dropdown-item v-if="t.startAt" command="now">取消定时，立即排队</el-dropdown-item>
                      </template>
                      <el-dropdown-item v-if="t.status === 'done' && isMedia(t.filePath)" command="subtitle">生成字幕</el-dropdown-item>
                      <el-dropdown-item command="detail" :divided="isWaiting(t) && t.status !== 'failed'">详情</el-dropdown-item>
                      <el-dropdown-item v-if="t.status === 'running' || t.status === 'paused' || t.status === 'queued'" command="cancel" divided class="danger-item">取消下载</el-dropdown-item>
                      <el-dropdown-item v-else command="remove" divided>移除</el-dropdown-item>
                    </el-dropdown-menu>
                  </template>
                </el-dropdown>
              </div>
            </div>
          </div>
        </template>
      </VirtualList></div>

      <div class="dtoolbar qfoot">
        <span class="mute small">全部完成后</span>
        <CcSegmented v-model="afterAll" :options="afterOptions" size="s" aria-label="全部完成后" @update:model-value="setAfterAll" />
        <span class="dspacer" />
        <span class="dnote">只对本次运行有效</span>
      </div>
    </section>

    <TaskDrawer v-model="detailId" />

    <CcDialog :model-value="!!scheduling" title="定时开始" size="sm" @update:model-value="(v: boolean) => !v && (scheduling = null)">
      <p class="mute small">到时间后自动开始下载“{{ scheduling?.title }}”。程序需要保持运行（可最小化到托盘）。</p>
      <el-date-picker v-model="scheduleAt" type="datetime" :disabled-date="(d: Date) => d.getTime() < Date.now() - 86400_000" style="width: 100%" />
      <template #footer>
        <CcButton @click="scheduling = null">取消</CcButton>
        <CcButton kind="primary" @click="confirmSchedule">确定</CcButton>
      </template>
    </CcDialog>
  </div>
</template>

<style scoped>
.seq {
  color: var(--cc-acc);
  font-size: var(--cc-fs-xs);
}
.warn-line {
  display: flex;
  align-items: flex-start;
  gap: var(--cc-sp-1);
  color: var(--cc-warn-text);
  background: var(--cc-warn-soft);
  border-radius: var(--cc-r-sm);
  padding: 1px var(--cc-sp-2);
  font-size: var(--cc-fs-sm);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.wic {
  flex: none;
  margin-top: 2px;
}
.page {
  height: 100%;
  box-sizing: border-box;
}
.speed {
  font-size: var(--cc-fs-xl);
}
/* 六张统计卡始终一行（900 宽也不折成两行，给列表留高度） */
.dstats {
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: var(--cc-sp-2);
}
.qpanel {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--cc-sp-3);
}
.qbar {
  gap: var(--cc-sp-2);
}
/* 列表底部渐隐：底部留出 56px，最后一行滚到底时完整露在渐隐区之上 */
.qlist {
  flex: 1;
  min-height: 0;
  display: flex;
}
.list {
  flex: 1;
  min-height: 0;
  margin-right: -8px;
  padding-right: 8px;
  padding-bottom: 56px;
  box-sizing: border-box;
  -webkit-mask-image: linear-gradient(to bottom, black calc(100% - 48px), transparent);
  mask-image: linear-gradient(to bottom, black calc(100% - 48px), transparent);
}
.small {
  font-size: var(--cc-fs-sm);
  align-self: center;
}
.dempty {
  padding: 40px 0;
  text-align: center;
  border: 1px dashed var(--cc-line-strong);
  border-radius: var(--cc-r-lg);
}
.task {
  height: 100%;
  box-sizing: border-box;
  overflow: hidden;
  display: grid;
  grid-template-columns: 110px 1fr auto;
}
.tcover {
  width: 110px;
}
.info {
  min-width: 0;
}
.info .dprog {
  margin: 4px 0 3px;
}
.detail {
  display: block;
  font-size: var(--cc-fs-xs);
  color: var(--cc-mute);
}
.meta {
  display: flex;
  align-items: center;
  gap: var(--cc-sp-2);
  min-width: 0;
}
.meta .detail {
  flex: 0 1 auto;
  min-width: 0;
}
.tags {
  flex: none;
  display: flex;
  gap: 6px;
  align-items: center;
}
.tags:empty {
  display: none;
}
.failed .detail {
  color: var(--cc-err);
}
.st {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  justify-content: center;
  gap: var(--cc-sp-2);
  font-size: var(--cc-fs-sm);
  max-width: 300px;
}
.actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 2px;
}
.qi {
  width: 16px;
  height: 16px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.qfoot {
  padding-top: 2px;
}
</style>
<style>
.el-dropdown-menu__item.danger-item {
  color: var(--cc-err);
}
</style>
