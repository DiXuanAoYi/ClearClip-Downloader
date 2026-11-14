import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const openUrl = vi.fn(() => Promise.resolve())
vi.mock('../../src/api', () => ({ api: { openUrl: (u: string) => openUrl(u) } }))

import CcEmpty from '../../src/components/ui/CcEmpty.vue'
import CcSkeleton from '../../src/components/ui/CcSkeleton.vue'
import CcLink from '../../src/components/ui/CcLink.vue'

describe('CcEmpty', () => {
  it('渲染标题与说明；图为装饰（aria-hidden）；无默认插槽时没有操作区', () => {
    const w = mount(CcEmpty, { props: { title: '还没有任务', desc: '粘贴链接开始' } })
    expect(w.get('h4').text()).toBe('还没有任务')
    expect(w.get('p').text()).toBe('粘贴链接开始')
    expect(w.get('.art').attributes('aria-hidden')).toBe('true')
    expect(w.find('.act').exists()).toBe(false)
  })
  it('没有说明不渲染 p；有默认插槽渲染操作区；compact 加类', () => {
    const w = mount(CcEmpty, { props: { title: 't', compact: true }, slots: { default: '<button>go</button>' } })
    expect(w.find('p').exists()).toBe(false)
    expect(w.get('.act button').text()).toBe('go')
    expect(w.classes()).toContain('cmp')
  })
  it('desc 插槽优先于 desc 属性的位置渲染', () => {
    const w = mount(CcEmpty, { props: { title: 't' }, slots: { desc: '<em>富文本</em>' } })
    expect(w.get('p em').text()).toBe('富文本')
  })
})

describe('CcSkeleton', () => {
  it('单块：role=status、默认 aria-label，数字高度加 px', () => {
    const w = mount(CcSkeleton, { props: { height: 20 } })
    expect(w.attributes('role')).toBe('status')
    expect(w.attributes('aria-label')).toBe('加载中')
    expect((w.element as HTMLElement).style.height).toBe('20px')
  })
  it('circle：宽等于高且为圆', () => {
    const w = mount(CcSkeleton, { props: { circle: true, height: 32 } })
    const s = (w.element as HTMLElement).style
    expect(s.width).toBe('32px')
    expect(s.borderRadius).toBe('50%')
  })
  it('多行：只有外层是 status，内部装饰；最后一行 60%', () => {
    const w = mount(CcSkeleton, { props: { lines: 3, label: '读取中' } })
    expect(w.attributes('aria-label')).toBe('读取中')
    const rows = w.findAll('.cc-sk')
    expect(rows).toHaveLength(3)
    expect(rows.every((r) => r.attributes('aria-hidden') === 'true')).toBe(true)
    expect((rows[2].element as HTMLElement).style.width).toBe('60%')
    expect((rows[0].element as HTMLElement).style.width).toBe('100%')
  })
})

describe('CcLink', () => {
  beforeEach(() => openUrl.mockClear())

  it('有 href：渲染 a；站内链接不拦截', async () => {
    const w = mount(CcLink, { props: { href: '#/x' }, slots: { default: '详情' } })
    expect(w.element.tagName).toBe('A')
    expect(w.attributes('target')).toBeUndefined()
    await w.trigger('click')
    expect(openUrl).not.toHaveBeenCalled()
    expect(w.emitted('click')).toHaveLength(1)
  })
  it('external：noopener、target=_blank、读屏提示，点击交给 api.openUrl 并阻止默认跳转', async () => {
    const w = mount(CcLink, { props: { href: 'https://example.com', external: true }, slots: { default: '官网' } })
    expect(w.attributes('rel')).toBe('noopener noreferrer')
    expect(w.attributes('target')).toBe('_blank')
    expect(w.get('.cc-sr').text()).toContain('浏览器')
    const ev = new MouseEvent('click', { bubbles: true, cancelable: true })
    w.element.dispatchEvent(ev)
    expect(ev.defaultPrevented).toBe(true)
    expect(openUrl).toHaveBeenCalledWith('https://example.com')
  })
  it('没有 href：渲染 button[type=button]', () => {
    const w = mount(CcLink, { slots: { default: '重试' } })
    expect(w.element.tagName).toBe('BUTTON')
    expect(w.attributes('type')).toBe('button')
  })
  it('disabled 的链接退化为禁用按钮，点击不触发', async () => {
    const w = mount(CcLink, { props: { href: 'https://e.com', external: true, disabled: true }, slots: { default: 'x' } })
    expect(w.element.tagName).toBe('BUTTON')
    expect(w.attributes('disabled')).toBeDefined()
    await w.trigger('click')
    expect(openUrl).not.toHaveBeenCalled()
  })
})
