<script setup lang="ts">
// 时间输入框：显示 `m:ss.mmm`，可以输入秒数（83.5）、分:秒（1:23.5）、时:分:秒。回车或离开输入框时生效，写错了就恢复原来的。
import { ref, watch } from 'vue'
import { msText, parseMs } from '../../../utils/edit'

const props = defineProps<{ modelValue: number; min?: number; max?: number; disabled?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [ms: number] }>()

const text = ref(msText(props.modelValue))
const bad = ref(false)
watch(
  () => props.modelValue,
  (v) => {
    text.value = msText(v)
    bad.value = false
  },
)

function commit() {
  const v = parseMs(text.value)
  if (v === null || v < (props.min ?? 0) || v > (props.max ?? Number.MAX_SAFE_INTEGER)) {
    bad.value = true
    setTimeout(() => (bad.value = false), 1200)
    text.value = msText(props.modelValue)
    return
  }
  bad.value = false
  text.value = msText(v)
  if (v !== props.modelValue) emit('update:modelValue', v)
}
</script>

<template>
  <el-input v-model="text" size="small" class="ti mono" :class="{ bad }" :disabled="disabled" @change="commit" @blur="commit" @keydown.enter="commit" />
</template>

<style scoped>
.ti {
  width: 104px;
}
.ti :deep(.el-input__inner) {
  font-family: var(--cc-mono);
  font-size: 12px;
}
.bad :deep(.el-input__wrapper) {
  box-shadow: 0 0 0 1px var(--cc-err) inset;
}
</style>
