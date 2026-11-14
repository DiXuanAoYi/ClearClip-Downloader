<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { open, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../api'
import type { AiSettings, CommunityGlossary, ExtractSite, Glossary, GlossaryCandidate, GlossaryData } from '../types'
import GlossaryTermTree from './GlossaryTermTree.vue'
import { countTerms } from '../utils/termtree'

const ai = defineModel<AiSettings>({ required: true })
const data = ref<GlossaryData>({ glossaries: [], candidates: [], sites: [], folders: [] })
const tab = ref<'mine' | 'extract' | 'community'>('mine')
const busy = ref(false)

const LANGS = [
  { v: '', l: '任意语言' },
  { v: 'zh-CN', l: '简体中文' },
  { v: 'zh-TW', l: '繁体中文' },
  { v: 'en', l: 'English' },
  { v: 'ja', l: '日本語' },
  { v: 'ko', l: '한국어' },
  { v: 'fr', l: 'Français' },
  { v: 'de', l: 'Deutsch' },
  { v: 'es', l: 'Español' },
  { v: 'ru', l: 'Русский' },
]
const ORIGIN: Record<string, string> = { user: '自建', community: '社区', extracted: '智能提取' }
const langName = (v: string) => LANGS.find((x) => x.v === v)?.l || v || '任意语言'

async function reload() {
  data.value = await api.glossaryState().catch(() => data.value)
}
onMounted(reload)

// 翻译记忆：所有翻译入口共用的“原文 → 译文”；换了术语库或专家后旧的机器译文自动不再命中
const tm = ref<[number, number]>([0, 0])
const loadTm = () => api.tmStats().then((v) => (tm.value = v), () => {})
onMounted(loadTm)
async function clearTm() {
  try {
    await ElMessageBox.confirm('清空翻译记忆里的机器译文？你在文件翻译阅读器里改过的译文会保留。', '翻译记忆', { type: 'warning' })
  } catch {
    return
  }
  try {
    const n = await api.tmClear()
    ElMessage.success(`已清空 ${n} 条`)
    await loadTm()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

// ---------- 文件夹（多层，用 / 分隔）----------
const collapsed = ref<Set<string>>(new Set())
/** 所有文件夹路径（含每一层的上级），按层级排好。 */
const allFolders = computed(() => {
  const set = new Set<string>()
  const add = (p: string) => {
    const parts = p.split('/').filter(Boolean)
    for (let i = 1; i <= parts.length; i++) set.add(parts.slice(0, i).join('/'))
  }
  data.value.folders.forEach(add)
  data.value.glossaries.forEach((g) => g.folder && add(g.folder))
  return [...set].sort((a, b) => a.localeCompare(b, 'zh'))
})
type TreeRow = { kind: 'folder'; path: string; name: string; depth: number; count: number } | { kind: 'glossary'; g: Glossary; depth: number }
const depthOf = (p: string) => (p ? p.split('/').length : 0)
const parentOf = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '')
const hiddenBy = (p: string) => [...collapsed.value].some((c) => p !== c && (p + '/').startsWith(c + '/'))
/** 树形列表：文件夹在前、术语库在后，每层按名称排；折叠的文件夹里的东西不显示。 */
const tree = computed<TreeRow[]>(() => {
  const rows: TreeRow[] = []
  const walk = (dir: string) => {
    for (const f of allFolders.value.filter((x) => parentOf(x) === dir)) {
      const count = data.value.glossaries.filter((g) => g.folder === f || g.folder.startsWith(f + '/')).length
      rows.push({ kind: 'folder', path: f, name: f.split('/').pop() || f, depth: depthOf(f) - 1, count })
      if (!collapsed.value.has(f)) walk(f)
    }
    for (const g of data.value.glossaries.filter((x) => (x.folder || '') === dir)) rows.push({ kind: 'glossary', g, depth: depthOf(dir) })
  }
  walk('')
  return rows.filter((r) => r.kind === 'glossary' || !hiddenBy(r.path))
})
function toggleFolder(p: string) {
  const s = new Set(collapsed.value)
  if (s.has(p)) s.delete(p)
  else s.add(p)
  collapsed.value = s
}
async function newFolder(parent = '') {
  let name: string
  try {
    name = (await ElMessageBox.prompt(parent ? `在“${parent}”里新建文件夹（可以用 / 一次建多层）` : '新建文件夹（可以用 / 一次建多层，如 游戏/原神）', '新建文件夹', { inputPlaceholder: '文件夹名' })).value
  } catch {
    return
  }
  if (!name?.trim()) return
  try {
    data.value = await api.glossaryFolderAdd(parent ? `${parent}/${name}` : name)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function renameFolder(p: string) {
  let to: string
  try {
    to = (await ElMessageBox.prompt('新的位置和名称（用 / 表示上下级，可以移到别的文件夹里）', '重命名 / 移动文件夹', { inputValue: p })).value
  } catch {
    return
  }
  if (!to?.trim() || to.trim() === p) return
  try {
    data.value = await api.glossaryFolderRename(p, to)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function removeFolder(p: string) {
  try {
    await ElMessageBox.confirm(`删除文件夹“${p}”？里面的术语库和子文件夹会挪到上一层，不会被删除。`, '删除文件夹', { type: 'warning' })
  } catch {
    return
  }
  try {
    data.value = await api.glossaryFolderRemove(p)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

// ---------- 术语库列表与编辑 ----------
const editing = ref<Glossary | null>(null)
/** 编辑区重新挂载用（打开另一个术语库、并入 CSV 之后） */
const editKey = ref(0)
const treeRef = ref<{ tree: () => Glossary['terms'] } | null>(null)
/** 当前这条术语的路径（原文 / 译文），显示在底栏 */
const curPath = ref<{ src: string[]; tgt: string[] }>({ src: [], tgt: [] })

function blank(folder = ''): Glossary {
  return { id: '', name: '', description: '', enabled: true, origin: 'user', remoteId: '', matches: [], lang: '', terms: [], updated: 0, folder }
}
function edit(g?: Glossary, folder = '') {
  editing.value = g ? JSON.parse(JSON.stringify(g)) : blank(folder)
  curPath.value = { src: [], tgt: [] }
  editKey.value++
}
async function saveEditing() {
  const g = editing.value
  if (!g) return
  g.terms = treeRef.value?.tree() ?? g.terms
  g.matches = g.matches.map((s) => s.trim()).filter(Boolean)
  busy.value = true
  try {
    await api.glossarySave(g)
    editing.value = null
    await reload()
    ElMessage.success('已保存')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
async function toggle(g: Glossary) {
  try {
    await api.glossarySave({ ...g })
  } catch (e) {
    ElMessage.error(errorText(e))
    await reload()
  }
}
async function remove(g: Glossary) {
  try {
    await ElMessageBox.confirm(`删除术语库“${g.name}”（${countTerms(g.terms)} 条）？`, '删除术语库', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' })
  } catch {
    return
  }
  await api.glossaryDelete(g.id).catch((e) => ElMessage.error(errorText(e)))
  reload()
}
async function importCsv(into = '') {
  const f = await open({ multiple: false, title: '选择术语 CSV', filters: [{ name: '术语表', extensions: ['csv', 'tsv', 'txt'] }] })
  if (typeof f !== 'string') return
  try {
    const g = await api.glossaryImportFile(f, '', into, into ? '' : '')
    ElMessage.success(`已导入 ${countTerms(g.terms)} 条`)
    await reload()
    if (into && editing.value) edit(data.value.glossaries.find((x) => x.id === into))
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function exportCsv(g: Glossary) {
  try {
    const text = await api.glossaryExport(g.id)
    const path = await saveDialog({ defaultPath: `${g.name}.csv`, filters: [{ name: 'CSV', extensions: ['csv'] }] })
    if (!path) return
    await api.batchSaveText(path, text)
    ElMessage.success('已导出（与沉浸式翻译的术语 CSV 格式相同）')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

// ---------- 智能提取 ----------
const srcKind = ref<'text' | 'file' | 'url'>('text')
const srcValue = ref('')
const extracting = ref(false)
const picked = ref<GlossaryCandidate[]>([])
const addTo = ref('')
const newName = ref('')
const NEW = '__new__'

async function pickSrcFile() {
  const f = await open({ multiple: false, title: '选择字幕或文本', filters: [{ name: '字幕 / 文本', extensions: ['srt', 'vtt', 'ass', 'ssa', 'txt', 'md', 'html', 'htm'] }] })
  if (typeof f === 'string') srcValue.value = f
}
async function runExtract() {
  if (!srcValue.value.trim()) return
  extracting.value = true
  try {
    const r = await api.glossaryExtract(srcKind.value, srcValue.value.trim())
    await reload()
    ElMessage.success(r.found ? `模型给出 ${r.found} 条，新增 ${r.added} 条到“待确认”（已有的不重复）` : '没有找到需要统一译法的术语')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    extracting.value = false
  }
}
async function acceptPicked() {
  if (!picked.value.length) return
  const toNew = addTo.value === NEW || !addTo.value
  if (toNew && !newName.value.trim()) {
    ElMessage.warning('请选择要加入的术语库，或给新术语库起个名字')
    return
  }
  try {
    const terms = picked.value.map((c) => ({ source: c.source, target: c.target, note: c.note }))
    const g = await api.glossaryResolve(picked.value.map((c) => c.id), toNew ? '' : addTo.value, toNew ? newName.value.trim() : '', terms)
    picked.value = []
    newName.value = ''
    await reload()
    ElMessage.success(g ? `已加入“${g.name}”` : '已处理')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function ignorePicked() {
  if (!picked.value.length) return
  await api.glossaryResolve(picked.value.map((c) => c.id), '', '', []).catch((e) => ElMessage.error(errorText(e)))
  picked.value = []
  reload()
}

// ---------- 提取网站 ----------
const newSite = ref('')
async function saveSites(sites: ExtractSite[]) {
  try {
    await api.glossarySitesSave(sites)
    await reload()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
function addSite() {
  const p = newSite.value.trim().replace(/^https?:\/\//, '').replace(/\/.*$/, '')
  if (!p) return
  newSite.value = ''
  saveSites([...data.value.sites, { pattern: p, enabled: true, glossary: '', last: 0, found: 0 }])
}
const when = (s: number) => (s ? new Date(s * 1000).toLocaleString() : '—')

// ---------- 社区术语库 ----------
const community = ref<CommunityGlossary[]>([])
const loadingCommunity = ref(false)
const cq = ref('')
const installing = ref('')
const cshown = computed(() => {
  const q = cq.value.trim().toLowerCase()
  return q ? community.value.filter((c) => `${c.name} ${c.description} ${c.id}`.toLowerCase().includes(q)) : community.value
})
const installed = (id: string) => data.value.glossaries.some((g) => g.remoteId === id)
async function loadCommunity() {
  loadingCommunity.value = true
  try {
    community.value = await api.glossaryCommunity()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loadingCommunity.value = false
  }
}
async function install(c: CommunityGlossary) {
  installing.value = c.id
  try {
    const g = await api.glossaryCommunityInstall(c.id)
    await reload()
    ElMessage.success(`已安装“${g.name}”（${countTerms(g.terms)} 条，${langName(g.lang)}）`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    installing.value = ''
  }
}
function onTab(name: string | number) {
  if (name === 'community' && !community.value.length) loadCommunity()
}
</script>

<template>
  <section class="group card gl">
    <div class="titlebar">
      <h3>AI 术语库</h3>
      <el-switch v-model="ai.glossaryOn" size="small" active-text="翻译时使用" />
    </div>
    <small class="mute">
      让专有名词、行话在翻译里始终是同一个译法。每次翻译只会把这一批文字里<b>出现过</b>的术语告诉模型，不增加多少用量。可以自己建、从沉浸式翻译的
      <a href="#" @click.prevent="api.openUrl('https://github.com/immersive-translate/terms')">公共术语库</a>安装，或者让模型从字幕 / 网页里智能提取，确认后入库。CSV 格式与沉浸式翻译相同，可互相导入导出。
    </small>
    <div class="tmrow">
      <small class="mute">翻译记忆：{{ tm[0] }} 条（其中人工改过 {{ tm[1] }} 条）。网页、字幕、屏幕、文件翻译共用，同一句话不重复翻；换了术语库或 AI 专家后，旧的机器译文自动不再使用。</small>
      <el-button size="small" text :disabled="!tm[0] || tm[0] === tm[1]" @click="clearTm">清空机器译文</el-button>
    </div>

    <el-tabs v-model="tab" @tab-change="onTab">
      <!-- 我的术语库 -->
      <el-tab-pane :label="`术语库（${data.glossaries.length}）`" name="mine">
        <div class="bar">
          <el-button size="small" type="primary" @click="edit()">新建术语库</el-button>
          <el-button size="small" @click="newFolder()">新建文件夹</el-button>
          <el-button size="small" @click="importCsv()">导入 CSV…</el-button>
        </div>
        <div v-if="!data.glossaries.length && !allFolders.length" class="empty mute">还没有术语库。点“新建术语库”自己写，或去“社区术语库”一键安装。</div>
        <template v-for="r in tree" :key="r.kind === 'folder' ? 'f:' + r.path : r.g.id">
          <div v-if="r.kind === 'folder'" class="item folder" :style="{ paddingLeft: r.depth * 18 + 'px' }">
            <button class="fold" type="button" :aria-label="collapsed.has(r.path) ? '展开' : '折叠'" @click="toggleFolder(r.path)">{{ collapsed.has(r.path) ? '▸' : '▾' }}</button>
            <div class="info" @click="toggleFolder(r.path)"><div class="nm">📁 {{ r.name }} <small class="mute">{{ r.count }} 个术语库</small></div></div>
            <el-button size="small" link type="primary" @click="edit(undefined, r.path)">新建术语库</el-button>
            <el-button size="small" link @click="newFolder(r.path)">新建子文件夹</el-button>
            <el-button size="small" link @click="renameFolder(r.path)">重命名 / 移动</el-button>
            <el-button size="small" link type="danger" @click="removeFolder(r.path)">删除</el-button>
          </div>
          <div v-else class="item" :class="{ off: !r.g.enabled }" :style="{ paddingLeft: r.depth * 18 + 'px' }">
            <el-switch v-model="r.g.enabled" size="small" @change="toggle(r.g)" />
            <div class="info">
              <div class="nm">{{ r.g.name }} <el-tag size="small" effect="plain">{{ ORIGIN[r.g.origin] }}</el-tag></div>
              <small class="mute">{{ countTerms(r.g.terms) }} 条 · {{ langName(r.g.lang) }} · {{ r.g.matches.length ? `仅 ${r.g.matches.slice(0, 3).join('、')}${r.g.matches.length > 3 ? '…' : ''}` : '所有网站' }}</small>
            </div>
            <el-button size="small" link type="primary" @click="edit(r.g)">编辑</el-button>
            <el-button size="small" link @click="exportCsv(r.g)">导出</el-button>
            <el-button size="small" link type="danger" @click="remove(r.g)">删除</el-button>
          </div>
        </template>
      </el-tab-pane>

      <!-- 智能提取 -->
      <el-tab-pane name="extract">
        <template #label>智能提取<el-badge v-if="data.candidates.length" :value="data.candidates.length" class="bd" /></template>
        <div class="box">
          <h4>提取术语</h4>
          <small class="mute">让模型读一段文字，挑出需要统一译法的专有名词和行话，并给出“翻译成{{ ai.targetLang }}”的建议。结果先放进下面的“待确认”，不会直接生效。</small>
          <el-radio-group v-model="srcKind" size="small" @change="srcValue = ''">
            <el-radio-button value="text">粘贴文字</el-radio-button>
            <el-radio-button value="file">字幕 / 文本文件</el-radio-button>
            <el-radio-button value="url">网页地址</el-radio-button>
          </el-radio-group>
          <el-input v-if="srcKind === 'text'" v-model="srcValue" type="textarea" :rows="4" placeholder="把文章、字幕或一段对话粘贴到这里" />
          <div v-else-if="srcKind === 'file'" class="row"><el-input v-model="srcValue" size="small" class="mono" placeholder=".srt / .vtt / .ass / .txt / .md / .html" /><el-button size="small" @click="pickSrcFile">选择…</el-button></div>
          <el-input v-else v-model="srcValue" size="small" class="mono" placeholder="https://… （需要登录的页面请直接粘贴文字）" />
          <div><el-button size="small" type="primary" :loading="extracting" :disabled="!srcValue.trim()" @click="runExtract">开始提取</el-button> <small class="mute">会消耗一些模型额度；长文本只取样其中几段。</small></div>
        </div>

        <div class="box">
          <h4>提取结果（待确认 {{ data.candidates.length }}）</h4>
          <div v-if="!data.candidates.length" class="mute">暂时没有。提取后，或在下面“提取网站”里的网站上翻译后，结果会出现在这里。</div>
          <template v-else>
            <el-table :data="data.candidates" size="small" max-height="320" @selection-change="(v: GlossaryCandidate[]) => (picked = v)">
              <el-table-column type="selection" width="38" />
              <el-table-column label="原文" min-width="140"><template #default="{ row }"><el-input v-model="row.source" size="small" /></template></el-table-column>
              <el-table-column label="建议译法（可改）" min-width="150"><template #default="{ row }"><el-input v-model="row.target" size="small" placeholder="留空 = 保持原文" /></template></el-table-column>
              <el-table-column label="说明" min-width="110"><template #default="{ row }"><el-input v-model="row.note" size="small" /></template></el-table-column>
              <el-table-column label="来自" width="120"><template #default="{ row }"><small class="mute">{{ row.site || '手动提取' }}<template v-if="row.count > 1"> ×{{ row.count }}</template></small></template></el-table-column>
            </el-table>
            <div class="row act">
              <el-select v-model="addTo" size="small" placeholder="加入到…" class="sel">
                <el-option v-for="g in data.glossaries" :key="g.id" :value="g.id" :label="g.folder ? `${g.folder} / ${g.name}` : g.name" />
                <el-option :value="NEW" label="＋ 新建术语库" />
              </el-select>
              <el-input v-if="addTo === NEW || !addTo" v-model="newName" size="small" class="sel" placeholder="新术语库名称" />
              <el-button size="small" type="primary" :disabled="!picked.length" @click="acceptPicked">加入（{{ picked.length }}）</el-button>
              <el-button size="small" :disabled="!picked.length" @click="ignorePicked">忽略</el-button>
            </div>
          </template>
        </div>

        <div class="box">
          <h4>提取网站</h4>
          <small class="mute">在这些网站上翻译网页 / 视频时，清影会顺手让模型提取术语，放进“待确认”（或你指定的术语库）。需要在扩展里开启“翻译前先判断网页领域”才会对网页触发；视频字幕翻译随时触发。域名写 <span class="mono">example.com</span> 或 <span class="mono">*.example.com</span>。</small>
          <el-table v-if="data.sites.length" :data="data.sites" size="small">
            <el-table-column label="网站" min-width="150"><template #default="{ row }"><span class="mono">{{ row.pattern }}</span></template></el-table-column>
            <el-table-column label="自动提取" width="84"><template #default="{ row }"><el-switch v-model="row.enabled" size="small" @change="saveSites(data.sites)" /></template></el-table-column>
            <el-table-column label="直接加入" min-width="140">
              <template #default="{ row }">
                <el-select v-model="row.glossary" size="small" clearable placeholder="先待确认" @change="saveSites(data.sites)">
                  <el-option v-for="g in data.glossaries" :key="g.id" :value="g.id" :label="g.folder ? `${g.folder} / ${g.name}` : g.name" />
                </el-select>
              </template>
            </el-table-column>
            <el-table-column label="最近 / 累计" min-width="130"><template #default="{ row }"><small class="mute">{{ when(row.last) }} · {{ row.found }} 条</small></template></el-table-column>
            <el-table-column width="60"><template #default="{ row }"><el-button size="small" link type="danger" @click="saveSites(data.sites.filter((s) => s !== row))">移除</el-button></template></el-table-column>
          </el-table>
          <div class="row"><el-input v-model="newSite" size="small" class="mono" placeholder="添加网站，如 youtube.com" @keyup.enter="addSite" /><el-button size="small" :disabled="!newSite.trim()" @click="addSite">添加</el-button></div>
        </div>
      </el-tab-pane>

      <!-- 社区术语库 -->
      <el-tab-pane label="社区术语库" name="community">
        <div class="bar">
          <el-input v-model="cq" size="small" clearable placeholder="搜索名称或说明" class="sel" />
          <el-button size="small" :loading="loadingCommunity" @click="loadCommunity">刷新</el-button>
          <a href="#" @click.prevent="api.openUrl('https://github.com/immersive-translate/terms')">在 GitHub 查看 / 贡献</a>
        </div>
        <small class="mute">来自沉浸式翻译的公开术语库（按“设置 → AI → 翻译成”选择对应语言的版本）。已安装的可以点“更新”拉取最新。</small>
        <div v-if="loadingCommunity && !community.length" class="mute">正在从 GitHub 读取列表…</div>
        <div v-for="c in cshown" :key="c.id" class="item">
          <div class="info">
            <div class="nm">{{ c.name }} <el-tag v-if="installed(c.id)" size="small" type="success" effect="plain">已安装</el-tag></div>
            <small class="mute">{{ c.description }}</small>
            <small class="mute">语言：{{ c.langs.join('、') }}<template v-if="c.matches.length"> · 仅 {{ c.matches.slice(0, 3).join('、') }}{{ c.matches.length > 3 ? '…' : '' }}</template></small>
          </div>
          <el-button size="small" :loading="installing === c.id" @click="install(c)">{{ installed(c.id) ? '更新' : '安装' }}</el-button>
        </div>
      </el-tab-pane>
    </el-tabs>

    <el-dialog append-to-body :model-value="!!editing" width="960px" top="4vh" class="gl-edit" :show-close="false" @close="editing = null">
      <template #header>
        <div class="ehead">
          <div class="et">
            <h2>{{ editing?.id ? '编辑术语库' : '新建术语库' }}</h2>
            <span v-if="editing" class="pz-note">{{ [editing.folder, editing.name].filter(Boolean).join(' / ') }}</span>
          </div>
          <button type="button" class="eclose" aria-label="关闭" @click="editing = null">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </div>
      </template>
      <div v-if="editing" class="ebody">
        <div class="einfo">
          <label class="field"><span>名称</span><el-input v-model="editing.name" placeholder="例如：清影界面" /></label>
          <label class="field"><span>适用的目标语言</span><el-select v-model="editing.lang" placeholder="任意语言"><el-option v-for="l in LANGS" :key="l.v" :value="l.v" :label="l.l" /></el-select></label>
          <label class="field">
            <span>文件夹</span>
            <el-select v-model="editing.folder" filterable allow-create clearable default-first-option placeholder="最外层">
              <el-option v-for="f in allFolders" :key="f" :value="f" :label="f" />
            </el-select>
          </label>
          <label class="field">
            <span>只在这些网站 / 软件生效</span>
            <el-select v-model="editing.matches" multiple filterable allow-create default-first-option collapse-tags collapse-tags-tooltip :reserve-keyword="false" placeholder="全部（留空）" no-data-text="输入网址规则后回车，如 youtube.com、*.bilibili.com">
              <el-option v-for="m in editing.matches" :key="m" :value="m" :label="m" />
            </el-select>
          </label>
          <label class="field wide"><span>说明</span><el-input v-model="editing.description" placeholder="可选：这个术语库是做什么用的" /></label>
        </div>
        <GlossaryTermTree :key="editKey" ref="treeRef" :terms="editing.terms" :can-import="!!editing.id" @path="(s, t) => (curPath = { src: s, tgt: t })" @import-csv="importCsv(editing?.id || '')" />
      </div>
      <template #footer>
        <div class="efoot">
          <div class="einfo2">
            <div class="cur">
              <template v-if="curPath.src.length">
                <span class="pz-note">当前</span>
                <template v-for="(p, k) in curPath.src" :key="k">
                  <span v-if="k" class="sepr">›</span>
                  <b v-if="k === curPath.src.length - 1">{{ p }}</b>
                  <span v-else>{{ p }}</span>
                </template>
                <span class="sepr eq">=</span>
                <span class="mono tp">{{ curPath.tgt.join(' › ') }}</span>
              </template>
              <span v-else class="pz-note">点一条术语，这里显示它的完整路径。译文留空 = 保持原文不翻译。</span>
            </div>
            <div class="keys">
              <span class="lg">层级底色 <i class="sw lv0" />1 <i class="sw lv1" />2 <i class="sw lv2" />3+</span>
              <span><kbd>Enter</kbd> 加同级</span>
              <span><kbd>Tab</kbd> 变成下级</span>
              <span><kbd>Shift+Tab</kbd> 升一层</span>
              <span><kbd>Alt+↑↓</kbd> 上下移动</span>
              <span><kbd>Alt+←→</kbd> 收起 / 展开</span>
            </div>
          </div>
          <el-button class="fb" @click="editing = null">取消</el-button>
          <el-button type="primary" class="pz-strong fb" :loading="busy" @click="saveEditing">保存</el-button>
        </div>
      </template>
    </el-dialog>
  </section>
</template>

<style scoped>
.gl { grid-column: 1 / -1; }
.group { padding: 14px 16px; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
.titlebar { display: flex; justify-content: space-between; align-items: center; }
h3 { margin: 0; font-size: 12px; color: var(--cc-mute); }
h4 { margin: 0; font-size: 13px; }
.bar, .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.act { margin-top: 8px; }
.bar { margin-bottom: 8px; }
.sel { width: 180px; }
.item { display: flex; align-items: center; gap: 10px; padding: 8px 0; border-top: 1px solid var(--cc-border, rgba(128,128,128,.2)); }
.item.off { opacity: .6; }
.item.folder .info { cursor: pointer; }
.fold { all: unset; cursor: pointer; width: 16px; text-align: center; color: var(--cc-mute); }
.info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.nm { font-size: 13px; }
.box { display: flex; flex-direction: column; gap: 8px; padding: 10px 0 14px; border-bottom: 1px solid var(--cc-border, rgba(128,128,128,.2)); }
.box:last-child { border-bottom: 0; }
.empty { padding: 8px 0; }
.edit { display: flex; flex-direction: column; gap: 10px; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
.field { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.field label { font-size: 12px; color: var(--cc-mute); }
.bd { margin-left: 6px; }
@media (max-width: 640px) { .grid { grid-template-columns: 1fr; } }
.tmrow { display: flex; align-items: center; gap: 8px; margin: 6px 0 2px; }

/* 编辑术语库（分层术语） */
.ehead { display: flex; align-items: center; justify-content: space-between; }
.et { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
.et h2 { margin: 0; font-size: 17px; font-weight: 600; }
.eclose { width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 0; border-radius: 6px; background: transparent; color: var(--cc-mute); cursor: pointer; }
.eclose:hover { background: var(--cc-hover); color: var(--cc-fg); }
.ebody { display: flex; flex-direction: column; height: min(640px, 70vh); }
.einfo { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; padding: 0 24px 16px; }
.einfo .wide { grid-column: 1 / -1; }
.einfo .field > span { font-size: 12px; color: var(--cc-fg2); }
.efoot { display: flex; align-items: center; gap: 16px; text-align: left; }
.einfo2 { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
.cur { display: flex; align-items: center; gap: 6px; font-size: 13px; white-space: nowrap; overflow: hidden; }
.cur b { font-weight: 600; }
.sepr { color: var(--cc-mute); }
.sepr.eq { margin: 0 4px; }
.tp { color: var(--cc-acc-text); overflow: hidden; text-overflow: ellipsis; }
.keys { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 12px; font-size: 11.5px; color: var(--cc-mute); }
.keys kbd { display: inline-flex; align-items: center; height: 18px; padding: 0 5px; border: 1px solid var(--cc-line); border-radius: 4px; background: var(--cc-fill); font-family: var(--cc-mono); font-size: 11px; color: var(--cc-fg2); }
.lg { display: inline-flex; align-items: center; gap: 4px; }
.sw { display: inline-block; width: 14px; height: 10px; border-radius: 3px; }
.sw.lv0 { background: var(--cc-lv0); box-shadow: inset 0 0 0 1px var(--cc-lvline0); }
.sw.lv1 { background: var(--cc-lv1); box-shadow: inset 0 0 0 1px var(--cc-lvline1); }
.sw.lv2 { background: var(--cc-lv2); box-shadow: inset 0 0 0 1px var(--cc-lvline2); }
.fb { height: 36px; margin: 0; }
@media (max-width: 760px) { .einfo { grid-template-columns: 1fr 1fr; } }
</style>

<style>
/* el-dialog 的外壳挂在 body 上：去掉默认内边距，让工具条和底栏顶到边 */
.gl-edit { padding: 0; overflow: hidden; border-radius: 12px; }
.gl-edit .el-dialog__header { padding: 18px 24px 14px; margin: 0; }
.gl-edit .el-dialog__body { padding: 0; }
.gl-edit .el-dialog__footer { padding: 14px 24px; border-top: 1px solid var(--cc-line); background: var(--cc-foot); }
</style>
