// scripts/version.mjs：改版本号、检查一致、取某一版的更新记录。
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  checkVersions,
  getCargoLockVersion,
  getCargoTomlVersion,
  getJsonVersion,
  getListingVersion,
  getLockVersions,
  notesFor,
  releaseHeading,
  setCargoLockVersion,
  setCargoTomlVersion,
  setJsonVersion,
  setListingVersion,
  setLockVersion,
  topHeading,
} from '../../scripts/version.mjs'

test('JSON：只改最外层的 version，格式不变', () => {
  const t = '{\n  "name": "clearclip",\n  "private": true,\n  "version": "1.14.0",\n  "dependencies": {\n    "x": { "version": "9.9.9" }\n  }\n}\n'
  const out = setJsonVersion(t, '1.14.1')
  assert.equal(getJsonVersion(out), '1.14.1')
  assert.ok(out.includes('"version": "9.9.9"'), '依赖里的 version 不动')
  assert.equal(out.replace('1.14.1', '1.14.0'), t, '别的地方一个字都不变')
})

test('package-lock：最外层和 packages[""] 两处都改', () => {
  const t = JSON.stringify({ name: 'clearclip', version: '0.6.1', lockfileVersion: 3, packages: { '': { name: 'clearclip', version: '0.6.1' }, 'node_modules/a': { version: '1.0.0' } } }, null, 2) + '\n'
  const out = setLockVersion(t, '1.14.1')
  assert.deepEqual(getLockVersions(out), ['1.14.1', '1.14.1'])
  assert.equal(JSON.parse(out).packages['node_modules/a'].version, '1.0.0')
  assert.ok(out.endsWith('}\n'))
})

test('Cargo.toml 和 Cargo.lock', () => {
  const toml = '[package]\nname = "clearclip"\nversion = "1.14.0"\nedition = "2021"\n\n[dependencies]\nserde = { version = "1" }\n'
  assert.equal(getCargoTomlVersion(setCargoTomlVersion(toml, '1.14.1')), '1.14.1')
  assert.ok(setCargoTomlVersion(toml, '1.14.1').includes('serde = { version = "1" }'))
  const lock = '[[package]]\nname = "clap"\nversion = "4.0.0"\n\n[[package]]\nname = "clearclip"\nversion = "1.14.0"\ndependencies = []\n'
  const out = setCargoLockVersion(lock, '1.14.1')
  assert.equal(getCargoLockVersion(out), '1.14.1')
  assert.ok(out.includes('name = "clap"\nversion = "4.0.0"'), '别的包不动')
  assert.throws(() => setCargoLockVersion('[[package]]\nname = "x"\nversion = "1"\n', '1.0.0'))
})

test('商店文案里的“当前版本”', () => {
  const t = '配套扩展（当前版本 1.14.0）。不含统计'
  assert.equal(getListingVersion(setListingVersion(t, '1.14.1')), '1.14.1')
})

const CHANGELOG = `# 更新记录

版本号与 \`tauri.conf.json\` 一致。

## 1.14.1（开发中，未发版）

### 修复
- 订阅不再跟错列表。

## 1.14.0（扩展 1.24.0）

### 解析
- 分栏。

## 1.13.0（扩展 1.23.0）

- 多轨道。
`

test('更新记录：开发中的标题改成正式标题', () => {
  assert.deepEqual({ ...topHeading(CHANGELOG), index: undefined }, { index: undefined, line: '## 1.14.1（开发中，未发版）', version: '1.14.1', ext: null, dev: true })
  const out = releaseHeading(CHANGELOG, '1.14.1', '1.24.0')
  assert.equal(topHeading(out).line, '## 1.14.1（扩展 1.24.0）')
  assert.equal(topHeading(out).dev, false)
  // 再跑一次（已经是正式标题）：只更新扩展版本
  assert.equal(topHeading(releaseHeading(out, '1.14.1', '1.24.1')).line, '## 1.14.1（扩展 1.24.1）')
  // “## 未发布”这种没写版本的也行
  assert.equal(topHeading(releaseHeading('# x\n\n## 未发布\n\n- a\n', '2.0.0', '3.0.0')).line, '## 2.0.0（扩展 3.0.0）')
})

test('更新记录：这一版还没写就报错，不乱改', () => {
  assert.throws(() => releaseHeading(CHANGELOG, '1.15.0', '1.24.0'), /还没有 1\.15\.0/)
  const released = releaseHeading(CHANGELOG, '1.14.1', '1.24.0')
  assert.throws(() => releaseHeading(released, '1.14.2', '1.24.0'), /还没有 1\.14\.2/)
  assert.throws(() => releaseHeading('# 空\n', '1.0.0', '1.0.0'))
})

test('取某一版的更新记录正文', () => {
  assert.equal(notesFor(CHANGELOG, '1.14.0'), '### 解析\n- 分栏。')
  assert.equal(notesFor(CHANGELOG, '1.13.0'), '- 多轨道。')
  assert.equal(notesFor(CHANGELOG, '1.14.1'), '### 修复\n- 订阅不再跟错列表。')
  assert.equal(notesFor(CHANGELOG, '1.1.0'), null, '1.1.0 不会匹配到 1.14.0 之类')
})

test('检查：各处一致才算过；开发中的更新记录标题不算不一致', () => {
  const files = { 'package.json': '1.14.0', 'src-tauri/tauri.conf.json': '1.14.0', 'src-tauri/Cargo.toml': '1.14.0' }
  assert.deepEqual(checkVersions(files, CHANGELOG, '1.24.0'), [], '最上面是开发中的 1.14.1，程序还是 1.14.0：正常')
  assert.equal(checkVersions({ ...files, 'package.json': '0.6.1' }, CHANGELOG, '1.24.0').length, 1, '漏改一处')
  assert.match(checkVersions(files, '# x\n\n## 1.14.0（扩展 1.23.0）\n', '1.24.0')[0], /扩展版本/)
  assert.match(checkVersions(files, '# x\n\n## 1.13.0（扩展 1.24.0）\n', '1.24.0')[0], /更新记录最上面是 1\.13\.0/)
})
