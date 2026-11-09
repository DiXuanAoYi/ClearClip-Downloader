// 批量发送命名：模板、清理、去重、扩展名。
import assert from 'node:assert/strict'
import fs from 'node:fs'
import vm from 'node:vm'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../extension')
const ctx = vm.createContext({ URL, console })
vm.runInContext(fs.readFileSync(path.join(dir, 'naming.js'), 'utf8'), ctx, { filename: 'naming.js' })
const N = vm.runInContext('ccNaming', ctx)
const plain = (v) => JSON.parse(JSON.stringify(v))

// 清理：非法字符、空白、开头的点、结尾的点和空格、限长
assert.equal(N.cleanName('  a/b\\c:d*e?f  '), 'a b c d e f')
assert.equal(N.cleanName('..隐藏文件. '), '隐藏文件')
assert.equal(N.cleanName('字'.repeat(100), 10), '字'.repeat(10))
assert.equal(N.cleanName(null), '')
assert.equal(N.stripExt('photo.JPG'), 'photo')
assert.equal(N.stripExt('v1.2'), 'v1.2', '版本号不当扩展名（后缀要有字母数字且 2-5 位，这里 .2 只有 1 位）')

// 模板
const date = new Date(2026, 10, 9)
const one = (tpl, o = {}) => N.applyTemplate(tpl, { index: 2, total: 12, title: '东京:夜景', orig: 'IMG_01.jpg', host: 'www.example.com', kind: 'image', date, ...o })
assert.equal(one('{序号}-{标题}'), '03-东京 夜景')
assert.equal(one('{站点}_{日期}_{原名}'), 'example.com_20261109_IMG_01')
assert.equal(one('{类型}{序号}'), '图片03')
assert.equal(one('{未知}-{序号}'), '{未知}-03', '不认识的占位符原样保留')
assert.equal(one('{标题}', { title: '' }), 'IMG_01', '结果为空时退回原名')
assert.equal(N.applyTemplate('{序号}', { index: 0, total: 1234, orig: 'a.png' }), '0001', '序号位数跟总数')

// 去重
assert.deepEqual(plain(N.uniqueNames(['a', 'A', 'b', '', 'a'])), ['a', 'A-2', 'b', '', 'a-3'])

// 批量
const items = [
  { url: 'https://x.com/a/one.jpg?x=1', kind: 'image' },
  { url: 'https://x.com/a/two.jpg', kind: 'image' },
  { url: 'https://x.com/v/clip.mp4', kind: 'video' },
]
assert.deepEqual(plain(N.planNames('{标题}', items, { url: 'https://www.x.com/p', title: '旅行', date })), ['旅行', '旅行-2', '旅行-3'])
assert.deepEqual(plain(N.planNames('{序号}-{原名}', items, { url: 'bad url', title: '', date })), ['01-one', '02-two', '03-clip'])

// 扩展名
assert.equal(N.withExt('封面', 'jpg'), '封面.jpg')
assert.equal(N.withExt('封面.JPG', 'jpg'), '封面.JPG')
assert.equal(N.withExt('封面', ''), '封面')
assert.equal(N.withExt('', 'jpg'), '')
console.log('extension-naming ok')
