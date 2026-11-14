// 提示：在现有的 ElMessage 上弹出 CcToast（成功 / 信息 / 警告 / 错误，错误可带“重试 / 查看详情”）。
// 老的 ElMessage 调用不用改；新代码和逐页改造时改用 notify。
import { h } from 'vue'
import { ElMessage } from 'element-plus'
import CcToast from '../components/ui/CcToast.vue'
import { toastDuration, type ToastKind } from '../components/ui/toast-policy'

export interface NotifyOptions {
  kind?: ToastKind
  title: string
  desc?: string
  detail?: string
  /** 给了就显示“重试”，点击后先关掉这条再调用。 */
  onRetry?: () => void
  /** 覆盖停留时长（毫秒，0 = 不自动消失）。 */
  duration?: number
}

export function notify(o: NotifyOptions) {
  const kind = o.kind ?? 'info'
  let inst: { close: () => void } | null = null
  const close = () => inst?.close()
  inst = ElMessage({
    customClass: 'cc-toast-host',
    duration: o.duration ?? toastDuration(kind, !!o.onRetry),
    showClose: false,
    grouping: false,
    message: h(CcToast, {
      kind,
      title: o.title,
      desc: o.desc,
      detail: o.detail,
      retryable: !!o.onRetry,
      closable: true,
      onClose: close,
      onRetry: () => {
        close()
        o.onRetry?.()
      },
    }) as never,
  })
  return inst
}

export const notifyError = (title: string, extra: Omit<NotifyOptions, 'title' | 'kind'> = {}) => notify({ ...extra, title, kind: 'error' })
