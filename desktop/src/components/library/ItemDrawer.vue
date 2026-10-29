<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { Chapter, LibraryItem, TagCount } from '../../types'
import { formatBytes, formatDateTime, formatDuration } from '../../utils/format'
import { formatClock } from '../../utils/time'

const props = defineProps<{ modelValue: boolean; item: LibraryItem | null; tags: TagCount[] }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; changed: [] }>()

const app = useAppStore()
const favorite = ref(false)
const rating = ref(0)
const note = ref('')
const itemTags = ref<string[]>([])
const sheet = ref('')
const sheetLoading = ref(false)
const scenes = ref<Chapter[]>([])
const sceneLoading = ref(false)
const threshold = ref(0.4)
const splitting = ref(false)

watch(
  () => [props.item?.id, props.modelValue] as const,
  () => {
    const i = props.item
    favorite.value = i?.favorite ?? false
    rating.value = i?.rating ?? 0
    note.value = i?.note ?? ''
    itemTags.value = [...(i?.tags ?? [])]
    sheet.value = ''
    scenes.value = []
  },
  { immediate: true },
)

const suggestions = computed(() => props.tags.filter((t) => !itemTags.value.some((x) => x.toLowerCase() === t.name.toLowerCase())).slice(0, 12))
const isVideo = computed(() => props.item?.kind === 'video' && props.item.exists)

