// Toast 的停留时长与图标语义：纯函数，单元测试直接引用。
export type ToastKind = 'success' | 'info' | 'warning' | 'error'

/** 毫秒；0 = 不自动消失（错误要等用户看过再关）。 */
export function toastDuration(kind: ToastKind, hasAction = false): number {
  if (kind === 'error') return 0
  if (kind === 'warning') return 6000
  return hasAction ? 6000 : 3000
}

/** 屏幕阅读器播报等级：错误用 assertive，其余 polite。 */
export function toastRole(kind: ToastKind): 'alert' | 'status' {
  return kind === 'error' ? 'alert' : 'status'
}
