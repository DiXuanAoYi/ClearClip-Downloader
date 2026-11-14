/**
 * 前端兜底错误日志：记最近 50 条到内存，并按间隔提示用户。
 * 后端没有“写前端日志”的命令，这里只做 console.error + 内存环形缓冲；
 * 需要时可以在开发者工具里读 `window.__clearclipErrors`。纯逻辑，不依赖 Vue / Element Plus，便于单测。
 */

export interface ErrorEntry {
  at: number
  source: string
  message: string
  stack?: string
}

const MAX = 50
/** 同一条消息的提示间隔，避免循环报错时刷屏 */
const TOAST_GAP_MS = 5000

export function describeError(e: unknown): { message: string; stack?: string } {
  if (e instanceof Error) return { message: e.message || e.name, stack: e.stack }
  if (typeof e === 'string') return { message: e }
  if (typeof e === 'object' && e !== null && 'message' in e) return { message: String((e as { message: unknown }).message) }
  try {
    return { message: JSON.stringify(e) ?? String(e) }
  } catch {
    return { message: String(e) }
  }
}

/** 浏览器自己会报、对用户没意义的噪声 */
export function isNoise(message: string): boolean {
  return /ResizeObserver loop/i.test(message)
}

/** 后端在应用锁定时拒绝命令的错误（“应用已锁定，请先解锁。”）：辅助窗口会被收起，这类错误不该弹提示 */
export function isLockedMessage(message: string): boolean {
  return message.includes('应用已锁定')
}

export function createErrorLog(notify: (message: string) => void, now: () => number = Date.now) {
  const entries: ErrorEntry[] = []
  const lastToast = new Map<string, number>()

  function record(source: string, e: unknown, opts: { toast?: boolean } = {}): ErrorEntry | null {
    const { message, stack } = describeError(e)
    if (isNoise(message)) return null
    const entry: ErrorEntry = { at: now(), source, message, stack }
    entries.push(entry)
    if (entries.length > MAX) entries.splice(0, entries.length - MAX)
    console.error(`[${source}]`, e)
    if (opts.toast !== false && !isLockedMessage(message)) {
      const last = lastToast.get(message)
      if (last === undefined || entry.at - last >= TOAST_GAP_MS) {
        lastToast.set(message, entry.at)
        if (lastToast.size > 100) lastToast.clear()
        try {
          notify(message)
        } catch {
          // 提示本身失败（比如 UI 还没挂载）不能再抛错
        }
      }
    }
    return entry
  }

  return { record, recent: () => entries.slice() }
}
