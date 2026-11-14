import { getCurrentScope, onScopeDispose } from 'vue'

export type Unlisten = () => void

/**
 * 在组件（或任何 effect 作用域）里注册 Tauri 事件监听，卸载时自动释放。
 *
 * 必须在 setup 同步阶段调用 `useTauriEvents()`；之后可以在 onMounted、await 之后随意调用 `on`。
 * 处理了“await 注册期间组件已卸载”的竞态：注册完成时作用域已销毁，会立刻取消监听，不会泄漏。
 * `on` 返回的取消函数是幂等的，手写的 `un?.()` 与自动释放同时发生也不会重复取消。
 */
export function useTauriEvents() {
  let disposed = false
  const offs = new Set<Unlisten>()

  const stop = () => {
    disposed = true
    const all = [...offs]
    offs.clear()
    for (const f of all) f()
  }
  if (getCurrentScope()) onScopeDispose(stop)

  /** 传入 `events.onX(cb)` / `listen(...)` 返回的 Promise，返回幂等的取消函数。 */
  async function on(reg: Promise<Unlisten> | Unlisten): Promise<Unlisten> {
    const raw = await reg
    let done = false
    const off: Unlisten = () => {
      if (done) return
      done = true
      offs.delete(off)
      try {
        raw()
      } catch {
        // 已被 Tauri 侧释放（窗口关闭等），忽略
      }
    }
    if (disposed) {
      off()
      return off
    }
    offs.add(off)
    return off
  }

  return { on, stop }
}

/** 单个事件的便捷写法：`useTauriEvent(() => events.onFoo(cb))`，在 setup 里同步调用。 */
export function useTauriEvent(register: () => Promise<Unlisten>) {
  const ev = useTauriEvents()
  void ev.on(register()).catch((e) => console.error('[事件监听注册失败]', e))
  return ev
}
