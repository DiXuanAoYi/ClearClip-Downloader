<script setup lang="ts">
import { ref, watch } from 'vue'
import LibStats from './LibStats.vue'
import LibDupes from './LibDupes.vue'
import LibTrash from './LibTrash.vue'
import LibReorganize from './LibReorganize.vue'
import LibCues from './LibCues.vue'
import LibBackup from './LibBackup.vue'

export type ToolTab = 'stats' | 'dupes' | 'cues' | 'reorg' | 'trash' | 'backup'

const props = defineProps<{ modelValue: boolean; tab: ToolTab }>()
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; 'update:tab': [t: ToolTab]; changed: [] }>()

const current = ref<ToolTab>(props.tab)
watch(
  () => props.tab,
  (t) => (current.value = t),
)
watch(current, (t) => emit('update:tab', t))
</script>

<template>
  <el-dialog :model-value="modelValue" title="媒体库工具" width="860px" top="6vh" destroy-on-close @update:model-value="(v: boolean) => emit('update:modelValue', v)">
    <el-tabs v-model="current">
      <el-tab-pane label="统计与清理" name="stats"><LibStats v-if="current === 'stats'" @changed="emit('changed')" /></el-tab-pane>
      <el-tab-pane label="重复文件" name="dupes"><LibDupes v-if="current === 'dupes'" @changed="emit('changed')" /></el-tab-pane>
      <el-tab-pane label="字幕搜索" name="cues"><LibCues v-if="current === 'cues'" /></el-tab-pane>
      <el-tab-pane label="整理文件夹" name="reorg"><LibReorganize v-if="current === 'reorg'" @changed="emit('changed')" /></el-tab-pane>
      <el-tab-pane label="回收站" name="trash"><LibTrash v-if="current === 'trash'" @changed="emit('changed')" /></el-tab-pane>
      <el-tab-pane label="导入与备份" name="backup"><LibBackup v-if="current === 'backup'" @changed="emit('changed')" /></el-tab-pane>
    </el-tabs>
  </el-dialog>
</template>
