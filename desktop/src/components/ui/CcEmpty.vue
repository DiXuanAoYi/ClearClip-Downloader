<script setup lang="ts">
// 空状态：图 + 标题 + 说明 + 主操作。compact 是横排的紧凑版（放在卡片里）。
defineProps<{ title: string; desc?: string; compact?: boolean }>()
</script>

<template>
  <div class="cc-empty" :class="{ cmp: compact }">
    <div class="art" aria-hidden="true">
      <slot name="art">
        <svg width="28" height="28" viewBox="0 0 28 28"><rect x="4" y="6" width="20" height="16" rx="3" fill="none" stroke="currentColor" stroke-width="1.8" /><path d="M4 12h20" stroke="currentColor" stroke-width="1.8" /></svg>
      </slot>
    </div>
    <div class="tx">
      <h4>{{ title }}</h4>
      <p v-if="desc || $slots.desc"><slot name="desc">{{ desc }}</slot></p>
    </div>
    <div v-if="$slots.default" class="act"><slot /></div>
  </div>
</template>

<style>
.cc-empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: var(--cc-sp-2); padding: var(--cc-sp-8) var(--cc-sp-6); }
.cc-empty .art { width: 64px; height: 64px; border-radius: 16px; display: grid; place-items: center; color: var(--cc-acc-text); background: var(--cc-acc-soft); border: 1px solid var(--cc-acc-line); margin-bottom: var(--cc-sp-2); }
.cc-empty .tx { display: flex; flex-direction: column; align-items: center; gap: var(--cc-sp-2); }
.cc-empty h4 { margin: 0; font-size: var(--cc-fs-lg); font-weight: 700; line-height: 1.3; color: var(--cc-fg); }
.cc-empty p { margin: 0; font-size: var(--cc-fs-sm); color: var(--cc-mute); max-width: 320px; line-height: var(--cc-lh-base); }
.cc-empty .act { display: flex; gap: var(--cc-sp-2); margin-top: var(--cc-sp-3); align-items: center; flex-wrap: wrap; justify-content: center; }
.cc-empty.cmp { flex-direction: row; text-align: left; padding: var(--cc-sp-4); gap: var(--cc-sp-4); }
.cc-empty.cmp .art { width: 40px; height: 40px; border-radius: 10px; margin: 0; flex: none; }
.cc-empty.cmp .tx { align-items: flex-start; gap: var(--cc-sp-1); min-width: 0; }
.cc-empty.cmp .act { margin: 0 0 0 auto; }
</style>
