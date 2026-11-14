import { invoke } from '@tauri-apps/api/core'
import type {
  LockStatus,
  SafeEntry,
  SafeStatus,
  WipeOptions,
  WipeReport,
} from '../types'

export const securityApi = {
  lockStatus: () => invoke<LockStatus>('lock_status'),
  lockSetPassword: (newPassword: string, current?: string) => invoke<void>('lock_set_password', { newPassword, current }),
  lockRemovePassword: (current?: string) => invoke<void>('lock_remove_password', { current }),
  lockNow: () => invoke<boolean>('lock_now'),
  lockUnlock: (password: string) => invoke<void>('lock_unlock', { password }),
  wipeTraces: (options: WipeOptions) => invoke<WipeReport>('wipe_traces', { options }),
  safeboxStatus: () => invoke<SafeStatus>('safebox_status'),
  safeboxCreate: (password: string) => invoke<void>('safebox_create', { password }),
  safeboxUnlock: (password: string) => invoke<void>('safebox_unlock', { password }),
  safeboxLock: () => invoke<void>('safebox_lock'),
  safeboxChangePassword: (current: string, newPassword: string) => invoke<void>('safebox_change_password', { current, newPassword }),
  safeboxList: () => invoke<SafeEntry[]>('safebox_list'),
  safeboxAddFiles: (paths: string[], deleteOriginal: boolean) => invoke<number[]>('safebox_add_files', { paths, deleteOriginal }),
  safeboxAddLibrary: (ids: number[]) => invoke<number[]>('safebox_add_library', { ids }),
  safeboxOpen: (id: string) => invoke<number>('safebox_open', { id }),
  safeboxExport: (id: string, dir: string) => invoke<number>('safebox_export', { id, dir }),
  safeboxRemove: (ids: string[]) => invoke<number>('safebox_remove', { ids }),
}
