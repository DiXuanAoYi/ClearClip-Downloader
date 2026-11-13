<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { save } from '@tauri-apps/plugin-dialog'
import { api, errorText, events } from '../api'
import type { Excerpt } from '../types'
import { formatDateTime } from '../utils/format'

/**
 * 摘录：清影里所有“记下这段文字”的地方都存到这里——
 * 桌面写作窗的“存入摘录”、截图翻译历史的“存进摘录”、浏览器扩展的“摘录选中文字到清影”和各处“存为笔记”。
 */
const list = ref<Excerpt[]>([])
const query = ref('')
const source = ref('')
const editing = ref('')
const draft = ref('')
let off: UnlistenFn | undefined

async function load() {
  list.value = await api.excerptList().catch(() => [])
}
onMounted(async () => {
  await load()
  off = await events.onExcerptsChanged(load)
})
onBeforeUnmount(() => off?.())

const sources = computed(() => [...new Set(list.value.map((e) => e.app).filter(Boolean))].slice(0, 200))
const rows = computed(() => {
  const q = query.value.trim().toLowerCase()
  return list.value.filter((e) => (!source.value || e.app === source.value) && (!q || `${e.text}\n${e.note}\n${e.app}`.toLowerCase().includes(q)))
})

async function copy(t: string, what = '内容') {
  try {
    await api.copyText(t)
    ElMessage.success(`已复制${what}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
function beginNote(e: Excerpt) {
  editing.value = e.id
  draft.value = e.note
}
async function saveNote(e: Excerpt) {
  editing.value = ''
  if (draft.value.trim() === e.note) return
  try {
    await api.excerptNote(e.id, draft.value)
    await load()
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
async function remove(e: Excerpt) {
  try {
    await ElMessageBox.confirm('删除这条摘录？', '摘录', { type: 'warning' })
  } catch {
    return
  }
  await api.excerptRemove(e.id).catch((err) => ElMessage.error(errorText(err)))
  await load()
}
async function exportMd() {
  const dest = await save({ defaultPath: '清影摘录.md', filters: [{ name: 'Markdown', extensions: ['md'] }] })
  if (!dest) return
  try {
    await api.excerptExport(dest)
    ElMessage.success('已导出')
    api.revealFile(dest).catch(() => {})
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div class="ex">
    <div class="bar">
      <el-input v-model="query" size="small" clearable placeholder="搜索摘录、笔记、来源" class="search" />
      <el-select v-model="source" size="small" clearable filterable placeholder="来源：全部" class="src">
        <el-option v-for="s in sources" :key="s" :value="s" :label="s.slice(0, 40)" />
      </el-select>
      <span class="grow" />
      <span class="mute">{{ rows.length }} / {{ list.length }} 条</span>
      <el-button size="small" :disabled="!list.length" @click="copy(rows.map((e) => e.text).join('\n\n'), '摘录')">复制</el-button>
      <el-button size="small" :disabled="!list.length" @click="exportMd">导出 Markdown</el-button>
    </div>
    <el-empty v-if="!rows.length" :description="list.length ? '没有符合条件的摘录' : '还没有摘录。在浏览器里选中文字右键“摘录选中文字到清影”、在划词卡片上点“存为笔记”，或在写作窗里点“存入摘录”，都会出现在这里。'" />
    <div v-for="e in rows" :key="e.id" class="it">
      <div class="meta">
        <span>{{ formatDateTime(e.created) }}</span>
        <span v-if="e.app">· {{ e.app }}</span>
        <a v-if="e.url" href="#" :title="e.url" @click.prevent="api.openUrl(e.url!)">打开网页</a>
        <span class="grow" />
        <el-button size="small" link @click="copy(e.text)">复制</el-button>
        <el-button size="small" link @click="beginNote(e)">{{ e.note ? '改笔记' : '加笔记' }}</el-button>
        <el-button size="small" link type="danger" @click="remove(e)">删除</el-button>
      </div>
      <div class="tx">{{ e.text }}</div>
      <el-input v-if="editing === e.id" v-model="draft" size="small" type="textarea" :rows="2" placeholder="写点笔记（失去焦点自动保存）" autofocus @blur="saveNote(e)" />
      <div v-else-if="e.note" class="note">笔记：{{ e.note }}</div>
    </div>
  </div>
</template>

<style scoped>
.ex { display: flex; flex-direction: column; gap: 10px; min-height: 0; overflow-y: auto; }
.bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; position: sticky; top: 0; background: var(--el-bg-color-page, var(--cc-card)); padding: 4px 0; z-index: 1; }
.search { width: 240px; }
.src { width: 200px; }
.grow { flex: 1; }
.mute { color: var(--cc-mute); font-size: 12px; }
.it { border: 1px solid var(--cc-line-soft); border-radius: 8px; padding: 8px 12px; display: flex; flex-direction: column; gap: 6px; background: var(--cc-card); }
.meta { display: flex; gap: 6px; align-items: center; font-size: 12px; color: var(--cc-mute); flex-wrap: wrap; }
.meta a { color: var(--cc-amber); }
.tx { white-space: pre-wrap; line-height: 1.6; max-height: 16em; overflow: auto; user-select: text; }
.note { font-size: 12.5px; color: var(--cc-fg2); background: var(--cc-hover); border-radius: 6px; padding: 4px 8px; }
</style>
