<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorKind, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { ErrorKind, LiveRoom, LiveSettings, LiveStatus, NormPreset, Recording } from '../types'
import { formatBytes, formatDateTime } from '../utils/format'
import ErrorAlert from '../components/ErrorAlert.vue'

const app = useAppStore()
const rooms = ref<LiveRoom[]>([])
const tick = ref(Date.now())
let unlisten: UnlistenFn | undefined
let timer: number | undefined

const DEFAULTS: LiveSettings = { autoRecord: true, quality: '', checkIntervalS: 60, segmentMinutes: 60, segmentMb: 0, convertMp4: false, mergeSegments: false, normalize: '', dir: '', notify: true, schedule: [] }

async function load() {
  try {
    rooms.value = await api.liveRooms()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

const presets = ref<NormPreset[]>([])

onMounted(async () => {
  presets.value = await api.videoPresets().catch(() => [])
  await load()
  unlisten = await events.onLive(load)
  // 录制中刷新已录时长和大小
  timer = window.setInterval(() => {
    tick.value = Date.now()
    if (rooms.value.some((r) => r.state === 'recording')) load()
  }, 2000)
})
onUnmounted(() => {
  unlisten?.()
  window.clearInterval(timer)
})

async function run<A extends unknown[]>(fn: (...args: A) => Promise<unknown>, ...args: A) {
  try {
    await fn(...args)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function elapsed(r: LiveRoom) {
  if (!r.recStarted) return ''
  const s = Math.max(0, Math.floor(tick.value / 1000 - r.recStarted))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return `${h}:${String(m).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
}

const DAYS = ['一', '二', '三', '四', '五', '六', '日']
const STATE: Record<string, string> = { offline: '未开播', live: '直播中', recording: '录制中', error: '异常', checking: '检测中…', scheduled: '等待预约时段' }

async function remove(r: LiveRoom) {
  try {
    await ElMessageBox.confirm(`删除直播间“${r.streamer}”？已录制的文件不受影响。`, '删除直播间', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' })
  } catch {
    return
  }
  await run(api.liveDelete, r.id)
}

async function setMax(v: number | undefined) {
  if (v) await app.patch({ liveMaxRecordings: v })
}

// ---------- 添加 / 编辑 ----------

const editing = ref<{ id: number | null; url: string; streamer: string; settings: LiveSettings; qualities: string[] } | null>(null)
const preview = ref<LiveStatus | null>(null)
const previewErr = ref<{ message: string; kind: ErrorKind } | null>(null)
const checking = ref(false)
const saving = ref(false)

function openAdd() {
  editing.value = { id: null, url: '', streamer: '', settings: { ...DEFAULTS }, qualities: [] }
  preview.value = null
  previewErr.value = null
}

function openEdit(r: LiveRoom) {
  editing.value = { id: r.id, url: r.url, streamer: r.streamer, settings: { ...r.settings }, qualities: r.qualities }
  preview.value = null
  previewErr.value = null
}

async function doCheck() {
  if (!editing.value?.url.trim()) return
  checking.value = true
  preview.value = null
  previewErr.value = null
  try {
    preview.value = await api.liveCheck(editing.value.url)
    editing.value.qualities = [...new Set(preview.value.streams.sort((a, b) => b.rank - a.rank).map((s) => s.quality))]
  } catch (e) {
    previewErr.value = { message: errorText(e), kind: errorKind(e) }
  } finally {
    checking.value = false
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
      await api.liveAdd(e.url, e.settings)
      ElMessage.success('已添加，开始监控')
    } else {
      await api.liveUpdate(e.id, e.streamer, e.settings)
    }
    editing.value = null
    await load()
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    saving.value = false
  }
}

// ---------- 录制记录 ----------

const recsFor = ref<LiveRoom | null>(null)
const recs = ref<Recording[]>([])

async function openRecs(r: LiveRoom) {
  recsFor.value = r
  recs.value = await api.liveRecordings(r.id).catch(() => [])
}

const REC_STATUS: Record<string, string> = { recording: '录制中', done: '已完成', error: '异常结束', interrupted: '程序退出时中断' }
</script>

<template>
  <div class="page">
    <div class="head">
      <h2>直播</h2>
      <span class="mute small">开播自动录制，下播自动结束</span>
      <span class="spacer" />
      <span class="mute small">自动删除</span>
      <el-input-number :model-value="app.settings?.liveCleanupDays ?? 0" :min="0" :max="3650" size="small" @change="(v: number | undefined) => app.patch({ liveCleanupDays: v ?? 0 })" />
      <span class="mute small">天前的录像（0 不删）</span>
      <span class="mute small">同时录制上限</span>
      <el-input-number :model-value="app.settings?.liveMaxRecordings ?? 3" :min="1" :max="10" size="small" @change="setMax" />
      <el-button type="primary" size="small" @click="openAdd">添加直播间</el-button>
    </div>

    <div v-if="!rooms.length" class="empty mute">
      还没有直播间。点“添加直播间”，粘贴直播间链接（B站、抖音、快手、虎牙原生支持；斗鱼、YouTube、Twitch 等通过 yt-dlp；也可以直接填 .flv / .m3u8 直播流地址）。
    </div>

    <div class="grid">
      <div v-for="r in rooms" :key="r.id" class="room card" :class="r.state">
        <div class="cover">
          <img v-if="r.cover" :src="r.cover" referrerpolicy="no-referrer" alt="" />
          <span class="badge" :class="r.state">{{ STATE[r.state] ?? r.state }}<template v-if="r.state === 'recording'"> {{ elapsed(r) }}</template></span>
          <span v-if="!r.monitoring" class="badge paused">监控已暂停</span>
        </div>
        <div class="body">
          <div class="who">
            <img v-if="r.avatar" :src="r.avatar" class="avatar" referrerpolicy="no-referrer" alt="" />
            <b class="ellipsis">{{ r.streamer }}</b>
            <span class="chip">{{ r.platformName }}</span>
          </div>
          <div class="title ellipsis" :title="r.title">{{ r.title || '—' }}</div>
          <small v-if="r.state === 'recording'" class="mono mute">已录 {{ formatBytes(r.recBytes) }} · 第 {{ r.recSegments }} 段</small>
          <small v-else class="mute">{{ r.lastCheck ? `上次检测 ${formatDateTime(r.lastCheck)}` : '等待检测' }} · 每 {{ r.settings.checkIntervalS }} 秒<template v-if="!r.settings.autoRecord"> · 只提醒不录制</template></small>
          <small v-if="r.error" class="err ellipsis" :title="r.error">{{ r.error }}</small>
          <div class="actions">
            <el-button v-if="r.state === 'recording'" size="small" type="danger" plain @click="run(api.liveStop, r.id)">停止录制</el-button>
            <el-button v-else size="small" type="primary" plain :disabled="r.state === 'offline' && !!r.lastCheck" @click="run(api.liveStart, r.id)">立即录制</el-button>
            <el-button v-if="r.recFile" size="small" link @click="run(api.revealFile, r.recFile!)">文件夹</el-button>
            <el-button size="small" link @click="run(api.liveSetMonitoring, r.id, !r.monitoring)">{{ r.monitoring ? '暂停监控' : '继续监控' }}</el-button>
            <el-button size="small" link @click="openRecs(r)">录像</el-button>
            <el-button size="small" link @click="openEdit(r)">编辑</el-button>
            <el-button size="small" link @click="remove(r)">删除</el-button>
          </div>
        </div>
      </div>
    </div>

    <el-dialog :model-value="!!editing" :title="editing?.id === null ? '添加直播间' : '编辑直播间'" width="560px" append-to-body @close="editing = null">
      <div v-if="editing" class="form">
        <div v-if="editing.id === null" class="row">
          <el-input v-model="editing.url" placeholder="直播间链接，例如 https://live.bilibili.com/123" class="grow mono" @keyup.enter="doCheck" />
          <el-button :loading="checking" @click="doCheck">检测</el-button>
        </div>
        <ErrorAlert v-if="previewErr" :message="previewErr.message" :kind="previewErr.kind" compact />
        <div v-if="preview" class="preview">
          <b>{{ preview.streamer || preview.roomId }}</b>
          <span class="chip">{{ preview.platformName }}</span>
          <span :class="preview.live ? 'okc' : 'mute'">{{ preview.live ? '● 直播中' : '未开播' }}</span>
          <div class="mute small ellipsis">{{ preview.title }}</div>
        </div>
        <div v-if="editing" class="fgrid">
          <label v-if="editing.id !== null">主播名</label>
          <el-input v-if="editing.id !== null" v-model="editing.streamer" size="small" />
          <label>开播时</label>
          <el-radio-group v-model="editing.settings.autoRecord" size="small">
            <el-radio-button :value="true">自动录制</el-radio-button>
            <el-radio-button :value="false">只发通知</el-radio-button>
          </el-radio-group>
          <label>画质</label>
          <el-select v-model="editing.settings.quality" size="small" clearable placeholder="最高画质">
            <el-option v-for="q in editing.qualities" :key="q" :value="q" :label="q" />
          </el-select>
          <label>检测间隔</label>
          <div class="row"><el-input-number v-model="editing.settings.checkIntervalS" :min="30" :max="3600" :step="30" size="small" /> <span class="mute small">秒（每个平台有下限）</span></div>
          <label>自动分段</label>
          <div class="row">
            每 <el-input-number v-model="editing.settings.segmentMinutes" :min="0" :max="1440" :step="30" size="small" /> 分钟 或
            <el-input-number v-model="editing.settings.segmentMb" :min="0" :max="102400" :step="512" size="small" /> MB
            <span class="mute small">（0 表示不限）</span>
          </div>
          <label>预约时段</label>
          <div class="col">
            <div v-for="(w, i) in editing.settings.schedule" :key="i" class="win">
              <el-checkbox-group v-model="w.days" size="small">
                <el-checkbox-button v-for="(d, k) in DAYS" :key="k" :value="k + 1">{{ d }}</el-checkbox-button>
              </el-checkbox-group>
              <el-time-select v-model="w.start" size="small" class="tsel" start="00:00" end="23:30" step="00:30" :clearable="false" />
              <span class="mute small">到</span>
              <el-time-select v-model="w.end" size="small" class="tsel" start="00:00" end="23:30" step="00:30" :clearable="false" />
              <el-button link size="small" @click="editing.settings.schedule.splice(i, 1)">删除</el-button>
            </div>
            <div>
              <el-button size="small" @click="editing.settings.schedule.push({ days: [], start: '20:00', end: '23:00' })">添加时段</el-button>
              <span class="mute small"> {{ editing.settings.schedule.length ? '只在这些时段检测开播和录制（不选星期表示每天；结束早于开始表示跨过午夜）' : '不设置时段则全天监控' }}</span>
            </div>
          </div>
          <label>录完后</label>
          <div class="col">
            <el-checkbox v-model="editing.settings.convertMp4">无损转为 MP4（修正时间戳）</el-checkbox>
            <el-checkbox v-model="editing.settings.mergeSegments" :disabled="!editing.settings.convertMp4">合并同一场的分段</el-checkbox>
            <span class="row">
              <span class="mute small">视频规整</span>
              <el-select v-model="editing.settings.normalize" size="small" clearable placeholder="不规整" class="grow">
                <el-option v-for="p in presets" :key="p.id" :value="p.id" :label="p.name" />
              </el-select>
            </span>
            <span class="mute small">规整会在录制文件旁边生成新文件（修正时间戳和音画不同步、统一响度等），原文件保留。推荐选“直播录制”。</span>
          </div>
          <label>保存位置</label>
          <div class="row">
            <el-input v-model="editing.settings.dir" size="small" class="grow" placeholder="留空：下载目录 / 直播 / 主播名" />
            <el-button size="small" @click="pickDir">选择…</el-button>
          </div>
          <label>通知</label>
          <el-checkbox v-model="editing.settings.notify">开播、录制开始 / 结束、异常时通知</el-checkbox>
        </div>
        <p class="mute small">录制格式为 FLV / TS：程序意外退出或断电时，已录部分仍可播放。仅供个人留存，付费和加密直播不支持。</p>
      </div>
      <template #footer>
        <el-button @click="editing = null">取消</el-button>
        <el-button type="primary" :loading="saving" :disabled="!editing?.url.trim()" @click="save">{{ editing?.id === null ? '添加' : '保存' }}</el-button>
      </template>
    </el-dialog>

    <el-drawer :model-value="!!recsFor" :title="`${recsFor?.streamer ?? ''} 的录像`" size="520px" append-to-body @close="recsFor = null">
      <div v-if="!recs.length" class="empty mute">还没有录像</div>
      <div v-for="rc in recs" :key="rc.id" class="rec">
        <div class="ellipsis"><b>{{ rc.title || '直播' }}</b></div>
        <small class="mute">{{ formatDateTime(rc.startedAt) }}<template v-if="rc.endedAt"> – {{ formatDateTime(rc.endedAt) }}</template> · {{ formatBytes(rc.size) }} · {{ REC_STATUS[rc.status] ?? rc.status }}</small>
        <div v-for="f in rc.files" :key="f" class="file">
          <span class="mono ellipsis selectable" :title="f">{{ f.split(/[\\/]/).pop() }}</span>
          <el-button link size="small" @click="run(api.openFile, f)">播放</el-button>
          <el-button link size="small" @click="run(api.revealFile, f)">文件夹</el-button>
        </div>
      </div>
    </el-drawer>
  </div>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 1080px;
}
.head {
  display: flex;
  align-items: center;
  gap: 10px;
}
.head :deep(.el-button) {
  margin-left: 0;
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
.empty {
  padding: 40px 0;
  text-align: center;
  line-height: 1.8;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 12px;
}
.room {
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.cover {
  position: relative;
  height: 140px;
  background: var(--cc-line);
}
.cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.badge {
  position: absolute;
  left: 8px;
  top: 8px;
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  font-family: var(--cc-mono);
}
.badge.recording {
  background: #d83b3b;
}
.badge.live {
  background: var(--cc-ok);
}
.badge.error {
  background: #b26a00;
}
.badge.paused {
  left: auto;
  right: 8px;
}
.body {
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.who {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.avatar {
  width: 22px;
  height: 22px;
  border-radius: 50%;
}
.title {
  font-size: 12.5px;
}
.body small {
  font-size: 11px;
}
.err {
  color: var(--cc-err);
}
.okc {
  color: var(--cc-ok);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 8px;
  align-items: center;
  margin-top: 4px;
}
.actions :deep(.el-button) {
  margin-left: 0;
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
  font-size: 12.5px;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.col {
  display: flex;
  flex-direction: column;
}
.grow {
  flex: 1;
  min-width: 0;
}
.fgrid {
  display: grid;
  grid-template-columns: 90px 1fr;
  gap: 10px 12px;
  align-items: center;
}
.fgrid label {
  font-size: 12.5px;
  color: var(--cc-mute);
}
.preview {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.rec {
  border-top: 1px dashed var(--cc-line);
  padding: 8px 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.rec small {
  font-size: 11px;
}
.file {
  display: grid;
  grid-template-columns: 1fr auto auto;
  gap: 6px;
  align-items: center;
  font-size: 12px;
}
.win {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.tsel {
  width: 110px;
}
</style>
