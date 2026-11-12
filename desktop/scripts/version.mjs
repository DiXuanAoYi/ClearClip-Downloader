#!/usr/bin/env node
// 版本号：一条命令改齐所有地方，CI 用 --check 检查有没有漏改。
//
//   node scripts/version.mjs                       列出各处的版本号并检查（同 --check）
//   node scripts/version.mjs 1.14.1                把程序版本改成 1.14.1
//   node scripts/version.mjs 1.14.1 --ext 1.24.1   同时把浏览器扩展改成 1.24.1
//   node scripts/version.mjs --notes 1.14.1        输出更新记录里这一版的内容（发布流水线用）
//
// 程序版本在这些地方：package.json、package-lock.json、src-tauri/tauri.conf.json、src-tauri/Cargo.toml、
// src-tauri/Cargo.lock、docs/release-guide/store-listing.md；扩展版本在 extension/manifest.json。
// 更新记录最上面开发中的标题（“## 1.14.1（开发中，未发版）”或“## 未发布”）会改成“## 1.14.1（扩展 1.24.1）”。

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const desktop = join(here, '..')
const repo = join(desktop, '..')

const SEMVER = /^\d+\.\d+\.\d+$/
/** 开发中、还没发版的更新记录标题 */
const DEV_HEADING = /开发中|未发布|未发版/

// ---------- 纯函数（单元测试用） ----------

/** 把 JSON 文本里第一个（也就是最外层的）"version" 换掉，保持原来的格式。 */
export function setJsonVersion(text, version) {
  const re = /("version"\s*:\s*")([^"]*)(")/
  if (!re.test(text)) throw new Error('没有找到 "version"')
  return text.replace(re, `$1${version}$3`)
}

export function getJsonVersion(text) {
  return text.match(/"version"\s*:\s*"([^"]*)"/)?.[1] ?? null
}

/** package-lock.json 的根版本有两处：最外层和 packages[""]。 */
export function setLockVersion(text, version) {
  const obj = JSON.parse(text)
  obj.version = version
  if (obj.packages?.['']) obj.packages[''].version = version
  return JSON.stringify(obj, null, 2) + '\n'
}

export function getLockVersions(text) {
  const obj = JSON.parse(text)
  return [obj.version, obj.packages?.['']?.version]
}

/** Cargo.toml 里 [package] 的 version（文件里第一个 version = "…"）。 */
export function setCargoTomlVersion(text, version) {
  const re = /^(version\s*=\s*")([^"]*)(")/m
  if (!re.test(text)) throw new Error('Cargo.toml 里没有 version')
  return text.replace(re, `$1${version}$3`)
}

export function getCargoTomlVersion(text) {
  return text.match(/^version\s*=\s*"([^"]*)"/m)?.[1] ?? null
}

/** Cargo.lock 里本程序（name = "clearclip"）那一条的版本。 */
export function setCargoLockVersion(text, version) {
  const re = /(\[\[package\]\]\r?\nname = "clearclip"\r?\nversion = ")([^"]*)(")/
  if (!re.test(text)) throw new Error('Cargo.lock 里没有 clearclip')
  return text.replace(re, `$1${version}$3`)
}

export function getCargoLockVersion(text) {
  return text.match(/\[\[package\]\]\r?\nname = "clearclip"\r?\nversion = "([^"]*)"/)?.[1] ?? null
}

/** 扩展商店文案里的“当前版本 x.y.z”。 */
export function setListingVersion(text, version) {
  return text.replace(/当前版本 \d+\.\d+\.\d+/g, `当前版本 ${version}`)
}

export function getListingVersion(text) {
  return text.match(/当前版本 (\d+\.\d+\.\d+)/)?.[1] ?? null
}

