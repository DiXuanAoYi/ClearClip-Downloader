<script setup lang="ts">
// 骨架屏：单个色块，或 lines 行文字占位。减少动态效果时不闪动（见 theme.css）。
import { computed } from 'vue'

const props = withDefaults(defineProps<{ width?: string; height?: number | string; radius?: string; circle?: boolean; lines?: number; label?: string }>(), { width: '100%', height: 14, label: '加载中' })
const h = computed(() => (typeof props.height === 'number' ? `${props.height}px` : props.height))
const widths = (i: number, n: number) => (i === n - 1 && n > 1 ? '60%' : '100%')
</script>

<template>
  <span v-if="lines && lines > 1" class="cc-sk-lines" role="status" :aria-label="label">
    <span v-for="i in lines" :key="i" class="cc-sk" aria-hidden="true" :style="{ width: widths(i - 1, lines), height: h }" />
  </span>
  <span v-else class="cc-sk" role="status" :aria-label="label" :style="{ width: circle ? h : width, height: h, borderRadius: circle ? '50%' : radius }" />
</template>

<style>
.cc-sk {
  display: block;
  border-radius: var(--cc-r-sm);
  background: linear-gradient(90deg, var(--cc-skel) 0%, var(--cc-skel-hi) 50%, var(--cc-skel) 100%) 0 0 / 200% 100%;
  animation: cc-sk 1.4s var(--cc-ease-io) infinite;
}
.cc-sk-lines { display: flex; flex-direction: column; gap: var(--cc-sp-2); width: 100%; }
@keyframes cc-sk { from { background-position: 120% 0; } to { background-position: -80% 0; } }
@media (prefers-reduced-motion: reduce) {
  .cc-sk { animation: none; background: var(--cc-skel); }
}
</style>
