import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'

/** 生成诊断信息并复制到剪贴板（内容已在 Rust 端隐去 Cookie 和令牌）。 */
export async function copyDiagnostics() {
  await copy(false)
}

/** 只带警告和错误的诊断摘要：比完整的短得多，粘贴到聊天里更方便。 */
export async function copyDiagnosticsSummary() {
  await copy(true)
}

async function copy(summary: boolean) {
  try {
    const text = await api.getDiagnostics(summary)
    await api.copyText(text)
    ElMessage.success(`${summary ? '诊断摘要' : '诊断信息'}已复制，可以粘贴到问题反馈里。其中的 Cookie 和令牌已隐去。`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
