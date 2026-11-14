<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api, errorText } from '../../api'
import type { AudioFxPreview, ToolTrial } from '../../types'
import { formatBytes, formatDuration } from '../../utils/format'
import { pickKinds, type PreviewSpec, type PreviewTier } from '../../utils/toolbox'

const props = defineProps<{ files: string[]; spec: PreviewSpec | null; tier: PreviewTier }>()

const video = computed(() => pickKinds(props.files, ['video'])[0] ?? '')
const src = (p: string) => convertFileSrc(p) + `?t=${stamp.value}`
const stamp = ref(Date.now())

// 三种结果：前后两张图 / 试一小段的两个文件 / 试听的两段声音。换文件或换工具时清掉。
interface Pair {
  before: string
  after: string
  labels: [string, string]
  notes: string[]
}
const pair = ref<Pair | null>(null)
const trial = ref<ToolTrial | null>(null)
const audio = ref<AudioFxPreview | null>(null)

// ---------- 时间点 ----------

const duration = ref(0)
const at = ref(0)
watch(
  video,
  async (v) => {
    duration.value = 0
    pair.value = null
    trial.value = null
    audio.value = null
    if (!v) return
    const i = await api.mediaInfo(v).catch(() => null) // 忽略：可选数据，取不到就按空处理
    if (video.value !== v) return
    duration.value = i?.durationMs ?? 0
    at.value = Math.min(Math.round(duration.value * 0.1), Math.max(0, duration.value - 1000))
  },
  { immediate: true },
)
const needsScrubber = computed(() => !!props.spec && ['frame', 'normalize', 'trial', 'loudness'].includes(props.spec.kind) && duration.value > 0)

// ---------- 实时：前后对比的图 ----------

const busy = ref(false)
const error = ref('')
let seq = 0
let timer = 0

const isLive = computed(() => !!props.spec && ['frame', 'normalize', 'still', 'trim'].includes(props.spec.kind))

async function runLive() {
  const s = props.spec
  const f = video.value
  if (!s || !f || !isLive.value) return
  const my = ++seq
  busy.value = true
  error.value = ''
  try {
    let next: Pair | null = null
    if (s.kind === 'frame') {
      const r = await api.toolFramePreview(f, at.value, s.op)
      next = { before: r.before, after: r.after, labels: ['原片', '处理后'], notes: [] }
    } else if (s.kind === 'normalize') {
      const r = await api.videoPreview(f, s.spec, at.value)
      next = { before: r.before, after: r.after, labels: ['原片', '规整后'], notes: [...r.notes, ...r.warnings.map((w) => `注意：${w}`)] }
    } else if (s.kind === 'still') {
      const r = await api.toolFramePreview(f, s.atMs, { kind: 'none' })
      next = { before: r.after, after: r.after, labels: ['截图', '截图'], notes: [] }
    } else if (s.kind === 'trim') {
      const [a, b] = await Promise.all([
        api.toolFramePreview(f, s.startMs, { kind: 'none' }),
        api.toolFramePreview(f, s.endMs ?? Math.max(0, duration.value - 500), { kind: 'none' }),
      ])
      next = { before: a.after, after: b.after, labels: ['开始', '结束'], notes: [] }
    }
    if (my === seq) {
      stamp.value = Date.now()
      pair.value = next
    }
  } catch (e) {
    if (my === seq) error.value = errorText(e)
  } finally {
    if (my === seq) busy.value = false
  }
}

function schedule() {
  window.clearTimeout(timer)
  if (!isLive.value) return
  timer = window.setTimeout(() => void runLive(), 350)
}
watch(() => [props.spec, at.value, video.value], schedule, { deep: true, immediate: true })
onBeforeUnmount(() => window.clearTimeout(timer))

// 换到别的工具，上一个工具的结果作废
watch(
  () => props.spec?.kind,
  () => {
    pair.value = null
    trial.value = null
    audio.value = null
    error.value = ''
  },
)

// ---------- 对比方式 ----------

