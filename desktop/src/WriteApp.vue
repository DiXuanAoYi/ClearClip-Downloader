<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText } from './api'
import { useLanguage } from './i18n/useLanguage'
import type { WritePending, WritingSettings } from './types'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ACTIONS, PRESETS, buildWriteRequest, presetById, type WriteAction } from './writePresets'

/** 写作窗（桌面端）：快捷键取到文字后弹出，选动作、看结果、替换回原来的程序。 */
useLanguage()
const text = ref('')
const note = ref('')
const src = ref<WritePending | null>(null)
const action = ref<WriteAction>(ACTIONS[0])
const presetId = ref('')
const extra = ref('')
const target = ref('English')
const busy = ref(false)
const progress = ref('')
const error = ref('')
type Guard = { missing: string[]; ratio: number; warn: boolean }
const outs = ref<{ text: string; guard?: Guard | null }[]>([])
const cfg = ref<WritingSettings | null>(null)
const pin = ref(true)
const autoClose = ref(false)
const extras = ref<string[]>([])
const showMore = ref(false)
const compare = ref(false)
const favs = ref<Set<string>>(new Set())
const canUndo = ref(false)
let undoTimer: ReturnType<typeof setTimeout> | undefined
const MAIN_IDS = ['rewrite', 'reply', 'grammar', 'optimize', 'concise']
const picked = ref(0)
const issues = ref<{ from: string; to: string; reason: string }[]>([])
const done = ref('')
const recording = ref(false)
const isReply = computed(() => action.value.id === 'reply')
let jobId = 0
let rec: MediaRecorder | null = null
let chunks: Blob[] = []
let unlisten: UnlistenFn | undefined

const out = computed(() => outs.value[picked.value]?.text ?? '')
const mainActions = computed(() => ACTIONS.filter((a) => MAIN_IDS.includes(a.id) || a.id === action.value.id))
const moreActions = computed(() => ACTIONS.filter((a) => !mainActions.value.includes(a)))
const hotkey = computed(() => cfg.value?.hotkey || '')
/** 把检查结果说成人话；续写、回复不提示长度。 */
const guardText = computed(() => {
  const g = outs.value[picked.value]?.guard
  if (!g?.warn) return ''
  const parts: string[] = []
  if (g.missing?.length) parts.push(`原文里的「${g.missing.slice(0, 4).join('」「')}」在结果里不见了，替换前请核对`)
  else if (!['continue', 'reply'].includes(action.value.id)) parts.push(g.ratio < 1 ? '结果比原文短了很多，可能漏掉内容' : '结果比原文长了很多，可能多出内容')
  return parts.join('；')
})
const faved = computed(() => favs.value.has(out.value))
const hasText = computed(() => !!text.value.trim())
const errHint = computed(() => /模型|key|密钥|未配置|没有可用|设置|api/i.test(error.value))
const charCount = computed(() => text.value.replace(/\s/g, '').length)
const outCount = computed(() => out.value.replace(/\s/g, '').length)
const delta = computed(() => outCount.value - charCount.value)
const hist = ref<string[]>([])
const media = window.matchMedia('(prefers-color-scheme: dark)')
let themeMode = 'system'
/** 配色跟随清影自己的主题设置（不是系统）：设置里选了浅色就一直是浅色。 */
function applyTheme() {
  document.documentElement.classList.toggle('dark', themeMode === 'dark' || (themeMode === 'system' && media.matches))
}
async function pasteIn() {
  try {
    const t = await navigator.clipboard.readText()
    if (t) text.value = t
  } catch {
    error.value = '读不到剪贴板，请直接按 Ctrl+V 粘贴'
  }
}
const canRun = computed(() => !!text.value.trim() && !busy.value)

async function load() {
  const p = await api.writeTake()
  src.value = p
  text.value = p?.text ?? ''
  // 取到文字就不再显示“没取到”的提示
  note.value = p?.text?.trim() ? '' : (p?.note ?? '')
  compare.value = false
  canUndo.value = false
  clearTimeout(undoTimer)
  outs.value = []
  issues.value = []
  error.value = ''
  done.value = ''
}

