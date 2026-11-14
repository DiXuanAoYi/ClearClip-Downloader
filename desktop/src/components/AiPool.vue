<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { AiSettings, ModelInfo, PoolEntry, PoolPreset, PoolStatus, ProbeResult } from '../types'
import { toastErr } from '../utils/toastErr'

const ai = defineModel<AiSettings>({ required: true })
const presets = ref<PoolPreset[]>([])
const status = ref<Record<string, PoolStatus>>({})
const keys = ref<Record<string, string>>({})
const busy = ref<Record<string, boolean>>({})
const supaKey = ref('')
const supaHas = ref(false)
let timer: ReturnType<typeof setInterval> | undefined

const TASKS = [
  { v: 'translate', l: '翻译字幕' },
  { v: 'summary', l: '摘要与章节' },
  { v: 'chat', l: '问答' },
  { v: 'edit', l: '剪辑文稿' },
  { v: 'stt', l: '语音转字幕（云端）' },
]
// 提供语音转文字接口的预设及推荐模型（与后端 ai_pool::stt_model_for 一致）
const STT_MODELS: Record<string, string> = { groq: 'whisper-large-v3-turbo', siliconflow: 'FunAudioLLM/SenseVoiceSmall' }
const REASON: Record<string, string> = { quota: '额度用完', rate: '请求太频繁', bad_key: '密钥无效', network: '网络不通', error: '出错' }

const addable = computed(() => presets.value.filter((p) => !ai.value.pool.some((e) => e.preset === p.id)))
const presetOf = (e: PoolEntry) => presets.value.find((p) => p.id === e.preset)
// 机器翻译服务（Google / 微软 / DeepL / DeepLX / 百度 / 有道 / 彩云 / 小牛）：不是对话模型，没有“模型名”，只用于翻译
const MT_KINDS = ['google', 'microsoft', 'deepl', 'deeplx', 'baidu', 'youdao', 'caiyun', 'niutrans', 'tencent', 'aliyun', 'volcano']
const isMt = (e: PoolEntry) => MT_KINDS.includes(e.kind)
const baseLabel = (e: PoolEntry) => (!isMt(e) ? '接口地址' : e.kind === 'deeplx' ? '接口地址（完整，必填）' : '接口地址（留空 = 官方）')
const basePlaceholder = (e: PoolEntry) => (e.kind === 'deeplx' ? 'http://127.0.0.1:1188/translate' : isMt(e) ? '官方地址' : '')
const chatPresets = computed(() => addable.value.filter((p) => !MT_KINDS.includes(p.kind)))
const mtPresets = computed(() => addable.value.filter((p) => MT_KINDS.includes(p.kind)))

async function refresh() {
  const list = await api.aiPoolStatus().catch(() => [] as PoolStatus[])
  status.value = Object.fromEntries(list.map((s) => [s.id, s]))
  supaHas.value = await api.secretHas('ai.supadata').catch(() => false)
}
onMounted(async () => {
  presets.value = await api.aiPoolPresets().catch(() => []) // 忽略：可选数据，取不到就按空处理
  await refresh()
  timer = setInterval(refresh, 5000)
})
onUnmounted(() => clearInterval(timer))

