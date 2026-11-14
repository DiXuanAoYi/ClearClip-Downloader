#!/usr/bin/env node
// 界面翻译辅助：找出源码里所有可见的中文文字，对照 src/i18n/en.ts 检查哪些还没有翻译。
//
//   node scripts/i18n.mjs            列出缺少翻译的条目（不影响退出码）
//   node scripts/i18n.mjs --strict   有缺少翻译的条目、或翻译表里有无人引用的条目时以退出码 1 结束
//   node scripts/i18n.mjs --strict --max-missing=N
//                                    基线模式：缺少的条目不超过 N 个才算通过（多余条目仍必须为 0）。
//                                    CI 里用 `npm run i18n -- --strict --max-missing=<N>`，补齐翻译后把 N 往下调
//   node scripts/i18n.mjs --dump     输出全部中文条目（JSON）
//
// 带变量的文字用 {0}、{1} 占位：模板里的 {{ x }} 和脚本里的 `${x}` 都算一个占位，
// 例如 `已选 {{ n }} 项` 和 `已选 ${n} 项` 对应同一个条目 "已选 {0} 项"。

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, extname } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { parse as parseSfc } from '@vue/compiler-sfc'
import { NodeTypes } from '@vue/compiler-dom'

const root = join(fileURLToPath(new URL('.', import.meta.url)), '..', 'src')
const CJK = /[一-鿿]/

/** 开发 / e2e 用的陈列页（不进安装包，文案不面向用户）：不扫描、不翻译。 */
export const EXCLUDED = ['ui-gallery.ts', 'components/ui/UiGallery.vue']
export const isExcluded = (p) => EXCLUDED.some((e) => p.split('\\').join('/').endsWith('/' + e))

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) {
      if (name !== 'i18n') yield* walk(p)
    } else if (['.vue', '.ts'].includes(extname(p)) && !p.endsWith('.d.ts') && !isExcluded(p)) {
      yield p
    }
  }
}

const norm = (s) => s.replace(/\s+/g, ' ').trim()

/** TypeScript / JavaScript 源码里的字符串字面量（含模板字符串）。 */
function scriptStrings(code, out, file) {
  const sf = ts.createSourceFile(file, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS)
  const add = (s) => {
    const t = norm(s)
    if (!CJK.test(t)) return
    // v-html 用的 HTML 片段：运行时显示的是标签之间的文字，按文字条目收集
    if (/<[a-z][^>]*>/i.test(t)) {
      for (const m of t.matchAll(/>([^<>]+)</g)) {
        const piece = norm(m[1])
        if (CJK.test(piece)) out.add(piece)
      }
      return
    }
    out.add(t)
  }
  const visit = (n) => {
    // 搜索用的关键词（拼音、同义词）不会显示在界面上，不需要翻译
    if (ts.isPropertyAssignment(n) && ts.isIdentifier(n.name) && n.name.text === 'keywords') return
    if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) add(n.text)
    else if (ts.isTemplateExpression(n)) {
      let s = n.head.text
      n.templateSpans.forEach((sp, i) => {
        s += `{${i}}` + sp.literal.text
      })
      add(s)
    }
    ts.forEachChild(n, visit)
  }
  visit(sf)
}

/** 模板里的文字：相邻的文本和插值在运行时是同一个文本节点，合并成一个条目。 */
function templateStrings(src, out, file) {
  const { descriptor } = parseSfc(src, { filename: file })
  if (descriptor.template) {
    const walkNode = (node) => {
      if (node.type === NodeTypes.ELEMENT) {
        for (const p of node.props) {
          if (p.type === NodeTypes.ATTRIBUTE && p.value && CJK.test(p.value.content)) out.add(norm(p.value.content))
          else if (p.type === NodeTypes.DIRECTIVE && p.exp?.loc?.source && CJK.test(p.exp.loc.source)) {
            // :title="'中文'"、@click="msg('中文')" 之类：按脚本处理
            scriptStrings(`(${p.exp.loc.source})`, out, file + '.ts')
          }
        }
      }
      const kids = node.children
      if (!Array.isArray(kids)) return
      let run = null
      let n = 0
      const flush = () => {
        if (run !== null) {
          const t = norm(run)
          if (CJK.test(t)) out.add(t)
        }
        run = null
        n = 0
      }
      for (const c of kids) {
        if (c.type === NodeTypes.TEXT) run = (run ?? '') + c.content
        else if (c.type === NodeTypes.INTERPOLATION) {
          const exp = c.content.loc?.source ?? ''
          // 插值里的三元表达式等也可能带中文字符串
          if (CJK.test(exp)) scriptStrings(`(${exp})`, out, file + '.ts')
          run = (run ?? '') + `{${n++}}`
        } else if (c.type === NodeTypes.COMMENT) {
          // 注释不显示
        } else {
          flush()
          walkNode(c)
        }
      }
      flush()
    }
    walkNode(descriptor.template.ast)
  }
  const script = [descriptor.script, descriptor.scriptSetup].filter(Boolean)
  for (const s of script) scriptStrings(s.content, out, file + '.ts')
}

