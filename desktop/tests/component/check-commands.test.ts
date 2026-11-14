import { describe, it, expect } from 'vitest'
// @ts-expect-error 纯 JS 脚本，没有类型声明
import { parseHandlers, parseInvokes, countDynamicInvokes, check, stripRustComments } from '../../scripts/check-commands.mjs'

describe('命令契约检查脚本', () => {
  it('parseHandlers 取路径最后一段，忽略注释掉的行与属性', () => {
    const rs = `.invoke_handler(lock_gate(tauri::generate_handler![
      commands::get_settings,
      // commands::old_one,
      /* commands::older, */
      security_cmds::lock_now,
      plain_fn,
    ]))`
    expect(parseHandlers(rs)).toEqual(['get_settings', 'lock_now', 'plain_fn'])
    expect(stripRustComments('a // b\nc /* d */ e')).toBe('a \nc  e')
  })

  it('parseInvokes 兼容泛型、嵌套泛型和多种引号', () => {
    const ts = `
      a: () => invoke<void>('a_cmd'),
      b: (x: number) => invoke<Record<string, Foo<Bar>>>("b_cmd", { x }),
      c: () => invoke(\`c_cmd\`),
    `
    expect(parseInvokes(ts)).toEqual(['a_cmd', 'b_cmd', 'c_cmd'])
  })

  it('非字面量的 invoke 被计数而不是悄悄漏掉', () => {
    expect(countDynamicInvokes(`invoke<void>(name)\ninvoke<void>('ok')`)).toBe(1)
  })

  it('check：前端用了未注册的命令 -> missing；未被前端使用的 -> unused', () => {
    const r = check(['a', 'b', 'a'], [{ name: 'a' }, { name: 'zzz' }])
    expect(r.missing).toEqual(['zzz'])
    expect(r.unused).toEqual(['b'])
    expect(r.dupHandlers).toEqual(['a'])
  })

  it('真实仓库：前端 api 调用的命令全部已注册', async () => {
    const { readFileSync, readdirSync } = await import('node:fs')
    const handlers = parseHandlers(readFileSync('src-tauri/src/lib.rs', 'utf8'))
    const invokes = readdirSync('src/api')
      .filter((f) => f.endsWith('.ts'))
      .flatMap((f) => parseInvokes(readFileSync(`src/api/${f}`, 'utf8')).map((name: string) => ({ name })))
    expect(handlers.length).toBeGreaterThan(100)
    expect(invokes.length).toBeGreaterThan(100)
    expect(check(handlers, invokes).missing).toEqual([])
  })
})
