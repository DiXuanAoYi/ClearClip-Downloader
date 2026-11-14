<script setup lang="ts">
// 链接：有 href 用 <a>（外链交给应用打开），没有 href 是行内的文字按钮。
import { api } from '../../api'

const props = defineProps<{
  href?: string
  /** 外链：用系统浏览器打开，末尾显示外链图标。 */
  external?: boolean
  disabled?: boolean
}>()
const emit = defineEmits<{ click: [e: MouseEvent] }>()

function onClick(e: MouseEvent) {
  if (props.disabled) {
    e.preventDefault()
    return
  }
  if (props.external && props.href) {
    e.preventDefault()
    void api.openUrl(props.href)
  }
  emit('click', e)
}
</script>

<template>
  <a v-if="href && !disabled" class="cc-link" :href="href" :target="external ? '_blank' : undefined" :rel="external ? 'noopener noreferrer' : undefined" @click="onClick">
    <slot />
    <svg v-if="external" class="ext" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M4 2h6v6M10 2 3 9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    <span v-if="external" class="cc-sr">（在浏览器中打开）</span>
  </a>
  <button v-else class="cc-link" type="button" :disabled="disabled" @click="onClick"><slot /></button>
</template>

<style>
.cc-link {
  display: inline;
  color: var(--cc-acc-text);
  text-decoration: underline;
  text-decoration-color: var(--cc-acc-line);
  text-underline-offset: 3px;
  cursor: pointer;
  background: none;
  border: 0;
  padding: 0;
  font: inherit;
  border-radius: var(--cc-r-sm);
  transition: text-decoration-color var(--cc-dur-fast) var(--cc-ease-out);
}
.cc-link:hover { text-decoration-color: currentColor; }
.cc-link:visited { color: var(--cc-fg2); }
.cc-link:disabled { color: var(--cc-faint); text-decoration: none; cursor: not-allowed; }
.cc-link .ext { margin-left: var(--cc-sp-1); vertical-align: -1px; }
</style>
