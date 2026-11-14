<script setup lang="ts">
import CcButton from './ui/CcButton.vue'
import CcField from './ui/CcField.vue'
import CcSelect from './ui/CcSelect.vue'
import CcSegmented from './ui/CcSegmented.vue'
import CcLink from './ui/CcLink.vue'
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
const volcKey = ref('')
const xfKey = ref('')
const xfRtKey = ref('')
const has = ref({ volc: false, xfyun: false, xfyunRt: false })
const testing = ref('')
const testMsg = ref<Record<string, { ok: boolean; text: string }>>({})
async function testCloud(engine: 'volc' | 'xfyun' | 'xfyun_rt') {
  testing.value = engine
  try {
    testMsg.value[engine] = { ok: true, text: await api.sttCloudTest(engine) }
  } catch (e) {
    testMsg.value[engine] = { ok: false, text: errorText(e) }
  } finally {
    testing.value = ''
  }
}

const STT_PRESETS = [
  { name: 'OpenAI', url: '', model: 'whisper-1' },
  { name: 'Groq', url: 'https://api.groq.com/openai/v1', model: 'whisper-large-v3-turbo' },
  { name: '硅基流动', url: 'https://api.siliconflow.cn/v1', model: 'FunAudioLLM/SenseVoiceSmall' },
]
const LANGS = TARGET_LANGS

async function refresh() {
  status.value = await api.aiStatus().catch(() => null) // 忽略：可选数据，取不到就按空处理
}
async function refreshHas() {
  has.value = {
    volc: await api.secretHas('stt.volc_key').catch(() => false),
    xfyun: await api.secretHas('stt.xfyun_secret').catch(() => false),
    xfyunRt: await api.secretHas('stt.xfyun_rt_key').catch(() => false),
  }
}
onMounted(() => {
  void refresh()
  void refreshHas()
})

