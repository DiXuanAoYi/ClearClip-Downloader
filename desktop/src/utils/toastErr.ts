import { ElMessage } from 'element-plus'
import { errorText } from '../api'

/**
 * 用户点了按钮触发的操作失败时：给一条提示并写进控制台，而不是悄悄吞掉。
 * 用法：`api.xxx().catch(toastErr('清空历史'))`
 */
export const toastErr = (what: string) => (e: unknown) => {
  console.error(`[${what}失败]`, e)
  ElMessage.error(`${what}失败：${errorText(e)}`)
}
