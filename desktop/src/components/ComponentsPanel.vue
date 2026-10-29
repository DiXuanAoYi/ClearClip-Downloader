<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import { useAppStore } from '../stores/app'
import type { CapsSummary, ToolProgress, ToolStatus } from '../types'
import { formatBytes } from '../utils/format'

const mirrors = defineModel<string[]>({ required: true })
const edition = defineModel<'lite' | 'full'>('edition', { default: 'lite' })
const app = useAppStore()
const caps = ref<CapsSummary | null>(null)

const INFO: Record<string, { name: string; desc: string }> = {
  'yt-dlp': { name: 'yt-dlp', desc: '解析和下载上千个视频网站（YouTube、Pornhub、Twitter/X、TikTok 等）。网站改版频繁，建议经常更新。' },
  ffmpeg: { name: 'ffmpeg', desc: '合并音视频分轨、m3u8 转 MP4、提取音频、Pixiv 动图合成，以及工具箱里的压缩、视频规整、防抖等。' },
}

const list = ref<ToolStatus[]>([])
const progress = ref<Record<string, ToolProgress>>({})
const loading = ref(false)
const sitesOpen = ref(false)
const sites = ref<string[]>([])
const sitesQuery = ref('')
const sitesLoading = ref(false)
const mirrorText = ref(mirrors.value.join('\n'))
let unlisten: UnlistenFn | undefined

const filteredSites = computed(() => {
  const q = sitesQuery.value.trim().toLowerCase()
  return q ? sites.value.filter((s) => s.toLowerCase().includes(q)) : sites.value
})

