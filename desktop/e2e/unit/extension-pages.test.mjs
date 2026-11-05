// 扩展自己的页面（总览、弹窗、高亮管理…）里的脚本共用一个全局作用域：
// 两个脚本顶层各声明一个同名 const/function，页面就会整页报错、什么都不显示（1.4.0 的总览就这样空过）。
// 这里把每个页面的脚本按加载顺序拼成一个脚本编译，重复声明会在编译时暴露。
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const pages = fs.readdirSync(dir).filter((f) => f.endsWith('.html'))

for (const page of pages) {
  test(`页面脚本无重复声明：${page}`, () => {
    const html = fs.readFileSync(path.join(dir, page), 'utf8')
    const srcs = [...html.matchAll(/<script[^>]+src="([^"]+)"/g)].map((m) => m[1]).filter((s) => !/^https?:/.test(s))
    if (!srcs.length) return
    const code = srcs.map((s) => `;/* ${s} */\n${fs.readFileSync(path.join(dir, s), 'utf8')}`).join('\n')
    assert.doesNotThrow(() => new vm.Script(code, { filename: page }), `${page} 的脚本合起来编译失败`)
  })
}
