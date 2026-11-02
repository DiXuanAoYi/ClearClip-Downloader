<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText } from './api'
import { useLanguage } from './i18n/useLanguage'
import type { WritePending } from './types'
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
const outs = ref<{ text: string; guard?: string | null }[]>([])
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
const canRun = computed(() => !!text.value.trim() && !busy.value)

async function load() {
  const p = await api.writeTake()
  src.value = p
  text.value = p?.text ?? ''
  note.value = p?.note ?? ''
  outs.value = []
  issues.value = []
  error.value = ''
  done.value = ''
}

async function run() {
  if (!canRun.value) return
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
  } catch (e) {
    error.value = errorText(e)
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') void api.writeClose()
  else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) void run()
}

onMounted(async () => {
  window.addEventListener('keydown', onKey)
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
      <b data-tauri-drag-region>清影写作</b>
      <span class="from" data-tauri-drag-region>{{ src?.app || '' }}</span>
      <button class="x" title="关闭 (Esc)" @click="api.writeClose()">×</button>
    </header>
    <main>
      <textarea v-model="text" class="src" rows="5" :placeholder="isReply ? '收到的消息（选中后按快捷键，或粘贴在这里）' : '选中文字后按快捷键，或在这里粘贴要处理的文字'" />
      <div class="row">
        <button class="ghost" :class="{ rec: recording }" :disabled="busy && !recording" @click="dictate">{{ recording ? '■ 停止并转成文字' : '🎤 听写' }}</button>
      </div>
      <small v-if="note" class="hint">{{ note }}</small>
      <div class="chips">
        <button v-for="a in ACTIONS" :key="a.id" class="chip" :class="{ on: action.id === a.id }" @click="action = a">{{ a.name }}</button>
      </div>
      <div class="row">
        <select v-model="presetId" aria-label="场景">
          <option value="">场景：通用</option>
          <option v-for="p in PRESETS" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
        <input v-if="action.id === 'translate'" v-model="target" placeholder="目标语言" />
        <input v-model="extra" :placeholder="isReply ? '我想表达的意思（例如：周五可以，改到下午）' : '额外要求（可选）'" maxlength="200" />
      </div>
      <div class="row">
        <button v-if="!busy" class="go" :disabled="!canRun" @click="run">{{ action.name }}（Ctrl+Enter）</button>
        <button v-else class="go stop" @click="stop">取消</button>
        <span v-if="busy" class="hint">{{ progress }}</span>
      </div>
      <p v-if="error" class="err">{{ error }}</p>
      <section v-if="outs.length" class="res">
        <div v-if="outs.length > 1" class="chips">
          <button v-for="(_o, i) in outs" :key="i" class="chip" :class="{ on: picked === i }" @click="picked = i">版本 {{ i + 1 }}</button>
        </div>
        <div class="out">{{ out }}</div>
        <small v-if="outs[picked]?.guard" class="warn">{{ outs[picked].guard }}</small>
        <ul v-if="issues.length" class="issues">
          <li v-for="(it, i) in issues" :key="i"><s>{{ it.from }}</s> → <b>{{ it.to }}</b> <small>{{ it.reason }}</small></li>
        </ul>
        <small v-else-if="action.grammar" class="hint">没有发现需要修改的地方</small>
        <div class="row">
          <button v-if="src?.canReplace" class="go" @click="apply(true)">替换原文</button>
          <button class="ghost" @click="apply(false)">复制</button>
          <button class="ghost" @click="run">重试</button>
          <span v-if="done" class="ok">{{ done }}</span>
        </div>
      </section>
    </main>
  </div>
</template>

<style scoped>
.wa { height: 100vh; display: flex; flex-direction: column; background: var(--el-bg-color, #fff); color: var(--el-text-color-primary, #14202b); font: 13px/1.6 system-ui, 'Noto Sans SC', sans-serif; border: 1px solid var(--el-border-color, #cbd5dc); box-sizing: border-box; }
.bar { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-bottom: 1px solid var(--el-border-color-lighter, #e3e8ec); cursor: move; user-select: none; }
.from { flex: 1; color: var(--el-text-color-secondary, #6b7b88); font-size: 12px; }
.x { border: 0; background: none; font-size: 18px; cursor: pointer; color: inherit; }
main { flex: 1; overflow: auto; padding: 10px; display: flex; flex-direction: column; gap: 8px; }
.src { width: 100%; box-sizing: border-box; resize: vertical; padding: 6px 8px; border-radius: 6px; border: 1px solid var(--el-border-color, #cbd5dc); background: transparent; color: inherit; font: inherit; }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { border: 1px solid var(--el-border-color, #cbd5dc); background: transparent; color: inherit; border-radius: 999px; padding: 1px 10px; cursor: pointer; font: inherit; }
.chip.on { background: #0f766e; border-color: #0f766e; color: #fff; }
.row { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.row select, .row input { flex: 1; min-width: 100px; padding: 3px 6px; border-radius: 6px; border: 1px solid var(--el-border-color, #cbd5dc); background: transparent; color: inherit; font: inherit; }
.go { background: #0f766e; color: #fff; border: 0; border-radius: 6px; padding: 5px 14px; cursor: pointer; font: inherit; }
.go:disabled { opacity: 0.5; cursor: default; }
.go.stop { background: #b45309; }
.ghost.rec { border-color: #b91c1c; color: #b91c1c; }
.ghost { border: 1px solid var(--el-border-color, #cbd5dc); background: transparent; color: inherit; border-radius: 6px; padding: 5px 12px; cursor: pointer; font: inherit; }
.hint { color: var(--el-text-color-secondary, #6b7b88); }
.err { color: #b91c1c; margin: 0; }
.warn { color: #b45309; }
.ok { color: #0f766e; }
.res { display: flex; flex-direction: column; gap: 6px; border-top: 1px dashed var(--el-border-color, #cbd5dc); padding-top: 8px; }
.out { white-space: pre-wrap; background: rgba(15, 118, 110, 0.08); border-radius: 6px; padding: 8px; user-select: text; }
.issues { margin: 0; padding-left: 18px; }
</style>
