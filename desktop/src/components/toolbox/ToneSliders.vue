<script setup lang="ts">
import { computed } from 'vue'
import type { ToneAdjust } from '../../types'

/** 五项手动调节（亮度、对比度、饱和度、色温、色调），每项 -100% 到 +100%，0 为不动。 */
const model = defineModel<ToneAdjust>({ required: true })

const ROWS: { k: keyof ToneAdjust; name: string; hint: string }[] = [
  { k: 'brightness', name: '亮度', hint: '整体变暗 / 变亮' },
  { k: 'contrast', name: '对比度', hint: '明暗反差变小 / 变大' },
  { k: 'saturation', name: '饱和度', hint: '-100% 变成黑白，+100% 颜色浓一倍' },
  { k: 'temperature', name: '色温', hint: '往冷（偏蓝）/ 往暖（偏黄）' },
  { k: 'tint', name: '色调', hint: '往偏绿 / 往偏品红' },
]

const neutral = computed(() => ROWS.every((r) => model.value[r.k] === 0))
const set = (k: keyof ToneAdjust, v: number) => (model.value = { ...model.value, [k]: v })
const reset = () => (model.value = { brightness: 0, contrast: 0, saturation: 0, temperature: 0, tint: 0 })
const pct = (v: number) => `${v > 0 ? '+' : ''}${Math.round(v * 100)}%`
</script>

<template>
  <div class="tone">
    <div v-for="r in ROWS" :key="r.k" class="row" :title="r.hint">
      <span class="name">{{ r.name }}</span>
      <el-slider :model-value="model[r.k]" :min="-1" :max="1" :step="0.05" :show-tooltip="false" size="small" class="sl" @update:model-value="(v: number | number[]) => set(r.k, Array.isArray(v) ? v[0] : v)" />
      <span class="val mono">{{ pct(model[r.k]) }}</span>
    </div>
    <el-button size="small" link type="primary" class="reset" :disabled="neutral" @click="reset">全部归零</el-button>
  </div>
</template>

<style scoped>
.tone {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.row {
  display: grid;
  grid-template-columns: 64px 280px 52px;
  gap: 10px;
  align-items: center;
}
.name {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.val {
  font-size: 11.5px;
  text-align: right;
}
.reset {
  align-self: flex-start;
}
</style>
