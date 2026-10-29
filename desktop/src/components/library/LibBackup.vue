<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../../api'
import { formatBytes } from '../../utils/format'

const emit = defineEmits<{ changed: [] }>()
const recursive = ref(true)
const importing = ref(false)
const prog = ref<{ done: number; total: number } | null>(null)
let un: UnlistenFn | undefined

onMounted(async () => {
  un = await events.onLibraryProgress((p) => {
    if (p.task === 'import') prog.value = p.done >= p.total ? null : { done: p.done, total: p.total }
  })
})
onBeforeUnmount(() => un?.())

function stamp() {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`
}

async function importFolder() {
  const dir = await open({ directory: true, multiple: false, title: '选择要导入的文件夹' })
  if (typeof dir !== 'string') return
  importing.value = true
  try {
    const r = await api.libraryImport(dir, recursive.value)
    ElMessage.success(`已导入 ${r.added} 个文件、${r.subtitles} 个字幕${r.skipped ? `（${r.skipped} 个已在媒体库里）` : ''}。视频时长和封面会在后台补全。`)
    emit('changed')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    importing.value = false
  }
}

async function exportList(format: 'csv' | 'json') {
  const dest = await save({ defaultPath: `清影媒体库-${stamp()}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] })
  if (!dest) return
  try {
    const n = await api.libraryExport(format, dest)
    ElMessage.success(`已导出 ${n} 条记录`)
    api.revealFile(dest).catch(() => {})
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function backup() {
  const dest = await save({ defaultPath: `清影备份-${stamp()}.zip`, filters: [{ name: '清影备份', extensions: ['zip'] }] })
  if (!dest) return
  try {
    await api.backupExport(dest)
    ElMessage.success('备份完成')
    api.revealFile(dest).catch(() => {})
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function restore() {
  const src = await open({ multiple: false, filters: [{ name: '清影备份', extensions: ['zip'] }], title: '选择备份文件' })
  if (typeof src !== 'string') return
  try {
    const info = await api.backupImport(src)
    await ElMessageBox.confirm(
      `这是清影 ${info.appVersion} 的备份（${formatBytes(info.size)}）。还原会用备份里的媒体库记录、标签、订阅、直播间和设置替换现在的内容（现在的数据会保留一份 .before-restore 副本）。需要重启清影才能生效，现在重启？`,
      '还原备份',
      { confirmButtonText: '重启并还原', cancelButtonText: '稍后重启', type: 'warning' },
    )
    await api.restartApp()
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error(errorText(e))
    else ElMessage.info('已准备好还原，下次启动清影时生效。')
  }
}
</script>

<template>
  <div class="bk">
    <section>
      <h4>导入已有的文件</h4>
      <p class="mute">把文件夹里已有的视频、音频、图片登记进媒体库，和视频同名的字幕会一起登记。已经在媒体库里的文件会跳过。</p>
      <div class="row">
        <el-button size="small" type="primary" :loading="importing" @click="importFolder">选择文件夹…</el-button>
        <el-checkbox v-model="recursive" size="small">包含子文件夹</el-checkbox>
      </div>
      <el-progress v-if="prog" :percentage="Math.round((prog.done / Math.max(prog.total, 1)) * 100)" :stroke-width="6" :format="() => `${prog!.done} / ${prog!.total}`" />
    </section>
    <section>
      <h4>导出清单</h4>
      <p class="mute">导出全部记录（标题、作者、路径、原链接、标签、评分等）。CSV 可以直接用 Excel 打开。</p>
      <div class="row">
        <el-button size="small" @click="exportList('csv')">导出 CSV</el-button>
        <el-button size="small" @click="exportList('json')">导出 JSON</el-button>
      </div>
    </section>
    <section>
      <h4>备份与还原</h4>
      <p class="mute">备份包含媒体库记录、标签、评分、订阅、直播间、解析历史、收到的链接和设置；不包含下载的文件本身，也不包含登录 Cookie 和手机配对令牌（换电脑后需要重新登录、重新配对）。</p>
      <div class="row">
        <el-button size="small" @click="backup">创建备份…</el-button>
        <el-button size="small" @click="restore">从备份还原…</el-button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.bk {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
h4 {
  margin: 0 0 4px;
  font-size: 13px;
}
p {
  margin: 0 0 8px;
  font-size: 12px;
}
.row {
  display: flex;
  gap: 10px;
  align-items: center;
}
</style>
