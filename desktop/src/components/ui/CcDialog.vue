<script setup lang="ts">
// 对话框：包 el-dialog，统一页头（标题 + 右上关闭）、页脚（右对齐，次 → 主）和宽度档。
// 宽度：sm 400 确认 / 危险操作 · md 560 表单 · lg 720 列表 / 预览（正文内部滚动）· full 播放器 / 校对。窄屏一律 min(档宽, 100vw − 32px)。
import { computed, nextTick } from 'vue'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    title: string
    size?: 'sm' | 'md' | 'lg' | 'full'
    /** 危险确认：标题前显示警示图标。 */
    danger?: boolean
    /** 打开后把焦点放到这个选择器匹配的元素上（危险确认应指向“先不…”按钮）。 */
    initialFocus?: string
    closeOnClickModal?: boolean
  }>(),
  { size: 'md', closeOnClickModal: false },
)
const emit = defineEmits<{ 'update:modelValue': [v: boolean]; closed: [] }>()
const WIDTH = { sm: '400px', md: '560px', lg: '720px', full: 'calc(100% - 48px)' } as const
const width = computed(() => (props.size === 'full' ? WIDTH.full : `min(${WIDTH[props.size]}, calc(100vw - 32px))`))

function close() {
  emit('update:modelValue', false)
}
async function onOpened() {
  if (!props.initialFocus) return
  await nextTick()
  document.querySelector<HTMLElement>(`.cc-dialog ${props.initialFocus}`)?.focus()
}
</script>

<template>
  <el-dialog
    append-to-body
    align-center
    :model-value="modelValue"
    :width="width"
    :show-close="false"
    :close-on-click-modal="closeOnClickModal"
    :title="title"
    class="cc-dialog"
    :class="`sz-${size}`"
    modal-class="cc-dialog-mask"
    @update:model-value="(v: boolean) => emit('update:modelValue', v)"
    @opened="onOpened"
    @closed="emit('closed')"
  >
    <template #header>
      <div class="cc-dlg-h">
        <svg v-if="danger" class="warn" width="18" height="18" viewBox="0 0 18 18" aria-hidden="true"><path d="M9 2 1.8 15h14.4L9 2Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" /><path d="M9 7v3.6M9 12.8v.1" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" /></svg>
        <h4>{{ title }}</h4>
        <span class="sp" />
        <button class="cc-dlg-x" type="button" aria-label="关闭" @click="close">
          <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="m2 2 10 10m0-10L2 12" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" /></svg>
        </button>
      </div>
    </template>
    <div class="cc-dlg-bd"><slot /></div>
    <template v-if="$slots.footer" #footer><div class="cc-dlg-ft"><slot name="footer" /></div></template>
  </el-dialog>
</template>

<style>
.cc-dialog-mask { background: var(--cc-scrim) !important; z-index: var(--cc-z-dialog); }
.el-dialog.cc-dialog {
  --el-dialog-padding-primary: 0;
  padding: 0;
  background: var(--cc-card);
  border-radius: var(--cc-r-lg);
  box-shadow: var(--cc-sh-3);
  display: flex;
  flex-direction: column;
  max-height: 85vh;
  overflow: hidden;
}
.el-dialog.cc-dialog .el-dialog__header { padding: 0; margin: 0; }
.el-dialog.cc-dialog .el-dialog__body { padding: 0; flex: 1; min-height: 0; overflow: auto; color: var(--cc-fg2); font-size: var(--cc-fs-md); line-height: 1.55; }
.el-dialog.cc-dialog .el-dialog__footer { padding: 0; }
.cc-dlg-h { display: flex; align-items: center; gap: var(--cc-sp-2); padding: var(--cc-sp-4) var(--cc-sp-5) var(--cc-sp-3); }
.cc-dlg-h h4 { margin: 0; font-size: var(--cc-fs-lg); font-weight: 700; color: var(--cc-fg); }
.cc-dlg-h .warn { color: var(--cc-warn-text); flex: none; }
.cc-dlg-h .sp { flex: 1; }
.cc-dlg-x { width: var(--cc-h-sm); height: var(--cc-h-sm); display: grid; place-items: center; border: 0; border-radius: var(--cc-r-md); background: transparent; color: var(--cc-fg2); cursor: pointer; }
.cc-dlg-x:hover { background: var(--cc-hover); color: var(--cc-fg); }
.cc-dlg-bd { padding: 0 var(--cc-sp-5) var(--cc-sp-4); display: flex; flex-direction: column; gap: var(--cc-sp-3); }
.cc-dlg-ft { display: flex; justify-content: flex-end; gap: var(--cc-sp-2); padding: var(--cc-sp-3) var(--cc-sp-5) var(--cc-sp-4); border-top: 1px solid var(--cc-line); background: var(--cc-foot); }
@media (max-width: 480px) {
  .el-dialog.cc-dialog.sz-md .cc-dlg-ft { flex-direction: column-reverse; }
  .el-dialog.cc-dialog.sz-md .cc-dlg-ft .cc-btn { width: 100%; }
}
@media (max-width: 899px) {
  .el-dialog.cc-dialog.sz-full { width: 100vw !important; max-width: 100vw; height: 100vh; max-height: 100vh; margin: 0; border-radius: 0; }
}
</style>
