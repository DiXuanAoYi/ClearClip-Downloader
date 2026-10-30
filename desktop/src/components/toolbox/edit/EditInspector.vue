<script setup lang="ts">
// 检查器：选中的片段 / 叠加素材 / 文字 / 音频的属性，以及输出设置。
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText, events } from '../../../api'
import type { EditApi } from '../../../composables/useEditProject'
import type { RegionApi } from '../../../composables/useRegions'
import type { OverlayAnim } from '../../../types'
import type { EditChroma, EditClip, EditOutput, EditOverlay } from '../../../types'
import { MAX_OVERLAY_TRACKS, MAX_START_MS, MIN_MS, LOW_BEAT_CONFIDENCE, MIN_TRANSITION_MS, OVERLAY_KEY_PROPS, SPEED_RAMPS, TEXT_KEY_PROPS, TRANSITIONS, clamp, clipDuration, defaultOverlayScale, isGif, msText, outputSize, overlayDuration, overlayFitScale, overlaySourceLimit, withKeys } from '../../../utils/edit'
import EditField from './EditField.vue'
import EditKeyframes from './EditKeyframes.vue'
import EditRegions from './EditRegions.vue'
import EditSmart from './EditSmart.vue'
import EditTranscript from './EditTranscript.vue'
import EditSlider from './EditSlider.vue'
import TimeInput from './TimeInput.vue'

const props = defineProps<{ ed: EditApi; rg: RegionApi }>()
const emit = defineEmits<{ goto: [ms: number] }>()
const ed = props.ed
const outDir = defineModel<string>('outDir', { default: '' })

const tab = ref<'item' | 'region' | 'smart' | 'script' | 'out'>('item')
watch(
  () => ed.sel.value,
  (s) => {
    // 选了别的片段时留在“区域”页，方便连着给几个镜头加；选了文字、配乐就回到“属性”
    if (s && !((tab.value === 'region' || tab.value === 'smart' || tab.value === 'script') && s.kind === 'clip')) tab.value = 'item'
  },
)

const clip = ed.selectedClip
const overlay = ed.selectedOverlay
const text = ed.selectedText
const audio = ed.selectedAudio
const src = computed(() => (clip.value ? ed.sources[clip.value.path] : overlay.value ? ed.sources[overlay.value.path] : audio.value ? ed.sources[audio.value.path] : undefined))
const index = computed(() => (clip.value ? ed.project.value.clips.findIndex((c) => c.id === clip.value?.id) : -1))
const placed = computed(() => (index.value >= 0 ? ed.placed.value[index.value] : undefined))
const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p

// ---------- 修改 ----------

function pc(patch: Partial<EditClip>, key: string) {
  if (clip.value) ed.patchClip(clip.value.id, patch, `${key}:${clip.value.id}`)
}
const po = (patch: Partial<EditOverlay>, key: string) => overlay.value && ed.patchOverlay(overlay.value.id, patch, `${key}:${overlay.value.id}`)
const pt = (patch: Parameters<typeof ed.patchText>[1], key: string) => text.value && ed.patchText(text.value.id, patch, `${key}:${text.value.id}`)
const pa = (patch: Parameters<typeof ed.patchAudio>[1], key: string) => audio.value && ed.patchAudio(audio.value.id, patch, `${key}:${audio.value.id}`)

// 叠加素材 / 文字此刻的实际值（有关键帧的属性随播放头变）；改这些值时，有关键帧的会在播放头处改（或新建）关键点
const ovLocal = computed(() => (overlay.value ? clamp(ed.playhead.value - overlay.value.startMs, 0, overlayDuration(overlay.value)) : 0))
const eo = computed(() => (overlay.value ? withKeys(overlay.value, ovLocal.value) : null))
const txLocal = computed(() => (text.value ? clamp(ed.playhead.value - text.value.startMs, 0, text.value.endMs - text.value.startMs) : 0))
const et = computed(() => (text.value ? withKeys(text.value, txLocal.value) : null))
const pk = (patch: Parameters<typeof ed.setOverlayProps>[1], key: string) => overlay.value && ed.setOverlayProps(overlay.value.id, patch, `${key}:${overlay.value.id}`)
const ptk = (patch: Parameters<typeof ed.setTextProps>[1], key: string) => text.value && ed.setTextProps(text.value.id, patch, `${key}:${text.value.id}`)
// ---- 抠像 ----
const pch = (patch: Partial<EditChroma>, key: string) => overlay.value && ed.patchOverlay(overlay.value.id, { chroma: { ...overlay.value.chroma, ...patch } }, `${key}:${overlay.value.id}`)
const KEY_COLORS = [
  { label: '绿幕', color: '#00FF00' },
  { label: '蓝幕', color: '#0000FF' },
]
const hasEyeDropper = typeof window !== 'undefined' && 'EyeDropper' in window
async function pickKeyColor() {
  try {
    const r = await new (window as unknown as { EyeDropper: new () => { open: () => Promise<{ sRGBHex: string }> } }).EyeDropper().open()
    pch({ color: r.sRGBHex.toUpperCase(), on: true }, 'chc')
  } catch {
    // 用户按 Esc 取消
  }
}

