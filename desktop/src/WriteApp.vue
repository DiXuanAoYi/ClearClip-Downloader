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
})
</script>

<template>
  <div class="wa">
    <header class="bar" data-tauri-drag-region>
      <span class="logo" data-tauri-drag-region>写</span>
      <b data-tauri-drag-region>清影写作</b>
      <span class="from" data-tauri-drag-region>{{ src?.app || '' }}</span>
      <span class="grip" data-tauri-drag-region><i /><i /><i /><i /><i /><i /></span>
      <button class="x pinb" :class="{ on: pin }" :title="pin ? '已固定在最前，点击取消' : '固定在最前'" @click="setPin">📌</button>
      <button class="x" title="关闭 (Esc)" @click="api.writeClose()">×</button>
    </header>
    <main>
      <textarea v-model="text" class="src" rows="5" :placeholder="isReply ? '收到的消息（选中后按快捷键，或粘贴在这里）' : '选中文字后按快捷键，或在这里粘贴要处理的文字'" />
      <div class="row">
        <button class="ghost" :class="{ rec: recording }" :disabled="busy && !recording" @click="dictate">{{ recording ? '■ 停止并转成文字' : '🎤 听写' }}</button>
      </div>
      <small v-if="note && !hasText" class="hint">{{ note }}</small>
      <div class="chips">
        <button v-for="a in mainActions" :key="a.id" class="chip" :class="{ on: action.id === a.id }" @click="pickAction(a)">{{ a.name }}</button>
        <button v-if="moreActions.length" class="chip more" :aria-expanded="showMore" @click="showMore = !showMore">{{ showMore ? '收起' : '更多 ▾' }}</button>
      </div>
      <div v-if="showMore" class="chips">
        <button v-for="a in moreActions" :key="a.id" class="chip" @click="pickAction(a)">{{ a.name }}</button>
      </div>
      <div class="row">
        <select v-model="presetId" aria-label="场景">
          <option value="">场景：通用</option>
          <option v-for="p in PRESETS" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
        <input v-if="action.id === 'translate'" v-model="target" placeholder="目标语言" />
        <input v-model="extra" :placeholder="isReply ? '我想表达的意思（例如：周五可以，改到下午）' : '额外要求（可选）'" maxlength="200" />
      </div>
      <div v-if="extras.length" class="chips recent">
        <button v-for="e in extras.slice(0, 5)" :key="e" class="chip" :title="e" @click="extra = e">{{ e.length > 12 ? e.slice(0, 12) + '…' : e }}</button>
      </div>
      <div class="row">
        <button v-if="!busy" class="go" :disabled="!canRun" @click="run">{{ action.name }}（Ctrl+Enter）</button>
        <button v-else class="go stop" @click="stop">取消</button>
        <span v-if="busy" class="hint">{{ progress }}</span>
      </div>
      <div v-if="error" class="errbox">
        <p class="err">{{ error }}</p>
        <div class="row">
          <button class="ghost" @click="run">重试</button>
          <button class="ghost" @click="openSettings">{{ errHint ? '去设置模型' : '打开设置' }}</button>
        </div>
      </div>
      <section v-if="outs.length" class="res">
        <div v-if="outs.length > 1" class="chips">
          <button v-for="(_o, i) in outs" :key="i" class="chip" :class="{ on: picked === i }" :title="`Alt+${i + 1}`" @click="picked = i">版本 {{ i + 1 }}</button>
        </div>
        <div v-if="compare" class="cmp">
          <small class="hint">原文</small>
          <div class="out orig">{{ text }}</div>
          <small class="hint">结果</small>
        </div>
        <div class="out">{{ out }}</div>
        <small v-if="guardText" class="warn">⚠ {{ guardText }}</small>
        <ul v-if="issues.length" class="issues">
          <li v-for="(it, i) in issues" :key="i"><s>{{ it.from }}</s> → <b>{{ it.to }}</b> <small>{{ it.reason }}</small></li>
        </ul>
        <small v-else-if="action.grammar" class="hint">没有发现需要修改的地方</small>
        <div class="row">
          <button v-if="src?.canReplace" class="go" @click="apply(true)">替换原文</button>
          <button class="ghost" @click="apply(false)">复制</button>
          <button class="ghost" @click="run">重试</button>
          <button class="ghost" :class="{ on: compare }" @click="compare = !compare">对比</button>
          <button class="ghost" :class="{ on: faved }" :title="faved ? '取消收藏' : '收藏这个版本'" @click="toggleFav">{{ faved ? '★' : '☆' }}</button>
        </div>
        <div class="row">
          <span v-if="done" class="ok">{{ done }}</span>
          <button v-if="canUndo" class="ghost" @click="undo">撤销</button>
          <label v-if="src?.canReplace" class="auto"><input type="checkbox" :checked="autoClose" @change="toggleAuto" /> 写回后自动关闭</label>
        </div>
      </section>
      <footer class="foot">
        <span>快捷键 {{ hotkey || '未设置' }}</span>
        <a href="#" @click.prevent="openSettings">修改</a>
        <span class="sp">Esc 关闭 · Ctrl+Enter 运行 · Alt+1/2/3 切换版本</span>
      </footer>
    </main>
    <i class="rz rz-r" @mousedown.prevent="resize('East')" />
    <i class="rz rz-b" @mousedown.prevent="resize('South')" />
    <i class="rz rz-l" @mousedown.prevent="resize('West')" />
    <i class="rz rz-t" @mousedown.prevent="resize('North')" />
    <i class="rz rz-br" @mousedown.prevent="resize('SouthEast')" />
    <i class="rz rz-bl" @mousedown.prevent="resize('SouthWest')" />
  </div>
