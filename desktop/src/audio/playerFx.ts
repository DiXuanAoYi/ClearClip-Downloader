// 清影内置播放器的实时音效：和浏览器扩展共用同一份处理代码（extension/content/audio-logic.js、audio-chain.js）和预设。
// 默认开启响度标准化（温和，-16 LUFS），也可以选其他预设或关闭；另有混响。设置记在本机的 localStorage 里。
import { onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'
import '../../extension/content/audio-logic.js'
import '../../extension/content/audio-chain.js'

interface Preset { id: string; name: string; target: number; note: string }
interface Reverb { id: string; name: string }
interface Cfg { on: boolean }
interface ChainLike {
  wire(o: { cfg: Cfg; bypass?: boolean; measure?: boolean }): void
  tick(): void
  reset(): void
  lufs: number | null
  curDb: number
}
interface AudioApi {
  LOUD_PRESETS: Preset[]
  REVERBS: Reverb[]
  normalizeCfg(raw: unknown): Cfg
  Chain: new (ac: AudioContext, el: HTMLMediaElement) => ChainLike
  fmtLufs(v: number | null): string
  fmtDb(v: number | null): string
}

const A = (globalThis as unknown as { ccAudio: AudioApi }).ccAudio
const KEY = 'cc.player.fx'

export const FX_PRESETS = A.LOUD_PRESETS
export const FX_REVERBS = A.REVERBS

function load(): { preset: string; reverb: string } {
  try {
    const r = JSON.parse(localStorage.getItem(KEY) || '{}')
    const preset = r.preset === 'off' || A.LOUD_PRESETS.some((p) => p.id === r.preset) ? r.preset : 'mild'
    const reverb = A.REVERBS.some((x) => x.id === r.reverb) ? r.reverb : ''
    return { preset, reverb }
  } catch {
    return { preset: 'mild', reverb: '' }
  }
}

/** 给播放器里的 <video> 接上音效。video 换了（换文件）就换一条新的链。 */
export function usePlayerFx(video: Ref<HTMLVideoElement | null>) {
  const init = load()
  const preset = ref(init.preset)
  const reverb = ref(init.reverb)
  const info = ref('')
  let ac: AudioContext | null = null
  let chain: ChainLike | null = null
  let el: HTMLVideoElement | null = null
  let timer: number | undefined

  const cfg = () => A.normalizeCfg({ on: preset.value !== 'off', preset: preset.value === 'off' ? 'mild' : preset.value, reverb: { on: !!reverb.value, id: reverb.value || 'room' } })
  const wanted = () => preset.value !== 'off' || !!reverb.value

  function wire() {
    if (!chain) return
    const c = cfg()
    chain.wire({ cfg: c, bypass: !wanted(), measure: true })
  }

  function dispose() {
    chain = null
    el = null
    info.value = ''
    void ac?.close().catch(() => {}) // 忽略：媒体元素/音频上下文已释放或被浏览器拦截，可忽略
    ac = null
  }

  /** 播放时调用：第一次需要时才接入（接进去之后撤不回来，所以不需要音效就不接）。 */
  async function attach() {
    const v = video.value
    if (!v || chain || !wanted()) return
    if ((v as HTMLVideoElement & { mediaKeys?: unknown }).mediaKeys) return
    try {
      const ctx = new AudioContext({ latencyHint: 'playback' })
      await Promise.race([ctx.resume().catch(() => {}), new Promise((r) => setTimeout(r, 300))]) // 忽略：媒体元素/音频上下文已释放或被浏览器拦截，可忽略
      if (ctx.state !== 'running') {
        void ctx.close()
        return // 等下一次播放（需要用户操作过）
      }
      ac = ctx
      el = v
      chain = new A.Chain(ctx, v)
      wire()
    } catch {
      dispose()
    }
  }

  watch([preset, reverb], () => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ preset: preset.value, reverb: reverb.value }))
    } catch {
      // 记不住就算了
    }
    if (chain) wire()
    else if (video.value && !video.value.paused) void attach()
  })
  watch(video, (v) => {
    if (v !== el) dispose()
  })

  onMounted(() => {
    timer = window.setInterval(() => {
      const v = video.value
      if (!chain || !v) return
      if (!v.paused && !v.ended) chain.tick()
      if (!wanted()) return void (info.value = '')
      const out = chain.lufs == null ? null : chain.lufs + chain.curDb
      info.value = chain.lufs == null ? '' : preset.value === 'off' ? '' : `${A.fmtLufs(out)} LUFS · ${A.fmtDb(chain.curDb)}`
    }, 100)
  })
  onBeforeUnmount(() => {
    window.clearInterval(timer)
    dispose()
  })

  return { preset, reverb, info, attach, reset: () => chain?.reset() }
}
