<script setup lang="ts">
// 本机语音识别：选哪个模型、下载 / 删除。“设置 → AI”和“设置 → 组件”共用这一个，选择存在同一个设置项里，两边永远一致。
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { ElMessage } from 'element-plus'
import { api, errorText, events } from '../api'
import type { ModelStatus, SenseVoiceStatus } from '../types'
import { useTauriEvents } from '../composables/useTauriEvent'
const ev = useTauriEvents()

const props = defineProps<{ modelValue: string; persist?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [id: string] }>()

const SV = 'sensevoice'
const whisper = ref<ModelStatus[]>([])
const sv = ref<SenseVoiceStatus | null>(null)
const busy = ref<string | null>(null)
const pct = ref(0)
let un: UnlistenFn | null = null

const mb = (n: number) => `${(n / 1048576).toFixed(0)} MB`
const options = computed(() => [
  {
    id: SV,
    label: 'SenseVoice（中文推荐）',
    note: '阿里开源，中文 / 粤语 / 英 / 日 / 韩，比 Whisper 快很多、中文更准，自带标点。只给整句时间，不支持“文稿剪辑”的逐词剪。',
    installed: !!sv.value?.ready,
    size: '约 170 MB',
  },
  ...whisper.value.map((m) => ({ id: m.id, label: m.label, note: m.note, installed: m.installed, size: m.installed ? mb(m.sizeBytes) : `约 ${m.sizeHint}` })),
])
const cur = computed(() => options.value.find((o) => o.id === props.modelValue))

async function choose(id: string) {
  emit('update:modelValue', id)
  if (!props.persist) return
  try {
    const s = await api.getSettings()
    await api.saveSettings({ ...s, ai: { ...s.ai, sttLocalModel: id, sttEngine: 'local' } }, s)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function reload() {
  const [m, s] = await Promise.all([api.mlModels(), api.sensevoiceStatus()])
  whisper.value = m.filter((x) => x.kind === 'whisper')
  sv.value = s
}
async function run(id: string, job: () => Promise<unknown>, pick = true) {
  busy.value = id
  pct.value = 0
  try {
    await job()
    await reload()
    if (pick) await choose(id)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = null
  }
}
const install = (id: string) => run(id, () => (id === SV ? api.sensevoiceInstall() : api.mlModelDownload(id)))
const remove = (id: string) =>
  run(
    id,
    async () => {
      if (id === SV) await api.sensevoiceRemove()
      else await api.mlModelRemove(id)
      if (props.modelValue === id) await choose('')
    },
    false,
  )

onMounted(async () => {
  un = await ev.on(events.onModelProgress((p) => {
    if (busy.value && (p.id === busy.value || (busy.value === SV && p.id === 'sensevoice')) && p.total > 0) pct.value = Math.round((p.got / p.total) * 100)
  }))
  try {
    await reload()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
})
onBeforeUnmount(() => un?.())
</script>

<template>
  <div class="stt" data-testid="stt-local">
    <el-select :model-value="modelValue" size="small" class="pick" placeholder="自动选择已安装的" data-testid="stt-pick" @update:model-value="(v: string) => choose(v)">
      <el-option value="" label="自动（用已安装的，优先 SenseVoice）" />
      <el-option v-for="o in options" :key="o.id" :value="o.id" :label="o.installed ? `${o.label} ✓` : o.label" />
    </el-select>
    <div v-if="cur" class="model">
      <div class="row">
        <b>{{ cur.label }}</b>
        <span class="muted">{{ cur.installed ? `已安装 · ${cur.size}` : `未安装 · ${cur.size}` }}</span>
      </div>
      <div class="muted note">{{ cur.note }}</div>
      <div v-if="cur.id === SV && sv?.note" class="muted note">{{ sv.note }}</div>
      <div class="row">
        <el-button v-if="!cur.installed" size="small" type="primary" :loading="busy === cur.id" @click="install(cur.id)">{{ busy === cur.id && pct ? `下载中 ${pct}%` : '下载' }}</el-button>
        <el-button v-else size="small" link type="danger" :loading="busy === cur.id" @click="remove(cur.id)">删除模型</el-button>
      </div>
    </div>
    <div v-else class="muted note">没选具体模型：转写时用已装好的，SenseVoice 优先，其次 Whisper。</div>
  </div>
</template>

<style scoped>
.stt { display: flex; flex-direction: column; gap: 8px; width: 100%; }
.pick { max-width: 380px; }
.model { display: flex; flex-direction: column; gap: 4px; padding: 8px; border: 1px solid var(--cc-line-soft); border-radius: 6px; }
.row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.muted { color: var(--cc-mute); font-size: 12px; }
.note { line-height: 1.4; }
</style>
