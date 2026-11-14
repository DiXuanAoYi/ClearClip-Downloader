import { describe, it, expect, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'
import { useTauriEvent, useTauriEvents } from '../../src/composables/useTauriEvent'

const flush = () => new Promise((r) => setTimeout(r, 0))

describe('useTauriEvent(s) 在真实组件里', () => {
  it('组件卸载时取消全部监听', async () => {
    const off1 = vi.fn()
    const off2 = vi.fn()
    let api!: ReturnType<typeof useTauriEvents>
    const C = defineComponent({
      setup() {
        api = useTauriEvents()
        return () => h('div')
      },
    })
    const w = mount(C)
    await api.on(Promise.resolve(off1))
    await api.on(off2)
    expect(off1).not.toHaveBeenCalled()
    w.unmount()
    expect(off1).toHaveBeenCalledTimes(1)
    expect(off2).toHaveBeenCalledTimes(1)
  })

  it('注册过程中组件已卸载：注册一完成就取消，不泄漏', async () => {
    const off = vi.fn()
    let resolve!: (f: () => void) => void
    const C = defineComponent({
      setup() {
        useTauriEvent(() => new Promise<() => void>((r) => (resolve = r)))
        return () => h('div')
      },
    })
    const w = mount(C)
    w.unmount()
    resolve(off)
    await flush()
    expect(off).toHaveBeenCalledTimes(1)
  })

  it('手动取消幂等：与卸载自动取消同时发生也只取消一次', async () => {
    const off = vi.fn()
    let un!: () => void
    const C = defineComponent({
      setup() {
        const ev = useTauriEvents()
        void ev.on(Promise.resolve(off)).then((u) => (un = u))
        return () => h('div')
      },
    })
    const w = mount(C)
    await flush()
    un()
    un()
    w.unmount()
    expect(off).toHaveBeenCalledTimes(1)
  })

  it('原始取消函数抛错（窗口已关闭）被吞掉', async () => {
    const C = defineComponent({
      setup() {
        const ev = useTauriEvents()
        void ev.on(Promise.resolve(() => { throw new Error('gone') }))
        return () => h('div')
      },
    })
    const w = mount(C)
    await flush()
    expect(() => w.unmount()).not.toThrow()
  })

  it('注册失败只记日志，不抛未处理异常', async () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    const C = defineComponent({
      setup() {
        useTauriEvent(() => Promise.reject(new Error('no ipc')))
        return () => h('div')
      },
    })
    mount(C)
    await flush()
    expect(err).toHaveBeenCalled()
  })
})
