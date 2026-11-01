<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { open, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../api'
import type { AiExpert, AiSettings, ExpertsState } from '../types'

const ai = defineModel<AiSettings>({ required: true })
const data = ref<ExpertsState>({ experts: [], selected: 'general', auto: true, communityPage: '' })
const tab = ref<'mine' | 'community'>('mine')
const busy = ref(false)
const ORIGIN: Record<string, string> = { builtin: '内置', user: '自建', community: '社区' }

async function reload() {
  data.value = await api.expertsState().catch(() => data.value)
}
onMounted(reload)

// ---------- 编辑 ----------
const editing = ref<AiExpert | null>(null)
const matchText = ref('')
function blank(): AiExpert {
  return { id: '', name: '', description: '', origin: 'user', guide: '', matches: [], remoteId: '', updated: 0 }
}
function edit(e?: AiExpert) {
  // 内置专家不能直接改：点“复制”得到一份自己的
  const copy = e ? JSON.parse(JSON.stringify(e)) : blank()
  if (e?.origin === 'builtin') {
    copy.id = ''
    copy.origin = 'user'
    copy.name = `${e.name}（我的）`
  }
  editing.value = copy
  matchText.value = (copy.matches || []).join('\n')
}
async function saveEditing() {
  const e = editing.value
  if (!e) return
  e.matches = matchText.value.split('\n').map((s) => s.trim()).filter(Boolean)
  busy.value = true
  try {
    await api.expertsSave(e)
    editing.value = null
    await reload()
    ElMessage.success('已保存')
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    busy.value = false
  }
}
async function remove(e: AiExpert) {
  try {
    await ElMessageBox.confirm(`删除专家“${e.name}”？`, '删除专家', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' })
  } catch {
    return
  }
  try {
    await api.expertsDelete(e.id)
    if (ai.value.expert === e.id) ai.value.expert = 'general'
    await reload()
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
function use(e: AiExpert) {
  ai.value.expert = e.id
}

// ---------- 导入 / 导出 ----------
async function importFile() {
  const f = await open({ multiple: false, title: '导入 AI 专家（JSON 或沉浸式翻译的 YAML）', filters: [{ name: '专家文件', extensions: ['json', 'yml', 'yaml'] }] })
  if (typeof f !== 'string') return
  try {
    const e = await api.expertsImportFile(f)
    await reload()
    ElMessage.success(`已导入“${e.name}”`)
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
async function exportOne(e: AiExpert) {
  const path = await saveDialog({ title: '导出 AI 专家', defaultPath: `${e.name}.json`, filters: [{ name: 'JSON', extensions: ['json'] }] })
  if (!path) return
  try {
    await api.batchSaveText(path, await api.expertsExport(e.id))
    ElMessage.success('已导出')
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}

// ---------- 社区 ----------
const community = ref<AiExpert[]>([])
const loadingCommunity = ref(false)
const installing = ref('')
async function loadCommunity() {
  loadingCommunity.value = true
  try {
    community.value = await api.expertsCommunity()
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    loadingCommunity.value = false
  }
}
function onTab(name: string | number) {
  if (name === 'community' && !community.value.length) loadCommunity()
}
const installed = (c: AiExpert) => data.value.experts.some((e) => e.origin === 'community' && e.remoteId === c.remoteId)
async function install(c: AiExpert) {
  installing.value = c.remoteId
  try {
    await api.expertsCommunityInstall(c.remoteId)
    await reload()
    ElMessage.success(`已安装“${c.name}”`)
  } catch (err) {
    ElMessage.error(errorText(err))
  } finally {
    installing.value = ''
  }
}
</script>

<template>
  <section class="group card ex">
    <div class="titlebar">
      <h3>AI 专家</h3>
      <el-switch v-model="ai.expertAuto" size="small" active-text="按网站自动选用" />
    </div>
    <small class="mute">
      “专家”是一套翻译策略（提示词）：针对某个领域规定用词、语气和格式处理。默认的【通用】适合大多数场景；翻译字幕、网页时会按这里选的专家翻译。
      开了“按网站自动选用”，默认专家是【通用】时会按网址换成匹配的专家（例如 github.com → 技术文档）。可以自己写，也可以从沉浸式翻译公开的
      <a href="#" @click.prevent="api.openUrl(data.communityPage || 'https://github.com/immersive-translate/prompts')">AI 专家仓库</a>安装。专家的说明只会补充用词和语气，译文的输出格式始终由程序保证。
    </small>

    <div class="row">
      <label>默认专家</label>
      <el-select v-model="ai.expert" class="sel" filterable>
        <el-option v-for="e in data.experts" :key="e.id" :label="e.name" :value="e.id" />
      </el-select>
    </div>

    <el-tabs v-model="tab" @tab-change="onTab">
      <el-tab-pane label="我的专家" name="mine">
        <div v-if="!editing">
          <div class="bar">
            <el-button size="small" type="primary" @click="edit()">新建专家</el-button>
            <el-button size="small" @click="importFile">导入</el-button>
          </div>
          <div v-for="e in data.experts" :key="e.id" class="item" :class="{ cur: ai.expert === e.id }">
            <div class="info">
              <span class="nm">{{ e.name }} <el-tag size="small" class="bd">{{ ORIGIN[e.origin] }}</el-tag><el-tag v-if="ai.expert === e.id" size="small" type="success" class="bd">使用中</el-tag></span>
              <small class="mute">{{ e.description || e.guide.slice(0, 80) }}<template v-if="e.matches.length"> · 网站：{{ e.matches.slice(0, 3).join('、') }}{{ e.matches.length > 3 ? '…' : '' }}</template></small>
            </div>
            <el-button size="small" link type="primary" :disabled="ai.expert === e.id" @click="use(e)">使用</el-button>
            <el-button v-if="e.origin === 'builtin'" size="small" link @click="edit(e)">复制</el-button>
            <el-button v-else size="small" link @click="edit(e)">编辑</el-button>
            <el-button v-if="e.id !== 'general'" size="small" link @click="exportOne(e)">导出</el-button>
            <el-button v-if="e.origin !== 'builtin'" size="small" link type="danger" @click="remove(e)">删除</el-button>
          </div>
        </div>
        <div v-else class="edit">
          <div class="grid">
            <div class="field"><label>名称</label><el-input v-model="editing.name" maxlength="40" /></div>
            <div class="field"><label>一句话说明</label><el-input v-model="editing.description" maxlength="300" /></div>
          </div>
          <div class="field">
            <label>策略说明（告诉模型这类内容该怎么翻；英文效果最好，中文也行）</label>
            <el-input v-model="editing.guide" type="textarea" :rows="8" maxlength="4000" show-word-limit placeholder="例如：Translate legal contracts formally. Keep defined terms consistent…" />
          </div>
          <div class="field">
            <label>自动选用的网站（每行一个，如 github.com、*.nature.com；可留空）</label>
            <el-input v-model="matchText" type="textarea" :rows="3" />
          </div>
          <div class="row">
            <el-button type="primary" :loading="busy" @click="saveEditing">保存</el-button>
            <el-button @click="editing = null">取消</el-button>
          </div>
        </div>
      </el-tab-pane>

      <el-tab-pane label="社区专家" name="community">
        <div class="bar">
          <el-button size="small" :loading="loadingCommunity" @click="loadCommunity">刷新</el-button>
          <small class="mute">来自沉浸式翻译公开仓库，按需安装，不随程序分发；安装后可在“我的专家”里使用和修改生效网站。</small>
        </div>
        <div v-if="!community.length && !loadingCommunity" class="empty mute">还没有取到列表，点“刷新”重试。</div>
        <div v-for="c in community" :key="c.remoteId" class="item">
          <div class="info">
            <span class="nm">{{ c.name }}</span>
            <small class="mute">{{ c.description }}</small>
          </div>
          <el-button size="small" :type="installed(c) ? 'default' : 'primary'" :loading="installing === c.remoteId" @click="install(c)">{{ installed(c) ? '更新' : '安装' }}</el-button>
        </div>
      </el-tab-pane>
    </el-tabs>
  </section>
</template>

<style scoped>
.ex { grid-column: 1 / -1; }
.group { padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
.titlebar { display: flex; justify-content: space-between; align-items: center; }
h3 { margin: 0; font-size: 12px; color: var(--cc-mute); }
.bar, .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.bar { margin-bottom: 8px; }
.sel { width: 220px; }
.item { display: flex; align-items: center; gap: 10px; padding: 8px 0; border-top: 1px solid var(--cc-border, rgba(128,128,128,.2)); }
.info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.nm { font-size: 13px; }
.bd { margin-left: 6px; }
.empty { padding: 8px 0; }
.edit { display: flex; flex-direction: column; gap: 10px; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
.field { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.field label { font-size: 12px; color: var(--cc-mute); }
@media (max-width: 640px) { .grid { grid-template-columns: 1fr; } }
</style>
