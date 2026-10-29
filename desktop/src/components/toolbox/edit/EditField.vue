<script setup lang="ts">
// 检查器里的一行：左边名称，右边控件。
defineProps<{ label: string; hint?: string }>()
</script>

<template>
  <div class="f" :title="hint">
    <span class="n">{{ label }}</span>
    <div class="c"><slot /></div>
  </div>
</template>

<style scoped>
.f {
  display: grid;
  grid-template-columns: 74px 1fr;
  gap: 8px;
  align-items: center;
  min-height: 28px;
}
.n {
  font-size: 12.5px;
  color: var(--cc-mute);
}
.c {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
</style>
