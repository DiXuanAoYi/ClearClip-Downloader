// 扩展的文件清单：files-manifest.js 是唯一来源。
// 检查：清单里的文件都存在；manifest.json 的 background.scripts 与它一致；各注入组满足依赖顺序，并且依赖表没有漏项；
// 后台消息的处理函数与入口检查的规则表一一对应。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const read = (f) => fs.readFileSync(path.join(dir, f), 'utf8')
const plain = (v) => JSON.parse(JSON.stringify(v))

const ctx = vm.createContext({})
vm.runInContext(read('files-manifest.js'), ctx, { filename: 'files-manifest.js' })
const F = plain(vm.runInContext('ccFiles', ctx))
const manifest = JSON.parse(read('manifest.json'))

test('清单里的每个文件都存在，且组内不重复', () => {
  const all = new Set([...F.BG_SCRIPTS, F.BG_MAIN])
  for (const [name, g] of Object.entries(F.GROUPS)) {
    assert.equal(new Set(g).size, g.length, `${name} 组里有重复的文件`)
    g.forEach((f) => all.add(f))
  }
  for (const f of all) assert.ok(fs.existsSync(path.join(dir, f)), `${f} 不存在`)
  assert.equal(new Set(F.BG_SCRIPTS).size, F.BG_SCRIPTS.length, '后台脚本清单有重复')
})

test('manifest.json 的 background.scripts 与 files-manifest.js 一致（顺序也一致，background.js 最后）', () => {
  assert.deepEqual(manifest.background.scripts, [...F.BG_SCRIPTS, F.BG_MAIN])
  assert.equal(manifest.background.service_worker, F.BG_MAIN)
})

test('background.js 只从 files-manifest.js 取清单，不再手写文件名', () => {
  const src = read('background.js')
  assert.match(src, /importScripts\('files-manifest\.js'\)/)
  assert.match(src, /ccFiles\.BG_SCRIPTS/)
  assert.ok(!/importScripts\([^)]*'shared\.js'/.test(src), 'importScripts 里不应再手写 shared.js 等')
})

test('manifest 静态内容脚本里的文件存在', () => {
  for (const cs of manifest.content_scripts || []) for (const f of cs.js || []) assert.ok(fs.existsSync(path.join(dir, f)), f)
})