function saveExtra() {
  const e = extra.value.trim()
  if (e) extras.value = [e, ...extras.value.filter((x) => x !== e)].slice(0, 8)
  void api.writePrefs(action.value.id, extras.value, autoClose.value).catch(() => {})
}

async function run() {
  if (!canRun.value) return
  saveExtra()
  busy.value = true
  error.value = ''
  outs.value = []
  issues.value = []
  progress.value = '正在处理…'
  try {
    jobId = await api.writeRun(buildWriteRequest({ action: action.value, text: text.value, preset: presetById(presetId.value), extra: extra.value, target: target.value, app: src.value?.app }))
    for (;;) {
      await new Promise((r) => setTimeout(r, 450))
      const j = await api.writeJob(jobId)
      if (!j) throw new Error('任务已过期')
      if (j.note) progress.value = j.note
      if (j.state === 'error') throw new Error(j.error ?? '处理失败')
      if (j.state === 'done') {
        const r = j.result ?? {}
        if (text.value.trim()) hist.value = [text.value, ...hist.value.filter((x) => x !== text.value)].slice(0, 5)
        if (r.candidates?.length) outs.value = r.candidates
        else if (r.corrected != null) {
          issues.value = r.issues ?? []
          outs.value = [{ text: r.corrected }]
        }
        if (!outs.value.length) error.value = '没有得到结果'
        picked.value = 0
        break
      }
    }
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
    progress.value = ''
  }
}

/** 听写：录一段话，交给清影里设置好的语音转文字（本机 whisper.cpp 或云端接口），文字追加到上面的输入框。 */
async function dictate() {
  if (recording.value) {
    rec?.stop()
    return
  }
  error.value = ''
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    chunks = []
    rec = new MediaRecorder(stream)
    rec.ondataavailable = (e) => chunks.push(e.data)
    rec.onstop = async () => {
      stream.getTracks().forEach((t) => t.stop())
      recording.value = false
      const blob = new Blob(chunks, { type: rec?.mimeType || 'audio/webm' })
      busy.value = true
      progress.value = '正在转成文字…'
      try {
        const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()))
        const id = await api.writeDictate(bytes, blob.type.includes('ogg') ? 'ogg' : 'webm')
        for (;;) {
          await new Promise((r) => setTimeout(r, 500))
          const j = await api.writeJob(id)
          if (!j) throw new Error('任务已过期')
          if (j.state === 'error') throw new Error(j.error ?? '听写失败')
          if (j.state === 'done') {
            const t = j.result?.text ?? ''
            if (!t) throw new Error('没有听清，请再说一次')
            text.value = (text.value ? text.value + '\n' : '') + t
            break
          }
        }
      } catch (e) {
        error.value = errorText(e)
      } finally {
        busy.value = false
        progress.value = ''
      }
    }
    rec.start()
    recording.value = true
  } catch (e) {
    error.value = '无法使用麦克风：' + errorText(e)
  }
}

async function stop() {
  if (jobId) await api.writeCancel(jobId)
}

async function apply(replace: boolean) {
  if (!out.value) return
  try {
    done.value = await api.writeApply(out.value, replace)
    if (replace && done.value === '已替换') {
      canUndo.value = true
      clearTimeout(undoTimer)
      undoTimer = setTimeout(() => {
        canUndo.value = false
        if (autoClose.value) void api.writeClose()
      }, 6000)
    }
  } catch (e) {
    error.value = errorText(e)
  }
}

async function undo() {
  clearTimeout(undoTimer)
  canUndo.value = false
  done.value = (await api.writeUndo().catch(() => false)) ? '已撤销' : '无法撤销，请在原程序里按 Ctrl+Z'
}

