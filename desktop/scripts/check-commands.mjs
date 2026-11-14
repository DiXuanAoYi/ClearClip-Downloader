#!/usr/bin/env node
// 命令契约检查：前端 src/api 里 invoke 的命令名，必须都在 Rust 的 generate_handler! 列表里。
// 只解析源码（不编译、不运行）；有不匹配就退出码 1。
// 另外列出“Rust 注册了但前端 api 没有调用”的命令（仅提示：可能由别处的 invoke 或原生窗口使用）。
//   node scripts/check-commands.mjs [--quiet]
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const apiDir = join(root, 'src/api')
const libRs = join(root, 'src-tauri/src/lib.rs')

/** 去掉 Rust 的行注释和块注释（命令列表里可能临时注释掉某一行）。 */
export function stripRustComments(src) {
  return src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '')
}

/** 取 generate_handler![ ... ] 里的命令名（路径的最后一段）。 */
export function parseHandlers(rustSrc) {
  const src = stripRustComments(rustSrc)
  const names = []
  const re = /generate_handler!\s*\[([\s\S]*?)\]/g
  let m
  while ((m = re.exec(src))) {
    for (const item of m[1].split(',')) {
      const t = item.trim().replace(/^#\[[^\]]*\]\s*/, '')
      if (!t) continue
      names.push(t.split('::').pop())
    }
  }
  return names
}

/** 取前端源码里 invoke('name' / invoke<T>('name' 的命令名。 */
export function parseInvokes(tsSrc) {
  const names = []
  const re = /\binvoke\s*(?:<[^()]*?(?:<[^()]*?>[^()]*?)*>)?\s*\(\s*(['"`])([A-Za-z0-9_]+)\1/g
  let m
  while ((m = re.exec(tsSrc))) names.push(m[2])
  return names
}

/** 前端用了 invoke 但名字不是字面量（检查不到）的次数。 */
export function countDynamicInvokes(tsSrc) {
  const all = (tsSrc.match(/\binvoke\s*(?:<[^()]*?(?:<[^()]*?>[^()]*?)*>)?\s*\(/g) || []).length
  return all - parseInvokes(tsSrc).length
}

export function check(handlers, invokes) {
  const have = new Set(handlers)
  const missing = [...new Set(invokes.map((i) => i.name))].filter((n) => !have.has(n)).sort()
  const used = new Set(invokes.map((i) => i.name))
  const unused = handlers.filter((h) => !used.has(h)).sort()
  const dupHandlers = handlers.filter((h, i) => handlers.indexOf(h) !== i)
  return { missing, unused, dupHandlers }
}

function main() {
  const quiet = process.argv.includes('--quiet')
  const handlers = parseHandlers(readFileSync(libRs, 'utf8'))
  const invokes = []
  let dynamic = 0
  for (const f of readdirSync(apiDir).filter((f) => f.endsWith('.ts'))) {
    const src = readFileSync(join(apiDir, f), 'utf8')
    for (const name of parseInvokes(src)) invokes.push({ name, file: f })
    dynamic += countDynamicInvokes(src)
  }
  if (handlers.length === 0 || invokes.length === 0) {
    console.error(`命令契约检查：解析结果为空（handlers=${handlers.length}, invokes=${invokes.length}），解析规则可能已失效`)
    process.exit(1)
  }
  const { missing, unused, dupHandlers } = check(handlers, invokes)
  if (!quiet) console.log(`命令契约：Rust 注册 ${handlers.length} 个命令，前端 api 调用 ${new Set(invokes.map((i) => i.name)).size} 个不同命令`)
  if (dynamic > 0) console.log(`提示：有 ${dynamic} 处 invoke 的命令名不是字面量，没有检查`)
  if (!quiet && unused.length) console.log(`提示：${unused.length} 个命令在 Rust 注册但前端 api 没有调用（不影响结果）`)
  if (dupHandlers.length) console.error(`警告：generate_handler! 里有重复命令：${[...new Set(dupHandlers)].join(', ')}`)
  if (missing.length) {
    console.error('命令契约检查失败：前端调用了 Rust 没有注册的命令：')
    for (const n of missing) console.error(`  - ${n}（${invokes.filter((i) => i.name === n).map((i) => 'src/api/' + i.file).filter((v, i, a) => a.indexOf(v) === i).join(', ')}）`)
    process.exit(1)
  }
  if (!quiet) console.log('命令契约检查通过')
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main()
