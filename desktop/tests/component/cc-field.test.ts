import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import CcField from '../../src/components/ui/CcField.vue'

describe('CcField', () => {
  it('标签通过 for 与插槽给出的 id 关联，说明 / 错误 id 一并提供', () => {
    const w = mount(CcField, {
      props: { label: '代理地址', desc: '形如 http://127.0.0.1:7890' },
      slots: { default: `<template #default="{ id, descId, errId, invalid }"><input :id="id" :aria-describedby="descId" :data-err="errId" :data-invalid="invalid" /></template>` },
    })
    const label = w.get('label')
    const input = w.get('input')
    expect(label.text()).toBe('代理地址')
    expect(label.attributes('for')).toBe(input.attributes('id'))
    const descId = input.attributes('aria-describedby')!
    expect(w.get(`#${descId}`).text()).toContain('127.0.0.1')
    expect(input.attributes('data-invalid')).toBe('false')
  })

  it('controlId 覆盖自动 id', () => {
    const w = mount(CcField, { props: { label: 'x', controlId: 'my-ctl' } })
    expect(w.get('label').attributes('for')).toBe('my-ctl')
  })

  it('有 error：role=alert 显示文字，根节点带 invalid，插槽 invalid=true', () => {
    const w = mount(CcField, {
      props: { label: 'x', error: '不能为空' },
      slots: { default: `<template #default="{ errId, invalid }"><input :aria-errormessage="errId" :data-invalid="invalid" /></template>` },
    })
    const alert = w.get('[role=alert]')
    expect(alert.text()).toBe('不能为空')
    expect(w.classes()).toContain('invalid')
    expect(w.get('input').attributes('data-invalid')).toBe('true')
    expect(w.get('input').attributes('aria-errormessage')).toBe(alert.attributes('id'))
  })

  it('没有 desc / error 时不渲染对应节点；layout 落到类名', () => {
    const w = mount(CcField, { props: { label: 'x', layout: 'col' } })
    expect(w.find('.desc').exists()).toBe(false)
    expect(w.find('[role=alert]').exists()).toBe(false)
    expect(w.classes()).toContain('l-col')
  })

  it('自定义 label 插槽覆盖文字', () => {
    const w = mount(CcField, { props: { label: '默认' }, slots: { label: '<b>自定义</b>' } })
    expect(w.get('label b').text()).toBe('自定义')
  })
})