async function saveKey(name: 'stt.api_key' | 'stt.volc_key' | 'stt.xfyun_secret' | 'stt.xfyun_rt_key', value: string) {
  try {
    await api.secretSet(name, value.trim())
    ElMessage.success(value ? '密钥已加密保存' : '密钥已清除')
    sttKey.value = ''
    volcKey.value = ''
    xfKey.value = ''
    xfRtKey.value = ''
    await Promise.all([refresh(), refreshHas()])
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
const STT_PH = 'https://api.groq.com/openai/v1'
const FILLER_OPTS = [{ value: 'off', label: '不去' }, { value: 'conservative', label: '保守（推荐）' }, { value: 'standard', label: '标准' }, { value: 'aggressive', label: '激进' }]
const STT_ENGINE_OPTS = [{ value: 'api', label: '在线接口' }, { value: 'local', label: '本机识别（离线）' }, { value: 'volc', label: '火山引擎（豆包）' }, { value: 'xfyun', label: '讯飞' }]
const CUE_OPTS = [{ value: 'short', label: '短（视频字幕层）' }, { value: 'standard', label: '标准（推荐）' }, { value: 'long', label: '长（只看文稿）' }]
const PUNCT_OPTS = [{ value: 'conservative', label: '保守' }, { value: 'aggressive', label: '积极' }, { value: 'rules', label: '只用规则' }]
</script>

<template>
  <div class="groups">
    <AiPool v-model="ai" />
    <GlossaryPanel v-model="ai" />
    <ExpertsPanel v-model="ai" />
    <section class="group card">
      <h3>翻译</h3>
      <CcField label="翻译成"><el-select v-model="ai.targetLang" size="small" filterable allow-create default-first-option class="sel"><el-option v-for="l in LANGS" :key="l" :value="l" :label="l" /></el-select></CcField>
      <CcField label="保留原文" desc="原文在上、译文在下，做成双语字幕">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.bilingual" /></template>
      </CcField>
      <CcField label="每次翻译的条数" desc="越大越省请求，但模型更容易漏条；漏条时会自动拆小重试">
        <template #default="{ id }"><el-input-number :id="id" v-model="ai.batchSize" size="small" :min="5" :max="100" :step="5" controls-position="right" /></template>
      </CcField>
      <CcField label="送给 AI 前去掉非语音说明" desc="[Music]、♪、(laughs)、&gt;&gt; 换人标记、说话人前缀。只影响送给模型的文字（摘要、导图、翻译等），文稿的显示和导出不变">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.subCleanHearing" /></template>
      </CcField>
      <CcField label="送给 AI 前去掉口水词" desc="保守：嗯、呃、um、uh、结巴和叠词；标准：再加成串的口头禅（you know、那个那个）；激进：再加被逗号隔开的单个口头禅（like、就是）。只影响送给模型的文字，文稿的显示和导出不变">
        <template #default="{ id }"><CcSelect :id="id" v-model="ai.subCleanFiller" :options="FILLER_OPTS" width="m" data-testid="sub-clean-filler" /></template>
      </CcField>
      <CcField label="预处理后自动补标点" desc="自动识别出来的字幕通常没有标点：读到文稿、做完上面的清理后，如果大部分句子没有标点，就自动用 AI 补标点并改同音错字（多一次模型调用，结果缓存）。可以在文稿栏点“还原顺句前”撤销">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.subCleanPunct" /></template>
      </CcField>
      <CcField label="语音转文字的结果也做预处理" desc="本机 / 在线 / 火山引擎 / 讯飞识别出来的字幕，同样去掉非语音说明和口水词再保存；清理前的原文另存为 .raw.srt">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.subCleanOutput" /></template>
      </CcField>
      <CcField label="下载到外语字幕时自动翻译" desc="作品里已经有目标语言的字幕就不再翻译；弹幕不翻译">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.autoTranslate" /></template>
      </CcField>
    </section>

    <section class="group card">
      <h3>语音转文字（给没有字幕的视频生成字幕）</h3>
      <CcField label="方式"><CcSegmented v-model="ai.sttEngine" :options="STT_ENGINE_OPTS" size="s" /></CcField>
      <template v-if="ai.sttEngine === 'volc'">
        <small class="mute">豆包语音“大模型录音文件极速版识别”：整段上传、接近说话速度出结果，中文很准。音频会先在本机转成 16 kHz 单声道 WAV，每 10 分钟一段上传到火山引擎。需要在火山引擎控制台开通该资源。<b>接口细节还没有用真实密钥验证过</b>，第一次用请先拿一小段音频试。</small>
        <CcField label="AppId（用新版控制台的 API Key 时留空）" layout="col">
          <template #default="{ id }"><el-input :id="id" v-model="ai.sttVolcAppId" size="small" class="mono" placeholder="旧版控制台的 AppId，没有就留空" /></template>
        </CcField>
        <div class="field">
          <label>密钥（Access Token / API Key） <span v-if="has.volc" class="ok">已保存</span> · <CcLink href="https://console.volcengine.com/speech/app" external>打开火山引擎控制台</CcLink></label>
          <div class="keyrow">
            <el-input v-model="volcKey" size="small" type="password" show-password placeholder="Access Token 或 API Key" />
            <CcButton kind="secondary" size="s" :disabled="!volcKey.trim()" @click="saveKey('stt.volc_key', volcKey)">保存</CcButton>
            <CcButton kind="secondary" size="s" @click="saveKey('stt.volc_key', '')">清除</CcButton>
          </div>
        </div>
        <CcField label="用火山自带的“语义顺滑”去口水词" desc="默认关：口水词统一交给“字幕预处理”，几种引擎行为一致。打开后由火山在识别时就去掉">
          <template #default="{ id }"><el-switch :id="id" v-model="ai.sttCloudSmooth" /></template>
        </CcField>
        <div class="keyrow">
          <CcButton kind="secondary" size="s" :loading="testing === 'volc'" :disabled="!has.volc" @click="testCloud('volc')">测试连接</CcButton>
          <span v-if="testMsg.volc" :class="testMsg.volc.ok ? 'ok' : 'bad'" class="mute">{{ testMsg.volc.text }}</span>
        </div>
      </template>
      <template v-else-if="ai.sttEngine === 'xfyun'">
        <small class="mute">讯飞开放平台“录音文件转写”：整个文件上传到讯飞，转写完成后取回带时间的句子。需要在讯飞控制台开通该服务并拿到 AppID 和 SecretKey。<b>接口细节还没有用真实密钥验证过</b>，第一次用请先拿一小段音频试。</small>
        <CcField label="AppID" layout="col">
          <template #default="{ id }"><el-input :id="id" v-model="ai.sttXfyunAppId" size="small" class="mono" placeholder="例如 595f23df" /></template>
        </CcField>
        <div class="field">
          <label>SecretKey <span v-if="has.xfyun" class="ok">已保存</span> · <CcLink href="https://console.xfyun.cn/services/lfasr" external>打开讯飞控制台</CcLink></label>
          <div class="keyrow">
            <el-input v-model="xfKey" size="small" type="password" show-password placeholder="录音文件转写的 SecretKey" />
            <CcButton kind="secondary" size="s" :disabled="!xfKey.trim()" @click="saveKey('stt.xfyun_secret', xfKey)">保存</CcButton>
            <CcButton kind="secondary" size="s" @click="saveKey('stt.xfyun_secret', '')">清除</CcButton>
          </div>
        </div>
        <div class="keyrow">
          <CcButton kind="secondary" size="s" :loading="testing === 'xfyun'" :disabled="!has.xfyun || !ai.sttXfyunAppId" @click="testCloud('xfyun')">测试连接</CcButton>
          <span v-if="testMsg.xfyun" :class="testMsg.xfyun.ok ? 'ok' : 'bad'" class="mute">{{ testMsg.xfyun.text }}</span>
        </div>
        <div class="field">
          <label>实时转写 APIKey（“屏幕翻译 → 语音翻译”用讯飞时需要，和上面的 SecretKey 不是同一个） <span v-if="has.xfyunRt" class="ok">已保存</span> · <CcLink href="https://console.xfyun.cn/services/rta" external>打开讯飞控制台</CcLink></label>
          <div class="keyrow">
            <el-input v-model="xfRtKey" size="small" type="password" show-password placeholder="实时语音转写的 APIKey" />
            <CcButton kind="secondary" size="s" :disabled="!xfRtKey.trim()" @click="saveKey('stt.xfyun_rt_key', xfRtKey)">保存</CcButton>
            <CcButton kind="secondary" size="s" @click="saveKey('stt.xfyun_rt_key', '')">清除</CcButton>
          </div>
        </div>
        <div class="keyrow">
          <CcButton kind="secondary" size="s" :loading="testing === 'xfyun_rt'" :disabled="!has.xfyunRt || !ai.sttXfyunAppId" @click="testCloud('xfyun_rt')">测试实时转写连接</CcButton>
          <span v-if="testMsg.xfyun_rt" :class="testMsg.xfyun_rt.ok ? 'ok' : 'bad'" class="mute">{{ testMsg.xfyun_rt.text }}</span>
        </div>
        <CcField label="用讯飞的“顺滑词”标记去口水词" desc="默认关：口水词统一交给“字幕预处理”。打开后识别结果里讯飞标成顺滑词的字直接不要">
          <template #default="{ id }"><el-switch :id="id" v-model="ai.sttCloudSmooth" /></template>
        </CcField>
      </template>
      <template v-else-if="ai.sttEngine === 'api'">
        <small class="mute">使用兼容 OpenAI 的 <span class="mono">/audio/transcriptions</span> 接口。音频会先在本机转成小体积的单声道再分段上传。</small>
        <div class="presets">
          <CcButton kind="text" v-for="p in STT_PRESETS" :key="p.name" size="s" @click="ai.sttBaseUrl = p.url; ai.sttModel = p.model">{{ p.name }}</CcButton>
        </div>
        <CcField label="接口地址" layout="col">
          <template #default="{ id }"><el-input :id="id" v-model="ai.sttBaseUrl" size="small" class="mono" :placeholder="STT_PH" /></template>
        </CcField>
        <CcField label="模型" layout="col">
          <template #default="{ id }"><el-input :id="id" v-model="ai.sttModel" size="small" class="mono" placeholder="whisper-1" /></template>
        </CcField>
        <div class="field">
          <label>API 密钥 <span v-if="status?.sttKey" class="ok">已保存</span></label>
          <div class="keyrow">
            <el-input v-model="sttKey" size="small" type="password" show-password placeholder="单独的语音接口密钥" />
            <CcButton kind="secondary" size="s" :disabled="!sttKey.trim()" @click="saveKey('stt.api_key', sttKey)">保存</CcButton>
            <CcButton kind="secondary" size="s" @click="saveKey('stt.api_key', '')">清除</CcButton>
          </div>
        </div>
      </template>
      <template v-else>
        <small class="mute">全程不联网。模型和“设置 → 组件 → 语音识别”是同一个选择，在哪边改都一样。中文建议用 SenseVoice（快、准）；Whisper 适合多语种和要逐词时间的场景。</small>
        <SttLocal v-model="ai.sttLocalModel" />
        <div class="field">
          <label>字幕句长（SenseVoice 一段声音一条字幕，太长时按标点和长度拆开）</label>
          <CcSegmented v-model="ai.cueLength" :options="CUE_OPTS" size="s" />
          <small class="mute">短：中文 16 字 / 英文 36 字符以内；标准：22 / 48；长：32 / 72。已经转写好的视频不会自动改，在视频摘要的文稿页点“重新断句”即可。</small>
          <el-switch v-model="ai.cueSemantic" size="small" active-text="AI 语义断句（高质量）：只处理仍然超长的句子，多一次模型调用" />
        </div>
        <el-collapse class="adv">
          <el-collapse-item title="高级：手动指定程序 / 模型文件（Whisper）" name="adv">
        <div class="field">
          <label>whisper-cli 位置 <span :class="status?.whisperFound ? 'ok' : 'bad'">{{ status?.whisperFound ? '已找到' : '未找到' }}</span></label>
          <small v-if="status?.whisperPath && status.whisperPath !== ai.whisperBin" class="mute">自动检测到：<span class="mono">{{ status.whisperPath }}</span></small>
          <div class="keyrow"><el-input v-model="ai.whisperBin" size="small" class="mono" placeholder="留空则自动查找（清影安装的、系统 PATH 里的 whisper-cli）" @change="refresh" /><CcButton kind="secondary" size="s" @click="pickFile('whisperBin')">选择…</CcButton></div>
        </div>
        <div class="field">
          <label>模型文件 <span :class="status?.whisperModelPath ? 'ok' : 'bad'">{{ status?.whisperModelPath ? '已找到' : '未找到' }}</span></label>
          <small v-if="status?.whisperModelPath && status.whisperModelPath !== ai.whisperModel" class="mute">自动检测到：<span class="mono">{{ status.whisperModelPath }}</span></small>
          <div class="keyrow"><el-input v-model="ai.whisperModel" size="small" class="mono" placeholder="留空则自动使用“设置 → 组件”里下载的模型" @change="refresh" /><CcButton kind="secondary" size="s" @click="pickFile('whisperModel')">选择…</CcButton></div>
        </div>
          </el-collapse-item>
        </el-collapse>
      </template>
      <CcField label="音频语言" layout="col">
        <template #default="{ id }"><el-input :id="id" v-model="ai.sttLanguage" size="small" class="short mono" placeholder="留空自动识别，或 zh / en / ja" /></template>
      </CcField>
      <div v-if="ai.sttEngine === 'local'" class="field">
        <label>VAD 模型（可选，ggml-silero-*.bin）</label>
        <div class="keyrow"><el-input v-model="ai.whisperVadModel" size="small" class="mono" placeholder="填了且 whisper-cli 支持时，只识别有人声的部分，减少静音处的胡乱识别" /><CcButton kind="secondary" size="s" @click="pickFile('whisperVadModel')">选择…</CcButton></div>
      </div>
      <CcField label="AI 纠错力度（补标点、改同音错字）" layout="col">
        <CcSegmented v-model="ai.correctMode" :options="PUNCT_OPTS" size="s" />
      </CcField>
      <CcField label="自动转写的字幕几乎没有标点时，自动 AI 纠错" desc="会多用一次对话模型；改动过大的行会被拒绝，原文保留">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.correctAuto" /></template>
      </CcField>
      <CcField label="下载的视频没有字幕时自动转写" desc="在线接口会按音频时长计费，请确认额度；导入的文件不会自动转写">
        <template #default="{ id }"><el-switch :id="id" v-model="ai.autoTranscribe" /></template>
      </CcField>
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
  font-size: var(--cc-fs-lg);
  color: var(--cc-fg);
  font-weight: 700;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.field label {
  font-size: var(--cc-fs-sm);
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
  color: var(--cc-ok-text);
  margin-left: 6px;
}
.bad {
  color: var(--cc-err-text);
  margin-left: 6px;
}
.sel {
  width: 160px;
}
.short {
  width: 220px;
}
</style>
