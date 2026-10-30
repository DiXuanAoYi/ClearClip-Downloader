<script setup lang="ts">
// 实时画面：把主轨的片段（含转场）、叠加轨的素材和配乐直接叠在一起播放，不用先生成预览。
// 每个素材一个 <video> / <img>，只保留正在用和马上要用的；位置、旋转、缩放、转场用 CSS 摆出来，是对导出效果的近似，
// 导出时仍然由 ffmpeg 渲染。播放位置由监视器给（`sync`），这里只负责让每个元素跟上。
import { convertFileSrc } from '@tauri-apps/api/core'
import { ElMessage } from 'element-plus'
import { computed, nextTick, onBeforeUnmount, reactive, watch } from 'vue'
import { errorText } from '../../../api'
import type { EditApi } from '../../../composables/useEditProject'
import type { EditClip, EditOverlay } from '../../../types'
import { audioSpan, clamp, isGif, liveFrame, mainGain, overlayAnimPose, overlayBox, overlayEnd, overlayGain, transitionLook } from '../../../utils/edit'
import type { LiveMain, LiveOverlay, TransitionLook } from '../../../utils/edit'

const props = defineProps<{
  ed: EditApi
  stage: { w: number; h: number }
  fit: 'contain' | 'cover' | 'blur'
  /** 这一段画面要放大到“聚焦窗口”（跟随聚焦的近似效果）：片段 id 和整层的 transform */
  zoom?: { clip: number; css: string } | null
}>()
const ed = props.ed

/** 往后多久内要用到的素材先装好，往前多久内的还留着（拖回去不用重新加载） */
const AHEAD = 3000
const BEHIND = 500
const MAX_PRELOAD = 10
/** 视频和时间对不上超过这么久（秒）就重新定位 */
const DRIFT = 0.3

interface Item {
  key: string
  main: boolean
  id: number
  path: string
  url: string
  /** 用 <img> 显示（图片、GIF），其余用 <video> */
  still: boolean
  gif: boolean
  inMs: number
  clip?: EditClip
  overlay?: EditOverlay
}

const srcDur = (path: string) => ed.sources[path]?.durationMs
const frame = computed(() => liveFrame(ed.project.value, ed.placed.value, ed.playhead.value, srcDur))
const mains = computed(() => new Map(frame.value.main.map((m) => ['m' + m.clip.id, m] as [string, LiveMain])))
const ovs = computed(() => new Map(frame.value.overlays.map((l) => ['o' + l.overlay.id, l] as [string, LiveOverlay])))
const look = computed<TransitionLook>(() => {
  const m = frame.value.main.find((x) => x.role !== 'solo')
  return m ? transitionLook(m.transition, m.p) : { a: {}, b: {}, backdrop: null }
})

/** 现在要放在舞台上的素材：看得见的，加上马上要用到的（按工程里的顺序排，元素不会因为别的素材出入而挪位置）。 */
const items = computed<Item[]>(() => {
  const p = ed.project.value
  const pl = ed.placed.value
  const t = ed.playhead.value
  const all: { it: Item; from: number; to: number }[] = []
  const add = (main: boolean, id: number, path: string, kind: 'video' | 'image', inMs: number, from: number, to: number, clip?: EditClip, overlay?: EditOverlay) => {
    const s = ed.sources[path]
    if (!s) return
    const gif = isGif(path)
    all.push({ it: { key: (main ? 'm' : 'o') + id, main, id, path, url: (kind === 'video' && ed.proxies[path]) || s.url, still: kind === 'image' || gif, gif, inMs, clip, overlay }, from, to })
  }
  p.clips.forEach((c, i) => pl[i] && add(true, c.id, c.path, c.kind, c.inMs, pl[i].startMs, pl[i].endMs, c))
  for (const o of p.overlays) add(false, o.id, o.path, o.kind, o.inMs, o.startMs, overlayEnd(o), undefined, o)
  const shown = new Set<string>([...mains.value.keys(), ...ovs.value.keys()])
  const near = all
    .filter((x) => !shown.has(x.it.key) && x.to > t - BEHIND && x.from < t + AHEAD)
    .sort((a, b) => Math.abs(a.from - t) - Math.abs(b.from - t))
    .slice(0, MAX_PRELOAD)
  const keep = new Set([...shown, ...near.map((x) => x.it.key)])
  return all.filter((x) => keep.has(x.it.key)).map((x) => x.it)
})

