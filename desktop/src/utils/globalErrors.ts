import type { App } from 'vue'
import { ElMessage } from 'element-plus'
import { createErrorLog } from './errorLog'

/** 透明浮层、拖拽条这类小窗口里弹提示没有意义（也看不见），只记录 */
const QUIET = new Set(['#float', '#wbadge', '#shotpin', '#liveov', '#livesub', '#livepick'])

const log = createErrorLog((message) => {
  ElMessage({ type: 'error', message: `出了点问题：${message.slice(0, 120)}`, duration: 5000, grouping: true })
})

declare global {
  interface Window {
    __clearclipErrors?: () => ReturnType<typeof log.recent>
  }
}

/** 所有窗口入口调用一次：组件里没捕获的错误、没处理的 Promise 拒绝都记下来，并给一条不打扰的提示，避免静默白屏。 */
export function installGlobalErrorHandlers(app: App) {
  const toast = !QUIET.has(location.hash)
  app.config.errorHandler = (err, _inst, info) => {
    log.record(`vue:${info}`, err, { toast })
  }
  window.addEventListener('unhandledrejection', (ev) => {
    log.record('unhandledrejection', ev.reason, { toast })
    ev.preventDefault()
  })
  window.addEventListener('error', (ev) => {
    // 资源加载失败的 error 事件没有 message，交给浏览器自己处理
    if (ev.message) log.record('window.error', ev.error ?? ev.message, { toast })
  })
  window.__clearclipErrors = log.recent
}