function toggleFav() {
  const n = new Set(favs.value)
  if (n.has(out.value)) n.delete(out.value)
  else n.add(out.value)
  favs.value = n
  try {
    localStorage.setItem('wa.favs', JSON.stringify([...n].slice(-30)))
  } catch {
    /* 存不了就算了 */
  }
}

function setPin() {
  pin.value = !pin.value
  void api.writePin(pin.value)
}

function toggleAuto() {
  autoClose.value = !autoClose.value
  void api.writePrefs(action.value.id, extras.value, autoClose.value)
}

function pickAction(a: WriteAction) {
  action.value = a
  void api.writePrefs(a.id, extras.value, autoClose.value).catch(() => {})
}

/** 拖边缘缩放（无边框窗口没有系统边框可拖）。 */
function resize(dir: string) {
  void (getCurrentWindow() as unknown as { startResizeDragging: (d: string) => Promise<void> }).startResizeDragging(dir)
}

function openSettings() {
  void api.writeOpenSettings()
}

function onKey(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.key === 'Escape') void api.writeClose()
  else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) void run()
  else if (e.altKey && ['1', '2', '3'].includes(e.key) && outs.value.length >= Number(e.key)) {
    picked.value = Number(e.key) - 1
    e.preventDefault()
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onKey)
  try {
    favs.value = new Set(JSON.parse(localStorage.getItem('wa.favs') || '[]'))
  } catch {
    /* 没有收藏 */
  }
  themeMode = (await api.getSettings().catch(() => null))?.theme ?? 'system'
  applyTheme()
  media.addEventListener('change', applyTheme)
  cfg.value = await api.writeSettings().catch(() => null)
  if (cfg.value) {
    pin.value = cfg.value.pin
    autoClose.value = cfg.value.autoClose
    extras.value = cfg.value.extras ?? []
    action.value = ACTIONS.find((a) => a.id === cfg.value?.lastAction) ?? ACTIONS[0]
  }
  unlisten = await listen('write://new', () => void load())
  await load()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  unlisten?.()
  media.removeEventListener('change', applyTheme)
})
</script>

