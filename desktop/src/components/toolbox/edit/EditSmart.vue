<script setup lang="ts">
// “智能”页：对所选的视频片段做分析类的操作——场景分割、去静音、波形、降噪 / 人声增强、一键重构画幅。
// 分析只给候选结果，用户确认后才改时间线（可撤销）。
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { convertFileSrc } from '@tauri-apps/api/core'
import { ElMessage } from 'element-plus'
import { api, errorText, events } from '../../../api'
import type { EditApi } from '../../../composables/useEditProject'
import type { AudioFxPreview, EditClip, TimeSpan } from '../../../types'
import { clipDuration, clamp, msText } from '../../../utils/edit'
import EditField from './EditField.vue'
import EditSlider from './EditSlider.vue'

const props = defineProps<{ ed: EditApi }>()
const emit = defineEmits<{ goto: [ms: number] }>()
const ed = props.ed

const clip = computed<EditClip | undefined>(() => (ed.selectedClip.value?.kind === 'video' ? ed.selectedClip.value : undefined))
const placed = computed(() => {
  const i = clip.value ? ed.project.value.clips.findIndex((c) => c.id === clip.value?.id) : -1
  return i >= 0 ? ed.placed.value[i] : undefined
})
const src = computed(() => (clip.value ? ed.sources[clip.value.path] : undefined))
const hasAudio = computed(() => !!src.value?.hasAudio && !clip.value?.mute)

/** 素材时间 → 时间线位置 */
function timelineAt(ms: number): number {
  const c = clip.value
  const pl = placed.value
  if (!c || !pl) return 0
  return clamp(pl.startMs + (ms - c.inMs) / Math.max(0.01, c.speed), pl.startMs, pl.endMs)
}
const span = (s: TimeSpan) => `${msText(timelineAt(s.startMs) - (placed.value?.startMs ?? 0))} – ${msText(timelineAt(s.endMs) - (placed.value?.startMs ?? 0))}`

// 选了别的片段：清掉上一个片段的候选结果
watch(
  () => clip.value?.id,
  () => {
    scene.cuts = []
    scene.picked = []
    sil.spans = []
    wave.url = ''
    aud.value = null
    note.value = ''
  },
)
const note = ref('')
const busy = ref('')

// ---------- 波形 ----------

const wave = reactive({ url: '' })
async function showWave() {
  const c = clip.value
  if (!c || !hasAudio.value) return
  busy.value = 'wave'
  try {
    wave.url = convertFileSrc(await api.editWaveform(c.path, c.inMs, c.outMs, 1000, 90))
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = ''
  }
}

// ---------- 场景分割 ----------

const scene = reactive({ sens: 0.4, cuts: [] as number[], picked: [] as number[] })
async function detectScenes() {
  const c = clip.value
  if (!c) return
  busy.value = 'scene'
  note.value = ''
  try {
    scene.cuts = await api.editScenes(c.path, c.inMs, c.outMs, scene.sens)
    scene.picked = [...scene.cuts]
    if (!scene.cuts.length) note.value = '没有检测到镜头切换。可以把灵敏度调高一点再试。'
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = ''
  }
}
function togglePick(t: number) {
  scene.picked = scene.picked.includes(t) ? scene.picked.filter((x) => x !== t) : [...scene.picked, t]
}
function applyScenes() {
  const c = clip.value
  if (!c || !scene.picked.length) return
  const n = ed.cutClipAt(c.id, scene.picked)
  if (n) {
    ElMessage.success(`已分成 ${n} 段。`)
    scene.cuts = []
    scene.picked = []
  } else {
    ElMessage.warning('这些切点离片段两端太近，没有可分割的位置。')
  }
}

// ---------- 去静音 ----------

const sil = reactive({ db: -35, minMs: 600, keepMs: 200, spans: [] as TimeSpan[] })
async function detectSilence() {
  const c = clip.value
  if (!c || !hasAudio.value) return
  busy.value = 'sil'
  note.value = ''
  try {
    sil.spans = await api.editSilences(c.path, c.inMs, c.outMs, sil.db, sil.minMs)
    if (!sil.spans.length) note.value = '没有检测到静音。可以把阈值调高（更容易算静音）或把最短时长调短。'
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = ''
  }
}
const silTotal = computed(() => sil.spans.reduce((n, s) => n + (s.endMs - s.startMs), 0))
function applySilence() {
  const c = clip.value
  if (!c || !sil.spans.length) return
  const ms = ed.removeClipSilences(c.id, sil.spans, sil.keepMs)
  if (ms > 0) {
    ElMessage.success(`已去掉 ${msText(ms)} 的静音，后面的内容已前移。`)
    sil.spans = []
  } else {
    ElMessage.warning('没有可去掉的静音（静音段比要留的停顿还短，或者整段都是静音）。')
  }
}

