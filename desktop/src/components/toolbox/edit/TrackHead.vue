<script setup lang="ts">
// 轨道头：编号牌（V1 / A2 / T1）、名字、类型，和一排小开关（静音 / 独奏 / 锁定 / 隐藏）。点编号牌换类型（视频叠加轨），右键有更多操作，双击名字改名。
import { computed, nextTick, ref } from 'vue'
import type { EditTrack } from '../../../types'
import { roleName, trackCode, trackTitle } from '../../../utils/tracks'

const props = defineProps<{ meta: EditTrack; main?: boolean; count?: number }>()
const emit = defineEmits<{ patch: [patch: Partial<EditTrack>]; menu: [e: MouseEvent]; type: [e: MouseEvent] }>()

const code = computed(() => trackCode(props.meta.kind, props.meta.no))
const title = computed(() => trackTitle(props.meta))
const role = computed(() => (props.meta.kind === 'video' && props.meta.no > 0 ? roleName(props.meta.role) : ''))
const hasMute = computed(() => props.meta.kind !== 'text')
const hasSolo = computed(() => props.meta.kind === 'audio')
const hasHide = computed(() => props.meta.kind !== 'audio' && !props.main)
const canType = computed(() => props.meta.kind === 'video' && props.meta.no > 0)

const editing = ref(false)
const draft = ref('')
const input = ref<HTMLInputElement | null>(null)
async function rename() {
  draft.value = props.meta.name
  editing.value = true
  await nextTick()
  input.value?.focus()
  input.value?.select()
}
function done(save: boolean) {
  if (!editing.value) return
  editing.value = false
  if (save && draft.value.trim() !== props.meta.name) emit('patch', { name: draft.value.trim() })
}
defineExpose({ rename })
</script>

<template>
  <div class="th" :class="[meta.kind, { locked: meta.lock, off: meta.hidden }]" :style="meta.color ? { '--tc': meta.color } : undefined" @contextmenu.prevent="emit('menu', $event)">
    <button v-if="canType" type="button" class="chip" :title="`${code}：点一下选类型（画中画 / 全屏 / 调整图层 / 绿幕）`" @click="emit('type', $event)">{{ code }}<i>▾</i></button>
    <span v-else class="chip plain">{{ code }}</span>
    <input v-if="editing" ref="input" v-model="draft" class="nm-in" maxlength="40" @keydown.enter.prevent="done(true)" @keydown.esc.prevent="done(false)" @blur="done(true)" />
    <span v-else class="nm" :title="title" @dblclick="rename">{{ title }}</span>
    <span v-if="role && !editing" class="role" :class="meta.role">{{ role }}</span>
    <span v-else-if="meta.magnet && meta.kind === 'video' && !editing" class="role mag" title="磁吸：素材首尾相接">磁吸</span>
    <span class="btns">
      <button v-if="hasMute" type="button" class="b" :class="{ on: meta.mute }" :aria-pressed="meta.mute" :title="meta.mute ? '取消静音' : '静音这条轨道'" aria-label="静音" @click="emit('patch', { mute: !meta.mute })">M</button>
      <button v-if="hasSolo" type="button" class="b solo" :class="{ on: meta.solo }" :aria-pressed="meta.solo" :title="meta.solo ? '取消独奏' : '独奏：预览时只听这一条（导出不受影响）'" aria-label="独奏" @click="emit('patch', { solo: !meta.solo })">S</button>
      <button v-if="hasHide" type="button" class="b" :class="{ on: meta.hidden }" :aria-pressed="meta.hidden" :title="meta.hidden ? '显示这条轨道' : '隐藏这条轨道（预览和导出都不显示）'" aria-label="隐藏" @click="emit('patch', { hidden: !meta.hidden })">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /><path v-if="meta.hidden" d="M3 3l18 18" /></svg>
      </button>
      <button type="button" class="b" :class="{ on: meta.lock }" :aria-pressed="meta.lock" :title="meta.lock ? '解锁' : '锁定：不能拖动、裁剪、删除这条轨道上的素材'" aria-label="锁定" @click="emit('patch', { lock: !meta.lock })">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2" /><path :d="meta.lock ? 'M8 11V8a4 4 0 0 1 8 0v3' : 'M8 11V8a4 4 0 0 1 7.5-2'" /></svg>
      </button>
    </span>
  </div>
</template>

<style scoped>
.th {
  --tc: var(--cc-mute);
  height: 100%;
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0 5px 0 6px;
  box-sizing: border-box;
  min-width: 0;
}
.th.video { --tc: #2f6f8f }
.th.audio { --tc: #b9812d }
.th.text { --tc: #4f7cc9 }
.chip {
  flex: none;
  height: 20px;
  min-width: 26px;
  padding: 0 5px;
  border-radius: 5px;
  background: var(--tc);
  color: #fff;
  font: 700 11px/20px var(--cc-mono, ui-monospace, monospace);
  border: 0;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 2px;
}
.chip.plain { cursor: default }
.chip i { font-style: normal; font-size: 9px; opacity: 0.85 }
.nm {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  font-weight: 600;
  color: var(--cc-fg);
}
.nm-in {
  flex: 1;
  min-width: 0;
  height: 20px;
  font: inherit;
  font-size: 11.5px;
  border: 1px solid var(--cc-acc);
  border-radius: 4px;
  padding: 0 4px;
  background: var(--cc-card);
  color: var(--cc-fg);
}
.role {
  flex: none;
  font-size: 10px;
  padding: 0 5px;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--tc) 50%, transparent);
  color: var(--tc);
  white-space: nowrap;
}
.role.mag { color: var(--cc-acc); border-color: color-mix(in srgb, var(--cc-acc) 50%, transparent) }
.btns { flex: none; display: flex; gap: 2px }
.b {
  width: 19px;
  height: 19px;
  padding: 0;
  border: 1px solid var(--cc-line);
  border-radius: 4px;
  background: var(--cc-card);
  color: var(--cc-mute);
  font: 700 10px/1 system-ui, sans-serif;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.b:hover { color: var(--cc-fg) }
.b.on { background: color-mix(in srgb, var(--cc-acc) 18%, var(--cc-card)); color: var(--cc-acc); border-color: var(--cc-acc) }
.b.solo.on { background: #e0a21a; color: #fff; border-color: #e0a21a }
.th.off .nm, .th.off .chip { opacity: 0.55 }
.th.locked .nm::after { content: ' 🔒'; font-size: 10px }
.b:focus-visible, .chip:focus-visible { outline: 2px solid var(--cc-acc); outline-offset: 1px }
</style>
