<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText } from './api'
import CcSelect from './components/ui/CcSelect.vue'
import CcSegmented from './components/ui/CcSegmented.vue'
import CcLink from './components/ui/CcLink.vue'
import CcSkeleton from './components/ui/CcSkeleton.vue'
import CcButton from './components/ui/CcButton.vue'
import { notifyError } from './utils/notify'
import { useLanguage } from './i18n/useLanguage'
import type { Excerpt, WritePending, WritePersona, WritingSettings } from './types'
import { isWord, speechLang } from './utils/selection'
import { floatToInt16, int16ToBase64, level as pcmLevel, resampleTo16k } from './utils/pcm'
import { joinFinals } from './utils/dictation'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  ACTIONS,
  ALTS,
  INTENTS,
  LENGTHS,
  PRESETS,
  RELATIONS,
  TABS,
  TAB_ACTIONS,
  TONES,
  TWEAKS,
  WHEN,
  actionById,
  appInfo,
  buildReadRequest,
  buildReplyRequest,
  buildTweakRequest,
  buildWriteRequest,
  detailField,
  humorRisk,
  newReplyForm,
  presetById,
  readParts,
  relationKey,
  replyMissing,
  tabOf,
  type ReplyForm,
  type ReplyRead,
  type Tab,
  type WriteAction,
} from './writePresets'
import { useTauriEvents } from './composables/useTauriEvent'
import { toastErr } from './utils/toastErr'
const ev = useTauriEvents()

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
type Guard = { missing: string[]; ratio: number; warn: boolean; added?: string[] }
const outs = ref<{ text: string; label?: string; guard?: Guard | null }[]>([])
const cfg = ref<WritingSettings | null>(null)
const pin = ref(true)
const autoClose = ref(false)
const extras = ref<string[]>([])
const compare = ref(false)
const favs = ref<Set<string>>(new Set())
const canUndo = ref(false)
let undoTimer: ReturnType<typeof setTimeout> | undefined
// 四个标签：帮我回 / 改写润色 / 检查 / 翻译 · 更多
const tab = ref<Tab>('rewrite')
// 帮我回：对方是谁 × 我的意思 × 怎么说
const form = ref<ReplyForm>(newReplyForm())
const read = ref<ReplyRead | null>(null)
const reading = ref(false)
const showCtx = ref(false)
const newQ = ref('')
/** 改写润色里的“怎么说”（可以不选） */
const rwTone = ref('')
const showFavs = ref(false)
let readKey = ''
let readTimer: ReturnType<typeof setTimeout> | undefined
let readSeq = 0
const picked = ref(0)
const issues = ref<{ from: string; to: string; reason: string }[]>([])
const done = ref('')
// 划词工具：对选中的文字做释义 / 朗读 / AI 搜索 / 总结 / 存入摘录（结果不能“替换原文”）
const selMode = ref(false)
const selTitle = ref('')
const excerpts = ref<Excerpt[]>([])
const showEx = ref(false)
const speaking = ref(false)
const recording = ref(false)
const isReply = computed(() => tab.value === 'reply')
const app = computed(() => appInfo(src.value?.app))
const who = computed(() => read.value?.who ?? '')
const tabActions = computed(() => TAB_ACTIONS[tab.value].map(actionById))
const field = computed(() => detailField(form.value.intent))
const missing = computed(() => (isReply.value ? replyMissing(form.value, text.value) : ''))
const risky = computed(() => humorRisk(form.value))
const look = computed(() => readParts(read.value))
const replaceLabel = computed(() => (app.value.chat ? `替换到${app.value.name}` : '替换原文'))
const favList = computed(() => [...favs.value].reverse())
let jobId = 0
let unlisten: UnlistenFn | undefined

