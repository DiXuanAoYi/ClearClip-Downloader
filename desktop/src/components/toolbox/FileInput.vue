<script setup lang="ts">
import { ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import type { LibraryItem } from '../../types'
import { formatBytes } from '../../utils/format'

const props = defineProps<{
  modelValue: string[]
  multiple?: boolean
  /** 文件对话框的扩展名过滤 */
  extensions?: string[]
  /** 媒体库里只列出这些类型 */
  kinds?: string[]
  label?: string
  /** 只显示按钮，不列出已选的文件（选完马上交给调用方处理的场景） */
  compact?: boolean
  /** 按钮上的字（默认“选择文件…”和“从媒体库选择…”） */
  browseText?: string
  libraryText?: string
  /** 两个按钮都用普通样式（页面上已经有更主要的操作时） */
  plain?: boolean
}>()
const emit = defineEmits<{ 'update:modelValue': [paths: string[]] }>()

const picking = ref(false)
const query = ref('')
const items = ref<LibraryItem[]>([])
const chosen = ref<Set<number>>(new Set())
const loading = ref(false)

const name = (p: string) => p.split(/[\\/]/).pop() ?? p

async function browse() {
  const r = await open({
    multiple: !!props.multiple,
    filters: props.extensions ? [{ name: props.label ?? '文件', extensions: props.extensions }] : undefined,
  })
  if (!r) return
  const list = (Array.isArray(r) ? r : [r]) as string[]
  emit('update:modelValue', props.multiple ? [...new Set([...props.modelValue, ...list])] : list.slice(0, 1))
}

async function loadLibrary() {
  loading.value = true
  try {
    const all: LibraryItem[] = []
    for (const kind of props.kinds ?? ['video']) {
      all.push(...(await api.listLibrary({ query: query.value, kind, sort: 'finished' })))
    }
    items.value = all.filter((i) => i.exists).slice(0, 300)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}

function openPicker() {
  chosen.value = new Set()
  query.value = ''
  picking.value = true
  loadLibrary()
}

function toggle(i: LibraryItem) {
  const next = new Set(props.multiple ? chosen.value : [])
  if (next.has(i.id)) next.delete(i.id)
  else next.add(i.id)
  chosen.value = next
}

function confirmPicker() {
  const paths = items.value.filter((i) => chosen.value.has(i.id)).map((i) => i.path)
  emit('update:modelValue', props.multiple ? [...new Set([...props.modelValue, ...paths])] : paths.slice(0, 1))
  picking.value = false
}

defineExpose({ browse, openPicker })

function remove(p: string) {
  emit('update:modelValue', props.modelValue.filter((x) => x !== p))
}

function move(i: number, d: number) {
  const next = [...props.modelValue]
  const j = i + d
  if (j < 0 || j >= next.length) return
  ;[next[i], next[j]] = [next[j], next[i]]
  emit('update:modelValue', next)
}
</script>

<template>
  <div class="fi">
    <div class="bar">
      <el-button size="small" :type="plain ? undefined : 'primary'" @click="browse">{{ browseText ?? `选择${multiple ? '多个' : ''}文件…` }}</el-button>
      <el-button size="small" @click="openPicker">{{ libraryText ?? '从媒体库选择…' }}</el-button>
      <el-button v-if="modelValue.length && !compact" link size="small" @click="emit('update:modelValue', [])">清空</el-button>
    </div>
    <div v-if="!modelValue.length && !compact" class="mute hint">{{ label ? `请选择${label}` : '请选择文件' }}，也可以在媒体库的“详情”里点“用工具箱处理”。</div>
    <div v-for="(p, i) in compact ? [] : modelValue" :key="p" class="file">
      <span class="ellipsis selectable" :title="p">{{ name(p) }}</span>
      <span class="ops">
        <template v-if="multiple && modelValue.length > 1">
          <el-button link size="small" :disabled="i === 0" @click="move(i, -1)">上移</el-button>
          <el-button link size="small" :disabled="i === modelValue.length - 1" @click="move(i, 1)">下移</el-button>
        </template>
        <el-button link size="small" @click="remove(p)">移除</el-button>
      </span>
    </div>

    <el-dialog v-model="picking" title="从媒体库选择" width="560px" append-to-body>
      <el-input v-model="query" size="small" clearable placeholder="搜索标题或作者" @input="loadLibrary" />
      <div v-loading="loading" class="plist">
        <div v-if="!items.length" class="mute empty">没有可选的文件。</div>
        <button v-for="i in items" :key="i.id" type="button" class="pitem" :class="{ on: chosen.has(i.id) }" @click="toggle(i)">
          <span class="ellipsis">{{ i.title }}</span>
          <small class="mono mute">{{ formatBytes(i.size) }}</small>
        </button>
      </div>
      <template #footer>
        <el-button size="small" @click="picking = false">取消</el-button>
        <el-button size="small" type="primary" :disabled="!chosen.size" @click="confirmPicker">选择{{ chosen.size ? `（${chosen.size}）` : '' }}</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.fi {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.bar {
  display: flex;
  gap: 8px;
  align-items: center;
}
.hint {
  font-size: 12px;
}
.file {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  padding: 3px 8px;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
  font-size: 12.5px;
}
.ops {
  flex: none;
}
.plist {
  margin-top: 8px;
  max-height: 340px;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 80px;
}
.pitem {
  all: unset;
  cursor: pointer;
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 10px;
  padding: 5px 8px;
  border-radius: 6px;
  font-size: 12.5px;
}
.pitem:hover {
  background: var(--cc-acc-soft);
}
.pitem.on {
  background: var(--cc-acc-soft);
  outline: 1px solid var(--cc-acc);
}
.empty {
  padding: 24px 0;
  text-align: center;
}
</style>
