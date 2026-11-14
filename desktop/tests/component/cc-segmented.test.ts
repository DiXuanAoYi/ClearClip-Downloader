import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import CcSegmented from '../../src/components/ui/CcSegmented.vue'

const options = [
  { value: 'a', label: 'A' },
  { value: 'b', label: 'B', disabled: true },
  { value: 'c', label: 'C' },
]
const make = (props: Record<string, unknown> = {}) => mount(CcSegmented, { props: { modelValue: 'a', options, ariaLabel: '模式', ...props }, attachTo: document.body })

describe('CcSegmented', () => {
  it('radiogroup 语义：aria-checked、漫游 tabindex', () => {
    const w = make()
    expect(w.attributes('role')).toBe('radiogroup')
    expect(w.attributes('aria-label')).toBe('模式')
    const radios = w.findAll('[role=radio]')
    expect(radios.map((r) => r.attributes('aria-checked'))).toEqual(['true', 'false', 'false'])
    expect(radios.map((r) => r.attributes('tabindex'))).toEqual(['0', '-1', '-1'])
    expect(radios[1].attributes('aria-disabled')).toBe('true')
    w.unmount()
  })

  it('点击另一项发出 update:modelValue；点当前项或禁用项不发', async () => {
    const w = make()
    const radios = w.findAll('[role=radio]')
    await radios[0].trigger('click')
    await radios[1].trigger('click')
    expect(w.emitted('update:modelValue')).toBeUndefined()
    await radios[2].trigger('click')
    expect(w.emitted('update:modelValue')).toEqual([['c']])
    w.unmount()
  })

  it('方向键跳过禁用项并循环；Home / End 到首尾', async () => {
    const w = make()
    const radios = w.findAll('[role=radio]')
    await radios[0].trigger('keydown', { key: 'ArrowRight' })
    expect(w.emitted('update:modelValue')?.at(-1)).toEqual(['c'])
    await radios[0].trigger('keydown', { key: 'ArrowLeft' })
    expect(w.emitted('update:modelValue')?.at(-1)).toEqual(['c'])
    await radios[0].trigger('keydown', { key: 'End' })
    expect(w.emitted('update:modelValue')?.at(-1)).toEqual(['c'])
    await w.setProps({ modelValue: 'c' })
    await w.findAll('[role=radio]')[2].trigger('keydown', { key: 'Home' })
    expect(w.emitted('update:modelValue')?.at(-1)).toEqual(['a'])
    w.unmount()
  })

  it('整体 disabled：所有项不可点，且没有可 Tab 进入的项', async () => {
    const w = make({ disabled: true })
    await w.findAll('[role=radio]')[2].trigger('click')
    expect(w.emitted('update:modelValue')).toBeUndefined()
    expect(w.attributes('aria-disabled')).toBe('true')
    expect(w.findAll('[role=radio]').every((r) => r.attributes('tabindex') === '-1')).toBe(true)
    w.unmount()
  })

  it('没有选中项时第一个可用项可被 Tab 进入', () => {
    const w = make({ modelValue: 'zzz' })
    expect(w.findAll('[role=radio]').map((r) => r.attributes('tabindex'))).toEqual(['0', '-1', '-1'])
    w.unmount()
  })
})