const isLive = (it: Item) => (it.main ? mains.value.has(it.key) : ovs.value.has(it.key))

// ---------- 元素 ----------

const els = new Map<string, HTMLVideoElement>()
const aEls = new Map<number, HTMLAudioElement>()
/** 还没开始用的视频停在入点等着，免得切过去时才去定位 */
const parked = new Set<string>()
/** 刚开始播放的时刻：头几百毫秒视频还没跑起来，不按“时间对不上”重新定位 */
const started = new Map<string, number>()
const gifOn = new Map<string, boolean>()
const epoch = reactive<Record<string, number>>({})
const failed = reactive<Record<string, boolean>>({})

function setEl(key: string, el: unknown) {
  if (el) els.set(key, el as HTMLVideoElement)
  else els.delete(key)
}
function setAudio(id: number, el: unknown) {
  if (el) aEls.set(id, el as HTMLAudioElement)
  else aEls.delete(id)
}
const isFailed = (it: Item) => !!failed[it.url]
const onError = (it: Item) => {
  if (it.url) failed[it.url] = true
}
const thumbOf = (it: Item) => {
  const t = ed.sources[it.path]?.thumb
  return t ? convertFileSrc(t) : ''
}
/** <img> 显示什么：图片本身；GIF 播放时是动图（每次重新开始），停下时是一张静帧；解不了的用缩略图代替 */
function stillSrc(it: Item): string | undefined {
  if (isFailed(it)) return thumbOf(it) || undefined
  if (it.gif) return isLive(it) && ed.playing.value ? `${it.url}?r=${epoch[it.key] ?? 0}` : thumbOf(it) || it.url
  return it.url
}
const problem = computed(() => items.value.some((it) => isLive(it) && isFailed(it)))
const stuck = computed(() => [...new Set(items.value.filter((it) => !it.still && isFailed(it) && !ed.proxies[it.path]).map((it) => it.path))])
const proxyBusy = computed(() => stuck.value.some((p) => ed.proxying[p]))
async function makeProxies() {
  for (const p of stuck.value) {
    try {
      await ed.makeProxy(p)
    } catch (e) {
      ElMessage.error(errorText(e))
    }
  }
}

// ---------- 摆放 ----------

function layerStyle(it: Item): Record<string, string> {
  if (it.main) {
    const m = mains.value.get(it.key)
    if (!m) return {}
    const fx = m.role === 'out' ? look.value.a : m.role === 'in' ? look.value.b : {}
    return { zIndex: m.role === 'in' ? '2' : '1', ...fx }
  }
  const l = ovs.value.get(it.key)
  if (!l) return {}
  const st: Record<string, string> = { zIndex: String(10 + l.rank), opacity: String(clamp(l.overlay.opacity * l.fade, 0, 1)) }
  if (l.overlay.animIn !== 'none' || l.overlay.animOut !== 'none') {
    const s = ed.sources[it.path]
    const b = overlayBox(l.overlay, s ? { w: s.width, h: s.height } : undefined, props.stage)
    const a = overlayAnimPose(l, b, props.stage)
    st.transformOrigin = `${b.cx}px ${b.cy}px`
    st.transform = `translate(${a.dx.toFixed(2)}px, ${a.dy.toFixed(2)}px) scale(${a.zoom.toFixed(4)})`
  }
  return st
}

const zoomStyle = (it: Item): Record<string, string> => (it.main && props.zoom && props.zoom.clip === it.id ? { transform: props.zoom.css, transformOrigin: '0 0' } : {})

