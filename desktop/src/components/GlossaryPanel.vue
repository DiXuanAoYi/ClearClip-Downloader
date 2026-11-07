<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { open, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../api'
import type { AiSettings, CommunityGlossary, ExtractSite, Glossary, GlossaryCandidate, GlossaryData, GlossaryTerm } from '../types'

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
const matchText = ref('')
const search = ref('')
const PAGE = 300
/** 显示从第几条开始（术语很多时只显示 300 条，新加的那行要在显示范围里） */
const winStart = ref(0)
const filtered = computed(() => {
  const q = search.value.trim().toLowerCase()
  const terms = editing.value?.terms || []
  return q ? terms.filter((t) => t.source.toLowerCase().includes(q) || t.target.toLowerCase().includes(q)) : terms
})
const shown = computed(() => filtered.value.slice(winStart.value, winStart.value + PAGE))
/** 当前激活的行（点过、或正在输入的那一行） */
const active = ref<GlossaryTerm | null>(null)
const termTable = ref<{ setCurrentRow: (row?: GlossaryTerm) => void } | null>(null)

function blank(folder = ''): Glossary {
  return { id: '', name: '', description: '', enabled: true, origin: 'user', remoteId: '', matches: [], lang: '', terms: [], updated: 0, folder }
}
function edit(g?: Glossary, folder = '') {
  editing.value = g ? JSON.parse(JSON.stringify(g)) : blank(folder)
  matchText.value = (editing.value?.matches || []).join('\n')
  search.value = ''
  winStart.value = 0
  active.value = null
}
/** 添加一行：插在当前激活的那一行下面；没有激活的行就加在最下面。新行变成激活行，光标放进去。 */
async function addRow() {
  const g = editing.value
  if (!g) return
  const row: GlossaryTerm = { source: '', target: '', note: '' }
  const at = active.value ? g.terms.indexOf(active.value) : -1
  const idx = at >= 0 ? at + 1 : g.terms.length
  g.terms.splice(idx, 0, row)
  // 搜索时新行（空的）会被筛掉：清掉搜索
  if (search.value) search.value = ''
  if (idx < winStart.value || idx >= winStart.value + PAGE) winStart.value = Math.max(0, idx - Math.floor(PAGE / 2))
  active.value = row
  await nextTick()
  termTable.value?.setCurrentRow(row)
  await nextTick()
  const el = document.querySelector('.gl-terms .current-row') as HTMLElement | null
  el?.scrollIntoView({ block: 'nearest' })
  el?.querySelector('input')?.focus()
}
function dropRow(t: GlossaryTerm) {
  if (!editing.value) return
  editing.value.terms = editing.value.terms.filter((x) => x !== t)
  if (active.value === t) active.value = null
}
async function saveEditing() {
  const g = editing.value
  if (!g) return
  g.matches = matchText.value.split('\n').map((s) => s.trim()).filter(Boolean)
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
    await ElMessageBox.confirm(`删除术语库“${g.name}”（${g.terms.length} 条）？`, '删除术语库', { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' })
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
    ElMessage.success(`已导入 ${g.terms.length} 条`)
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
    ElMessage.success(`已安装“${g.name}”（${g.terms.length} 条，${langName(g.lang)}）`)
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
              <small class="mute">{{ r.g.terms.length }} 条 · {{ langName(r.g.lang) }} · {{ r.g.matches.length ? '仅 ' + r.g.matches.slice(0, 3).join('、') + (r.g.matches.length > 3 ? '…' : '') : '所有网站' }}</small>
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

    <el-dialog :model-value="!!editing" :title="editing?.id ? '编辑术语库' : '新建术语库'" width="760px" top="6vh" @close="editing = null">
      <div v-if="editing" class="edit">
        <div class="grid">
          <div class="field"><label>名称</label><el-input v-model="editing.name" size="small" /></div>
          <div class="field"><label>适用的目标语言</label><el-select v-model="editing.lang" size="small"><el-option v-for="l in LANGS" :key="l.v" :value="l.v" :label="l.l" /></el-select></div>
        </div>
        <div class="grid">
          <div class="field"><label>说明</label><el-input v-model="editing.description" size="small" /></div>
          <div class="field">
            <label>文件夹（多层用 / 分隔，可以直接输入新的）</label>
            <el-select v-model="editing.folder" size="small" filterable allow-create clearable default-first-option placeholder="最外层">
              <el-option v-for="f in allFolders" :key="f" :value="f" :label="f" />
            </el-select>
          </div>
        </div>
        <div class="field"><label>只在这些网站生效（每行一个；留空 = 所有网站）</label><el-input v-model="matchText" type="textarea" :rows="2" class="mono" placeholder="youtube.com&#10;*.bilibili.com&#10;https://github.com/*" /></div>
        <div class="row">
          <el-input v-model="search" size="small" clearable placeholder="搜索术语" class="sel" @input="winStart = 0" />
          <el-button size="small" :title="active ? '插在当前选中的那一行下面' : '加在最下面'" @click="addRow">添加一行</el-button>
          <el-button v-if="editing?.id" size="small" @click="importCsv(editing?.id || '')">并入 CSV…</el-button>
          <small class="mute">共 {{ editing?.terms.length }} 条；译文留空 = 保持原文不翻译</small>
        </div>
        <el-table ref="termTable" class="gl-terms" :data="shown" size="small" max-height="340" highlight-current-row @current-change="(r: GlossaryTerm | undefined) => r && (active = r)" @row-click="(r: GlossaryTerm) => (active = r)">
          <el-table-column label="原文" min-width="160"><template #default="{ row }"><el-input v-model="row.source" size="small" @focus="active = row" /></template></el-table-column>
          <el-table-column label="译文" min-width="160"><template #default="{ row }"><el-input v-model="row.target" size="small" @focus="active = row" /></template></el-table-column>
          <el-table-column label="说明" min-width="120"><template #default="{ row }"><el-input v-model="row.note" size="small" @focus="active = row" /></template></el-table-column>
          <el-table-column width="56"><template #default="{ row }"><el-button size="small" link type="danger" @click="dropRow(row)">删</el-button></template></el-table-column>
        </el-table>
        <div v-if="filtered.length > PAGE" class="row">
          <small class="mute">共 {{ filtered.length }} 条，显示第 {{ winStart + 1 }}–{{ Math.min(filtered.length, winStart + PAGE) }} 条</small>
          <el-button size="small" link :disabled="winStart === 0" @click="winStart = Math.max(0, winStart - PAGE)">上一页</el-button>
          <el-button size="small" link :disabled="winStart + PAGE >= filtered.length" @click="winStart = winStart + PAGE">下一页</el-button>
        </div>
      </div>
      <template #footer>
        <el-button size="small" @click="editing = null">取消</el-button>
        <el-button size="small" type="primary" :loading="busy" @click="saveEditing">保存</el-button>
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
</style>
