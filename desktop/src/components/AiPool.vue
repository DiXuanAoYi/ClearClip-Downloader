<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { AiSettings, PoolEntry, PoolPreset, PoolStatus } from '../types'

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
]
const REASON: Record<string, string> = { quota: '额度用完', rate: '请求太频繁', bad_key: '密钥无效', network: '网络不通', error: '出错' }

const addable = computed(() => presets.value.filter((p) => !ai.value.pool.some((e) => e.preset === p.id)))
const presetOf = (e: PoolEntry) => presets.value.find((p) => p.id === e.preset)

async function refresh() {
  const list = await api.aiPoolStatus().catch(() => [] as PoolStatus[])
  status.value = Object.fromEntries(list.map((s) => [s.id, s]))
  supaHas.value = await api.secretHas('ai.supadata').catch(() => false)
}
onMounted(async () => {
  presets.value = await api.aiPoolPresets().catch(() => [])
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
  ai.value.pool.push({ id: nextId(p.id), preset: p.id, name: p.name, baseUrl: p.baseUrl, model: p.model, enabled: true, tasks: [...p.tasks], priority: 10, dailyLimit: 0 })
}
function addCustom() {
  ai.value.pool.push({ id: nextId('custom'), preset: 'custom', name: '自定义服务', baseUrl: '', model: '', enabled: true, tasks: [], priority: 20, dailyLimit: 0 })
}
function remove(e: PoolEntry) {
  api.secretSet(`ai.pool.${e.id}`, '').catch(() => {})
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
  }
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
async function reset(e: PoolEntry) {
  await api.aiPoolReset(e.id)
  refresh()
}
function state(e: PoolEntry): { text: string; cls: string } {
  const s = status.value[e.id]
  if (!e.enabled) return { text: '已停用', cls: 'mute' }
  if (!s) return { text: '—', cls: 'mute' }
  if (!s.hasKey && !presetOf(e)?.local) return { text: '未填密钥', cls: 'bad' }
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

    <div v-if="!ai.pool.length" class="empty mute">模型池是空的，现在只使用下面“默认服务”。从下面的“添加服务”选一个有免费额度的开始。</div>

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
      <div class="grid">
        <div class="field"><label>接口地址</label><el-input v-model="e.baseUrl" size="small" class="mono" /></div>
        <div class="field"><label>模型</label><el-input v-model="e.model" size="small" class="mono" /></div>
      </div>
      <div class="field">
        <label>API 密钥 <span v-if="status[e.id]?.hasKey" class="ok">已保存</span>
          <template v-if="presetOf(e)?.keyUrl">
            · <a href="#" @click.prevent="api.openUrl(presetOf(e)!.keyUrl)">获取 API 密钥</a>
            <template v-if="presetOf(e)?.signupUrl && presetOf(e)?.signupUrl !== presetOf(e)?.keyUrl"> · <a href="#" @click.prevent="api.openUrl(presetOf(e)!.signupUrl)">注册 / 登录</a></template>
          </template>
        </label>
        <div class="keyrow">
          <el-input v-model="keys[e.id]" size="small" type="password" show-password :placeholder="presetOf(e)?.local ? '本机服务可留空' : status[e.id]?.hasKey ? '输入新密钥可替换' : '粘贴 API 密钥'" />
          <el-button size="small" :disabled="!(keys[e.id] || '').trim()" @click="saveKey(e)">保存</el-button>
          <el-button v-if="status[e.id]?.hasKey" size="small" @click="keys[e.id] = ''; saveKey(e)">清除</el-button>
        </div>
      </div>
      <div class="grid">
        <div class="field">
          <label>适用任务（不选 = 全部）</label>
          <el-select v-model="e.tasks" multiple size="small" collapse-tags placeholder="全部任务"><el-option v-for="t in TASKS" :key="t.v" :value="t.v" :label="t.l" /></el-select>
        </div>
        <div class="field two">
          <div><label>优先级</label><el-input-number v-model="e.priority" size="small" :min="0" :max="999" controls-position="right" /></div>
          <div><label>每天上限（0 = 不限）</label><el-input-number v-model="e.dailyLimit" size="small" :min="0" :step="100" controls-position="right" /></div>
        </div>
      </div>
      <div class="acts">
        <el-button size="small" :loading="busy[e.id]" @click="test(e)">测试</el-button>
        <el-button size="small" @click="reset(e)">清除冷却和计数</el-button>
      </div>
    </div>

    <div class="adds">
      <el-dropdown trigger="click" @command="(id: string) => (id === 'custom' ? addCustom() : addPreset(presets.find((p) => p.id === id)!))">
        <el-button size="small" type="primary">添加服务</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item v-for="p in addable" :key="p.id" :command="p.id">{{ p.name }}<span v-if="p.free" class="tag">有免费额度</span><span v-if="p.local" class="tag">本机</span></el-dropdown-item>
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
.supa { border-top: 1px solid var(--cc-border, rgba(128,128,128,.25)); padding-top: 12px; display: flex; flex-direction: column; gap: 8px; }
</style>