function mediaStyle(it: Item): Record<string, string> {
  const { w, h } = props.stage
  const f: string[] = []
  const grade = (c: { brightness: number; contrast: number; saturation: number }) => {
    if (Math.abs(c.brightness) > 1e-6) f.push(`brightness(${(1 + c.brightness).toFixed(3)})`)
    if (Math.abs(c.contrast - 1) > 1e-6) f.push(`contrast(${c.contrast.toFixed(3)})`)
    if (Math.abs(c.saturation - 1) > 1e-6) f.push(`saturate(${c.saturation.toFixed(3)})`)
  }
  if (it.main && it.clip) {
    const c = it.clip
    const turned = c.rotate === 90 || c.rotate === 270
    const bw = turned ? h : w
    const bh = turned ? w : h
    grade(c)
    // 片段自己的淡入淡出是淡到黑色（不是变透明），转场时也一样
    const fade = mains.value.get(it.key)?.fade ?? 1
    if (fade < 0.999) f.push(`brightness(${fade.toFixed(3)})`)
    return {
      position: 'absolute',
      left: `${(w - bw) / 2}px`,
      top: `${(h - bh) / 2}px`,
      width: `${bw}px`,
      height: `${bh}px`,
      objectFit: props.fit === 'cover' ? 'cover' : 'contain',
      transform: `scale(${c.flipH ? -1 : 1}, ${c.flipV ? -1 : 1}) rotate(${c.rotate}deg)`,
      filter: f.join(' ') || 'none',
    }
  }
  const o = it.overlay as EditOverlay
  const s = ed.sources[it.path]
  const b = overlayBox(o, s ? { w: s.width, h: s.height } : undefined, props.stage)
  grade(o)
  return {
    position: 'absolute',
    left: `${b.cx - b.ew / 2}px`,
    top: `${b.cy - b.eh / 2}px`,
    width: `${b.ew}px`,
    height: `${b.eh}px`,
    objectFit: 'fill',
    transform: b.transform,
    filter: f.join(' ') || 'none',
  }
}

// ---------- 播放 ----------

function lookup(t: number) {
  const fr = liveFrame(ed.project.value, ed.placed.value, t, srcDur)
  return { m: new Map(fr.main.map((x) => ['m' + x.clip.id, x] as [string, LiveMain])), o: new Map(fr.overlays.map((x) => ['o' + x.overlay.id, x] as [string, LiveOverlay])) }
}

function drive(it: Item, el: HTMLVideoElement, l: LiveMain | LiveOverlay | undefined, playing: boolean, seek: boolean) {
  if (!l) {
    // 还没轮到（或已经过了）：停着，停在入点
    if (!el.paused) el.pause()
    if (!parked.has(it.key) && el.readyState >= 1) {
      el.currentTime = it.inMs / 1000
      parked.add(it.key)
    }
    return
  }
  parked.delete(it.key)
  const main = 'clip' in l
  const gain = main ? mainGain(l) : overlayGain(l)
  const rate = clamp(main ? l.clip.speed : l.overlay.speed, 0.25, 4)
  const want = l.srcMs / 1000
  if (main && (l.clip.freeze || l.clip.reverse)) {
    // 定格和倒放没法让浏览器直接播：停住，随时间线跳到该在的那一帧（倒放时没有声音）
    el.muted = true
    if (!el.paused) el.pause()
    if (el.readyState >= 1 && !el.seeking && Math.abs(el.currentTime - want) > 0.03) el.currentTime = want
    return
  }
  if (el.playbackRate !== rate) el.playbackRate = rate
  el.volume = clamp(gain, 0, 1)
  el.muted = !playing || gain <= 0
  if (el.readyState < 1) return
  const settling = performance.now() - (started.get(it.key) ?? 0) < 700
  if (seek || (!el.seeking && !settling && Math.abs(el.currentTime - want) > DRIFT)) el.currentTime = want
  if (playing) {
    if (el.paused) {
      started.set(it.key, performance.now())
      el.play().catch(() => undefined)
    }
  } else if (!el.paused) el.pause()
}

/** 时间线走到 t：让每个视频停在 / 播在该在的位置，GIF 重新开始，配乐跟上。`seek` 为真表示这是跳转（拖动、点击），不是连续播放。 */
function sync(t: number, seek: boolean) {
  const { m, o } = lookup(t)
  const playing = ed.playing.value
  for (const it of items.value) {
    const l = it.main ? m.get(it.key) : o.get(it.key)
    if (it.still) {
      const on = !!l && playing
      if (it.gif && on && (!gifOn.get(it.key) || seek)) epoch[it.key] = (epoch[it.key] ?? 0) + 1
      gifOn.set(it.key, on)
      continue
    }
    const el = els.get(it.key)
    if (el && !isFailed(it)) drive(it, el, l, playing, seek)
  }
  syncMusic(t, playing)
}

