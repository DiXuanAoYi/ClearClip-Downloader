<script setup lang="ts">
// 模型管理：下载（地址可改）/ 从本地文件导入 / 删除。人像抠图、人声分离、RNNoise 降噪、Whisper 都用它。
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText, events } from '../../../api'
import type { ModelStatus } from '../../../types'

const props = defineProps<{ kind: ModelStatus['kind']; selected?: string; single?: boolean }>()
const emit = defineEmits<{ select: [id: string]; changed: [models: ModelStatus[]] }>()

const EXT: Record<ModelStatus['kind'], string[]> = { matte: ['onnx'], vocals: ['onnx'], denoise: ['rnnn'], whisper: ['bin'] }
const all = ref<ModelStatus[]>([])
const models = computed(() => all.value.filter((m) => m.kind === props.kind))
// single：只用一个下拉选择模型，只展示选中那一个的详情（Whisper 三档不再一次摊开）
const pick = ref('')
const shown = computed(() => {
  if (!props.single) return models.value
  const cur = models.value.find((m) => m.id === pick.value) ?? models.value.find((m) => m.installed) ?? models.value.find((m) => m.id.endsWith('base')) ?? models.value[0]
  return cur ? [cur] : []
})
const urls = ref<Record<string, string>>({})
const busy = ref<string | null>(null)
const pct = ref(0)
let un: UnlistenFn | null = null

const mb = (n: number) => `${(n / 1048576).toFixed(0)} MB`

async function reload() {
  all.value = await api.mlModels()
  for (const m of all.value) urls.value[m.id] ??= m.url
  emit('changed', all.value)
}
function upsert(m: ModelStatus) {
  all.value = all.value.map((x) => (x.id === m.id ? m : x))
  emit('changed', all.value)
}
async function run(id: string, job: () => Promise<ModelStatus | void>) {
  busy.value = id
  pct.value = 0
  try {
    const m = await job()
    if (m) upsert(m)
    else await reload()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = null
  }
}
const download = (m: ModelStatus) => run(m.id, () => api.mlModelDownload(m.id, urls.value[m.id]))
async function importFile(m: ModelStatus) {
  const r = await open({ multiple: false, filters: [{ name: '模型文件', extensions: EXT[props.kind] }] })
  if (typeof r === 'string') await run(m.id, () => api.mlModelImport(m.id, r))
}
const remove = (m: ModelStatus) => run(m.id, () => api.mlModelRemove(m.id))

onMounted(async () => {
  un = await events.onModelProgress((p) => {
    if (p.id === busy.value && p.total > 0) pct.value = Math.round((p.got / p.total) * 100)
  })
  try {
    await reload()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
})
onBeforeUnmount(() => un?.())
</script>

<template>
  <div class="models">
    <el-select v-if="single && models.length > 1" :model-value="shown[0]?.id" size="small" class="pick" data-testid="model-pick" @update:model-value="(v: string) => (pick = v)">
      <el-option v-for="m in models" :key="m.id" :value="m.id" :label="m.installed ? `${m.label} ✓` : m.label" />
    </el-select>
    <div v-for="m in shown" :key="m.id" class="model" :data-model="m.id">
      <div class="row">
        <el-radio v-if="selected !== undefined" :model-value="selected" :value="m.id" :disabled="!m.installed" size="small" @update:model-value="emit('select', m.id)">{{ m.label }}</el-radio>
        <b v-else>{{ m.label }}</b>
        <span class="muted">{{ m.installed ? `已安装 · ${mb(m.sizeBytes)}` : `未安装 · 约 ${m.sizeHint}` }}</span>
      </div>
      <div class="muted note">{{ m.note }}</div>
      <template v-if="!m.installed">
        <el-input v-model="urls[m.id]" size="small" placeholder="模型下载地址（可改成镜像）" :disabled="busy === m.id" />
        <div class="row">
          <el-button size="small" type="primary" :loading="busy === m.id" @click="download(m)">{{ busy === m.id && pct ? `下载中 ${pct}%` : '下载' }}</el-button>
          <el-button size="small" :disabled="busy === m.id" @click="importFile(m)">从文件导入…</el-button>
        </div>
      </template>
      <el-button v-else size="small" link type="danger" :loading="busy === m.id" @click="remove(m)">删除模型</el-button>
    </div>
  </div>
</template>

<style scoped>
.pick { max-width: 360px; }
.models { display: flex; flex-direction: column; gap: 10px; width: 100%; }
.model { display: flex; flex-direction: column; gap: 4px; padding: 8px; border: 1px solid var(--el-border-color-lighter); border-radius: 6px; }
.row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.muted { color: var(--el-text-color-secondary); font-size: 12px; }
.note { line-height: 1.4; }
</style>
