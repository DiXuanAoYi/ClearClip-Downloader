<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import { useAppStore, type SettingsTab } from '../stores/app'
import type { ToolProgress, ToolStatus } from '../types'
import { formatBytes } from '../utils/format'

const app = useAppStore()
const visible = computed(() => !!app.settings && app.settings.disclaimerAccepted && !app.settings.onboarded)
const step = ref(0)
const LAST = 3

const dir = ref('')
const language = ref<'zh' | 'en' | 'ja'>('zh')
const watchClipboard = ref(true)
const tools = ref<ToolStatus[]>([])
const progress = ref<Record<string, ToolProgress>>({})
let un: UnlistenFn | undefined

const NAMES: Record<string, string> = { ffmpeg: 'ffmpeg（合并音视频、转码、字幕烧录）', 'yt-dlp': 'yt-dlp（YouTube 等上千个网站）' }

watch(
  visible,
  async (v) => {
    if (!v || !app.settings) return
    dir.value = app.settings.downloadDir
    language.value = app.settings.language
    watchClipboard.value = app.settings.watchClipboard
    tools.value = await api.toolsStatus().catch(() => [])
    un = await events.onToolProgress((p) => (progress.value[p.tool] = p))
  },
  { immediate: true },
)
onBeforeUnmount(() => un?.())

async function pickDir() {
  const p = await open({ directory: true, title: '选择保存位置', defaultPath: dir.value || undefined })
  if (typeof p === 'string') dir.value = p
}

async function install(t: ToolStatus) {
  t.busy = true
  try {
    const s = await api.installTool(t.id)
    const i = tools.value.findIndex((x) => x.id === t.id)
    if (i >= 0) tools.value[i] = s
  } catch (e) {
    ElMessage.error(errorText(e))
    t.busy = false
  } finally {
    delete progress.value[t.id]
  }
}

async function finish(goto?: SettingsTab) {
  if (!app.settings) return
  try {
    await app.patch({ onboarded: true, downloadDir: dir.value || app.settings.downloadDir, language: language.value, watchClipboard: watchClipboard.value })
    if (goto) app.goSettings(goto)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <el-dialog :model-value="visible" width="520px" :show-close="false" :close-on-click-modal="false" :close-on-press-escape="false" align-center>
    <template #header>
      <div class="hd">
        <b>欢迎使用清影</b>
        <span class="mute">{{ step + 1 }} / {{ LAST + 1 }}</span>
      </div>
    </template>

    <div v-if="step === 0" class="pane">
      <p>几步设置，一分钟搞定。所有选项以后都能在“设置”里修改。</p>
      <div class="row">
        <span>界面语言</span>
        <el-radio-group v-model="language" size="small">
          <el-radio-button value="zh">中文</el-radio-button>
          <el-radio-button value="en">English</el-radio-button>
        </el-radio-group>
      </div>
      <p class="mute small">English 界面由机器翻译整理，个别较长的提示和动态消息仍为中文。</p>
    </div>

    <div v-else-if="step === 1" class="pane">
      <p>下载的文件放在哪里？</p>
      <div class="row">
        <el-input v-model="dir" size="small" class="mono" />
        <el-button size="small" @click="pickDir">选择…</el-button>
      </div>
      <p class="mute small">默认是“下载”文件夹下的 ClearClip，按网站分子文件夹。</p>
    </div>

    <div v-else-if="step === 2" class="pane">
      <p>下面两个组件让清影能处理更多网站和格式。需要时会联网下载，也可以跳过，以后在“设置 → 组件”里装。</p>
      <div v-for="t in tools" :key="t.id" class="tool">
        <div class="tn">
          <div>{{ NAMES[t.id] ?? t.id }}</div>
          <small class="mute">{{ t.installed ? `已安装${t.version ? ' ' + t.version : ''}` : '未安装' }}</small>
        </div>
        <el-button v-if="!t.installed" size="small" type="primary" :loading="t.busy" @click="install(t)">
          {{ progress[t.id]?.total ? `${Math.round((progress[t.id].received / progress[t.id].total!) * 100)}% · ${formatBytes(progress[t.id].received)}` : '安装' }}
        </el-button>
        <el-tag v-else type="success" size="small">就绪</el-tag>
      </div>
    </div>

    <div v-else class="pane">
      <div class="row">
        <span>自动识别复制的链接<small class="mute block">复制链接后弹出提示，一键解析</small></span>
        <el-switch v-model="watchClipboard" />
      </div>
      <p>还有这些可以逐步了解：</p>
      <ul class="tips">
        <li><a @click="finish('phone')">手机发送与浏览器扩展</a>：在手机或浏览器里一键发送链接给电脑。</li>
        <li><a @click="finish('security')">应用锁与隐私模式</a>：离开座位时遮住界面，或不留下任何记录。</li>
        <li><a @click="finish('accounts')">登录账号</a>：需要登录才能看的内容，登录后可以下载。</li>
      </ul>
    </div>

    <template #footer>
      <el-button v-if="step === 0" link @click="finish()">跳过引导</el-button>
      <el-button v-else @click="step--">上一步</el-button>
      <el-button v-if="step < LAST" type="primary" @click="step++">下一步</el-button>
      <el-button v-else type="primary" @click="finish()">开始使用</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.hd {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  font-size: 16px;
}
.pane {
  min-height: 150px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  line-height: 1.7;
}
.pane p {
  margin: 0;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.small {
  font-size: 12px;
}
.block {
  display: block;
  font-size: 11px;
}
.tool {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 12px;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
}
.tips {
  margin: 0;
  padding-left: 18px;
  font-size: 13px;
}
.tips a {
  color: var(--cc-acc);
  cursor: pointer;
}
</style>
