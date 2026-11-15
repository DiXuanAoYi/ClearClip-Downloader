<script setup lang="ts">
// 页头：每页的大标题已由左侧导航高亮代替，这里只留一个视觉隐藏的 h1（读屏 / 文档大纲用）。
// 需要的话，默认插槽放一条紧凑的单行工具栏（说明文字、状态徽标、页面操作），不再占一整块标题区。
defineProps<{ title: string }>()
</script>

<template>
  <h1 class="dh-sr">{{ title }}</h1>
  <div v-if="$slots.default" class="dhead"><slot /></div>
</template>

<style>
.dh-sr { position: absolute; width: 1px; height: 1px; margin: -1px; padding: 0; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; border: 0; }
.dhead {
  display: flex;
  align-items: center;
  gap: 10px;
  max-height: 40px;
  min-width: 0;
}
.dhead > .dgrow { flex: 1 1 0; min-width: 0; margin: 0; }
.dhead > .dgrow.one { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.dhead > .chip { flex: none; }
</style>
