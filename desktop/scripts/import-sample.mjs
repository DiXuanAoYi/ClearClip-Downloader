// 把“录制样本”（设置里打开后存到数据目录 samples/ 下的 .txt）导入成测试样本。
// 用法：node scripts/import-sample.mjs <样本文件> [名字]
// 结果写到 src-tauri/tests/fixtures/real/：<平台>-<名字>.json|html 和同名的 .meta.json（来源地址、导入时间）。
// 样本保存时已经去掉了 Cookie、令牌和地址里的查询参数；这里再去掉手机号、邮箱、内网地址，
// 然后由 Rust 测试 `real_fixtures_are_clean` 在每次 cargo test 里复查一遍。
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..')
export const realDir = path.join(root, 'src-tauri/tests/fixtures/real')

/** 样本文件：“URL: …\nTIME: …\n\n正文”。 */
export function parseSample(text) {
  const m = /^URL: (.*)\r?\nTIME: (.*)\r?\n\r?\n/.exec(text)
  if (!m) throw new Error('不是“录制样本”保存的文件（开头应是 URL: 和 TIME: 两行）')
  return { url: m[1].trim(), time: m[2].trim(), body: text.slice(m[0].length) }
}

/** 去掉个人信息：手机号、邮箱、内网 IP。返回处理后的文字和各类被替换的个数。 */
export function scrub(body) {
  const counts = { phone: 0, email: 0, ip: 0 }
  let out = body.replace(/(?<![0-9])1[3-9]\d{9}(?![0-9])/g, () => (counts.phone++, '13800000000'))
  out = out.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, () => (counts.email++, 'user@example.com'))
  out = out.replace(/\b(?:10\.\d+\.\d+\.\d+|192\.168\.\d+\.\d+|172\.(?:1[6-9]|2\d|3[01])\.\d+\.\d+)\b/g, () => (counts.ip++, '10.0.0.1'))
  return { text: out, counts }
}

export function kindOf(body) {
  try {
    JSON.parse(body)
    return 'json'
  } catch {
    return 'html'
  }
}

/** 文件名：<平台>-<名字>；没给名字时用样本文件名里去掉时间戳的部分。 */
export function fixtureName(platform, file, name) {
  const base = name || path.basename(file).replace(/\.txt$/i, '').replace(/^\d{8}-\d{6}(\.\d+)?-/, '')
  const clean = base.replace(/[^A-Za-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '')
  if (!clean) throw new Error('名字里没有可用的字母或数字，请换一个')
  return `${platform}-${clean}`
}

export function importSample(file, name, dir = realDir) {
  const { url, time, body } = parseSample(fs.readFileSync(file, 'utf8'))
  const platform = path.basename(path.dirname(path.resolve(file)))
  const { text, counts } = scrub(body)
  const kind = kindOf(text)
  const stem = fixtureName(platform, file, name)
  fs.mkdirSync(dir, { recursive: true })
  const target = path.join(dir, `${stem}.${kind}`)
  fs.writeFileSync(target, kind === 'json' ? JSON.stringify(JSON.parse(text), null, 2) + '\n' : text)
  fs.writeFileSync(path.join(dir, `${stem}.meta.json`), JSON.stringify({ platform, url, recordedAt: time, kind, scrubbed: counts }, null, 2) + '\n')
  return { target, counts }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [file, name] = process.argv.slice(2)
  if (!file) {
    console.error('用法：node scripts/import-sample.mjs <样本文件> [名字]')
    process.exit(2)
  }
  try {
    const { target, counts } = importSample(file, name)
    console.log(`已写入 ${path.relative(process.cwd(), target)}`)
    const n = counts.phone + counts.email + counts.ip
    if (n) console.log(`另外替换了 ${counts.phone} 个手机号、${counts.email} 个邮箱、${counts.ip} 个内网地址`)
    console.log('下一步：在对应平台的解析代码里 include_str! 这个文件，写一个断言解析结果的测试。')
  } catch (e) {
    console.error(e.message)
    process.exit(1)
  }
}