// ---------- 声音处理 ----------

const DENOISE = [
  { v: 'off', t: '关' },
  { v: 'light', t: '轻' },
  { v: 'medium', t: '中' },
  { v: 'strong', t: '强' },
] as const
const aud = ref<AudioFxPreview | null>(null)
const auding = ref(false)

async function setDenoise(v: EditClip['denoise']) {
  const c = clip.value
  if (!c) return
  ed.patchClip(c.id, { denoise: v }, `dn:${c.id}`)
  aud.value = null
  // 第一次打开降噪时测一下底噪，降噪按它设置（测不出来就让滤镜自己跟踪）
  if (v !== 'off' && c.noiseFloorDb === null && hasAudio.value) {
    try {
      const nf = await api.editNoiseFloor(c.path, c.inMs, Math.min(c.outMs, c.inMs + 60_000))
      if (nf !== null) ed.patchClip(c.id, { noiseFloorDb: nf }, `nf:${c.id}`)
    } catch {
      /* 测不出来就算了 */
    }
  }
}
function setEnhance(v: boolean) {
  if (!clip.value) return
  ed.patchClip(clip.value.id, { enhance: v }, `en:${clip.value.id}`)
  aud.value = null
}
async function audition() {
  const c = clip.value
  if (!c) return
  if (c.denoise === 'off' && !c.enhance) return ElMessage.warning('请先打开降噪或人声增强。')
  auding.value = true
  try {
    aud.value = await api.audioFxPreview(
      c.path,
      { denoise: c.denoise, enhance: c.enhance, reverb: null, normalize: null },
      c.inMs,
    )
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    auding.value = false
  }
}
const audSrc = (p: string) => convertFileSrc(p) + `?t=${Date.now()}`

// ---------- 一键重构画幅 ----------

const RATIOS = [
  { key: '9:16', label: '9:16 竖屏', w: 1080, h: 1920 },
  { key: '1:1', label: '1:1 方形', w: 1080, h: 1080 },
  { key: '4:5', label: '4:5 竖版', w: 1080, h: 1350 },
  { key: '16:9', label: '16:9 横屏', w: 1920, h: 1080 },
] as const
const rf = reactive({ ratio: '9:16' as (typeof RATIOS)[number]['key'], all: false, smooth: 0.8 })
const tracking = reactive({ running: false, id: 0, percent: 0, done: 0, total: 0 })
let seq = (Math.floor(Date.now() / 1000) % 1_000_000) * 1000 + 500
let unlisten: UnlistenFn | undefined
onMounted(async () => {
  unlisten = await events.onTrackProgress((p) => {
    if (tracking.running && p.id === tracking.id) tracking.percent = clamp(p.percent, 0, 100)
  })
})
onBeforeUnmount(() => {
  unlisten?.()
  if (tracking.running) api.trackCancel(tracking.id).catch(() => undefined)
})

async function reframe() {
  const r = RATIOS.find((x) => x.key === rf.ratio)
  if (!r || tracking.running) return
  const targets = (rf.all ? ed.project.value.clips : clip.value ? [clip.value] : []).filter((c) => c.kind === 'video')
  if (!targets.length) return ElMessage.warning('请先选中一个视频片段。')
  ed.mutate((p) => Object.assign(p.out, { width: r.w, height: r.h }), 'out:reframe')
  Object.assign(tracking, { running: true, done: 0, total: targets.length, percent: 0 })
  const notes: string[] = []
  let centered = 0
  try {
    for (const c of targets) {
      tracking.id = ++seq
      tracking.percent = 0
      const res = await api.editAutoReframe({ id: tracking.id, path: c.path, fromMs: c.inMs, toMs: c.outMs, smooth: rf.smooth })
      if (!ed.clipOf(c.id)) continue
      ed.putRegion(c.id, res.region)
      if (!res.tracked) centered++
      if (res.note && !notes.includes(res.note)) notes.push(res.note)
      tracking.done++
    }
    ElMessage.success(targets.length > 1 ? `已为 ${tracking.done} 个片段取景${centered ? `（其中 ${centered} 个居中取景）` : ''}。` : centered ? '没有找到明显的运动物体，已居中取景。' : '已生成取景区域，可以在“区域”页微调。')
    note.value = notes.join(' ')
  } catch (e) {
    const msg = errorText(e)
    if (msg !== 'canceled') ElMessage.error(msg)
    else ElMessage.info('已取消。')
  } finally {
    tracking.running = false
    tracking.percent = 0
  }
}
async function cancelReframe() {
  if (tracking.running) await api.trackCancel(tracking.id).catch(() => undefined)
}
const hasFocus = computed(() => !!clip.value?.regions.some((r) => r.effect === 'focus'))
</script>

