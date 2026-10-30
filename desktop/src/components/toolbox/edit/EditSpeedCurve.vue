<script setup lang="ts">
// 平滑变速曲线编辑器：横轴是片段的素材范围，纵轴是速度（对数刻度，0.25× – 4×）。
// 点空白处加控制点，拖动控制点改位置和速度，双击控制点删除。应用后按曲线拆成许多小段，相邻两段速度只差一点点。
import { computed, ref } from 'vue'
import type { EditClip } from '../../../types'
import { SPEED_MAX, SPEED_MIN, SPEED_RAMPS, curveSteps, msText, rampPieceCount, rampPoints, speedAt, type SpeedPoint } from '../../../utils/edit'

const props = defineProps<{ clip: EditClip }>()
const emit = defineEmits<{ apply: [points: SpeedPoint[]]; cancel: [] }>()

const W = 300
const H = 120
const PAD = 8
const lo = Math.log(SPEED_MIN)
const hi = Math.log(SPEED_MAX)
const px = (x: number) => PAD + x * (W - PAD * 2)
const py = (v: number) => PAD + (1 - (Math.log(v) - lo) / (hi - lo)) * (H - PAD * 2)
const fromX = (cx: number) => Math.min(1, Math.max(0, (cx - PAD) / (W - PAD * 2)))
const fromY = (cy: number) => Math.exp(lo + (1 - (cy - PAD) / (H - PAD * 2)) * (hi - lo))

const points = ref<SpeedPoint[]>([
  { x: 0, speed: 1 },
  { x: 1, speed: 1 },
])
const sorted = computed(() => [...points.value].sort((a, b) => a.x - b.x))
const path = computed(() => {
  const n = 80
  return Array.from({ length: n + 1 }, (_, i) => `${i ? 'L' : 'M'}${px(i / n).toFixed(1)},${py(speedAt(points.value, i / n)).toFixed(1)}`).join(' ')
})
const pieces = computed(() => rampPieceCount(props.clip.outMs - props.clip.inMs))
/** 套上曲线后这个片段的时长（素材范围 ÷ 各段速度，再乘上片段原来的速度） */
const resultMs = computed(() => {
  const n = pieces.value
  if (!n) return 0
  const span = props.clip.outMs - props.clip.inMs
  return Math.round(curveSteps(points.value, n).reduce((t, v) => t + span / n / Math.min(4, Math.max(0.25, v * props.clip.speed)), 0))
})
const nowMs = computed(() => Math.round((props.clip.outMs - props.clip.inMs) / Math.max(0.01, props.clip.speed)))

const svg = ref<SVGSVGElement>()
function local(e: PointerEvent): { x: number; y: number } {
  const r = svg.value!.getBoundingClientRect()
  return { x: ((e.clientX - r.left) / r.width) * W, y: ((e.clientY - r.top) / r.height) * H }
}
let drag = -1
function down(i: number, e: PointerEvent) {
  drag = i
  ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
  e.stopPropagation()
}
function move(e: PointerEvent) {
  if (drag < 0) return
  const p = local(e)
  points.value[drag] = { x: Math.round(fromX(p.x) * 1000) / 1000, speed: Math.round(Math.min(SPEED_MAX, Math.max(SPEED_MIN, fromY(p.y))) * 100) / 100 }
}
function up() {
  drag = -1
}
function add(e: MouseEvent) {
  const r = svg.value!.getBoundingClientRect()
  const x = fromX(((e.clientX - r.left) / r.width) * W)
  const v = fromY(((e.clientY - r.top) / r.height) * H)
  points.value.push({ x: Math.round(x * 1000) / 1000, speed: Math.round(Math.min(SPEED_MAX, Math.max(SPEED_MIN, v)) * 100) / 100 })
}
function remove(i: number) {
  if (points.value.length > 2) points.value.splice(i, 1)
}
function preset(steps: number[]) {
  points.value = rampPoints(steps)
}
const guide = [0.25, 0.5, 1, 2, 4]
</script>

<template>
  <div class="sc">
    <div class="presets">
      <el-button v-for="r in SPEED_RAMPS" :key="r.id" size="small" :title="r.hint" @click="preset(r.steps)">{{ r.label }}</el-button>
    </div>
    <svg ref="svg" class="plot" :viewBox="`0 0 ${W} ${H}`" @click="add">
      <line v-for="g in guide" :key="g" :x1="PAD" :x2="W - PAD" :y1="py(g)" :y2="py(g)" :class="['g', { one: g === 1 }]" />
      <text v-for="g in guide" :key="`t${g}`" :x="W - PAD" :y="py(g) - 2" class="gt">{{ g }}×</text>
      <path :d="path" class="curve" />
      <circle
        v-for="(p, i) in points"
        :key="i"
        :cx="px(p.x)"
        :cy="py(p.speed)"
        r="5"
        class="pt"
        @pointerdown="down(i, $event)"
        @pointermove="move"
        @pointerup="up"
        @click.stop
        @dblclick.stop="remove(i)"
      >
        <title>{{ p.speed }}×（双击删除）</title>
      </circle>
    </svg>
    <div class="mute small">点空白处加控制点，拖动改位置和速度，双击删除。左边是片段开头，右边是结尾。</div>
    <div class="mute small">
      <template v-if="pieces">
        将拆成 {{ pieces }} 小段；片段时长 {{ msText(nowMs) }} → {{ msText(resultMs) }}（共 {{ sorted.length }} 个控制点）
      </template>
      <template v-else>片段太短，不能套平滑变速（至少 0.6 秒）。</template>
    </div>
    <div class="row">
      <el-button type="primary" size="small" :disabled="!pieces" @click="emit('apply', sorted)">应用曲线</el-button>
      <el-button size="small" @click="emit('cancel')">取消</el-button>
    </div>
  </div>
</template>

<style scoped>
.sc {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.presets,
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.presets :deep(.el-button + .el-button),
.row :deep(.el-button + .el-button) {
  margin-left: 0;
}
.plot {
  width: 100%;
  background: var(--el-fill-color-light);
  border-radius: 6px;
  touch-action: none;
  cursor: crosshair;
}
.g {
  stroke: var(--el-border-color);
  stroke-width: 0.6;
}
.g.one {
  stroke: var(--el-text-color-secondary);
  stroke-dasharray: 3 3;
}
.gt {
  font-size: 7px;
  text-anchor: end;
  fill: var(--el-text-color-secondary);
}
.curve {
  fill: none;
  stroke: var(--el-color-primary);
  stroke-width: 2;
}
.pt {
  fill: var(--el-bg-color);
  stroke: var(--el-color-primary);
  stroke-width: 2;
  cursor: grab;
}
.small {
  font-size: 11.5px;
}
</style>
