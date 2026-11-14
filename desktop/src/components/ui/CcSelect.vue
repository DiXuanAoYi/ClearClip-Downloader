<script setup lang="ts" generic="T extends string | number">
// 下拉选择：包 el-select，统一高度 / 圆角 / 菜单样式。键盘由 el-select 提供（Enter / 空格展开，上下选择，Esc 关闭）；超过 8 项自动出现搜索。
import { computed } from 'vue'

export interface SelectOption<V> {
  value: V
  label: string
  disabled?: boolean
}
const props = withDefaults(
  defineProps<{
    modelValue: T | undefined
    options: SelectOption<T>[]
    size?: 's' | 'm'
    /** 宽度档：s 120 · m 180 · l 260 · full 通栏。 */
    width?: 's' | 'm' | 'l' | 'full'
    placeholder?: string
    disabled?: boolean
    invalid?: boolean
    /** 不写时：选项多于 8 项自动开启搜索。 */
    filterable?: boolean
    ariaLabel?: string
    id?: string
  }>(),
  // filterable 是 Boolean 属性，不写默认会被 Vue 转成 false；显式默认 undefined，才能走“超过 8 项自动搜索”。
  { size: 'm', width: 'm', placeholder: '请选择', filterable: undefined },
)
const emit = defineEmits<{ 'update:modelValue': [v: T]; change: [v: T] }>()
const canFilter = computed(() => props.filterable ?? props.options.length > 8)
function onChange(v: T) {
  emit('update:modelValue', v)
  emit('change', v)
}
</script>

<template>
  <el-select
    :model-value="modelValue"
    class="cc-select"
    :class="[`s-${size}`, `w-${width}`, { invalid }]"
    :size="size === 's' ? 'small' : 'default'"
    :placeholder="placeholder"
    :disabled="disabled"
    :filterable="canFilter"
    :aria-label="ariaLabel"
    :id="id"
    :aria-invalid="invalid || undefined"
    popper-class="cc-select-pop"
    :teleported="true"
    @update:model-value="onChange"
  >
    <el-option v-for="o in options" :key="String(o.value)" :value="o.value as never" :label="o.label" :disabled="o.disabled" />
  </el-select>
</template>

<style>
.cc-select { --cc-sel-w: 180px; width: var(--cc-sel-w); }
.cc-select.w-s { --cc-sel-w: 120px; }
.cc-select.w-l { --cc-sel-w: 260px; }
.cc-select.w-full { --cc-sel-w: 100%; }
.cc-select .el-select__wrapper {
  min-height: var(--cc-h-md);
  padding: 0 var(--cc-sp-3);
  border-radius: var(--cc-r-md);
  background: var(--cc-fill);
  box-shadow: 0 0 0 1px var(--cc-line-strong) inset;
  font-size: var(--cc-fs-md);
  transition: box-shadow var(--cc-dur-fast) var(--cc-ease-out);
}
.cc-select.s-s .el-select__wrapper { min-height: var(--cc-h-sm); font-size: var(--cc-fs-sm); }
.cc-select .el-select__wrapper.is-hovering:not(.is-focused) { box-shadow: 0 0 0 1px var(--cc-mute) inset; }
.cc-select .el-select__wrapper.is-focused { box-shadow: 0 0 0 1px var(--cc-acc) inset; }
.cc-select .el-select__wrapper.is-disabled { background: var(--cc-card); box-shadow: 0 0 0 1px var(--cc-line) inset; }
.cc-select.invalid .el-select__wrapper { box-shadow: 0 0 0 1px var(--cc-err) inset; }
.cc-select .el-select__selected-item { color: var(--cc-fg); }
.cc-select .el-select__placeholder.is-transparent, .cc-select .el-select__placeholder { color: var(--cc-faint); }
.cc-select .el-select__wrapper:has(input:focus-visible) { outline: var(--cc-focus-w) solid var(--cc-focus); outline-offset: var(--cc-focus-off); }
.cc-select-pop.el-popper { background: var(--cc-card); border: 0; border-radius: var(--cc-r-md); box-shadow: var(--cc-sh-2); }
.cc-select-pop .el-select-dropdown__item { height: 32px; line-height: 32px; margin: 0 var(--cc-sp-1); border-radius: 6px; font-size: var(--cc-fs-sm); color: var(--cc-fg2); }
.cc-select-pop .el-select-dropdown__item.is-hovering { background: var(--cc-hover); color: var(--cc-fg); }
.cc-select-pop .el-select-dropdown__item.is-selected { background: var(--cc-acc-soft); color: var(--cc-acc-text); font-weight: 500; }
.cc-select-pop .el-select-dropdown__item.is-disabled { color: var(--cc-faint); }
.cc-select-pop .el-popper__arrow { display: none; }
</style>