export function extract() {
  const out = new Set()
  for (const f of walk(root)) {
    const src = readFileSync(f, 'utf8')
    if (f.endsWith('.vue')) templateStrings(src, out, f)
    else scriptStrings(src, out, f)
  }
  return [...out].sort()
}

async function loadEn() {
  // en.ts 是纯数据，去掉类型标注后当 JS 执行
  const text = readFileSync(join(root, 'i18n', 'en.ts'), 'utf8')
  const body = text.replace(/export const en[^=]*=/, 'return ')
  return new Function(body)()
}

/** 后端（Rust）里的中文消息会原样显示在界面上，翻译表里为它们留的条目不算多余。 */
function backendText() {
  const dir = join(root, '..', 'src-tauri', 'src')
  let text = ''
  const rec = (d) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name)
      if (statSync(p).isDirectory()) rec(p)
      else if (name.endsWith('.rs')) text += readFileSync(p, 'utf8') + '\n'
    }
  }
  try {
    rec(dir)
  } catch {
    /* 没有 Rust 源码时（只拿到 src/）不做这项检查 */
  }
  return text.replace(/\s+/g, ' ')
}

/** 条目里最长的一段固定文字（占位符之间）；Rust 的 `{}` / `{x}` 占位和这里的 `{0}` 写法不同，所以只比固定文字。 */
export function longestFragment(key) {
  return key.split(/\{\d+\}/).map((x) => x.trim()).sort((a, b) => b.length - a.length)[0] ?? ''
}

/** Rust 里用常量或拼接生成的消息：源码里找不到整段，登记在这里（键 → 说明）。 */
const BACKEND_BUILT = new Set(['工程文件的扩展名是 .ccedit。'])

export function findStale(en, all, backend = backendText()) {
  const used = new Set(all)
  const inBackend = (frag) => frag.length >= 2 && backend.includes(frag)
  return Object.keys(en).filter((k) => {
    if (used.has(k) || BACKEND_BUILT.has(k)) return false
    if (inBackend(longestFragment(k))) return false
    // 后端按“，”“：”分段拼出来的长句（例如“分段色彩匹配：…”）：每一段都能在后端找到才算
    const chunks = k.split(/[，：]/).map(longestFragment).filter((x) => x.length >= 2)
    return !(chunks.length > 1 && chunks.every(inBackend))
  })
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const all = extract()
  if (process.argv.includes('--dump')) {
    console.log(JSON.stringify(all, null, 1))
    process.exit(0)
  }
  const en = await loadEn()
  const missing = all.filter((s) => !(s in en))
  const stale = findStale(en, all)
  console.log(`中文条目 ${all.length} 个，已翻译 ${all.length - missing.length} 个，缺少 ${missing.length} 个，翻译表里多余 ${stale.length} 个。`)
  if (missing.length) {
    console.log('\n缺少翻译：')
    for (const m of missing) console.log('  ' + JSON.stringify(m))
  }
  if (stale.length && process.argv.includes('--stale')) {
    console.log('\n多余的（源码里已经没有）：')
    for (const s of stale) console.log('  ' + JSON.stringify(s))
  }
  if (process.argv.includes('--strict')) {
    const arg = process.argv.find((a) => a.startsWith('--max-missing='))
    const maxMissing = arg ? Number(arg.slice('--max-missing='.length)) : 0
    if (!Number.isInteger(maxMissing) || maxMissing < 0) {
      console.error(`--max-missing 要是非负整数：${arg}`)
      process.exit(2)
    }
    let bad = false
    if (missing.length > maxMissing) {
      console.error(`\n缺少翻译 ${missing.length} 个，超过允许的 ${maxMissing} 个。`)
      bad = true
    }
    if (stale.length) {
      console.error(`翻译表里有 ${stale.length} 个条目源码和后端都没有引用，请删除（用 --stale 查看）。`)
      bad = true
    }
    if (bad) process.exit(1)
  }
}
void relative
