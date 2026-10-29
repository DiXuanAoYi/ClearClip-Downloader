<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../../api'
import type { MediaInfoLite, ToolJob, ToolOp } from '../../types'
import { formatDuration } from '../../utils/format'
import { parseClock } from '../../utils/time'
import FileInput from './FileInput.vue'

const props = defineProps<{ preset?: string[]; tool?: string }>()
const emit = defineEmits<{ started: [] }>()

type Tool = { id: string; name: string; desc: string; multi?: boolean; audioOk?: boolean }
const TOOLS: Tool[] = [
  { id: 'compress', name: '压缩视频', desc: '重新编码缩小体积，可限制最高分辨率。' },
  { id: 'trim', name: '裁剪片段', desc: '只保留一个时间段。' },
  { id: 'concat', name: '拼接', desc: '把多个文件首尾相接。', multi: true, audioOk: true },
  { id: 'convert', name: '转换格式', desc: '转封装（不重新编码）、转 MP4 / MKV，或提取为音频。', audioOk: true },
  { id: 'gif', name: '转 GIF', desc: '截取一段做成动图。' },
  { id: 'frame', name: '截图', desc: '截取某一帧，或每隔几秒截一张。' },
  { id: 'speed', name: '倍速', desc: '加快或放慢，声音音调保持不变。', audioOk: true },
  { id: 'loudness', name: '响度标准化', desc: '把音量调到统一水平，画面不重新编码。', audioOk: true },
  { id: 'landscape', name: '竖屏转横屏', desc: '竖屏视频放进 16:9 画面，两侧用模糊背景或黑边补齐。' },
  { id: 'rotate', name: '旋转 / 翻转', desc: '顺时针、逆时针、180°、水平或垂直翻转。' },
  { id: 'mute', name: '去除音轨', desc: '得到没有声音的视频，不重新编码。' },
]

const tool = ref(props.tool && TOOLS.some((t) => t.id === props.tool) ? props.tool : 'compress')
const files = ref<string[]>(props.preset ? [...props.preset] : [])
const info = ref<MediaInfoLite | null>(null)
const outDir = ref('')
const busy = ref(false)
const cur = computed(() => TOOLS.find((t) => t.id === tool.value)!)

watch(
  () => props.preset,
  (p) => p && (files.value = [...p]),
)
watch(
  () => props.tool,
  (t) => t && TOOLS.some((x) => x.id === t) && (tool.value = t),
)

watch(
  files,
  async (list) => {
    info.value = null
    if (list.length === 1) info.value = await api.mediaInfo(list[0]).catch(() => null)
  },
  { immediate: true },
)

const p = reactive({
  quality: 'balanced' as 'small' | 'balanced' | 'high',
  maxHeight: 0,
  gifStart: '0:00',
  gifLen: '5',
  gifWidth: 480,
  gifFps: 12,
  landW: 1920,
  landH: 1080,
  landMode: 'blur' as 'blur' | 'black',
  speed: 1.5,
  lufs: -16,
  rotate: 'cw' as 'cw' | 'ccw' | 'flip180' | 'hflip' | 'vflip',
  format: 'mp4',
  reencode: false,
  frameAt: '0:01',
  frameFmt: 'jpg' as 'jpg' | 'png',
  frameEvery: false,
  everySecs: 10,
  trimStart: '0:00',
  trimEnd: '',
  precise: false,
  concatRe: false,
})

const FORMATS = [
  { v: 'mp4', t: 'MP4（视频）' },
  { v: 'mkv', t: 'MKV（视频）' },
  { v: 'mov', t: 'MOV（视频）' },
  { v: 'webm', t: 'WebM（视频，需重新编码）' },
  { v: 'mp3', t: 'MP3（仅音频）' },
  { v: 'm4a', t: 'M4A（仅音频）' },
  { v: 'flac', t: 'FLAC（无损音频）' },
  { v: 'opus', t: 'Opus（仅音频）' },
  { v: 'wav', t: 'WAV（无损音频）' },
]
const isAudioFormat = computed(() => ['mp3', 'm4a', 'flac', 'opus', 'wav'].includes(p.format))

