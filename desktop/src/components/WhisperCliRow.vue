<script setup lang="ts">
// whisper.cpp 命令行程序（whisper-cli）：本机转写要用。Windows 一键下载官方包；其他系统给出安装办法并可重新检测。
import { onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText, events } from '../api'
import type { WhisperCliStatus } from '../types'

const st = ref<WhisperCliStatus | null>(null)
const busy = ref(false)
const pct = ref(0)
let un: UnlistenFn | null = null

async function reload() {
  st.value = await api.whisperCliStatus()
}
async function install() {
  busy.value = true
  pct.value = 0
  try {
    st.value = await api.whisperCliInstall()
    ElMessage.success('whisper-cli 已装好，并填进“设置 → AI”')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
async function pick() {
  const r = await open({ multiple: false, filters: [{ name: 'whisper-cli', extensions: ['exe', ''] }] })
  if (typeof r !== 'string') return
  try {
    const s = await api.getSettings()
    await api.saveSettings({ ...s, ai: { ...s.ai, whisperBin: r } })
    await reload()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
onMounted(async () => {
  un = await events.onModelProgress((p) => {
    if (p.id === 'whisper-cli' && busy.value && p.total > 0) pct.value = Math.round((p.got / p.total) * 100)
  })
  try {
    await reload()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
})
onBeforeUnmount(() => un?.())
</script>

<template>
  <div v-if="st" class="wcli" data-testid="whisper-cli">
    <div class="row">
      <b>whisper-cli 程序</b>
      <span class="muted">{{ st.installed ? `已就绪 · ${st.path}` : '未找到' }}</span>
    </div>
    <div v-if="st.note" class="muted note">{{ st.note }}</div>
    <div class="row">
      <el-button v-if="st.autoInstall && !st.installed" size="small" type="primary" :loading="busy" @click="install">{{ busy && pct ? `下载中 ${pct}%` : '下载安装' }}</el-button>
      <el-button v-if="!st.installed" size="small" :disabled="busy" @click="reload">重新检测</el-button>
      <el-button size="small" :disabled="busy" @click="pick">指定位置…</el-button>
    </div>
  </div>
</template>

<style scoped>
.wcli { display: flex; flex-direction: column; gap: 4px; padding: 8px; border: 1px solid var(--el-border-color-lighter); border-radius: 6px; margin-bottom: 10px; }
.row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.muted { color: var(--el-text-color-secondary); font-size: 12px; word-break: break-all; }
.note { line-height: 1.4; }
</style>
