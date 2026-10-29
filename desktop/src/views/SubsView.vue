<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { ListResult, SubItem, SubSettings, Subscription } from '../types'
import { formatDateTime, formatDuration } from '../utils/format'
import ErrorAlert from '../components/ErrorAlert.vue'
import { errorKind } from '../api'
import type { ErrorKind } from '../types'


const app = useAppStore()
const subs = ref<Subscription[]>([])
let unlisten: UnlistenFn | undefined

const DEFAULTS: SubSettings = {
  intervalHours: 6,
  firstRun: 'new_only',
  firstN: 5,
  maxAuto: 20,
  include: [],
  exclude: [],
  minDurationS: null,
  maxDurationS: null,
  maxAgeDays: null,
  quality: null,
  dir: '',
  template: '',
  notify: true,
  keepLatest: 0,
  keepDays: 0,
}

// ---------- 列表 ----------

async function load() {
  try {
    subs.value = await api.subsList()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

onMounted(async () => {
  await load()
  unlisten = await events.onSubs(load)
})
onUnmounted(() => unlisten?.())

async function enable() {
  try {
    await ElMessageBox.confirm(
      '<p>订阅会定期访问你添加的频道、UP 主、画师等页面，有新内容时自动下载。</p><ul style="padding-left:18px;margin:8px 0 0;line-height:1.7"><li>程序需要保持运行（可最小化到托盘），可在“设置 → 通用”中开启开机自启。</li><li>检查太频繁、一次下载太多可能触发网站风控，使用登录账号时有被限制的风险。清影默认每 6 小时检查一次，并限制单次下载数量。</li><li>请只订阅你有权保存的内容。</li></ul>',
      '开启订阅',
      { confirmButtonText: '开启', cancelButtonText: '取消', type: 'info', dangerouslyUseHTMLString: true },
    )
  } catch {
    return
  }
  await app.patch({ subscriptionsEnabled: true })
}

async function run<A extends unknown[]>(fn: (...args: A) => Promise<unknown>, ...args: A) {
  try {
    await fn(...args)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function checkNow(s: Subscription) {
  try {
    const n = await api.subsCheck(s.id)
    ElMessage.success(n ? `“${s.title}”发现 ${n} 个新内容` : `“${s.title}”没有新内容`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function remove(s: Subscription) {
  try {
    await ElMessageBox.confirm(`取消订阅“${s.title}”？已下载的文件不受影响。`, '取消订阅', { type: 'warning', confirmButtonText: '取消订阅', cancelButtonText: '保留' })
  } catch {
    return
  }
  await run(api.subsDelete, s.id)
}

function statusText(s: Subscription) {
  if (s.checking) return '检查中…'
  if (s.status === 'paused') return '已暂停'
  if (s.status === 'error') return '异常，已暂停'
  return '正常'
}

function relative(ts: number | null) {
  if (!ts) return '—'
  const d = ts - Date.now() / 1000
  const abs = Math.abs(d)
  const text = abs < 3600 ? `${Math.max(1, Math.round(abs / 60))} 分钟` : abs < 86400 ? `${Math.round(abs / 3600)} 小时` : `${Math.round(abs / 86400)} 天`
  return d > 0 ? `${text}后` : `${text}前`
}

// ---------- 添加 / 编辑 ----------

const editing = ref<{ id: number | null; url: string; title: string; settings: SubSettings } | null>(null)
const preview = ref<ListResult | null>(null)
const previewErr = ref<{ message: string; kind: ErrorKind } | null>(null)
const previewing = ref(false)
const saving = ref(false)
const includeText = ref('')
const excludeText = ref('')
const minMin = ref<number | null>(null)
const maxMin = ref<number | null>(null)

function openAdd() {
  editing.value = { id: null, url: '', title: '', settings: { ...DEFAULTS } }
  preview.value = null
  previewErr.value = null
  syncFields()
}

function openEdit(s: Subscription) {
  editing.value = { id: s.id, url: s.url, title: s.title, settings: JSON.parse(JSON.stringify(s.settings)) }
  preview.value = null
  previewErr.value = null
  syncFields()
}

function syncFields() {
  const st = editing.value!.settings
  includeText.value = st.include.join('，')
  excludeText.value = st.exclude.join('，')
  minMin.value = st.minDurationS ? Math.round(st.minDurationS / 60) : null
  maxMin.value = st.maxDurationS ? Math.round(st.maxDurationS / 60) : null
}

function collect(): SubSettings {
  const st = editing.value!.settings
  const split = (t: string) => t.split(/[,，\s]+/).map((x) => x.trim()).filter(Boolean)
  return {
    ...st,
    include: split(includeText.value),
    exclude: split(excludeText.value),
    minDurationS: minMin.value ? minMin.value * 60 : null,
    maxDurationS: maxMin.value ? maxMin.value * 60 : null,
  }
}

async function doPreview() {
  if (!editing.value?.url.trim()) return
  previewing.value = true
  preview.value = null
  previewErr.value = null
  try {
    preview.value = await api.subsPreview(editing.value.url)
    if (!editing.value.title) editing.value.title = preview.value.title
  } catch (e) {
    previewErr.value = { message: errorText(e), kind: errorKind(e) }
  } finally {
    previewing.value = false
  }
}

async function pickDir() {
  const dir = await open({ directory: true, title: '选择保存位置' })
  if (typeof dir === 'string' && editing.value) editing.value.settings.dir = dir
}

async function save() {
  const e = editing.value
  if (!e) return
  saving.value = true
  try {
    if (e.id === null) {
      await api.subsAdd(e.url, e.title || null, collect())
      ElMessage.success('已订阅，正在进行首次检查')
    } else {
      await api.subsUpdate(e.id, e.title, collect())
      ElMessage.success('已保存')
    }
    editing.value = null
    await load()
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    saving.value = false
  }
}

// ---------- 详情 ----------

const detail = ref<Subscription | null>(null)
const tab = ref<'pending' | 'downloaded' | 'ignored' | 'failed'>('pending')
const items = ref<SubItem[]>([])
const picked = ref<Set<string>>(new Set())
const TAB_STATUSES: Record<string, string[]> = { pending: ['pending'], downloaded: ['downloaded', 'queued'], ignored: ['ignored', 'seen'], failed: ['failed'] }

async function openDetail(s: Subscription) {
  detail.value = s
  tab.value = s.pending ? 'pending' : 'downloaded'
  await loadItems()
  if (s.newCount) api.subsClearNew(s.id).then(load).catch(() => {})
}

async function loadItems() {
  if (!detail.value) return
  picked.value = new Set()
  items.value = await api.subsItems(detail.value.id, TAB_STATUSES[tab.value]).catch(() => [])
}

function toggle(id: string) {
  const n = new Set(picked.value)
  if (n.has(id)) n.delete(id)
  else n.add(id)
  picked.value = n
}

const allPicked = computed(() => items.value.length > 0 && picked.value.size === items.value.length)

function pickAll() {
  picked.value = allPicked.value ? new Set() : new Set(items.value.map((i) => i.itemId))
}

async function downloadPicked() {
  if (!detail.value || !picked.value.size) return
  const n = await api.subsDownloadItems(detail.value.id, [...picked.value]).catch((e) => {
    ElMessage.error(errorText(e))
    return 0
  })
  if (n) ElMessage.success(`正在加入下载队列：${n} 项`)
  await loadItems()
}

async function ignorePicked() {
  if (!detail.value || !picked.value.size) return
  await run(api.subsIgnoreItems, detail.value.id, [...picked.value])
  await loadItems()
}

const ITEM_STATUS: Record<string, string> = { seen: '订阅前已发布', pending: '待下载', queued: '下载中', downloaded: '已下载', ignored: '已忽略', failed: '失败' }
</script>

<template>
  <div class="page">
    <div class="head">
      <h2>订阅</h2>
      <span class="mute small">关注频道、UP 主、画师、合集，有新内容自动下载</span>
      <span class="spacer" />
      <el-button v-if="app.settings?.subscriptionsEnabled" type="primary" size="small" @click="openAdd">添加订阅</el-button>
    </div>

    <section v-if="!app.settings?.subscriptionsEnabled" class="card intro">
      <h3>订阅与追更</h3>
      <p>添加 YouTube 频道 / 播放列表、B站 UP 主空间、Pixiv 画师、抖音用户主页，或任何 yt-dlp 能列出条目的列表页，清影会定期检查，有新内容时自动下载。</p>
      <ul class="mute">
        <li>每个订阅单独设置检查间隔（默认 6 小时）、过滤条件、清晰度和保存位置</li>
        <li>首次订阅可选择只下载以后的新内容、下载最近几条或全部</li>
        <li>已下载、已忽略的条目不会重复下载；连续失败会自动暂停并通知</li>
      </ul>
      <el-button type="primary" @click="enable">开启订阅</el-button>
    </section>

    <template v-else>
      <div v-if="!subs.length" class="empty mute">还没有订阅。点“添加订阅”，粘贴频道、UP 主空间或播放列表的链接。</div>
      <div class="list">
        <div v-for="s in subs" :key="s.id" class="sub card" :class="s.status">
          <img v-if="s.avatar" :src="s.avatar" class="avatar" referrerpolicy="no-referrer" alt="" />
          <div v-else class="avatar initial">{{ s.title.slice(0, 1) }}</div>
          <div class="info">
            <div class="title">
              <span class="ellipsis" :title="s.title">{{ s.title }}</span>
              <span class="chip">{{ s.platformName }}</span>
              <span v-if="s.newCount" class="chip acc">{{ s.newCount }} 个新内容</span>
            </div>
            <small class="mute">
              <span :class="{ err: s.status === 'error', ok: s.status === 'active' && !s.checking }">{{ statusText(s) }}</span>
              · 上次检查 {{ relative(s.lastCheck) }}<template v-if="s.status === 'active'"> · 下次 {{ relative(s.nextCheck) }}</template>
              · 每 {{ s.settings.intervalHours }} 小时 · 已下载 {{ s.downloaded }}<template v-if="s.pending"> · 待确认 {{ s.pending }}</template>
            </small>
            <small v-if="s.lastError" class="err ellipsis" :title="s.lastError">{{ s.lastError }}</small>
          </div>
          <div class="actions">
            <el-button link size="small" type="primary" :loading="s.checking" @click="checkNow(s)">立即检查</el-button>
            <el-button link size="small" @click="openDetail(s)">内容</el-button>
            <el-button v-if="s.status === 'active'" link size="small" @click="run(api.subsSetPaused, s.id, true)">暂停</el-button>
            <el-button v-else link size="small" type="primary" @click="run(api.subsSetPaused, s.id, false)">{{ s.status === 'error' ? '恢复' : '继续' }}</el-button>
            <el-button link size="small" @click="openEdit(s)">编辑</el-button>
            <el-button link size="small" @click="remove(s)">删除</el-button>
          </div>
        </div>
      </div>
    </template>

    <el-dialog :model-value="!!editing" :title="editing?.id === null ? '添加订阅' : '编辑订阅'" width="620px" append-to-body @close="editing = null">
      <div v-if="editing" class="form">
        <div v-if="editing.id === null" class="row">
          <el-input v-model="editing.url" placeholder="频道、UP 主空间、画师主页、播放列表或合集的链接" class="grow mono" @keyup.enter="doPreview" />
          <el-button :loading="previewing" @click="doPreview">预览</el-button>
        </div>
        <ErrorAlert v-if="previewErr" :message="previewErr.message" :kind="previewErr.kind" compact />
        <div v-if="preview" class="preview">
          <b>{{ preview.title }}</b> <span class="mute small">· {{ preview.platformName }} · 最新 {{ preview.entries.length }} 条</span>
          <ol>
            <li v-for="e in preview.entries.slice(0, 5)" :key="e.id" class="ellipsis">{{ e.title }}<span v-if="e.publishedAt" class="mute"> · {{ formatDateTime(e.publishedAt) }}</span></li>
          </ol>
        </div>
        <div class="grid">
          <label>名称</label>
          <el-input v-model="editing.title" size="small" placeholder="留空则使用页面标题" />
          <label>检查间隔</label>
          <div class="row"><el-input-number v-model="editing.settings.intervalHours" :min="1" :max="168" size="small" /> <span class="mute small">小时（会随机提前或推迟几分钟）</span></div>
          <template v-if="editing.id === null">
            <label>首次订阅</label>
            <div class="row">
              <el-radio-group v-model="editing.settings.firstRun" size="small">
                <el-radio-button value="new_only">只下载以后的新内容</el-radio-button>
                <el-radio-button value="latest">下载最近几条</el-radio-button>
                <el-radio-button value="all">下载全部</el-radio-button>
              </el-radio-group>
              <el-input-number v-if="editing.settings.firstRun === 'latest'" v-model="editing.settings.firstN" :min="1" :max="100" size="small" />
            </div>
          </template>
          <label>单次最多自动下载</label>
          <div class="row"><el-input-number v-model="editing.settings.maxAuto" :min="1" :max="200" size="small" /> <span class="mute small">条，超出的进入“待确认”</span></div>
          <label>标题包含</label>
          <el-input v-model="includeText" size="small" placeholder="多个关键词用逗号分隔，满足任一即可；留空不限" />
          <label>标题不含</label>
          <el-input v-model="excludeText" size="small" placeholder="例如：预告，直播回放" />
          <label>时长</label>
          <div class="row">
            <el-input-number v-model="minMin" :min="0" :max="1440" size="small" placeholder="不限" /> <span class="mute small">到</span>
            <el-input-number v-model="maxMin" :min="0" :max="1440" size="small" placeholder="不限" /> <span class="mute small">分钟</span>
          </div>
          <label>只下载最近</label>
          <div class="row"><el-input-number v-model="editing.settings.maxAgeDays" :min="0" :max="3650" size="small" placeholder="不限" /> <span class="mute small">天内发布的</span></div>
          <label>只保留最近</label>
          <div class="row">
            <el-input-number v-model="editing.settings.keepLatest" :min="0" :max="100000" size="small" /> <span class="mute small">个作品</span>
            <el-input-number v-model="editing.settings.keepDays" :min="0" :max="36500" size="small" /> <span class="mute small">天内下载的（0 表示不清理；超出的移到回收站）</span>
          </div>
          <label>清晰度</label>
          <el-select v-model="editing.settings.quality" size="small" clearable placeholder="跟随全局设置">
            <el-option value="best" label="最高画质" />
            <el-option value="max1080" label="不超过 1080P" />
            <el-option value="small" label="省空间（≤720P）" />
            <el-option value="audio" label="只要音频" />
          </el-select>
          <label>保存位置</label>
          <div class="row">
            <el-input v-model="editing.settings.dir" size="small" class="grow" placeholder="留空：下载目录 / 订阅名称" />
            <el-button size="small" @click="pickDir">选择…</el-button>
          </div>
          <label>文件命名</label>
          <el-input v-model="editing.settings.template" size="small" class="mono" placeholder="留空使用全局模板，例如 {date}_{title}" />
          <label>通知</label>
          <el-checkbox v-model="editing.settings.notify">有新内容时发送通知</el-checkbox>
        </div>
      </div>
      <template #footer>
        <el-button @click="editing = null">取消</el-button>
        <el-button type="primary" :loading="saving" :disabled="!editing?.url.trim()" @click="save">{{ editing?.id === null ? '订阅' : '保存' }}</el-button>
      </template>
    </el-dialog>

    <el-drawer :model-value="!!detail" :title="detail?.title" size="560px" append-to-body @close="detail = null">
      <el-radio-group v-model="tab" size="small" @change="loadItems">
        <el-radio-button value="pending">待确认</el-radio-button>
        <el-radio-button value="downloaded">已下载</el-radio-button>
        <el-radio-button value="ignored">已忽略</el-radio-button>
        <el-radio-button value="failed">失败</el-radio-button>
      </el-radio-group>
      <div class="itools">
        <el-checkbox :model-value="allPicked" :disabled="!items.length" @change="pickAll">全选</el-checkbox>
        <span class="spacer" />
        <el-button size="small" type="primary" :disabled="!picked.size" @click="downloadPicked">下载所选</el-button>
        <el-button v-if="tab !== 'ignored'" size="small" :disabled="!picked.size" @click="ignorePicked">忽略所选</el-button>
      </div>
      <div v-if="!items.length" class="empty mute">没有内容</div>
      <label v-for="i in items" :key="i.itemId" class="item">
        <el-checkbox :model-value="picked.has(i.itemId)" @change="toggle(i.itemId)" />
        <div class="iinfo">
          <span class="ellipsis" :title="i.title">{{ i.title }}</span>
          <small class="mute">
            {{ ITEM_STATUS[i.status] }}<template v-if="i.publishedAt"> · {{ formatDateTime(i.publishedAt) }}</template><template v-if="i.durationMs"> · {{ formatDuration(i.durationMs) }}</template><template v-if="i.reason && i.reason !== ITEM_STATUS[i.status]"> · {{ i.reason }}</template>
          </small>
        </div>
        <el-button link size="small" @click.prevent="api.openUrl(i.url)">原页面</el-button>
      </label>
    </el-drawer>
  </div>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 980px;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 10px;
}
h2 {
  margin: 0;
  font-size: 16px;
}
.spacer {
  flex: 1;
}
.small {
  font-size: 12px;
}
.intro {
  padding: 18px 20px;
}
.intro h3 {
  margin: 0 0 8px;
  font-size: 14px;
}
.intro p {
  margin: 0 0 6px;
  line-height: 1.7;
}
.intro ul {
  margin: 0 0 14px;
  padding-left: 18px;
  font-size: 12.5px;
  line-height: 1.8;
}
.empty {
  padding: 40px 0;
  text-align: center;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.sub {
  display: grid;
  grid-template-columns: 44px 1fr auto;
  gap: 12px;
  align-items: center;
  padding: 10px 12px;
}
.sub.paused,
.sub.error {
  opacity: 0.85;
}
.avatar {
  width: 44px;
  height: 44px;
  border-radius: 50%;
  object-fit: cover;
  background: var(--cc-line);
}
.initial {
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  color: var(--cc-acc);
}
.info {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.title {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.title .ellipsis {
  min-width: 0;
}
.info small {
  font-size: 11px;
}
.err {
  color: var(--cc-err);
}
.ok {
  color: var(--cc-ok);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  max-width: 230px;
}
.actions :deep(.el-button) {
  margin-left: 8px;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.grow {
  flex: 1;
  min-width: 0;
}
.grid {
  display: grid;
  grid-template-columns: 110px 1fr;
  gap: 10px 12px;
  align-items: center;
}
.grid label {
  font-size: 12.5px;
  color: var(--cc-mute);
}
.preview {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 10px 12px;
}
.preview ol {
  margin: 6px 0 0;
  padding-left: 18px;
  font-size: 12.5px;
}
.itools {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 12px 0 8px;
}
.itools :deep(.el-button) {
  margin-left: 0;
}
.item {
  display: grid;
  grid-template-columns: 24px 1fr auto;
  gap: 8px;
  align-items: center;
  padding: 6px 0;
  border-top: 1px dashed var(--cc-line);
  cursor: pointer;
}
.iinfo {
  display: flex;
  flex-direction: column;
  min-width: 0;
  font-size: 12.5px;
}
.iinfo small {
  font-size: 11px;
}
</style>