function nextId(base: string) {
  let id = base
  for (let n = 2; ai.value.pool.some((e) => e.id === id); n++) id = `${base}-${n}`
  return id
}
function addPreset(p: PoolPreset) {
  ai.value.pool.push({ id: nextId(p.id), preset: p.id, kind: p.kind === 'openai' ? '' : p.kind, name: p.name, baseUrl: p.baseUrl, model: p.model, sttModel: STT_MODELS[p.id] || '', enabled: true, tasks: [...p.tasks], priority: 10, dailyLimit: 0, monthlyChars: p.kind === 'deepl' ? 500000 : p.kind === 'google' ? 500000 : p.kind === 'microsoft' ? 2000000 : 0 })
}
function addCustom() {
  ai.value.pool.push({ id: nextId('custom'), preset: 'custom', kind: '', name: '自定义服务', baseUrl: '', model: '', sttModel: '', enabled: true, tasks: [], priority: 20, dailyLimit: 0, monthlyChars: 0 })
}
function remove(e: PoolEntry) {
  api.secretSet(`ai.pool.${e.id}`, '').catch(toastErr('清除密钥'))
  ai.value.pool = ai.value.pool.filter((x) => x.id !== e.id)
}
async function saveKey(e: PoolEntry) {
  const v = (keys.value[e.id] || '').trim()
  try {
    await api.secretSet(`ai.pool.${e.id}`, v)
    keys.value[e.id] = ''
    await api.aiPoolReset(e.id)
    ElMessage.success(v ? '密钥已加密保存' : '密钥已清除')
    refresh()
  } catch (err) {
    ElMessage.error(errorText(err))
    return
  }
  // 保存了新密钥：自动检测这个服务有哪些模型，当前模型为空或不在列表里就默认选第一个
  if (v && !isMt(e)) void detectModels(e, v, true)
}
// ---------- 模型下拉（保存密钥时自动检测） ----------
const modelOpts = ref<Record<string, string[]>>({})
const detecting = ref<Record<string, boolean>>({})
async function detectModels(e: PoolEntry, key: string | null, auto: boolean) {
  if (detecting.value[e.id]) return
  detecting.value[e.id] = true
  try {
    const list = await api.aiProbeModels(e.id, e.baseUrl, key)
    const chat = list.filter((m) => m.caps.includes('chat'))
    const ids = (chat.length ? chat : list).map((m) => m.id)
    modelOpts.value[e.id] = ids
    if (auto && ids.length && !ids.includes(e.model)) {
      e.model = ids[0]
      ElMessage.success(`检测到 ${ids.length} 个模型，已选用 ${ids[0]}（记得点右下角“保存”）`)
    } else if (auto) {
      ElMessage.success(`检测到 ${ids.length} 个模型，可在“模型”下拉里选`)
    }
  } catch (err) {
    if (auto) ElMessage.warning(`密钥已保存，但没能读到模型列表：${errorText(err)}。可以手动填写模型名。`)
  } finally {
    detecting.value[e.id] = false
  }
}
function onModelDrop(e: PoolEntry, open: boolean) {
  if (open && !modelOpts.value[e.id]?.length) void detectModels(e, null, false)
}
async function saveSupadata(clear = false) {
  try {
    await api.secretSet('ai.supadata', clear ? '' : supaKey.value.trim())
    supaKey.value = ''
    ElMessage.success(clear ? '已清除' : '密钥已加密保存')
    refresh()
  } catch (err) {
    ElMessage.error(errorText(err))
  }
}
async function test(e: PoolEntry) {
  busy.value[e.id] = true
  try {
    // 测试走已保存的设置，所以先提醒保存
    const r = await api.aiPoolTest(e.id)
    ElMessage.success(`${e.name} 可用，回复：${r}`)
  } catch (err) {
    ElMessage.error(`${e.name}：${errorText(err)}（修改过设置请先点右下角“保存”再测试）`)
  } finally {
    busy.value[e.id] = false
    refresh()
  }
}

