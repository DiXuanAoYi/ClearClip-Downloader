// 从 Rust 结构生成 TypeScript 类型：node scripts/gen-types.mjs [--check]
// Rust 里给结构加 `#[derive(ts_rs::TS)]` 和 `#[ts(export, export_to = "../../src/generated/")]`，
// 这个脚本跑 `cargo test export_bindings`，把 ts-rs 生成的文件写到 src/generated/，
// 并把 64 位整数的 `bigint` 改成 `number`（Tauri 传的是 JSON 数字，界面里一直用 number）。
// --check：生成后如果和已提交的文件不一样就报错（CI 用）。
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..')
const dir = path.join(root, 'src/generated')

/** ts-rs 默认把 u64 / i64 生成 bigint；这里统一成 number，并保证文件末尾有换行。 */
export function normalize(text) {
  return text.replace(/\bbigint\b/g, 'number').replace(/\n*$/, '\n')
}

export function snapshot() {
  if (!fs.existsSync(dir)) return {}
  return Object.fromEntries(fs.readdirSync(dir).filter((f) => f.endsWith('.ts')).sort().map((f) => [f, fs.readFileSync(path.join(dir, f), 'utf8')]))
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes('--check')
  const before = snapshot()
  fs.rmSync(dir, { recursive: true, force: true })
  execFileSync('cargo', ['test', '--lib', 'export_bindings', '--quiet'], { cwd: path.join(root, 'src-tauri'), stdio: 'inherit' })
  for (const f of Object.keys(snapshot())) {
    const p = path.join(dir, f)
    fs.writeFileSync(p, normalize(fs.readFileSync(p, 'utf8')))
  }
  const after = snapshot()
  console.log(`生成了 ${Object.keys(after).length} 个类型文件`)
  if (check) {
    const diff = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter((f) => before[f] !== after[f])
    if (diff.length) {
      console.error('生成的类型和已提交的不一致，请运行 npm run gen:types 后提交：' + diff.join('、'))
      process.exit(1)
    }
  }
}
