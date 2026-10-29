<script setup lang="ts">
// “区域”页：给所选片段加跟着物体走的区域——框选或感知运动的物体、追踪、手动校正，再选效果（马赛克 / 模糊 / 局部调色 / 跟随聚焦）。
import { computed, onBeforeUnmount, onMounted } from 'vue'
import type { EditApi } from '../../../composables/useEditProject'
import type { RegionApi } from '../../../composables/useRegions'
import type { EditRegion, RegionEffect } from '../../../types'
import { MAX_REGIONS, MIN_MS, REGION_EFFECTS, isStatic, lostSpans, msText, regionTitle, trackSpan } from '../../../utils/edit'
import EditField from './EditField.vue'
import EditSlider from './EditSlider.vue'
import TimeInput from './TimeInput.vue'

const props = defineProps<{ ed: EditApi; rg: RegionApi }>()
const emit = defineEmits<{ goto: [ms: number] }>()
const rg = props.rg

onMounted(() => (rg.tabActive.value = true))
onBeforeUnmount(() => {
  rg.tabActive.value = false
  rg.cancelDraw()
})

const clip = rg.clip
const region = rg.region
const regions = computed(() => clip.value?.regions ?? [])
const busy = computed(() => rg.tracking.running)
const effectOf = (id: RegionEffect) => REGION_EFFECTS.find((e) => e.id === id)

function pr(patch: Partial<EditRegion>, key: string) {
  if (region.value) rg.patch(region.value.id, patch, key)
}

/** 追踪状态的说明 */
const trackText = computed(() => {
  const r = region.value
  if (!r) return ''
  if (isStatic(r.track)) return '还没有追踪：现在只有一个固定的框。'
  const span = trackSpan(r.track)
  const pins = r.track.filter((p) => p.pin).length
  const lost = lostSpans(r.track).reduce((s, [a, b]) => s + (b - a), 0)
  const total = span ? Math.max(1, span[1] - span[0]) : 1
  const pct = Math.round((lost * 100) / total)
  return pct >= 1 ? `已追踪 ${msText(span?.[0] ?? 0)} – ${msText(span?.[1] ?? 0)}（${pins} 个手动点，约 ${pct}% 的时间追踪不到）` : `已追踪 ${msText(span?.[0] ?? 0)} – ${msText(span?.[1] ?? 0)}（${pins} 个手动点）`
})
const hasLost = computed(() => !!region.value && lostSpans(region.value.track).length > 0)

function jumpToLost() {
  if (!region.value) return
  const t = rg.nextLost(region.value.id)
  if (t !== null) emit('goto', rg.timelineAt(t))
}
function toClip() {
  const pl = rg.placed.value
  if (pl) emit('goto', pl.startMs + Math.floor(pl.durMs / 2))
}

// 生效时间：只对马赛克 / 模糊 / 局部调色；聚焦始终跟随整个片段
const limited = computed(() => !!region.value && (region.value.startMs !== null || region.value.endMs !== null))
function setLimited(on: unknown) {
  const c = clip.value
  if (!c) return
  pr(on ? { startMs: c.inMs, endMs: c.outMs } : { startMs: null, endMs: null }, 'range')
}
function stamp(which: 'start' | 'end') {
  const r = region.value
  const c = clip.value
  if (!r || !c) return
  const at = rg.srcMs.value
  if (which === 'start') pr({ startMs: Math.min(at, (r.endMs ?? c.outMs) - MIN_MS) }, 'range')
  else pr({ endMs: Math.max(at, (r.startMs ?? c.inMs) + MIN_MS) }, 'range')
}

const noteClass = computed(() => (rg.note.value ? `note ${rg.note.value.kind}` : ''))
</script>

