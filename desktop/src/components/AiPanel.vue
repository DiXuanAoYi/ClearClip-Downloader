<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { AiSettings, AiStatus } from '../types'

const ai = defineModel<AiSettings>({ required: true })
const status = ref<AiStatus | null>(null)
const chatKey = ref('')
const sttKey = ref('')
const testing = ref(false)

const CHAT_PRESETS = [
  { name: 'OpenAI', url: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
  { name: 'DeepSeek', url: 'https://api.deepseek.com/v1', model: 'deepseek-chat' },
  { name: '月之暗面', url: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
  { name: '通义千问', url: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model: 'qwen-turbo' },
  { name: '智谱', url: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash' },
  { name: '本机 Ollama', url: 'http://localhost:11434/v1', model: 'qwen2.5:7b' },
]
const STT_PRESETS = [
  { name: 'OpenAI', url: '', model: 'whisper-1' },
  { name: 'Groq', url: 'https://api.groq.com/openai/v1', model: 'whisper-large-v3-turbo' },
]
const LANGS = ['简体中文', '繁體中文', 'English', '日本語', '한국어', 'Français', 'Deutsch', 'Español', 'Русский']

async function refresh() {
  status.value = await api.aiStatus().catch(() => null)
}
onMounted(refresh)

async function saveKey(name: 'ai.api_key' | 'stt.api_key', value: string) {
  try {
    await api.secretSet(name, value)
    ElMessage.success(value ? '密钥已加密保存' : '密钥已清除')
    if (name === 'ai.api_key') chatKey.value = ''
    else sttKey.value = ''
    await refresh()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function test() {
  testing.value = true
  try {
    const r = await api.aiTest()
    ElMessage.success(`接口可用，模型回复：${r}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    testing.value = false
  }
}

async function pickFile(field: 'whisperBin' | 'whisperModel') {
  const f = await open({ multiple: false, title: field === 'whisperBin' ? '选择 whisper-cli 可执行文件' : '选择 ggml 模型文件', filters: field === 'whisperModel' ? [{ name: 'ggml 模型', extensions: ['bin'] }] : undefined })
  if (typeof f === 'string') {
    ai.value[field] = f
    refresh()
  }
}
</script>

<template>
  <div class="groups">
    <section class="group card">
      <h3>对话模型（翻译字幕、摘要与章节）</h3>
      <small class="mute">使用兼容 OpenAI 的接口。字幕文字会发送到你填写的服务商；用本机 Ollama 则完全不出本机。密钥加密保存在本机，不会写进设置文件，也不会进入备份。</small>
      <div class="presets">
        <el-button v-for="p in CHAT_PRESETS" :key="p.name" size="small" link type="primary" @click="ai.baseUrl = p.url; ai.model = p.model">{{ p.name }}</el-button>
      </div>
      <div class="field"><label>接口地址</label><el-input v-model="ai.baseUrl" size="small" class="mono" placeholder="https://api.openai.com/v1" /></div>
      <div class="field"><label>模型</label><el-input v-model="ai.model" size="small" class="mono" placeholder="gpt-4o-mini" /></div>
      <div class="field">
        <label>API 密钥 <span v-if="status?.chatKey" class="ok">已保存</span></label>
        <div class="keyrow">
          <el-input v-model="chatKey" size="small" type="password" show-password :placeholder="status?.chatKey ? '输入新密钥可替换' : 'sk-…（本机 Ollama 可留空）'" />
          <el-button size="small" :disabled="!chatKey.trim()" @click="saveKey('ai.api_key', chatKey)">保存</el-button>
          <el-button v-if="status?.chatKey" size="small" @click="saveKey('ai.api_key', '')">清除</el-button>
        </div>
      </div>
      <div><el-button size="small" :loading="testing" @click="test">测试接口</el-button></div>
    </section>

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
          <el-radio-button value="local">本机 whisper.cpp</el-radio-button>
        </el-radio-group>
      </div>
      <template v-if="ai.sttEngine === 'api'">
        <small class="mute">使用兼容 OpenAI 的 <span class="mono">/audio/transcriptions</span> 接口。音频会先在本机转成小体积的单声道再分段上传。</small>
        <div class="presets">
          <el-button v-for="p in STT_PRESETS" :key="p.name" size="small" link type="primary" @click="ai.sttBaseUrl = p.url; ai.sttModel = p.model">{{ p.name }}</el-button>
        </div>
        <div class="field"><label>接口地址（留空与上面相同）</label><el-input v-model="ai.sttBaseUrl" size="small" class="mono" placeholder="https://api.groq.com/openai/v1" /></div>
        <div class="field"><label>模型</label><el-input v-model="ai.sttModel" size="small" class="mono" placeholder="whisper-1" /></div>
        <div class="field">
          <label>API 密钥（留空沿用上面的密钥） <span v-if="status?.sttKey" class="ok">{{ status?.chatKey && !sttKey ? '沿用对话密钥' : '已保存' }}</span></label>
          <div class="keyrow">
            <el-input v-model="sttKey" size="small" type="password" show-password placeholder="单独的语音接口密钥" />
            <el-button size="small" :disabled="!sttKey.trim()" @click="saveKey('stt.api_key', sttKey)">保存</el-button>
            <el-button size="small" @click="saveKey('stt.api_key', '')">清除</el-button>
          </div>
        </div>
      </template>
      <template v-else>
        <small class="mute">
          需要自己安装 <a href="#" @click.prevent="api.openUrl('https://github.com/ggml-org/whisper.cpp')">whisper.cpp</a>（macOS：<span class="mono">brew install whisper-cpp</span>；Windows 可下载它发布的程序包），并下载一个 ggml 模型（如 <span class="mono">ggml-base.bin</span>，中文推荐 small 或更大）。全程不联网。
        </small>
        <div class="field">
          <label>whisper-cli 位置 <span :class="status?.whisperFound ? 'ok' : 'bad'">{{ status?.whisperFound ? '已找到' : '未找到' }}</span></label>
          <div class="keyrow"><el-input v-model="ai.whisperBin" size="small" class="mono" placeholder="留空则在系统 PATH 里找 whisper-cli" @change="refresh" /><el-button size="small" @click="pickFile('whisperBin')">选择…</el-button></div>
        </div>
        <div class="field">
          <label>模型文件</label>
          <div class="keyrow"><el-input v-model="ai.whisperModel" size="small" class="mono" placeholder="ggml-base.bin 的路径" @change="refresh" /><el-button size="small" @click="pickFile('whisperModel')">选择…</el-button></div>
        </div>
      </template>
      <div class="field"><label>音频语言</label><el-input v-model="ai.sttLanguage" size="small" class="short mono" placeholder="留空自动识别，或 zh / en / ja" /></div>
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