/** 更新记录最上面的版本标题：{ line, version, ext, dev }。 */
export function topHeading(changelog) {
  const lines = changelog.split('\n')
  const i = lines.findIndex((l) => l.startsWith('## '))
  if (i < 0) return null
  const line = lines[i]
  return {
    index: i,
    line,
    version: line.match(/^## (\d+\.\d+\.\d+)/)?.[1] ?? null,
    ext: line.match(/扩展 (\d+\.\d+\.\d+)/)?.[1] ?? null,
    dev: DEV_HEADING.test(line),
  }
}

/**
 * 发版时改更新记录的标题。最上面是开发中的标题（版本号相同或没写版本）→ 改成正式标题；
 * 已经是这一版的正式标题 → 只更新扩展版本；否则说明这一版还没写更新记录，报错。
 */
export function releaseHeading(changelog, version, ext) {
  const h = topHeading(changelog)
  if (!h) throw new Error('更新记录里没有任何“## ”标题')
  const ok = (h.dev && (h.version === null || h.version === version)) || (!h.dev && h.version === version)
  if (!ok) throw new Error(`更新记录最上面是“${h.line.slice(3)}”，还没有 ${version} 这一版的内容。先在最上面写好“## ${version}（开发中，未发版）”和改动`)
  const lines = changelog.split('\n')
  lines[h.index] = `## ${version}（扩展 ${ext}）`
  return lines.join('\n')
}

/** 某一版的更新记录正文（标题下面到下一个“## ”之前）。 */
export function notesFor(changelog, version) {
  const lines = changelog.split('\n')
  const start = lines.findIndex((l) => l.startsWith('## ') && l.slice(3).startsWith(version) && !/^\d/.test(l.slice(3 + version.length)))
  if (start < 0) return null
  let end = lines.findIndex((l, i) => i > start && l.startsWith('## '))
  if (end < 0) end = lines.length
  return lines.slice(start + 1, end).join('\n').trim()
}

/**
 * 检查各处版本是否一致。files：{ 文件名: 版本 }；changelog 为更新记录全文；ext 为扩展版本。
 * 返回问题列表（空表示没问题）。
 */
export function checkVersions(files, changelog, ext) {
  const problems = []
  const app = files['src-tauri/tauri.conf.json']
  for (const [name, v] of Object.entries(files)) {
    if (v !== app) problems.push(`${name} 是 ${v ?? '（没找到）'}，tauri.conf.json 是 ${app}`)
  }
  const h = topHeading(changelog)
  if (h && !h.dev) {
    if (h.version !== app) problems.push(`更新记录最上面是 ${h.version}，程序版本是 ${app}（开发中的版本请写“## x.y.z（开发中，未发版）”）`)
    if (h.ext && h.ext !== ext) problems.push(`更新记录标题里的扩展版本是 ${h.ext}，extension/manifest.json 是 ${ext}`)
  }
  return problems
}

// ---------- 读写文件 ----------

const FILES = {
  'package.json': [join(desktop, 'package.json'), getJsonVersion, setJsonVersion],
  'package-lock.json': [join(desktop, 'package-lock.json'), (t) => {
    const [a, b] = getLockVersions(t)
    return a === b ? a : `${a} / ${b}`
  }, setLockVersion],
  'src-tauri/tauri.conf.json': [join(desktop, 'src-tauri/tauri.conf.json'), getJsonVersion, setJsonVersion],
  'src-tauri/Cargo.toml': [join(desktop, 'src-tauri/Cargo.toml'), getCargoTomlVersion, setCargoTomlVersion],
  'src-tauri/Cargo.lock': [join(desktop, 'src-tauri/Cargo.lock'), getCargoLockVersion, setCargoLockVersion],
  'docs/release-guide/store-listing.md': [join(repo, 'docs/release-guide/store-listing.md'), getListingVersion, setListingVersion],
}
const MANIFEST = join(desktop, 'extension/manifest.json')
const CHANGELOG = join(desktop, 'CHANGELOG.md')

const read = (p) => readFileSync(p, 'utf8')

function current() {
  const files = {}
  for (const [name, [path, get]] of Object.entries(FILES)) files[name] = get(read(path))
  return { files, ext: getJsonVersion(read(MANIFEST)), changelog: read(CHANGELOG) }
}

function check() {
  const { files, ext, changelog } = current()
  for (const [name, v] of Object.entries(files)) console.log(`${v}\t${name}`)
  console.log(`${ext}\textension/manifest.json（扩展）`)
  console.log(`更新记录最上面：${topHeading(changelog)?.line ?? '（没有）'}`)
  const problems = checkVersions(files, changelog, ext)
  if (problems.length) {
    console.error('\n版本号不一致：')
    for (const p of problems) console.error(`  - ${p}`)
    console.error('\n用 node scripts/version.mjs <版本> [--ext <扩展版本>] 一次改齐。')
    return 1
  }
  console.log('\n各处版本一致。')
  return 0
}

function bump(version, extArg) {
  if (!SEMVER.test(version)) throw new Error(`版本号要写成 1.2.3 这样：${version}`)
  if (extArg && !SEMVER.test(extArg)) throw new Error(`扩展版本号要写成 1.2.3 这样：${extArg}`)
  const ext = extArg ?? getJsonVersion(read(MANIFEST))
  // 先把所有改动算好，有一处出错就一处都不写
  const writes = []
  for (const [name, [path, , set]] of Object.entries(FILES)) writes.push([name, path, set(read(path), version)])
  if (extArg) writes.push(['extension/manifest.json', MANIFEST, setJsonVersion(read(MANIFEST), extArg)])
  writes.push(['CHANGELOG.md', CHANGELOG, releaseHeading(read(CHANGELOG), version, ext)])
  let changed = 0
  for (const [name, path, text] of writes) {
    if (read(path) === text) continue
    writeFileSync(path, text)
    changed++
    console.log(`改了 ${relative(repo, path)}`)
  }
  console.log(changed ? `\n程序 ${version}，扩展 ${ext}。` : '\n已经是这个版本，没有改动。')
}

function main(argv) {
  const args = argv.slice(2)
  if (!args.length || args[0] === '--check') return check()
  if (args[0] === '--notes') {
    const version = args[1] ?? getJsonVersion(read(FILES['src-tauri/tauri.conf.json'][0]))
    const notes = notesFor(read(CHANGELOG), version)
    if (notes == null) {
      console.error(`更新记录里没有 ${version}`)
      return 1
    }
    console.log(notes)
    return 0
  }
  if (args[0] === '-h' || args[0] === '--help') {
    console.log(readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 11).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'))
    return 0
  }
  const i = args.indexOf('--ext')
  bump(args[0], i >= 0 ? args[i + 1] : undefined)
  return 0
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main(process.argv)
  } catch (e) {
    console.error(e.message)
    process.exitCode = 1
  }
}