const mode = ref<'slide' | 'side' | 'switch'>('slide')
const pos = ref(50)
const showAfter = ref(true)
const twin = computed(() => props.spec?.kind === 'trim')
const one = computed(() => props.spec?.kind === 'still')

// ---------- 试一小段 ----------

const trialLen = ref(5)
const trialBusy = ref(false)
const trialView = ref<'before' | 'after' | 'side'>('side')

async function runTrial() {
  const s = props.spec
  if (!s || !video.value) return
  error.value = ''
  trialBusy.value = true
  trial.value = null
  audio.value = null
  try {
    if (s.kind === 'trial') {
      const start = s.startMs ?? at.value
      trial.value = await api.toolTrial(video.value, s.op, start, (s.durMs ?? trialLen.value * 1000))
      stamp.value = Date.now()
    } else if (s.kind === 'loudness') {
      audio.value = await api.audioFxPreview(
        video.value,
        { denoise: 'off', enhance: false, reverb: null, normalize: s.target, normalizeTp: s.tp ?? null, normalizeLra: s.lra ?? null },
        at.value,
        trialLen.value * 4000,
      )
      stamp.value = Date.now()
    }
  } catch (e) {
    error.value = errorText(e)
  } finally {
    trialBusy.value = false
  }
}

const isTrial = computed(() => props.spec?.kind === 'trial' || props.spec?.kind === 'loudness')
const fixedStart = computed(() => props.spec?.kind === 'trial' && props.spec.startMs != null)
/** 压缩时按这一小段的体积粗略推算整段 */
const estimate = computed(() => {
  const t = trial.value
  if (!t || !t.totalSecs || props.spec?.kind !== 'trial' || props.spec.op.op !== 'compress') return ''
  return formatBytes((t.afterBytes / t.secs) * t.totalSecs)
})
const ratio = computed(() => {
  const t = trial.value
  return t && t.beforeBytes > 0 ? Math.round((t.afterBytes / t.beforeBytes) * 100) : 0
})
</script>