const out = computed(() => outs.value[picked.value]?.text ?? '')
const hotkey = computed(() => cfg.value?.hotkey || '')
/** 把检查结果说成人话；续写、回复不提示长度。 */
const guardText = computed(() => {
  const g = outs.value[picked.value]?.guard
  if (!g?.warn) return ''
  if (g.added?.length) return `结果里的「${g.added.slice(0, 4).join('」「')}」在消息和你给的内容里都没有，发送前请核对`
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
// 人设：选一个，输入收到的消息或话题，一次给出结果
const personas = ref<WritePersona[]>([])
const personaId = ref('')
const persona = computed(() => personas.value.find((p) => p.id === personaId.value))
void api.writePersonas().then((l) => (personas.value = l)).catch(() => {}) // 忽略：人物列表是可选的
const canRun = computed(() => !!text.value.trim() && !busy.value && !missing.value && (tab.value !== 'persona' || !!personaId.value))

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
  // 从聊天、邮件程序取到的文字直接进“帮我回”；别的程序停在上次用的标签
  tab.value = app.value.chat ? 'reply' : tabOf(cfg.value?.lastAction || 'rewrite')
  if (tab.value === 'reply') action.value = actionById('reply')
  else if (!TAB_ACTIONS[tab.value].includes(action.value.id)) action.value = actionById(TAB_ACTIONS[tab.value][0])
  const keep = form.value
  form.value = { ...newReplyForm(), tone: keep.tone || 'eq', length: keep.length || 'medium' }
  form.value.relation = cfg.value?.relations?.[relationKey(src.value?.app, '')] ?? ''
  read.value = null
  readKey = ''
  showCtx.value = false
  scheduleRead()
}

// ---------- 帮我回 ----------

/** 收到的消息变了（或贴了前面的聊天）就重新“看懂”一次：一句话说出对方要什么，列出没说清的地方。 */
function scheduleRead() {
  clearTimeout(readTimer)
  if (tab.value !== 'reply') return
  const t = text.value.trim()
  const key = t + '\u0000' + form.value.context.trim()
  if (t.length < 2 || key === readKey) return
  readTimer = setTimeout(() => void runRead(key), 700)
}

async function runRead(key: string) {
  readKey = key
  const seq = ++readSeq
  reading.value = true
  try {
    const id = await api.writeRun(buildReadRequest(text.value, form.value.context))
    for (;;) {
      await new Promise((r) => setTimeout(r, 400))
      if (seq !== readSeq) return
      const j = await api.writeJob(id)
      if (!j || j.state === 'error') return
      if (j.state === 'done') {
        const r = (j.result ?? {}) as ReplyRead
        read.value = { ask: r.ask ?? '', facts: r.facts ?? '', unclear: r.unclear ?? [], relation: r.relation ?? '', who: r.who ?? '' }
        // 没说清的地方：最要紧的两条默认勾上
        form.value.questions = read.value.unclear.map((q, i) => ({ text: q, on: i < 2 }))
        const remembered = cfg.value?.relations?.[relationKey(src.value?.app, read.value.who)]
        if (remembered) form.value.relation = remembered
        else if (!form.value.relation && read.value.relation) form.value.relation = read.value.relation
        return
      }
    }
  } catch {
    // 看不懂也不影响生成回复
  } finally {
    if (seq === readSeq) reading.value = false
  }
}

function pickRelation(id: string) {
  form.value.relation = form.value.relation === id ? '' : id
  const key = relationKey(src.value?.app, who.value)
  if (!key) return
  if (cfg.value) cfg.value.relations = { ...(cfg.value.relations ?? {}), [key]: form.value.relation }
  void api.writeRelation(key, form.value.relation).catch(toastErr('保存对象关系'))
}

function pickIntent(id: string) {
  const f = form.value
  if (f.intent === id) {
    f.intent = ''
    return
  }
  if (f.intent !== id) f.detail = ''
  f.intent = id
  // “拒绝”默认配“高情商”：就是体面拒绝
  if (id === 'decline') f.tone = 'eq'
}

function addQuestion() {
  const q = newQ.value.trim()
  if (!q || form.value.questions.length >= 8) return
  form.value.questions.push({ text: q, on: true })
  newQ.value = ''
}

const ENGINE_OPTS = [
  { value: 'auto', label: '听写：跟随设置' },
  { value: 'local', label: '本机开源模型' },
  { value: 'api', label: '在线接口' },
]
const presetOpts = computed(() => [{ value: '', label: '场景：通用' }, ...PRESETS.map((p) => ({ value: p.id, label: p.name }))])
const actionOpts = computed(() => tabActions.value.map((a) => ({ value: a.id, label: a.name })))
function pickActionId(id: string) {
  const a = tabActions.value.find((x) => x.id === id)
  if (a) pickAction(a)
}
function pickTab(t: Tab) {
  if (tab.value === t) return
  tab.value = t
  const ids = TAB_ACTIONS[t]
  if (!ids.includes(action.value.id)) action.value = actionById(ids[0])
  outs.value = []
  issues.value = []
  error.value = ''
  selMode.value = false
  void api.writePrefs(action.value.id, extras.value, autoClose.value).catch(toastErr('保存偏好'))
  scheduleRead()
}

/** 微调：在当前这一版上改（更软、更硬、再短点…），改完替换这一版。 */
async function tweak(id: string) {
  const cur = outs.value[picked.value]
  if (!cur || busy.value) return
  busy.value = true
  error.value = ''
  progress.value = '正在微调…'
  try {
    jobId = await api.writeRun(buildTweakRequest(id, cur.text, text.value, src.value?.app))
    for (;;) {
      await new Promise((r) => setTimeout(r, 450))
      const j = await api.writeJob(jobId)
      if (!j) throw new Error('任务已过期')
      if (j.state === 'error') throw new Error(j.error ?? '处理失败')
      if (j.state === 'done') {
        const c = (j.result?.candidates ?? [])[0]
        if (!c?.text) throw new Error('没有得到结果')
        outs.value = outs.value.map((o, i) => (i === picked.value ? { ...o, text: c.text, guard: c.guard } : o))
        break
      }
    }
  } catch (e) {
    error.value = errorText(e)
    notifyError('生成失败', { desc: '模型没有给出结果。', detail: error.value, onRetry: () => void run() })
  } finally {
    busy.value = false
    progress.value = ''
  }
}

async function navCopy(t: string) {
  try {
    await navigator.clipboard.writeText(t)
    done.value = '已复制'
  } catch {
    error.value = '复制失败'
  }
}

function dropFav(t: string) {
  const n = new Set(favs.value)
  n.delete(t)
  favs.value = n
  try {
    localStorage.setItem('wa.favs', JSON.stringify([...n].slice(-30)))
  } catch {
    /* 存不了就算了 */
  }
}

function saveExtra() {
  if (isReply.value) return void api.writePrefs('reply', extras.value, autoClose.value).catch(toastErr('保存偏好'))
  const e = extra.value.trim()
  if (e) extras.value = [e, ...extras.value.filter((x) => x !== e)].slice(0, 8)
  void api.writePrefs(action.value.id, extras.value, autoClose.value).catch(toastErr('保存偏好'))
}

async function run() {
  if (!canRun.value) return
  saveExtra()
  selMode.value = false
  busy.value = true
  error.value = ''
  outs.value = []
  issues.value = []
  progress.value = '正在处理…'
  try {
    const req = isReply.value
      ? buildReplyRequest(form.value, text.value, src.value?.app)
      : buildWriteRequest({ action: action.value, text: text.value, preset: tab.value === 'rewrite' ? presetById(presetId.value) : undefined, extra: extra.value, target: target.value, app: src.value?.app, tone: tab.value === 'rewrite' ? rwTone.value : '', persona: personaId.value })
    jobId = await api.writeRun(req)
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

// ---------- 实时听写 ----------
// 麦克风 → 16 kHz 单声道 PCM → 分块送后端 → 后端按停顿切句、边说边识别：
// 说到一半是“临时结果”（会被改写，灰字显示），停顿后这一句定稿。引擎：跟着“设置 → AI → 语音转文字”，也可以临时选本机开源模型或在线接口。
const dictEngine = ref<'auto' | 'local' | 'api'>('auto')
const dictLabel = ref('')
const dictLevel = ref(0)
let dictBase = ''
let dictFinals: string[] = []
let dictInterim = ''
let dictSession = 0
let dictStream: MediaStream | null = null
let dictCtx: AudioContext | null = null
let dictOff: UnlistenFn | undefined
let dictDone: (() => void) | null = null

function dictRender() {
  const heard = joinFinals(dictFinals) + (dictInterim ? (dictFinals.length ? ' ' : '') + dictInterim : '')
  text.value = dictBase + (dictBase && heard ? '\n' : '') + heard
}

function dictTeardown() {
  dictStream?.getTracks().forEach((t) => t.stop())
  void dictCtx?.close().catch(() => {}) // 忽略：音频上下文可能已关闭
  dictStream = null
  dictCtx = null
  dictLevel.value = 0
}

async function dictate() {
  if (recording.value) {
    // 停止：不再录，等后端把没收尾的那一句也识别完（done）
    dictTeardown()
    progress.value = '正在收尾…'
    if (dictSession) void api.writeDictateStop(dictSession)
    await new Promise<void>((ok) => {
      dictDone = ok
      setTimeout(ok, 30000)
    })
    return
  }
  error.value = ''
  try {
    const info = await api.writeDictateStart(dictEngine.value)
    dictSession = info.session
    dictLabel.value = info.engine
    dictBase = text.value.trim()
    dictFinals = []
    dictInterim = ''
    dictOff?.()
    dictOff = await ev.on(api.onWriteDictate((p) => {
      if (p.session !== dictSession) return
      if (p.kind === 'interim') {
        dictInterim = p.text
        dictRender()
      } else if (p.kind === 'final') {
        dictInterim = ''
        if (p.text) dictFinals.push(p.text)
        dictRender()
      } else if (p.kind === 'error') {
        error.value = p.text
      } else if (p.kind === 'done') {
        dictInterim = ''
        dictRender()
        recording.value = false
        progress.value = ''
        dictSession = 0
        dictDone?.()
        dictDone = null
        if (!dictFinals.length && !error.value) error.value = '没有听清，请再说一次'
      }
    }))
    try {
      dictStream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } })
    } catch (e) {
      void api.writeDictateStop(info.session)
      throw new Error(`无法使用麦克风：${errorText(e)}`)
    }
    dictCtx = new AudioContext()
    const src = dictCtx.createMediaStreamSource(dictStream)
    // ScriptProcessor 虽然标了“已弃用”，但在 WebView2 / WebKit 里都能用，不需要额外的 worklet 文件
    const proc = dictCtx.createScriptProcessor(4096, 1, 1)
    const rate = dictCtx.sampleRate
    const session = info.session
    proc.onaudioprocess = (e) => {
      const ch = new Float32Array(e.inputBuffer.getChannelData(0))
      dictLevel.value = pcmLevel(ch)
      const pcm = int16ToBase64(floatToInt16(resampleTo16k(ch, rate)))
      void api.writeDictatePush(session, pcm).catch(() => {}) // 忽略：丢一块音频无碍，识别会继续
    }
    src.connect(proc)
    proc.connect(dictCtx.destination)
    recording.value = true
  } catch (e) {
    dictTeardown()
    error.value = errorText(e)
  }
}

