<script setup lang="ts">
import { computed } from 'vue'
import { CUSTOM_LOUDNESS, LOUDNESS_PRESETS, findLoudnessPreset, loudnessPresetId } from '../../utils/loudness'

// 响度标准化的目标：先选行业预设，也可以自己填。目标响度、真峰值、响度范围三个数都能改，改了就变成“自定义”。
const lufs = defineModel<number>('lufs', { required: true })
const tp = defineModel<number>('tp', { required: true })
const lra = defineModel<number>('lra', { required: true })

const id = computed(() => loudnessPresetId(lufs.value, tp.value, lra.value))
const note = computed(() => findLoudnessPreset(id.value)?.note ?? '')

function pick(v: string) {
  const p = findLoudnessPreset(v)
  if (!p) return
  lufs.value = p.lufs
  tp.value = p.tp
  lra.value = p.lra
}
</script>

<template>
  <div class="lp">
    <el-select :model-value="id" size="small" class="sel" @update:model-value="pick(String($event))">
      <el-option v-for="p in LOUDNESS_PRESETS" :key="p.id" :value="p.id" :label="`${p.name}（${p.lufs} LUFS）`" />
      <el-option :value="CUSTOM_LOUDNESS" label="自定义" disabled />
    </el-select>
    <div class="row">
      <label><span>目标响度（LUFS）</span><el-input-number v-model="lufs" size="small" :min="-30" :max="-5" :step="1" /></label>
      <label><span>真峰值上限（dBTP）</span><el-input-number v-model="tp" size="small" :min="-9" :max="0" :step="0.5" /></label>
      <label><span>响度范围（LU）</span><el-input-number v-model="lra" size="small" :min="1" :max="20" :step="1" /></label>
    </div>
    <small class="mute">{{ note || '自定义：三个数都按你填的来。目标响度越小声音越轻；真峰值上限防止声音爆音；响度范围越小，轻声和大声之间差得越少。' }}</small>
  </div>
</template>

<style scoped>
.lp { display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
.sel { width: 320px; max-width: 100%; }
.row { display: flex; flex-wrap: wrap; gap: 12px 18px; }
.row label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
.mute { color: var(--cc-mute); }
</style>
