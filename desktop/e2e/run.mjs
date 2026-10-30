// 剪辑界面的浏览器测试：起 vite、依次跑几个测试脚本，任何一个失败就返回非 0。
// 用法：npm run test:e2e   （需要本机有 ffmpeg / ffprobe 和 Chromium，见 e2e/README.md）
import { spawn, spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '..')
const PORT = 5199
const suites = process.argv.slice(2).length ? process.argv.slice(2) : ['ops.cjs', 'edit.cjs', 'region.cjs', 'hdr.cjs', 'subtitle.cjs', 'proxy.cjs', 'transcript.cjs', 'keyframes.cjs', 'beats.cjs']

for (const tool of ['ffmpeg', 'ffprobe']) {
  if (spawnSync(tool, ['-version'], { stdio: 'ignore' }).status !== 0) {
    console.error(`找不到 ${tool}：测试用它读取素材信息，请先安装 ffmpeg。`)
    process.exit(2)
  }
}

const vite = spawn(process.execPath, [path.join(root, 'node_modules/vite/bin/vite.js'), '--port', String(PORT), '--strictPort', '--host', '127.0.0.1'], {
  cwd: root,
  stdio: ['ignore', 'pipe', 'inherit'],
})
const stop = () => vite.kill()
process.on('exit', stop)

async function ready() {
  for (let i = 0; i < 120; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/ui-harness.html`)
      if (r.ok) return
    } catch {}
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error('vite 没有在 60 秒内启动')
}

let failed = 0
try {
  await ready()
  for (const s of suites) {
    console.log(`\n===== ${s} =====`)
    const r = spawnSync(process.execPath, [path.join(here, s)], { cwd: root, stdio: 'inherit', env: { ...process.env, NO_PROXY: '127.0.0.1,localhost', no_proxy: '127.0.0.1,localhost' } })
    if (r.status !== 0) failed++
  }
} catch (e) {
  console.error(e.message)
  failed++
} finally {
  stop()
}
console.log(failed ? `\n${failed} 个测试脚本失败` : '\n全部通过')
process.exit(failed ? 1 : 0)
