<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { PlayerSource, PlayerTrack } from '../types'
import { formatClock } from '../utils/time'

const app = useAppStore()
const src = ref<PlayerSource | null>(null)
const error = ref('')
const failed = ref(false)
const failWhy = ref('')
const trackIndex = ref(-1)
const speed = ref(1)
const now = ref(0)
const video = ref<HTMLVideoElement | null>(null)
const listEl = ref<HTMLElement | null>(null)

const open = computed(() => app.player !== null)
const mediaUrl = computed(() => src.value?.url ?? '')
const track = computed<PlayerTrack | null>(() => (src.value && trackIndex.value >= 0 ? src.value.tracks[trackIndex.value] : null))
const subtitleText = computed(() => (currentCue.value >= 0 && track.value ? track.value.cues[currentCue.value].text : ''))
const currentCue = computed(() => {
  const t = track.value
  if (!t) return -1
  const ms = now.value * 1000
  return t.cues.findIndex((c) => ms >= c.startMs && ms < c.endMs)
})

watch(
  () => app.player,
  async (p) => {
    src.value = null
    error.value = ''
    failed.value = false
    failWhy.value = ''
    trackIndex.value = -1
    now.value = 0
    speed.value = 1
    if (!p) return
    try {
      const s = await api.playerSource(p.path)
      src.value = s
      trackIndex.value = s.tracks.length ? 0 : -1
    } catch (e) {
      error.value = errorText(e)
    }
  },
  { immediate: true },
)

const MEDIA_ERR: Record<number, string> = { 1: '加载被中止', 2: '读取文件失败', 3: '解码失败', 4: '格式或编码不支持' }

function onVideoError() {
  failed.value = true
  const e = video.value?.error
  failWhy.value = e ? `${MEDIA_ERR[e.code] ?? '未知错误'}（错误码 ${e.code}${e.message ? '：' + e.message : ''}）` : ''
}

function onMeta() {
  const v = video.value
  const p = app.player
  if (v && p?.startMs) v.currentTime = p.startMs / 1000
  if (v) {
    v.playbackRate = speed.value
    void v.play().catch(() => {})
  }
}

watch(speed, (s) => video.value && (video.value.playbackRate = s))

function seek(ms: number) {
  if (video.value) {
    video.value.currentTime = ms / 1000
    void video.value.play().catch(() => {})
  }
}

watch(currentCue, async (i) => {
  if (i < 0 || !listEl.value) return
  await nextTick()
  const el = listEl.value.querySelector<HTMLElement>(`[data-i="${i}"]`)
  el?.scrollIntoView({ block: 'nearest' })
})

function close() {
  app.player = null
}

function toToolbox() {
  const path = src.value?.path
  close()
  if (path) app.goToolbox([path])
}

function onKey(e: KeyboardEvent) {
  if (!open.value) return
  const v = video.value
  if (e.key === 'Escape') close()
  else if (!v || (e.target instanceof HTMLInputElement)) return
  else if (e.key === ' ') {
    e.preventDefault()
    if (v.paused) void v.play()
    else v.pause()
  } else if (e.key === 'ArrowLeft') v.currentTime = Math.max(0, v.currentTime - 5)
  else if (e.key === 'ArrowRight') v.currentTime += 5
}
// 字幕由我们自己叠加显示（各系统的原生字幕轨渲染不一致），播放时每 100 毫秒对一次时间
let tick: number | undefined
onMounted(() => {
  window.addEventListener('keydown', onKey)
  tick = window.setInterval(() => {
    if (video.value && !video.value.paused) now.value = video.value.currentTime
  }, 100)
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  window.clearInterval(tick)
})

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2]
</script>

