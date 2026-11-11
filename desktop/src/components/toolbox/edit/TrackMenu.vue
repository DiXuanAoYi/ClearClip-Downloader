<script setup lang="ts">
// 一个小菜单（右键轨道头、选视频轨类型都用它）：点菜单外面或按 Esc 关闭，上下键移动，回车选中。
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'

export interface MenuItem {
  label: string
  /** 第二行的小字说明 */
  desc?: string
  danger?: boolean
  disabled?: boolean
  checked?: boolean
  divider?: boolean
  run?: () => void
}

const props = defineProps<{ x: number; y: number; items: MenuItem[]; title?: string }>()
const emit = defineEmits<{ close: [] }>()
const box = ref<HTMLElement | null>(null)
const pos = ref({ x: props.x, y: props.y })
const cur = ref(-1)

function pick(it: MenuItem) {
  if (it.disabled || it.divider) return
  emit('close')
  it.run?.()
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  const live = props.items.map((it, i) => ({ it, i })).filter((q) => !q.it.disabled && !q.it.divider)
  if (!live.length) return
  const at = live.findIndex((q) => q.i === cur.value)
  if (e.key === 'ArrowDown') cur.value = live[(at + 1) % live.length].i
  else if (e.key === 'ArrowUp') cur.value = live[(at - 1 + live.length) % live.length].i
  else if (e.key === 'Enter' && cur.value >= 0) pick(props.items[cur.value])
  else return
  e.preventDefault()
}
function onDown(e: PointerEvent) {
  if (!box.value?.contains(e.target as Node)) emit('close')
}
onMounted(async () => {
  window.addEventListener('keydown', onKey, true)
  window.addEventListener('pointerdown', onDown, true)
  await nextTick()
  const r = box.value?.getBoundingClientRect()
  if (r) pos.value = { x: Math.max(6, Math.min(props.x, window.innerWidth - r.width - 6)), y: Math.max(6, Math.min(props.y, window.innerHeight - r.height - 6)) }
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey, true)
  window.removeEventListener('pointerdown', onDown, true)
})
</script>

<template>
  <Teleport to="body">
    <div ref="box" class="tmenu" role="menu" :style="{ left: pos.x + 'px', top: pos.y + 'px' }">
      <div v-if="title" class="ttl">{{ title }}</div>
      <template v-for="(it, i) in items" :key="i">
        <div v-if="it.divider" class="sep" role="separator" />
        <button v-else type="button" role="menuitem" class="it" :class="{ danger: it.danger, on: cur === i }" :disabled="it.disabled" @mouseenter="cur = i" @click="pick(it)">
          <b>{{ it.checked ? '✓ ' : '' }}{{ it.label }}</b>
          <small v-if="it.desc">{{ it.desc }}</small>
        </button>
      </template>
    </div>
  </Teleport>
</template>

<style scoped>
.tmenu {
  position: fixed;
  z-index: 4000;
  min-width: 190px;
  max-width: 300px;
  padding: 4px;
  background: var(--el-bg-color, #fff);
  border: 1px solid var(--el-border-color, #cfd5e0);
  border-radius: 10px;
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.22);
  font-size: 13px;
}
.ttl { padding: 5px 10px; font-size: 11.5px; font-weight: 700; color: var(--el-text-color-secondary) }
.sep { height: 1px; margin: 4px 6px; background: var(--el-border-color-lighter, #e4e7ec) }
.it {
  display: flex;
  flex-direction: column;
  gap: 1px;
  width: 100%;
  text-align: left;
  padding: 6px 10px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--el-text-color-primary);
  font: inherit;
  cursor: pointer;
}
.it small { color: var(--el-text-color-secondary); font-size: 11.5px; line-height: 1.35 }
.it:hover:not(:disabled), .it.on:not(:disabled) { background: var(--el-fill-color) }
.it:disabled { opacity: 0.45; cursor: not-allowed }
.it.danger b { color: var(--el-color-danger) }
</style>
