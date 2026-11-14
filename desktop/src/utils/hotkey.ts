// 全局快捷键录入：把按键事件转成 Tauri 快捷键字符串，并检查是否可用。纯函数，便于单测。

export type CtrlName = 'CommandOrControl' | 'Ctrl'

export interface KeyLike {
  code: string
  key: string
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  metaKey: boolean
}

const MOD_CODES = new Set(['ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'ShiftLeft', 'ShiftRight', 'MetaLeft', 'MetaRight', 'OSLeft', 'OSRight'])
const NAMED: Record<string, string> = {
  Space: 'Space', Enter: 'Enter', NumpadEnter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete', Insert: 'Insert',
  Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
  ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right',
  Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\', Semicolon: ';', Quote: "'", Comma: ',', Period: '.', Slash: '/',
}

export const isModifierCode = (code: string) => MOD_CODES.has(code)

/** 主键名；纯修饰键、认不出的键返回空串。 */
export function keyName(code: string): string {
  if (MOD_CODES.has(code)) return ''
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit\d$/.test(code)) return code.slice(5)
  if (/^Numpad\d$/.test(code)) return 'Num' + code.slice(6)
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code
  return NAMED[code] ?? ''
}

/** 按键事件 → 组合字符串（修饰键顺序固定：Ctrl、Alt、Shift、Super）。主键为空时只返回修饰键部分。 */
export function comboFromEvent(e: KeyLike, ctrl: CtrlName = 'CommandOrControl'): string {
  const parts: string[] = []
  if (e.ctrlKey || (e.metaKey && ctrl === 'CommandOrControl')) parts.push(ctrl)
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey) parts.push('Shift')
  if (e.metaKey && (ctrl !== 'CommandOrControl' || e.ctrlKey)) parts.push('Super')
  const k = keyName(e.code)
  if (k) parts.push(k)
  return parts.join('+')
}

const norm = (s: string) => s.trim().toLowerCase().replace(/(^|\+)(commandorcontrol|cmdorctrl|control|ctrl)(?=\+|$)/g, '$1ctrl')

/** 是否同一个组合（Ctrl / CommandOrControl 视为相同，修饰键顺序不计）。 */
export function sameCombo(a: string, b: string): boolean {
  const split = (s: string) => norm(s).split('+').filter(Boolean)
  const x = split(a)
  const y = split(b)
  return x.length === y.length && [...x].sort().join('+') === [...y].sort().join('+')
}

const RESERVED = ['Ctrl+C', 'Ctrl+V', 'Ctrl+X', 'Ctrl+Z', 'Ctrl+Y', 'Ctrl+A', 'Ctrl+S', 'Ctrl+W', 'Ctrl+Q', 'Ctrl+Tab', 'Alt+F4', 'Alt+Tab', 'Ctrl+Alt+Delete', 'Ctrl+Shift+Escape']

export type HotkeyProblem = 'noMain' | 'noModifier' | 'reserved' | null

/** 检查组合能否当全局快捷键：要有主键；除 F1–F24 外至少一个修饰键；不抢常用系统快捷键。 */
export function checkCombo(combo: string): HotkeyProblem {
  const parts = combo.split('+').filter(Boolean)
  const mods = parts.filter((p) => ['CommandOrControl', 'Ctrl', 'Alt', 'Shift', 'Super'].includes(p))
  const main = parts.filter((p) => !mods.includes(p))
  if (main.length !== 1) return 'noMain'
  if (!mods.length && !/^F\d+$/.test(main[0])) return 'noModifier'
  if (RESERVED.some((r) => sameCombo(r, combo))) return 'reserved'
  return null
}
