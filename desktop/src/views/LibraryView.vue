<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api, errorText, events } from '../api'
import VirtualList from '../components/VirtualList.vue'
import InboxPanel from '../components/InboxPanel.vue'
import ItemDrawer from '../components/library/ItemDrawer.vue'
import LibraryTools, { type ToolTab } from '../components/library/LibraryTools.vue'
import { useAppStore, useParseStore, useQueueStore } from '../stores/app'
import type { HistoryItem, LibraryFilter, LibraryItem, PlatformCount, TagCount } from '../types'
import { formatBytes, formatDateTime, formatDuration } from '../utils/format'

const app = useAppStore()
const parse = useParseStore()
const queue = useQueueStore()

const tab = computed({ get: () => app.libraryTab, set: (v) => (app.libraryTab = v) })
const query = ref('')
const files = ref<LibraryItem[]>([])
const history = ref<HistoryItem[]>([])
const loading = ref(false)
const platforms = ref<PlatformCount[]>([])
const fPlatform = ref('')
const fKind = ref('')
const fSince = ref('')
const fMissing = ref(false)
const fTag = ref('')
const fFavorite = ref(false)
const fRating = ref(0)
const fSort = ref<NonNullable<LibraryFilter['sort']>>('finished')
const tags = ref<TagCount[]>([])
const mode = ref<'list' | 'grid'>('list')
const COLS = 5

// 多选
const selecting = ref(false)
const selected = ref<Set<number>>(new Set())
// 详情抽屉与工具
const detailId = ref<number | null>(null)
const detailOpen = ref(false)
const detail = computed(() => files.value.find((f) => f.id === detailId.value) ?? null)
const toolsOpen = ref(false)
const toolTab = ref<ToolTab>('stats')

function openTools(tab: ToolTab) {
  toolTab.value = tab
  toolsOpen.value = true
}

function openDetail(f: LibraryItem) {
  detailId.value = f.id
  detailOpen.value = true
}

