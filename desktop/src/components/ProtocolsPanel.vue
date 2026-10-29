<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { Settings, TrackerStatus } from '../types'

const form = defineModel<Settings>({ required: true })

const serversText = ref(form.value.ed2kServers.join('\n'))
const sourcesText = ref(form.value.btTrackerSources.join('\n'))
const customText = ref(form.value.btCustomTrackers.join('\n'))
const status = ref<TrackerStatus | null>(null)
const updating = ref(false)

const lines = (t: string) =>
  t
    .split(/[\n,，]+/)
    .map((l) => l.trim())
    .filter(Boolean)

watch(serversText, (t) => (form.value.ed2kServers = lines(t)))
watch(sourcesText, (t) => (form.value.btTrackerSources = lines(t)))
watch(customText, (t) => (form.value.btCustomTrackers = lines(t)))

async function refresh() {
  try {
    status.value = await api.btTrackerStatus()
  } catch {
    status.value = null
  }
}

async function updateNow() {
  updating.value = true
  try {
    // 先保存当前填的来源，再更新
    await api.saveSettings(form.value)
    status.value = await api.btUpdateTrackers()
    ElMessage.success(`已更新：共 ${status.value.listCount} 个 tracker`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    updating.value = false
  }
}

function when(ts: number | null) {
  return ts ? new Date(ts * 1000).toLocaleString() : '还没有更新过'
}

onMounted(refresh)
</script>

<template>
  <div class="groups">
    <section class="group card">
      <h3>BT / 磁力</h3>
      <small class="mute">粘贴 magnet: 链接或 .torrent 地址即可下载；多文件的种子会展开成列表，选择要下载的文件。下载完成后不做种。DHT、PEX、UPnP 已启用。</small>
      <div class="kv">
        <span>自动更新 tracker 列表<small class="mute block">定期从开源列表下载最新的 tracker，让更多种子能找到下载来源；失败时继续用上次的列表</small></span>
        <el-switch v-model="form.btTrackerAutoUpdate" />
      </div>
      <div class="kv">
        <span>更新间隔（天）</span>
        <el-input-number v-model="form.btTrackerIntervalDays" :min="1" :max="90" size="small" controls-position="right" />
      </div>
      <div class="field">
        <label>列表来源</label>
        <el-input v-model="sourcesText" type="textarea" :rows="3" class="mono" placeholder="每行一个 http / https 地址；按顺序尝试，留空使用内置的 ngosang/trackerslist" />
      </div>
      <div class="field">
        <label>自己添加的 tracker</label>
        <el-input v-model="customText" type="textarea" :rows="3" class="mono" placeholder="每行一个，支持 udp:// http:// https://；始终使用，排在列表前面" />
      </div>
      <div class="status">
        <span v-if="status">
          上次更新：{{ when(status.updatedAt) }}<template v-if="status.source">（{{ status.source }}）</template>；列表 {{ status.listCount }} 个，实际使用 {{ status.effectiveCount }} 个
        </span>
        <el-button size="small" :loading="updating" @click="updateNow">立即更新</el-button>
      </div>
    </section>

    <section class="group card">
      <h3>ed2k（电驴）</h3>
      <small class="mute">粘贴 ed2k://|file|… 链接即可下载，来源从链接自带的地址和下面的服务器获取。没有内置资源搜索。</small>
      <div class="field">
        <label>服务器</label>
        <el-input v-model="serversText" type="textarea" :rows="3" class="mono" placeholder="每行一个，例如 45.82.80.155:5687（可选）" />
      </div>
      <div class="field">
        <label>服务器列表（server.met）地址</label>
        <el-input v-model="form.ed2kServerMetUrl" size="small" class="mono" placeholder="留空则只用上面手动添加的服务器" />
        <small class="mute">每次下载时取其中前几台服务器；下载失败时使用上次缓存的列表</small>
      </div>
    </section>
  </div>
</template>

<style scoped>
.groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
  gap: 14px;
  align-items: start;
}
.group {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
h3 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-mute);
}
.kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.block {
  display: block;
}
.status {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  font-size: 12px;
  color: var(--cc-mute);
}
</style>
