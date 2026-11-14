// 颜色写死检查：src/ 下的 .vue/.css/.ts 里直接写的颜色字面量（#rgb / rgb() / hsl()）按文件计数，
// 与 scripts/color-baseline.json 对比：只许减不许增。theme.css、skin.css、components/ui/ 豁免。
// 用法：npm run lint:colors；减少后用 npm run lint:colors -- --update 刷新基线。
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '..')
const SRC = path.join(root, 'src')
const BASELINE = path.join(here, 'color-baseline.json')
const EXEMPT = ['src/styles/theme.css', 'src/styles/skin.css', 'src/components/ui/']
const EXT = new Set(['.vue', '.css', '.ts'])
const RE = /#[0-9a-fA-F]{8}\b|#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3,4}\b(?![\w-])|\b(?:rgba?|hsla?)\s*\(/g

export function countColors(text) {
  // 去掉注释，免得说明文字里的色值被计入
  const clean = text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/<!--[\s\S]*?-->/g, '').replace(/(^|[^:'"`])\/\/.*$/gm, '$1')
  return (clean.match(RE) || []).length
}

export function isExempt(rel) {
  return EXEMPT.some((e) => rel === e || (e.endsWith('/') && rel.startsWith(e)))
}

export function compare(current, baseline) {
  const bad = []
  for (const [f, n] of Object.entries(current)) {
    const b = baseline[f] ?? 0
    if (n > b) bad.push({ file: f, now: n, base: b })
  }
  return bad
}

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else if (EXT.has(path.extname(e.name))) out.push(p)
  }
  return out
}

export function scan() {
  const res = {}
  for (const f of walk(SRC)) {
    const rel = path.relative(root, f).split(path.sep).join('/')
    if (isExempt(rel)) continue
    const n = countColors(fs.readFileSync(f, 'utf8'))
    if (n) res[rel] = n
  }
  return Object.fromEntries(Object.entries(res).sort(([a], [b]) => a.localeCompare(b)))
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const cur = scan()
  if (process.argv.includes('--update')) {
    fs.writeFileSync(BASELINE, JSON.stringify(cur, null, 2) + '\n')
    console.log(`基线已更新：${Object.keys(cur).length} 个文件，共 ${Object.values(cur).reduce((a, b) => a + b, 0)} 处`)
    process.exit(0)
  }
  const base = JSON.parse(fs.readFileSync(BASELINE, 'utf8'))
  const bad = compare(cur, base)
  if (bad.length) {
    for (const b of bad) console.error(`颜色写死变多：${b.file} ${b.base} → ${b.now}（请改用 var(--cc-*) 令牌）`)
    process.exit(1)
  }
  const total = Object.values(cur).reduce((a, b) => a + b, 0)
  const was = Object.values(base).reduce((a, b) => a + b, 0)
  console.log(`lint:colors 通过：${total} 处（基线 ${was}）${total < was ? '，已减少，可用 -- --update 刷新基线' : ''}`)
}
