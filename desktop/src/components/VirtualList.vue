<script setup lang="ts" generic="T">
import { computed, onMounted, onUnmounted, ref } from 'vue'

/**
 * 固定行高的虚拟列表：只渲染可见区域附近的行，上千条任务时界面也不卡。
 * 自己就是滚动容器，父元素需要给它一个确定的高度（如 flex: 1）。
 */
const props = withDefaults(defineProps<{ items: T[]; itemHeight: number; itemKey: (item: T) => string | number; buffer?: number; gap?: number }>(), {
  buffer: 6,
  gap: 0,
})

const el = ref<HTMLElement | null>(null)
const scrollTop = ref(0)
const viewport = ref(600)
const row = computed(() => props.itemHeight + props.gap)

const range = computed(() => {
  const start = Math.max(0, Math.floor(scrollTop.value / row.value) - props.buffer)
  const end = Math.min(props.items.length, Math.ceil((scrollTop.value + viewport.value) / row.value) + props.buffer)
  return { start, end }
})
const visible = computed(() => props.items.slice(range.value.start, range.value.end).map((item, i) => ({ item, index: range.value.start + i })))

function onScroll() {
  if (el.value) scrollTop.value = el.value.scrollTop
}

let ro: ResizeObserver | undefined
onMounted(() => {
  if (!el.value) return
  viewport.value = el.value.clientHeight || 600
  ro = new ResizeObserver(() => {
    if (el.value) viewport.value = el.value.clientHeight
  })
  ro.observe(el.value)
})
onUnmounted(() => ro?.disconnect())
</script>

<template>
  <div ref="el" class="vlist" @scroll.passive="onScroll">
    <div class="spacer" :style="{ height: `${items.length * row}px` }">
      <div v-for="v in visible" :key="itemKey(v.item)" class="vrow" :style="{ transform: `translateY(${v.index * row}px)`, height: `${itemHeight}px` }">
        <slot :item="v.item" :index="v.index" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.vlist {
  overflow-y: auto;
  min-height: 0;
}
.spacer {
  position: relative;
}
.vrow {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
}
</style>
