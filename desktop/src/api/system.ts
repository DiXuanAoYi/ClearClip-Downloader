import { invoke } from '@tauri-apps/api/core'
import type {
  AppInfo,
  SaveSettingsResult,
  MiniState,
  Settings,
  UpdateInfo,
} from '../types'

export const systemApi = {
  appInfo: () => invoke<AppInfo>('get_app_info'),
  getSettings: () => invoke<Settings>('get_settings'),
  /** base：表单打开时的那份设置；带上它后端做三方合并，不会覆盖别处的改动 */
  saveSettings: (settings: Settings, base?: Settings | null) => invoke<SaveSettingsResult>('save_settings', { settings, base: base ?? null }),

  isPortable: () => invoke<boolean>('is_portable'),
  restartApp: () => invoke<void>('restart_app'),

  copyText: (text: string) => invoke<void>('copy_text', { text }),
  openFile: (path: string) => invoke<void>('open_file', { path }),
  revealFile: (path: string) => invoke<void>('reveal_file', { path }),
  openUrl: (url: string) => invoke<void>('open_url', { url }),
  showMain: () => invoke<void>('show_main'),
  hideMini: () => invoke<void>('hide_mini'),
  miniReady: () => invoke<MiniState>('mini_ready'),
  /** summary：日志只保留警告和错误 */
  getDiagnostics: (summary = false) => invoke<string>('get_diagnostics', { summary }),
  openLogDir: () => invoke<void>('open_log_dir'),
  openSamplesDir: () => invoke<void>('open_samples_dir'),
  checkUpdate: () => invoke<UpdateInfo>('check_update'),
  installUpdate: () => invoke<void>('install_update'),
}
