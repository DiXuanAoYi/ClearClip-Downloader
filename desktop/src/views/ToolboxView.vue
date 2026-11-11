<script setup lang="ts">
import { computed, onMounted, provide, ref, shallowRef, watch } from 'vue'
import { useAppStore } from '../stores/app'
import { ACCEPTS, PREVIEW_KEY, QUICK_TOOLS, byId, idFor, loadRecent, pickKinds, pushRecent, type PreviewSpec } from '../utils/toolbox'
import ToolboxSidebar from '../components/toolbox/ToolboxSidebar.vue'
import FileBasket from '../components/toolbox/FileBasket.vue'
import ToolPreview from '../components/toolbox/ToolPreview.vue'
import VideoTools from '../components/toolbox/VideoTools.vue'
import NormalizeTools from '../components/toolbox/NormalizeTools.vue'
import LutTools from '../components/toolbox/LutTools.vue'
import EditorTools from '../components/toolbox/EditorTools.vue'
import SubtitleTools from '../components/toolbox/SubtitleTools.vue'
import TagTools from '../components/toolbox/TagTools.vue'
import AiTools from '../components/toolbox/AiTools.vue'
import JobList from '../components/toolbox/JobList.vue'

const app = useAppStore()
const cur = ref('compress')
const recent = ref(loadRecent())
const videoTool = ref<string | undefined>()
const hint = ref<string | undefined>()
const lut = ref<string | undefined>()
/** 从媒体库送进剪辑的文件，剪辑收下就清掉，免得切走再回来又添加一遍。 */
const editPreset = ref<string[] | undefined>()
/** LUT 工作室、剪辑看过一次之后一直保留，切走再回来，调了一半的东西还在。 */
const lutSeen = ref(false)
const editSeen = ref(false)

// 文件篮在所有工具之间共用；工具里的“主输入”选择框由它代替
provide('toolBasket', true)
const previewSpec = shallowRef<PreviewSpec | null>(null)
provide(PREVIEW_KEY, previewSpec)

const def = computed(() => byId(cur.value))
const accepts = computed(() => ACCEPTS[cur.value] ?? [])
const basket = computed(() => app.toolFiles)
const usable = computed(() => pickKinds(basket.value, accepts.value).length)

function select(id: string, track = true) {
  cur.value = id
  if (track) recent.value = pushRecent(id)
  const d = byId(id)
  if (d.tab === 'video') {
    if (id === 'quick') videoTool.value = videoTool.value && QUICK_TOOLS.includes(videoTool.value) ? videoTool.value : 'trim'
    else videoTool.value = d.tool
  }
  if (id === 'lut') lutSeen.value = true
  if (id === 'edit') editSeen.value = true
}

function consume() {
  const r = app.toolboxRequest
  if (!r) return
  if (r.paths.length) app.toolFiles = [...r.paths]
  hint.value = r.hint
  lut.value = r.lut
  const id = idFor(r.tab, r.tool, r.aiMode)
  select(id)
  if (r.tool && id === 'quick') videoTool.value = r.tool
  editPreset.value = id === 'edit' ? r.paths : undefined
  app.toolboxRequest = null
}
onMounted(() => {
  if (app.toolboxRequest) consume()
  else select(cur.value, false)
})
watch(() => app.toolboxRequest, consume)

function back() {
  app.view = app.toolsFrom && app.toolsFrom !== 'tools' ? app.toolsFrom : 'parse'
}

/** 预览种类 → 档位（实时 / 试一小段） */
const tier = computed(() => {
  const k = previewSpec.value?.kind
  return k === 'trial' || k === 'loudness' ? 'trial' : 'live'
})
const showPreview = computed(() => !!previewSpec.value && cur.value !== 'edit')
const note = computed(() => {
  const n = usable.value
  if (!n) return ''
  const multi = cur.value === 'quick' && videoTool.value === 'concat'
  return multi ? `${n} 个文件按顺序拼成一个。` : `将处理 ${n} 个文件，每个各生成一个新文件。`
})
const singleFile = computed(() => ['tags', 'subtitle', 'lut'].includes(cur.value))
</script>

<template>
  <div class="tb">
    <ToolboxSidebar :model-value="cur" :recent="recent" @update:model-value="select" @back="back" />
    <div class="content" :class="{ wide: cur === 'edit' }">
      <p class="mute note">所有处理都在本机完成，会生成新文件并登记到媒体库，不会修改原文件。需要 ffmpeg（在“设置 → 组件”里安装）。</p>
      <FileBasket v-if="cur !== 'edit'" v-model="app.toolFiles" :accepts="accepts" :note="singleFile ? (usable > 1 ? '这个工具一次处理一个文件，用篮里的第一个。' : '') : note" />
      <div class="row">
        <div class="tool">
          <VideoTools v-if="def.tab === 'video'" bare :preset="basket" :tool="videoTool" />
          <NormalizeTools v-else-if="def.tab === 'normalize'" :preset="basket" :hint="hint" :lut="lut" :section="def.section" />
          <SubtitleTools v-else-if="def.tab === 'subtitle'" :preset="basket" />
          <TagTools v-else-if="def.tab === 'tags'" :preset="basket" />
          <AiTools v-else-if="def.tab === 'ai'" bare :preset="basket" :mode="def.aiMode" />
          <EditorTools v-if="editSeen" v-show="cur === 'edit'" :preset="editPreset" @consumed="editPreset = undefined" />
          <LutTools v-if="lutSeen" v-show="cur === 'lut'" :preset="cur === 'lut' ? basket : undefined" />
        </div>
        <ToolPreview v-if="showPreview" :files="basket" :spec="previewSpec" :tier="tier" />
      </div>
      <JobList v-if="cur !== 'subtitle' && cur !== 'lut'" class="joblist" />
    </div>
  </div>
</template>

<style scoped>
.tb {
  display: flex;
  align-items: flex-start;
  min-height: 100%;
}
.tb > :first-child {
  position: sticky;
  top: 0;
  height: 100vh;
  align-self: flex-start;
}
.content {
  flex: 1;
  min-width: 0;
  box-sizing: border-box;
  padding: 18px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 1360px;
}
.content.wide {
  max-width: none;
}
.note {
  margin: 0;
  font-size: 12px;
}
.row {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  flex-wrap: wrap;
}
.tool {
  flex: 1;
  min-width: 360px;
}
.joblist {
  margin-top: 6px;
}
</style>