function toggleSelect(id: number) {
  const next = new Set(selected.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selected.value = next
}

function selectAll() {
  selected.value = new Set(files.value.map((f) => f.id))
}

function stopSelecting() {
  selecting.value = false
  selected.value = new Set()
}

const selectedIds = computed(() => [...selected.value])

async function askTags(title: string): Promise<string[] | null> {
  try {
    const { value } = await ElMessageBox.prompt('多个标签用逗号分隔', title, { confirmButtonText: '确定', cancelButtonText: '取消', inputPattern: /\S/, inputErrorMessage: '请输入标签' })
    return String(value).split(/[,，、]/).map((t) => t.trim()).filter(Boolean)
  } catch {
    return null
  }
}

async function bulkTag(remove: boolean) {
  const list = await askTags(remove ? '去掉标签' : '添加标签')
  if (!list?.length) return
  await run(api.libraryBulkTags, selectedIds.value, list, remove)
  await load()
}

async function bulkFavorite(on: boolean) {
  await run(api.libraryBulkFavorite, selectedIds.value, on)
  await load()
}

async function bulkDelete() {
  const trash = app.settings?.library.useTrash ?? true
  try {
    await ElMessageBox.confirm(`${selected.value.size} 项：${trash ? '文件会移到回收站，可以还原。' : '文件会被永久删除。'}只想去掉记录而保留文件请选“只删除记录”。`, '删除所选', {
      confirmButtonText: trash ? '移到回收站' : '删除记录和文件',
      cancelButtonText: '只删除记录',
      distinguishCancelAndClose: true,
      type: 'warning',
    })
    await removeMany(true)
  } catch (action) {
    if (action === 'cancel') await removeMany(false)
  }
}

/** 把所选的视频交给工具箱的“视频规整”（来源提示取第一个：直播录制 / 平台下载）。 */
function bulkNormalize() {
  const videos = files.value.filter((i) => selected.value.has(i.id) && i.kind === 'video' && i.exists)
  if (!videos.length) {
    ElMessage.info('所选的项目里没有可以规整的视频文件。')
    return
  }
  const first = videos[0]
  app.goToolbox(videos.map((v) => v.path), undefined, { tab: 'normalize', hint: first.source === 'live' ? 'live' : first.platform !== 'local' ? 'download' : undefined })
}

async function bulkSafebox() {
  const st = await api.safeboxStatus().catch(() => null)
  if (!st?.unlocked) {
    ElMessage.info(st?.exists ? '请先到“保险箱”页面输入密码解锁。' : '还没有创建保险箱，请先到“保险箱”页面创建。')
    app.view = 'safebox'
    return
  }
  try {
    await ElMessageBox.confirm(`把所选 ${selected.value.size} 项加密放进保险箱？加密完成后，它们会从媒体库里消失，原文件和封面缓存会被覆盖删除。`, '移入保险箱', {
      confirmButtonText: '移入',
      cancelButtonText: '取消',
      type: 'warning',
    })
  } catch {
    return
  }
  try {
    await api.safeboxAddLibrary(selectedIds.value)
    ElMessage.success('已开始加密，进度在“保险箱”页面查看')
    stopSelecting()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function removeMany(deleteFiles: boolean) {
  try {
    const r = await api.libraryDelete(selectedIds.value, deleteFiles)
    if (r.failed.length) ElMessage.warning(`${r.failed.length} 项失败：${r.failed[0]}`)
    else ElMessage.success(r.trashed ? `已把 ${r.trashed} 个文件移到回收站` : `已删除 ${r.removed} 项`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
  stopSelecting()
  await load()
}

function setFavorite(f: LibraryItem) {
  const next = !f.favorite
  f.favorite = next
  api.librarySetMeta(f.id, { favorite: next }).catch((e) => {
    f.favorite = !next
    ElMessage.error(errorText(e))
  })
}

const gridRows = computed(() => {
  const rows: { key: number; items: LibraryItem[] }[] = []
  for (let i = 0; i < files.value.length; i += COLS) rows.push({ key: files.value[i].id, items: files.value.slice(i, i + COLS) })
  return rows
})

function coverOf(f: LibraryItem): string | null {
  if (f.coverPath) return convertFileSrc(f.coverPath)
  return f.cover
}

function sinceValue(): number | null {
  const day = 86400
  const now = Math.floor(Date.now() / 1000)
  if (fSince.value === 'today') {
    const d = new Date()
    d.setHours(0, 0, 0, 0)
    return Math.floor(d.getTime() / 1000)
  }
  if (fSince.value === '7d') return now - 7 * day
  if (fSince.value === '30d') return now - 30 * day
  return null
}

async function redownload(f: LibraryItem) {
  try {
    const r = await api.redownload(f.id)
    if (r.tasks.length) ElMessage.success('已重新加入下载队列')
    else ElMessage.info('已在队列中')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

const platformName = computed<Record<string, string>>(() => Object.fromEntries((app.info?.providers ?? []).map((p) => [p.id, p.name])))

const KIND_NAME: Record<string, string> = { video: '视频', image: '图片', audio: '音频', cover: '封面', subtitle: '字幕' }

function assetName(f: LibraryItem) {
  const id = f.assetId
  if (id.startsWith('image-')) return `图片 ${Number(id.slice(6)) + 1}`
  return ({ video: '视频', music: '背景音乐', cover: '封面' } as Record<string, string>)[id] ?? KIND_NAME[f.kind] ?? id
}

async function load() {
  loading.value = true
  try {
    if (tab.value === 'inbox') return
    if (tab.value === 'files') {
      files.value = await api.listLibrary({
        query: query.value,
        platform: fPlatform.value || null,
        kind: fKind.value || null,
        since: sinceValue(),
        missingOnly: fMissing.value,
        tag: fTag.value || null,
        favoriteOnly: fFavorite.value,
        minRating: fRating.value,
        sort: fSort.value,
      })
      ;[platforms.value, tags.value] = await Promise.all([api.libraryPlatforms(), api.libraryTags()])
      if (fTag.value && !tags.value.some((t) => t.name.toLowerCase() === fTag.value.toLowerCase())) fTag.value = ''
    }
    else history.value = await api.listHistory(query.value)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}

let timer: number | undefined
watch(query, () => {
  window.clearTimeout(timer)
  timer = window.setTimeout(load, 250)
})
watch([tab, fPlatform, fKind, fSince, fMissing, fTag, fFavorite, fRating, fSort], load)
// 有任务完成时刷新媒体库
watch(
  () => queue.counts.done,
  () => tab.value === 'files' && load(),
)
function consumeQuery() {
  if (app.libraryQuery !== null) {
    query.value = app.libraryQuery
    app.libraryQuery = null
  }
}
watch(() => app.libraryQuery, consumeQuery)

onMounted(async () => {
  consumeQuery()
  await load()
  await events.onLibraryChanged(() => tab.value === 'files' && load())
})

async function run<A extends unknown[]>(fn: (...args: A) => Promise<unknown>, ...args: A) {
  try {
    await fn(...args)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function removeFile(item: LibraryItem) {
  try {
    const trash = app.settings?.library.useTrash ?? true
    await ElMessageBox.confirm(`删除“${item.title}”的记录。是否同时处理磁盘上的文件？${trash ? '（文件会先移到回收站，可以还原）' : ''}`, '删除记录', {
      confirmButtonText: trash ? '移到回收站' : '删除记录和文件',
      cancelButtonText: '只删除记录',
      distinguishCancelAndClose: true,
      type: 'warning',
    })
    await run(api.deleteLibrary, item.id, true)
  } catch (action) {
    if (action === 'cancel') await run(api.deleteLibrary, item.id, false)
    else return
  }
  await load()
}

function reopen(item: HistoryItem) {
  parse.result = item.info
  parse.text = item.sourceUrl
  parse.error = ''
  app.view = 'parse'
}

function reparse(item: HistoryItem) {
  app.view = 'parse'
  parse.parse(item.sourceUrl)
}

async function clearHistory() {
  try {
    await ElMessageBox.confirm('清空全部解析历史？已下载的文件不受影响。', '清空历史', { type: 'warning', confirmButtonText: '清空', cancelButtonText: '取消' })
  } catch {
    return
  }
  await run(api.clearHistory)
  await load()
  await parse.loadRecent()
}
</script>

<template>
  <div class="page">
    <div class="head">
      <el-radio-group v-model="tab" size="small">
        <el-radio-button value="files">已下载</el-radio-button>
        <el-radio-button value="inbox">收到的链接<template v-if="app.inboxUnhandled"> ({{ app.inboxUnhandled }})</template></el-radio-button>
        <el-radio-button value="history">解析历史</el-radio-button>
      </el-radio-group>
      <el-input v-model="query" size="small" clearable :placeholder="tab === 'inbox' ? '搜索链接、标题或设备' : '搜索标题或作者'" class="search" />
      <el-button v-if="tab === 'files' && app.settings" size="small" @click="run(api.revealFile, app.settings!.downloadDir)">打开下载目录</el-button>
      <el-button v-if="tab === 'files'" size="small" :type="selecting ? 'primary' : 'default'" @click="selecting ? stopSelecting() : (selecting = true)">{{ selecting ? '退出多选' : '多选' }}</el-button>
      <el-dropdown v-if="tab === 'files'" trigger="click" @command="(c: ToolTab) => openTools(c)">
        <el-button size="small">工具 ▾</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="stats">统计与清理</el-dropdown-item>
            <el-dropdown-item command="dupes">重复文件</el-dropdown-item>
            <el-dropdown-item command="cues">字幕搜索</el-dropdown-item>
            <el-dropdown-item command="reorg">整理文件夹</el-dropdown-item>
            <el-dropdown-item command="trash">回收站</el-dropdown-item>
            <el-dropdown-item command="backup">导入与备份</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
      <el-radio-group v-if="tab === 'files'" v-model="mode" size="small">
        <el-radio-button value="list">列表</el-radio-button>
        <el-radio-button value="grid">网格</el-radio-button>
      </el-radio-group>
      <el-button v-if="tab === 'history' && history.length" size="small" @click="clearHistory">清空历史</el-button>
    </div>

    <div v-if="tab === 'files'" class="filters">
      <el-select v-model="fPlatform" size="small" clearable placeholder="全部平台" class="fsel">
        <el-option v-for="p in platforms" :key="p.platform" :value="p.platform" :label="`${platformName[p.platform] ?? p.name}（${p.count}）`" />
      </el-select>
      <el-select v-model="fKind" size="small" clearable placeholder="全部类型" class="fsel">
        <el-option value="video" label="视频" />
        <el-option value="image" label="图片" />
        <el-option value="audio" label="音频" />
        <el-option value="cover" label="封面" />
        <el-option value="subtitle" label="字幕 / 弹幕" />
      </el-select>
      <el-select v-model="fSince" size="small" clearable placeholder="全部时间" class="fsel">
        <el-option value="today" label="今天" />
        <el-option value="7d" label="最近 7 天" />
        <el-option value="30d" label="最近 30 天" />
      </el-select>
      <el-select v-if="tags.length" v-model="fTag" size="small" clearable placeholder="全部标签" class="fsel">
        <el-option v-for="t in tags" :key="t.name" :value="t.name" :label="`${t.name}（${t.count}）`" />
      </el-select>
      <el-select v-model="fRating" size="small" class="fsel narrow">
        <el-option :value="0" label="不限评分" />
        <el-option v-for="n in 5" :key="n" :value="n" :label="`${n} 星以上`" />
      </el-select>
      <el-select v-model="fSort" size="small" class="fsel narrow">
        <el-option value="finished" label="最近下载" />
        <el-option value="size" label="文件大小" />
        <el-option value="title" label="标题" />
        <el-option value="rating" label="评分" />
      </el-select>
      <el-checkbox v-model="fFavorite" size="small">只看收藏</el-checkbox>
      <el-checkbox v-model="fMissing" size="small">只看文件已丢失的</el-checkbox>
      <span class="mute count">{{ files.length }} 项</span>
    </div>

    <div v-if="tab === 'files' && selecting" class="bulk card">
      <span>已选 <b>{{ selected.size }}</b> 项</span>
      <el-button link size="small" type="primary" @click="selectAll">全选当前列表</el-button>
      <el-button link size="small" @click="selected = new Set()">清除选择</el-button>
      <span class="sp" />
      <el-button size="small" :disabled="!selected.size" @click="bulkTag(false)">加标签</el-button>
      <el-button size="small" :disabled="!selected.size" @click="bulkTag(true)">去标签</el-button>
      <el-button size="small" :disabled="!selected.size" @click="bulkFavorite(true)">收藏</el-button>
      <el-button size="small" :disabled="!selected.size" @click="bulkFavorite(false)">取消收藏</el-button>
      <el-button size="small" :disabled="!selected.size" @click="bulkNormalize">视频规整</el-button>
      <el-button size="small" :disabled="!selected.size" @click="bulkSafebox">移入保险箱</el-button>
      <el-button size="small" type="danger" plain :disabled="!selected.size" @click="bulkDelete">删除</el-button>
    </div>

    <InboxPanel v-if="tab === 'inbox'" :query="query" />
    <div v-else v-loading="loading" class="list">
      <template v-if="tab === 'files'">
        <div v-if="!files.length" class="empty mute">{{ query || fPlatform || fKind || fSince || fMissing ? '没有匹配的文件。' : '还没有下载过文件。' }}</div>
        <VirtualList v-else-if="mode === 'grid'" :items="gridRows" :item-height="190" :gap="10" :item-key="(r) => r.key" class="vl">
          <template #default="{ item: r }">
            <div class="grid-row">
              <div
                v-for="f in r.items"
                :key="f.id"
                class="tile card"
                :class="{ missing: !f.exists, picked: selected.has(f.id) }"
                :title="f.title"
                @click="selecting ? toggleSelect(f.id) : openDetail(f)"
                @dblclick="!selecting && f.exists && ((f.kind === 'video' || f.kind === 'audio') ? app.openPlayer(f.path) : run(api.openFile, f.path))"
              >
                <div class="imgbox">
                  <img v-if="coverOf(f)" :src="coverOf(f)!" referrerpolicy="no-referrer" loading="lazy" alt="" />
                  <div v-else class="noimg" />
                  <span v-if="f.favorite" class="fav">★</span>
                  <el-checkbox v-if="selecting" class="pick" :model-value="selected.has(f.id)" @click.stop @change="toggleSelect(f.id)" />
                  <span v-if="f.durationMs" class="dur mono">{{ formatDuration(f.durationMs) }}</span>
                </div>
                <div class="tname ellipsis">{{ f.title }}</div>
                <small class="mute ellipsis">{{ platformName[f.platform] ?? f.platformName ?? f.platform }}<template v-if="!f.exists"> · 已丢失</template></small>
              </div>
            </div>
          </template>
        </VirtualList>
        <VirtualList v-else :items="files" :item-height="selecting || files.some((f) => f.tags.length) ? 74 : 58" :gap="8" :item-key="(f) => f.id" class="vl">
          <template #default="{ item: f }">
        <div class="item card" :class="{ missing: !f.exists, picked: selected.has(f.id), selecting }">
          <el-checkbox v-if="selecting" :model-value="selected.has(f.id)" @change="toggleSelect(f.id)" />
          <img v-if="coverOf(f)" :src="coverOf(f)!" class="thumb" referrerpolicy="no-referrer" loading="lazy" alt="" />
          <div v-else class="thumb" />
          <div class="info">
            <div class="titleline">
              <button type="button" class="star" :class="{ on: f.favorite }" :aria-label="f.favorite ? '取消收藏' : '收藏'" @click="setFavorite(f)">{{ f.favorite ? '★' : '☆' }}</button>
              <span class="ellipsis ttl" :title="f.title" @click="selecting ? toggleSelect(f.id) : openDetail(f)">{{ f.title }}</span>
              <span v-if="f.rating" class="rate" :aria-label="`${f.rating} 星`">{{ '★'.repeat(f.rating) }}</span>
            </div>
            <small class="mono mute ellipsis selectable" :title="f.path">
              {{ platformName[f.platform] ?? (f.platformName || f.platform) }} · {{ assetName(f) }} · {{ formatBytes(f.size) }}<template v-if="f.durationMs"> · {{ formatDuration(f.durationMs) }}</template> · {{ formatDateTime(f.finishedAt) }}
              <template v-if="!f.exists"> · 文件已不存在</template>
            </small>
            <div v-if="f.tags.length" class="tagline">
              <button v-for="t in f.tags.slice(0, 5)" :key="t" type="button" class="tg" @click="fTag = t">{{ t }}</button>
              <span v-if="f.tags.length > 5" class="mute">+{{ f.tags.length - 5 }}</span>
            </div>
          </div>
          <div class="actions">
            <el-button link size="small" @click="openDetail(f)">详情</el-button>
            <el-button v-if="f.kind === 'video' || f.kind === 'audio'" link size="small" type="primary" :disabled="!f.exists" @click="app.openPlayer(f.path)">播放</el-button>
            <el-button link size="small" :type="f.kind === 'video' || f.kind === 'audio' ? undefined : 'primary'" :disabled="!f.exists" @click="run(api.openFile, f.path)">打开</el-button>
            <el-button link size="small" :disabled="!f.exists" @click="run(api.revealFile, f.path)">文件夹</el-button>
            <el-button v-if="!f.exists && f.sourceUrl" link size="small" type="primary" @click="redownload(f)">重新下载</el-button>
            <el-button link size="small" @click="removeFile(f)">删除</el-button>
          </div>
        </div>
          </template>
        </VirtualList>
      </template>

      <template v-else>
        <div v-if="!history.length" class="empty mute">{{ query ? '没有匹配的记录。' : '还没有解析记录。' }}</div>
        <div v-for="h in history" :key="h.id" class="item card">
          <img v-if="h.cover" :src="h.cover" class="thumb" referrerpolicy="no-referrer" alt="" />
          <div v-else class="thumb" />
          <div class="info">
            <div class="ellipsis" :title="h.title">{{ h.title }}</div>
            <small class="mono mute ellipsis">
              {{ h.info.platformName }} · {{ h.kind === 'video' ? '视频' : '图集' }}<template v-if="h.author"> · @{{ h.author }}</template> · {{ formatDateTime(h.createdAt) }}
            </small>
          </div>
          <div class="actions">
            <el-button link size="small" type="primary" @click="reopen(h)">查看</el-button>
            <el-button link size="small" @click="reparse(h)">重新解析</el-button>
            <el-button link size="small" @click="run(api.deleteHistory, h.id).then(load)">删除</el-button>
          </div>
        </div>
      </template>
    </div>
    <ItemDrawer v-model="detailOpen" :item="detail" :tags="tags" @changed="load" />
    <LibraryTools v-model="toolsOpen" v-model:tab="toolTab" @changed="load" />
  </div>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 1080px;
  height: 100%;
  box-sizing: border-box;
}
.filters {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.fsel {
  width: 150px;
}
.fsel.narrow {
  width: 110px;
}
.count {
  font-size: 12px;
  margin-left: auto;
}
.bulk {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  font-size: 12px;
}
.bulk .sp {
  flex: 1;
}
.bulk :deep(.el-button) {
  margin-left: 0;
}
.picked {
  border-color: var(--cc-acc);
  background: var(--cc-acc-soft);
}
.vl {
  flex: 1;
  margin-right: -8px;
  padding-right: 8px;
}
.grid-row {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 10px;
  height: 100%;
}
.tile {
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  cursor: default;
}
.tile .imgbox {
  position: relative;
}
.tile img,
.tile .noimg {
  width: 100%;
  height: 130px;
  object-fit: cover;
  border-radius: 5px;
  background: var(--cc-line);
  display: block;
}
.tile .fav {
  position: absolute;
  top: 4px;
  right: 6px;
  color: #f5b73b;
  text-shadow: 0 0 3px #000a;
}
.tile .pick {
  position: absolute;
  top: 2px;
  left: 4px;
}
.tile .dur {
  position: absolute;
  right: 4px;
  bottom: 4px;
  font-size: 10px;
  background: #000a;
  color: #fff;
  padding: 0 4px;
  border-radius: 3px;
}
.tile .tname {
  font-size: 12px;
}
.tile small {
  font-size: 10.5px;
}
.tile.missing {
  opacity: 0.55;
}
.head {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.head :deep(.el-button) {
  margin-left: 0;
}
.search {
  width: 240px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 120px;
  flex: 1;
}
.empty {
  padding: 40px 0;
  text-align: center;
}
.item {
  height: 100%;
  box-sizing: border-box;
  display: grid;
  grid-template-columns: 32px 1fr auto;
  gap: 12px;
  align-items: center;
  padding: 8px 12px;
}
.item.selecting {
  grid-template-columns: 24px 32px 1fr auto;
}
.item .thumb {
  width: 32px;
  height: 42px;
}
.titleline {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.ttl {
  cursor: pointer;
}
.ttl:hover {
  color: var(--cc-acc);
}
.star {
  all: unset;
  cursor: pointer;
  color: var(--cc-mute);
  font-size: 14px;
  line-height: 1;
}
.star.on {
  color: #e6a23c;
}
.star:focus-visible {
  outline: 2px solid var(--cc-acc);
  border-radius: 3px;
}
.rate {
  color: #e6a23c;
  font-size: 10px;
  letter-spacing: -1px;
  flex: none;
}
.tagline {
  display: flex;
  gap: 4px;
  align-items: center;
  font-size: 10.5px;
  margin-top: 1px;
}
.tg {
  all: unset;
  cursor: pointer;
  font-size: 10.5px;
  padding: 0 6px;
  line-height: 16px;
  border-radius: 999px;
  background: var(--cc-acc-soft);
  color: var(--cc-acc);
}
.item.missing {
  opacity: 0.6;
}
.info {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.info small {
  font-size: 10.5px;
}
.actions {
  display: flex;
  gap: 2px;
}
.actions :deep(.el-button) {
  margin-left: 6px;
}
</style>