<template>
  <div class="reg">
    <div v-if="!clip" class="mute small">在时间线上点选一个视频片段，再来给它加区域。区域会跟着画面里的物体走，可以用来遮住人脸、车牌，只调某个物体的颜色，或者让画面始终对准它。</div>

    <template v-else>
      <div class="mute small">先把播放位置停在物体清楚可见的那一帧，再加区域：可以点“感知”出来的物体，也可以自己拖出一个框。然后点“开始追踪”，它就会在整个片段里跟着物体走。</div>

      <div v-if="!rg.here.value" class="alert soft">
        <span>播放位置不在这个片段里。</span>
        <el-button size="small" type="primary" @click="toClip">移到片段中间</el-button>
      </div>

      <EditField label="添加区域">
        <div class="adds">
          <el-button v-for="e in REGION_EFFECTS" :key="e.id" size="small" :title="e.hint" :disabled="busy || regions.length >= MAX_REGIONS" :type="rg.drawing.value && rg.drawEffect.value === e.id ? 'primary' : 'default'" @click="rg.startDraw(e.id)">{{ e.label }}</el-button>
        </div>
      </EditField>
      <div v-if="rg.drawing.value" class="alert soft">
        <span v-if="rg.detecting.value">正在感知这一帧里运动的物体…</span>
        <span v-else>
          <span>在监视器里选一个物体：点虚线框，或拖出一个框。</span>
          <span v-if="!rg.candidates.value.length" class="sub">没有感知到运动的物体时，直接拖出框就行。</span>
        </span>
        <span class="acts">
          <el-button size="small" link type="primary" :disabled="rg.detecting.value || !rg.here.value" @click="rg.detect()">重新感知</el-button>
          <el-button size="small" link @click="rg.cancelDraw()">取消</el-button>
        </span>
      </div>
      <div v-if="regions.length >= MAX_REGIONS" class="mute small pad">一个片段最多 {{ MAX_REGIONS }} 个区域。</div>

      <div v-if="regions.length" class="list">
        <div v-for="(r, i) in regions" :key="r.id" class="item" :class="{ on: r.id === rg.selId.value }" @click="rg.select(r.id)">
          <span class="chip">{{ effectOf(r.effect)?.label }}</span>
          <span class="ellipsis nm" data-no-i18n>{{ regionTitle(r, i) }}</span>
          <span v-if="isStatic(r.track)" class="mute small">未追踪</span>
          <el-button size="small" link type="danger" @click.stop="rg.remove(r.id)">删除</el-button>
        </div>
      </div>
      <div v-else-if="!rg.drawing.value" class="mute small pad">这个片段还没有区域。</div>

      <div v-if="rg.note.value" :class="noteClass">{{ rg.note.value.text }}</div>

      <template v-if="region">
        <div class="hr" />
        <EditField label="名称">
          <el-input :model-value="region.name" size="small" placeholder="可以留空" clearable maxlength="30" @update:model-value="(v: string) => pr({ name: v }, 'name')" />
        </EditField>

        <!-- 追踪 -->
        <div class="trk">
          <div class="small">{{ trackText }}</div>
          <div v-if="busy" class="prog">
            <el-progress :percentage="Math.round(rg.tracking.percent)" :stroke-width="8" />
            <el-button size="small" @click="rg.cancelTrack()">取消</el-button>
          </div>
          <div v-else class="acts">
            <el-button size="small" type="primary" :disabled="!rg.here.value" @click="rg.follow(region.id)">{{ isStatic(region.track) ? '开始追踪' : '从这一帧重新追踪' }}</el-button>
            <el-button size="small" :disabled="!rg.pinHere.value" @click="rg.unpin(region.id)">去掉这个手动点</el-button>
            <el-button v-if="hasLost" size="small" @click="jumpToLost">到下一处追踪不到的地方</el-button>
          </div>
          <div class="mute small">追踪从播放位置这一帧的框出发，向前向后走完整个片段。物体被挡住或追偏了：把播放位置移到那里，在监视器里把框拖回物体上（会留下一个手动点，重新追踪也不会改它），再点“从这一帧重新追踪”。拖角上的小方块改变大小，按住 Shift 只改这一刻。</div>
        </div>

        <div class="hr" />
        <EditField label="效果">
          <el-radio-group :model-value="region.effect" size="small" @update:model-value="(v: string | number | boolean | undefined) => pr({ effect: v as RegionEffect }, 'effect')">
            <el-radio-button v-for="e in REGION_EFFECTS" :key="e.id" :value="e.id" :title="e.hint">{{ e.label }}</el-radio-button>
          </el-radio-group>
        </EditField>

        <template v-if="region.effect === 'focus'">
          <EditSlider label="放大" :model-value="region.zoom" :min="1" :max="4" :step="0.05" unit="×" :digits="2" hint="1 = 不放大，只在 “按成片比例取景” 时裁切" @update:model-value="(v) => pr({ zoom: v }, 'zoom')" />
          <EditField label="">
            <el-checkbox :model-value="region.reframe" @update:model-value="(v: unknown) => pr({ reframe: !!v }, 'reframe')">按成片的比例取景（横屏素材裁成竖屏，跟着人走）</el-checkbox>
          </EditField>
          <EditSlider label="镜头平滑" :model-value="region.smooth" :min="0" :max="5" :step="0.1" unit=" 秒" :digits="1" hint="越大镜头越稳、跟得越慢" @update:model-value="(v) => pr({ smooth: v }, 'smooth')" />
          <div class="mute small pad">一个片段只有一个“跟随聚焦”，它改变整个镜头的取景，不受“生效时间”限制。监视器里橙色虚线框是取景窗口的示意位置（没有算镜头平滑）。</div>
        </template>

        <template v-else>
          <EditField label="形状">
            <el-radio-group :model-value="region.shape" size="small" @update:model-value="(v: string | number | boolean | undefined) => pr({ shape: v as EditRegion['shape'] }, 'shape')">
              <el-radio-button value="rect">方形</el-radio-button>
              <el-radio-button value="ellipse">椭圆</el-radio-button>
            </el-radio-group>
          </EditField>
          <EditSlider label="边缘羽化" :model-value="region.feather" :min="0" :max="1" :step="0.01" unit="%" :scale="100" hint="边缘从有效果渐变到没有效果" @update:model-value="(v) => pr({ feather: v }, 'feather')" />
          <EditSlider label="范围扩大" :model-value="region.grow" :min="-0.5" :max="2" :step="0.01" unit="%" :scale="100" hint="比框大多少（负数是缩小）。物体动得快、框跟不太准时，放大一点更保险" @update:model-value="(v) => pr({ grow: v }, 'grow')" />
          <EditField label="">
            <el-checkbox :model-value="region.invert" @update:model-value="(v: unknown) => pr({ invert: !!v }, 'invert')">作用于区域以外（保留区域里的，处理其余部分）</el-checkbox>
          </EditField>

          <template v-if="region.effect === 'tone'">
            <EditSlider label="亮度" :model-value="region.brightness" :min="-1" :max="1" :step="0.01" :scale="100" :digits="0" @update:model-value="(v) => pr({ brightness: v }, 'tone')" />
            <EditSlider label="对比度" :model-value="region.contrast" :min="0" :max="3" :step="0.01" :scale="100" unit="%" @update:model-value="(v) => pr({ contrast: v }, 'tone')" />
            <EditSlider label="饱和度" :model-value="region.saturation" :min="0" :max="3" :step="0.01" :scale="100" unit="%" @update:model-value="(v) => pr({ saturation: v }, 'tone')" />
          </template>
          <EditSlider v-else :label="region.effect === 'mosaic' ? '马赛克大小' : '模糊程度'" :model-value="region.strength" :min="0" :max="1" :step="0.01" unit="%" :scale="100" @update:model-value="(v) => pr({ strength: v }, 'strength')" />

          <EditField label="生效时间">
            <el-checkbox :model-value="limited" @update:model-value="setLimited">只在一段时间里生效</el-checkbox>
          </EditField>
          <template v-if="limited && clip">
            <EditField label="从">
              <TimeInput :model-value="region.startMs ?? clip.inMs" :min="0" :max="(region.endMs ?? clip.outMs) - MIN_MS" @update:model-value="(v) => pr({ startMs: v }, 'range')" />
              <el-button size="small" link @click="stamp('start')">取播放位置</el-button>
            </EditField>
            <EditField label="到">
              <TimeInput :model-value="region.endMs ?? clip.outMs" :min="(region.startMs ?? clip.inMs) + MIN_MS" @update:model-value="(v) => pr({ endMs: v }, 'range')" />
              <el-button size="small" link @click="stamp('end')">取播放位置</el-button>
            </EditField>
            <div class="mute small pad">这里的时间是素材自己的时间，和“入点 / 出点”一致。</div>
          </template>
          <div class="mute small pad">监视器里的马赛克 / 模糊 / 调色只是近似的示意（也不显示“作用于区域以外”），准确效果请生成“精确预览”或导出查看。</div>
        </template>
      </template>
    </template>
  </div>
