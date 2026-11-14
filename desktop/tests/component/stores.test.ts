import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invoke = vi.fn()
const handlers = new Map<string, (e: { payload: unknown }) => void>()
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    handlers.set(name, cb)
    return Promise.resolve(() => handlers.delete(name))
  },
}))

import { useAppStore, useQueueStore, useParseStore } from '../../src/stores/app'

const emit = (name: string, payload?: unknown) => handlers.get(name)!({ payload })
const task = (id: number, over: Record<string, unknown> = {}) => ({ id, status: 'queued', speed: 0, received: 0, ...over })

beforeEach(() => {
  invoke.mockReset()
  handlers.clear()
  document.documentElement.classList.remove('dark')
  setActivePinia(createPinia())
})

describe('useAppStore', () => {
  const settings = (theme: string) => ({ theme, downloadDir: '/d' })
  function backend(s: unknown, shortcutError: string | null = null) {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_settings') return Promise.resolve(s)
      if (cmd === 'get_app_info') return Promise.resolve({ version: '1.0.0', shortcutError })
      if (cmd === 'save_settings') return Promise.resolve({ settings: s, shortcutError: null })
      return Promise.resolve(undefined)
    })
  }

  it('load：并行取设置与应用信息，应用主题', async () => {
    backend(settings('dark'), '快捷键冲突')
    const app = useAppStore()
    await app.load()
    expect(app.settings).toEqual(settings('dark'))
    expect(app.info?.version).toBe('1.0.0')
    expect(app.shortcutError).toBe('快捷键冲突')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('load 只注册一次设置变更监听；别处改了设置后同步并 settingsRev 加一', async () => {
    backend(settings('light'))
    const app = useAppStore()
    await app.load()
    await app.load()
    expect(handlers.size).toBe(1)
    emit('settings-changed', settings('dark'))
    expect(app.settings?.theme).toBe('dark')
    expect(app.settingsRev).toBe(1)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('save：把最近同步的设置作为基准发给后端，并采用返回的设置', async () => {
    backend(settings('light'))
    const app = useAppStore()
    await app.load()
    const next = { ...settings('light'), downloadDir: '/new' } as never
    invoke.mockClear()
    backend({ ...settings('light'), downloadDir: '/new' })
    await app.save(next)
    const call = invoke.mock.calls.find((c) => c[0] === 'save_settings')!
    expect(call[1].settings).toBe(next)
    expect(call[1].base).toEqual(settings('light'))
    expect((app.settings as unknown as { downloadDir: string }).downloadDir).toBe('/new')
  })

  it('patch：未加载设置时什么也不做；加载后合并字段再保存', async () => {
    const app = useAppStore()
    await app.patch({ theme: 'dark' } as never)
    expect(invoke).not.toHaveBeenCalled()
    backend(settings('light'))
    await app.load()
    invoke.mockClear()
    backend(settings('dark'))
    await app.patch({ theme: 'dark' } as never)
    expect(invoke.mock.calls.find((c) => c[0] === 'save_settings')![1].settings).toEqual(settings('dark'))
  })

  it('refreshLock：后端失败时保持原状态', async () => {
    const app = useAppStore()
    invoke.mockResolvedValueOnce({ enabled: true, locked: true, lockedOutSecs: 3 })
    await app.refreshLock()
    expect(app.lock.locked).toBe(true)
    invoke.mockRejectedValueOnce('x')
    await app.refreshLock()
    expect(app.lock.locked).toBe(true)
  })

  it('导航：goSettings 切页并选标签', () => {
    const app = useAppStore()
    app.goSettings('network')
    expect(app.view).toBe('settings')
    expect(app.settingsTab).toBe('network')
  })
})

describe('useQueueStore', () => {
  it('start 取初始列表并只启动一次；counts / active / totalSpeed 计算正确', async () => {
    invoke.mockResolvedValue([task(1, { status: 'running', speed: 100 }), task(2, { status: 'running', speed: 50 }), task(3), task(4, { status: 'done', speed: 999 }), task(5, { status: 'failed' }), task(6, { status: 'paused' })])
    const q = useQueueStore()
    await q.start()
    await q.start()
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(q.counts).toEqual({ running: 2, queued: 1, done: 1, failed: 1, paused: 1 })
    expect(q.active).toBe(3)
    expect(q.totalSpeed).toBe(150)
  })

  it('进度事件按帧合并：同帧多条只写最后一条，其它任务引用不变', async () => {
    vi.useFakeTimers()
    const t1 = task(1, { status: 'running' })
    const t2 = task(2)
    invoke.mockResolvedValue([t1, t2])
    const q = useQueueStore()
    await q.start()
    emit('tasks://progress', task(1, { status: 'running', received: 10 }))
    emit('tasks://progress', task(1, { status: 'running', received: 20 }))
    expect(q.tasks[0]).toBe(t1) // 还没写入
    vi.advanceTimersByTime(300)
    expect(q.tasks[0]).toMatchObject({ id: 1, received: 20 })
    expect(q.tasks[1]).toBe(t2)
    vi.useRealTimers()
  })

  it('完整列表到达时丢弃还没写入的旧进度', async () => {
    vi.useFakeTimers()
    invoke.mockResolvedValue([task(1)])
    const q = useQueueStore()
    await q.start()
    emit('tasks://progress', task(1, { received: 5 }))
    emit('tasks://updated', [task(1, { status: 'done', received: 100 })])
    vi.advanceTimersByTime(300)
    expect(q.tasks[0]).toMatchObject({ status: 'done', received: 100 })
    vi.useRealTimers()
  })
})

describe('useParseStore', () => {
  it('空输入：提示先粘贴链接，不调用后端', async () => {
    const p = useParseStore()
    await p.parse('   ')
    expect(p.error).toContain('粘贴')
    expect(invoke).not.toHaveBeenCalled()
  })

  it('成功：写入结果、清错误、刷新最近记录（最多 5 条）', async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'resolve_link') return Promise.resolve({ title: 'T' })
      if (cmd === 'list_history') return Promise.resolve(Array.from({ length: 8 }, (_, i) => ({ id: i })))
      return Promise.resolve(undefined)
    })
    const p = useParseStore()
    await p.parse('  https://x.test/1 ')
    expect(invoke).toHaveBeenCalledWith('resolve_link', { text: 'https://x.test/1' })
    expect(p.result).toEqual({ title: 'T' })
    expect(p.error).toBe('')
    expect(p.recent).toHaveLength(5)
    expect(p.loading).toBe(false)
  })

  it('需要登录：记录错误类型，并查询该链接对应的登录网站', async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'resolve_link') return Promise.reject({ kind: 'need_login', message: '请先登录' })
      if (cmd === 'login_site') return Promise.resolve('bilibili')
      return Promise.resolve(undefined)
    })
    const p = useParseStore()
    await p.parse('https://b.test/1')
    expect(p.error).toBe('请先登录')
    expect(p.errorKind).toBe('need_login')
    expect(p.errorSite).toBe('bilibili')
    expect(p.result).toBeNull()
    expect(p.loading).toBe(false)
  })

  it('普通错误：不查登录网站；clear 清空状态', async () => {
    invoke.mockRejectedValue('网络不通')
    const p = useParseStore()
    await p.parse('https://x.test')
    expect(p.error).toBe('网络不通')
    expect(p.errorKind).toBe('other')
    expect(invoke.mock.calls.some((c) => c[0] === 'login_site')).toBe(false)
    p.clear()
    expect(p.text).toBe('')
    expect(p.error).toBe('')
    expect(p.result).toBeNull()
  })
})