</template>

<style>
/* 窗口不透明、内容铺满：底色和卡片同色，四周不会露出别的颜色 */
html, body, #app { background: #fff !important; overflow: hidden; }
@media (prefers-color-scheme: dark) { html, body, #app { background: #22292c !important; } }
</style>

<style scoped>
.wa { --acc: #d9762b; --accd: #b4561a; --soft: #fbeadb; --line: #d9dedb; --bg: #f4f5f4; --card: #fff; --mut: #5d6862; --err: #c2453b; --ok: #2f8a57; position: relative; height: 100vh; display: flex; flex-direction: column; background: var(--card); color: #1d2420; font: 13px/1.55 -apple-system, 'PingFang SC', 'Microsoft YaHei', 'Noto Sans SC', sans-serif; border: 1px solid var(--line); border-radius: 0; overflow: hidden; box-sizing: border-box; }
@media (prefers-color-scheme: dark) { .wa { --acc: #f08a3c; --accd: #c8641f; --soft: #3a2a1d; --line: #2f383c; --bg: #14181a; --card: #22292c; --mut: #9aa6a2; --err: #ec7a70; --ok: #5cc28a; color: #e7ecea; } }
.bar { display: flex; align-items: center; gap: 8px; padding: 9px 10px 9px 12px; border-bottom: 1px solid var(--line); background: var(--bg); cursor: grab; user-select: none; }
.bar:active { cursor: grabbing; }
.logo { width: 20px; height: 20px; border-radius: 6px; background: var(--acc); color: #fff; display: grid; place-items: center; font: 700 11px/1 inherit; flex: none; }
.from { font-size: 11px; color: var(--mut); background: var(--card); border: 1px solid var(--line); border-radius: 999px; padding: 0 8px; line-height: 18px; max-width: 150px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.from:empty { display: none; }
.grip { margin-left: auto; display: grid; grid-template-columns: repeat(3, 3px); gap: 3px; opacity: 0.55; pointer-events: none; }
.grip i { width: 3px; height: 3px; border-radius: 50%; background: var(--mut); }
.x { border: 0; background: none; width: 24px; height: 24px; border-radius: 6px; font-size: 16px; line-height: 1; cursor: pointer; color: var(--mut); }
.x:hover { background: var(--line); color: inherit; }
main { flex: 1; overflow: auto; padding: 12px; display: flex; flex-direction: column; gap: 10px; }
.src { width: 100%; box-sizing: border-box; resize: vertical; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--bg); color: inherit; font: inherit; }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 8px; padding: 2px 10px; cursor: pointer; font: inherit; font-size: 12px; }
.chip.on { background: var(--soft); border-color: var(--acc); color: var(--accd); font-weight: 600; }
@media (prefers-color-scheme: dark) { .chip.on { color: var(--acc); } }
.row { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.row select, .row input { flex: 1; min-width: 100px; padding: 5px 8px; border-radius: 6px; border: 1px solid var(--line); background: var(--card); color: inherit; font: inherit; }
button:focus-visible, select:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--acc); outline-offset: 1px; }
.go { flex: 1; background: var(--accd); color: #fff; border: 0; border-radius: 8px; padding: 8px 14px; cursor: pointer; font: inherit; font-weight: 600; }
.go:hover:not(:disabled) { background: var(--acc); }
.go:disabled { opacity: 0.5; cursor: default; }
.go.stop { background: #8a5a14; }
.ghost { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 8px; padding: 7px 12px; cursor: pointer; font: inherit; }
.ghost.rec { border-color: var(--err); color: var(--err); }
.hint { color: var(--mut); }
.err { color: var(--err); margin: 0; }
.warn { color: #b4701a; }
.ok { color: var(--ok); }
.res { display: flex; flex-direction: column; gap: 8px; border-top: 1px solid var(--line); padding-top: 10px; }
.out { white-space: pre-wrap; background: var(--soft); border: 1px solid color-mix(in srgb, var(--acc) 35%, transparent); border-radius: 8px; padding: 9px 11px; user-select: text; }
.issues { margin: 0; padding-left: 18px; font-size: 12px; }
.issues s { background: color-mix(in srgb, var(--err) 16%, transparent); color: var(--err); border-radius: 4px; padding: 0 4px; }
.issues b { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); border-radius: 4px; padding: 0 4px; }
.x.pinb { filter: grayscale(1); opacity: .5; font-size: 13px; }
.x.pinb.on { filter: none; opacity: 1; }
.chip.more { border-style: dashed; color: var(--mut); }
.recent .chip { color: var(--mut); }
.errbox { display: flex; flex-direction: column; gap: 6px; }
.ghost.on { background: var(--soft); border-color: var(--acc); color: var(--accd); }
.orig { background: var(--bg); border-color: var(--line); color: var(--mut); }
.cmp { display: flex; flex-direction: column; gap: 4px; }
.auto { margin-left: auto; font-size: 12px; color: var(--mut); display: flex; gap: 4px; align-items: center; }
.foot { display: flex; gap: 8px; font-size: 11px; color: var(--mut); border-top: 1px dashed var(--line); padding-top: 8px; flex-wrap: wrap; }
.foot a { color: var(--accd); }
.foot .sp { margin-left: auto; }
.rz { position: absolute; z-index: 5; }
.rz-r { right: 0; top: 8px; bottom: 8px; width: 5px; cursor: ew-resize; }
.rz-l { left: 0; top: 8px; bottom: 8px; width: 5px; cursor: ew-resize; }
.rz-b { bottom: 0; left: 8px; right: 8px; height: 5px; cursor: ns-resize; }
.rz-t { top: 0; left: 8px; right: 8px; height: 4px; cursor: ns-resize; }
.rz-br { right: 0; bottom: 0; width: 12px; height: 12px; cursor: nwse-resize; }
.rz-bl { left: 0; bottom: 0; width: 12px; height: 12px; cursor: nesw-resize; }
</style>