<template>
  <section class="pv" aria-label="预览">
    <div class="head">
      <strong>预览</strong>
      <span class="pill" :class="tier">{{ tier === 'live' ? '实时' : '试一小段' }}</span>
      <span class="sp" />
      <div v-if="pair && !twin && !one && isLive" class="seg" role="group" aria-label="对比方式">
        <button type="button" :class="{ on: mode === 'slide' }" @click="mode = 'slide'">滑动</button>
        <button type="button" :class="{ on: mode === 'side' }" @click="mode = 'side'">并排</button>
        <button type="button" :class="{ on: mode === 'switch' }" @click="mode = 'switch'">切换</button>
      </div>
      <div v-else-if="trial && !trial.afterIsImage" class="seg" role="group" aria-label="对比方式">
        <button type="button" :class="{ on: trialView === 'before' }" @click="trialView = 'before'">原片</button>
        <button type="button" :class="{ on: trialView === 'after' }" @click="trialView = 'after'">处理后</button>
        <button type="button" :class="{ on: trialView === 'side' }" @click="trialView = 'side'">并排</button>
      </div>
    </div>

    <div v-if="!video && spec?.kind !== 'loudness'" class="empty mute">把视频放进上面的文件篮，这里显示预览。</div>
    <div v-else-if="!spec" class="empty mute">这个工具现在没有可预览的内容。</div>

    <template v-else>
      <!-- 实时：图 -->
      <div v-if="isLive" class="stage" :class="{ loading: busy }">
        <template v-if="pair">
          <div v-if="one" class="single"><img :src="src(pair.after)" alt="截图" /></div>
          <div v-else-if="twin || mode === 'side'" class="sidebyside">
            <figure v-for="(img, i) in [pair.before, pair.after]" :key="i">
              <img :src="src(img)" :alt="pair.labels[i]" />
              <figcaption>{{ pair.labels[i] }}</figcaption>
            </figure>
          </div>
          <div v-else-if="mode === 'switch'" class="single">
            <img :src="src(showAfter ? pair.after : pair.before)" :alt="showAfter ? pair.labels[1] : pair.labels[0]" />
            <button type="button" class="flip" @click="showAfter = !showAfter">{{ showAfter ? pair.labels[1] : pair.labels[0] }}（点击切换）</button>
          </div>
          <div v-else class="slide">
            <img class="base" :src="src(pair.after)" :alt="pair.labels[1]" />
            <img class="over" :src="src(pair.before)" :alt="pair.labels[0]" :style="{ clipPath: `inset(0 ${100 - pos}% 0 0)` }" />
            <div class="line" :style="{ left: pos + '%' }" />
            <span class="tag l">{{ pair.labels[0] }}</span>
            <span class="tag r">{{ pair.labels[1] }}</span>
            <input v-model.number="pos" class="range" type="range" min="0" max="100" aria-label="对比位置" />
          </div>
        </template>
        <div v-else-if="!error" class="empty mute">{{ busy ? '正在取画面…' : '准备中…' }}</div>
      </div>

      <!-- 试一小段：视频 / 动图 / 声音 -->
      <div v-else-if="isTrial" class="stage trial">
        <div class="tbar">
          <template v-if="!fixedStart">
            <span class="mute">从</span>
            <span class="mono">{{ formatDuration(at) }}</span>
          </template>
          <span class="mute">试</span>
          <el-select v-model="trialLen" size="small" class="len">
            <el-option v-for="n in [3, 5, 8]" :key="n" :value="n" :label="spec.kind === 'loudness' ? `${n * 4} 秒` : `${n} 秒`" />
          </el-select>
          <span class="sp" />
          <el-button size="small" type="primary" plain :loading="trialBusy" @click="runTrial">{{ spec.kind === 'loudness' ? '试听一下' : '试一下' }}</el-button>
        </div>
        <template v-if="trial">
          <div v-if="trial.afterIsImage" class="single"><img :src="src(trial.after)" alt="预览动图" /></div>
          <div v-else class="vids" :class="trialView">
            <figure v-if="trialView !== 'after'"><video :src="src(trial.before)" controls muted loop /><figcaption>原片</figcaption></figure>
            <figure v-if="trialView !== 'before'"><video :src="src(trial.after)" controls muted loop /><figcaption>处理后</figcaption></figure>
          </div>
          <div class="stats">
            <div><span class="mute">这 {{ trial.secs }} 秒</span><span>{{ formatBytes(trial.beforeBytes) }} → <b>{{ formatBytes(trial.afterBytes) }}</b><template v-if="ratio && !trial.afterIsImage">（{{ ratio }}%）</template></span></div>
            <div v-if="estimate"><span class="mute">整段粗略估算</span><span>约 {{ estimate }}（按这一小段推算，仅供参考）</span></div>
            <div v-if="trial.note" class="mute">{{ trial.note }}</div>
          </div>
        </template>
        <template v-else-if="audio">
          <div class="aud"><span class="mute">原声</span><audio :src="src(audio.before)" controls preload="none" /></div>
          <div class="aud"><span class="mute">处理后</span><audio :src="src(audio.after)" controls preload="none" /></div>
          <div v-for="n in audio.notes" :key="n" class="mute small">{{ n }}</div>
        </template>
        <div v-else-if="!error" class="empty mute">{{ trialBusy ? '正在处理这一小段…' : spec.kind === 'loudness' ? '点“试听一下”，只处理这一小段，不生成文件。' : '点“试一下”，只处理这一小段，不生成文件。' }}</div>
      </div>

      <div v-if="needsScrubber && !fixedStart" class="scrub">
        <span class="mono mute">{{ formatDuration(at) }}</span>
        <el-slider v-model="at" :min="0" :max="Math.max(1, duration - 500)" :step="500" :show-tooltip="false" size="small" aria-label="预览时间点" />
        <span class="mono mute">{{ formatDuration(duration) }}</span>
      </div>
      <div v-if="error" class="err selectable">{{ error }}</div>
      <div v-if="pair?.notes.length" class="notes mute">
        <div v-for="n in pair.notes" :key="n">· {{ n }}</div>
      </div>
      <p class="foot mute">
        <template v-if="isLive">{{ tier === 'live' ? '拖动时间条换一帧；改任何参数，画面自动重画。' : '' }}</template>
        <template v-else>预览用文件篮里的第一个文件，只处理这一小段，不生成文件、不进媒体库。</template>
      </p>
    </template>
  </section>
