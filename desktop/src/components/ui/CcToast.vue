<script setup lang="ts">
// Toast 的内容卡片：标题 + 说明 + 可选“重试 / 查看详情”。通常经 utils/notify.ts 的 notify() 弹出（挂在现有的 ElMessage 上），也可以直接内嵌显示。
import { ref } from 'vue'
import { toastRole, type ToastKind } from './toast-policy'

const props = defineProps<{
  kind: ToastKind
  title: string
  desc?: string
  /** 原始错误文本；有值时出现“查看详情”，展开后可复制。 */
  detail?: string
  /** 显示“重试”按钮（点击触发 retry 事件）。 */
  retryable?: boolean
  closable?: boolean
}>()
const emit = defineEmits<{ retry: []; close: []; copy: [text: string] }>()
const open = ref(false)
const copied = ref(false)
async function copy() {
  if (!props.detail) return
  try {
    await navigator.clipboard.writeText(props.detail)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    /* 剪贴板不可用：详情文字本身可选中 */
  }
  emit('copy', props.detail)
}
</script>

<template>
  <div class="cc-toast" :class="`k-${kind}`" :role="toastRole(kind)">
    <svg class="ic" width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <circle cx="9" cy="9" r="8" fill="none" stroke="currentColor" stroke-width="1.6" />
      <path v-if="kind === 'success'" d="m5.5 9.2 2.4 2.4 4.6-5" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      <path v-else-if="kind === 'info'" d="M9 8v4.4M9 5.6v.1" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
      <path v-else-if="kind === 'warning'" d="M9 5v4.6M9 12.2v.1" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
      <path v-else d="m6.2 6.2 5.6 5.6m0-5.6-5.6 5.6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
    </svg>
    <div class="t">{{ title }}</div>
    <button v-if="closable" class="x" type="button" aria-label="关闭提示" @click="emit('close')">
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="m2 2 8 8m0-8-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
    </button>
    <div v-if="desc" class="d">{{ desc }}</div>
    <div v-if="retryable || detail || $slots.default" class="a">
      <button v-if="retryable" class="cc-link" type="button" @click="emit('retry')">重试</button>
      <button v-if="detail" class="cc-link" type="button" :aria-expanded="open" @click="open = !open">{{ open ? '收起详情' : '查看详情' }}</button>
      <slot />
    </div>
    <div v-if="detail && open" class="det">
      <pre class="selectable">{{ detail }}</pre>
      <button class="cc-link" type="button" @click="copy">{{ copied ? '已复制' : '复制详情' }}</button>
    </div>
  </div>
</template>

<style>
.cc-toast {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) auto;
  gap: var(--cc-sp-1) var(--cc-sp-3);
  align-items: start;
  padding: var(--cc-sp-3) var(--cc-sp-4);
  border-radius: var(--cc-r-md);
  background: var(--cc-card);
  color: var(--cc-fg);
  box-shadow: var(--cc-sh-2);
  border: 1px solid var(--cc-line);
  width: 360px;
  max-width: 100%;
  box-sizing: border-box;
}
.cc-toast .ic { margin-top: 1px; }
.cc-toast .t { font-size: var(--cc-fs-md); font-weight: 500; line-height: 1.4; }
.cc-toast .d { grid-column: 2; font-size: var(--cc-fs-sm); color: var(--cc-mute); line-height: var(--cc-lh-base); word-break: break-word; }
.cc-toast .a { grid-column: 2 / -1; display: flex; gap: var(--cc-sp-3); margin-top: var(--cc-sp-1); font-size: var(--cc-fs-sm); }
.cc-toast .det { grid-column: 2 / -1; display: flex; flex-direction: column; gap: var(--cc-sp-2); align-items: flex-start; font-size: var(--cc-fs-sm); }
.cc-toast .det pre { margin: 0; max-height: 120px; overflow: auto; width: 100%; padding: var(--cc-sp-2); border-radius: var(--cc-r-sm); background: var(--cc-fill); color: var(--cc-fg2); font-family: var(--cc-mono); font-size: var(--cc-fs-xs); white-space: pre-wrap; word-break: break-all; box-sizing: border-box; }
.cc-toast .x { grid-column: 3; grid-row: 1; width: 20px; height: 20px; display: grid; place-items: center; border: 0; border-radius: var(--cc-r-sm); background: transparent; color: var(--cc-mute); cursor: pointer; }
.cc-toast .x:hover { background: var(--cc-hover); color: var(--cc-fg); }
.cc-toast.k-success .ic { color: var(--cc-ok-text); }
.cc-toast.k-info .ic { color: var(--cc-info-text); }
.cc-toast.k-warning { border-color: var(--cc-warn-line); }
.cc-toast.k-warning .ic { color: var(--cc-warn-text); }
.cc-toast.k-error { border-color: var(--cc-err-line); }
.cc-toast.k-error .ic { color: var(--cc-err-text); }
/* 挂在 ElMessage 里时，去掉它自带的底、边、阴影，由 .cc-toast 自己负责 */
.el-message.cc-toast-host { padding: 0; background: transparent; border: 0; box-shadow: none; z-index: var(--cc-z-toast); }
.el-message.cc-toast-host .el-message__content { padding: 0; }
</style>