<template>
  <div v-if="open" class="mask" @mousedown.self="close">
    <div class="pl card" role="dialog" aria-label="播放器">
      <div class="top">
        <span class="ttl ellipsis" :title="src?.title">{{ src?.title ?? '播放器' }}</span>
        <el-select v-model="speed" size="small" class="spd">
          <el-option v-for="s in SPEEDS" :key="s" :label="`${s}×`" :value="s" />
        </el-select>
        <el-select v-if="src?.tracks.length" v-model="trackIndex" size="small" class="trk">
          <el-option :value="-1" label="不显示字幕" />
          <el-option v-for="(t, i) in src.tracks" :key="t.path" :value="i" :label="t.label" />
        </el-select>
        <el-button size="small" @click="close">关闭</el-button>
      </div>

      <div v-if="error" class="err">
        <p class="selectable">{{ error }}</p>
        <el-button v-if="app.player" size="small" @click="api.openFile(app.player.path)">用系统播放器打开</el-button>
      </div>

      <div v-else-if="src" class="stage">
        <div class="vwrap">
          <video
            v-if="src.kind === 'video' || src.kind === 'audio'"
            ref="video"
            :key="src.path"
            class="vid"
            :class="{ audio: src.kind === 'audio' }"
            :src="mediaUrl"
            controls
            preload="metadata"
            @loadedmetadata="onMeta"
            @timeupdate="now = video?.currentTime ?? 0"
            @error="onVideoError"
          >
          </video>
          <div v-if="subtitleText" class="sub" :class="{ audio: src.kind === 'audio' }">{{ subtitleText }}</div>
          <div v-if="failed" class="failed">
            <p>播放器打不开这个文件（常见于 mkv / flv / HEVC / 部分 ts，或系统缺少对应的解码器）。</p>
            <p v-if="failWhy" class="mono why">{{ failWhy }}</p>
            <div>
              <el-button size="small" type="primary" @click="api.openFile(src.path)">用系统播放器打开</el-button>
              <el-button size="small" @click="toToolbox">用工具箱转成 MP4</el-button>
            </div>
          </div>
        </div>

        <div v-if="track" ref="listEl" class="cues">
          <div v-for="(c, i) in track.cues" :key="i" :data-i="i" class="cue" :class="{ on: i === currentCue }" @click="seek(c.startMs)">
            <span class="mono tm">{{ formatClock(c.startMs) }}</span>
            <span class="tx">{{ c.text }}</span>
          </div>
        </div>
      </div>
      <p v-if="src && !src.tracks.length && !error" class="mute small">没有找到同名的字幕文件（SRT / VTT / ASS）。弹幕请在下载时选择“烧录进画面”。</p>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 3200;
  background: rgba(0, 0, 0, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
}
.pl {
  width: min(1040px, 94vw);
  max-height: 92vh;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.top {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ttl {
  flex: 1;
  font-weight: 600;
  min-width: 0;
}
.spd {
  width: 84px;
}
.trk {
  width: 170px;
}
.stage {
  display: flex;
  gap: 10px;
  min-height: 0;
}
.vwrap {
  flex: 1;
  min-width: 0;
  position: relative;
  background: #000;
  border-radius: 8px;
  overflow: hidden;
}
.vid {
  width: 100%;
  max-height: 68vh;
  display: block;
}
.vid.audio {
  height: 54px;
}
.sub {
  position: absolute;
  left: 8%;
  right: 8%;
  bottom: 62px;
  text-align: center;
  color: #fff;
  font-size: 18px;
  line-height: 1.4;
  white-space: pre-line;
  text-shadow: 0 0 3px #000, 0 1px 3px #000, 0 0 8px #000;
  pointer-events: none;
}
.sub.audio {
  position: static;
  background: #000;
  padding: 8px;
}
.failed {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.82);
  color: #fff;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 16px;
  text-align: center;
  font-size: 13px;
}
.failed p {
  margin: 0;
}
.why {
  font-size: 11px;
  opacity: 0.7;
}
.cues {
  flex: 0 0 280px;
  max-height: 68vh;
  overflow-y: auto;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 4px;
}
.cue {
  display: flex;
  gap: 8px;
  padding: 5px 8px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 12.5px;
  line-height: 1.5;
}
.cue:hover {
  background: var(--cc-side);
}
.cue.on {
  background: var(--cc-acc-soft);
}
.tm {
  color: var(--cc-mute);
  flex: 0 0 auto;
  font-size: 11.5px;
}
.tx {
  white-space: pre-line;
}
.err {
  padding: 18px;
}
.small {
  font-size: 11.5px;
  margin: 0;
}
</style>
