import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  WritePersona,
  WriteJob,
  WritePending,
  WritingSettings,
} from '../types'

export const writeApi = {
  writeTake: () => invoke<WritePending | null>('write_take'),
  writeSettings: () => invoke<WritingSettings>('write_settings'),
  writeRun: (req: Record<string, unknown>) => invoke<number>('write_run', { req }),
  writeDictate: (audio: number[], ext: string) => invoke<number>('write_dictate', { audio, ext }),
  writeDictateStart: (engine: string, lang?: string) => invoke<{ session: number; engine: string; local: boolean }>('write_dictate_start', { engine, lang }),
  writeDictatePush: (session: number, pcm: string) => invoke<boolean>('write_dictate_push', { session, pcm }),
  writeDictateStop: (session: number) => invoke<void>('write_dictate_stop', { session }),
  onWriteDictate: (cb: (p: { session: number; kind: 'interim' | 'final' | 'error' | 'done'; text: string }) => void): Promise<UnlistenFn> => listen<{ session: number; kind: 'interim' | 'final' | 'error' | 'done'; text: string }>('write://dictate', (e) => cb(e.payload)),
  writeJob: (id: number) => invoke<WriteJob | null>('write_job', { id }),
  writeCancel: (id: number) => invoke<boolean>('write_cancel', { id }),
  writeApply: (text: string, replace: boolean, sourceApp?: string) => invoke<string>('write_apply', { text, replace, sourceApp }),
  writePersonas: () => invoke<WritePersona[]>('write_personas'),
  writeMemorySet: (on: boolean, notes: string) => invoke<void>('write_memory_set', { on, notes }),
  writeMemoryClear: () => invoke<void>('write_memory_clear'),
  writeMemoryForget: (index: number) => invoke<void>('write_memory_forget', { index }),
  writeUndo: () => invoke<boolean>('write_undo'),
  writePin: (on: boolean) => invoke<void>('write_pin', { on }),
  writePrefs: (lastAction: string, extras: string[], autoClose: boolean) => invoke<void>('write_prefs', { lastAction, extras, autoClose }),
  /** 帮我回：记住这个程序 / 这个人是谁（relation 为空时忘掉） */
  writeRelation: (key: string, relation: string) => invoke<void>('write_relation', { key, relation }),
  writeOpenSettings: () => invoke<void>('write_open_settings'),
  writeBadgeClick: () => invoke<void>('write_badge_click'),
  writeClose: () => invoke<void>('write_close'),
  writePause: (minutes: number) => invoke<void>('write_pause', { minutes }),
}
