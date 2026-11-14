<script setup lang="ts">
// 按钮：主 / 次 / 文字 / 危险 / 图标，三种尺寸。原生 <button>，loading 时保持宽度、忽略点击、仍可聚焦。
import { computed, useSlots, watchEffect } from 'vue'

const props = withDefaults(
  defineProps<{
    kind?: 'primary' | 'secondary' | 'text' | 'danger'
    size?: 's' | 'm' | 'l'
    /** 只有图标的按钮（方形）。必须同时给 ariaLabel。 */
    iconOnly?: boolean
    /** 图标按钮的无障碍名称；也作为悬停提示。 */
    ariaLabel?: string
    loading?: boolean
    disabled?: boolean
    /** 禁用时说明原因（悬停提示，并供读屏读出）。 */
    disabledReason?: string
    type?: 'button' | 'submit' | 'reset'
  }>(),
  { kind: 'secondary', size: 'm', iconOnly: false, loading: false, disabled: false, type: 'button' },
)
const emit = defineEmits<{ click: [e: MouseEvent] }>()
const slots = useSlots()
const hasIcon = computed(() => !!slots.icon)
const tip = computed(() => (props.disabled && props.disabledReason ? props.disabledReason : props.ariaLabel))
watchEffect(() => {
  if (import.meta.env?.DEV && props.iconOnly && !props.ariaLabel) console.warn('[CcButton] 图标按钮需要 ariaLabel')
})
function onClick(e: MouseEvent) {
  if (props.loading || props.disabled) {
    e.preventDefault()
    e.stopPropagation()
    return
  }
  emit('click', e)
}
</script>

<template>
  <button
    class="cc-btn"
    :class="[`k-${kind}`, `s-${size}`, { icon: iconOnly, loading, 'has-icon': hasIcon }]"
    :type="type"
    :disabled="disabled"
    :aria-label="ariaLabel"
    :aria-busy="loading || undefined"
    :aria-disabled="loading || undefined"
    :title="tip"
    @click="onClick"
  >
    <span v-if="loading" class="cc-spin" aria-hidden="true" />
    <span v-if="hasIcon" class="ic" :class="{ off: loading }" aria-hidden="true"><slot name="icon" /></span>
    <span v-if="!iconOnly" class="tx" :class="{ ghost: loading && !hasIcon }"><slot /></span>
    <span v-if="disabled && disabledReason" class="cc-sr">（{{ disabledReason }}）</span>
  </button>
</template>

<style>
.cc-btn {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--cc-sp-2);
  height: var(--cc-h-md);
  padding: 0 var(--cc-sp-4);
  border-radius: var(--cc-r-md);
  border: 1px solid transparent;
  font: inherit;
  font-size: var(--cc-fs-md);
  font-weight: 500;
  line-height: 1;
  cursor: pointer;
  white-space: nowrap;
  user-select: none;
  transition:
    background var(--cc-dur-fast) var(--cc-ease-out),
    border-color var(--cc-dur-fast) var(--cc-ease-out),
    color var(--cc-dur-fast) var(--cc-ease-out),
    transform var(--cc-dur-fast) var(--cc-ease-out);
}
.cc-btn.s-s { height: var(--cc-h-sm); padding: 0 var(--cc-sp-3); font-size: var(--cc-fs-sm); gap: var(--cc-sp-1); }
.cc-btn.s-l { height: var(--cc-h-lg); padding: 0 var(--cc-sp-6); font-size: var(--cc-fs-lg); }
.cc-btn .ic { display: inline-flex; align-items: center; }
.cc-btn .ic.off { visibility: hidden; position: absolute; }
.cc-btn .tx.ghost { visibility: hidden; }
.cc-btn.loading .cc-spin { position: static; }
.cc-btn.loading:not(.has-icon) .cc-spin { position: absolute; }
.cc-spin {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 2px solid currentColor;
  border-right-color: transparent;
  animation: cc-spin 0.8s linear infinite;
  flex: none;
}
@keyframes cc-spin { to { transform: rotate(360deg); } }
.cc-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }

.cc-btn.k-primary { background: var(--cc-acc-strong); color: var(--cc-on-acc); }
.cc-btn.k-primary:hover:not(:disabled) { background: var(--cc-acc-strong-hover); }
.cc-btn.k-secondary { background: transparent; color: var(--cc-fg); border-color: var(--cc-line-strong); }
.cc-btn.k-secondary:hover:not(:disabled) { background: var(--cc-hover); border-color: var(--cc-mute); }
.cc-btn.k-text { background: transparent; color: var(--cc-fg2); padding: 0 var(--cc-sp-2); }
.cc-btn.k-text:hover:not(:disabled) { background: var(--cc-hover); color: var(--cc-fg); }
.cc-btn.k-danger { background: var(--cc-err-soft); color: var(--cc-err-text); border-color: var(--cc-err-line); }
.cc-btn.k-danger:hover:not(:disabled) { background: var(--cc-err); color: var(--cc-on-err); border-color: var(--cc-err); }
.cc-btn.icon { width: var(--cc-h-md); padding: 0; }
.cc-btn.icon.s-s { width: var(--cc-h-sm); }
.cc-btn.icon.s-l { width: var(--cc-h-lg); }
.cc-btn.icon.k-secondary { border-color: var(--cc-line-strong); }
.cc-btn:active:not(:disabled) { transform: translateY(1px); }
.cc-btn.loading { cursor: progress; }
.cc-btn:disabled {
  cursor: not-allowed;
  background: var(--cc-fill);
  color: var(--cc-faint);
  border-color: var(--cc-line);
}
.cc-btn.k-text:disabled, .cc-btn.icon.k-text:disabled { background: transparent; border-color: transparent; }
</style>
