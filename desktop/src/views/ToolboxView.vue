<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useAppStore, type ToolboxTab } from '../stores/app'
import VideoTools from '../components/toolbox/VideoTools.vue'
import NormalizeTools from '../components/toolbox/NormalizeTools.vue'
import LutTools from '../components/toolbox/LutTools.vue'
import EditorTools from '../components/toolbox/EditorTools.vue'
import SubtitleTools from '../components/toolbox/SubtitleTools.vue'
import TagTools from '../components/toolbox/TagTools.vue'
import AiTools from '../components/toolbox/AiTools.vue'
import JobList from '../components/toolbox/JobList.vue'

const app = useAppStore()
const tab = ref<ToolboxTab>('video')
const preset = ref<string[] | undefined>()
const tool = ref<string | undefined>()
const aiMode = ref<'transcribe' | 'translate' | 'summarize' | undefined>()
const hint = ref<string | undefined>()
const lut = ref<string | undefined>()
/** 从媒体库送进剪辑的文件，剪辑收下就清掉，免得切走再回来又添加一遍。 */
const editPreset = ref<string[] | undefined>()
/** LUT 工作室看过一次之后一直保留，切到别的标签再回来，调了一半的东西还在。 */
const lutSeen = ref(false)
/** 剪辑同理：切走再回来，时间线和正在播放的位置都还在。 */
const editSeen = ref(false)

function consume() {
  const r = app.toolboxRequest
  if (!r) return
  preset.value = r.paths
  tool.value = r.tool
  aiMode.value = r.aiMode
  hint.value = r.hint
  lut.value = r.lut
  tab.value = r.tab ?? 'video'
  editPreset.value = tab.value === 'edit' ? r.paths : undefined
  app.toolboxRequest = null
}
onMounted(consume)
watch(() => app.toolboxRequest, consume)
watch(
  tab,
  (t) => {
    if (t === 'lut') lutSeen.value = true
    if (t === 'edit') editSeen.value = true
  },
  { immediate: true },
)
</script>

<template>
  <div class="page" :class="{ wide: tab === 'edit' }">
    <div class="head">
      <h2>工具箱</h2>
      <el-radio-group v-model="tab" size="small">
        <el-radio-button value="video">视频与音频</el-radio-button>
        <el-radio-button value="normalize">视频规整</el-radio-button>
        <el-radio-button value="edit">剪辑</el-radio-button>
        <el-radio-button value="lut">LUT 工作室</el-radio-button>
        <el-radio-button value="subtitle">字幕</el-radio-button>
        <el-radio-button value="tags">音频标签</el-radio-button>
        <el-radio-button value="ai">AI</el-radio-button>
      </el-radio-group>
    </div>
    <p class="mute note">所有处理都在本机完成，会生成新文件并登记到媒体库，不会修改原文件。需要 ffmpeg（在“设置 → 组件”里安装）。</p>
    <VideoTools v-if="tab === 'video'" :preset="preset" :tool="tool" />
    <NormalizeTools v-else-if="tab === 'normalize'" :preset="preset" :hint="hint" :lut="lut" />
    <SubtitleTools v-else-if="tab === 'subtitle'" />
    <TagTools v-else-if="tab === 'tags'" />
    <AiTools v-else-if="tab === 'ai'" :preset="preset" :mode="aiMode" />
    <EditorTools v-if="editSeen" v-show="tab === 'edit'" :preset="editPreset" @consumed="editPreset = undefined" />
    <LutTools v-if="lutSeen" v-show="tab === 'lut'" :preset="tab === 'lut' ? preset : undefined" />
    <JobList v-if="tab !== 'subtitle' && tab !== 'lut'" class="joblist" />
  </div>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  max-width: 1040px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.page.wide {
  max-width: none;
}
.head {
  display: flex;
  align-items: center;
  gap: 16px;
}
h2 {
  margin: 0;
  font-size: 16px;
}
.note {
  margin: 0;
  font-size: 12px;
}
.joblist {
  margin-top: 6px;
}
</style>
