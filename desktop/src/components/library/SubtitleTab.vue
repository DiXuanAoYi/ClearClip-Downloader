<script setup lang="ts">
// 媒体库条目的“字幕”区：列出视频旁边的所有字幕轨，可校对、AI 纠错、翻译，并把字幕内嵌（软）或烧录（硬）进视频。
import { computed, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { BurnStyle, SubTrackInfo } from '../../types'
import { formatBytes } from '../../utils/format'
import SubtitleEditor from './SubtitleEditor.vue'

const props = defineProps<{ video: string }>()
const app = useAppStore()
const tracks = ref<SubTrackInfo[]>([])
const loading = ref(false)
const editPath = ref('')
const editing = ref(false)
const muxOpen = ref(false)
const mux = reactive({ mode: 'soft' as 'soft' | 'hard', picked: [] as string[], preset: 'standard', busy: false, preview: '', previewing: false })
const presets = ref<[string, string, BurnStyle][]>([])
const style = ref<BurnStyle | null>(null)

async function load() {
  loading.value = true
  try {
    tracks.value = await api.subtitleTracks(props.video)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}
watch(() => props.video, load, { immediate: true })

const hasTracks = computed(() => tracks.value.length > 0)

function edit(t: SubTrackInfo) {
  editPath.value = t.path
  editing.value = true
}
async function job(fn: () => Promise<number>, what: string) {
  try {
    await fn()
    ElMessage.success(`${what}已开始，进度在“工具箱”的任务列表里。`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
const correct = (t: SubTrackInfo) => job(() => api.subgenCorrect(t.path), 'AI 纠错')
const translate = (t: SubTrackInfo) => job(() => api.aiTranslate(t.path), '翻译')
const generate = () => {
  app.goToolbox([props.video], undefined, { tab: 'ai', aiMode: 'transcribe' })
}

async function openMux() {
  if (!presets.value.length) presets.value = await api.subgenBurnPresets()
  mux.picked = tracks.value.length ? [tracks.value[0].path] : []
  pickPreset(mux.preset)
  mux.preview = ''
  muxOpen.value = true
}
function pickPreset(id: string) {
  mux.preset = id
  const p = presets.value.find((x) => x[0] === id)
  if (p) style.value = { ...p[2] }
}
async function preview() {
  if (!style.value || !mux.picked.length) return
  mux.previewing = true
  try {
    mux.preview = await api.subgenBurnPreview(props.video, mux.picked[0], style.value)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    mux.previewing = false
  }
}
async function run() {
  if (!mux.picked.length) return ElMessage.warning('请先选择字幕。')
  mux.busy = true
  try {
    await api.subgenMux(props.video, { mode: mux.mode, tracks: mux.mode === 'hard' ? [mux.picked[0]] : mux.picked, style: mux.mode === 'hard' ? style.value : null })
    ElMessage.success('已开始，进度在“工具箱”的任务列表里，完成后得到一个新文件，原视频不动。')
    muxOpen.value = false
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    mux.busy = false
  }
}
</script>

<template>
  <div v-loading="loading" class="stab">
    <div v-if="!hasTracks" class="mute empty">视频旁边还没有字幕。<el-button size="small" link type="primary" @click="generate">用 AI 生成</el-button></div>
    <div v-for="t in tracks" :key="t.path" class="trk">
      <div class="nm">
        <b>{{ t.label }}</b>
        <span class="mute mono">{{ t.ext.toUpperCase() }} · {{ t.cues }} 条 · {{ formatBytes(t.size) }}<template v-if="t.bilingual"> · 双语</template></span>
      </div>
      <div class="ac">
        <el-button size="small" @click="edit(t)">校对</el-button>
        <el-button size="small" @click="correct(t)">AI 纠错</el-button>
        <el-button size="small" @click="translate(t)">翻译</el-button>
      </div>
    </div>
    <div v-if="hasTracks" class="foot">
      <el-button size="small" @click="openMux">合成到视频…</el-button>
      <el-button size="small" link type="primary" @click="generate">再生成一条</el-button>
    </div>

    <SubtitleEditor v-model="editing" :path="editPath" :video="video" @saved="load" />

    <el-dialog append-to-body v-model="muxOpen" title="把字幕合成到视频" width="min(640px, 94vw)">
      <div class="mx">
        <el-radio-group v-model="mux.mode" size="small">
          <el-radio-button value="soft">软字幕（可多轨，不重新编码）</el-radio-button>
          <el-radio-button value="hard">硬字幕（烧进画面）</el-radio-button>
        </el-radio-group>
        <p class="mute small">
          {{ mux.mode === 'soft' ? '字幕作为独立轨道放进视频，播放器里可以切换；MP4 / MKV / WebM 可用，速度很快。' : '字幕变成画面的一部分，任何播放器都能看到；需要重新编码，较慢。双语字幕第二行可以用不同的字号和颜色。' }}
          输出是视频旁边的新文件，原视频不动。
        </p>
        <div class="pick">
          <span class="mute">{{ mux.mode === 'soft' ? '选择字幕（可多选）' : '选择字幕（只用一个）' }}</span>
          <el-checkbox-group v-if="mux.mode === 'soft'" v-model="mux.picked">
            <el-checkbox v-for="t in tracks" :key="t.path" :value="t.path">{{ t.label }}</el-checkbox>
          </el-checkbox-group>
          <el-radio-group v-else :model-value="mux.picked[0]" @update:model-value="(v: string) => (mux.picked = [v])">
            <el-radio v-for="t in tracks" :key="t.path" :value="t.path">{{ t.label }}</el-radio>
          </el-radio-group>
        </div>
        <template v-if="mux.mode === 'hard' && style">
          <div class="pick">
            <span class="mute">样式</span>
            <el-radio-group :model-value="mux.preset" size="small" @update:model-value="pickPreset">
              <el-radio-button v-for="p in presets" :key="p[0]" :value="p[0]">{{ p[1] }}</el-radio-button>
            </el-radio-group>
          </div>
          <div class="sty">
            <label>第一行字号<el-input-number v-model="style.size" size="small" :min="16" :max="200" /></label>
            <label>第二行字号<el-input-number v-model="style.size2" size="small" :min="16" :max="200" /></label>
            <label>第一行颜色<el-color-picker v-model="style.color" size="small" /></label>
            <label>第二行颜色<el-color-picker v-model="style.color2" size="small" /></label>
            <label>描边<el-input-number v-model="style.outline" size="small" :min="0" :max="12" /></label>
            <label>距边缘<el-input-number v-model="style.margin" size="small" :min="0" :max="600" /></label>
            <label>黑底框<el-switch v-model="style.boxed" /></label>
            <label>加粗<el-switch v-model="style.bold" /></label>
            <label>位置<el-radio-group v-model="style.position" size="small"><el-radio-button value="bottom">底部</el-radio-button><el-radio-button value="top">顶部</el-radio-button></el-radio-group></label>
          </div>
          <div>
            <el-button size="small" :loading="mux.previewing" @click="preview">预览一帧</el-button>
            <span class="mute small"> 字幕大小以 1080 高的画面为准，其他分辨率自动按比例缩放。</span>
          </div>
          <img v-if="mux.preview" :src="mux.preview" class="pvimg" alt="烧录效果预览" />
        </template>
      </div>
      <template #footer>
        <el-button @click="muxOpen = false">取消</el-button>
        <el-button type="primary" :loading="mux.busy" @click="run">开始</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.stab {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.empty {
  font-size: 12px;
}
.trk {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  padding: 6px 8px;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
}
.nm {
  display: flex;
  flex-direction: column;
  min-width: 0;
  font-size: 12px;
}
.ac,
.foot {
  display: flex;
  gap: 6px;
  align-items: center;
}
.mx {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.small {
  font-size: 12px;
  margin: 0;
}
.pick {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sty {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 8px;
}
.sty label {
  display: flex;
  gap: 8px;
  align-items: center;
  font-size: 12px;
}
.pvimg {
  max-width: 100%;
  border-radius: 6px;
  border: 1px solid var(--cc-line);
}
</style>