async function runSel(kind: 'define' | 'search' | 'summarize') {
  if (!canRun.value) return
  busy.value = true
  error.value = ''
  outs.value = []
  issues.value = []
  selMode.value = true
  selTitle.value = kind === 'define' ? '释义' : kind === 'search' ? 'AI 搜索' : '总结'
  progress.value = '正在处理…'
  try {
    const t = text.value.trim()
    const word = kind === 'define' && isWord(t)
    const req = word ? { kind: 'lookup', text: t, context: '', title: src.value?.app ?? '' } : kind === 'define' ? { kind: 'explain', text: t, context: '', title: src.value?.app ?? '' } : { kind: 'answer', format: kind === 'summarize' ? 'summarize' : 'search', text: t, context: '', title: src.value?.app ?? '' }
    jobId = await api.writeRun(req)
    for (;;) {
      await new Promise((r) => setTimeout(r, 450))
      const j = await api.writeJob(jobId)
      if (!j) throw new Error('任务已过期')
      if (j.state === 'error') throw new Error(j.error ?? '处理失败')
      if (j.state === 'done') {
        const r = (j.result ?? {}) as Record<string, string>
        const txt = word
          ? r.offline
            ? `清影里没有可用的 AI 模型，只能显示词汇等级：${r.level ?? ''}`
            : [`${r.lemma || t}　${r.phonetic || ''}`.trim(), [r.pos, r.meaning].filter(Boolean).join(' '), r.context_meaning && `在这句话里：${r.context_meaning}`, r.note].filter(Boolean).join('\n')
          : (r.text ?? '')
        if (!txt) throw new Error('没有得到结果')
        outs.value = [{ text: txt }]
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

function speakText() {
  try {
    if (speaking.value) {
      speechSynthesis.cancel()
      speaking.value = false
      return
    }
    const t = text.value.trim()
    if (!t) return
    const u = new SpeechSynthesisUtterance(t.slice(0, 2000))
    u.lang = speechLang(t)
    u.onend = () => (speaking.value = false)
    u.onerror = () => (speaking.value = false)
    speechSynthesis.cancel()
    speechSynthesis.speak(u)
    speaking.value = true
  } catch {
    error.value = '这台电脑不能朗读'
  }
}

async function addExcerpt() {
  const t = text.value.trim()
  if (!t) return
  try {
    await api.excerptAdd(t, src.value?.app ?? '')
    done.value = '已存入摘录'
    excerpts.value = await api.excerptList()
  } catch (e) {
    error.value = errorText(e)
  }
}
async function toggleEx() {
  showEx.value = !showEx.value
  if (showEx.value) excerpts.value = await api.excerptList().catch(() => []) // 忽略：可选数据，取不到就按空处理
}
async function dropEx(e: Excerpt) {
  await api.excerptRemove(e.id)
  excerpts.value = excerpts.value.filter((x) => x.id !== e.id)
}
async function copyExAll() {
  await navigator.clipboard.writeText(await api.excerptMarkdown())
  done.value = '摘录已复制为 Markdown'
}

async function stop() {
  if (jobId) await api.writeCancel(jobId)
}

async function apply(replace: boolean) {
  if (!out.value) return
  try {
    done.value = await api.writeApply(out.value, replace, src.value?.app)
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
  void api.writePrefs(a.id, extras.value, autoClose.value).catch(toastErr('保存偏好'))
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
  themeMode = (await api.getSettings().catch(() => null))?.theme ?? 'system' // 忽略：可选数据，取不到就按空处理
  applyTheme()
  media.addEventListener('change', applyTheme)
  cfg.value = await api.writeSettings().catch(() => null) // 忽略：可选数据，取不到就按空处理
  if (cfg.value) {
    pin.value = cfg.value.pin
    autoClose.value = cfg.value.autoClose
    extras.value = cfg.value.extras ?? []
    action.value = ACTIONS.find((a) => a.id === cfg.value?.lastAction) ?? ACTIONS[0]
  }
  watch([text, () => form.value.context], scheduleRead)
  unlisten = await ev.on(listen('write://new', () => void load()))
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
          <span v-if="src?.app" class="from" data-tauri-drag-region>{{ app.name }}{{ isReply && who ? ` · ${who}` : '' }}</span>
        </div>
        <button class="ib" :class="{ on: pin }" :title="pin ? '已固定在最前，点击取消' : '固定在最前'" :aria-label="pin ? '取消固定' : '固定在最前'" @click="setPin">
          <svg viewBox="0 0 24 24"><path d="M9 4h6l-1 6 3 3v2h-5v5l-1 1-1-1v-5H5v-2l3-3z" /></svg>
        </button>
        <button class="ib" title="设置" aria-label="设置" @click="openSettings">
          <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M19 12a7 7 0 0 0-.1-1.2l2-1.5-2-3.4-2.3 1a7 7 0 0 0-2-1.2L14 3h-4l-.6 2.7a7 7 0 0 0-2 1.2l-2.3-1-2 3.4 2 1.5A7 7 0 0 0 5 12c0 .4 0 .8.1 1.2l-2 1.5 2 3.4 2.3-1a7 7 0 0 0 2 1.2L10 21h4l.6-2.7a7 7 0 0 0 2-1.2l2.3 1 2-3.4-2-1.5c.1-.4.1-.8.1-1.2z" /></svg>
        </button>
        <button class="ib close" title="关闭 (Esc)" aria-label="关闭" @click="api.writeClose()">
          <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </header>
      <main>
        <nav class="tabs" role="tablist">
          <button v-for="t in TABS" :key="t.id" class="tab" role="tab" :aria-selected="tab === t.id" :class="{ on: tab === t.id }" @click="pickTab(t.id)">{{ t.name }}</button>
        </nav>

        <!-- 帮我回：收到的消息 + 看懂了 -->
        <section v-if="isReply" class="box recv">
          <div class="recvhd">
            <span class="lbl">收到的消息</span>
            <span class="sp" />
            <button class="lnk" @click="showCtx = !showCtx">{{ showCtx ? '收起前面的聊天' : '＋ 贴上前面的聊天' }}</button>
          </div>
          <textarea v-model="text" class="src msg" rows="2" placeholder="选中收到的消息后按快捷键，或粘贴在这里" aria-label="收到的消息" />
          <textarea v-if="showCtx" v-model="form.context" class="src ctx" rows="3" placeholder="前面的几条聊天（只用来理解，不会回复它们）" aria-label="前面的聊天" />
          <div v-if="reading || look" class="look">
            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 8v5M12 16v.5" /></svg>
            <span v-if="look">看懂了：<b>{{ look.ask }}</b>{{ look.rest }}</span>
            <span v-else>正在看对方要什么…</span>
          </div>
        </section>

        <!-- 其他标签：要处理的文字 -->
        <section v-else class="box">
          <textarea v-model="text" class="src" :class="{ live: recording }" :readonly="recording" rows="4" placeholder="选中文字后按快捷键，或在这里粘贴要处理的文字" aria-label="要处理的文字" />
          <div class="boxbar">
            <span class="cnt">{{ hasText ? `${charCount} 字` : '还没有文字' }}</span>
            <span class="sp" />
            <button class="lnk" :disabled="busy" @click="pasteIn">粘贴</button>
            <button class="lnk" :disabled="!hasText || busy" @click="text = ''">清空</button>
            <span v-if="recording" class="meter" :title="dictLabel"><i :style="{ width: Math.round(dictLevel * 100) + '%' }" /></span>
            <span v-if="recording" class="cnt">实时听写 · {{ dictLabel }}</span>
            <CcSelect v-else v-model="dictEngine" :options="ENGINE_OPTS" size="s" width="m" aria-label="听写引擎" />
            <button class="lnk mic" :class="{ rec: recording }" :disabled="busy && !recording" @click="dictate">{{ recording ? '■ 停止' : '听写' }}</button>
          </div>
          <div v-if="hist.length > 1" class="histrow">
            <span>最近</span>
            <button v-for="h in hist.slice(1, 4)" :key="h" class="chip tiny" :title="h" @click="text = h">{{ h.length > 10 ? h.slice(0, 10) + '…' : h }}</button>
          </div>
        </section>
        <small v-if="note && !hasText" class="hint">{{ note }}</small>

        <!-- 帮我回：对方是谁 × 我的意思 × 怎么说 -->
        <template v-if="isReply">
          <section class="grp">
            <span class="lbl">对方是</span>
            <div class="picks">
              <button v-for="r in RELATIONS" :key="r.id" class="pk" :class="{ on: form.relation === r.id }" :aria-pressed="form.relation === r.id" @click="pickRelation(r.id)">{{ r.name }}</button>
            </div>
          </section>
          <section class="grp">
            <span class="lbl">我的意思</span>
            <div class="picks">
              <button v-for="i in INTENTS" :key="i.id" class="pk" :class="{ on: form.intent === i.id }" :aria-pressed="form.intent === i.id" @click="pickIntent(i.id)">{{ i.name }}</button>
            </div>
            <label v-if="field" class="fld" :class="{ need: field.required && !form.detail.trim() }">
              <span>{{ field.label }}</span>
              <input v-model="form.detail" :placeholder="field.placeholder" maxlength="200" />
            </label>
            <div v-if="form.intent === 'later'" class="mini">
              <button v-for="w in WHEN" :key="w" class="tw" :class="{ on: form.detail === w }" @click="form.detail = w">{{ w }}</button>
            </div>
            <div v-if="form.intent === 'decline'" class="mini">
              <span class="ml">给个替代</span>
              <span class="seg3">
                <button v-for="a in ALTS" :key="a.id" :class="{ on: form.alt === a.id }" @click="form.alt = a.id">{{ a.name }}</button>
              </span>
            </div>
            <div v-if="form.intent === 'ask'" class="qs">
              <span class="ml">要问的（自动找出没说清的地方，可以勾掉或自己加）</span>
              <span v-if="!form.questions.length" class="ml">{{ reading ? '正在找没说清的地方…' : '没找到含糊的地方，自己加一个吧' }}</span>
              <label v-for="(q, i) in form.questions" :key="i" class="q"><input v-model="q.on" type="checkbox" /><span :class="{ off: !q.on }">{{ q.text }}</span></label>
              <div class="qadd">
                <input v-model="newQ" placeholder="＋ 自己加一个问题" maxlength="100" aria-label="自己加一个问题" @keydown.enter.prevent="addQuestion" />
                <button v-if="newQ.trim()" class="lnk" @click="addQuestion">加上</button>
              </div>
            </div>
          </section>
          <section class="grp">
            <span class="lbl">怎么说</span>
            <div class="picks">
              <button v-for="t in TONES" :key="t.id" class="pk" :class="{ on: form.tone === t.id }" :aria-pressed="form.tone === t.id" @click="form.tone = t.id">{{ t.name }}</button>
            </div>
            <small v-if="risky" class="warn"><svg class="wi" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4 2.5 20h19zM12 10v4.5M12 17.5v.5" /></svg>对上级、客户开玩笑可能不合适，确定要用“幽默接话”吗？</small>
            <div class="lenrow">
              <span>长度</span>
              <span class="seg3">
                <button v-for="l in LENGTHS" :key="l.id" :class="{ on: form.length === l.id }" @click="form.length = l.id">{{ l.name }}</button>
              </span>
              <input v-model="form.extra" class="say" placeholder="还想说（可选）" maxlength="200" aria-label="还想说" />
            </div>
          </section>
        </template>

        <!-- 改写润色 -->
        <template v-else-if="tab === 'rewrite'">
          <div class="seg">
            <span class="lbl">怎么改</span>
            <CcSegmented :model-value="action.id" :options="actionOpts" size="s" aria-label="怎么改" @update:model-value="pickActionId" />
          </div>
          <section class="grp">
            <span class="lbl">怎么说（可不选）</span>
            <div class="picks">
              <button v-for="t in TONES" :key="t.id" class="pk" :class="{ on: rwTone === t.id }" :aria-pressed="rwTone === t.id" @click="rwTone = rwTone === t.id ? '' : t.id">{{ t.name }}</button>
            </div>
          </section>
          <section class="opts">
            <CcSelect v-model="presetId" :options="presetOpts" placeholder="场景：通用" size="s" width="m" aria-label="场景" />
            <input v-model="extra" class="ex" placeholder="额外要求（可选）" maxlength="200" />
          </section>
          <div v-if="extras.length" class="recent">
            <button v-for="e in extras.slice(0, 4)" :key="e" class="chip tiny" :title="e" @click="extra = e">{{ e.length > 12 ? e.slice(0, 12) + '…' : e }}</button>
          </div>
        </template>

        <!-- 人设 -->
        <template v-else-if="tab === 'persona'">
          <section class="grp">
            <span class="lbl">选一个人设</span>
            <div class="personas">
              <button v-for="p in personas" :key="p.id" class="pcard" :class="{ on: personaId === p.id }" :aria-pressed="personaId === p.id" :title="p.desc" @click="personaId = p.id">
                <b>{{ p.name }}</b>
                <small>{{ p.desc }}</small>
              </button>
            </div>
          </section>
          <small class="hint">{{ persona ? persona.hint : '人设只是说话的风格：不辱骂、不歧视、不编造，恋爱类的不教操控和伪造身份。' }}</small>
          <section class="opts">
            <input v-model="extra" class="ex" placeholder="额外要求（可选，比如：别太长、带点东北话）" maxlength="200" />
          </section>
        </template>

        <!-- 检查 -->
        <small v-else-if="tab === 'check'" class="hint">找出错别字、语病、标点和不通顺的地方，逐条给出改法。</small>

        <!-- 翻译 · 更多 -->
        <template v-else>
          <div class="seg">
            <span class="lbl">怎么改</span>
            <CcSegmented :model-value="action.id" :options="actionOpts" size="s" aria-label="怎么改" @update:model-value="pickActionId" />
          </div>
          <section class="opts">
            <input v-if="action.id === 'translate'" v-model="target" class="tg" placeholder="目标语言" aria-label="目标语言" />
            <input v-model="extra" class="ex" placeholder="额外要求（可选）" maxlength="200" />
          </section>
          <section class="selrow">
            <span class="sl">对选中的文字</span>
            <button class="chip" :disabled="!canRun" @click="runSel('define')">释义</button>
            <button class="chip" :class="{ on: speaking }" :disabled="!hasText" @click="speakText">{{ speaking ? '■ 停止朗读' : '朗读' }}</button>
            <button class="chip" :disabled="!canRun" @click="runSel('search')">AI 搜索</button>
            <button class="chip" :disabled="!hasText" title="存入摘录（桌面端没法给别的软件里的文字画高亮）" @click="addExcerpt">存入摘录</button>
            <button class="chip" :class="{ on: showEx }" @click="toggleEx">摘录列表</button>
            <button class="chip" :class="{ on: showFavs }" @click="showFavs = !showFavs">我的收藏</button>
          </section>
          <section v-if="showEx" class="exlist">
            <div class="exhd"><b>摘录（{{ excerpts.length }}）</b><span class="sp" /><button class="lnk" :disabled="!excerpts.length" @click="copyExAll">复制为 Markdown</button></div>
            <div v-if="!excerpts.length" class="hint">还没有摘录。选中文字，点“存入摘录”。</div>
            <div v-for="e in excerpts.slice(0, 30)" :key="e.id" class="exit">
              <div class="ext">{{ e.text.length > 120 ? e.text.slice(0, 120) + '…' : e.text }}</div>
              <div class="exm"><span>{{ e.app }}</span><button class="lnk" @click="text = e.text">放回输入框</button><button class="lnk" @click="dropEx(e)">删除</button></div>
            </div>
          </section>
          <section v-if="showFavs" class="exlist favs">
            <div class="exhd"><b>我的收藏（{{ favList.length }}）</b></div>
            <div v-if="!favList.length" class="hint">还没有收藏。结果旁边的 ☆ 可以收藏满意的版本。</div>
            <div v-for="f in favList" :key="f" class="exit">
              <div class="ext">{{ f.length > 160 ? f.slice(0, 160) + '…' : f }}</div>
              <div class="exm"><button class="lnk" @click="navCopy(f)">复制</button><button class="lnk" @click="dropFav(f)">取消收藏</button></div>
            </div>
          </section>
        </template>

        <div class="runrow">
          <button v-if="!busy" class="go" :disabled="!canRun" @click="run">{{ isReply ? '生成回复' : action.name }}<kbd>Ctrl ↵</kbd></button>
          <button v-else class="go stop" @click="stop"><i class="spin" />取消</button>
          <span v-if="busy" class="hint prog">{{ progress }}</span>
          <span v-else-if="missing && hasText" class="hint prog">还差：{{ missing }}</span>
        </div>
        <div v-if="error" class="errbox" role="alert">
          <p class="err">{{ error }}</p>
          <div class="row">
            <CcButton kind="secondary" size="s" @click="run">重试</CcButton>
            <CcButton kind="secondary" size="s" @click="openSettings">{{ errHint ? '去设置模型' : '打开设置' }}</CcButton>
          </div>
        </div>
        <section v-else-if="busy && !outs.length" class="res" aria-busy="true"><CcSkeleton :lines="3" height="14" label="正在生成…" /></section>
        <section v-if="outs.length" class="res">
          <div class="reshd">
            <b>{{ selMode ? selTitle : isReply ? '回复' : '结果' }}</b>
            <div v-if="outs.length > 1" class="vers">
              <button v-for="(o, i) in outs" :key="i" class="vt" :class="{ on: picked === i, lab: !!o.label }" :title="`Alt+${i + 1}`" @click="picked = i">{{ o.label || i + 1 }}</button>
            </div>
            <span class="sp" />
            <span class="cnt">{{ outCount }} 字<em v-if="delta && !isReply" :class="delta > 0 ? 'up' : 'dn'">{{ delta > 0 ? '+' : '' }}{{ delta }}</em></span>
          </div>
          <div v-if="compare" class="out orig"><small>原文</small>{{ text }}</div>
          <div class="out" :class="{ big: isReply }">{{ out }}</div>
          <small v-if="guardText" class="warn"><svg class="wi" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4 2.5 20h19zM12 10v4.5M12 17.5v.5" /></svg>{{ guardText }}</small>
          <ul v-if="issues.length" class="issues">
            <li v-for="(it, i) in issues" :key="i"><s>{{ it.from }}</s> → <b>{{ it.to }}</b> <small>{{ it.reason }}</small></li>
          </ul>
          <small v-else-if="action.grammar && !isReply" class="hint">没有发现需要修改的地方</small>
          <div v-if="isReply && !selMode" class="tweaks">
            <span>微调</span>
            <button v-for="t in TWEAKS" :key="t.id" class="tw" :disabled="busy" @click="tweak(t.id)">{{ t.name }}</button>
          </div>
          <div class="row">
            <button v-if="src?.canReplace && !selMode" class="go sm" @click="apply(true)">{{ replaceLabel }}</button>
            <button class="ghost" @click="apply(false)">复制</button>
            <button v-if="!selMode" class="ghost" @click="run">再来一版</button>
            <button v-if="!selMode && !isReply" class="ghost" :class="{ on: compare }" @click="compare = !compare">对比</button>
            <span class="sp" />
            <button class="ghost star" :class="{ on: faved }" :title="faved ? '取消收藏' : '收藏这个版本'" :aria-label="faved ? '取消收藏' : '收藏这个版本'" @click="toggleFav">{{ faved ? '★' : '☆' }}</button>
          </div>
          <small v-if="isReply && !selMode" class="hint fine">只用你给的条件和时间，不替你编理由、不多承诺。</small>
          <div v-if="done || canUndo || (src?.canReplace && !selMode)" class="row after">
            <span v-if="done" class="ok">✓ {{ done }}</span>
            <button v-if="canUndo" class="lnk" @click="undo">撤销</button>
            <label v-if="src?.canReplace" class="auto"><input type="checkbox" :checked="autoClose" @change="toggleAuto" /> 写回后自动关闭</label>
          </div>
        </section>
      </main>
      <footer class="foot">
        <span>快捷键 <kbd>{{ hotkey || '未设置' }}</kbd></span>
        <CcLink @click.prevent="openSettings">修改</CcLink>
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
/* 窗口本身是透明的（阴影必须在窗口留白 --m 以内淡出，超出窗口边界会被切成一个看得见的方框）：页面底色一律透明，只有里面那张圆角卡片有颜色，窗口四周不会再有任何多余的底层容器 */
html, body, #app { background: transparent !important; overflow: hidden; }
.personas { display: grid; grid-template-columns: 1fr 1fr; gap: 6px }
.pcard { display: flex; flex-direction: column; gap: 2px; align-items: flex-start; text-align: left; padding: 6px 8px; border: 1px solid var(--line); border-radius: 8px; background: transparent; color: inherit; cursor: pointer; font: inherit }
.pcard b { font-size: 13px }
.pcard small { font-size: 11px; opacity: .7; line-height: 1.3 }
.pcard.on { border-color: var(--acc); background: var(--pick) }
</style>

<style scoped>
.shell { --m: 14px; position: relative; height: 100vh; padding: var(--m); box-sizing: border-box; }
.wa { --pick: var(--cc-acc-text); --go: var(--cc-acc-strong); --gofg: var(--cc-on-acc); --look: var(--cc-fg2); --dash: var(--cc-line-strong); --acc: var(--cc-acc-strong); --accd: var(--cc-acc-text); --soft: var(--cc-acc-soft); --line: var(--cc-line); --bg: var(--cc-bg); --card: var(--cc-card); --mut: var(--cc-mute); --err: var(--cc-err); --ok: var(--cc-ok); --fg: var(--cc-fg); --edge: var(--cc-line-strong); --shadow: var(--cc-shadow);
  height: 100%; display: flex; flex-direction: column; background: var(--card); color: var(--fg); border-radius: 14px; border: 1px solid var(--edge); box-shadow: var(--shadow); overflow: hidden; font: 13px/1.55 -apple-system, 'PingFang SC', 'Microsoft YaHei', 'Noto Sans SC', sans-serif; }
.bar { display: flex; align-items: center; gap: 6px; padding: 10px 10px 8px 14px; cursor: grab; user-select: none; background: var(--bg); border-bottom: 1px solid var(--line); }
.bar:active { cursor: grabbing; }
.logo { width: 24px; height: 24px; border-radius: 8px; background: var(--acc); color: var(--gofg); display: grid; place-items: center; font: 700 12px/1 inherit; flex: none; }
.ttl { display: flex; align-items: center; gap: 8px; min-width: 0; flex: 1; }
.ttl b { font-size: 14px; }
.from { font-size: 11px; line-height: 20px; padding: 0 9px; border-radius: 999px; background: var(--card); border: 1px solid var(--line); color: var(--mut); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 190px; }
.ib { border: 0; background: none; width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; cursor: pointer; color: var(--mut); }
.ib svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.ib:hover { background: var(--bg); color: var(--fg); }
.ib.on { color: var(--acc); }
.ib.on svg { fill: currentColor; fill-opacity: 0.25; }
.ib.close:hover { background: var(--err); color: var(--cc-on-err); }
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
.seg { display: flex; flex-direction: column; gap: 6px; }
.seg .cc-seg { width: 100%; }
.seg .seg-item { flex: 1; }
.more-grid { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
.chip { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 999px; padding: 2px 11px; cursor: pointer; font: inherit; font-size: 12px; }
.chip:hover { border-color: var(--acc); color: var(--accd); }
.chip.on { border-color: var(--acc); color: var(--accd); background: var(--soft); font-weight: 600; }
.chip.tiny { padding: 0 8px; font-size: 11px; color: var(--mut); line-height: 20px; max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.opts { display: flex; gap: 6px; }
.opts select, .opts input { min-width: 0; padding: 6px 9px; border-radius: 8px; border: 1px solid var(--line); background: var(--card); color: inherit; font: inherit; outline: none; }
.opts select { flex: 0 0 auto; max-width: 130px; }
.opts .ex { flex: 1; }
.opts .tg { width: 90px; }
.opts select:focus, .opts input:focus { border-color: var(--acc); }
.recent { display: flex; gap: 6px; flex-wrap: wrap; margin-top: -4px; }
.runrow { display: flex; align-items: center; gap: 10px; }
.go { flex: 1; display: flex; align-items: center; justify-content: center; gap: 8px; background: var(--go); color: var(--gofg); border: 0; border-radius: 10px; padding: 9px 14px; cursor: pointer; font: inherit; font-weight: 600; font-size: 14px; }
.go kbd { font: 11px/1 inherit; opacity: 0.8; border: 1px solid currentColor; border-radius: 4px; padding: 2px 5px; font-weight: 400; }
.go:hover:not(:disabled) { filter: brightness(1.08); }
.go:disabled { background: var(--cc-fill); color: var(--cc-mute); cursor: not-allowed; }
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
textarea.src.live { color: var(--mut) }
.meter { width: 46px; height: 6px; border-radius: 3px; background: var(--line); overflow: hidden; display: inline-block }
.meter i { display: block; height: 100%; background: var(--acc); transition: width .08s }
button:focus-visible, select:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--acc); outline-offset: 1px; }
.hint { color: var(--mut); }
.err { color: var(--err); margin: 0; }
.errbox { display: flex; flex-direction: column; gap: 6px; background: color-mix(in srgb, var(--err) 9%, transparent); border: 1px solid color-mix(in srgb, var(--err) 35%, transparent); border-radius: 10px; padding: 8px 10px; }
.warn { color: var(--cc-warn-text); }
.ok { color: var(--ok); }
.wi { width: 13px; height: 13px; vertical-align: -2px; margin-right: 4px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
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
.selrow { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; padding: 2px 0 }
.selrow .sl { color: var(--mut); font-size: 12px; margin-right: 2px }
.exlist { border: 1px solid var(--line); border-radius: 10px; padding: 8px 10px; background: var(--bg); display: flex; flex-direction: column; gap: 6px; max-height: 190px; overflow: auto }
.exhd { display: flex; align-items: center; gap: 8px }
.exit { border-top: 1px solid var(--line); padding-top: 5px }
.ext { font-size: 12.5px; line-height: 1.5 }
.exm { display: flex; gap: 10px; align-items: center; color: var(--mut); font-size: 11.5px }

/* ---------- 四个标签 ---------- */
.tabs { display: flex; gap: 2px; background: var(--bg); border: 1px solid var(--line); border-radius: 10px; padding: 3px; flex: none; }
.tab { flex: 1; border: 0; background: none; color: var(--mut); border-radius: 7px; height: 30px; font: inherit; font-size: 13px; cursor: pointer; white-space: nowrap; }
.tab:hover { color: var(--fg); }
.tab.on { background: var(--card); color: var(--pick); font-weight: 600; box-shadow: var(--cc-sh-1); }
/* ---------- 帮我回 ---------- */
.lbl { font-size: 12px; color: var(--mut); font-weight: 500; }
.recv { padding: 9px 11px 8px; gap: 6px; }
.recvhd { display: flex; align-items: center; gap: 6px; }
.recv .src { padding: 0; min-height: 0; }
.recv .src.msg { min-height: 44px; }
.recv .src.ctx { border-top: 1px dashed var(--dash); padding-top: 6px; color: var(--mut); font-size: 12.5px; }
.look { display: flex; gap: 6px; align-items: flex-start; border-top: 1px dashed var(--dash); padding-top: 7px; font-size: 12px; color: var(--look); }
.look svg { width: 14px; height: 14px; flex: none; margin-top: 3px; fill: none; stroke: var(--acc); stroke-width: 2; stroke-linecap: round; }
.look b { color: var(--fg); font-weight: 600; }
.grp { display: flex; flex-direction: column; gap: 7px; }
.picks { display: flex; flex-wrap: wrap; gap: 6px; }
.pk { border: 1px solid var(--line); background: var(--card); color: var(--fg); border-radius: 999px; padding: 0 12px; height: 30px; font: inherit; font-size: 12.5px; white-space: nowrap; cursor: pointer; }
.pk:hover { border-color: var(--acc); }
.pk.on { border-color: var(--acc); background: var(--soft); color: var(--pick); font-weight: 600; }
.fld { display: flex; align-items: center; gap: 8px; border: 1px solid var(--line); border-radius: 8px; padding: 0 10px; height: 36px; background: var(--card); }
.fld:focus-within { border-color: var(--acc); box-shadow: 0 0 0 3px color-mix(in srgb, var(--acc) 15%, transparent); }
.fld.need { border-color: var(--acc); }
.fld span { font-size: 12px; color: var(--pick); font-weight: 600; white-space: nowrap; }
.fld input { flex: 1; min-width: 0; border: 0; outline: none; font: inherit; color: var(--fg); background: none; }
.mini { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
.ml { font-size: 12px; color: var(--mut); }
.qs { border: 1px solid var(--line); border-radius: 10px; padding: 9px 11px; display: flex; flex-direction: column; gap: 7px; background: var(--bg); }
.q { display: flex; gap: 8px; align-items: flex-start; font-size: 12.5px; cursor: pointer; }
.q input { margin: 3px 0 0; accent-color: var(--acc); width: 14px; height: 14px; flex: none; }
.q .off { color: var(--mut); }
.qadd { display: flex; gap: 6px; align-items: center; }
.qadd input { flex: 1; min-width: 0; border: 0; border-bottom: 1px dashed transparent; background: none; color: var(--fg); font: inherit; font-size: 12px; padding: 2px 0; outline: none; }
.qadd input::placeholder { color: var(--pick); }
.qadd input:focus { border-bottom-color: var(--dash); }
.lenrow { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--mut); }
.seg3 { display: inline-flex; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; flex: none; }
.seg3 button { border: 0; background: var(--card); height: 26px; padding: 0 10px; font: inherit; font-size: 12px; color: var(--mut); cursor: pointer; }
.seg3 button + button { border-left: 1px solid var(--line); }
.seg3 button.on { background: var(--soft); color: var(--pick); font-weight: 600; }
.say { flex: 1; min-width: 0; border: 0; border-bottom: 1px dashed var(--dash); background: none; color: var(--fg); font: inherit; font-size: 12px; padding: 2px 0; outline: none; }
.tw { border: 1px dashed var(--dash); background: none; border-radius: 999px; height: 26px; padding: 0 10px; font: inherit; font-size: 12px; color: var(--look); cursor: pointer; }
.tw:hover:not(:disabled), .tw.on { border-style: solid; border-color: var(--acc); color: var(--pick); }
.tw:disabled { opacity: 0.5; cursor: default; }
.tweaks { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; font-size: 12px; color: var(--mut); }
.vt.lab { width: auto; height: 28px; padding: 0 10px; border-radius: 8px; font-size: 12px; background: var(--card); }
.vt.lab.on { background: var(--soft); color: var(--pick); }

.out.big { font-size: var(--cc-fs-md); line-height: 1.7; background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; }
html.dark .shell .out.big { background: var(--bg); }
.fine { font-size: 11.5px; }
.favs .exm { justify-content: flex-end; }
</style>