</template>

<style scoped>
.pv {
  width: 468px;
  flex: none;
  box-sizing: border-box;
  background: var(--cc-card, var(--cc-bg));
  border: 1px solid var(--cc-line);
  border-radius: 10px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-self: flex-start;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.sp {
  flex: 1;
}
.pill {
  font-size: 12px;
  font-weight: 600;
  padding: 1px 8px;
  border-radius: 10px;
  border: 1px solid;
}
.pill.live {
  color: #0b6b3a;
  background: #e6f6ec;
  border-color: #b4e2c6;
}
.pill.trial {
  color: #a14a12;
  background: #fdf0e6;
  border-color: #f3cfb2;
}
:global(html.dark .pill.live) {
  color: #7fd9a6;
  background: #16301f;
  border-color: #2c5a3c;
}
:global(html.dark .pill.trial) {
  color: #f0b27f;
  background: #35261a;
  border-color: #5c4028;
}
.seg {
  display: flex;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
  overflow: hidden;
  font-size: 12px;
}
.seg button {
  all: unset;
  cursor: pointer;
  padding: 3px 10px;
}
.seg button + button {
  border-left: 1px solid var(--cc-line);
}
.seg button.on {
  background: var(--cc-acc-soft);
  color: var(--cc-acc);
  font-weight: 600;
}
.seg button:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.empty {
  padding: 36px 8px;
  text-align: center;
  font-size: 12.5px;
}
.stage {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 120px;
}
.stage.loading img {
  opacity: 0.6;
}
.stage img,
.stage video {
  width: 100%;
  display: block;
  border-radius: 6px;
  background: #1b1f27;
}
.single {
  position: relative;
}
.flip {
  all: unset;
  cursor: pointer;
  position: absolute;
  left: 8px;
  top: 8px;
  font-size: 12px;
  font-weight: 600;
  color: #fff;
  background: rgba(15, 19, 32, 0.72);
  padding: 2px 8px;
  border-radius: 4px;
}
.sidebyside {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
figure {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
figcaption {
  font-size: 12px;
  color: var(--cc-mute);
  text-align: center;
}
.slide {
  position: relative;
  border-radius: 6px;
  overflow: hidden;
}
.slide .over {
  position: absolute;
  inset: 0;
  height: 100%;
  object-fit: contain;
}
.line {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  margin-left: -1px;
  background: #fff;
  box-shadow: 0 0 3px rgba(0, 0, 0, 0.5);
  pointer-events: none;
}
.tag {
  position: absolute;
  top: 8px;
  font-size: 12px;
  font-weight: 600;
  color: #fff;
  background: rgba(15, 19, 32, 0.72);
  padding: 2px 8px;
  border-radius: 4px;
  pointer-events: none;
}
.tag.l {
  left: 8px;
}
.tag.r {
  right: 8px;
}
.range {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  opacity: 0;
  cursor: ew-resize;
}
.range:focus-visible {
  opacity: 0.001;
}
.tbar {
  display: flex;
  align-items: center;
  gap: 8px;
}
.len {
  width: 84px;
}
.vids {
  display: grid;
  gap: 8px;
}
.vids.side {
  grid-template-columns: 1fr 1fr;
}
.stats {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--cc-bg);
  font-size: 12.5px;
}
.stats > div {
  display: flex;
  justify-content: space-between;
  gap: 12px;
}
.aud {
  display: flex;
  align-items: center;
  gap: 8px;
}
.aud audio {
  flex: 1;
  min-width: 0;
  height: 34px;
}
.scrub {
  display: flex;
  align-items: center;
  gap: 10px;
}
.scrub .el-slider {
  flex: 1;
}
.err {
  color: var(--cc-err);
  font-size: 12.5px;
}
.notes,
.small {
  font-size: 12px;
}
.foot {
  margin: 0;
  font-size: 12px;
}
</style>
