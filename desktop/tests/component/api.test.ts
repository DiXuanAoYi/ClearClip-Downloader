import { describe, it, expect, vi, beforeEach } from 'vitest'

const invoke = vi.fn()
const listen = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listen(...a) }))

import { api, events, errorKind, errorText } from '../../src/api'

beforeEach(() => {
  invoke.mockReset()
  listen.mockReset()
  invoke.mockResolvedValue(undefined)
})

describe('api 层：命令名与参数形状', () => {
  it('无参命令只传命令名', async () => {
    await api.listTasks()
    expect(invoke).toHaveBeenCalledWith('list_tasks')
    await api.appInfo()
    expect(invoke).toHaveBeenLastCalledWith('get_app_info')
  })

  it('带参命令传对象，缺省值被补全', async () => {
    await api.resolve('https://x.test/v/1')
    expect(invoke).toHaveBeenLastCalledWith('resolve_link', { text: 'https://x.test/v/1' })
    await api.listHistory()
    expect(invoke).toHaveBeenLastCalledWith('list_history', { query: '' })
    await api.getDiagnostics()
    expect(invoke).toHaveBeenLastCalledWith('get_diagnostics', { summary: false })
  })

  it('saveSettings：base 缺省时传 null（后端据此区分“没有基准”）', async () => {
    const s = { theme: 'dark' } as never
    await api.saveSettings(s)
    expect(invoke).toHaveBeenLastCalledWith('save_settings', { settings: s, base: null })
    const b = { theme: 'light' } as never
    await api.saveSettings(s, b)
    expect(invoke).toHaveBeenLastCalledWith('save_settings', { settings: s, base: b })
  })

  it('返回值原样透传，后端错误原样抛出', async () => {
    invoke.mockResolvedValueOnce([{ id: 1 }])
    await expect(api.listTasks()).resolves.toEqual([{ id: 1 }])
    const err = { kind: 'need_login', message: '需要登录' }
    invoke.mockRejectedValueOnce(err)
    await expect(api.listTasks()).rejects.toBe(err)
  })

  it('合并后的 api 对象没有因同名覆盖而丢命令：每个领域文件的导出都在', async () => {
    const mods = import.meta.glob('../../src/api/*.ts', { eager: true }) as Record<string, Record<string, Record<string, unknown>>>
    let total = 0
    for (const [path, m] of Object.entries(mods)) {
      if (path.endsWith('/index.ts')) continue
      for (const v of Object.values(m)) {
        if (v && typeof v === 'object') {
          for (const k of Object.keys(v)) {
            total++
            expect(api, `${path}#${k}`).toHaveProperty(k)
          }
        }
      }
    }
    expect(total).toBeGreaterThan(100)
    expect(Object.keys(api).length).toBe(total) // 没有两个领域文件导出同名方法（后者会悄悄覆盖前者）
  })
})

describe('错误辅助', () => {
  it('errorText 兼容 AppError、字符串、Error、其它', () => {
    expect(errorText({ kind: 'network', message: '断网' })).toBe('断网')
    expect(errorText('boom')).toBe('boom')
    expect(errorText(new Error('e'))).toBe('e')
    expect(errorText(42)).toBe('42')
    expect(errorText(null)).toBe('null')
  })
  it('errorKind 只认带 kind+message 的对象，否则 other', () => {
    expect(errorKind({ kind: 'need_login', message: 'x' })).toBe('need_login')
    expect(errorKind({ message: 'x' })).toBe('other')
    expect(errorKind('x')).toBe('other')
    expect(errorKind(null)).toBe('other')
  })
})

describe('events：事件名与载荷解包', () => {
  it('onProgress 监听 tasks://progress 并把 payload 交给回调', async () => {
    const unlisten = () => {}
    listen.mockResolvedValue(unlisten)
    const cb = vi.fn()
    await expect(events.onProgress(cb)).resolves.toBe(unlisten)
    expect(listen.mock.calls[0][0]).toBe('tasks://progress')
    listen.mock.calls[0][1]({ payload: { id: 7 } })
    expect(cb).toHaveBeenCalledWith({ id: 7 })
  })

  it('无载荷事件的回调不带参数', () => {
    listen.mockResolvedValue(() => {})
    const cb = vi.fn()
    void events.onLibraryChanged(cb)
    listen.mock.calls[0][1]({ payload: 'ignored' })
    expect(listen.mock.calls[0][0]).toBe('library://changed')
    expect(cb).toHaveBeenCalledWith()
  })

  it('onLock 只取 payload.locked', () => {
    listen.mockResolvedValue(() => {})
    const cb = vi.fn()
    void events.onLock(cb)
    listen.mock.calls[0][1]({ payload: { locked: true } })
    expect(cb).toHaveBeenCalledWith(true)
  })

  it('onToastError 兼容字符串与 {message}，忽略空消息', () => {
    listen.mockResolvedValue(() => {})
    const cb = vi.fn()
    void events.onToastError(cb)
    const h = listen.mock.calls[0][1]
    h({ payload: '文字' })
    h({ payload: { message: '对象' } })
    h({ payload: null })
    h({ payload: { message: '' } })
    expect(cb.mock.calls).toEqual([['文字'], ['对象']])
  })
})
