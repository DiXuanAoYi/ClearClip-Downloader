<script setup lang="ts">
// 标准设置行：标签 + 说明（默认最多 2 行，超出折叠为“了解更多”）+ 控件 + 错误。
// 默认插槽给出 { id, descId, errId, invalid }，把它们绑到控件上即可把标签、说明、错误和控件关联起来。
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'

const props = withDefaults(defineProps<{ label: string; desc?: string; error?: string; layout?: 'row' | 'col'; controlId?: string }>(), { layout: 'row' })
const uid = `cc-field-${useId()}`
const id = computed(() => props.controlId || `${uid}-ctl`)
const descId = `${uid}-desc`
const errId = `${uid}-err`
const moreId = `${uid}-more`
const descEl = ref<HTMLElement | null>(null)
const clamped = ref(false)
const expanded = ref(false)
let ro: ResizeObserver | null = null

function measure() {
  const el = descEl.value
  if (!el || expanded.value) return
  // line-clamp 下 scrollHeight 也被截成 2 行，所以临时去掉截断量一次完整高度（同步完成，不会闪）
  const clampedH = el.clientHeight
  el.classList.add('open')
  const fullH = el.clientHeight
  el.classList.remove('open')
  clamped.value = fullH - clampedH > 1
}
onMounted(() => {
  measure()
  if (typeof ResizeObserver !== 'undefined' && descEl.value) {
    ro = new ResizeObserver(measure)
    ro.observe(descEl.value)
  }
})
onBeforeUnmount(() => ro?.disconnect())
watch(() => props.desc, () => nextTick(measure))
const showMore = computed(() => clamped.value || expanded.value)
</script>

<template>
  <div class="cc-field" :class="[`l-${layout}`, { invalid: !!error }]">
    <label class="lab" :for="id"><slot name="label">{{ label }}</slot></label>
    <div v-if="desc || $slots.desc" :id="descId" ref="descEl" class="desc" :class="{ open: expanded }"><slot name="desc">{{ desc }}</slot></div>
    <button
      v-if="showMore"
      :id="moreId"
      class="more cc-link"
      type="button"
      :aria-expanded="expanded"
      :aria-controls="descId"
      @click="expanded = !expanded"
    >{{ expanded ? '收起' : '了解更多' }}</button>
    <div class="ctl"><slot :id="id" :desc-id="descId" :err-id="errId" :invalid="!!error" /></div>
    <p v-if="error" :id="errId" class="errm" role="alert">
      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" stroke-width="1.5" /><path d="M7 4v3.4M7 9.6v.1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
      <span>{{ error }}</span>
    </p>
  </div>
</template>

<style>
.cc-field {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  column-gap: var(--cc-sp-6);
  row-gap: var(--cc-sp-1);
  align-items: center;
  padding: var(--cc-sp-3) 0;
  border-top: 1px solid var(--cc-line-soft);
}
.cc-field:first-child { border-top: 0; }
.cc-field .lab { grid-column: 1; font-size: var(--cc-fs-md); font-weight: 500; color: var(--cc-fg); line-height: 1.4; display: flex; align-items: center; gap: var(--cc-sp-2); flex-wrap: wrap; }
.cc-field .desc {
  grid-column: 1;
  font-size: var(--cc-fs-sm);
  color: var(--cc-mute);
  line-height: var(--cc-lh-base);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.cc-field .desc.open { display: block; -webkit-line-clamp: unset; overflow: visible; }
.cc-field .more { grid-column: 1; justify-self: start; font-size: var(--cc-fs-sm); }
.cc-field .ctl { grid-column: 2; grid-row: 1 / span 4; display: flex; align-items: center; gap: var(--cc-sp-2); justify-content: flex-end; min-width: 0; }
.cc-field.l-col { grid-template-columns: minmax(0, 1fr); }
.cc-field.l-col .ctl { grid-column: 1; grid-row: auto; justify-content: flex-start; flex-wrap: wrap; margin-top: var(--cc-sp-2); }
.cc-field .errm { grid-column: 1 / -1; margin: 0; display: flex; align-items: center; gap: var(--cc-sp-1); font-size: var(--cc-fs-sm); color: var(--cc-err-text); }
.cc-field .errm svg { flex: none; }
</style>
