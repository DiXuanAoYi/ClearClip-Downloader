<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../../api'
import { useAppStore } from '../../stores/app'
import type { AiStatus, Chapter, JobSnap } from '../../types'
import { parseChapterLines } from '../../utils/time'
import FileInput from './FileInput.vue'

const props = defineProps<{ preset?: string[]; mode?: 'transcribe' | 'translate' | 'summarize' | 'correct' }>()
const app = useAppStore()
const mode = ref<'transcribe' | 'translate' | 'summarize' | 'correct'>(props.mode ?? 'transcribe')
const status = ref<AiStatus | null>(null)
const media = ref<string[]>(props.mode === 'transcribe' ? [...(props.preset ?? [])] : [])
const subs = ref<string[]>(props.mode && props.mode !== 'transcribe' ? [...(props.preset ?? [])] : [])
const language = ref('')
// 生成字幕 / 纠错的选项
const correct = ref<'auto' | 'conservative' | 'aggressive' | 'rules' | 'off'>('auto')
const resegment = ref(true)
const semantic = ref(false)
const reflect = ref(false)
const wordTiming = ref(false)
const targets = ref<string[]>([])
const display = ref<'orig' | 'trans' | 'orig-top' | 'trans-top'>('orig-top')
const formats = ref<string[]>([])
const TARGET_LANGS = ['简体中文', '繁體中文', 'English', '日本語', '한국어', 'Français', 'Deutsch', 'Español', 'Русский']
const FORMATS = [
  ['vtt', 'VTT'],
  ['ass', 'ASS'],
  ['lrc', 'LRC 歌词'],
  ['txt', 'TXT 文稿'],
  ['json', 'JSON'],
]
const correctMode = ref<'conservative' | 'aggressive' | 'rules'>('conservative')
const busy = ref(false)
const summaryJob = ref<number | null>(null)
const summaryText = ref('')
const chaptersText = ref('')
const summaryFile = ref('')
const chapterVideo = ref<string[]>([])
let un: UnlistenFn | undefined

async function refresh() {
  status.value = await api.aiStatus().catch(() => null)
}
onMounted(async () => {
  refresh()
  un = await events.onMediaJobs(onJobs)
})
onBeforeUnmount(() => un?.())
watch(mode, refresh)

