<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { MovePlan } from '../../types'

const emit = defineEmits<{ changed: [] }>()
const app = useAppStore()
const template = ref(app.settings?.library.reorganizeTemplate ?? '{platform}/{author}')
const plans = ref<MovePlan[]>([])
const busy = ref(false)
const planned = ref(false)
const skipped = ref<string[]>([])

const root = computed(() => (app.settings?.downloadDir ?? '').replace(/[\\/]+$/, ''))

function rel(p: string) {
  return p.startsWith(root.value) ? p.slice(root.value.length + 1) : p
}

const PRESETS = [
  { label: '平台/作者', v: '{platform}/{author}' },
  { label: '作者', v: '{author}' },
  { label: '平台/年/月', v: '{platform}/{year}/{month}' },
  { label: '年-月/平台', v: '{year}-{month}/{platform}' },
  { label: '类型/平台', v: '{kind}/{platform}' },
]

async function preview() {
  busy.value = true
  skipped.value = []
  try {
    plans.value = await api.reorganizePlan(template.value)
    planned.value = true
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

async function apply() {
  try {
    await ElMessageBox.confirm(`把 ${plans.value.length} 个文件移动到新的文件夹？同一个作品的字幕、封面会一起移动，目标已有同名文件的会跳过。`, '整理文件夹', {
      confirmButtonText: '开始整理',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }
  busy.value = true
  try {
    const r = await api.reorganizeApply(plans.value)
    skipped.value = r.skipped
    ElMessage.success(`已移动 ${r.moved} 个文件${r.skipped.length ? `，跳过 ${r.skipped.length} 个` : ''}`)
    emit('changed')
    plans.value = []
    planned.value = false
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="reorg">
    <p class="mute tip">按规则把已下载的文件移到子文件夹里，目录相对于下载目录。变量：<span class="mono">{platform} {author} {year} {month} {day} {date} {kind}</span>；用 / 表示下一级文件夹。媒体库里的路径会同步更新。</p>
    <div class="bar">
      <el-input v-model="template" size="small" class="tpl mono" placeholder="{platform}/{author}" @change="planned = false" />
      <el-button size="small" type="primary" :loading="busy" @click="preview">预览</el-button>
    </div>
    <div class="presets">
      <el-button v-for="p in PRESETS" :key="p.v" link size="small" @click="template = p.v; planned = false">{{ p.label }}</el-button>
    </div>
    <div v-if="planned && !plans.length" class="mute empty">文件已经都在对应的文件夹里了，不需要整理。</div>
    <template v-if="plans.length">
      <div class="list">
        <div v-for="p in plans.slice(0, 200)" :key="p.id" class="plan mono">
          <span class="ellipsis" :title="p.from">{{ rel(p.from) }}</span>
          <span class="arrow">→</span>
          <span class="ellipsis" :title="p.to">{{ rel(p.to) }}</span>
        </div>
        <div v-if="plans.length > 200" class="mute">…另有 {{ plans.length - 200 }} 个</div>
      </div>
      <div class="foot">
        <span>将移动 {{ plans.length }} 个文件</span>
        <el-button type="primary" size="small" :loading="busy" @click="apply">开始整理</el-button>
      </div>
    </template>
    <div v-if="skipped.length" class="skipped">
      <div class="mute">跳过的文件：</div>
      <div v-for="s in skipped.slice(0, 30)" :key="s" class="mono sk">{{ s }}</div>
    </div>
  </div>
</template>

<style scoped>
.reorg {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.tip {
  margin: 0;
  font-size: 12px;
}
.bar {
  display: flex;
  gap: 8px;
}
.tpl {
  max-width: 360px;
}
.presets :deep(.el-button) {
  margin-right: 8px;
  margin-left: 0;
}
.list {
  max-height: 300px;
  overflow: auto;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 6px 10px;
}
.plan {
  display: grid;
  grid-template-columns: 1fr 20px 1fr;
  gap: 6px;
  font-size: 11px;
  line-height: 20px;
}
.arrow {
  color: var(--cc-acc);
  text-align: center;
}
.foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.skipped {
  font-size: 11.5px;
}
.sk {
  color: var(--cc-err);
  font-size: 11px;
}
.empty {
  padding: 24px 0;
  text-align: center;
}
</style>