// ---- 节拍 ----
const beatBusy = ref(false)
const perClip = ref(2)
async function findBeats() {
  const a = audio.value
  if (!a) return
  const end = src.value?.durationMs
  if (!end) return void ElMessage.warning('读不到这条配乐的长度。')
  beatBusy.value = true
  try {
    const r = await api.editBeats(a.path, 0, end)
    ed.patchAudio(a.id, { beats: { bpm: r.bpm, confidence: r.confidence, beatsMs: r.beatsMs } })
    if (r.confidence < LOW_BEAT_CONFIDENCE) ElMessage.warning(`找到 ${r.beatsMs.length} 个拍子（约 ${r.bpm} BPM），但节奏不太清晰，结果可能不准。`)
    else ElMessage.success(`找到 ${r.beatsMs.length} 个拍子，约 ${r.bpm} BPM。`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    beatBusy.value = false
  }
}
function arrangeNow() {
  const a = audio.value
  if (!a) return
  const r = ed.arrangeToBeatsOf(a.id, perClip.value)
  if (!r) return void ElMessage.warning('先分析节拍，并且主轨要有片段。')
  const parts = [`${r.arranged} 个片段已对齐节拍（转场已去掉）`]
  if (r.short) parts.push(`${r.short} 个视频素材不够长，已用满它`)
  if (r.left) parts.push(`${r.left} 个片段没有拍子可用，保持原样`)
  ElMessage.success(parts.join('；') + '。可以撤回。')
}
function fitNow() {
  const a = audio.value
  if (a && !ed.fitAudioToVideo(a.id)) ElMessage.warning('成片太短，或配乐开始得太晚，没法适配。')
}

const keyedCount = (item: { keyframes: Record<string, unknown[]> } | undefined | null) => Object.values(item?.keyframes ?? {}).filter((v) => v.length).length

const clipLimit = computed(() => src.value?.durationMs ?? Number.MAX_SAFE_INTEGER)

const holdSec = ref(2)
function applyRamp(id: string) {
  const c = clip.value
  const r = SPEED_RAMPS.find((x) => x.id === id)
  if (!c || !r) return
  const n = ed.applySpeedRamp(c.id, r.steps)
  if (n) ElMessage.success(`已拆成 ${n} 段，各段速度不同；可以在时间线上选中某一段继续调`)
  else ElMessage.warning('这个片段太短，或已经倒放 / 定格，不能套变速曲线。')
}
function freezeNow() {
  if (!ed.freezeAtPlayhead(Math.round(holdSec.value * 1000))) ElMessage.warning('播放头不在一个普通视频片段上。把播放头移到要定格的画面再点。')
}

function setTransition(kind: string) {
  if (!clip.value) return
  if (!kind || kind === 'none') pc({ transition: null }, 'tr')
  else pc({ transition: { kind, durationMs: clip.value.transition?.durationMs ?? 500 } }, 'tr')
}
function setTransitionMs(ms: number) {
  if (clip.value?.transition) pc({ transition: { ...clip.value.transition, durationMs: Math.round(ms) } }, 'trd')
}
const appliedOverlap = computed(() => placed.value?.overlapMs ?? 0)

/** 把这个片段的设置应用到其他所有片段（撤回一次就能还原） */
function applyAll(what: 'transition' | 'tone' | 'turn') {
  const n = ed.applyToAll(what)
  ElMessage.success(n ? `已应用到其他 ${n} 个片段。` : '其他片段已经是这样了。')
}
function clearAllTransitions() {
  const n = ed.removeAllTransitions()
  ElMessage.success(n ? `已去掉 ${n} 个转场。` : '没有转场可去掉。')
}
const hasTransitions = computed(() => ed.project.value.clips.some((c) => c.transition))
const multiClip = computed(() => ed.project.value.clips.length > 1)

function resetTone() {
  pc({ brightness: 0, contrast: 1, saturation: 1 }, 'tone')
}
const toned = computed(() => !!clip.value && (clip.value.brightness !== 0 || clip.value.contrast !== 1 || clip.value.saturation !== 1))

// ---------- 叠加素材 ----------

const ovLimit = computed(() => (overlay.value ? overlaySourceLimit(overlay.value, src.value) : Infinity))
const ovMax = computed(() => (Number.isFinite(ovLimit.value) ? ovLimit.value : Number.MAX_SAFE_INTEGER))
const ovSrcSize = computed(() => (src.value ? { w: src.value.width, h: src.value.height } : undefined))
const frame = computed(() => outputSize(ed.project.value, ed.sources))
const ovToned = computed(() => !!overlay.value && (overlay.value.brightness !== 0 || overlay.value.contrast !== 1 || overlay.value.saturation !== 1))

function fitOverlay(mode: 'contain' | 'cover') {
  if (!overlay.value) return
  po({ scale: overlayFitScale(overlay.value, ovSrcSize.value, frame.value, mode), x: 0.5, y: 0.5 }, 'fit')
}
function resetOverlayBox() {
  if (!overlay.value) return
  po({ scale: defaultOverlayScale({ w: src.value?.width ?? 0, h: src.value?.height ?? 0 }, frame.value), x: 0.5, y: 0.5, rotate: 0 }, 'fit')
}
function turnOverlay(deg: number) {
  const o = overlay.value
  if (!o) return
  let r = o.rotate + deg
  if (r > 360) r -= 360
  if (r < -360) r += 360
  po({ rotate: r }, 'orot')
}
function resetOverlayTone() {
  po({ brightness: 0, contrast: 1, saturation: 1 }, 'otone')
}
function moveToMain() {
  if (overlay.value && !ed.overlayToMain(overlay.value.id)) ElMessage.warning('主轨的片段已经到上限了，放不下。')
}
function moveToOverlay() {
  const c = clip.value
  if (!c) return
  const lost = ed.clipToOverlayTrack(c.id)
  if (lost === null) ElMessage.warning('叠加素材已经到上限了，放不下。')
  else if (lost > 0) ElMessage.info(`这个片段上的 ${lost} 个区域不会带到叠加轨（叠加素材没有区域功能）。`)
}
const ANIM_OPTIONS: { value: OverlayAnim; label: string }[] = [
  { value: 'none', label: '无' },
  { value: 'slide-left', label: '从左滑入 / 向左滑出' },
  { value: 'slide-right', label: '从右滑入 / 向右滑出' },
  { value: 'slide-up', label: '从上滑入 / 向上滑出' },
  { value: 'slide-down', label: '从下滑入 / 向下滑出' },
  { value: 'zoom', label: '缩放' },
]
const ovHint = computed(() => {
  const o = overlay.value
  if (!o) return ''
  const total = ed.total.value
  if (!total) return '主轨上还没有视频，叠加素材要有主轨才能出现在成片里。'
  if (o.startMs >= total) return '它从主轨结尾之后才开始，成片里不会出现。'
  if (o.startMs + overlayDuration(o) > total) return '它的结尾超出了主轨，成片里超出的部分会被截掉（叠加素材不会撑长成片）。'
  return ''
})

async function relink() {
  const c = clip.value
  const a = audio.value
  const old = c?.path ?? overlay.value?.path ?? a?.path
  if (!old) return
  try {
    const r = await open({ multiple: false })
    if (!r || Array.isArray(r)) return
    await ed.relink(old, r)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

// ---------- HDR 素材转成普通画面 ----------

/** 正在转换的素材：转换完成后自动把工程里用到它的地方换成转换后的文件（可以撤回）。 */
const hdrJob = ref<{ id: number; path: string; percent: number } | null>(null)
let unlistenJobs: UnlistenFn | undefined
onMounted(async () => {
  unlistenJobs = await events.onMediaJobs(async (list) => {
    const cur = hdrJob.value
    if (!cur) return
    const j = list.find((x) => x.id === cur.id)
    if (!j) return
    if (j.status === 'done' && j.output) {
      hdrJob.value = null
      try {
        await ed.relink(cur.path, j.output)
        ElMessage.success('已换成转换后的普通画面（可以撤回）')
      } catch (e) {
        ElMessage.error(errorText(e))
      }
    } else if (j.status === 'failed') {
      hdrJob.value = null
      ElMessage.error(j.error ?? '转换失败。')
    } else if (j.status === 'canceled') {
      hdrJob.value = null
    } else {
      cur.percent = j.percent
    }
  })
})
onBeforeUnmount(() => unlistenJobs?.())

/** 用视频规整的“通用兼容”预设只做 HDR → SDR：尺寸、帧率、声音都不动，转完放在原文件旁边。 */
async function convertHdr() {
  const path = clip.value?.path ?? overlay.value?.path
  if (!path || hdrJob.value) return
  try {
    const preset = (await api.videoPresets()).find((p) => p.id === 'compat')
    if (!preset) throw new Error('找不到“通用兼容”预设。')
    const spec = { ...preset.spec, size: 'keep' as const, fps: 'keep' as const, fixSync: false, quality: 'high' as const }
    const id = await api.mediaJobStart({ op: 'normalize', spec, preset: 'compat', inputs: [path], outputDir: null })
    hdrJob.value = { id, path, percent: 0 }
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
const hdrBusy = computed(() => hdrJob.value !== null && hdrJob.value.path === (clip.value?.path ?? overlay.value?.path))

async function pickFont() {
  try {
    const r = await open({ multiple: false, filters: [{ name: '字体', extensions: ['ttf', 'otf', 'ttc'] }] })
    if (r && !Array.isArray(r)) pt({ font: r }, 'font')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

/** 配乐的终点：勾上“到结尾”就清空，取消勾选时落在素材结尾。 */
function audioToEnd(v: unknown) {
  const a = audio.value
  if (a) pa({ outMs: v ? null : (src.value?.durationMs ?? a.inMs + 1000) }, 'ao')
}

function stampStart() {
  const t = text.value
  if (!t) return
  const at = Math.round(ed.playhead.value)
  pt({ startMs: at, endMs: Math.max(t.endMs, at + MIN_MS) }, 'tstart')
}
function stampEnd() {
  const t = text.value
  if (!t) return
  const at = Math.round(ed.playhead.value)
  if (at > t.startMs + MIN_MS - 1) pt({ endMs: at }, 'tend')
}

// ---------- 输出 ----------

const RES: { key: string; label: string; w: number; h: number }[] = [
  { key: 'auto', label: '跟随第一个片段', w: 0, h: 0 },
  { key: '1920x1080', label: '1920×1080 横屏 1080p', w: 1920, h: 1080 },
  { key: '1280x720', label: '1280×720 横屏 720p', w: 1280, h: 720 },
  { key: '3840x2160', label: '3840×2160 横屏 4K', w: 3840, h: 2160 },
  { key: '1080x1920', label: '1080×1920 竖屏', w: 1080, h: 1920 },
  { key: '720x1280', label: '720×1280 竖屏', w: 720, h: 1280 },
  { key: '1080x1080', label: '1080×1080 方形', w: 1080, h: 1080 },
  { key: '1080x1350', label: '1080×1350 竖版 4:5', w: 1080, h: 1350 },
]
const out = computed(() => ed.project.value.out)
const resKey = computed(() => {
  const o = out.value
  const hit = RES.find((r) => r.w === o.width && r.h === o.height)
  return hit?.key ?? 'custom'
})
function setRes(key: string) {
  if (key === 'custom') {
    const s = outputSize(ed.project.value, ed.sources)
    setOut({ width: s.w, height: s.h })
    return
  }
  const r = RES.find((x) => x.key === key)
  if (r) setOut({ width: r.w, height: r.h })
}
function setOut(patch: Partial<EditOutput>) {
  ed.mutate((p) => Object.assign(p.out, patch), `out:${Object.keys(patch).join(',')}`)
}
const FPS = [0, 24, 25, 30, 50, 60]
const fpsOptions = computed(() => (FPS.includes(out.value.fps) ? FPS : [...FPS, out.value.fps]))

async function pickDir() {
  try {
    const r = await open({ directory: true, multiple: false })
    if (r && !Array.isArray(r)) outDir.value = r
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
</script>

<template>
  <div class="insp">
    <el-radio-group v-model="tab" size="small" class="seg">
      <el-radio-button value="item">属性</el-radio-button>
      <el-radio-button value="region">区域</el-radio-button>
      <el-radio-button value="smart">智能</el-radio-button>
      <el-radio-button value="script">文稿</el-radio-button>
      <el-radio-button value="out">输出</el-radio-button>
    </el-radio-group>

    <!-- 片段 -->
    <div v-if="tab === 'item' && clip" class="sec">
      <div class="title">
        <span class="chip">{{ clip.kind === 'image' ? '图片' : '视频' }}</span>
        <span class="ellipsis" :title="clip.path">{{ baseName(clip.path) }}</span>
      </div>
      <div v-if="clip.path in ed.broken" class="alert">
        <span>{{ ed.broken[clip.path] }}</span>
        <el-button size="small" type="primary" @click="relink">重新指定文件…</el-button>
      </div>
      <div v-else-if="src && src.hdr" class="alert soft">
        <span>这个素材是 HDR，剪辑不会自动转成普通画面，导出的颜色会偏灰。</span>
        <el-button v-if="!hdrBusy" size="small" type="primary" @click="convertHdr">转成普通画面并替换</el-button>
        <span v-else class="mute small">正在转换 {{ Math.round(hdrJob?.percent ?? 0) }}%…</span>
      </div>

      <template v-if="clip.kind === 'video'">
        <EditField label="入点">
          <TimeInput :model-value="clip.inMs" :max="clip.outMs - MIN_MS" @update:model-value="(v) => pc({ inMs: v }, 'in')" />
          <el-button size="small" link @click="emit('goto', placed?.startMs ?? 0)">定位</el-button>
        </EditField>
        <EditField label="出点">
          <TimeInput :model-value="clip.outMs" :min="clip.inMs + MIN_MS" :max="clip.freeze ? Number.MAX_SAFE_INTEGER : clipLimit" @update:model-value="(v) => pc({ outMs: v }, 'out')" />
          <span class="mute small">素材共 {{ src?.durationMs ? msText(src.durationMs) : '—' }}</span>
        </EditField>
        <EditField label="成片里长">
          <span class="mono">{{ msText(clipDuration(clip)) }}</span>
          <span v-if="clip.speed !== 1" class="mute small">（{{ clip.speed }}× 速度）</span>
        </EditField>
        <EditSlider v-if="!clip.freeze" label="速度" :model-value="clip.speed" :min="0.25" :max="4" :step="0.05" unit="×" :digits="2" @update:model-value="(v) => pc({ speed: v }, 'spd')" />
        <template v-if="!clip.freeze">
          <EditField label="">
            <el-checkbox :model-value="clip.reverse" @update:model-value="(v: unknown) => pc({ reverse: !!v }, 'rev')">倒放（声音也倒过来）</el-checkbox>
          </EditField>
          <EditField label="变速曲线" hint="把这个片段拆成几段，每段用不同的速度；之后每一段都可以单独调">
            <el-dropdown trigger="click" :disabled="clip.reverse" @command="(id: string) => applyRamp(id)">
              <el-button size="small" :disabled="clip.reverse">选一个曲线…</el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item v-for="r in SPEED_RAMPS" :key="r.id" :command="r.id">{{ r.label }}<span class="mute small">　{{ r.hint }}</span></el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </EditField>
        </template>
        <EditField label="定格" :hint="clip.freeze ? '这一段就是定格的画面，停多久由出点 − 入点决定' : '在播放头处把画面停住几秒（要先把播放头放在这个片段上）'">
          <template v-if="!clip.freeze">
            <el-input-number v-model="holdSec" :min="0.2" :max="30" :step="0.5" :precision="1" size="small" controls-position="right" />
            <el-button size="small" @click="freezeNow">在播放头处定格</el-button>
          </template>
          <span v-else class="mute small">定格片段：没有声音；预览里是静止画面</span>
        </EditField>
        <template v-if="!clip.freeze && (!src || src.hasAudio)">
          <EditSlider label="音量" :model-value="clip.volume" :min="0" :max="4" :step="0.05" unit="%" :scale="100" :disabled="clip.mute" @update:model-value="(v) => pc({ volume: v }, 'vol')" />
          <EditField label="">
            <el-checkbox :model-value="clip.mute" @update:model-value="(v: unknown) => pc({ mute: !!v }, 'mute')">静音这个片段</el-checkbox>
          </EditField>
        </template>
        <div v-else-if="!clip.freeze" class="mute small pad">这个素材没有声音。</div>
      </template>
      <template v-else>
        <EditField label="显示时长">
          <TimeInput :model-value="clip.outMs" :min="MIN_MS" @update:model-value="(v) => pc({ outMs: v }, 'out')" />
        </EditField>
      </template>

      <EditField label="淡入（秒）" hint="画面从黑色淡入，声音同步">
        <el-input-number :model-value="clip.fadeInMs / 1000" :min="0" :max="clipDuration(clip) / 1000" :step="0.1" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => pc({ fadeInMs: Math.round((v ?? 0) * 1000) }, 'fi')" />
      </EditField>
      <EditField label="淡出（秒）" hint="画面淡出到黑色，声音同步">
        <el-input-number :model-value="clip.fadeOutMs / 1000" :min="0" :max="clipDuration(clip) / 1000" :step="0.1" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => pc({ fadeOutMs: Math.round((v ?? 0) * 1000) }, 'fo')" />
      </EditField>

      <EditField label="画面方向">
        <el-radio-group :model-value="clip.rotate" size="small" @update:model-value="(v: string | number | boolean | undefined) => pc({ rotate: Number(v) as EditClip['rotate'] }, 'rot')">
          <el-radio-button :value="0">不转</el-radio-button>
          <el-radio-button :value="90">右转 90°</el-radio-button>
          <el-radio-button :value="180">180°</el-radio-button>
          <el-radio-button :value="270">左转 90°</el-radio-button>
        </el-radio-group>
      </EditField>
      <EditField label="">
        <el-checkbox :model-value="clip.flipH" @update:model-value="(v: unknown) => pc({ flipH: !!v }, 'fh')">左右翻转</el-checkbox>
        <el-checkbox :model-value="clip.flipV" @update:model-value="(v: unknown) => pc({ flipV: !!v }, 'fv')">上下翻转</el-checkbox>
        <el-button v-if="multiClip" size="small" link type="primary" @click="applyAll('turn')">应用到全部片段</el-button>
      </EditField>

      <EditSlider label="亮度" :model-value="clip.brightness" :min="-1" :max="1" :step="0.01" :scale="100" :digits="0" @update:model-value="(v) => pc({ brightness: v }, 'tone')" />
      <EditSlider label="对比度" :model-value="clip.contrast" :min="0" :max="3" :step="0.01" :scale="100" unit="%" @update:model-value="(v) => pc({ contrast: v }, 'tone')" />
      <EditSlider label="饱和度" :model-value="clip.saturation" :min="0" :max="3" :step="0.01" :scale="100" unit="%" @update:model-value="(v) => pc({ saturation: v }, 'tone')" />
      <EditField label="">
        <el-button size="small" link type="primary" :disabled="!toned" @click="resetTone">调色归零</el-button>
        <el-button v-if="multiClip" size="small" link type="primary" @click="applyAll('tone')">应用到全部片段</el-button>
      </EditField>

      <EditField label="位置" hint="叠加轨上的素材可以放在画面的任意位置、任意大小，和主轨同时出现">
        <el-button size="small" @click="moveToOverlay">移到叠加轨（画中画）</el-button>
      </EditField>

      <template v-if="index > 0">
        <EditField label="转场" hint="和上一个片段之间的过渡">
          <el-select :model-value="clip.transition?.kind ?? 'none'" size="small" style="width: 150px" @update:model-value="setTransition">
            <el-option value="none" label="无（直接切换）" />
            <el-option v-for="t in TRANSITIONS" :key="t.id" :value="t.id" :label="t.label" />
          </el-select>
        </EditField>
        <EditSlider v-if="clip.transition" label="转场时长" :model-value="clip.transition.durationMs" :min="MIN_TRANSITION_MS" :max="3000" :step="50" unit=" 秒" :scale="0.001" :digits="2" @update:model-value="setTransitionMs" />
        <div v-if="clip.transition && appliedOverlap < clip.transition.durationMs" class="mute small pad">
          {{ appliedOverlap ? `片段太短，实际转场 ${(appliedOverlap / 1000).toFixed(2)} 秒。` : '片段太短放不下这个转场，会直接切换。' }}
        </div>
        <EditField v-if="clip.transition || hasTransitions" label="">
          <el-button v-if="clip.transition && ed.project.value.clips.length > 2" size="small" link type="primary" @click="applyAll('transition')">应用到所有衔接处</el-button>
          <el-button v-if="hasTransitions" size="small" link @click="clearAllTransitions">去掉所有转场</el-button>
        </EditField>
      </template>
    </div>

    <!-- 叠加素材 -->
    <div v-else-if="tab === 'item' && overlay" class="sec">
      <div class="title">
        <span class="chip">叠加</span>
        <span class="ellipsis" :title="overlay.path">{{ baseName(overlay.path) }}</span>
      </div>
      <div v-if="overlay.path in ed.broken" class="alert">
        <span>{{ ed.broken[overlay.path] }}</span>
        <el-button size="small" type="primary" @click="relink">重新指定文件…</el-button>
      </div>
      <div v-else-if="src && src.hdr" class="alert soft">
        <span>这个叠加素材是 HDR，导出的颜色会偏灰。</span>
        <el-button v-if="!hdrBusy" size="small" type="primary" @click="convertHdr">转成普通画面并替换</el-button>
        <span v-else class="mute small">正在转换 {{ Math.round(hdrJob?.percent ?? 0) }}%…</span>
      </div>
      <div v-else-if="ovHint" class="alert soft">{{ ovHint }}</div>

      <EditField label="叠加轨" hint="轨号越大，越在上面（盖住小的）">
        <el-select :model-value="overlay.track" size="small" style="width: 150px" @update:model-value="(v: number) => po({ track: v }, 'otr')">
          <el-option v-for="n in MAX_OVERLAY_TRACKS" :key="n" :value="n" :label="`叠加 ${n}`" />
        </el-select>
        <el-button size="small" link type="primary" @click="moveToMain">移到主轨</el-button>
      </EditField>
      <EditField label="开始于">
        <TimeInput :model-value="overlay.startMs" :max="MAX_START_MS" @update:model-value="(v) => po({ startMs: v }, 'os')" />
        <el-button size="small" link @click="po({ startMs: Math.round(ed.playhead.value) }, 'os')">取播放位置</el-button>
      </EditField>
      <template v-if="overlay.kind === 'video'">
        <EditField label="入点">
          <TimeInput :model-value="overlay.inMs" :max="overlay.outMs - MIN_MS" @update:model-value="(v) => po({ inMs: v }, 'oin')" />
        </EditField>
        <EditField label="出点">
          <TimeInput :model-value="overlay.outMs" :min="overlay.inMs + MIN_MS" :max="ovMax" @update:model-value="(v) => po({ outMs: v }, 'oout')" />
          <span class="mute small">素材共 {{ src?.durationMs ? msText(src.durationMs) : '—' }}</span>
        </EditField>
        <EditField label="成片里长">
          <span class="mono">{{ msText(overlayDuration(overlay)) }}</span>
          <span v-if="overlay.speed !== 1" class="mute small">（{{ overlay.speed }}× 速度）</span>
        </EditField>
        <EditSlider label="速度" :model-value="overlay.speed" :min="0.25" :max="4" :step="0.05" unit="×" :digits="2" @update:model-value="(v) => po({ speed: v }, 'ospd')" />
        <EditField label="">
          <el-checkbox :model-value="overlay.looped" @update:model-value="(v: unknown) => po({ looped: !!v }, 'oloop')">{{ isGif(overlay.path) ? 'GIF 循环播放（拉长出点就一直重复）' : '素材不够长时重复播放' }}</el-checkbox>
        </EditField>
      </template>
      <EditField v-else label="显示时长">
        <TimeInput :model-value="overlay.outMs" :min="MIN_MS" @update:model-value="(v) => po({ outMs: v }, 'oout')" />
      </EditField>

      <EditSlider label="左右位置" :model-value="eo!.x" :min="-0.5" :max="1.5" :step="0.005" unit="%" :scale="100" hint="素材中心离画面左边的距离。也可以在监视器里直接拖动素材" @update:model-value="(v) => pk({ x: v }, 'ox')" />
      <EditSlider label="上下位置" :model-value="eo!.y" :min="-0.5" :max="1.5" :step="0.005" unit="%" :scale="100" @update:model-value="(v) => pk({ y: v }, 'oy')" />
      <EditSlider label="大小" :model-value="eo!.scale" :min="0.02" :max="3" :step="0.01" unit="%" :scale="100" hint="素材宽度占画面宽度的比例（100% = 和画面一样宽）" @update:model-value="(v) => pk({ scale: v }, 'osc')" />
      <EditSlider label="旋转" :model-value="eo!.rotate" :min="-360" :max="360" :step="1" unit="°" @update:model-value="(v) => pk({ rotate: v }, 'orot')" />
      <EditField label="">
        <el-button size="small" @click="turnOverlay(-90)">左转 90°</el-button>
        <el-button size="small" @click="turnOverlay(90)">右转 90°</el-button>
        <el-button size="small" link type="primary" :disabled="overlay.rotate === 0" @click="po({ rotate: 0 }, 'orot')">归零</el-button>
      </EditField>
      <EditField label="">
        <el-button size="small" @click="fitOverlay('contain')">放进画面</el-button>
        <el-button size="small" @click="fitOverlay('cover')">铺满画面</el-button>
        <el-button size="small" @click="resetOverlayBox">还原</el-button>
      </EditField>
      <EditSlider label="不透明度" :model-value="eo!.opacity" :min="0" :max="1" :step="0.01" unit="%" :scale="100" @update:model-value="(v) => pk({ opacity: v }, 'oop')" />
      <EditField label="">
        <el-checkbox :model-value="overlay.flipH" @update:model-value="(v: unknown) => po({ flipH: !!v }, 'ofh')">左右翻转</el-checkbox>
        <el-checkbox :model-value="overlay.flipV" @update:model-value="(v: unknown) => po({ flipV: !!v }, 'ofv')">上下翻转</el-checkbox>
      </EditField>
      <EditField label="淡入（秒）" hint="从透明逐渐显现，声音同步">
        <el-input-number :model-value="overlay.fadeInMs / 1000" :min="0" :max="overlayDuration(overlay) / 1000" :step="0.1" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => po({ fadeInMs: Math.round((v ?? 0) * 1000) }, 'ofi')" />
      </EditField>
      <EditField label="淡出（秒）" hint="逐渐变透明，声音同步">
        <el-input-number :model-value="overlay.fadeOutMs / 1000" :min="0" :max="overlayDuration(overlay) / 1000" :step="0.1" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => po({ fadeOutMs: Math.round((v ?? 0) * 1000) }, 'ofo')" />
      </EditField>
      <EditField label="入场动画" hint="开始时从画面外滑进来，或由小变大">
        <el-select :model-value="overlay.animIn" size="small" style="width: 130px" @update:model-value="(v: OverlayAnim) => po({ animIn: v }, 'oai')">
          <el-option v-for="a in ANIM_OPTIONS" :key="a.value" :label="a.label" :value="a.value" />
        </el-select>
      </EditField>
      <EditField label="出场动画" hint="结束时滑出画面，或由大变小消失">
        <el-select :model-value="overlay.animOut" size="small" style="width: 130px" @update:model-value="(v: OverlayAnim) => po({ animOut: v }, 'oao')">
          <el-option v-for="a in ANIM_OPTIONS" :key="a.value" :label="a.label" :value="a.value" />
        </el-select>
      </EditField>
      <EditField v-if="overlay.animIn !== 'none' || overlay.animOut !== 'none'" label="动画时长（秒）" hint="入场、出场各用这么久；素材很短时自动缩短">
        <el-input-number :model-value="overlay.animMs / 1000" :min="0.1" :max="3" :step="0.1" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => po({ animMs: Math.round((v ?? 0.5) * 1000) }, 'oam')" />
      </EditField>
      <template v-if="overlay.kind === 'video' && src && src.hasAudio">
        <EditSlider label="音量" :model-value="eo!.volume" :min="0" :max="4" :step="0.05" unit="%" :scale="100" :disabled="overlay.mute" @update:model-value="(v) => pk({ volume: v }, 'ovol')" />
        <EditField label="">
          <el-checkbox :model-value="overlay.mute" @update:model-value="(v: unknown) => po({ mute: !!v }, 'omute')">静音这个素材</el-checkbox>
        </EditField>
      </template>
      <EditField label="抠像" hint="把一种颜色（绿幕 / 蓝幕）变成透明，露出下面的画面。预览是近似的，导出为准">
        <el-checkbox :model-value="overlay.chroma.on" @update:model-value="(v: unknown) => pch({ on: !!v }, 'chon')">去掉背景色</el-checkbox>
      </EditField>
      <template v-if="overlay.chroma.on">
        <EditField label="背景色">
          <el-color-picker :model-value="overlay.chroma.color" size="small" @update:model-value="(v: string | null) => v && pch({ color: v.toUpperCase() }, 'chc')" />
          <el-button v-for="k in KEY_COLORS" :key="k.color" size="small" link :type="overlay.chroma.color === k.color ? 'primary' : undefined" @click="pch({ color: k.color }, 'chc')">{{ k.label }}</el-button>
          <el-button v-if="hasEyeDropper" size="small" link @click="pickKeyColor">吸管</el-button>
        </EditField>
        <EditSlider label="相似度" :model-value="overlay.chroma.similarity" :min="0.01" :max="0.6" :step="0.01" :scale="100" :digits="0" unit="%" @update:model-value="(v) => pch({ similarity: v }, 'chs')" />
        <EditSlider label="边缘柔化" :model-value="overlay.chroma.blend" :min="0" :max="0.5" :step="0.01" :scale="100" :digits="0" unit="%" @update:model-value="(v) => pch({ blend: v }, 'chb')" />
        <EditField label="">
          <el-checkbox :model-value="overlay.chroma.despill" @update:model-value="(v: unknown) => pch({ despill: !!v }, 'chd')">去溢色（压掉边缘的绿 / 蓝反光）</el-checkbox>
        </EditField>
      </template>
      <EditSlider label="亮度" :model-value="overlay.brightness" :min="-1" :max="1" :step="0.01" :scale="100" :digits="0" @update:model-value="(v) => po({ brightness: v }, 'otone')" />
      <EditSlider label="对比度" :model-value="overlay.contrast" :min="0" :max="3" :step="0.01" :scale="100" unit="%" @update:model-value="(v) => po({ contrast: v }, 'otone')" />
      <EditSlider label="饱和度" :model-value="overlay.saturation" :min="0" :max="3" :step="0.01" :scale="100" unit="%" @update:model-value="(v) => po({ saturation: v }, 'otone')" />
      <EditField label="">
        <el-button size="small" link type="primary" :disabled="!ovToned" @click="resetOverlayTone">调色归零</el-button>
      </EditField>
      <el-collapse class="kfc" :model-value="keyedCount(overlay) ? ['k'] : []">
        <el-collapse-item name="k" :title="`关键帧动画${keyedCount(overlay) ? `（${keyedCount(overlay)} 项）` : ''}`">
          <EditKeyframes :ed="ed" kind="overlay" :item="overlay" :defs="OVERLAY_KEY_PROPS" :duration-ms="overlayDuration(overlay)" :start-ms="overlay.startMs" @goto="(ms) => emit('goto', ms)" />
        </el-collapse-item>
      </el-collapse>
      <div class="mute small pad">叠加素材不会撑长成片，超出主轨结尾的部分会被截掉。</div>
    </div>

    <!-- 文字 -->
    <div v-else-if="tab === 'item' && text" class="sec">
      <div class="title"><span class="chip">文字</span></div>
      <el-input :model-value="text.text" type="textarea" :rows="3" resize="none" placeholder="输入文字，可以多行" @update:model-value="(v: string) => pt({ text: v }, 'txt')" />
      <EditField label="出现于">
        <TimeInput :model-value="text.startMs" :max="text.endMs - MIN_MS" @update:model-value="(v) => pt({ startMs: v }, 'ts')" />
        <el-button size="small" link @click="stampStart">取播放位置</el-button>
      </EditField>
      <EditField label="消失于">
        <TimeInput :model-value="text.endMs" :min="text.startMs + MIN_MS" @update:model-value="(v) => pt({ endMs: v }, 'te')" />
        <el-button size="small" link @click="stampEnd">取播放位置</el-button>
      </EditField>
      <EditSlider label="左右位置" :model-value="et!.x" :min="0" :max="1" :step="0.005" unit="%" :scale="100" hint="文字中心离画面左边的距离。也可以在监视器里直接拖动文字" @update:model-value="(v) => ptk({ x: v }, 'tx')" />
      <EditSlider label="上下位置" :model-value="et!.y" :min="0" :max="1" :step="0.005" unit="%" :scale="100" @update:model-value="(v) => ptk({ y: v }, 'ty')" />
      <EditSlider label="字号" :model-value="text.size" :min="1" :max="30" :step="0.5" unit="%" :digits="1" hint="占画面高度的百分比" @update:model-value="(v) => pt({ size: v }, 'tsz')" />
      <EditField label="颜色">
        <el-color-picker :model-value="text.color" size="small" @update:model-value="(v: string | null) => v && pt({ color: v }, 'tc')" />
      </EditField>
      <EditSlider label="不透明度" :model-value="et!.opacity" :min="0" :max="1" :step="0.05" unit="%" :scale="100" @update:model-value="(v) => ptk({ opacity: v }, 'to')" />
      <EditSlider label="淡入淡出" :model-value="text.fadeMs" :min="0" :max="1000" :step="50" :scale="0.001" :digits="2" unit=" 秒" hint="出现和消失时渐变；导出时只作用在字上，描边和底色块不跟着变" @update:model-value="(v) => pt({ fadeMs: v }, 'tfd')" />
      <el-collapse v-if="!(text.words.length && text.highlight)" class="kfc" :model-value="keyedCount(text) ? ['k'] : []">
        <el-collapse-item name="k" :title="`关键帧动画${keyedCount(text) ? `（${keyedCount(text)} 项）` : ''}`">
          <EditKeyframes :ed="ed" kind="text" :item="text" :defs="TEXT_KEY_PROPS" :duration-ms="text.endMs - text.startMs" :start-ms="text.startMs" @goto="(ms) => emit('goto', ms)" />
        </el-collapse-item>
      </el-collapse>
      <div v-if="text.words.length" class="mute small pad">这是按词生成的字幕：{{ text.highlight ? '说到哪个词，哪个词变色' : '每个词都有各自的时间' }}。改字后会变成普通文字。</div>
      <EditField v-if="text.words.length" label="高亮色">
        <el-color-picker :model-value="text.highlight || '#FFD400'" size="small" @update:model-value="(v: string | null) => v && pt({ highlight: v }, 'thl')" />
        <el-button v-if="text.highlight" size="small" link @click="pt({ highlight: '' }, 'thl')">不高亮</el-button>
      </EditField>
      <EditField label="描边">
        <el-checkbox :model-value="text.outline" @update:model-value="(v: unknown) => pt({ outline: !!v }, 'tol')">加描边</el-checkbox>
        <el-color-picker v-if="text.outline" :model-value="text.outlineColor" size="small" @update:model-value="(v: string | null) => v && pt({ outlineColor: v }, 'tolc')" />
      </EditField>
      <EditField label="底色块">
        <el-checkbox :model-value="text.boxed" @update:model-value="(v: unknown) => pt({ boxed: !!v }, 'tb')">文字后面加底色</el-checkbox>
        <el-color-picker v-if="text.boxed" :model-value="text.boxColor" size="small" @update:model-value="(v: string | null) => v && pt({ boxColor: v }, 'tbc')" />
      </EditField>
      <EditSlider v-if="text.boxed" label="底色浓度" :model-value="text.boxOpacity" :min="0.05" :max="1" :step="0.05" unit="%" :scale="100" @update:model-value="(v) => pt({ boxOpacity: v }, 'tbo')" />
      <EditField label="字体">
        <span class="ellipsis small" :title="text.font ?? ''">{{ text.font ? baseName(text.font) : '默认（自动找中文字体）' }}</span>
        <el-button size="small" link type="primary" @click="pickFont">选择字体文件…</el-button>
        <el-button v-if="text.font" size="small" link @click="pt({ font: null }, 'font')">恢复默认</el-button>
      </EditField>
      <div class="mute small pad">导出的文字用选定的字体文件绘制，监视器里用系统字体显示，字形会略有不同。</div>
    </div>

    <!-- 音频 -->
    <div v-else-if="tab === 'item' && audio" class="sec">
      <div class="title">
        <span class="chip">配乐</span>
        <span class="ellipsis" :title="audio.path">{{ baseName(audio.path) }}</span>
      </div>
      <div v-if="audio.path in ed.broken" class="alert">
        <span>{{ ed.broken[audio.path] }}</span>
        <el-button size="small" type="primary" @click="relink">重新指定文件…</el-button>
      </div>
      <EditField label="开始于">
        <TimeInput :model-value="audio.startMs" @update:model-value="(v) => pa({ startMs: v }, 'as')" />
        <el-button size="small" link @click="pa({ startMs: Math.round(ed.playhead.value) }, 'as')">取播放位置</el-button>
      </EditField>
      <EditField label="素材起点">
        <TimeInput :model-value="audio.inMs" :max="(audio.outMs ?? clipLimit) - MIN_MS" @update:model-value="(v) => pa({ inMs: v }, 'ai')" />
      </EditField>
      <EditField label="素材终点">
        <TimeInput :model-value="audio.outMs ?? src?.durationMs ?? 0" :min="audio.inMs + MIN_MS" :max="clipLimit" :disabled="audio.outMs === null" @update:model-value="(v) => pa({ outMs: v }, 'ao')" />
        <el-checkbox :model-value="audio.outMs === null" @update:model-value="audioToEnd">到结尾</el-checkbox>
      </EditField>
      <EditSlider label="音量" :model-value="audio.volume" :min="0" :max="4" :step="0.05" unit="%" :scale="100" @update:model-value="(v) => pa({ volume: v }, 'av')" />
      <EditField label="淡入（秒）">
        <el-input-number :model-value="audio.fadeInMs / 1000" :min="0" :max="60" :step="0.5" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => pa({ fadeInMs: Math.round((v ?? 0) * 1000) }, 'afi')" />
      </EditField>
      <EditField label="淡出（秒）">
        <el-input-number :model-value="audio.fadeOutMs / 1000" :min="0" :max="60" :step="0.5" :precision="1" size="small" controls-position="right" @update:model-value="(v: number | undefined) => pa({ fadeOutMs: Math.round((v ?? 0) * 1000) }, 'afo')" />
      </EditField>
      <EditField label="">
        <el-checkbox :model-value="audio.looped" @update:model-value="(v: unknown) => pa({ looped: !!v }, 'al')">不够长时重复播放，直到视频结束</el-checkbox>
      </EditField>
      <EditField label="">
        <el-checkbox :model-value="audio.duck" @update:model-value="(v: unknown) => pa({ duck: !!v }, 'ad')">片段里有人声时自动压低这条配乐</el-checkbox>
      </EditField>
      <EditField label="节拍" hint="找出配乐的拍子，时间线上显示成小竖线，拖东西时会吸附上去；也可以让主轨按拍子切换">
        <el-button size="small" :loading="beatBusy" @click="findBeats">{{ audio.beats ? '重新分析' : '分析节拍' }}</el-button>
        <span v-if="audio.beats" class="mute small">约 {{ audio.beats.bpm }} BPM · {{ audio.beats.beatsMs.length }} 拍{{ audio.beats.confidence < LOW_BEAT_CONFIDENCE ? '（节奏不清晰，可能不准）' : '' }}</span>
      </EditField>
      <EditField v-if="audio.beats" label="卡点">
        <span class="mute small">每个片段占</span>
        <el-select v-model="perClip" size="small" style="width: 84px">
          <el-option v-for="n in [1, 2, 4, 8]" :key="n" :label="`${n} 拍`" :value="n" />
        </el-select>
        <el-button size="small" @click="arrangeNow">让主轨按节拍切换</el-button>
      </EditField>
      <EditField label="">
        <el-button size="small" @click="fitNow">适配成片长度</el-button>
        <span class="mute small">太短就循环，太长就截断并淡出</span>
      </EditField>
      <div class="mute small pad">监视器里配乐的音量最大 100%，自动压低也不会播放出来，导出时才生效。</div>
    </div>

    <div v-else-if="tab === 'item'" class="sec mute small">
      在时间线上点一个片段、叠加素材、文字或配乐，在这里修改它的属性。<br />
      点时间线空白处可以移动播放位置；拖动片段可以换顺序，拖动两端可以裁剪；叠加素材可以拖到任意时间和轨道。
    </div>

    <!-- 区域 -->
    <div v-if="tab === 'region'" class="sec">
      <EditRegions :ed="ed" :rg="rg" @goto="(ms) => emit('goto', ms)" />
    </div>

    <!-- 智能 -->
    <div v-if="tab === 'smart'" class="sec">
      <EditSmart :ed="ed" @goto="(ms) => emit('goto', ms)" />
    </div>

    <!-- 文稿 -->
    <div v-if="tab === 'script'" class="sec">
      <EditTranscript :ed="ed" @goto="(ms) => emit('goto', ms)" />
    </div>

    <!-- 输出 -->
    <div v-if="tab === 'out'" class="sec">
      <EditField label="文件名">
        <el-input :model-value="ed.project.value.title" size="small" placeholder="留空则用第一个片段的名字" clearable @update:model-value="(v: string) => ed.mutate((p) => (p.title = v), 'title')" />
      </EditField>
      <EditField label="画面尺寸">
        <el-select :model-value="resKey" size="small" style="width: 220px" @update:model-value="setRes">
          <el-option v-for="r in RES" :key="r.key" :value="r.key" :label="r.label" />
          <el-option value="custom" label="自定义…" />
        </el-select>
      </EditField>
      <EditField v-if="resKey === 'custom'" label="宽 × 高">
        <el-input-number :model-value="out.width" :min="64" :max="7680" :step="2" size="small" controls-position="right" @update:model-value="(v: number | undefined) => setOut({ width: v ?? 0 })" />
        <span>×</span>
        <el-input-number :model-value="out.height" :min="64" :max="7680" :step="2" size="small" controls-position="right" @update:model-value="(v: number | undefined) => setOut({ height: v ?? 0 })" />
      </EditField>
      <EditField label="帧率">
        <el-select :model-value="out.fps" size="small" style="width: 160px" @update:model-value="(v: number) => setOut({ fps: v })">
          <el-option v-for="f in fpsOptions" :key="f" :value="f" :label="f ? `${f} 帧/秒` : '跟随第一个视频'" />
        </el-select>
      </EditField>
      <EditField label="画面适配" hint="片段和输出画面比例不同时怎么处理">
        <el-select :model-value="out.fit" size="small" style="width: 220px" @update:model-value="(v: EditOutput['fit']) => setOut({ fit: v })">
          <el-option value="contain" label="完整显示，留黑边" />
          <el-option value="cover" label="铺满画面，裁掉多余部分" />
          <el-option value="blur" label="完整显示，背景用模糊画面" />
        </el-select>
      </EditField>
      <EditField label="画质">
        <el-radio-group :model-value="out.quality" size="small" @update:model-value="(v: string | number | boolean | undefined) => setOut({ quality: v as EditOutput['quality'] })">
          <el-radio-button value="small">体积小</el-radio-button>
          <el-radio-button value="balanced">均衡</el-radio-button>
          <el-radio-button value="high">高画质</el-radio-button>
        </el-radio-group>
      </EditField>
      <EditField label="编码">
        <el-radio-group :model-value="out.codec" size="small" @update:model-value="(v: string | number | boolean | undefined) => setOut({ codec: v as EditOutput['codec'] })">
          <el-radio-button value="h264">H.264（通用）</el-radio-button>
          <el-radio-button value="hevc">H.265（更小）</el-radio-button>
        </el-radio-group>
      </EditField>
      <EditField label="格式">
        <el-radio-group :model-value="out.format" size="small" @update:model-value="(v: string | number | boolean | undefined) => setOut({ format: v as EditOutput['format'] })">
          <el-radio-button value="mp4">MP4</el-radio-button>
          <el-radio-button value="mkv">MKV</el-radio-button>
        </el-radio-group>
      </EditField>
      <EditField label="保存到">
        <span class="ellipsis small" :title="outDir">{{ outDir || '默认（媒体库的下载文件夹）' }}</span>
        <el-button size="small" link type="primary" @click="pickDir">选择文件夹…</el-button>
        <el-button v-if="outDir" size="small" link @click="outDir = ''">恢复默认</el-button>
      </EditField>
    </div>
  </div>
</template>

<style scoped>
.insp {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.seg {
  align-self: flex-start;
}
.sec {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 500;
  min-width: 0;
}
.small {
  font-size: 11.5px;
}
.pad {
  padding-left: 82px;
}
.alert {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--cc-err) 14%, transparent);
  color: var(--cc-err);
  font-size: 12px;
}
.alert.soft {
  background: var(--cc-acc-soft);
  color: var(--cc-fg);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}
.kfc {
  border: none;
  margin: 4px 0;
}
</style>
