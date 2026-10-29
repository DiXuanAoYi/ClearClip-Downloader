<script setup lang="ts">
// 带数值显示的滑块。拖动时不停发出新值，调用方用同一个撤销合并键把一次拖动合成一步。
import EditField from './EditField.vue'

const props = defineProps<{ label: string; modelValue: number; min: number; max: number; step: number; unit?: string; scale?: number; digits?: number; hint?: string; disabled?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [v: number] }>()

const shown = () => {
  const v = props.modelValue * (props.scale ?? 1)
  return `${v.toFixed(props.digits ?? 0)}${props.unit ?? ''}`
}
</script>

<template>
  <EditField :label="label" :hint="hint">
    <el-slider
      class="sl"
      size="small"
      :model-value="modelValue"
      :min="min"
      :max="max"
      :step="step"
      :show-tooltip="false"
      :disabled="disabled"
      @update:model-value="(v: number | number[]) => emit('update:modelValue', Array.isArray(v) ? v[0] : v)"
    />
    <span class="val mono">{{ shown() }}</span>
  </EditField>
</template>

<style scoped>
.sl {
  flex: 1;
  min-width: 0;
  margin: 0 6px 0 4px;
}
.val {
  flex: none;
  width: 54px;
  text-align: right;
  font-size: 11.5px;
}
</style>