<template>
  <div class="shell">
    <div class="wa" :class="{ busy }">
      <header class="bar" data-tauri-drag-region>
        <span class="logo" data-tauri-drag-region>写</span>
        <div class="ttl" data-tauri-drag-region>
          <b data-tauri-drag-region>清影写作</b>
          <span v-if="src?.app" class="from" data-tauri-drag-region>来自 {{ src.app }}</span>
        </div>
        <button class="ib" :class="{ on: pin }" :title="pin ? '已固定在最前，点击取消' : '固定在最前'" @click="setPin">
          <svg viewBox="0 0 24 24"><path d="M9 4h6l-1 6 3 3v2h-5v5l-1 1-1-1v-5H5v-2l3-3z" /></svg>
        </button>
        <button class="ib" title="设置" @click="openSettings">
          <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.5-2-3.4-2.3 1a7 7 0 0 0-2-1.2L14 3h-4l-.6 2.7a7 7 0 0 0-2 1.2l-2.3-1-2 3.4 2 1.5A7 7 0 0 0 5 12c0 .4 0 .8.1 1.2l-2 1.5 2 3.4 2.3-1a7 7 0 0 0 2 1.2L10 21h4l.6-2.7a7 7 0 0 0 2-1.2l2.3 1 2-3.4-2-1.5c.1-.4.1-.8.1-1.2z" /></svg>
        </button>
        <button class="ib close" title="关闭 (Esc)" @click="api.writeClose()">
          <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </header>

      <main>
        <section class="box">
          <textarea v-model="text" class="src" rows="4" :placeholder="isReply ? '收到的消息（选中后按快捷键，或粘贴在这里）' : '选中文字后按快捷键，或在这里粘贴要处理的文字'" />
          <div class="boxbar">
            <span class="cnt">{{ hasText ? `${charCount} 字` : '还没有文字' }}</span>
            <span class="sp" />
            <button class="lnk" :disabled="busy" @click="pasteIn">粘贴</button>
            <button class="lnk" :disabled="!hasText || busy" @click="text = ''">清空</button>
            <button class="lnk mic" :class="{ rec: recording }" :disabled="busy && !recording" @click="dictate">{{ recording ? '■ 停止' : '听写' }}</button>
          </div>
          <div v-if="hist.length > 1" class="histrow">
            <span>最近</span>
            <button v-for="h in hist.slice(1, 4)" :key="h" class="chip tiny" :title="h" @click="text = h">{{ h.length > 10 ? h.slice(0, 10) + '…' : h }}</button>
          </div>
        </section>
        <small v-if="note && !hasText" class="hint">{{ note }}</small>

        <section class="acts">
          <div class="seg">
            <button v-for="a in mainActions" :key="a.id" class="sg" :class="{ on: action.id === a.id }" @click="pickAction(a)">{{ a.name }}</button>
            <button v-if="moreActions.length" class="sg more" :aria-expanded="showMore" @click="showMore = !showMore">{{ showMore ? '收起' : '更多' }}</button>
          </div>
          <div v-if="showMore" class="more-grid">
            <button v-for="a in moreActions" :key="a.id" class="chip" @click="pickAction(a); showMore = false">{{ a.name }}</button>
          </div>
        </section>

        <section class="opts">
          <select v-model="presetId" aria-label="场景">
            <option value="">场景：通用</option>
            <option v-for="p in PRESETS" :key="p.id" :value="p.id">{{ p.name }}</option>
          </select>
          <input v-if="action.id === 'translate'" v-model="target" class="tg" placeholder="目标语言" />
          <input v-model="extra" class="ex" :placeholder="isReply ? '我想表达的意思（例如：周五可以，改到下午）' : '额外要求（可选）'" maxlength="200" />
        </section>
        <div v-if="extras.length" class="recent">
          <button v-for="e in extras.slice(0, 4)" :key="e" class="chip tiny" :title="e" @click="extra = e">{{ e.length > 12 ? e.slice(0, 12) + '…' : e }}</button>
        </div>

        <div class="runrow">
          <button v-if="!busy" class="go" :disabled="!canRun" @click="run">{{ action.name }}<kbd>Ctrl ↵</kbd></button>
          <button v-else class="go stop" @click="stop"><i class="spin" />取消</button>
          <span v-if="busy" class="hint prog">{{ progress }}</span>
        </div>

        <div v-if="error" class="errbox">
          <p class="err">{{ error }}</p>
          <div class="row">
            <button class="ghost" @click="run">重试</button>
            <button class="ghost" @click="openSettings">{{ errHint ? '去设置模型' : '打开设置' }}</button>
          </div>
        </div>

        <section v-if="outs.length" class="res">
          <div class="reshd">
            <b>结果</b>
            <div v-if="outs.length > 1" class="vers">
              <button v-for="(_o, i) in outs" :key="i" class="vt" :class="{ on: picked === i }" :title="`Alt+${i + 1}`" @click="picked = i">{{ i + 1 }}</button>
            </div>
            <span class="sp" />
            <span class="cnt">{{ outCount }} 字<em v-if="delta" :class="delta > 0 ? 'up' : 'dn'">{{ delta > 0 ? '+' : '' }}{{ delta }}</em></span>
          </div>
          <div v-if="compare" class="out orig"><small>原文</small>{{ text }}</div>
          <div class="out">{{ out }}</div>
          <small v-if="guardText" class="warn">⚠ {{ guardText }}</small>
          <ul v-if="issues.length" class="issues">
            <li v-for="(it, i) in issues" :key="i"><s>{{ it.from }}</s> → <b>{{ it.to }}</b> <small>{{ it.reason }}</small></li>
          </ul>
          <small v-else-if="action.grammar" class="hint">没有发现需要修改的地方</small>
          <div class="row">
            <button v-if="src?.canReplace" class="go sm" @click="apply(true)">替换原文</button>
            <button class="ghost" @click="apply(false)">复制</button>
            <button class="ghost" @click="run">再来一版</button>
            <button class="ghost" :class="{ on: compare }" @click="compare = !compare">对比</button>
            <button class="ghost star" :class="{ on: faved }" :title="faved ? '取消收藏' : '收藏这个版本'" @click="toggleFav">{{ faved ? '★' : '☆' }}</button>
          </div>
          <div v-if="done || canUndo || src?.canReplace" class="row after">
            <span v-if="done" class="ok">✓ {{ done }}</span>
            <button v-if="canUndo" class="lnk" @click="undo">撤销</button>
            <label v-if="src?.canReplace" class="auto"><input type="checkbox" :checked="autoClose" @change="toggleAuto" /> 写回后自动关闭</label>
          </div>
        </section>
      </main>

      <footer class="foot">
        <span>快捷键 <kbd>{{ hotkey || '未设置' }}</kbd></span>
        <a href="#" @click.prevent="openSettings">修改</a>
        <span class="sp">Esc 关闭 · Alt+1/2/3 切版本</span>
      </footer>
    </div>
    <i class="rz rz-r" @mousedown.prevent="resize('East')" />
    <i class="rz rz-b" @mousedown.prevent="resize('South')" />
    <i class="rz rz-l" @mousedown.prevent="resize('West')" />
    <i class="rz rz-t" @mousedown.prevent="resize('North')" />
    <i class="rz rz-br" @mousedown.prevent="resize('SouthEast')" />
    <i class="rz rz-bl" @mousedown.prevent="resize('SouthWest')" />
    <i class="rz rz-tr" @mousedown.prevent="resize('NorthEast')" />
    <i class="rz rz-tl" @mousedown.prevent="resize('NorthWest')" />
  </div>
