<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { AppGlossaryPack, OcrStatus, ScreenEntry, ScreenSettings, ScreenStatus, ScreenTarget, ScreenTry, ScreenWindow } from '../types'
import { formatBytes, formatDateTime } from '../utils/format'
import { clone, merge3 } from '../utils/merge3'

/** 实时屏幕翻译：翻译别的软件和游戏画面上的文字。控制在这里，也可以用悬浮钮、托盘和快捷键。 */
const app = useAppStore()
const form = ref<ScreenSettings>(JSON.parse(JSON.stringify(app.settings!.live)) as ScreenSettings)
// 最近一次同步的整份设置：只把“屏幕翻译”这一块当作自己的修改交给后端合并（框选位置、程序配置等由后端写，不会被覆盖）
let baseAll = clone(app.settings!)
watch(
  () => app.settingsRev,
  () => {
    if (!app.settings) return
    const merged = merge3(baseAll.live, form.value, app.settings.live) as ScreenSettings
    baseAll = clone(app.settings)
    if (JSON.stringify(merged) !== JSON.stringify(form.value)) form.value = merged
  },
)
const st = ref<ScreenStatus>({ supported: true, running: false, mode: 'standard', target: null, hidden: false, paused: false, msg: '', audio: false })
const wins = ref<ScreenWindow[]>([])
const winPick = ref<number | ''>('')
const langs = ref<{ tag: string; name: string }[]>([])
const ocr = ref<OcrStatus | null>(null)
const dl = ref<{ got: number; total: number } | null>(null)
const tried = ref<ScreenTry | null>(null)
const trying = ref(false)
const hist = ref<ScreenEntry[]>([])
const tab = ref<'main' | 'ocr' | 'sub' | 'manga' | 'audio' | 'apps' | 'terms' | 'history'>('main')
let offs: UnlistenFn[] = []

const MODES: { id: ScreenSettings['mode']; name: string; desc: string }[] = [
  { id: 'standard', name: '标准', desc: '画面上的文字块被译文盖住，适合游戏界面、软件菜单、网页和文档。' },
  { id: 'subtitle', name: '字幕', desc: '只盯字幕区，译文稳定后才显示；换句子时去重、跨屏合并，适合看视频。' },
  { id: 'manga', name: '漫画', desc: '用视觉模型整页识别、翻译，竖排原文译成竖排，横排译成横排；翻页自动清除。' },
]
const OCRS: { id: ScreenSettings['ocr']; name: string }[] = [
  { id: 'system', name: 'Windows 自带（快、不用下载）' },
  { id: 'enhanced', name: '增强识字组件（更准，需要下载一次）' },
  { id: 'api', name: '云端识字服务（自带密钥）' },
  { id: 'vision', name: '视觉模型（直接看图，会上传图片）' },
]

const targetText = computed(() => {
  const t = st.value.target
  if (!t) return '还没有选'
  const where = t.kind === 'window' ? `窗口：${t.title || t.exe || '未命名'}` : `区域 ${t.rect[2]}×${t.rect[3]}`
  return t.exe && t.kind !== 'window' ? `${where}（${t.exe}）` : where
})

let timer = 0
watch(
  form,
  () => {
    window.clearTimeout(timer)
    timer = window.setTimeout(save, 400)
  },
  { deep: true },
)

