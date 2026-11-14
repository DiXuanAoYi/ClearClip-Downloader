import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import CcButton from '../../src/components/ui/CcButton.vue'

describe('CcButton', () => {
  it('默认是 type=button 的次要按钮，渲染插槽文字', () => {
    const w = mount(CcButton, { slots: { default: '保存' } })
    const b = w.get('button')
    expect(b.attributes('type')).toBe('button')
    expect(b.classes()).toContain('k-secondary')
    expect(b.text()).toBe('保存')
  })

  it('点击触发 click', async () => {
    const w = mount(CcButton, { slots: { default: 'x' } })
    await w.get('button').trigger('click')
    expect(w.emitted('click')).toHaveLength(1)
  })

  it('loading：不触发 click，aria-busy / aria-disabled，仍可聚焦（不 disabled）', async () => {
    const w = mount(CcButton, { props: { loading: true }, slots: { default: 'x' } })
    const b = w.get('button')
    await b.trigger('click')
    expect(w.emitted('click')).toBeUndefined()
    expect(b.attributes('aria-busy')).toBe('true')
    expect(b.attributes('aria-disabled')).toBe('true')
    expect(b.attributes('disabled')).toBeUndefined()
  })

  it('disabled：原生 disabled，不触发 click；给原因时写入 title 并供读屏读出', async () => {
    const w = mount(CcButton, { props: { disabled: true, disabledReason: '没有选中项' }, slots: { default: '删除' } })
    const b = w.get('button')
    expect(b.attributes('disabled')).toBeDefined()
    expect(b.attributes('title')).toBe('没有选中项')
    expect(b.find('.cc-sr').text()).toContain('没有选中项')
    // 原生 disabled 按钮 trigger 不派发事件；直接 dispatch 也应被组件内部拦住
    b.element.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(w.emitted('click')).toBeUndefined()
  })

  it('图标按钮：使用 ariaLabel 作为名称与提示，不渲染文字', () => {
    const w = mount(CcButton, { props: { iconOnly: true, ariaLabel: '关闭' }, slots: { icon: '<i class="x" />', default: '不应出现' } })
    const b = w.get('button')
    expect(b.attributes('aria-label')).toBe('关闭')
    expect(b.attributes('title')).toBe('关闭')
    expect(b.classes()).toContain('icon')
    expect(b.find('.tx').exists()).toBe(false)
    expect(b.find('.ic').attributes('aria-hidden')).toBe('true')
  })

  it('kind / size / type 落到类名和属性上', () => {
    const w = mount(CcButton, { props: { kind: 'danger', size: 'l', type: 'submit' } })
    const b = w.get('button')
    expect(b.classes()).toEqual(expect.arrayContaining(['k-danger', 's-l']))
    expect(b.attributes('type')).toBe('submit')
  })
})