// ---------- 检测可用模型 ----------
interface ProbeRow extends ModelInfo { picked: boolean; res?: ProbeResult; testing?: boolean }
const CAP_LABEL: Record<string, string> = { chat: '对话', vision: '识图', stt: '语音', embed: '嵌入', image: '生图', tts: '朗读' }
const probe = ref<{ e: PoolEntry; rows: ProbeRow[]; q: string; cap: string; loading: boolean; error: string; ms: number } | null>(null)
const shown = computed(() => {
  const p = probe.value
  if (!p) return []
  const q = p.q.trim().toLowerCase()
  return p.rows.filter((r) => (!q || r.id.toLowerCase().includes(q)) && (!p.cap || r.caps.includes(p.cap)) && (p.cap || !r.caps.every((c) => c === 'embed' || c === 'image' || c === 'tts')))
})
const capCount = (c: string) => probe.value?.rows.filter((r) => r.caps.includes(c)).length ?? 0
const pickedRows = computed(() => probe.value?.rows.filter((r) => r.picked) ?? [])
async function openProbe(e: PoolEntry) {
  probe.value = { e, rows: [], q: '', cap: '', loading: true, error: '', ms: 0 }
  const t0 = Date.now()
  try {
    const list = await api.aiProbeModels(e.id, e.baseUrl, keys.value[e.id] || null)
    probe.value.rows = list.map((m) => ({ ...m, picked: m.id === e.model }))
    probe.value.ms = Date.now() - t0
  } catch (err) {
    if (probe.value) probe.value.error = errorText(err)
  } finally {
    if (probe.value) probe.value.loading = false
  }
}
async function testRow(r: ProbeRow) {
  const p = probe.value
  if (!p) return
  r.testing = true
  try {
    r.res = await api.aiProbeTest(p.e.id, p.e.baseUrl, keys.value[p.e.id] || null, r.id, r.caps.includes('vision'))
  } catch (err) {
    r.res = { ok: false, ms: 0, text: errorText(err), vision: null }
  } finally {
    r.testing = false
  }
}
async function testPicked() {
  for (const r of pickedRows.value) await testRow(r)
}
function applyProbe() {
  const p = probe.value
  if (!p) return
  const rows = pickedRows.value.filter((r) => !r.res || r.res.ok)
  if (!rows.length) return ElMessage.warning('请先勾选模型；测试失败的不会加入。')
  const [first, ...rest] = rows
  p.e.model = first.id
  if (first.caps.includes('stt') && !p.e.tasks.includes('stt')) { p.e.tasks = ['stt']; p.e.sttModel = first.id }
  for (const r of rest) {
    const id = nextId(p.e.id.replace(/-\d+$/, ''))
    const stt = r.caps.includes('stt')
    ai.value.pool.push({ ...p.e, id, name: `${p.e.name} · ${r.id.split('/').pop()}`, model: r.id, tasks: stt ? ['stt'] : [...p.e.tasks], sttModel: stt ? r.id : p.e.sttModel, priority: p.e.priority + 1 })
    api.aiPoolCopyKey(p.e.id, id).catch(toastErr('复制密钥'))
  }
  ElMessage.success(rest.length ? `已选用 ${first.id}，另外 ${rest.length} 个作为新条目加入（记得点右下角“保存”）` : `已选用 ${first.id}（记得点右下角“保存”）`)
  probe.value = null
}
async function reset(e: PoolEntry) {
  await api.aiPoolReset(e.id)
  refresh()
}
function state(e: PoolEntry): { text: string; cls: string } {
  const s = status.value[e.id]
  if (!e.enabled) return { text: '已停用', cls: 'mute' }
  if (!s) return { text: '—', cls: 'mute' }
  if (!s.hasKey && !presetOf(e)?.local && e.kind !== 'deeplx') return { text: '未填密钥', cls: 'bad' }
  if (s.badKey) return { text: '密钥无效', cls: 'bad' }
  if (s.coolingSecs > 0) {
    const m = Math.ceil(s.coolingSecs / 60)
    return { text: `${REASON[s.coolReason] || '冷却中'}，${m >= 90 ? Math.round(m / 60) + ' 小时' : m + ' 分钟'}后再用`, cls: 'warn' }
  }
  if (e.dailyLimit > 0 && s.used >= e.dailyLimit) return { text: '今日已达上限', cls: 'warn' }
  return { text: '可用', cls: 'ok' }
}
</script>

