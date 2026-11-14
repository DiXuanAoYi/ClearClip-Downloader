// 扩展脚本文件清单：从 extension/files-manifest.js（唯一来源）取，测试脚本不再各写一份文件数组。
const fs = require('fs')
const path = require('path')
const vm = require('vm')

const EXT = path.join(__dirname, '../extension')
const ctx = vm.createContext({})
vm.runInContext(fs.readFileSync(path.join(EXT, 'files-manifest.js'), 'utf8'), ctx, { filename: 'files-manifest.js' })
const F = JSON.parse(JSON.stringify(vm.runInContext('ccFiles', ctx)))

/** 某个注入组的文件（相对 extension/，按加载顺序）。 */
const group = (name) => {
  if (!F.GROUPS[name]) throw new Error(`files-manifest.js 里没有 ${name} 组`)
  return [...F.GROUPS[name]]
}
/** 视频摘要纯逻辑的文件（vsum-logic-*.js + vsum-logic.js，按加载顺序）：单独加载它们就能得到 ccVsum。 */
const vsumLogic = () => group('vsum').filter((f) => /^vsum-logic/.test(f))
module.exports = { group, vsumLogic, EXT, GROUPS: F.GROUPS }