async function refresh() {
  loading.value = true
  try {
    list.value = await api.toolsStatus()
    caps.value = await api.videoCaps().catch(() => null)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}

function replace(s: ToolStatus) {
  const i = list.value.findIndex((x) => x.id === s.id)
  if (i >= 0) list.value[i] = s
}

async function install(t: ToolStatus) {
  t.busy = true
  try {
    // 要下载哪个版本以保存的设置为准，先保存再安装
    if (t.id === 'ffmpeg' && app.settings && app.settings.ffmpegEdition !== edition.value) await app.patch({ ffmpegEdition: edition.value })
    const s = await api.installTool(t.id)
    replace(s)
    ElMessage.success(`${INFO[t.id].name} 已就绪：${s.version ?? ''}`)
  } catch (e) {
    ElMessage.error(errorText(e))
    await refresh()
  } finally {
    delete progress.value[t.id]
  }
}

async function rollback(t: ToolStatus) {
  try {
    replace(await api.rollbackTool(t.id))
    ElMessage.success('已恢复到上一个版本')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function importFile(t: ToolStatus) {
  const path = await open({ title: `选择 ${INFO[t.id].name} 可执行文件`, multiple: false, directory: false })
  if (typeof path !== 'string') return
  try {
    replace(await api.importTool(t.id, path))
    ElMessage.success('已导入')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function openDir() {
  try {
    await api.openToolsDir()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function saveMirrors() {
  mirrors.value = mirrorText.value
    .split(/\s+/)
    .map((s) => s.trim())
    .filter((s) => /^https?:\/\//.test(s))
    .map((s) => (s.endsWith('/') ? s : `${s}/`))
  mirrorText.value = mirrors.value.join('\n')
}

async function showSites() {
  sitesOpen.value = true
  if (sites.value.length) return
  sitesLoading.value = true
  try {
    sites.value = await api.listExtractors()
  } catch (e) {
    ElMessage.error(errorText(e))
    sitesOpen.value = false
  } finally {
    sitesLoading.value = false
  }
}

function stageText(p: ToolProgress): string {
  if (p.stage === 'prepare') return '正在连接下载源…'
  if (p.stage === 'verify') return '正在校验…'
  if (p.stage === 'extract') return '正在解压…'
  if (p.stage === 'done') return '完成'
  return p.total ? `正在下载 ${formatBytes(p.received)} / ${formatBytes(p.total)}` : `正在下载 ${formatBytes(p.received)}`
}

function percent(p: ToolProgress): number {
  return p.total ? Math.min(100, Math.round((p.received / p.total) * 100)) : 0
}

onMounted(async () => {
  unlisten = await events.onToolProgress((p) => (progress.value[p.tool] = p))
  await refresh()
})
onUnmounted(() => unlisten?.())
</script>

<template>
  <div class="panel">
    <section v-for="t in list" :key="t.id" class="card block">
      <div class="title">
        <h3>{{ INFO[t.id]?.name ?? t.id }}</h3>
        <span v-if="t.installed" class="chip ok">{{ t.version ?? '已安装' }}</span>
        <span v-else class="chip warn">未安装</span>
        <span v-if="t.installed && !t.managed" class="chip">系统自带</span>
      </div>
      <p class="mute small">{{ INFO[t.id]?.desc }}</p>
      <small v-if="t.path" class="mono mute ellipsis selectable">{{ t.path }}</small>
      <small v-if="t.note" class="mute">{{ t.note }}</small>
      <div v-if="progress[t.id]" class="prog">
        <el-progress :percentage="percent(progress[t.id])" :show-text="false" :stroke-width="4" :indeterminate="!progress[t.id].total || progress[t.id].stage !== 'download'" />
        <small class="mute">{{ stageText(progress[t.id]) }}</small>
      </div>
      <template v-if="t.id === 'ffmpeg'">
        <div v-if="t.installed" class="row edition">
          <span class="chip" :class="t.edition === 'full' ? 'ok' : ''">{{ t.edition === 'full' ? '当前是完整版' : '当前是精简版' }}</span>
          <small v-if="caps" class="mute">
            H.264 编码器：{{ caps.h264 ?? '无' }}
            <template v-if="caps.hardware.length">；硬件编码：{{ caps.hardware.join('、') }}</template>
            ；防抖：{{ { vidstab: 'vidstab', deshake: 'deshake（较弱）', none: '不可用' }[caps.stabilize] }}
            ；HDR 转 SDR：{{ caps.tonemap ? 'zscale' : '程序内置' }}
          </small>
        </div>
        <div v-if="t.autoInstall" class="row edition">
          <span class="mute small">下载哪个版本</span>
          <el-radio-group v-model="edition" size="small" :disabled="t.busy">
            <el-radio-button value="lite">精简版（约 40 MB）</el-radio-button>
            <el-radio-button value="full">完整版（约 130 MB）</el-radio-button>
          </el-radio-group>
        </div>
        <small v-if="t.autoInstall" class="mute">
          精简版（LGPL）没有 x264 / x265 编码器和 vidstab 防抖，只能用硬件编码、openh264 或 VP9 输出视频。
          完整版（GPL）带 x264、x265、vidstab 防抖、zscale 色调映射等，画质和效果更好；它按 GPL 授权，由你自己下载使用，不随清影分发。选好后点下面的按钮即可切换。
        </small>
        <small v-for="n in caps?.notes ?? []" :key="n" class="mute warn-note">· {{ n }}</small>
      </template>
      <div class="row">
        <el-button v-if="t.autoInstall" size="small" type="primary" :loading="t.busy" @click="install(t)">{{ t.id === 'ffmpeg' && t.installed && (t.edition ?? 'lite') !== edition ? `切换到${edition === 'full' ? '完整版' : '精简版'}` : t.installed && t.managed ? '检查更新' : '下载安装' }}</el-button>
        <el-button v-if="t.hasPrevious" size="small" :disabled="t.busy" @click="rollback(t)">回退到上一版本</el-button>
        <el-button size="small" :disabled="t.busy" @click="importFile(t)">导入本地文件…</el-button>
        <el-button v-if="t.id === 'yt-dlp' && t.installed" size="small" link @click="showSites">支持的网站</el-button>
      </div>
    </section>

    <section class="card block">
      <h3>下载源</h3>
      <p class="mute small">组件从 GitHub 官方发布页下载，并用官方提供的 SHA-256 校验。访问 GitHub 较慢时，可以填写镜像前缀（每行一个），会自动选择最快的来源；校验仍以官方文件为准。</p>
      <el-input v-model="mirrorText" type="textarea" :rows="3" size="small" class="mono" placeholder="https://ghfast.top/" @change="saveMirrors" />
      <div class="row">
        <el-button size="small" :loading="loading" @click="refresh">刷新状态</el-button>
        <el-button size="small" @click="openDir">打开组件目录</el-button>
      </div>
    </section>

    <el-dialog v-model="sitesOpen" title="yt-dlp 支持的网站" width="520px" append-to-body>
      <el-input v-model="sitesQuery" size="small" clearable placeholder="搜索，例如 youtube、pornhub、twitter" />
      <p class="mute small count">共 {{ sites.length }} 个提取器{{ sitesQuery ? `，匹配 ${filteredSites.length} 个` : '' }}。部分网站需要登录或代理。</p>
      <div v-loading="sitesLoading" class="sites mono">
        <div v-for="s in filteredSites.slice(0, 500)" :key="s" class="site">{{ s }}</div>
        <div v-if="filteredSites.length > 500" class="mute">还有 {{ filteredSites.length - 500 }} 个，请输入关键词筛选</div>
      </div>
    </el-dialog>
  </div>
</template>

<style scoped>
.panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.block {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.title {
  display: flex;
  align-items: center;
  gap: 8px;
}
h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}
.chip {
  font-size: 11px;
  padding: 0 7px;
  line-height: 18px;
  border-radius: 9px;
  border: 1px solid var(--cc-line);
  color: var(--cc-mute);
}
.chip.ok {
  color: var(--cc-ok);
  border-color: var(--cc-ok);
}
.chip.warn {
  color: var(--cc-warn, #d48806);
  border-color: var(--cc-warn, #d48806);
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
.prog {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.small {
  font-size: 12px;
  margin: 0;
  line-height: 1.6;
}
.count {
  margin: 8px 0;
}
.sites {
  max-height: 360px;
  overflow: auto;
  font-size: 12px;
}
.site {
  padding: 2px 0;
  border-top: 1px dashed var(--cc-line);
}
</style>