<template>
  <section class="group card pool">
    <h3>模型池（翻译字幕、摘要与章节）</h3>
    <small class="mute">
      可以放多个服务：每次请求按“适用任务 → 优先级（数字小的先用）→ 今天用得少的先用”自动挑一个；某个服务额度用完、被限流或出错时自动冷却并换下一个，不用你手动切换。免费额度用完的服务会在次日零点（北京时间）自动恢复。密钥加密保存在本机，不进设置文件和备份。
    </small>

    <div v-if="!ai.pool.length" class="empty mute">模型池是空的：从下面的“添加服务”选一个有免费额度的开始。</div>

    <div v-for="e in ai.pool" :key="e.id" class="svc" :class="{ off: !e.enabled }">
      <div class="head">
        <el-switch v-model="e.enabled" size="small" />
        <el-input v-model="e.name" size="small" class="nm" />
        <span class="st" :class="state(e).cls">{{ state(e).text }}</span>
        <span class="mute used">今日 {{ status[e.id]?.used ?? 0 }}<template v-if="e.dailyLimit"> / {{ e.dailyLimit }}</template> 次</span>
        <el-button size="small" link type="danger" @click="remove(e)">删除</el-button>
      </div>
      <small v-if="presetOf(e)?.note" class="mute">{{ presetOf(e)?.note }}</small>
      <small v-if="status[e.id]?.lastError" class="err">最近错误：{{ status[e.id].lastError }}</small>
      <small v-if="isMt(e)" class="mute mtnote">机器翻译服务：只用于翻译字幕 / 网页 / 文件 / 屏幕，不能做摘要和问答；不支持术语库和 AI 专家（要术语一致时把对话模型排在它前面）。</small>
      <div class="grid">
        <div class="field">
          <label>{{ baseLabel(e) }}</label>
          <el-input v-model="e.baseUrl" size="small" class="mono" :placeholder="basePlaceholder(e)" />
        </div>
        <div v-if="!isMt(e)" class="field">
          <label>模型 · <a href="#" @click.prevent="openProbe(e)">详细检测 / 测试</a></label>
          <el-select v-model="e.model" size="small" class="mono" filterable allow-create default-first-option :loading="detecting[e.id]" placeholder="保存密钥后自动检测，也可手动输入" no-data-text="还没有检测到模型：先保存密钥" @visible-change="(o: boolean) => onModelDrop(e, o)">
            <el-option v-for="m in modelOpts[e.id] || (e.model ? [e.model] : [])" :key="m" :value="m" :label="m" />
          </el-select>
        </div>
        <div v-else-if="e.kind === 'microsoft'" class="field">
          <label>区域（Azure 资源所在区域，如 eastasia；全球资源留空）</label>
          <el-input v-model="e.model" size="small" class="mono" placeholder="eastasia" />
        </div>
      </div>
      <div class="field">
        <label>API 密钥 <span v-if="status[e.id]?.hasKey" class="ok">已保存</span>
          <template v-if="presetOf(e)?.keyUrl">
            · <a href="#" @click.prevent="api.openUrl(presetOf(e)!.keyUrl)">获取 API 密钥</a>
            <template v-if="presetOf(e)?.signupUrl && presetOf(e)?.signupUrl !== presetOf(e)?.keyUrl"> · <a href="#" @click.prevent="api.openUrl(presetOf(e)!.signupUrl)">注册 / 登录</a></template>
          </template>
        </label>
        <div class="keyrow">
          <el-input v-model="keys[e.id]" size="small" type="password" show-password :placeholder="presetOf(e)?.local || e.kind === 'deeplx' ? '没设密钥可留空' : status[e.id]?.hasKey ? '输入新密钥可替换' : (['baidu', 'youdao', 'tencent', 'aliyun', 'volcano'].includes(e.kind) ? 'ID:密钥（英文冒号连起来）' : '粘贴 API 密钥')" />
          <el-button size="small" :disabled="!(keys[e.id] || '').trim()" @click="saveKey(e)">保存</el-button>
          <el-button v-if="status[e.id]?.hasKey" size="small" @click="keys[e.id] = ''; saveKey(e)">清除</el-button>
        </div>
      </div>
      <div class="grid">
        <div class="field">
          <label>适用任务（不选 = 全部）</label>
          <el-select v-if="!isMt(e)" v-model="e.tasks" multiple size="small" collapse-tags placeholder="全部任务"><el-option v-for="t in TASKS" :key="t.v" :value="t.v" :label="t.l" /></el-select>
          <el-input v-else size="small" model-value="翻译（固定）" disabled />
        </div>
        <div v-if="e.tasks.includes('stt')" class="field">
          <label>语音转文字模型（云端字幕识别用）</label>
          <el-input v-model="e.sttModel" size="small" placeholder="如 whisper-large-v3-turbo" />
        </div>
        <div class="field two">
          <div><label>优先级</label><el-input-number v-model="e.priority" size="small" :min="0" :max="999" controls-position="right" /></div>
          <div><label>每天上限（0 = 不限）</label><el-input-number v-model="e.dailyLimit" size="small" :min="0" :step="100" controls-position="right" /></div>
          <div v-if="isMt(e)">
            <label>每月字符上限（0 = 不限）<span class="mute"> · 本月已用 {{ (status[e.id]?.charsUsed ?? 0).toLocaleString() }}</span></label>
            <el-input-number v-model="e.monthlyChars" size="small" :min="0" :step="100000" controls-position="right" />
          </div>
        </div>
      </div>
      <div class="acts">
        <el-button size="small" :loading="busy[e.id]" @click="test(e)">测试</el-button>
        <el-button size="small" @click="reset(e)">清除冷却和计数</el-button>
      </div>
    </div>


    <el-dialog append-to-body :model-value="!!probe" title="检测可用模型" width="880px" @close="probe = null">
      <template v-if="probe">
        <div class="pr-top mute">
          <span>{{ probe.e.name }} · {{ probe.e.baseUrl }}</span>
          <span v-if="probe.loading">正在读取模型列表…</span>
          <span v-else-if="!probe.error" class="ok">已连通 · 读到 {{ probe.rows.length }} 个模型 · 用时 {{ (probe.ms / 1000).toFixed(1) }} 秒</span>
        </div>
        <small class="mute">密钥只在本机发出请求，不会上传。</small>
        <el-alert v-if="probe.error" type="warning" :closable="false" show-icon :title="probe.error" />
        <template v-else>
          <div class="pr-bar">
            <el-input v-model="probe.q" size="small" placeholder="搜索模型，例如 qwen、vl" clearable style="width: 240px" />
            <el-button size="small" :type="probe.cap === '' ? 'primary' : 'default'" plain @click="probe.cap = ''">全部</el-button>
            <el-button v-for="c in ['chat', 'vision', 'stt', 'embed']" :key="c" size="small" :type="probe.cap === c ? 'primary' : 'default'" plain @click="probe.cap = c">{{ CAP_LABEL[c] }} {{ capCount(c) }}</el-button>
            <span class="grow" />
            <el-button size="small" :disabled="!pickedRows.length" @click="testPicked">测试已勾选（{{ pickedRows.length }}）</el-button>
          </div>
          <div class="pr-list">
            <div v-for="r in shown" :key="r.id" class="pr-row">
              <el-checkbox v-model="r.picked" />
              <b class="mono nm2">{{ r.id }}</b>
              <span class="caps"><el-tag v-for="c in r.caps" :key="c" size="small" :type="c === 'vision' ? 'warning' : 'info'">{{ CAP_LABEL[c] || c }}</el-tag><el-tag v-if="r.free" size="small" type="success">免费</el-tag></span>
              <span class="res">
                <template v-if="r.testing">测试中…</template>
                <template v-else-if="r.res"><span :class="r.res.ok ? 'ok' : 'bad'">{{ r.res.ok ? '✓ 可用' + (r.res.vision === true ? '，能看图' : r.res.vision === false ? '，不能看图' : '') : '✗ ' + r.res.text }}</span> <span class="mute">{{ (r.res.ms / 1000).toFixed(1) }} 秒</span></template>
                <a v-else href="#" @click.prevent="testRow(r)">测试</a>
              </span>
            </div>
            <div v-if="!shown.length" class="mute" style="padding: 14px">没有符合的模型。</div>
          </div>
        </template>
      </template>
      <template #footer>
        <small class="mute" style="float: left">已勾选 {{ pickedRows.length }} 个；测试失败的不会加入。多选时第一个用于当前条目，其余新增条目。</small>
        <el-button @click="probe = null">取消</el-button>
        <el-button type="primary" :disabled="!pickedRows.length" @click="applyProbe">选用</el-button>
      </template>
    </el-dialog>

    <div class="adds">
      <el-dropdown trigger="click" @command="(id: string) => (id === 'custom' ? addCustom() : addPreset(presets.find((p) => p.id === id)!))">
        <el-button size="small" type="primary">添加服务</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item v-for="p in chatPresets" :key="p.id" :command="p.id">{{ p.name }}<span v-if="p.free" class="tag">有免费额度</span><span v-if="p.local" class="tag">本机</span></el-dropdown-item>
            <el-dropdown-item v-for="(p, i) in mtPresets" :key="p.id" :command="p.id" :divided="i === 0">{{ p.name }}<span class="tag">机器翻译</span><span v-if="p.free" class="tag">有免费额度</span></el-dropdown-item>
            <el-dropdown-item command="custom" divided>自定义（兼容 OpenAI 的接口）</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
      <small class="mute">提示：改完设置后点页面右下角“保存”才会生效。</small>
    </div>

    <div class="supa">
      <div class="kv">
        <span>用 Supadata 取在线字幕（备用，更快）<small class="mute block">贴 YouTube 等链接时，先向 Supadata 要现成字幕，比本机下载快，也不容易被限流；没有字幕或额度用完会自动退回原来的方式。视频网址会发送给 Supadata。</small></span>
        <el-switch v-model="ai.supadataOn" />
      </div>
      <div v-if="ai.supadataOn" class="field">
        <label>Supadata 密钥 <span v-if="supaHas" class="ok">已保存</span> · <a href="#" @click.prevent="api.openUrl('https://dash.supadata.ai/')">获取 API 密钥</a></label>
        <div class="keyrow">
          <el-input v-model="supaKey" size="small" type="password" show-password placeholder="粘贴 Supadata 密钥" />
          <el-button size="small" :disabled="!supaKey.trim()" @click="saveSupadata()">保存</el-button>
          <el-button v-if="supaHas" size="small" @click="saveSupadata(true)">清除</el-button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.pool { grid-column: 1 / -1; }