const kinds = computed(() => (cur.value.audioOk ? ['video', 'audio'] : ['video']))
const exts = computed(() => (cur.value.audioOk ? ['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'ts', 'm4v', 'mp3', 'm4a', 'flac', 'wav', 'ogg', 'opus', 'aac'] : ['mp4', 'mkv', 'webm', 'mov', 'avi', 'flv', 'ts', 'm4v']))

function time(label: string, text: string, required = true): number | null | undefined {
  if (!text.trim()) return required ? undefined : null
  const v = parseClock(text)
  if (v === null) {
    ElMessage.warning(`${label}的写法不对，请用 83、1:23 或 1:02:03.5 这样的格式。`)
    return undefined
  }
  return v
}

function build(): ToolOp | null {
  switch (tool.value) {
    case 'compress':
      return { op: 'compress', quality: p.quality, maxHeight: p.maxHeight || null }
    case 'gif': {
      const start = time('开始时间', p.gifStart)
      const len = parseClock(p.gifLen)
      if (start == null || len == null || len <= 0) return ElMessage.warning('请填写开始时间和时长。'), null
      return { op: 'gif', startMs: start, durationMs: len, width: p.gifWidth, fps: p.gifFps }
    }
    case 'landscape':
      return { op: 'landscape', width: p.landW, height: p.landH, mode: p.landMode }
    case 'speed':
      return { op: 'speed', factor: p.speed }
    case 'loudness':
      return { op: 'loudness', target: p.lufs }
    case 'rotate':
      return { op: 'rotate', mode: p.rotate }
    case 'convert':
      return { op: 'convert', format: p.format, reencode: p.reencode }
    case 'frame': {
      if (p.frameEvery) return { op: 'frames', everySecs: p.everySecs, format: p.frameFmt }
      const at = time('截图时间', p.frameAt)
      if (at == null) return null
      return { op: 'frame', atMs: at, format: p.frameFmt }
    }
    case 'concat':
      return { op: 'concat', reencode: p.concatRe }
    case 'trim': {
      const s = time('开始时间', p.trimStart)
      const e = time('结束时间', p.trimEnd, false)
      if (s == null || e === undefined) return null
      return { op: 'trim', startMs: s, endMs: e, precise: p.precise }
    }
    case 'mute':
      return { op: 'mute' }
  }
  return null
}

async function pickDir() {
  const d = await open({ directory: true, multiple: false })
  if (typeof d === 'string') outDir.value = d
}

async function start() {
  if (!files.value.length) return ElMessage.warning('请先选择文件。')
  const op = build()
  if (!op) return
  busy.value = true
  try {
    await api.mediaJobStart({ ...op, inputs: files.value, outputDir: outDir.value || null } as ToolJob)
    ElMessage.success('已开始处理，可以在下方查看进度。')
    emit('started')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="vt">
    <nav class="tools" aria-label="工具">
      <button v-for="t in TOOLS" :key="t.id" type="button" class="tool" :class="{ on: tool === t.id }" @click="tool = t.id">{{ t.name }}</button>
    </nav>
    <div class="panel card">
      <h3>{{ cur.name }}</h3>
      <p class="mute desc">{{ cur.desc }}</p>
      <FileInput v-model="files" :multiple="cur.multi" :kinds="kinds" :extensions="exts" :label="cur.multi ? '要拼接的文件（按顺序）' : '文件'" />
      <div v-if="info" class="mute meta mono">
        <template v-if="info.width">{{ info.width }}×{{ info.height }} · </template>
        <template v-if="info.durationMs">{{ formatDuration(info.durationMs) }}</template>
        <template v-if="!info.hasAudio"> · 没有声音</template>
      </div>

      <div class="form">
        <template v-if="tool === 'compress'">
          <label>
            <span>质量</span>
            <el-radio-group v-model="p.quality" size="small">
              <el-radio-button value="small">体积最小</el-radio-button>
              <el-radio-button value="balanced">均衡</el-radio-button>
              <el-radio-button value="high">画质优先</el-radio-button>
            </el-radio-group>
          </label>
          <label>
            <span>最高分辨率</span>
            <el-select v-model="p.maxHeight" size="small" class="w">
              <el-option :value="0" label="保持原样" />
              <el-option :value="1080" label="1080P" />
              <el-option :value="720" label="720P" />
              <el-option :value="480" label="480P" />
              <el-option :value="360" label="360P" />
            </el-select>
          </label>
        </template>

        <template v-else-if="tool === 'trim'">
          <label><span>开始</span><el-input v-model="p.trimStart" size="small" class="w mono" placeholder="0:00" /></label>
          <label><span>结束</span><el-input v-model="p.trimEnd" size="small" class="w mono" placeholder="留空表示到结尾" /></label>
          <label>
            <span>精确到帧</span>
            <el-switch v-model="p.precise" />
            <small class="mute">关闭时不重新编码，速度很快，但开始点会落在最近的关键帧上；打开则重新编码，精确但较慢。</small>
          </label>
        </template>

        <template v-else-if="tool === 'concat'">
          <label>
            <span>重新编码</span>
            <el-switch v-model="p.concatRe" />
            <small class="mute">默认先尝试直接拼接（最快，要求编码参数一致）；画面尺寸不同或直接拼接失败时会自动重新编码。</small>
          </label>
        </template>

        <template v-else-if="tool === 'convert'">
          <label>
            <span>目标格式</span>
            <el-select v-model="p.format" size="small" class="w">
              <el-option v-for="f in FORMATS" :key="f.v" :value="f.v" :label="f.t" />
            </el-select>
          </label>
          <label v-if="!isAudioFormat && p.format !== 'webm'">
            <span>重新编码</span>
            <el-switch v-model="p.reencode" />
            <small class="mute">关闭时只换封装，不损失画质、瞬间完成；播放器不认编码时再打开。</small>
          </label>
        </template>

        <template v-else-if="tool === 'gif'">
          <label><span>开始</span><el-input v-model="p.gifStart" size="small" class="w mono" /></label>
          <label><span>时长（秒）</span><el-input v-model="p.gifLen" size="small" class="w mono" /></label>
          <label><span>宽度</span><el-input-number v-model="p.gifWidth" size="small" :min="120" :max="1280" :step="40" /></label>
          <label><span>帧率</span><el-input-number v-model="p.gifFps" size="small" :min="5" :max="30" /></label>
        </template>

        <template v-else-if="tool === 'frame'">
          <label><span>方式</span><el-radio-group v-model="p.frameEvery" size="small"><el-radio-button :value="false">截一张</el-radio-button><el-radio-button :value="true">每隔几秒截一张</el-radio-button></el-radio-group></label>
          <label v-if="!p.frameEvery"><span>时间点</span><el-input v-model="p.frameAt" size="small" class="w mono" /></label>
          <label v-else><span>间隔（秒）</span><el-input-number v-model="p.everySecs" size="small" :min="0.2" :max="3600" :step="1" /></label>
          <label><span>格式</span><el-radio-group v-model="p.frameFmt" size="small"><el-radio-button value="jpg">JPG</el-radio-button><el-radio-button value="png">PNG（无损）</el-radio-button></el-radio-group></label>
        </template>

        <template v-else-if="tool === 'speed'">
          <label>
            <span>倍速</span>
            <el-slider v-model="p.speed" :min="0.25" :max="4" :step="0.25" show-input size="small" class="sl" />
          </label>
        </template>

        <template v-else-if="tool === 'loudness'">
          <label>
            <span>目标响度（LUFS）</span>
            <el-input-number v-model="p.lufs" size="small" :min="-30" :max="-5" />
            <small class="mute">-16 适合手机和网页，-14 是常见的流媒体标准，-23 是广播标准。</small>
          </label>
        </template>

        <template v-else-if="tool === 'landscape'">
          <label><span>画布</span><el-select v-model="p.landH" size="small" class="w" @change="p.landW = p.landH * 16 / 9"><el-option :value="1080" label="1920×1080" /><el-option :value="720" label="1280×720" /><el-option :value="2160" label="3840×2160" /></el-select></label>
          <label><span>背景</span><el-radio-group v-model="p.landMode" size="small"><el-radio-button value="blur">模糊背景</el-radio-button><el-radio-button value="black">黑边</el-radio-button></el-radio-group></label>
        </template>

        <template v-else-if="tool === 'rotate'">
          <label>
            <span>方式</span>
            <el-radio-group v-model="p.rotate" size="small">
              <el-radio-button value="cw">顺时针 90°</el-radio-button>
              <el-radio-button value="ccw">逆时针 90°</el-radio-button>
              <el-radio-button value="flip180">180°</el-radio-button>
              <el-radio-button value="hflip">水平翻转</el-radio-button>
              <el-radio-button value="vflip">垂直翻转</el-radio-button>
            </el-radio-group>
          </label>
        </template>

        <label class="dir">
          <span>保存到</span>
          <span class="dirbox"><el-input v-model="outDir" size="small" clearable placeholder="留空放在原文件旁边" class="w" /><el-button size="small" @click="pickDir">选择…</el-button></span>
        </label>
      </div>
      <div class="go"><el-button type="primary" :loading="busy" :disabled="!files.length" @click="start">开始处理</el-button></div>
    </div>
  </div>
</template>

<style scoped>
.vt {
  display: grid;
  grid-template-columns: 150px 1fr;
  gap: 14px;
  align-items: start;
}
.tools {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.tool {
  all: unset;
  cursor: pointer;
  padding: 6px 12px;
  border-radius: 7px;
  font-size: 13px;
}
.tool:hover {
  background: var(--cc-acc-soft);
}
.tool.on {
  background: var(--cc-acc-soft);
  color: var(--cc-acc);
  font-weight: 600;
}
.tool:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.panel {
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
.desc {
  margin: 0;
  font-size: 12.5px;
}
.meta {
  font-size: 11.5px;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 4px;
}
.form label {
  display: grid;
  grid-template-columns: 110px auto;
  gap: 10px;
  align-items: center;
  justify-content: start;
}
.form label > span:first-child {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.form label small {
  grid-column: 2;
  font-size: 11.5px;
  max-width: 520px;
}
.w {
  width: 220px;
}
.sl {
  width: 340px;
}
.dirbox {
  display: flex;
  gap: 8px;
}
.go {
  margin-top: 4px;
}
</style>
