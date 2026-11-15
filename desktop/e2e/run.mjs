// 剪辑界面的浏览器测试：起 vite、依次跑几个测试脚本，任何一个失败就返回非 0。
// 用法：npm run test:e2e   （需要本机有 ffmpeg / ffprobe 和 Chromium，见 e2e/README.md）
import { spawn, spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import net from 'node:net'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '..')
const suites = process.argv.slice(2).length ? process.argv.slice(2) : ['ops.cjs', 'edit.cjs', 'region.cjs', 'hdr.cjs', 'subtitle.cjs', 'proxy.cjs', 'transcript.cjs', 'keyframes.cjs', 'beats.cjs', 'highlights.cjs', 'background.cjs', 'vocals.cjs', 'vsum-panel.cjs', 'sublayer-session.cjs', 'video-dock.cjs', 'merge-ui.cjs', 'parse-ui.cjs', 'write-ui.cjs', 'webtrans.cjs', 'bili-sub.cjs', 'batch-ui.cjs', 'audio.cjs', 'pdfcanvas.cjs', 'filetr-ui.cjs', 'toolbox-ui.cjs', 'tracks-ui.cjs', 'groups-ui.cjs', 'meters-ui.cjs', 'compound-ui.cjs', 'home-batch.cjs', 'overview-naming.cjs', 'phone-page.cjs', 'transfer-ui.cjs', 'queue-progress-ui.cjs', 'shell-queue-ui.cjs', 'settings-layout.cjs', 'ui-gallery.cjs', 'p3-ui.cjs', 'ext-smoke.cjs']

for (const tool of ['ffmpeg', 'ffprobe']) {
  if (spawnSync(tool, ['-version'], { stdio: 'ignore' }).status !== 0) {
    console.error(`找不到 ${tool}：测试用它读取素材信息，请先安装 ffmpeg。`)
    process.exit(2)
  }
}

// 端口：默认随机（先绑 0 号端口问系统要一个空闲的），这样并行的 run 和残留的孤儿 vite 都不会互相干扰。
// 想固定端口时设 E2E_PORT / E2E_MEDIA_PORT；vite 用 --strictPort，被占用就直接失败，不会静默连上别人的旧服务。
function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer()
    srv.once('error', reject)
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address()
      srv.close(() => resolve(port))
    })
  })
}
const PORT = Number(process.env.E2E_PORT) || (await freePort())
let MPORT = Number(process.env.E2E_MEDIA_PORT) || (await freePort())
while (MPORT === PORT) MPORT = await freePort()

// 就绪总时限：vite 8（Rolldown）冷启动时依赖预构建在慢机器上可能超过一分钟，默认给 180 秒；可用 E2E_READY_TIMEOUT_MS 调整。
const READY_TIMEOUT_MS = Number(process.env.E2E_READY_TIMEOUT_MS) || 180000
const viteBin = path.join(root, 'node_modules/vite/bin/vite.js')

// 预热：先单独跑一次 `vite optimize`，把 vite.config.ts 里 optimizeDeps.include 列出的依赖预构建进 node_modules/.vite。
// 这样正式起的 dev server 直接复用缓存，页面请求不会卡在“等预构建”，也不会在爬模块图的途中发现新依赖而整体重新优化
// （重新优化会让旧的 ?v=<hash> 依赖地址返回 504 Outdated Optimize Dep）。失败不致命——后面的就绪探测仍然兜底。
{
  const t0 = Date.now()
  const r = spawnSync(process.execPath, [viteBin, 'optimize'], { cwd: root, encoding: 'utf8', timeout: READY_TIMEOUT_MS, env: { ...process.env, NO_COLOR: '1' } })
  const sec = ((Date.now() - t0) / 1000).toFixed(1)
  if (r.status === 0) console.log(`依赖预热完成（${sec} 秒）`)
  else console.error(`依赖预热未成功（${sec} 秒，status=${r.status} signal=${r.signal}），继续由就绪探测兜底。输出尾部：\n${((r.stdout || '') + (r.stderr || '')).slice(-1500)}`)
}

// vite 放在自己的进程组里（detached），清理时整组杀掉，不留孤儿
const vite = spawn(process.execPath, [viteBin, '--port', String(PORT), '--strictPort', '--host', '127.0.0.1'], {
  cwd: root,
  stdio: ['ignore', 'pipe', 'pipe'],
  detached: true,
})
let viteLog = ''
const viteStartedAt = Date.now()
for (const s of [vite.stdout, vite.stderr]) s.on('data', (d) => { viteLog = (viteLog + d).slice(-6000) })
let viteExited = false
let stopping = false
let child = null
vite.on('exit', (code, sig) => {
  viteExited = true
  if (stopping) return
  console.error(`\nvite 意外退出（code=${code} signal=${sig}）。最后的输出：\n${viteLog}`)
  // 正在跑的脚本没有服务可连，直接结束它，整个 run 以失败收场
  child?.kill('SIGKILL')
})
function stop() {
  stopping = true
  child?.kill('SIGKILL')
  if (!viteExited && vite.pid) {
    try { process.kill(-vite.pid, 'SIGKILL') } catch { try { vite.kill('SIGKILL') } catch {} }
  }
}
process.on('exit', stop)
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => { stop(); process.exit(128 + ({ SIGINT: 2, SIGTERM: 15, SIGHUP: 1 })[sig]) })
process.on('uncaughtException', (e) => { console.error(e); stop(); process.exit(1) })
process.on('unhandledRejection', (e) => { console.error(e); stop(); process.exit(1) })