test('其他代码里没有手写的注入文件数组（必须引用 ccFiles）', () => {
  const offenders = []
  for (const f of ['features.js', 'bg-page.js', 'bg-actions.js', 'bg-menus.js', 'bg-trans.js', 'bg-capture.js', 'bg-link.js', 'background.js']) {
    const lines = read(f).split('\n')
    lines.forEach((line, i) => {
      if (/\.js'\s*,\s*'[\w/-]+\.js'/.test(line) && !/importScripts|ccFiles/.test(line) && !/^\s*\/\//.test(line)) offenders.push(`${f}:${i + 1}`)
    })
  }
  assert.deepEqual(offenders, [], '这些行手写了脚本文件数组')
  // 常驻脚本的 js 必须来自 ccFiles.GROUPS
  const ctx2 = vm.createContext({ ccFiles: vm.runInContext('ccFiles', ctx) })
  vm.runInContext(read('features.js'), ctx2)
  const resident = plain(vm.runInContext('RESIDENT_SCRIPTS', ctx2))
  const groups = Object.values(F.GROUPS).map((g) => JSON.stringify(g))
  for (const s of resident) assert.ok(groups.includes(JSON.stringify(s.js)), `${s.id} 的 js 不在 ccFiles.GROUPS 里`)
})

test('每个注入组都满足依赖表的顺序，依赖的文件也在组里', () => {
  for (const [name, g] of Object.entries(F.GROUPS)) {
    g.forEach((f, i) => {
      for (const dep of F.REQUIRES[f] || []) {
        const j = g.indexOf(dep)
        assert.ok(j >= 0, `${name} 组里的 ${f} 依赖 ${dep}，但组里没有它`)
        assert.ok(j < i, `${name} 组里的 ${dep} 必须排在 ${f} 前面`)
      }
    })
  }
})

test('依赖表引用的文件都存在，没有循环依赖', () => {
  for (const [f, deps] of Object.entries(F.REQUIRES)) {
    assert.ok(fs.existsSync(path.join(dir, f)), f)
    for (const d of deps) assert.ok(fs.existsSync(path.join(dir, d)), `${f} 依赖的 ${d} 不存在`)
  }
  const seen = new Map()
  const visit = (f, stack) => {
    if (seen.get(f) === 2) return
    assert.ok(seen.get(f) !== 1, `循环依赖：${[...stack, f].join(' → ')}`)
    seen.set(f, 1)
    for (const d of F.REQUIRES[f] || []) visit(d, [...stack, f])
    seen.set(f, 2)
  }
  Object.keys(F.REQUIRES).forEach((f) => visit(f, []))
})

// 扫描源码核对依赖表：文件里用到了别的文件提供的全局对象（globalThis.ccXxx），依赖表（含传递）里就要有那个文件。
// 只在运行时、用户操作之后才用到的（可选的）写在 LAZY 里，注明原因。
// `globalThis.ccX ||= {}` 是几个文件共用的命名空间（如 ccVsumTabs），不算“提供者”，不检查。
const PROVIDE_RE = /(?:globalThis\.(cc[A-Za-z]+)\s*=(?!=)|^\s*(?:const|var|let)\s+(cc[A-Za-z]+)\s*=)/gm
const USE_RE = /globalThis\.(cc[A-Za-z]+)/g
const LAZY = {
  // 其他功能存在时才联动，没有也能工作
  'content/vdock.js': ['ccSubLayerRun'],
  'content/vsum.js': ['ccSubLayerRun'],
  'content/vsum-transcript.js': ['ccSubLayerRun'],
  'content/vsession.js': ['ccVsum', 'ccVsumSource'], // 用的时候才取（?.），读不到有退路
  'content/vsum-source.js': ['ccVsum'],
  'content/video-control.js': ['ccAudioEngine'], // 音效脚本在时才联动
  'content/selbar.js': ['ccWebTrans', 'ccSel'], // ccWebTrans 联动翻译；ccSel 在 selbar-logic 里，已是硬依赖
  'content/sublayer.js': ['ccVsum', 'ccVsumSource'],
}
test('依赖表没有漏项：源码里用到的全局对象，提供者都在依赖表里', () => {
  const files = new Set()
  for (const g of Object.values(F.GROUPS)) g.forEach((f) => files.add(f))
  const provider = new Map()
  for (const f of files) {
    for (const m of read(f).matchAll(PROVIDE_RE)) provider.set(m[1] || m[2], f)
  }
  const closure = (f, acc = new Set()) => {
    for (const d of F.REQUIRES[f] || []) if (!acc.has(d)) (acc.add(d), closure(d, acc))
    return acc
  }
  const missing = []
  for (const f of files) {
    if (f === 'content/unlock.js') continue
    const need = closure(f)
    const lazy = new Set(LAZY[f] || [])
    for (const m of read(f).matchAll(USE_RE)) {
      const name = m[1]
      const p = provider.get(name)
      if (!p || p === f || need.has(p) || lazy.has(name)) continue
      missing.push(`${f} 用到 ${name}（由 ${p} 提供），依赖表里没有`)
    }
  }
  assert.deepEqual([...new Set(missing)], [])
})

test('后台消息处理函数与入口检查的规则表一一对应', () => {
  const src = read('bg-actions.js')
  const start = src.indexOf('const HANDLERS = {')
  const end = src.indexOf('\n}\n', start)
  const handlers = [...src.slice(start, end).matchAll(/^ {2}'(cc-[\w-]+)':/gm)].map((m) => m[1])
  assert.ok(handlers.length > 30)
  const gctx = vm.createContext({})
  vm.runInContext(read('bg-guard.js'), gctx)
  const spec = Object.keys(plain(vm.runInContext('ccGuard.SPEC', gctx)))
  assert.deepEqual([...handlers].sort(), [...spec].sort())
})
