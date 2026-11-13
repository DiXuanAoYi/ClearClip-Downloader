<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { BatchItem, BatchOpts, PreviewItem, PreviewStatus } from '../types'
import { formatBytes, formatDuration } from '../utils/format'
import { siteName } from '../utils/siteGuides'

const MAX = 500
const app = useAppStore()

const visible = ref(false)
const source = ref('粘贴')
const truncated = ref(false)
const urls = ref<string[]>([])
const rows = ref<Map<string, PreviewItem>>(new Map())
const finished = ref(true)
const key = ref('')
const picked = ref<Set<string>>(new Set())
const filter = ref<'all' | 'ok' | 'dup' | 'done' | 'login' | 'bad' | 'wait'>('all')
const more = ref('')
const showMore = ref(false)
const busy = ref(false)
const ranges = ref<Record<string, [number, number]>>({})
const opts = ref<BatchOpts>(defaults())
/** 整批设置默认折成一行摘要，点“修改”才展开 */
const showSet = ref(false)
const emit = defineEmits<{ active: [boolean]; added: [] }>()
watch(visible, (v) => emit('active', v))

function defaults(): BatchOpts {
  const s = app.settings
  return {
    name: '',
    dir: '',
    folder: false,
    quality: '',
    concurrency: Math.min(5, s?.perSiteConcurrency || 2),
    jitterMin: s?.batchJitterMin ?? 2,
    jitterMax: s?.batchJitterMax ?? 6,
    speedLimitKbps: 0,
    afterDone: 'notify',
    useWindow: false,
    startPaused: false,
    template: '',
  }
}

let unlisten: (() => void) | null = null
events
  .onBatchPreview((p) => {
    if (!visible.value || p.key !== key.value) return
    if ('finished' in p) {
      finished.value = true
      return
    }
    const it = p as PreviewItem
    rows.value.set(it.url, it)
    rows.value = new Map(rows.value)
    if (it.status === 'ok' || it.status === 'playlist' || it.status === 'file') picked.value.add(it.url)
    else if (it.status === 'dup' || it.status === 'done' || it.status === 'queued') picked.value.delete(it.url)
    picked.value = new Set(picked.value)
    if (it.status === 'playlist') ranges.value[it.url] = [1, it.entries.length]
  })
  .then((u) => (unlisten = u))
onBeforeUnmount(() => {
  unlisten?.()
  stopPreview()
})

function takeRequest() {
  const r = app.batchRequest
  if (!r) return
  app.batchRequest = null
  start(r.urls, r.source, !!r.truncated)
}
watch(() => app.batchRequest, takeRequest)
onMounted(takeRequest)

function stopPreview() {
  if (key.value) api.batchPreviewCancel(key.value).catch(() => {})
}

async function start(list: string[], src: string, cut: boolean) {
  stopPreview()
  urls.value = list.slice(0, MAX)
  source.value = src
  truncated.value = cut || list.length > MAX
  rows.value = new Map()
  picked.value = new Set()
  ranges.value = {}
  filter.value = 'all'
  showMore.value = list.length === 0
  more.value = ''
  opts.value = defaults()
  opts.value.folder = urls.value.length >= (app.settings?.batchFolderMin ?? 3)
  if (app.settings?.batchPreview === false) {
    // 设置里关了“先预览”：不打开面板，直接按默认选项整批加入，逐条解析
    busy.value = true
    try {
      const r = await api.batchEnqueue(urls.value.map((u) => ({ url: u, title: '', media: null })), { ...opts.value, afterDone: 'notify' }, src)
      ElMessage.success(`已建立批次“${r.name}”，${r.pending} 条正在逐条解析并加入队列`)
      emit('added')
    } catch (e) {
      ElMessage.error(errorText(e))
    } finally {
      busy.value = false
    }
    return
  }
  visible.value = true
  if (truncated.value) ElMessage.warning(`一次最多 ${MAX} 条，多出来的请分批添加。`)
  await runPreview()
}

async function runPreview() {
  key.value = `p${Date.now()}`
  finished.value = false
  try {
    await api.batchPreview(key.value, urls.value, opts.value.quality)
  } catch (e) {
    finished.value = true
    ElMessage.error(errorText(e))
  }
}

