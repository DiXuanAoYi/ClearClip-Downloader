<script setup lang="ts">
// 快捷键录入：点一下进入录入，直接按组合键；Esc 取消，失焦也取消。键盘可用（Tab 到按钮，回车 / 空格开始录入）。
// 不合格的组合（没有修饰键、抢系统常用键、与别的快捷键重复）会就地提示，停留在录入状态。
import { computed, nextTick, ref, useId } from 'vue'
import { checkCombo, comboFromEvent, isModifierCode, sameCombo, type CtrlName } from '../utils/hotkey'
import CcButton from './ui/CcButton.vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    /** 无障碍名称，例如“老板键”。 */
    label: string
    /** Ctrl 键写成什么：默认 CommandOrControl（跨平台），仅 Windows 的功能可用 Ctrl。 */
    ctrl?: CtrlName
    /** 应用里已经占用的其他快捷键，录入时检查冲突。 */
    others?: { name: string; value: string }[]
    /** 外部给出的错误（例如注册失败）。 */
    error?: string | null
    placeholder?: string
    id?: string
  }>(),
  { ctrl: 'CommandOrControl', others: () => [], error: null, placeholder: '未设置' },
)
const emit = defineEmits<{ 'update:modelValue': [v: string] }>()

const recording = ref(false)
const live = ref('')
const problem = ref('')
const btn = ref<HTMLButtonElement | null>(null)
const uid = `hk-${useId()}`
let swallowUp = false

const shown = computed(() => (props.modelValue ? props.modelValue.split('+') : []))
const err = computed(() => problem.value || props.error || '')

function start() {
  if (recording.value) return
  recording.value = true
  live.value = ''
  problem.value = ''
}
function stop() {
  recording.value = false
  live.value = ''
}
function cancel() {
  stop()
  problem.value = ''
}
function onKeyDown(e: KeyboardEvent) {
  if (!recording.value) return
  if (e.key === 'Tab' && !e.ctrlKey && !e.altKey && !e.metaKey) return cancel() // 让 Tab 照常移动焦点
  e.preventDefault()
  e.stopPropagation()
  if (e.key === 'Escape') {
    swallowUp = true
    return cancel()
  }
  if (e.repeat) return
  const combo = comboFromEvent(e, props.ctrl)
  if (isModifierCode(e.code)) {
    live.value = combo
    return
  }
  const bad = checkCombo(combo)
  if (bad === 'noMain') problem.value = '没认出这个按键，换一个'
  else if (bad === 'noModifier') problem.value = '需要和 Ctrl、Alt 或 Shift 一起按（F1–F24 除外）'
  else if (bad === 'reserved') problem.value = `${combo} 是常用的系统快捷键，换一个组合`
  else {
    const hit = props.others.find((o) => o.value && sameCombo(o.value, combo))
    if (hit) problem.value = `和“${hit.name}”用的是同一个组合，换一个`
    else {
      problem.value = ''
      emit('update:modelValue', combo)
      swallowUp = true
      stop()
      return
    }
  }
  live.value = combo
}
function onKeyUp(e: KeyboardEvent) {
  if (swallowUp) {
    swallowUp = false
    e.preventDefault() // 空格 / 回车的 keyup 会再触发一次点击
  }
}
function onClick() {
  if (!recording.value) start()
}
function clear() {
  emit('update:modelValue', '')
  problem.value = ''
  void nextTick(() => btn.value?.focus())
}
</script>

<template>
  <div class="hk">
    <div class="hk-row">
      <button
        :id="id"
        ref="btn"
        type="button"
        class="hk-btn mono"
        :class="{ rec: recording, bad: !!err }"
        :aria-label="`${label}：${modelValue || '未设置'}。${recording ? '正在录入，按下新的组合键，Esc 取消' : '按回车开始录入'}`"
        :aria-describedby="`${uid}-msg`"
        :aria-invalid="!!err || undefined"
        @click="onClick"
        @keydown="onKeyDown"
        @keyup="onKeyUp"
        @blur="cancel"
      >
        <template v-if="recording">
          <span class="hint">{{ live ? live.split('+').join(' + ') + ' …' : '按下新的组合键…' }}</span>
          <span class="esc">Esc 取消</span>
        </template>
        <template v-else-if="shown.length">
          <template v-for="(k, i) in shown" :key="i"><kbd>{{ k }}</kbd><span v-if="i < shown.length - 1" class="plus" aria-hidden="true">+</span></template>
        </template>
        <span v-else class="ph">{{ placeholder }}</span>
      </button>
      <CcButton v-if="modelValue && !recording" kind="text" size="s" @click="clear">清除</CcButton>
    </div>
    <p :id="`${uid}-msg`" class="hk-msg" :class="{ err: !!err }" :role="err ? 'alert' : undefined" aria-live="polite">
      <template v-if="err">
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" stroke-width="1.5" /><path d="M7 4v3.4M7 9.6v.1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
        <span>{{ err }}</span>
      </template>
    </p>
  </div>
</template>

<style scoped>
.hk { display: flex; flex-direction: column; gap: var(--cc-sp-1); min-width: 0; }
.hk-row { display: flex; align-items: center; gap: var(--cc-sp-2); }
.hk-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--cc-sp-1);
  min-width: 200px;
  min-height: var(--cc-h-sm);
  padding: 0 var(--cc-sp-3);
  border: 1px solid var(--cc-line-strong);
  border-radius: var(--cc-r-md);
  background: var(--cc-inset);
  color: var(--cc-fg);
  font-size: var(--cc-fs-sm);
  cursor: pointer;
  text-align: left;
}
.hk-btn:hover { border-color: var(--cc-acc-line); }
.hk-btn:focus-visible { outline: var(--cc-focus-w) solid var(--cc-focus); outline-offset: var(--cc-focus-off); }
.hk-btn.rec { border-color: var(--cc-acc); background: var(--cc-acc-soft); min-width: 280px; white-space: nowrap; }
.hk-btn.bad { border-color: var(--cc-err); }
.hk-btn kbd {
  font: inherit;
  padding: 0 var(--cc-sp-2);
  line-height: 20px;
  border: 1px solid var(--cc-line-strong);
  border-radius: var(--cc-r-sm);
  background: var(--cc-card);
}
.plus { color: var(--cc-faint); }
.ph { color: var(--cc-mute); }
.hint { color: var(--cc-acc-text); flex: 1; }
.esc { color: var(--cc-mute); font-size: var(--cc-fs-xs); }
.hk-msg { margin: 0; min-height: 0; font-size: var(--cc-fs-sm); color: var(--cc-mute); display: flex; gap: var(--cc-sp-1); align-items: center; }
.hk-msg.err { color: var(--cc-err-text); }
.hk-msg svg { flex: none; }
</style>