function onMeta(it: Item) {
  const el = els.get(it.key)
  if (!el) return
  parked.delete(it.key)
  const t = ed.playhead.value
  const { m, o } = lookup(t)
  drive(it, el, it.main ? m.get(it.key) : o.get(it.key), ed.playing.value, true)
}

function syncMusic(t: number, playing: boolean) {
  const end = ed.total.value
  for (const a of ed.project.value.audio) {
    const el = aEls.get(a.id)
    const src = ed.sources[a.path]
    if (!el || !src) continue
    const span = audioSpan(a, src, end)
    const active = playing && t >= a.startMs && t < a.startMs + span
    if (!active) {
      if (!el.paused) el.pause()
      continue
    }
    const seg = a.outMs !== null ? a.outMs - a.inMs : Math.max(1, (src.durationMs ?? 1) - a.inMs)
    const local = t - a.startMs
    const want = (a.inMs + (a.looped ? local % Math.max(1, seg) : local)) / 1000
    let f = 1
    if (a.fadeInMs > 0) f = Math.min(f, local / a.fadeInMs)
    if (a.fadeOutMs > 0) f = Math.min(f, (span - local) / a.fadeOutMs)
    el.volume = clamp(a.volume * clamp(f, 0, 1), 0, 1)
    if (Math.abs(el.currentTime - want) > DRIFT) el.currentTime = want
    if (el.paused) el.play().catch(() => undefined)
  }
}

function pause() {
  for (const el of els.values()) if (!el.paused) el.pause()
  for (const el of aEls.values()) if (!el.paused) el.pause()
  gifOn.clear()
}

// 有新素材装上来时，让它们尽快到位（停在入点，或者追上现在的位置）
watch(
  () => items.value.map((i) => i.key).join('|'),
  () => nextTick(() => sync(ed.playhead.value, false)),
  { flush: 'post' },
)
onBeforeUnmount(pause)
defineExpose({ sync, pause })
</script>

<template>
  <div class="live" :style="{ background: look.backdrop ?? 'transparent' }">
    <div v-for="it in items" :key="it.key" class="lay" :class="{ off: !isLive(it) }" :data-key="it.key" :style="layerStyle(it)">
      <div class="zoom" :style="zoomStyle(it)">
        <video
          v-if="!it.still && !isFailed(it)"
          :ref="(el) => setEl(it.key, el)"
          :src="it.url"
          :style="mediaStyle(it)"
          muted
          preload="auto"
          playsinline
          @loadedmetadata="onMeta(it)"
          @error="onError(it)"
        />
        <img v-else-if="stillSrc(it)" :src="stillSrc(it)" :style="mediaStyle(it)" alt="" draggable="false" @error="onError(it)" />
      </div>
    </div>
    <div class="fxslot"><slot name="fx" /></div>
    <audio v-for="a in ed.project.value.audio" :key="a.id" :ref="(el) => setAudio(a.id, el)" :src="ed.sources[a.path]?.url" preload="auto" />
    <div v-if="problem" class="warn">
      有素材在实时画面里播不出来（系统不支持它的编码），先用静态画面代替；导出不受影响。
      <button v-if="stuck.length" class="lnk" :disabled="proxyBusy" @click="makeProxies">{{ proxyBusy ? '正在生成…' : '生成预览用的小文件' }}</button>
    </div>
  </div>
</template>

<style scoped>
.live {
  position: absolute;
  inset: 0;
  z-index: 0;
  overflow: hidden;
  pointer-events: none;
}
.lay {
  position: absolute;
  inset: 0;
}
.lay.off {
  visibility: hidden;
}
.zoom {
  position: absolute;
  inset: 0;
}
.lay :is(video, img) {
  background: transparent;
  max-width: none;
  user-select: none;
}
.fxslot {
  position: absolute;
  inset: 0;
  z-index: 5;
  pointer-events: none;
}
audio {
  display: none;
}
.warn {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 8px;
  z-index: 50;
  text-align: center;
  font-size: 12px;
  padding: 0 12px;
  color: #ffd27a;
  text-shadow: 0 1px 2px #000;
}
.warn .lnk {
  pointer-events: auto;
  border: 0;
  background: none;
  padding: 0;
  margin-left: 6px;
  font: inherit;
  color: #fff;
  text-decoration: underline;
  cursor: pointer;
}
.warn .lnk:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