async function addMore() {
  const found = await api.batchExtract(more.value)
  const have = new Set(urls.value)
  const fresh = found.filter((u) => !have.has(u))
  if (!fresh.length) {
    ElMessage.info('没有新的链接')
    return
  }
  if (urls.value.length + fresh.length > MAX) {
    ElMessage.warning(`一次最多 ${MAX} 条，已只加入前面的部分。`)
  }
  const add = fresh.slice(0, MAX - urls.value.length)
  urls.value = [...urls.value, ...add]
  more.value = ''
  showMore.value = false
  stopPreview()
  // 只解析新增的，已有结果保留
  key.value = `p${Date.now()}`
  finished.value = false
  await api.batchPreview(key.value, add, opts.value.quality).catch((e) => ElMessage.error(errorText(e)))
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
    more.value = list.join('\n')
    if (cut) ElMessage.warning(`文件里的链接超过 ${MAX} 条，只取前 ${MAX} 条。`)
    await addMore()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function pickDir() {
  const p = await open({ directory: true, title: '选择保存位置', defaultPath: opts.value.dir || undefined })
  if (typeof p === 'string') opts.value.dir = p
}

const list = computed(() => urls.value.map((u, i) => ({ url: u, i, row: rows.value.get(u) ?? null })))
const statusOf = (r: PreviewItem | null): PreviewStatus | 'wait' => (r ? r.status : 'wait')
const counts = computed(() => {
  const c = { all: list.value.length, ok: 0, dup: 0, done: 0, login: 0, bad: 0, wait: 0 }
  for (const x of list.value) {
    const s = statusOf(x.row)
    if (s === 'ok' || s === 'playlist' || s === 'file') c.ok++
    else if (s === 'dup' || s === 'queued') c.dup++
    else if (s === 'done') c.done++
    else if (s === 'login') c.login++
    else if (s === 'bad') c.bad++
    else c.wait++
  }
  return c
})
const shown = computed(() =>
  list.value.filter((x) => {
    const s = statusOf(x.row)
    switch (filter.value) {
      case 'all':
        return true
      case 'ok':
        return s === 'ok' || s === 'playlist' || s === 'file'
      case 'dup':
        return s === 'dup' || s === 'queued'
      default:
        return s === filter.value
    }
  }),
)
const parsed = computed(() => urls.value.length - counts.value.wait)

function selectable(r: PreviewItem | null) {
  return !!r && (r.status === 'ok' || r.status === 'playlist' || r.status === 'file' || r.status === 'done' || r.status === 'dup' || r.status === 'queued')
}
function toggle(u: string, on: boolean) {
  if (on) picked.value.add(u)
  else picked.value.delete(u)
  picked.value = new Set(picked.value)
}
function selectAllOk() {
  const s = new Set<string>()
  for (const x of list.value) if (x.row && (x.row.status === 'ok' || x.row.status === 'playlist' || x.row.status === 'file')) s.add(x.url)
  picked.value = s
}

const pickedCount = computed(() => {
  let n = 0
  for (const u of picked.value) {
    const r = rows.value.get(u)
    if (r?.status === 'playlist') {
      const [a, b] = ranges.value[u] ?? [1, r.entries.length]
      n += Math.max(0, Math.min(b, r.entries.length) - Math.max(a, 1) + 1)
    } else n++
  }
  return n
})
const pickedSize = computed(() => {
  let n = 0
  for (const u of picked.value) n += rows.value.get(u)?.sizeEst ?? 0
  return n
})

const QUALITIES = [
  { v: '', l: '跟随设置' },
  { v: 'best', l: '最高' },
  { v: '1080', l: '1080p' },
  { v: '720', l: '720p' },
  { v: 'audio', l: '仅音频' },
]

function buildItems(): BatchItem[] {
  const out: BatchItem[] = []
  for (const x of list.value) {
    if (!picked.value.has(x.url)) continue
    const r = x.row
    if (r?.status === 'playlist') {
      const [a, b] = ranges.value[x.url] ?? [1, r.entries.length]
      for (const e of r.entries.slice(Math.max(a, 1) - 1, b)) out.push({ url: e.url, title: e.title, media: null })
    } else out.push({ url: x.url, title: r?.title ?? '', media: r?.media ?? null })
  }
  return out
}

async function submit(paused: boolean) {
  const items = buildItems()
  if (!items.length) {
    ElMessage.info('还没有勾选要下载的链接')
    return
  }
  if (items.length > MAX) {
    ElMessage.warning(`勾选了 ${items.length} 条，超过一次 ${MAX} 条的上限，请取消一部分后分批添加。`)
    return
  }
  busy.value = true
  try {
    const r = await api.batchEnqueue(items, { ...opts.value, startPaused: paused }, source.value)
    stopPreview()
    visible.value = false
    const parts = [`已建立批次“${r.name}”：加入 ${r.queued} 条`]
    if (r.pending) parts.push(`${r.pending} 条稍后逐条解析`)
    if (r.alreadyDownloaded) parts.push(`${r.alreadyDownloaded} 条之前已下载`)
    if (r.alreadyQueued) parts.push(`${r.alreadyQueued} 条已在队列`)
    ElMessage.success(parts.join('，'))
    emit('added')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

function cancel() {
  stopPreview()
  visible.value = false
}
defineExpose({ cancel, active: visible })

const QUALITY_TEXT = computed(() => QUALITIES.find((q) => q.v === opts.value.quality)?.l ?? '跟随设置')
const AFTER_TEXT: Record<string, string> = { none: '完成后不操作', notify: '完成后通知', open: '完成后打开文件夹', sleep: '完成后睡眠', shutdown: '完成后关机' }
/** 设置摘要：折叠时显示的几枚小标签 */
const summary = computed(() => {
  const o = opts.value
  const out = [QUALITY_TEXT.value, `同时 ${o.concurrency} 个`, `间隔 ${o.jitterMin}–${o.jitterMax} 秒`, o.speedLimitKbps ? `限速 ${o.speedLimitKbps} KB/s` : '不限速']
  if (o.folder) out.push('放进批次文件夹')
  if (o.dir) out.push('保存到自选位置')
  if (o.useWindow) out.push('只在设定时段')
  out.push(AFTER_TEXT[o.afterDone] ?? '')
  return out.filter(Boolean)
})

const STATUS: Record<string, { t: string; c: string }> = {
  ok: { t: '可下载', c: 'ok' },
  file: { t: '文件', c: 'ok' },
  playlist: { t: '列表', c: 'info' },
  dup: { t: '重复', c: 'warn' },
  queued: { t: '已在队列', c: 'warn' },
  done: { t: '已下载过', c: 'warn' },
  login: { t: '需登录', c: 'err' },
  bad: { t: '不支持', c: 'err' },
  wait: { t: '解析中', c: 'mute' },
}
const site = (r: PreviewItem | null, u: string) => (r?.platformName || (r?.site ? siteName(r.site, app.info?.providers) : '') || hostOf(u))
function hostOf(u: string) {
  try {
    return new URL(u).hostname.replace(/^www\./, '')
  } catch {
    return u.slice(0, 12)
  }
}
</script>

<template>
  <section v-if="visible" class="bw">
    <div class="top">
      <div>
        识别到 <b>{{ urls.length }}</b> 条链接 <span class="mute">· 来源：{{ source }}</span>
        <span v-if="truncated" class="warn"> · 超过 {{ MAX }} 条的部分已忽略</span>
      </div>
      <div class="gap">
        <el-button size="small" @click="showMore = !showMore">粘贴更多</el-button>
        <el-button size="small" @click="importFile">导入文件…</el-button>
      </div>
    </div>
    <div v-if="showMore" class="more">
      <el-input v-model="more" type="textarea" :rows="3" placeholder="把更多链接粘贴到这里" />
      <el-button type="primary" size="small" @click="addMore">加入</el-button>
    </div>
    <el-progress :percentage="urls.length ? Math.round((parsed / urls.length) * 100) : 100" :stroke-width="5" :show-text="false" :status="finished ? 'success' : undefined" />
    <div class="mute small">{{ finished ? `已解析 ${parsed} / ${urls.length}` : `正在解析 ${parsed} / ${urls.length}（同一网站会放慢速度，避免被限制）` }}</div>

        <div class="card setc">
      <div class="sum">
        <b>整批设置</b>
        <span v-for="x in summary" :key="x" class="chip">{{ x }}</span>
        <span class="sp" />
        <a class="lk" @click="showSet = !showSet">{{ showSet ? '收起 ▴' : '修改 ▾' }}</a>
      </div>
      <div v-if="showSet" class="set">
          <label>批次名<el-input v-model="opts.name" size="small" placeholder="留空自动命名（批量-月日-时分）" /></label>
          <label>
            清晰度
            <el-select v-model="opts.quality" size="small" @change="runPreview">
              <el-option v-for="q in QUALITIES" :key="q.v" :value="q.v" :label="q.l" />
            </el-select>
          </label>
          <label>
            保存到
            <div class="dir">
              <el-input v-model="opts.dir" size="small" placeholder="留空用设置里的下载目录" />
              <el-button size="small" @click="pickDir">选择</el-button>
            </div>
          </label>
          <div class="kv"><span>放进批次文件夹</span><el-switch v-model="opts.folder" size="small" /></div>
          <label>
            同时下载数：{{ opts.concurrency }}
            <el-slider v-model="opts.concurrency" :min="1" :max="5" size="small" />
          </label>
          <label>
            条目间随机间隔（秒）
            <div class="dir">
              <el-input-number v-model="opts.jitterMin" :min="0" :max="opts.jitterMax" size="small" controls-position="right" />
              <span>—</span>
              <el-input-number v-model="opts.jitterMax" :min="opts.jitterMin" :max="120" size="small" controls-position="right" />
            </div>
          </label>
          <label>
            整批限速（KB/s，0 不限）
            <el-input-number v-model="opts.speedLimitKbps" :min="0" :max="1048576" :step="256" size="small" />
          </label>
          <label>
            批次完成后
            <el-select v-model="opts.afterDone" size="small">
              <el-option value="none" label="不操作" />
              <el-option value="notify" label="通知" />
              <el-option value="open" label="通知并打开文件夹" />
              <el-option value="sleep" label="睡眠" />
              <el-option value="shutdown" label="关机" />
            </el-select>
          </label>
          <div class="kv">
            <span>只在设定时段下载<small v-if="!app.settings?.runWindowOn" class="mute block">先在“设置 → 下载”里开启并设置时段</small></span>
            <el-switch v-model="opts.useWindow" size="small" :disabled="!app.settings?.runWindowOn" />
          </div>
          <small class="mute">文件名模板里可用 <code>{index}</code> 表示批次序号。</small>
        </div>
    </div>

    <div class="card listc">
        <div class="chips">
          <button :class="{ on: filter === 'all' }" @click="filter = 'all'">全部 {{ counts.all }}</button>
          <button :class="{ on: filter === 'ok' }" @click="filter = 'ok'">可下载 {{ counts.ok }}</button>
          <button v-if="counts.dup" :class="{ on: filter === 'dup' }" @click="filter = 'dup'">重复 {{ counts.dup }}</button>
          <button v-if="counts.done" :class="{ on: filter === 'done' }" @click="filter = 'done'">已下载过 {{ counts.done }}</button>
          <button v-if="counts.login" :class="{ on: filter === 'login' }" @click="filter = 'login'">需登录 {{ counts.login }}</button>
          <button v-if="counts.bad" :class="{ on: filter === 'bad' }" @click="filter = 'bad'">不支持 {{ counts.bad }}</button>
          <button v-if="counts.wait" :class="{ on: filter === 'wait' }" @click="filter = 'wait'">解析中 {{ counts.wait }}</button>
          <span class="sp" />
          <el-button link size="small" @click="selectAllOk">全选可下载</el-button>
          <el-button link size="small" @click="picked = new Set()">清除勾选</el-button>
        </div>
        <div class="rows">
          <div v-for="x in shown" :key="x.url" class="row" :class="statusOf(x.row)">
            <el-checkbox :model-value="picked.has(x.url)" :disabled="!selectable(x.row)" @update:model-value="(v: unknown) => toggle(x.url, !!v)" />
            <span class="idx mono">{{ x.i + 1 }}</span>
            <span class="badge">{{ site(x.row, x.url) }}</span>
            <div class="mid">
              <div class="ttl ellipsis">
                <template v-if="x.row?.title">{{ x.row.title }}</template>
                <span v-else class="sk" />
              </div>
              <div class="url mono ellipsis" :title="x.url">{{ x.url }}</div>
              <div v-if="x.row?.error" class="errl ellipsis" :title="x.row.error">{{ x.row.error }}</div>
              <div v-if="x.row?.status === 'playlist' && picked.has(x.url)" class="rng">
                展开 {{ x.row.entries.length }} 项，下载第
                <el-input-number v-model="ranges[x.url][0]" :min="1" :max="x.row.entries.length" size="small" controls-position="right" />
                到
                <el-input-number v-model="ranges[x.url][1]" :min="1" :max="x.row.entries.length" size="small" controls-position="right" />
                项
              </div>
            </div>
            <span class="meta mono">
              <template v-if="x.row">{{ [x.row.durationMs ? formatDuration(x.row.durationMs) : '', x.row.quality, x.row.sizeEst ? '≈' + formatBytes(x.row.sizeEst) : ''].filter(Boolean).join(' · ') }}</template>
            </span>
            <span class="pill" :class="STATUS[statusOf(x.row)].c">{{ STATUS[statusOf(x.row)].t }}</span>
            <el-button v-if="x.row?.status === 'login'" link size="small" type="primary" @click="app.goLogin(x.row.site)">去登录</el-button>
          </div>
          <div v-if="!shown.length" class="mute empty">这个分类下没有链接</div>
        </div>
      </div>

    <div class="foot">
        <span class="mute">已勾选 <b>{{ pickedCount }}</b> 条<template v-if="pickedSize"> · 预计 {{ formatBytes(pickedSize) }}</template></span>
        <span class="sp" />
        <el-button @click="cancel">取消</el-button>
        <el-button :loading="busy" @click="submit(true)">先暂停地加入</el-button>
        <el-button type="primary" :loading="busy" :disabled="!pickedCount" @click="submit(false)">加入队列并开始</el-button>
    </div>
  </section>
</template>

<style scoped>
.top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  flex-wrap: wrap;
}
.gap {
  display: flex;
  gap: 6px;
}
.more {
  display: flex;
  gap: 8px;
  align-items: flex-end;
  margin-bottom: 8px;
}
.small {
  font-size: 11.5px;
  margin: 4px 0 8px;
}
.warn {
  color: #d9a441;
}
.bw {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
.card {
  background: var(--cc-card);
  border: 1px solid var(--cc-line);
  border-radius: 14px;
}
.setc {
  padding: 10px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.sum {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: 13px;
}
.chip {
  font-size: 12px;
  padding: 2px 10px;
  border-radius: 999px;
  border: 1px solid var(--cc-line);
  color: var(--cc-mute);
}
.lk {
  cursor: pointer;
  color: var(--cc-acc);
  font-weight: 600;
  font-size: 12.5px;
}
.set {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px 14px;
  font-size: 12.5px;
}
.set label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.set .kv {
  align-self: end;
}
.listc {
  padding: 10px 12px;
  min-width: 0;
}
.chips {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  align-items: center;
  margin-bottom: 8px;
}
.chips button {
  border: 1px solid var(--cc-line);
  background: var(--cc-card);
  color: var(--cc-fg);
  border-radius: 12px;
  padding: 2px 10px;
  font-size: 12px;
  cursor: pointer;
}
.chips button.on {
  border-color: var(--cc-acc);
  color: var(--cc-acc);
}
.sp {
  flex: 1;
}
.rows {
  max-height: 52vh;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
}
.row {
  display: grid;
  grid-template-columns: 22px 28px 76px 1fr auto 74px auto;
  gap: 8px;
  align-items: center;
  padding: 6px 10px;
  border-bottom: 1px solid var(--cc-line);
  font-size: 12.5px;
}
.row:last-child {
  border-bottom: 0;
}
.row.done,
.row.dup,
.row.queued {
  opacity: 0.75;
}
.idx {
  color: var(--cc-mute);
  font-size: 11px;
  text-align: right;
}
.badge {
  font-size: 11px;
  color: var(--cc-mute);
  border: 1px solid var(--cc-line);
  border-radius: 4px;
  padding: 0 4px;
  text-align: center;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mid {
  min-width: 0;
}
.ttl {
  font-weight: 500;
}
.url {
  font-size: 10.5px;
  color: var(--cc-mute);
}
.errl {
  font-size: 11px;
  color: var(--cc-err);
}
.sk {
  display: inline-block;
  width: 55%;
  height: 10px;
  border-radius: 4px;
  background: var(--cc-line);
}
.meta {
  font-size: 10.5px;
  color: var(--cc-mute);
  white-space: nowrap;
}
.pill {
  font-size: 11px;
  text-align: center;
  border-radius: 10px;
  padding: 1px 6px;
  border: 1px solid var(--cc-line);
  color: var(--cc-mute);
}
.pill.ok {
  color: var(--cc-ok);
  border-color: var(--cc-ok);
}
.pill.warn {
  color: #d9a441;
  border-color: #d9a441;
}
.pill.err {
  color: var(--cc-err);
  border-color: var(--cc-err);
}
.pill.info {
  color: var(--cc-acc);
  border-color: var(--cc-acc);
}
.rng {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11.5px;
  margin-top: 4px;
}
.rng :deep(.el-input-number) {
  width: 88px;
}
.empty {
  padding: 24px;
  text-align: center;
}
.dir {
  display: flex;
  gap: 6px;
  align-items: center;
}
.kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
}
.block {
  display: block;
}
.foot {
  position: sticky;
  bottom: 0;
  z-index: 5;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 4px 4px;
  margin: 0 -4px;
  background: var(--cc-bg, var(--cc-bg));
  border-top: 1px solid var(--cc-line);
}
@media (max-width: 900px) {
  .set {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