.group { padding: 14px 16px; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
h3 { margin: 0; font-size: 12px; color: var(--cc-mute); }
.svc { border: 1px solid var(--cc-border, rgba(128,128,128,.25)); border-radius: 8px; padding: 10px 12px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
.svc.off { opacity: .6; }
.head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.nm { width: 160px; }
.st { font-size: 12px; }
.used { margin-left: auto; font-size: 12px; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 8px 12px; }
.field { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.field.two { flex-direction: row; gap: 12px; flex-wrap: wrap; }
.field label { font-size: 12px; color: var(--cc-mute); }
.keyrow { display: flex; gap: 8px; }
.kv { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
.block { display: block; }
.acts, .adds { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.ok { color: var(--cc-ok); }
.bad { color: var(--cc-err); }
.warn { color: var(--cc-warn, #e6a23c); }
.err { color: var(--cc-err); word-break: break-all; }
.tag { margin-left: 8px; font-size: 11px; color: var(--cc-ok); }
.empty { padding: 8px 0; }
.mtnote { display: block }
.supa { border-top: 1px solid var(--cc-border, rgba(128,128,128,.25)); padding-top: 12px; display: flex; flex-direction: column; gap: 8px; }
.pr-top { display: flex; gap: 14px; flex-wrap: wrap; font-size: 13px; }
.pr-bar { display: flex; gap: 6px; align-items: center; margin: 10px 0; flex-wrap: wrap; }
.pr-bar .grow { flex: 1; }
.pr-list { border: 1px solid var(--cc-border, #dcdfe6); border-radius: 8px; max-height: 360px; overflow-y: auto; }
.pr-row { display: grid; grid-template-columns: 24px minmax(0, 1fr) minmax(0, 150px) minmax(0, 190px); gap: 10px; align-items: center; padding: 8px 12px; border-top: 1px solid var(--cc-border, #ebeef5); font-size: 13px; }
.pr-row:first-child { border-top: 0; }
.pr-row .nm2 { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pr-row .caps { display: flex; gap: 4px; flex-wrap: wrap; }
.bad { color: var(--cc-err); font-weight: 600; }
</style>