</template>

<style>
/* 窗口本身是透明的：页面底色一律透明，只有里面那张圆角卡片有颜色，窗口四周不会再有任何多余的底层容器 */
html, body, #app { background: transparent !important; overflow: hidden; }
html.dark .shell .wa { --acc: #f08a3c; --accd: #c8641f; --soft: #3a2a1d; --line: #2f383c; --bg: #1a2023; --card: #22292c; --mut: #9aa6a2; --err: #ec7a70; --ok: #5cc28a; --fg: #e7ecea; --shadow: 0 10px 30px rgba(0, 0, 0, 0.55), 0 0 0 1px #2f383c; }
html.dark .shell .sg.on { color: var(--acc); background: var(--soft); }
html.dark .shell .ghost.on { color: var(--acc); }
html.dark .shell .foot a { color: var(--acc); }
</style>

<style scoped>
.shell { --m: 10px; position: relative; height: 100vh; padding: var(--m); box-sizing: border-box; }
.wa { --acc: #d9762b; --accd: #b4561a; --soft: #fbeadb; --line: #e1e5e2; --bg: #f5f6f5; --card: #fff; --mut: #66716c; --err: #c2453b; --ok: #2f8a57; --fg: #1d2420; --shadow: 0 8px 28px rgba(20, 30, 25, 0.22), 0 1px 3px rgba(20, 30, 25, 0.18);
  height: 100%; display: flex; flex-direction: column; background: var(--card); color: var(--fg); border-radius: 14px; box-shadow: var(--shadow); overflow: hidden; font: 13px/1.55 -apple-system, 'PingFang SC', 'Microsoft YaHei', 'Noto Sans SC', sans-serif; }
.bar { display: flex; align-items: center; gap: 6px; padding: 10px 10px 8px 14px; cursor: grab; user-select: none; }
.bar:active { cursor: grabbing; }
.logo { width: 24px; height: 24px; border-radius: 8px; background: linear-gradient(135deg, var(--acc), var(--accd)); color: #fff; display: grid; place-items: center; font: 700 12px/1 inherit; flex: none; }
.ttl { display: flex; align-items: baseline; gap: 8px; min-width: 0; flex: 1; }
.ttl b { font-size: 14px; }
.from { font-size: 11px; color: var(--mut); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ib { border: 0; background: none; width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; cursor: pointer; color: var(--mut); }
.ib svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.ib:hover { background: var(--bg); color: var(--fg); }
.ib.on { color: var(--acc); }
.ib.on svg { fill: currentColor; fill-opacity: 0.25; }
.ib.close:hover { background: var(--err); color: #fff; }
main { flex: 1; overflow: auto; padding: 4px 14px 12px; display: flex; flex-direction: column; gap: 10px; scrollbar-width: thin; }
.box { border: 1px solid var(--line); background: var(--bg); border-radius: 10px; display: flex; flex-direction: column; }
.box:focus-within { border-color: var(--acc); box-shadow: 0 0 0 3px color-mix(in srgb, var(--acc) 18%, transparent); }
.src { width: 100%; box-sizing: border-box; resize: none; padding: 9px 11px 4px; border: 0; background: transparent; color: inherit; font: inherit; outline: none; min-height: 84px; }
.boxbar { display: flex; align-items: center; gap: 4px; padding: 2px 8px 6px 11px; font-size: 12px; color: var(--mut); }
.sp { flex: 1; }
.lnk { border: 0; background: none; color: var(--mut); cursor: pointer; font: inherit; font-size: 12px; padding: 1px 6px; border-radius: 6px; }
.lnk:hover:not(:disabled) { background: var(--line); color: var(--fg); }
.lnk:disabled { opacity: 0.4; cursor: default; }
.lnk.mic.rec { color: var(--err); font-weight: 600; }
.histrow { display: flex; gap: 6px; align-items: center; padding: 0 10px 8px; font-size: 11px; color: var(--mut); }
.seg { display: flex; flex-wrap: wrap; gap: 2px; background: var(--bg); border: 1px solid var(--line); border-radius: 10px; padding: 3px; }
.sg { flex: 1; border: 0; background: none; color: var(--mut); border-radius: 7px; padding: 5px 4px; cursor: pointer; font: inherit; font-size: 12.5px; white-space: nowrap; }
.sg:hover { color: var(--fg); }
.sg.on { background: var(--card); color: var(--accd); font-weight: 600; box-shadow: 0 1px 3px rgba(0, 0, 0, 0.14); }
.sg.more { flex: 0 0 auto; padding: 5px 9px; }
.more-grid { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
.chip { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 999px; padding: 2px 11px; cursor: pointer; font: inherit; font-size: 12px; }
.chip:hover { border-color: var(--acc); color: var(--accd); }
.chip.tiny { padding: 0 8px; font-size: 11px; color: var(--mut); line-height: 20px; max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.opts { display: flex; gap: 6px; }
.opts select, .opts input { min-width: 0; padding: 6px 9px; border-radius: 8px; border: 1px solid var(--line); background: var(--card); color: inherit; font: inherit; outline: none; }
.opts select { flex: 0 0 auto; max-width: 130px; }
.opts .ex { flex: 1; }
.opts .tg { width: 90px; }
.opts select:focus, .opts input:focus { border-color: var(--acc); }
.recent { display: flex; gap: 6px; flex-wrap: wrap; margin-top: -4px; }
.runrow { display: flex; align-items: center; gap: 10px; }
.go { flex: 1; display: flex; align-items: center; justify-content: center; gap: 8px; background: linear-gradient(180deg, var(--acc), var(--accd)); color: #fff; border: 0; border-radius: 10px; padding: 9px 14px; cursor: pointer; font: inherit; font-weight: 600; font-size: 14px; }
.go kbd { font: 11px/1 inherit; opacity: 0.8; border: 1px solid rgba(255, 255, 255, 0.45); border-radius: 4px; padding: 2px 5px; font-weight: 400; }
.go:hover:not(:disabled) { filter: brightness(1.08); }
.go:disabled { opacity: 0.45; cursor: default; }
.go.stop { background: var(--bg); color: var(--fg); border: 1px solid var(--line); }
.go.sm { flex: 0 0 auto; padding: 6px 16px; font-size: 13px; }
.spin { width: 12px; height: 12px; border-radius: 50%; border: 2px solid var(--line); border-top-color: var(--acc); animation: sp 0.8s linear infinite; }
@keyframes sp { to { transform: rotate(360deg); } }
.prog { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ghost { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 8px; padding: 5px 11px; cursor: pointer; font: inherit; }
.ghost:hover { border-color: var(--acc); }
.ghost.on { background: var(--soft); border-color: var(--acc); color: var(--accd); }
.ghost.star { padding: 5px 9px; }
.row { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.row.after { font-size: 12px; }
button:focus-visible, select:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--acc); outline-offset: 1px; }
.hint { color: var(--mut); }
.err { color: var(--err); margin: 0; }
.errbox { display: flex; flex-direction: column; gap: 6px; background: color-mix(in srgb, var(--err) 9%, transparent); border: 1px solid color-mix(in srgb, var(--err) 35%, transparent); border-radius: 10px; padding: 8px 10px; }
.warn { color: #b4701a; }
.ok { color: var(--ok); }
.res { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--line); padding-top: 10px; }
.reshd { display: flex; align-items: center; gap: 8px; }
.vers { display: flex; gap: 2px; }
.vt { width: 22px; height: 22px; border-radius: 6px; border: 1px solid var(--line); background: none; color: var(--mut); cursor: pointer; font: inherit; font-size: 11px; }
.vt.on { background: var(--soft); border-color: var(--acc); color: var(--accd); font-weight: 600; }
.cnt { font-size: 12px; color: var(--mut); }
.cnt em { font-style: normal; margin-left: 6px; }
.cnt .up { color: var(--ok); }
.cnt .dn { color: var(--acc); }
.out { white-space: pre-wrap; background: var(--soft); border: 1px solid color-mix(in srgb, var(--acc) 30%, transparent); border-radius: 10px; padding: 10px 12px; user-select: text; line-height: 1.7; }
.out.orig { background: var(--bg); border-color: var(--line); color: var(--mut); display: flex; flex-direction: column; gap: 2px; }
.out.orig small { font-size: 11px; }
.issues { margin: 0; padding-left: 18px; font-size: 12px; }
.issues s { background: color-mix(in srgb, var(--err) 16%, transparent); color: var(--err); border-radius: 4px; padding: 0 4px; }
.issues b { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); border-radius: 4px; padding: 0 4px; }
.auto { margin-left: auto; font-size: 12px; color: var(--mut); display: flex; gap: 4px; align-items: center; }
.foot { display: flex; gap: 8px; align-items: center; font-size: 11px; color: var(--mut); border-top: 1px solid var(--line); padding: 7px 14px; background: var(--bg); }
.foot a { color: var(--accd); }
.foot .sp { margin-left: auto; flex: none; }
kbd { font: 11px/1 ui-monospace, Consolas, monospace; }
.foot kbd { border: 1px solid var(--line); border-radius: 4px; padding: 1px 5px; background: var(--card); }
/* 缩放把手放在透明边距里，窗口没有系统边框，四周都能拖 */
.rz { position: absolute; z-index: 5; }
.rz-r { right: 0; top: 14px; bottom: 14px; width: 8px; cursor: ew-resize; }
.rz-l { left: 0; top: 14px; bottom: 14px; width: 8px; cursor: ew-resize; }
.rz-b { bottom: 0; left: 14px; right: 14px; height: 8px; cursor: ns-resize; }
.rz-t { top: 0; left: 14px; right: 14px; height: 8px; cursor: ns-resize; }
.rz-br { right: 0; bottom: 0; width: 16px; height: 16px; cursor: nwse-resize; }
.rz-bl { left: 0; bottom: 0; width: 16px; height: 16px; cursor: nesw-resize; }
.rz-tr { right: 0; top: 0; width: 16px; height: 16px; cursor: nesw-resize; }
.rz-tl { left: 0; top: 0; width: 16px; height: 16px; cursor: nwse-resize; }
</style>
