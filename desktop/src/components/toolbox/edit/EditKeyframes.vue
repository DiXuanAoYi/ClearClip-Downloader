<script setup lang="ts">
// 关键帧面板：给叠加素材 / 文字的位置、大小、旋转、不透明度、音量加“关键点”，中间按曲线自动补值。
// 点菱形在播放头处加（或删）一个关键点；之后拖监视器里的素材、或拖上面的滑块，会在播放头处改（或新建）关键点。
import { computed } from 'vue'
import type { EditApi } from '../../../composables/useEditProject'
import type { EditClip, EditKeyframe, EditOverlay, EditText } from '../../../types'
import { KEY_EASES, MAX_KEYFRAMES, clamp, msText, withKeys } from '../../../utils/edit'
import type { KeyProp } from '../../../utils/edit'

const props = defineProps<{ ed: EditApi; kind: 'overlay' | 'text' | 'clip'; item: EditOverlay | EditText | EditClip; defs: KeyProp[]; durationMs: number; startMs: number }>()
const emit = defineEmits<{ goto: [ms: number] }>()

/** 播放头相对这个素材开头的位置 */
const local = computed(() => Math.round(clamp(props.ed.playhead.value - props.startMs, 0, props.durationMs)))
const eff = computed((): Record<string, number> => ({ zoom: 1, panX: 0, panY: 0, ...(withKeys(props.item, local.value) as unknown as Record<string, number>) }))
/** 离播放头这么近（毫秒）就算“正好在关键点上” */
const NEAR = 40

const keysOf = (id: string): EditKeyframe[] => props.item.keyframes[id] ?? []
const atPlayhead = (id: string) => keysOf(id).find((k) => Math.abs(k.tMs - local.value) <= NEAR)
const shown = (d: KeyProp, v: number) => `${(v * (d.mult ?? 1)).toFixed(d.digits)}${d.unit ?? ''}`

function toggle(d: KeyProp) {
  const k = atPlayhead(d.id)
  if (k) props.ed.deleteKey(props.kind, props.item.id, d.id, k.tMs)
  else props.ed.setKey(props.kind, props.item.id, d.id, local.value, eff.value[d.id])
}
const setEase = (d: KeyProp, k: EditKeyframe, ease: EditKeyframe['ease']) => props.ed.setKey(props.kind, props.item.id, d.id, k.tMs, k.value, ease)
const drop = (d: KeyProp, k: EditKeyframe) => props.ed.deleteKey(props.kind, props.item.id, d.id, k.tMs)
const clear = (d: KeyProp) => props.ed.clearKeys(props.kind, props.item.id, d.id, eff.value[d.id])
const full = (id: string) => keysOf(id).length >= MAX_KEYFRAMES
</script>

<template>
  <div class="kfp">
    <div v-for="d in defs" :key="d.id" class="row">
      <div class="head">
        <el-button
          size="small"
          link
          :type="keysOf(d.id).length ? 'primary' : undefined"
          :disabled="!atPlayhead(d.id) && full(d.id)"
          :title="atPlayhead(d.id) ? '删掉播放头处的关键点' : '在播放头处加一个关键点（用此刻的值）'"
          @click="toggle(d)"
        >
          {{ atPlayhead(d.id) ? '◆' : '◇' }}
        </el-button>
        <span class="n">{{ d.label }}</span>
        <span class="mono val">{{ shown(d, eff[d.id]) }}</span>
        <el-button v-if="keysOf(d.id).length" size="small" link type="danger" title="去掉这个属性的全部关键点，回到固定值" @click="clear(d)">清除</el-button>
      </div>
      <div v-for="k in keysOf(d.id)" :key="k.tMs" class="key" :class="{ now: Math.abs(k.tMs - local) <= NEAR }">
        <el-button size="small" link @click="emit('goto', startMs + k.tMs)">{{ msText(k.tMs) }}</el-button>
        <span class="mono">{{ shown(d, k.value) }}</span>
        <el-select :model-value="k.ease" size="small" class="ease" @update:model-value="(v: EditKeyframe['ease']) => setEase(d, k, v)">
          <el-option v-for="e in KEY_EASES" :key="e.id" :label="e.label" :value="e.id" />
        </el-select>
        <el-button size="small" link @click="drop(d, k)">删除</el-button>
      </div>
    </div>
    <div class="mute small pad">曲线是从这个关键点到下一个关键点之间用的。第一个点之前和最后一个点之后保持不变。</div>
  </div>
</template>

<style scoped>
.kfp {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.row {
  display: flex;
  flex-direction: column;
}
.head {
  display: flex;
  align-items: center;
  gap: 6px;
  min-height: 26px;
}
.n {
  font-size: 12.5px;
  color: var(--cc-mute);
  flex: 1;
}
.val {
  font-size: 11.5px;
}
.key {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-left: 26px;
  font-size: 12px;
}
.key.now {
  background: var(--el-fill-color-light);
  border-radius: 4px;
}
.ease {
  width: 140px;
}
.pad {
  padding: 4px 0;
}
</style>
