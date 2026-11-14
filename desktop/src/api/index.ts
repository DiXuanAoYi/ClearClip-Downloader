import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ModelProgress,
  AppErrorPayload,
  ErrorKind,
  AutoResult,
  BatchProgress,
  PreviewItem,
  JobSnap,
  PairRequest,
  ChatMsg,
  TransferProgress,
  TransferReceived,
  ToolProgress,
  ClipboardLink,
  InboxCounts,
  Settings,
  TaskSnapshot,
} from '../types'

import { accountApi } from './account'
import { aiApi } from './ai'
import { downloadApi } from './download'
import { libraryApi } from './library'
import { mediaApi } from './media'
import { networkApi } from './network'
import { screenApi } from './screen'
import { securityApi } from './security'
import { systemApi } from './system'
import { writeApi } from './write'

/** 按领域拆在同目录的各文件里（download / library / ai / media / screen / write / network / account / security / system），这里合成一个对象，调用方式不变。 */
export const api = {
  ...accountApi,
  ...aiApi,
  ...downloadApi,
  ...libraryApi,
  ...mediaApi,
  ...networkApi,
  ...screenApi,
  ...securityApi,
  ...systemApi,
  ...writeApi,
}

export const events = {
  /** 摘录变了（浏览器扩展存进来一条） */
  onExcerptsChanged: (cb: () => void): Promise<UnlistenFn> => listen('excerpts://changed', () => cb()),
  onModelProgress: (cb: (p: ModelProgress) => void): Promise<UnlistenFn> => listen<ModelProgress>('ml://download', (e) => cb(e.payload)),
  onTasks: (cb: (tasks: TaskSnapshot[]) => void): Promise<UnlistenFn> => listen<TaskSnapshot[]>('tasks://updated', (e) => cb(e.payload)),
  onProgress: (cb: (task: TaskSnapshot) => void): Promise<UnlistenFn> => listen<TaskSnapshot>('tasks://progress', (e) => cb(e.payload)),
  onClipboardLink: (cb: (p: ClipboardLink) => void): Promise<UnlistenFn> => listen<ClipboardLink>('clipboard://link', (e) => cb(e.payload)),
  onAutoResult: (cb: (p: AutoResult) => void): Promise<UnlistenFn> => listen<AutoResult>('clipboard://auto-result', (e) => cb(e.payload)),
  onBatchPreview: (cb: (p: PreviewItem | { key: string; finished: true }) => void): Promise<UnlistenFn> => listen<PreviewItem | { key: string; finished: true }>('batch://preview', (e) => cb(e.payload)),
  onBatchOpen: (cb: (p: { urls: string[]; source: string; truncated: boolean }) => void): Promise<UnlistenFn> => listen<{ urls: string[]; source: string; truncated: boolean }>('batch://open', (e) => cb(e.payload)),
  onBatchChanged: (cb: () => void): Promise<UnlistenFn> => listen('batch://changed', () => cb()),
  onToolProgress: (cb: (p: ToolProgress) => void): Promise<UnlistenFn> => listen<ToolProgress>('tools://progress', (e) => cb(e.payload)),
  onBatch: (cb: (p: BatchProgress) => void): Promise<UnlistenFn> => listen<BatchProgress>('batch://progress', (e) => cb(e.payload)),
  /** 后台功能（取景窗口、截图翻译等）失败时发来的提示：可能是文字，也可能是带 message 的错误 */
  onToastError: (cb: (msg: string) => void): Promise<UnlistenFn> =>
    listen<unknown>('toast://error', (e) => {
      const p = e.payload as { message?: string } | string | null
      const msg = typeof p === 'string' ? p : (p?.message ?? '')
      if (msg) cb(msg)
    }),
  /** 窗口组件（WebView2）断开：新窗口打不开，需要重启清影 */
  onWebviewBroken: (cb: (msg: string) => void): Promise<UnlistenFn> => listen<string>('webview://broken', (e) => cb(e.payload)),
  onPairRequest: (cb: (p: PairRequest) => void): Promise<UnlistenFn> => listen<PairRequest>('phone://pair-request', (e) => cb(e.payload)),
  onChat: (cb: (m: ChatMsg & { cleared?: boolean }) => void): Promise<UnlistenFn> => listen<ChatMsg & { cleared?: boolean }>('phone://chat', (e) => cb(e.payload)),
  onTransferChanged: (cb: () => void): Promise<UnlistenFn> => listen('transfer://changed', () => cb()),
  onTransferProgress: (cb: (p: TransferProgress) => void): Promise<UnlistenFn> => listen<TransferProgress>('transfer://progress', (e) => cb(e.payload)),
  onTransferReceived: (cb: (p: TransferReceived) => void): Promise<UnlistenFn> => listen<TransferReceived>('transfer://received', (e) => cb(e.payload)),
  onPhoneReceived: (cb: (text: string) => void): Promise<UnlistenFn> => listen<string>('phone://received', (e) => cb(e.payload)),
  onPowerCountdown: (cb: (p: { action: string; seconds: number }) => void): Promise<UnlistenFn> =>
    listen<{ action: string; seconds: number }>('app://power-countdown', (e) => cb(e.payload)),
  onLive: (cb: () => void): Promise<UnlistenFn> => listen('live://updated', () => cb()),
  onSubs: (cb: () => void): Promise<UnlistenFn> => listen('subs://updated', () => cb()),
  onInbox: (cb: (c: InboxCounts) => void): Promise<UnlistenFn> => listen<InboxCounts>('inbox://changed', (e) => cb(e.payload)),
  onLibraryProgress: (cb: (p: { task: string; done: number; total: number }) => void): Promise<UnlistenFn> =>
    listen<{ task: string; done: number; total: number }>('library://progress', (e) => cb(e.payload)),
  onTrackProgress: (cb: (p: { id: number; percent: number }) => void): Promise<UnlistenFn> => listen<{ id: number; percent: number }>('edit://track', (e) => cb(e.payload)),
  /** 设置在别处（写作窗、屏幕翻译、托盘、另一个窗口）被改了 */
  onSettingsChanged: (cb: (s: Settings) => void): Promise<UnlistenFn> => listen<Settings>('settings-changed', (e) => cb(e.payload)),
  onMediaJobs: (cb: (jobs: JobSnap[]) => void): Promise<UnlistenFn> => listen<JobSnap[]>('media://jobs', (e) => cb(e.payload)),
  onLibraryChanged: (cb: () => void): Promise<UnlistenFn> => listen('library://changed', () => cb()),
  onLock: (cb: (locked: boolean) => void): Promise<UnlistenFn> => listen<{ locked: boolean }>('security://lock', (e) => cb(e.payload.locked)),
  onSafebox: (cb: () => void): Promise<UnlistenFn> => listen('security://safebox', () => cb()),
  onAccounts: (cb: () => void): Promise<UnlistenFn> => listen('accounts://updated', () => cb()),
  onParseRequest: (cb: (text: string) => void): Promise<UnlistenFn> => listen<string>('app://parse-request', (e) => cb(e.payload)),
}

function isAppError(e: unknown): e is AppErrorPayload {
  return typeof e === 'object' && e !== null && 'message' in e && 'kind' in e
}

/** Rust 端的错误序列化为 `{ kind, message }`。 */
export function errorText(e: unknown): string {
  if (isAppError(e)) return e.message
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return String(e)
}

export function errorKind(e: unknown): ErrorKind {
  return isAppError(e) ? e.kind : 'other'
}