async function save() {
  try {
    await app.save({ ...baseAll, live: form.value }, baseAll)
    baseAll = clone(app.settings!)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function refresh() {
  try {
    st.value = await api.scStatus()
  } catch {
    /* 非桌面环境 */
  }
}
async function loadHist() {
  hist.value = await api.scHistory(200).catch(() => [])
}
async function loadWins() {
  wins.value = await api.scWindows().catch(() => [])
}

onMounted(async () => {
  await refresh()
  void loadWins()
  void loadHist()
  void loadPacks()
  langs.value = await api.scOcrLangs().catch(() => [])
  experts.value = (await api.expertsState().catch(() => null))?.experts ?? []
  ocr.value = await api.ocrStatus().catch(() => null)
  offs = [
    await api.onScreenState((s) => (st.value = s)),
    await api.onOcrDownload((p) => (dl.value = p.got >= p.total && p.total > 0 ? null : { got: p.got, total: p.total })),
  ]
})
onBeforeUnmount(() => offs.forEach((f) => f()))

async function ensureMangaAck(mode: string): Promise<boolean> {
  if (mode !== 'manga' || form.value.mangaEngine !== 'vision' || form.value.mangaAck) return true
  try {
    await ElMessageBox.confirm('漫画模式会把截到的画面上传给你在“设置 → AI”里选的视觉模型来识别和翻译，其他两种模式默认不上传图片。只对你选定的区域生效，可以随时停止。确认使用吗？', '漫画模式会上传图片', { confirmButtonText: '我知道了，继续', cancelButtonText: '取消', type: 'warning' })
  } catch {
    return false
  }
  form.value.mangaAck = true
  await api.scAckManga().catch(() => {})
  return true
}

async function setMode(m: ScreenSettings['mode']) {
  if (!(await ensureMangaAck(m))) return
  form.value.mode = m
  try {
    st.value = await api.scSetMode(m)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function start(target?: ScreenTarget) {
  if (!(await ensureMangaAck(form.value.mode))) return
  try {
    await save()
    st.value = await api.scStart(target, form.value.mode)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function stop() {
  st.value = await api.scStop().catch(() => st.value)
}
async function pick() {
  try {
    await api.scPick()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function startWindow() {
  const w = wins.value.find((x) => x.hwnd === winPick.value)
  if (!w) return ElMessage.info('先选一个窗口')
  await start({ kind: 'window', rect: [w.x, w.y, w.w, w.h], hwnd: w.hwnd, exe: w.exe, title: w.title })
}
/** 取景方式：整个屏幕（主显示器，物理像素） */
async function startScreen() {
  const dpr = window.devicePixelRatio || 1
  const w = Math.round(window.screen.width * dpr)
  const h = Math.round(window.screen.height * dpr)
  await start({ kind: 'region', rect: [0, 0, w, h], hwnd: 0, exe: '', title: '整个屏幕' })
}
const SPEEDS: [number, string][] = [
  [1, '省电 · 1 帧/秒'],
  [3, '均衡 · 3 帧/秒'],
  [6, '流畅 · 6 帧/秒'],
]
const experts = ref<{ id: string; name: string }[]>([])
const expertId = computed({
  get: () => (app.settings?.ai.expertAuto ? '__auto' : app.settings?.ai.expert || ''),
  set: (v: string) => {
    const ai = app.settings?.ai
    if (!ai) return
    void app.patch({ ai: { ...ai, expertAuto: v === '__auto', expert: v === '__auto' ? ai.expert : v } })
  },
})
const glossaryOn = computed({
  get: () => !!app.settings?.ai.glossaryOn,
  set: (v: boolean) => {
    const ai = app.settings?.ai
    if (ai) void app.patch({ ai: { ...ai, glossaryOn: v } })
  },
})

const packs = ref<AppGlossaryPack[]>([])
async function loadPacks() {
  packs.value = await api.glossaryAppPacks().catch(() => [])
}
async function installPacks(ids: string[]) {
  try {
    const n = await api.glossaryAppInstall(ids)
    ElMessage.success(n ? `已装上 ${n} 个软件术语库，在“设置 → AI → 术语库”里可以改` : '这些已经装过了')
    await loadPacks()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
/** 当前翻译的软件（或最近一次翻译的软件）。 */
const targetExe = computed(() => st.value.target?.exe || '')
async function openAppGlossary() {
  try {
    const g = await api.glossaryForApp(targetExe.value)
    ElMessage.success(`“${g.name}”已就绪，到“设置 → AI → 术语库”里往里加术语`)
    app.goSettings('ai')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function tryOnce() {
  trying.value = true
  tried.value = null
  try {
    tried.value = await api.scTry()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    trying.value = false
  }
}

async function installOcr() {
  try {
    dl.value = { got: 0, total: 0 }
    ocr.value = await api.ocrInstall()
    ElMessage.success('增强识字组件已装好')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    dl.value = null
  }
}
async function importOcr(kind: 'det' | 'rec' | 'dict') {
  const f = await open({ multiple: false, title: kind === 'dict' ? '选择字典（.txt）' : '选择模型（.onnx）' })
  if (typeof f !== 'string') return
  try {
    ocr.value = await api.ocrImport(kind, f)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function removeOcr() {
  ocr.value = await api.ocrRemove().catch(() => ocr.value)
}

function onWinOpen(v: boolean) {
  if (v) void loadWins()
}
function onBar(v: string | number | boolean) {
  void toggleBar(!!v)
}
async function audioOn() {
  try {
    st.value = await api.audioStart()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function audioOff() {
  await api.audioStop().catch(() => {})
  await refresh()
}
async function clearCache() {
  await api.scCacheClear().catch(() => {})
  ElMessage.success('译文缓存已清')
}
async function toggleBar(on: boolean) {
  form.value.floatButton = on
  await api.scBar(on).catch(() => {})
}
function removeProfile(exe: string) {
  form.value.profiles = form.value.profiles.filter((p) => p.exe !== exe)
}
async function clearHist() {
  await api.scHistoryClear().catch(() => {})
  hist.value = []
}
async function copy(t: string) {
  try {
    await navigator.clipboard.writeText(t)
    ElMessage.success('已复制')
  } catch {
    ElMessage.info('复制失败，请手动选中')
  }
}
const progress = computed(() => (dl.value && dl.value.total > 0 ? Math.round((dl.value.got / dl.value.total) * 100) : 0))
const modeName = (m: string) => MODES.find((x) => x.id === m)?.name ?? m
</script>

<template>
  <div class="scr">
    <header class="head">
      <div>
        <h2>屏幕翻译</h2>
        <p class="sub">翻译别的软件和游戏画面上的文字。译文贴在原位，不挡鼠标，也不会被自己再识别一遍。</p>
      </div>
      <div class="state">
        <span class="dot" :class="{ on: st.running }" />
        {{ st.running ? `运行中 · ${modeName(st.mode)}` : '未运行' }}<span v-if="st.msg" class="mute"> · {{ st.msg }}</span>
      </div>
    </header>

    <el-alert v-if="!st.supported" type="info" :closable="false" show-icon title="屏幕翻译目前只支持 Windows。其他系统暂时不能用。" />

    <section class="card">
      <h3>1. 翻译哪里</h3>
      <div class="row">
        <b>{{ targetText }}</b>
        <el-button type="primary" :disabled="!st.supported" @click="pick">框选区域 / 点选窗口</el-button>
        <el-select v-model="winPick" size="default" placeholder="或从窗口列表选" style="width: 260px" filterable clearable @visible-change="onWinOpen">
          <el-option v-for="w in wins" :key="w.hwnd" :value="w.hwnd" :label="`${w.title || w.exe}（${w.exe}）`" />
        </el-select>
        <el-button :disabled="!winPick || !st.supported" @click="startWindow">翻译这个窗口</el-button>
      </div>
      <div class="row">
        <span class="mute">取景方式</span>
        <el-button-group>
          <el-button :disabled="!st.supported" @click="pick">框选区域</el-button>
          <el-button :disabled="!winPick || !st.supported" @click="startWindow">跟随窗口</el-button>
          <el-button :disabled="!st.supported" @click="startScreen">整个屏幕</el-button>
        </el-button-group>
        <small class="mute">“跟随窗口”先在上面的下拉框里选窗口；框选后译文会盖在原处。</small>
      </div>
      <div class="row">
        <span class="mute">刷新速度</span>
        <el-radio-group v-model="form.fps">
          <el-radio-button v-for="[n, t] in SPEEDS" :key="n" :value="n">{{ t }}</el-radio-button>
        </el-radio-group>
        <span class="mute">术语库</span>
        <el-switch v-model="glossaryOn" />
        <span class="mute">专家</span>
        <el-select v-model="expertId" style="width: 180px" placeholder="自动匹配">
          <el-option value="__auto" label="自动匹配" />
          <el-option v-for="e in experts" :key="e.id" :value="e.id" :label="e.name" />
        </el-select>
      </div>
      <div class="row">
        <el-button v-if="!st.running" type="success" :disabled="!st.supported || !st.target" @click="start()">开始（{{ form.hotkey }}）</el-button>
        <el-button v-else type="danger" @click="stop">停止（{{ form.hotkey }}）</el-button>
        <el-button :disabled="!st.supported || !st.target" :loading="trying" @click="tryOnce">试识别一次</el-button>
        <el-button v-if="st.running" @click="api.scPause()">{{ st.paused ? '继续' : '暂停' }}</el-button>
        <el-button v-if="st.running" @click="api.scHide()">{{ st.hidden ? '显示译文' : '隐藏译文' }}（{{ form.hotkeyHide }}）</el-button>
        <el-button v-if="st.running" @click="api.scShot()">保存译图（{{ form.hotkeyShot }}）</el-button>
      </div>
      <div v-if="tried" class="try">
        <div class="mute">用“{{ tried.engine }}”识别 {{ tried.w }}×{{ tried.h }}，{{ tried.ms }} 毫秒，{{ tried.lines.length }} 段：</div>
        <div v-for="(l, i) in tried.lines" :key="i" class="tl">{{ l }}</div>
        <div v-if="!tried.lines.length" class="mute">没认出文字。可以换识字引擎，或检查识字语言是否装了（Windows 设置 → 时间和语言 → 语言和区域）。</div>
      </div>
    </section>

    <section class="card">
      <h3>2. 模式</h3>
      <div class="modes">
        <button v-for="m in MODES" :key="m.id" type="button" class="mode" :class="{ sel: form.mode === m.id }" @click="setMode(m.id)">
          <b>{{ m.name }}</b>
          <small>{{ m.desc }}</small>
        </button>
      </div>
    </section>

    <el-tabs v-model="tab" class="tabs">
      <el-tab-pane label="翻译与显示" name="main">
        <div class="grid">
          <div class="field"><label>译成什么语言</label><el-input v-model="form.targetLang" size="small" placeholder="留空 = 用“设置 → AI”里的目标语言" /></div>
          <div class="field"><label>每秒看几次屏幕：{{ form.fps }}</label><el-slider v-model="form.fps" :min="1" :max="10" /></div>
          <div class="field"><label>画面稳定多久再翻：{{ form.stableMs }} 毫秒</label><el-slider v-model="form.stableMs" :min="100" :max="3000" :step="50" /></div>
          <div class="field"><label>画面变化多少才重新识别：{{ form.changePct }}%</label><el-slider v-model="form.changePct" :min="1" :max="50" /></div>
          <div class="field"><label>文字消失后译文再保留：{{ form.keepMs }} 毫秒</label><el-slider v-model="form.keepMs" :min="0" :max="5000" :step="100" /></div>
          <div class="field"><label>译文最小字号：{{ form.fontMin }}px</label><el-slider v-model="form.fontMin" :min="8" :max="32" /></div>
          <div class="field"><label>译文字号：{{ form.fontScale }}%</label><el-slider v-model="form.fontScale" :min="60" :max="200" :step="5" /></div>
          <div class="field"><label>译文不透明度：{{ form.opacity }}%</label><el-slider v-model="form.opacity" :min="30" :max="100" /></div>
        </div>
        <div class="kv"><span>同时显示原文（小字）</span><el-switch v-model="form.bilingual" /></div>
        <div class="kv"><span>数字、符号、已经是目标语言的文字不翻<small class="mute block">省请求，也避免把专有名词翻坏</small></span><el-switch v-model="form.skipSame" /></div>
        <div class="kv"><span>屏幕边上放一个小悬浮钮<small class="mute block">点开能开始 / 停止、切模式、重新框选、隐藏译文；也可以只用托盘和快捷键</small></span><el-switch :model-value="form.floatButton" @change="onBar" /></div>
        <div class="grid">
          <div class="field"><label>开始 / 停止</label><el-input v-model="form.hotkey" size="small" class="mono" /></div>
          <div class="field"><label>重新框选</label><el-input v-model="form.hotkeyPick" size="small" class="mono" /></div>
          <div class="field"><label>显示 / 隐藏译文</label><el-input v-model="form.hotkeyHide" size="small" class="mono" /></div>
          <div class="field"><label>切换模式</label><el-input v-model="form.hotkeyMode" size="small" class="mono" /></div>
          <div class="field"><label>保存带译文的截图</label><el-input v-model="form.hotkeyShot" size="small" class="mono" /></div>
        </div>
        <small class="mute block">译文贴字层不会出现在系统截图或别的截图软件里（否则会被自己再识别一遍）。想留下带译文的图，用“保存译图”：它会取一帧干净的画面，把译文按现在的排版叠上去，存到“下载文件夹/屏幕翻译截图”，同时复制到剪贴板。</small>
        <small class="mute">快捷键留空表示不使用；与别的程序冲突时会在设置页提示。游戏如果是“独占全屏”，系统截不到画面，请改成“无边框窗口”或“窗口化”。</small>
      </el-tab-pane>

      <el-tab-pane label="识字" name="ocr">
        <div class="field">
          <label>识字方式</label>
          <el-radio-group v-model="form.ocr" class="vert">
            <el-radio v-for="o in OCRS" :key="o.id" :value="o.id">{{ o.name }}</el-radio>
          </el-radio-group>
        </div>
        <div v-if="form.ocr === 'system'" class="field">
          <label>识字语言</label>
          <el-select v-model="form.ocrLang" size="small" style="width: 280px" clearable placeholder="自动（用已装语言里最合适的）">
            <el-option v-for="l in langs" :key="l.tag" :value="l.tag" :label="`${l.name}（${l.tag}）`" />
          </el-select>
          <small class="mute">只能识别 Windows 已安装的语言。没有想要的语言就到 Windows 设置里添加对应语言包，或改用“增强识字组件”。</small>
        </div>
        <div v-if="form.ocr === 'enhanced' || ocr?.installed" class="field">
          <label>增强识字组件（PaddleOCR，中英文及数字，竖排也能认）</label>
          <div class="row">
            <span v-if="ocr?.installed" class="ok">已安装 · {{ formatBytes(ocr.size) }}</span>
            <span v-else class="mute">未安装</span>
            <el-button v-if="!ocr?.installed" type="primary" size="small" :loading="!!dl" @click="installOcr">下载并安装（约 15 MB）</el-button>
            <el-button v-else size="small" @click="removeOcr">卸载</el-button>
            <el-progress v-if="dl" :percentage="progress" style="width: 200px" />
          </div>
          <div class="row">
            <small class="mute">下载不了？手动放入：</small>
            <el-button size="small" text @click="importOcr('det')">检测模型</el-button>
            <el-button size="small" text @click="importOcr('rec')">识别模型</el-button>
            <el-button size="small" text @click="importOcr('dict')">字典</el-button>
          </div>
        </div>
        <div v-if="form.ocr === 'api'" class="field">
          <label>云端识字服务</label>
          <el-select v-model="form.ocrApi.provider" size="small" style="width: 220px">
            <el-option value="ocrspace" label="OCR.space（有免费额度）" />
            <el-option value="baidu" label="百度智能云 OCR" />
            <el-option value="google" label="Google Cloud Vision" />
          </el-select>
          <el-input v-model="form.ocrApi.key" size="small" type="password" show-password :placeholder="form.ocrApi.provider === 'baidu' ? 'API Key' : 'API Key / 密钥'" />
          <el-input v-if="form.ocrApi.provider === 'baidu'" v-model="form.ocrApi.secret" size="small" type="password" show-password placeholder="Secret Key" />
          <el-input v-model="form.ocrApi.endpoint" size="small" placeholder="自建或代理地址（可留空，用官方地址）" />
          <small class="mute">选了云端识字，截下来的画面会发给这家服务。密钥只存在本机设置里。</small>
        </div>
        <div v-if="form.ocr === 'vision'" class="field"><small class="mute">用“设置 → AI”里选的视觉模型直接看图识字、翻译。准、能认游戏艺术字，但会上传图片，也更慢、更费额度。</small></div>
      </el-tab-pane>

      <el-tab-pane label="字幕模式" name="sub">
        <div class="field">
          <label>译文放哪里</label>
          <el-radio-group v-model="form.subPlace">
            <el-radio value="cover">盖在原字幕上</el-radio>
            <el-radio value="bar">放进悬浮字幕条（可拖动）</el-radio>
          </el-radio-group>
        </div>
        <div class="grid">
          <div class="field"><label>每秒看几次：{{ form.subFps }}</label><el-slider v-model="form.subFps" :min="1" :max="10" /></div>
          <div class="field"><label>字幕稳定多久再翻：{{ form.subStableMs }} 毫秒</label><el-slider v-model="form.subStableMs" :min="100" :max="1500" :step="50" /></div>
          <div class="field"><label>字幕消失后保留：{{ form.subKeepMs }} 毫秒</label><el-slider v-model="form.subKeepMs" :min="0" :max="3000" :step="100" /></div>
          <div class="field"><label>字幕条字号：{{ form.subFont }}px</label><el-slider v-model="form.subFont" :min="14" :max="64" /></div>
        </div>
        <div class="kv"><span>字幕条里同时显示原文</span><el-switch v-model="form.subBilingual" /></div>
        <div class="kv"><span>相似的句子不重复翻<small class="mute block">字幕逐字出现、略有改动时，只翻最终那句</small></span><el-switch v-model="form.subDedupe" /></div>
        <div class="kv"><span>跨屏合并<small class="mute block">一句话分两屏显示时，合成一句再翻</small></span><el-switch v-model="form.subMerge" /></div>
        <el-alert type="info" :closable="false" show-icon title="看的是本地视频文件时，用工具箱里的“生成字幕”更准：提前翻好，不会晚一拍。" />
      </el-tab-pane>

      <el-tab-pane label="漫画模式" name="manga">
        <div class="field">
          <label>识别方式</label>
          <el-radio-group v-model="form.mangaEngine">
            <el-radio value="vision">视觉模型（推荐，竖排准；会上传图片）</el-radio>
            <el-radio value="local">本机识字 + 文字翻译（不上传图片）</el-radio>
          </el-radio-group>
        </div>
        <div class="field">
          <label>阅读方向</label>
          <el-radio-group v-model="form.mangaRtl">
            <el-radio :value="true">右→左（日漫）</el-radio>
            <el-radio :value="false">左→右</el-radio>
          </el-radio-group>
        </div>
        <div class="field">
          <label>擦字方式</label>
          <el-radio-group v-model="form.mangaErase">
            <el-radio value="fill">取色填充</el-radio>
            <el-radio value="inpaint">邻近修补（渐变、网点底更自然）</el-radio>
            <el-radio value="none">只盖译文，不擦</el-radio>
          </el-radio-group>
        </div>
        <div class="field">
          <label>译文字体</label>
          <el-select v-model="form.mangaFont" style="width: 200px">
            <el-option value="hand" label="手写风黑体" />
            <el-option value="sans" label="黑体" />
            <el-option value="serif" label="宋体" />
          </el-select>
        </div>
        <div class="kv"><span>拟声词保留原样，旁边加小注</span><el-switch v-model="form.mangaSfx" /></div>
        <div class="kv"><span>翻页后自动重新翻译</span><el-switch v-model="form.mangaFollow" /></div>
        <div class="kv"><span>人名、称呼前后页保持一致<small class="mute block">本次会话里记住模型报的译名，下一页沿用</small></span><el-switch v-model="form.mangaNames" /></div>
        <el-alert type="warning" :closable="false" show-icon title="漫画模式会把当前这一页图片发给你配置的视觉模型。开启前会提示一次，并显示用的是哪个模型。" />
        <div class="field">
          <label>为什么默认走视觉模型</label>
          <small class="mute">本机文字识别对竖排日文、手写体、艺术字不稳；视觉模型一次给出文字、翻译和位置。代价是要上传图片、每页等 2–4 秒。</small>
        </div>
        <div class="field">
          <label>做不到的，提前说</label>
          <small class="mute">没有框的旁白、画面里的手写字、特效字识别率会低；目标是“能看懂”，不是专业汉化组的逐页打磨。拟声词按原样保留。</small>
        </div>
        <small class="mute">原文是竖排的，译文也竖排；原文横排的，译文也横排。翻页（画面大幅变化）时旧译文自动清除。{{ form.mangaAck ? '已确认过上传提示。' : '第一次使用会提示一次上传图片的说明。' }}</small>
      </el-tab-pane>

      <el-tab-pane label="系统声音字幕" name="audio">
        <p class="mute">把电脑正在播放的声音（视频、直播、会议）实时转成文字并翻译，显示在字幕条里。需要本机语音识别组件，先在“设置 → AI”里装好 SenseVoice。</p>
        <div class="grid">
          <div class="field"><label>听的语言</label><el-input v-model="form.audioLang" size="small" placeholder="留空 = 自动判断" /></div>
          <div class="field">
            <label>语音识别</label>
            <el-radio-group v-model="form.audioStt">
              <el-radio value="local">本机（SenseVoice）</el-radio>
              <el-radio value="cloud">云端</el-radio>
            </el-radio-group>
          </div>
        </div>
        <div class="kv"><span>字幕条里同时显示原文</span><el-switch v-model="form.audioBilingual" /></div>
        <div class="row">
          <el-button type="primary" :disabled="!st.supported || st.audio" @click="audioOn">开始系统声音字幕</el-button>
          <el-button :disabled="!st.audio" @click="audioOff">停止</el-button>
          <span v-if="st.audio" class="ok">运行中</span>
        </div>
      </el-tab-pane>

      <el-tab-pane label="按程序记住" name="apps">
        <p class="mute">框选过某个程序后，下次它在前台时会自动沿用上次的模式、识字方式和区域。下面可以删除。</p>
        <div v-if="!form.profiles.length" class="mute">还没有记住任何程序。</div>
        <div v-for="p in form.profiles" :key="p.exe" class="prof">
          <b>{{ p.exe }}</b>
          <span class="mute">{{ p.mode ? modeName(p.mode) : '沿用全局模式' }} · {{ p.target === 'window' ? '整个窗口' : `区域 ${p.region[2]}×${p.region[3]}` }}{{ p.ocr ? ' · ' + p.ocr : '' }}</span>
          <el-button size="small" text type="danger" @click="removeProfile(p.exe)">删除</el-button>
        </div>
      </el-tab-pane>

      <el-tab-pane label="软件术语库" name="terms" lazy>
        <p class="mute">每个软件有自己的叫法：同一个 “Layer” 在 Photoshop 里是“图层”，在别处可能是“层”。给软件配一个术语库，翻译这个软件的画面时只带上它用得到的术语，译法统一，提示词也更短、更省额度。术语库的“生效范围”写 <code>app:软件名</code>（比如 <code>app:photoshop</code>），只在翻译这个软件时用。需要在“设置 → AI”里打开术语库。</p>
        <div class="row">
          <el-button :disabled="!targetExe" @click="openAppGlossary">为{{ targetExe ? ` ${targetExe} ` : '当前软件' }}新建 / 打开术语库</el-button>
          <small v-if="!targetExe" class="mute">先框选或开始翻译一个软件，这里就知道它叫什么。</small>
        </div>
        <h4>内置起步包</h4>
        <small class="mute block">取自这些软件官方中文界面常见的叫法，只是起点；软件版本不同叫法会有出入，装好后可以随意改。已经装过的不会被覆盖。</small>
        <div v-for="p in packs" :key="p.id" class="prof">
          <b>{{ p.name }}</b>
          <span class="mute">{{ p.count }} 条 · 进程 {{ p.exes.join('、') }}</span>
          <el-tag v-if="p.installed" size="small" type="success">已装</el-tag>
          <el-button v-else size="small" @click="installPacks([p.id])">安装</el-button>
        </div>
        <div class="row"><el-button size="small" :disabled="packs.every((p) => p.installed)" @click="installPacks(packs.filter((p) => !p.installed).map((p) => p.id))">全部安装</el-button></div>
      </el-tab-pane>

      <el-tab-pane label="翻译历史" name="history" lazy>
        <div class="row">
          <div class="kv" style="flex: 1"><span>保存翻译历史（只存在本机）</span><el-switch v-model="form.history" /></div>
          <el-button size="small" @click="loadHist">刷新</el-button>
          <el-button size="small" @click="clearHist">清空</el-button>
          <el-button size="small" @click="clearCache">清译文缓存</el-button>
        </div>
        <div v-if="!hist.length" class="mute">暂无记录。</div>
        <div v-for="(h, i) in hist" :key="i" class="hrow" @click="copy(h.text)">
          <div class="ht"><span class="mute">{{ formatDateTime(h.at) }} · {{ modeName(h.mode) }}{{ h.exe ? ' · ' + h.exe : '' }}</span></div>
          <div class="hs">{{ h.src }}</div>
          <div>{{ h.text }}</div>
        </div>
      </el-tab-pane>
    </el-tabs>
  </div>
</template>

<style scoped>
.scr { display: flex; flex-direction: column; gap: 14px; padding: 20px 24px; height: 100%; overflow-y: auto; box-sizing: border-box }
.head { display: flex; align-items: center; gap: 20px; flex-wrap: wrap }
.head h2 { margin: 0 }
.sub { margin: 4px 0 0; color: var(--el-text-color-secondary); font-size: 13px }
.state { margin-left: auto; display: flex; align-items: center; gap: 6px; font-size: 13px }
.dot { width: 9px; height: 9px; border-radius: 50%; background: var(--el-text-color-placeholder) }
.dot.on { background: var(--el-color-success) }
.mute { color: var(--el-text-color-secondary); font-size: 12.5px }
.block { display: block }
.card { border: 1px solid var(--el-border-color); border-radius: 10px; padding: 12px 16px; display: flex; flex-direction: column; gap: 10px; background: var(--el-bg-color) }
.card h3 { margin: 0; font-size: 14px }
.row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap }
.modes { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 10px }
.mode { text-align: left; border: 1px solid var(--el-border-color); background: var(--el-fill-color-blank); border-radius: 8px; padding: 10px 12px; cursor: pointer; display: flex; flex-direction: column; gap: 4px; color: var(--el-text-color-primary) }
.mode small { color: var(--el-text-color-secondary); line-height: 1.5 }
.mode.sel { border-color: var(--el-color-primary); background: var(--el-color-primary-light-9) }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: 6px 24px }
.field { display: flex; flex-direction: column; gap: 6px; margin: 8px 0 }
.field label { font-size: 13px; font-weight: 600 }
.vert { display: flex; flex-direction: column; align-items: flex-start }
.kv { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 8px 0; border-bottom: 1px solid var(--el-border-color-extra-light) }
.mono :deep(input) { font-family: ui-monospace, Consolas, monospace }
.ok { color: var(--el-color-success); font-size: 13px }
.try { padding: 8px 10px; border-radius: 6px; background: var(--el-fill-color-light); display: flex; flex-direction: column; gap: 4px; max-height: 220px; overflow-y: auto }
.tl { font-size: 13px }
.prof { display: flex; align-items: center; gap: 12px; padding: 7px 0; border-bottom: 1px solid var(--el-border-color-extra-light) }
.hrow { padding: 8px 10px; border-radius: 6px; cursor: pointer; display: flex; flex-direction: column; gap: 2px; font-size: 13px }
.hrow:hover { background: var(--el-fill-color-light) }
.hs { color: var(--el-text-color-secondary); font-size: 12px }
</style>
