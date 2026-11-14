// e2e 基础设施：轮询断言 soon / waitForTexts 的行为，以及 run.mjs 随机分配端口、跑完后不留孤儿 vite。
import assert from 'node:assert/strict'
import test from 'node:test'
import { createRequire } from 'node:module'
import { spawn, spawnSync } from 'node:child_process'
import path from 'node:path'
import net from 'node:net'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const { soon, waitForTexts } = createRequire(import.meta.url)('../lib.cjs')

test('soon：条件过一会儿才成立也能通过', async () => {
  const t0 = Date.now()
  const r = await soon(async () => Date.now() - t0 > 200, null, 3000)
  assert.equal(r.pass, true)
})

test('soon：cond 抛错按“还没成立”处理；超时后带上 extra', async () => {
  let n = 0
  const r1 = await soon(async () => {
    if (++n < 3) throw new Error('元素还没出现')
    return true
  }, null, 3000)
  assert.equal(r1.pass, true)
  const r2 = await soon(async () => false, async () => '当时的内容', 300)
  assert.deepEqual(r2, { pass: false, extra: '当时的内容' })
})

test('waitForTexts：先等到有内容再返回，空数组不算读到', async () => {
  let n = 0
  const loc = { page: () => ({ waitForTimeout: (ms) => new Promise((r) => setTimeout(r, ms)) }), allInnerTexts: async () => (++n < 4 ? [] : ['a', 'b']) }
  assert.deepEqual(await waitForTexts(loc), ['a', 'b'])
  const never = { page: loc.page, allInnerTexts: async () => [] }
  await assert.rejects(() => waitForTexts(never, undefined, 200), /等待文字超时/)
})

const hasFfmpeg = ['ffmpeg', 'ffprobe'].every((t) => spawnSync(t, ['-version'], { stdio: 'ignore' }).status === 0)
// 进程被 SIGKILL 后内核释放监听口要几毫秒，所以“已关闭”用轮询等
const closedSoon = async (port) => {
  for (let i = 0; i < 30; i++) {
    if (!(await portOpen(port))) return true
    await new Promise((r) => setTimeout(r, 100))
  }
  return false
}
const portOpen = (port) =>
  new Promise((resolve) => {
    const s = net.connect(port, '127.0.0.1')
    s.once('connect', () => (s.destroy(), resolve(true)))
    s.once('error', () => resolve(false))
  })

test('run.mjs：端口随机，脚本失败时整体失败，结束后 vite 已经退出', { skip: !hasFfmpeg, timeout: 120000 }, async () => {
  const r = spawnSync(process.execPath, [path.join(here, '../run.mjs'), 'no-such-suite.cjs'], { encoding: 'utf8', cwd: path.join(here, '../..') })
  assert.equal(r.status, 1)
  const m = /vite 端口 (\d+)，素材服务端口 (\d+)/.exec(r.stdout)
  assert.ok(m, r.stdout)
  assert.notEqual(m[1], '5199')
  assert.equal(await closedSoon(Number(m[1])), true, 'vite 还在监听：成了孤儿进程')
})

test('run.mjs：被 SIGTERM 杀掉时一并杀掉 vite', { skip: !hasFfmpeg, timeout: 120000 }, async () => {
  // 用一个会一直睡着的“套件”撑住 run，再发 SIGTERM
  const child = spawn(process.execPath, [path.join(here, '../run.mjs'), 'unit/sleep-suite.cjs'], { cwd: path.join(here, '../..'), stdio: ['ignore', 'pipe', 'inherit'] })
  let out = ''
  child.stdout.on('data', (d) => (out += d))
  const exited = new Promise((r) => child.on('exit', (code, sig) => r({ code, sig })))
  for (let i = 0; i < 400 && !/素材服务端口/.test(out) && !/===== unit/.test(out); i++) await new Promise((r) => setTimeout(r, 100))
  const m = /vite 端口 (\d+)/.exec(out)
  assert.ok(m, out)
  assert.equal(await portOpen(Number(m[1])), true)
  child.kill('SIGTERM')
  const e = await exited
  assert.equal(e.code, 143)
  assert.equal(await closedSoon(Number(m[1])), true, 'vite 还在监听：成了孤儿进程')
})