<template>
  <div class="smart">
    <div v-if="!clip" class="mute small">先在时间线上选中一个视频片段。</div>
    <template v-else>
      <div class="mute small">{{ src?.name ?? '' }} · {{ msText(clipDuration(clip)) }}。分析只给出候选结果，确认后才会修改时间线，可以撤销。</div>

      <h4>场景分割</h4>
      <EditSlider label="灵敏度" :model-value="scene.sens" :min="0.1" :max="0.9" :step="0.05" :digits="2" hint="越小越敏感：渐变、闪光多的素材适合调大一点避免误判" @update:model-value="(v) => (scene.sens = v)" />
      <div class="row">
        <el-button size="small" :loading="busy === 'scene'" @click="detectScenes">检测场景</el-button>
        <el-button v-if="scene.cuts.length" size="small" type="primary" :disabled="!scene.picked.length" @click="applyScenes">按选中的 {{ scene.picked.length }} 个切点分割</el-button>
      </div>
      <div v-if="scene.cuts.length" class="chips">
        <el-check-tag v-for="t in scene.cuts" :key="t" :checked="scene.picked.includes(t)" class="chip" @change="togglePick(t)" @dblclick="emit('goto', timelineAt(t))">{{ msText(timelineAt(t) - (placed?.startMs ?? 0)) }}</el-check-tag>
      </div>
      <div v-if="scene.cuts.length" class="mute small">点一下选中 / 取消，双击跳到那个位置查看。</div>

      <h4>去静音</h4>
      <template v-if="hasAudio">
        <EditSlider label="静音阈值" :model-value="sil.db" :min="-60" :max="-20" :step="1" unit=" dB" hint="低于这个音量算静音；背景音乐大时先降噪再检测" @update:model-value="(v) => (sil.db = v)" />
        <EditSlider label="最短时长" :model-value="sil.minMs" :min="200" :max="3000" :step="100" :scale="0.001" :digits="1" unit=" 秒" @update:model-value="(v) => (sil.minMs = v)" />
        <EditSlider label="保留停顿" :model-value="sil.keepMs" :min="0" :max="1000" :step="50" :scale="0.001" :digits="2" unit=" 秒" hint="每个静音段剪掉后保留这么长的停顿，免得语气太生硬" @update:model-value="(v) => (sil.keepMs = v)" />
        <div class="row">
          <el-button size="small" :loading="busy === 'sil'" @click="detectSilence">检测静音</el-button>
          <el-button v-if="sil.spans.length" size="small" type="primary" @click="applySilence">去掉 {{ sil.spans.length }} 段静音（共 {{ msText(silTotal) }}）</el-button>
          <el-button size="small" :loading="busy === 'wave'" @click="showWave">显示波形</el-button>
        </div>
        <img v-if="wave.url" :src="wave.url" class="wave" alt="声音波形" />
        <div v-if="sil.spans.length" class="chips">
          <el-tag v-for="s in sil.spans" :key="s.startMs" size="small" class="chip" @dblclick="emit('goto', timelineAt(s.startMs))">{{ span(s) }}</el-tag>
        </div>
        <div v-if="sil.spans.length" class="mute small">双击一段可以跳到那个位置听一下。去掉静音后，这个片段之后的叠加素材、文字、配乐会一起前移；片段中间的不会动。</div>
      </template>
      <div v-else class="mute small">这个片段没有声音（或已静音）。</div>

      <h4>降噪 / 人声增强</h4>
      <template v-if="hasAudio">
        <EditField label="降噪">
          <el-radio-group :model-value="clip.denoise" size="small" @update:model-value="(v: unknown) => setDenoise(v as EditClip['denoise'])">
            <el-radio-button v-for="d in DENOISE" :key="d.v" :value="d.v">{{ d.t }}</el-radio-button>
          </el-radio-group>
        </EditField>
        <EditField label="人声增强" hint="切掉低频隆隆声、提一点清晰度、轻度压缩让音量更均匀">
          <el-switch :model-value="clip.enhance" @update:model-value="(v: unknown) => setEnhance(!!v)" />
        </EditField>
        <div class="row">
          <el-button size="small" :loading="auding" :disabled="clip.denoise === 'off' && !clip.enhance" @click="audition">试听 15 秒</el-button>
        </div>
        <template v-if="aud">
          <EditField label="原声"><audio :src="audSrc(aud.before)" controls preload="none" class="aud" /></EditField>
          <EditField label="处理后"><audio :src="audSrc(aud.after)" controls preload="none" class="aud" /></EditField>
          <div v-for="n in aud.notes" :key="n" class="mute small">{{ n }}</div>
        </template>
        <div class="mute small">实时预览听不到处理效果，请用“试听 15 秒”；导出时才会真正处理。底噪是稳定的嘶嘶声时效果最好。</div>
      </template>
      <div v-else class="mute small">这个片段没有声音（或已静音）。</div>

      <h4>一键重构画幅</h4>
      <EditField label="目标比例">
        <el-radio-group v-model="rf.ratio" size="small">
          <el-radio-button v-for="r in RATIOS" :key="r.key" :value="r.key">{{ r.label }}</el-radio-button>
        </el-radio-group>
      </EditField>
      <EditSlider label="镜头平滑" :model-value="rf.smooth" :min="0" :max="3" :step="0.1" :digits="1" unit=" 秒" hint="越大越稳、跟得越慢" @update:model-value="(v) => (rf.smooth = v)" />
      <el-checkbox v-model="rf.all" :disabled="ed.project.value.clips.length < 2">对时间线上所有视频片段分别处理（先用“场景分割”切成镜头效果更好）</el-checkbox>
      <div class="row">
        <el-button v-if="!tracking.running" size="small" type="primary" @click="reframe">自动取景</el-button>
        <template v-else>
          <el-progress :percentage="tracking.percent" :stroke-width="6" class="prog" />
          <span class="mute small">{{ tracking.done + 1 }} / {{ tracking.total }}</span>
          <el-button size="small" @click="cancelReframe">取消</el-button>
        </template>
      </div>
      <div class="mute small">
        会把成片画面尺寸改成所选比例，找到画面里最显眼的运动物体并整段追踪，生成一个“聚焦”区域（可以在“区域”页微调或手动校正）。只认运动物体，不认人脸；静止的画面会居中取景。{{ hasFocus ? '这个片段已经有取景区域，重新取景会替换它。' : '' }}
      </div>
      <div v-if="note" class="note small">{{ note }}</div>
    </template>
  </div>
</template>

<style scoped>
.smart {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
h4 {
  margin: 10px 0 0;
  font-size: 13px;
  padding-top: 8px;
  border-top: 1px solid var(--cc-line, rgba(127, 127, 127, 0.25));
}
h4:first-of-type {
  border-top: 0;
  padding-top: 0;
}
.small {
  font-size: 11.5px;
}
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}
.row :deep(.el-button + .el-button) {
  margin-left: 0;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.chip {
  cursor: pointer;
  user-select: none;
  font-variant-numeric: tabular-nums;
}
.wave {
  width: 100%;
  height: 56px;
  object-fit: fill;
  border-radius: 6px;
  background: rgba(127, 127, 127, 0.12);
}
.aud {
  height: 32px;
  max-width: 100%;
}
.prog {
  flex: 1;
  min-width: 120px;
}
.note {
  color: var(--cc-warn, #b7791f);
}
.smart :deep(.el-checkbox) {
  height: auto;
  min-height: 24px;
  align-items: flex-start;
  white-space: normal;
}
.smart :deep(.el-checkbox__label) {
  white-space: normal;
  line-height: 1.45;
}
</style>