</template>

<style scoped>
.reg {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.small {
  font-size: 11.5px;
}
.pad {
  padding-left: 82px;
}
.sub {
  margin-left: 0.3em;
}
.adds {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
}
.adds :deep(.el-button + .el-button),
.acts :deep(.el-button + .el-button) {
  margin-left: 0;
}
/* 复选框的说明比较长：允许换行，不要被截掉 */
.reg :deep(.el-checkbox) {
  height: auto;
  min-height: 24px;
  align-items: flex-start;
  white-space: normal;
}
.reg :deep(.el-checkbox__label) {
  white-space: normal;
  line-height: 1.45;
}
.reg :deep(.el-checkbox__input) {
  margin-top: 2px;
}
.hr {
  height: 1px;
  background: var(--cc-border, rgba(127, 127, 127, 0.25));
  margin: 4px 0;
}
.alert {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 6px;
  background: var(--cc-acc-soft);
  color: var(--cc-fg);
  font-size: 12px;
}
.acts {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  border-radius: 6px;
  border: 1px solid transparent;
  cursor: pointer;
  min-width: 0;
}
.item:hover {
  background: var(--cc-acc-soft);
}
.item.on {
  border-color: var(--el-color-primary);
  background: var(--cc-acc-soft);
}
.nm {
  flex: 1;
  font-size: 12.5px;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}
.note {
  font-size: 12px;
  padding: 6px 10px;
  border-radius: 6px;
  background: var(--cc-acc-soft);
}
.note.warn {
  background: color-mix(in srgb, #e6a23c 18%, transparent);
}
.note.error {
  background: color-mix(in srgb, var(--cc-err) 14%, transparent);
  color: var(--cc-err);
}
.trk {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.prog {
  display: flex;
  align-items: center;
  gap: 8px;
}
.prog :deep(.el-progress) {
  flex: 1;
}
</style>
