<script setup lang="ts">
import { TARGET_LANGS } from '../utils/langs'
import SttLocal from './SttLocal.vue'
import { onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { AiSettings, AiStatus } from '../types'
import AiPool from './AiPool.vue'
import GlossaryPanel from './GlossaryPanel.vue'
import ExpertsPanel from './ExpertsPanel.vue'

const ai = defineModel<AiSettings>({ required: true })
const status = ref<AiStatus | null>(null)
const sttKey = ref('')

const STT_PRESETS = [
  { name: 'OpenAI', url: '', model: 'whisper-1' },
  { name: 'Groq', url: 'https://api.groq.com/openai/v1', model: 'whisper-large-v3-turbo' },
  { name: '硅基流动', url: 'https://api.siliconflow.cn/v1', model: 'FunAudioLLM/SenseVoiceSmall' },
]
const LANGS = TARGET_LANGS

async function refresh() {
  status.value = await api.aiStatus().catch(() => null)
}
onMounted(refresh)

async function saveKey(name: 'stt.api_key', value: string) {
  try {
    await api.secretSet(name, value)
    ElMessage.success(value ? '密钥已加密保存' : '密钥已清除')
    sttKey.value = ''
    await refresh()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function pickFile(field: 'whisperBin' | 'whisperModel' | 'whisperVadModel') {
  const f = await open({ multiple: false, title: field === 'whisperBin' ? '选择 whisper-cli 可执行文件' : field === 'whisperVadModel' ? '选择 VAD 模型文件' : '选择 ggml 模型文件', filters: field !== 'whisperBin' ? [{ name: 'ggml 模型', extensions: ['bin'] }] : undefined })
  if (typeof f === 'string') {
    ai.value[field] = f
    refresh()
  }
}
</script>

<template>
  <div class="groups">
    <AiPool v-model="ai" />
    <GlossaryPanel v-model="ai" />
    <ExpertsPanel v-model="ai" />
    <section class="group card">
      <h3>翻译</h3>
      <div class="kv">
        <span>翻译成</span>
        <el-select v-model="ai.targetLang" size="small" filterable allow-create default-first-option class="sel"><el-option v-for="l in LANGS" :key="l" :value="l" :label="l" /></el-select>
      </div>
      <div class="kv"><span>保留原文<small class="mute block">原文在上、译文在下，做成双语字幕</small></span><el-switch v-model="ai.bilingual" /></div>
      <div class="kv">
        <span>每次翻译的条数<small class="mute block">越大越省请求，但模型更容易漏条；漏条时会自动拆小重试</small></span>
        <el-input-number v-model="ai.batchSize" size="small" :min="5" :max="100" :step="5" controls-position="right" />
      </div>
      <div class="kv">
        <span>下载到外语字幕时自动翻译<small class="mute block">作品里已经有目标语言的字幕就不再翻译；弹幕不翻译</small></span>
        <el-switch v-model="ai.autoTranslate" />
      </div>
    </section>

    <section class="group card">
      <h3>语音转文字（给没有字幕的视频生成字幕）</h3>
      <div class="kv">
        <span>方式</span>
        <el-radio-group v-model="ai.sttEngine" size="small">
          <el-radio-button value="api">在线接口</el-radio-button>
          <el-radio-button value="local">本机识别（离线）</el-radio-button>
        </el-radio-group>
      </div>
      <template v-if="ai.sttEngine === 'api'">
        <small class="mute">使用兼容 OpenAI 的 <span class="mono">/audio/transcriptions</span> 接口。音频会先在本机转成小体积的单声道再分段上传。</small>
        <div class="presets">
          <el-button v-for="p in STT_PRESETS" :key="p.name" size="small" link type="primary" @click="ai.sttBaseUrl = p.url; ai.sttModel = p.model">{{ p.name }}</el-button>
        </div>
        <div class="field"><label>接口地址</label><el-input v-model="ai.sttBaseUrl" size="small" class="mono" placeholder="https://api.groq.com/openai/v1" /></div>
        <div class="field"><label>模型</label><el-input v-model="ai.sttModel" size="small" class="mono" placeholder="whisper-1" /></div>
        <div class="field">
          <label>API 密钥 <span v-if="status?.sttKey" class="ok">已保存</span></label>
          <div class="keyrow">
            <el-input v-model="sttKey" size="small" type="password" show-password placeholder="单独的语音接口密钥" />
            <el-button size="small" :disabled="!sttKey.trim()" @click="saveKey('stt.api_key', sttKey)">保存</el-button>
            <el-button size="small" @click="saveKey('stt.api_key', '')">清除</el-button>
          </div>
        </div>
      </template>
      <template v-else>
        <small class="mute">全程不联网。模型和“设置 → 组件 → 语音识别”是同一个选择，在哪边改都一样。中文建议用 SenseVoice（快、准）；Whisper 适合多语种和要逐词时间的场景。</small>
        <SttLocal v-model="ai.sttLocalModel" />
        <el-collapse class="adv">
          <el-collapse-item title="高级：手动指定程序 / 模型文件（Whisper）" name="adv">
        <div class="field">
          <label>whisper-cli 位置 <span :class="status?.whisperFound ? 'ok' : 'bad'">{{ status?.whisperFound ? '已找到' : '未找到' }}</span></label>
          <small v-if="status?.whisperPath && status.whisperPath !== ai.whisperBin" class="mute">自动检测到：<span class="mono">{{ status.whisperPath }}</span></small>
          <div class="keyrow"><el-input v-model="ai.whisperBin" size="small" class="mono" placeholder="留空则自动查找（清影安装的、系统 PATH 里的 whisper-cli）" @change="refresh" /><el-button size="small" @click="pickFile('whisperBin')">选择…</el-button></div>
        </div>
        <div class="field">
          <label>模型文件 <span :class="status?.whisperModelPath ? 'ok' : 'bad'">{{ status?.whisperModelPath ? '已找到' : '未找到' }}</span></label>
          <small v-if="status?.whisperModelPath && status.whisperModelPath !== ai.whisperModel" class="mute">自动检测到：<span class="mono">{{ status.whisperModelPath }}</span></small>
          <div class="keyrow"><el-input v-model="ai.whisperModel" size="small" class="mono" placeholder="留空则自动使用“设置 → 组件”里下载的模型" @change="refresh" /><el-button size="small" @click="pickFile('whisperModel')">选择…</el-button></div>
        </div>
          </el-collapse-item>
        </el-collapse>
      </template>
      <div class="field"><label>音频语言</label><el-input v-model="ai.sttLanguage" size="small" class="short mono" placeholder="留空自动识别，或 zh / en / ja" /></div>
      <div v-if="ai.sttEngine === 'local'" class="field">
        <label>VAD 模型（可选，ggml-silero-*.bin）</label>
        <div class="keyrow"><el-input v-model="ai.whisperVadModel" size="small" class="mono" placeholder="填了且 whisper-cli 支持时，只识别有人声的部分，减少静音处的胡乱识别" /><el-button size="small" @click="pickFile('whisperVadModel')">选择…</el-button></div>
      </div>
      <div class="field">
        <label>AI 纠错力度（补标点、改同音错字）</label>
        <el-radio-group v-model="ai.correctMode" size="small">
          <el-radio-button value="conservative">保守</el-radio-button>
          <el-radio-button value="aggressive">积极</el-radio-button>
          <el-radio-button value="rules">只用规则</el-radio-button>
        </el-radio-group>
      </div>
      <div class="kv">
        <span>自动转写的字幕几乎没有标点时，自动 AI 纠错<small class="mute block">会多用一次对话模型；改动过大的行会被拒绝，原文保留</small></span>
        <el-switch v-model="ai.correctAuto" />
      </div>
      <div class="kv">
        <span>下载的视频没有字幕时自动转写<small class="mute block">在线接口会按音频时长计费，请确认额度；导入的文件不会自动转写</small></span>
        <el-switch v-model="ai.autoTranscribe" />
      </div>
    </section>
  </div>
</template>

<style scoped>
.groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
  gap: 12px;
  align-items: start;
}
.group {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
h3 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-mute);
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.field label {
  font-size: 12px;
  color: var(--cc-mute);
}
.kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.block {
  display: block;
}
.presets {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
}
.presets :deep(.el-button) {
  margin-left: 0;
}
.keyrow {
  display: flex;
  gap: 8px;
}
.ok {
  color: var(--cc-ok);
  margin-left: 6px;
}
.bad {
  color: var(--cc-err);
  margin-left: 6px;
}
.sel {
  width: 160px;
}
.short {
  width: 220px;
}
</style>