async function onJobs(list: JobSnap[]) {
  const j = list.find((x) => x.id === summaryJob.value)
  if (!j || j.status !== 'done' || summaryText.value) return
  try {
    summaryText.value = await api.mediaJobText(j.id)
    chaptersText.value = await api.mediaJobText(j.id, 'chapters').catch(() => '')
    summaryFile.value = j.output ?? ''
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function go() {
  busy.value = true
  try {
    if (mode.value === 'transcribe') {
      if (!media.value.length) return ElMessage.warning('请先选择视频或音频文件。')
      await api.subgenStart(media.value[0], {
        language: language.value || undefined,
        correct: correct.value === 'auto' ? undefined : correct.value,
        resegment: resegment.value,
        semantic: semantic.value || undefined,
        reflect: reflect.value || undefined,
        wordTiming: wordTiming.value || undefined,
        targets: targets.value,
        display: display.value,
        formats: formats.value,
      })
      ElMessage.success('已开始生成字幕，进度在下方；完成后字幕会保存在视频旁边并登记到媒体库。')
    } else if (mode.value === 'correct') {
      if (!subs.value.length) return ElMessage.warning('请先选择字幕文件。')
      await api.subgenCorrect(subs.value[0], correctMode.value)
      ElMessage.success('已开始纠错，完成后生成“已修正”的新字幕，原字幕不会被修改，改动明细保存在 .改动.json 里。')
    } else if (mode.value === 'translate') {
      if (!subs.value.length) return ElMessage.warning('请先选择字幕文件。')
      await api.aiTranslate(subs.value[0])
      ElMessage.success('已开始翻译，完成后会生成新的字幕文件，原字幕不会被修改。')
    } else {
      if (!subs.value.length) return ElMessage.warning('请先选择字幕文件。')
      summaryText.value = ''
      chaptersText.value = ''
      summaryJob.value = await api.aiSummarize(subs.value[0])
      ElMessage.success('已开始总结，完成后结果显示在下方。')
    }
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

async function copy(text: string, what: string) {
  try {
    await api.copyText(text)
    ElMessage.success(`已复制${what}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function writeChapters() {
  if (!chapterVideo.value.length) return ElMessage.warning('请先选择要写入章节的视频。')
  try {
    const info = await api.mediaInfo(chapterVideo.value[0])
    const chapters: Chapter[] = parseChapterLines(chaptersText.value, info.durationMs ?? 0)
    if (!chapters.length) return ElMessage.warning('没有可用的章节。')
    await api.mediaJobStart({ op: 'chapters', inputs: chapterVideo.value, chapters })
    ElMessage.success('已开始写入章节，会保存为新文件，进度在下方。')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div class="ai">
    <div class="modes">
      <el-radio-group v-model="mode" size="small">
        <el-radio-button value="transcribe">生成字幕</el-radio-button>
        <el-radio-button value="correct">AI 纠错</el-radio-button>
        <el-radio-button value="translate">翻译字幕</el-radio-button>
        <el-radio-button value="summarize">摘要与章节</el-radio-button>
      </el-radio-group>
    </div>
    <div class="panel card">
      <template v-if="mode === 'transcribe'">
        <h3>给没有字幕的视频生成字幕</h3>
        <p class="mute desc">识别说话内容 → 去掉误识别 → 补标点、改错字 → 断句 → 可选翻译成双语，保存在视频旁边并登记到媒体库、加入字幕搜索。</p>
        <el-alert v-if="status && !status.sttReady" type="warning" :closable="false" show-icon title="还没有配置语音转文字">
          <el-button link type="primary" size="small" @click="app.goSettings('ai')">去“设置 → AI”配置</el-button>
        </el-alert>
        <FileInput v-model="media" :kinds="['video', 'audio']" :extensions="['mp4', 'mkv', 'webm', 'mov', 'flv', 'ts', 'm4v', 'mp3', 'm4a', 'flac', 'wav', 'ogg', 'opus', 'aac']" label="视频或音频" />
        <label class="row"><span>语言</span><el-input v-model="language" size="small" class="short mono" placeholder="留空用设置里的；zh / en / ja" /></label>
        <label class="row">
          <span>AI 纠错</span>
          <el-select v-model="correct" size="small" class="short">
            <el-option value="auto" label="自动（识别结果没有标点时才用 AI）" />
            <el-option value="conservative" label="保守：补标点 + 改明显错字" />
            <el-option value="aggressive" label="积极：再修语法、更书面" />
            <el-option value="rules" label="只用规则（离线，去口头禅、统一标点）" />
            <el-option value="off" label="不处理" />
          </el-select>
        </label>
        <label class="row"><span>断句</span><el-switch v-model="resegment" active-text="过长的字幕按标点自动拆开" /></label>
        <label v-if="resegment" class="row"><span>语义断句</span><el-switch v-model="semantic" active-text="让 AI 在语义停顿处断开（不改一个字，不行就退回规则）" /></label>
        <label v-if="resegment" class="row"><span>逐词时间</span><el-switch v-model="wordTiming" active-text="拆开的字幕对到真实的词间停顿（仅本机 whisper.cpp）" /></label>
        <label v-if="targets.length" class="row"><span>反思翻译</span><el-switch v-model="reflect" active-text="译完再让 AI 对照原文检查润色（更准，请求量约翻倍）" /></label>
        <label class="row">
          <span>翻译成</span>
          <el-select v-model="targets" multiple size="small" class="wide" placeholder="不翻译（可多选）" collapse-tags collapse-tags-tooltip>
            <el-option v-for="l in TARGET_LANGS" :key="l" :value="l" :label="l" />
          </el-select>
        </label>
        <label v-if="targets.length" class="row">
          <span>显示方式</span>
          <el-radio-group v-model="display" size="small">
            <el-radio-button value="orig-top">双语（原文在上）</el-radio-button>
            <el-radio-button value="trans-top">双语（译文在上）</el-radio-button>
            <el-radio-button value="trans">只要译文</el-radio-button>
            <el-radio-button value="orig">只要原文</el-radio-button>
          </el-radio-group>
        </label>
        <label class="row">
          <span>另存格式</span>
          <el-checkbox-group v-model="formats" size="small">
            <el-checkbox v-for="f in FORMATS" :key="f[0]" :value="f[0]">{{ f[1] }}</el-checkbox>
          </el-checkbox-group>
        </label>
        <small class="mute">SRT 总会保存。原字幕轨、译文轨、双语轨各是一个文件（如 视频.zh.srt、视频.zh.双语.srt）。云端识别和翻译会把内容发给你配置的服务。</small>
      </template>
      <template v-else-if="mode === 'correct'">
        <h3>AI 纠错：补标点、改同音错字</h3>
        <p class="mute desc">适合没有标点的自动字幕（语音识别、YouTube 自动字幕、B 站 AI 字幕）。每一行的改动都会被检查：改得太多、改了数字、动了术语库里的词，都会被拒绝并保留原文；时间不会变。结果存成新文件，原字幕不动。</p>
        <el-alert v-if="status && !status.chatReady && correctMode !== 'rules'" type="warning" :closable="false" show-icon title="还没有配置对话模型，只能用“只用规则”">
          <el-button link type="primary" size="small" @click="app.goSettings('ai')">去“设置 → AI”配置</el-button>
        </el-alert>
        <FileInput v-model="subs" :kinds="['subtitle']" :extensions="['srt', 'vtt', 'ass', 'ssa']" label="字幕文件" />
        <label class="row">
          <span>力度</span>
          <el-radio-group v-model="correctMode" size="small">
            <el-radio-button value="conservative">保守</el-radio-button>
            <el-radio-button value="aggressive">积极</el-radio-button>
            <el-radio-button value="rules">只用规则</el-radio-button>
          </el-radio-group>
        </label>
      </template>
      <template v-else-if="mode === 'translate'">
        <h3>翻译字幕</h3>
        <p class="mute desc">用大模型把字幕翻译成“设置 → AI”里选定的语言（现在是 {{ app.settings?.ai.targetLang }}，{{ app.settings?.ai.bilingual ? '双语对照' : '只保留译文' }}）。</p>
        <el-alert v-if="status && !status.chatReady" type="warning" :closable="false" show-icon title="还没有配置对话模型">
          <el-button link type="primary" size="small" @click="app.goSettings('ai')">去“设置 → AI”配置</el-button>
        </el-alert>
        <FileInput v-model="subs" :kinds="['subtitle']" :extensions="['srt', 'vtt', 'ass', 'ssa']" label="字幕文件" />
      </template>
      <template v-else>
        <h3>摘要与章节</h3>
        <p class="mute desc">读取字幕，生成内容摘要、要点和带时间的章节（Markdown 保存在字幕旁边）。可以把章节写进视频文件，播放器的进度条上就会出现章节。</p>
        <el-alert v-if="status && !status.chatReady" type="warning" :closable="false" show-icon title="还没有配置对话模型">
          <el-button link type="primary" size="small" @click="app.goSettings('ai')">去“设置 → AI”配置</el-button>
        </el-alert>
        <FileInput v-model="subs" :kinds="['subtitle']" :extensions="['srt', 'vtt', 'ass', 'ssa']" label="字幕文件" />
      </template>
      <div class="go"><el-button type="primary" :loading="busy" @click="go">开始</el-button></div>
    </div>

    <div v-if="mode === 'summarize' && summaryText" class="result card">
      <div class="rh">
        <h3>结果</h3>
        <span>
          <el-button size="small" @click="copy(summaryText, '全文')">复制全文</el-button>
          <el-button v-if="chaptersText" size="small" @click="copy(chaptersText, '章节')">复制章节（可贴到视频简介）</el-button>
          <el-button size="small" @click="api.revealFile(summaryFile)">所在文件夹</el-button>
        </span>
      </div>
      <pre class="md selectable">{{ summaryText }}</pre>
      <div v-if="chaptersText" class="wc">
        <h4>把章节写进视频</h4>
        <FileInput v-model="chapterVideo" :kinds="['video', 'audio']" :extensions="['mp4', 'mkv', 'mov', 'm4a', 'mp3']" label="要写入章节的视频" />
        <el-button size="small" type="primary" :disabled="!chapterVideo.length" @click="writeChapters">写入章节（保存为新文件）</el-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.ai {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.panel,
.result {
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: 760px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
h4 {
  margin: 4px 0 0;
  font-size: 12.5px;
  color: var(--cc-mute);
  font-weight: 500;
}
.desc {
  margin: 0;
  font-size: 12.5px;
}
.row {
  display: grid;
  grid-template-columns: 70px auto;
  align-items: center;
  gap: 10px;
}
.row > span {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.short {
  width: 260px;
}
.wide {
  width: 360px;
}
.rh {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.md {
  margin: 0;
  max-height: 340px;
  overflow: auto;
  white-space: pre-wrap;
  font-family: var(--cc-font);
  font-size: 12.5px;
  line-height: 1.7;
  background: var(--cc-bg);
  border-radius: 8px;
  padding: 10px 14px;
}
.wc {
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: flex-start;
}
</style>
