<script setup lang="ts" generic="T extends string | number">
// 分段选择：同一块内容的 2–4 种视图 / 模式。单选 radiogroup，方向键切换并选中，Home / End 到首尾。
import { computed, ref } from 'vue'

export interface SegOption<V> {
  value: V
  label: string
  disabled?: boolean
  /** 禁用或需要解释时的悬停提示。 */
  hint?: string
}
const props = withDefaults(defineProps<{ modelValue: T; options: SegOption<T>[]; size?: 's' | 'm'; ariaLabel?: string; disabled?: boolean }>(), { size: 'm' })
const emit = defineEmits<{ 'update:modelValue': [v: T] }>()
const btns = ref<HTMLButtonElement[]>([])

const enabled = (i: number) => !props.disabled && !props.options[i]?.disabled
const current = computed(() => props.options.findIndex((o) => o.value === props.modelValue))
// 漫游 tabindex：选中项可 Tab 进入；没有选中项时第一个可用项可进入
const tabIndexOf = computed(() => {
  if (current.value >= 0 && enabled(current.value)) return current.value
  return props.options.findIndex((_, i) => enabled(i))
})

function pick(i: number, focus = false) {
  if (!enabled(i)) return
  if (focus) btns.value[i]?.focus()
  if (props.options[i].value !== props.modelValue) emit('update:modelValue', props.options[i].value)
}
function step(from: number, dir: 1 | -1) {
  const n = props.options.length
  for (let k = 1; k <= n; k++) {
    const i = (from + dir * k + n * k) % n
    if (enabled(i)) return i
  }
  return from
}
function onKey(e: KeyboardEvent, i: number) {
  let to = -1
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') to = step(i, 1)
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') to = step(i, -1)
  else if (e.key === 'Home') to = props.options.findIndex((_, k) => enabled(k))
  else if (e.key === 'End') to = props.options.map((_, k) => k).reverse().find((k) => enabled(k)) ?? -1
  if (to < 0) return
  e.preventDefault()
  pick(to, true)
}
</script>

<template>
  <div class="cc-seg" :class="`s-${size}`" role="radiogroup" :aria-label="ariaLabel" :aria-disabled="disabled || undefined">
    <button
      v-for="(o, i) in options"
      :key="String(o.value)"
      :ref="(el) => (btns[i] = el as HTMLButtonElement)"
      type="button"
      role="radio"
      class="seg-item"
      :aria-checked="i === current"
      :aria-disabled="!enabled(i) || undefined"
      :tabindex="i === tabIndexOf ? 0 : -1"
      :title="o.hint"
      :class="{ on: i === current, off: !enabled(i) }"
      @click="pick(i)"
      @keydown="onKey($event, i)"
    >{{ o.label }}</button>
  </div>
</template>

<style>
.cc-seg {
  display: inline-flex;
  max-width: 100%;
  padding: 2px;
  gap: 2px;
  background: var(--cc-fill);
  border: 1px solid var(--cc-line);
  border-radius: var(--cc-r-md);
}
.cc-seg .seg-item {
  height: calc(var(--cc-h-md) - 6px);
  padding: 0 var(--cc-sp-3);
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--cc-mute);
  font: inherit;
  font-size: var(--cc-fs-sm);
  cursor: pointer;
  white-space: nowrap;
  transition: background var(--cc-dur-fast) var(--cc-ease-out), color var(--cc-dur-fast) var(--cc-ease-out);
}
.cc-seg.s-s .seg-item { height: calc(var(--cc-h-sm) - 6px); }
.cc-seg .seg-item:hover:not(.off) { color: var(--cc-fg); }
.cc-seg .seg-item.on {
  background: var(--cc-card);
  color: var(--cc-acc-text);
  box-shadow: var(--cc-sh-1), inset 0 0 0 1px var(--cc-acc-line);
  font-weight: 500;
}
.cc-seg .seg-item.off { color: var(--cc-faint); cursor: not-allowed; }
</style>
