import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import ElementPlus from 'element-plus'
import CcSelect from '../../src/components/ui/CcSelect.vue'
import CcDialog from '../../src/components/ui/CcDialog.vue'

const opts = (n: number) => Array.from({ length: n }, (_, i) => ({ value: `v${i}`, label: `选项${i}` }))
const global = { plugins: [ElementPlus] }

describe('CcSelect', () => {
  it('把 size / width / invalid / ariaLabel 映射到 el-select', () => {
    const w = mount(CcSelect, { props: { modelValue: 'v0', options: opts(3), size: 's', width: 'l', invalid: true, ariaLabel: '语言' }, global })
    const sel = w.findComponent({ name: 'ElSelect' })
    expect(sel.exists()).toBe(true)
    expect(sel.classes()).toEqual(expect.arrayContaining(['cc-select', 's-s', 'w-l', 'invalid']))
    expect(sel.props('size')).toBe('small')
    expect(sel.props('placeholder')).toBe('请选择')
  })

  it('filterable 显式开启时传给 el-select；选项不多且未指定时为关', () => {
    const few = mount(CcSelect, { props: { modelValue: 'v0', options: opts(8) }, global })
    const forced = mount(CcSelect, { props: { modelValue: 'v0', options: opts(3), filterable: true }, global })
    expect(few.findComponent({ name: 'ElSelect' }).props('filterable')).toBe(false)
    expect(forced.findComponent({ name: 'ElSelect' }).props('filterable')).toBe(true)
  })

  // 已知问题（未改产品行为）：组件注释写“超过 8 项自动开启搜索”，但 filterable 是 Boolean 属性，
  // 不传时 Vue 会把 undefined 转成 false，`props.filterable ?? options.length > 8` 的右边永远不会生效。
  // it.fails：现在“失败”才算通过；修好以后这条会报错，提醒把它改成普通 it。
  it.fails('选项超过 8 项自动可搜索（文档约定，目前未生效）', () => {
    const many = mount(CcSelect, { props: { modelValue: 'v0', options: opts(9) }, global })
    expect(many.findComponent({ name: 'ElSelect' }).props('filterable')).toBe(true)
  })

  it('el-select 变更时同时发出 update:modelValue 与 change', async () => {
    const w = mount(CcSelect, { props: { modelValue: 'v0', options: opts(3) }, global })
    w.findComponent({ name: 'ElSelect' }).vm.$emit('update:modelValue', 'v2')
    await nextTick()
    expect(w.emitted('update:modelValue')).toEqual([['v2']])
    expect(w.emitted('change')).toEqual([['v2']])
  })
})

describe('CcDialog', () => {
  const mk = (props: Record<string, unknown> = {}, slots: Record<string, string> = {}) =>
    mount(CcDialog, { props: { modelValue: true, title: '确认删除', ...props }, slots: { default: '<p class="body">正文</p>', ...slots }, global, attachTo: document.body })

  it('打开时渲染标题、正文，关闭按钮有无障碍名称', async () => {
    const w = mk()
    await nextTick()
    const body = document.body
    expect(body.querySelector('.cc-dlg-h h4')?.textContent).toBe('确认删除')
    expect(body.querySelector('.body')?.textContent).toBe('正文')
    expect(body.querySelector('.cc-dlg-x')?.getAttribute('aria-label')).toBe('关闭')
    w.unmount()
  })

  it('点关闭按钮发出 update:modelValue=false', async () => {
    const w = mk()
    await nextTick()
    ;(document.body.querySelector('.cc-dlg-x') as HTMLElement).click()
    expect(w.emitted('update:modelValue')).toEqual([[false]])
    w.unmount()
  })

  it('danger 显示警示图标；有 footer 插槽才渲染页脚', async () => {
    const w = mk({ danger: true }, { footer: '<button class="ok">好</button>' })
    await nextTick()
    expect(document.body.querySelector('.cc-dlg-h svg.warn')).not.toBeNull()
    expect(document.body.querySelector('.cc-dlg-ft .ok')).not.toBeNull()
    w.unmount()
  })

  it('modelValue=false 时不渲染内容', async () => {
    const w = mk({ modelValue: false })
    await nextTick()
    expect(document.body.querySelector('.cc-dlg-bd')).toBeNull()
    w.unmount()
  })
})
