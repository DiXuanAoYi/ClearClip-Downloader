import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'

/** 生成诊断信息并复制到剪贴板（内容已在 Rust 端隐去 Cookie 和令牌）。 */
export async function copyDiagnostics() {
  try {
    const text = await api.getDiagnostics()
    await api.copyText(text)
    ElMessage.success('诊断信息已复制，可以粘贴到问题反馈里。其中的 Cookie 和令牌已隐去。')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
