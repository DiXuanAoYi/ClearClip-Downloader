// i18n 检查脚本：多余条目的判断（后端消息不算多余）和 --strict / --max-missing 的退出码。
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { extract, findStale, isExcluded, longestFragment } from '../../scripts/i18n.mjs'

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '../..')
const run = (...args) => spawnSync(process.execPath, ['scripts/i18n.mjs', ...args], { cwd: root, encoding: 'utf8' })

// 条目里最长的固定文字（占位符之间）
assert.equal(longestFragment('已选 {0} / {1} 条'), '已选')
assert.equal(longestFragment('第 {0} 个片段没有素材文件。'), '个片段没有素材文件。')
assert.equal(longestFragment('{0}'), '')

// 多余条目：源码用到的不算；后端（Rust）里有的不算；都没有才算
{
  const en = { 已用到: 'used', 后端消息: 'backend', '后端 {0} 消息': 'backend 2', 没人用: 'dead', '没人用 {0} 的': 'dead 2' }
  const stale = findStale(en, ['已用到'], '这里是 Rust 源码里的 后端消息 和 后端 {} 消息 两句')
  assert.deepEqual(stale.sort(), ['没人用', '没人用 {0} 的'].sort())
}

// 后端按“，”分段拼出来的长句：每一段都能在后端找到才算有引用
{
  const en = { '前缀：{0} 项，其余保持': 'x', '前缀：{0} 项，查无此段': 'y' }
  const stale = findStale(en, [], '前缀： 项， 其余保持 这是后端')
  assert.deepEqual(stale, ['前缀：{0} 项，查无此段'])
}

// --strict：翻译表现在没有多余条目；--max-missing 要是非负整数
{
  assert.equal(run('--strict', '--max-missing=1000000').status, 0)
  assert.equal(run('--strict', '--max-missing=-1').status, 2)
  assert.equal(run('--strict', '--max-missing=abc').status, 2)
  const out = run('--strict', '--max-missing=1000000').stdout
  assert.match(out, /翻译表里多余 0 个/)
  // 不带 --strict 永远不影响退出码
  assert.equal(run().status, 0)
}
// 陈列页（开发用）不扫描；组件库里的真实组件仍然扫描；英文表补齐
{
  assert.ok(isExcluded('/x/src/ui-gallery.ts'))
  assert.ok(isExcluded('C:\\x\\src\\components\\ui\\UiGallery.vue'))
  assert.ok(!isExcluded('/x/src/components/ui/CcField.vue'))
  const all = extract()
  assert.ok(!all.includes('清影组件库'), '陈列页文案不应被扫描')
  assert.ok(all.includes('了解更多'), 'CcField 的文案要被扫描')
  assert.equal(run('--strict', '--max-missing=0').status, 0)
}
console.log('i18n-script ok')
