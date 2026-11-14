import test from 'node:test'
import assert from 'node:assert/strict'
import { comboFromEvent, checkCombo, sameCombo, keyName } from '../../src/utils/hotkey.ts'

const ev = (code, o = {}) => ({ code, key: '', ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...o })

test('按键 → 组合：修饰键顺序固定', () => {
  assert.equal(comboFromEvent(ev('KeyH', { shiftKey: true, ctrlKey: true })), 'CommandOrControl+Shift+H')
  assert.equal(comboFromEvent(ev('KeyT', { altKey: true, ctrlKey: true }), 'Ctrl'), 'Ctrl+Alt+T')
  assert.equal(comboFromEvent(ev('Digit5', { ctrlKey: true })), 'CommandOrControl+5')
  assert.equal(comboFromEvent(ev('ArrowUp', { altKey: true })), 'Alt+Up')
  assert.equal(comboFromEvent(ev('F9')), 'F9')
})

test('只按了修饰键时不带主键', () => {
  assert.equal(comboFromEvent(ev('ShiftLeft', { shiftKey: true })), 'Shift')
  assert.equal(keyName('ControlLeft'), '')
})

test('检查：缺主键 / 缺修饰键 / 系统保留', () => {
  assert.equal(checkCombo('CommandOrControl+Shift'), 'noMain')
  assert.equal(checkCombo('H'), 'noModifier')
  assert.equal(checkCombo('F9'), null)
  assert.equal(checkCombo('CommandOrControl+C'), 'reserved')
  assert.equal(checkCombo('Alt+F4'), 'reserved')
  assert.equal(checkCombo('CommandOrControl+Shift+H'), null)
})

test('同一组合：Ctrl 与 CommandOrControl 等价，顺序无关', () => {
  assert.ok(sameCombo('Ctrl+Shift+D', 'Shift+CommandOrControl+d'))
  assert.ok(!sameCombo('Ctrl+Shift+D', 'Ctrl+Alt+D'))
})