// 就绪探测：取到两个测试页面后，顺着它们的 import 把整张模块图都取一遍，让 vite 先把首屏要用的模块转换（并预构建依赖）好。
// 只取页面会让第一个脚本在满载的机器上等 vite 现编译几十秒；vite 退出则立刻报错。
// 总时限从 vite 启动算起（READY_TIMEOUT_MS），超时时附上最后一次响应和 vite 日志尾部，方便定位是哪一步卡住。
let lastProbe = ''
let staleDeps = false
function timeoutError(url) {
  return new Error(`vite 没有在 ${READY_TIMEOUT_MS / 1000} 秒内就绪（${url}；最后一次探测：${lastProbe || '无响应'}）。vite 日志尾部：\n${viteLog || '（无输出）'}`)
}
async function get(url) {
  for (let i = 0; ; i++) {
    if (viteExited) throw new Error('vite 启动失败：\n' + viteLog)
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}${url}`)
      lastProbe = `${url} -> HTTP ${r.status}`
      if (r.ok) return await r.text()
      if (r.status === 404 && i > 3) return ''
      // 依赖被重新优化后，旧 ?v=<hash> 的地址会一直 504：重试没有意义，记下来由 ready() 重爬一遍拿新地址
      if (r.status === 504 && url.includes('/node_modules/.vite/deps/')) {
        staleDeps = true
        return ''
      }
    } catch (e) {
      lastProbe = `${url} -> ${e.cause?.code || e.message}`
    }
    if (Date.now() - viteStartedAt > READY_TIMEOUT_MS) throw timeoutError(url)
    await new Promise((r) => setTimeout(r, 250))
  }
}
async function crawl() {
  const seen = new Set()
  let queue = ['/ui-harness.html', '/merge-harness.html']
  while (queue.length && seen.size < 3000) {
    const batch = queue.splice(0, 16).filter((u) => !seen.has(u))
    const bodies = await Promise.all(batch.map(async (u) => (seen.add(u), [u, await get(u)])))
    for (const [u, body] of bodies) {
      const re = /(?:from\s*|import\s*\(?\s*|src=)["'](\/[^"']+|\.{1,2}\/[^"']+)["']/g
      for (let m; (m = re.exec(body)); ) {
        const t = new URL(m[1], 'http://x' + u).pathname + new URL(m[1], 'http://x' + u).search
        if (!seen.has(t) && !queue.includes(t) && !/\.(png|jpe?g|svg|gif|webm|ogg|woff2?)(\?|$)/.test(t)) queue.push(t)
      }
    }
  }
  return seen
}
async function ready() {
  // 最多重爬 3 轮：只有中途发生过依赖重新优化（出现过 504）才需要再来
  let seen
  for (let pass = 1; pass <= 3; pass++) {
    staleDeps = false
    seen = await crawl()
    if (!staleDeps) break
    console.log(`依赖在探测途中被重新优化，重新探测（第 ${pass} 轮）`)
  }
  if (staleDeps) throw timeoutError('依赖反复被重新优化')
  // 关键依赖也要确实取到过：@tauri-apps/api/core 是每个页面都要的（探测它正是 CI 上失败的那一步）
  if (![...seen].some((u) => /\/node_modules\/\.vite\/deps\/@tauri-apps_api_core\.js/.test(u))) throw new Error('就绪探测没有取到预构建的 @tauri-apps/api/core，模块图可能不完整。最后探测：' + lastProbe)
  return ((Date.now() - viteStartedAt) / 1000).toFixed(1)
}

function runSuite(s) {
  return new Promise((resolve) => {
    child = spawn(process.execPath, [path.join(here, s)], {
      cwd: root,
      stdio: 'inherit',
      env: { ...process.env, E2E_PORT: String(PORT), E2E_MEDIA_PORT: String(MPORT), NO_PROXY: '127.0.0.1,localhost', no_proxy: '127.0.0.1,localhost' },
    })
    child.on('exit', (code) => resolve(code ?? 1))
    child.on('error', () => resolve(1))
  })
}

let failed = 0
try {
  const sec = await ready()
  console.log(`vite 就绪耗时 ${sec} 秒`)
  console.log(`vite 端口 ${PORT}，素材服务端口 ${MPORT}`)
  for (const s of suites) {
    console.log(`\n===== ${s} =====`)
    if ((await runSuite(s)) !== 0) failed++
    if (viteExited) { failed++; break }
  }
} catch (e) {
  console.error(e.message)
  failed++
} finally {
  stop()
}
console.log(failed ? `\n${failed} 个测试脚本失败` : '\n全部通过')
process.exit(failed ? 1 : 0)