async function run(fn: () => Promise<unknown>) {
  try {
    await fn()
    emit('changed')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function setFavorite(v: boolean) {
  favorite.value = v
  if (props.item) run(() => api.librarySetMeta(props.item!.id, { favorite: v }))
}

function setRating(v: number) {
  // 再点一次当前分数清除评分
  const next = v === rating.value ? 0 : v
  rating.value = next
  if (props.item) run(() => api.librarySetMeta(props.item!.id, { rating: next }))
}

function saveNote() {
  if (props.item && note.value !== props.item.note) run(() => api.librarySetMeta(props.item!.id, { note: note.value }))
}

function saveTags() {
  if (props.item) run(() => api.librarySetTags(props.item!.id, itemTags.value))
}

function addTag(name: string) {
  itemTags.value = [...itemTags.value, name]
  saveTags()
}

async function makeSheet() {
  if (!props.item) return
  sheetLoading.value = true
  try {
    sheet.value = convertFileSrc(await api.libraryPreview(props.item.id)) + `?t=${Date.now()}`
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    sheetLoading.value = false
  }
}

async function detect() {
  if (!props.item) return
  sceneLoading.value = true
  scenes.value = []
  try {
    const r = await api.libraryScenes(props.item.id, threshold.value)
    scenes.value = r.scenes
    if (r.scenes.length < 2) ElMessage.info('没有检测到明显的镜头切换，可以把灵敏度调高一些再试。')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    sceneLoading.value = false
  }
}

async function split() {
  if (!props.item) return
  splitting.value = true
  try {
    const dir = await api.librarySplitScenes(props.item.id, scenes.value)
    ElMessage.success(`已拆分为 ${scenes.value.length} 个文件`)
    api.revealFile(dir).catch(() => {})
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    splitting.value = false
  }
}

async function copyUrl() {
  if (!props.item) return
  await run(async () => {
    await api.copyText(props.item!.sourceUrl)
    ElMessage.success('已复制原链接')
  })
}

function toTools() {
  if (!props.item) return
  emit('update:modelValue', false)
  app.goToolbox([props.item.path])
}

function toNormalize() {
  if (!props.item) return
  emit('update:modelValue', false)
  const hint = props.item.source === 'live' ? 'live' : props.item.platform !== 'local' ? 'download' : undefined
  app.goToolbox([props.item.path], undefined, { tab: 'normalize', hint })
}

function toEdit() {
  if (!props.item) return
  emit('update:modelValue', false)
  app.goToolbox([props.item.path], undefined, { tab: 'edit' })
}

function toAi(aiMode: 'transcribe' | 'translate' | 'summarize') {
  if (!props.item) return
  emit('update:modelValue', false)
  app.goToolbox([props.item.path], undefined, { tab: 'ai', aiMode })
}

function cover(i: LibraryItem) {
  return i.coverPath ? convertFileSrc(i.coverPath) : i.cover
}
</script>

<template>
  <el-drawer :model-value="modelValue" size="460px" :title="item?.title ?? ''" @update:model-value="(v: boolean) => emit('update:modelValue', v)">
    <div v-if="item" class="detail">
      <img v-if="cover(item)" :src="cover(item)!" class="cover" referrerpolicy="no-referrer" alt="" />
      <div class="meta mute">
        <div>{{ item.platformName || item.platform }}<template v-if="item.author"> · @{{ item.author }}</template></div>
        <div class="mono">{{ formatBytes(item.size) }}<template v-if="item.durationMs"> · {{ formatDuration(item.durationMs) }}</template> · {{ formatDateTime(item.finishedAt) }}</div>
        <div class="path selectable" :title="item.path">{{ item.path }}</div>
        <div v-if="!item.exists" class="miss">文件已不在原位置</div>
      </div>
      <div class="acts">
        <el-button v-if="item.kind === 'video' || item.kind === 'audio'" size="small" type="primary" :disabled="!item.exists" @click="app.openPlayer(item.path)">播放</el-button>
        <el-button size="small" :type="item.kind === 'video' || item.kind === 'audio' ? undefined : 'primary'" :disabled="!item.exists" @click="api.openFile(item.path)">{{ item.kind === 'video' || item.kind === 'audio' ? '用系统程序打开' : '打开' }}</el-button>
        <el-button size="small" :disabled="!item.exists" @click="api.revealFile(item.path)">所在文件夹</el-button>
        <el-button v-if="item.sourceUrl" size="small" @click="copyUrl">复制原链接</el-button>
        <el-button v-if="item.exists && (item.kind === 'video' || item.kind === 'audio')" size="small" @click="toTools">用工具箱处理…</el-button>
        <el-button v-if="item.exists && item.kind === 'video'" size="small" @click="toNormalize">视频规整…</el-button>
        <el-button v-if="item.exists && item.kind === 'video'" size="small" @click="toEdit">放进剪辑…</el-button>
        <el-button v-if="item.exists && (item.kind === 'video' || item.kind === 'audio')" size="small" @click="toAi('transcribe')">语音转文字</el-button>
        <template v-if="item.exists && item.kind === 'subtitle' && !item.path.endsWith('.xml')">
          <el-button size="small" @click="toAi('translate')">翻译字幕</el-button>
          <el-button size="small" @click="toAi('summarize')">摘要与章节</el-button>
        </template>
      </div>

      <el-divider />
      <div class="field">
        <span class="label">收藏与评分</span>
        <div class="rate">
          <el-button :type="favorite ? 'warning' : 'default'" size="small" @click="setFavorite(!favorite)">{{ favorite ? '★ 已收藏' : '☆ 收藏' }}</el-button>
          <span class="stars" role="radiogroup" aria-label="评分">
            <button v-for="n in 5" :key="n" type="button" class="star" :class="{ on: n <= rating }" :aria-label="`${n} 星`" @click="setRating(n)">★</button>
          </span>
        </div>
      </div>
      <div class="field">
        <span class="label">标签</span>
        <el-input-tag v-model="itemTags" size="small" placeholder="输入标签后回车" :max="20" @change="saveTags" />
        <div v-if="suggestions.length" class="sugg">
          <button v-for="t in suggestions" :key="t.name" type="button" class="chip" @click="addTag(t.name)">+ {{ t.name }}</button>
        </div>
      </div>
      <div class="field">
        <span class="label">备注</span>
        <el-input v-model="note" type="textarea" :rows="3" maxlength="2000" placeholder="写点什么，比如为什么收藏它" @blur="saveNote" />
      </div>

      <template v-if="isVideo">
        <el-divider />
        <div class="field">
          <span class="label">预览图</span>
          <div>
            <el-button size="small" :loading="sheetLoading" @click="makeSheet">{{ sheet ? '重新生成' : '生成预览图' }}</el-button>
            <span class="mute hint">在视频里均匀取 12 帧拼成一张图</span>
          </div>
          <img v-if="sheet" :src="sheet" class="sheet" alt="预览图" />
        </div>
        <div class="field">
          <span class="label">镜头检测</span>
          <div class="scene-bar">
            <span class="mute">灵敏度</span>
            <el-slider v-model="threshold" :min="0.1" :max="0.8" :step="0.05" :show-tooltip="false" class="sl" />
            <el-button size="small" :loading="sceneLoading" @click="detect">检测</el-button>
          </div>
          <div v-if="scenes.length" class="scenes">
            <div v-for="(s, i) in scenes" :key="i" class="sc mono"><span>{{ s.title }}</span><span class="mute">{{ formatClock(s.startMs) }} – {{ formatClock(s.endMs) }}</span></div>
          </div>
          <el-button v-if="scenes.length > 1" size="small" type="primary" :loading="splitting" @click="split">拆分成 {{ scenes.length }} 个文件</el-button>
          <small v-if="scenes.length > 1" class="mute">不重新编码，速度很快；起点会落在最近的关键帧上，可能有零点几秒的偏差。文件保存在视频旁边的“镜头”文件夹里。</small>
        </div>
      </template>
    </div>
  </el-drawer>
</template>

<style scoped>
.detail {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.cover {
  width: 100%;
  max-height: 220px;
  object-fit: contain;
  background: var(--cc-line);
  border-radius: 8px;
}
.meta {
  font-size: 12px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.path {
  word-break: break-all;
  font-size: 11px;
}
.miss {
  color: var(--cc-err);
}
.acts {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.label {
  font-size: 11.5px;
  color: var(--cc-mute);
}
.rate {
  display: flex;
  align-items: center;
  gap: 14px;
}
.stars {
  display: inline-flex;
  gap: 2px;
}
.star {
  all: unset;
  cursor: pointer;
  font-size: 20px;
  line-height: 1;
  color: var(--cc-line);
}
.star.on {
  color: #e6a23c;
}
.star:focus-visible {
  outline: 2px solid var(--cc-acc);
  border-radius: 3px;
}
.sugg {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.sugg .chip {
  all: unset;
  cursor: pointer;
  font-size: 11px;
  padding: 0 8px;
  line-height: 18px;
  border: 1px dashed var(--cc-line);
  border-radius: 999px;
  color: var(--cc-mute);
}
.sugg .chip:hover {
  border-color: var(--cc-acc);
  color: var(--cc-acc);
}
.hint {
  margin-left: 10px;
  font-size: 11.5px;
}
.sheet {
  width: 100%;
  border-radius: 6px;
  border: 1px solid var(--cc-line);
}
.scene-bar {
  display: flex;
  gap: 12px;
  align-items: center;
}
.sl {
  flex: 1;
}
.scenes {
  max-height: 160px;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 4px 10px;
}
.sc {
  display: flex;
  justify-content: space-between;
  font-size: 11.5px;
  line-height: 22px;
}
</style>
