// 剪辑界面的浏览器测试公共部分（用法见 e2e/README.md）：本地媒体服务器（支持 Range）、假的 Tauri 后端（用真的 ffprobe 读素材）。
const http = require('http')
const fs = require('fs')
const path = require('path')
const { execFileSync, spawnSync } = require('child_process')
const os = require('os')
const { chromium } = require('playwright')

const MEDIA = path.join(__dirname, 'media')
const OUT = path.join(os.tmpdir(), 'clearclip-e2e')
fs.mkdirSync(OUT, { recursive: true })
const MPORT = 5301
const MIME = { '.webm': 'video/webm', '.ogg': 'audio/ogg', '.png': 'image/png', '.jpg': 'image/jpeg', '.gif': 'image/gif' }

function startMediaServer() {
  const srv = http.createServer((req, res) => {
    const u = new URL(req.url, 'http://x')
    const p = u.searchParams.get('p')
    res.setHeader('Access-Control-Allow-Origin', '*')
    if (!p || !(p.startsWith(MEDIA) || p.startsWith(OUT)) || !fs.existsSync(p)) {
      res.statusCode = 404
      return res.end()
    }
    const st = fs.statSync(p)
    const type = MIME[path.extname(p)] || 'application/octet-stream'
    const range = req.headers.range
    if (range) {
      const m = /bytes=(\d*)-(\d*)/.exec(range)
      const s = m[1] ? parseInt(m[1]) : 0
      const e = m[2] ? Math.min(parseInt(m[2]), st.size - 1) : st.size - 1
      res.writeHead(206, { 'Content-Type': type, 'Content-Range': `bytes ${s}-${e}/${st.size}`, 'Accept-Ranges': 'bytes', 'Content-Length': e - s + 1 })
      fs.createReadStream(p, { start: s, end: e }).pipe(res)
    } else {
      res.writeHead(200, { 'Content-Type': type, 'Accept-Ranges': 'bytes', 'Content-Length': st.size })
      fs.createReadStream(p).pipe(res)
    }
  })
  return new Promise((r) => srv.listen(MPORT, '127.0.0.1', () => r(srv)))
}
const urlOf = (p) => `http://127.0.0.1:${MPORT}/m?p=${encodeURIComponent(p)}`

function probe(file) {
  if (!fs.existsSync(file)) throw new Error('找不到文件。')
  const j = JSON.parse(execFileSync('ffprobe', ['-v', 'error', '-print_format', 'json', '-show_streams', '-show_format', file]).toString())
  const v = j.streams.find((s) => s.codec_type === 'video')
  const a = j.streams.find((s) => s.codec_type === 'audio')
  const ext = path.extname(file).toLowerCase()
  const dur = parseFloat(j.format.duration || '0')
  // 和后端一致：GIF 超过 0.2 秒算视频，其余图片格式算图片
  const isImg = ['.png', '.jpg'].includes(ext) || (ext === '.gif' && !(dur > 0.2))
  const kind = isImg ? 'image' : v ? 'video' : 'audio'
  let thumb = null
  if (v) {
    thumb = path.join(OUT, 'thumb-' + path.basename(file) + '.jpg')
    spawnSync('ffmpeg', ['-v', 'error', '-y', '-i', file, '-frames:v', '1', '-vf', 'scale=-2:144', thumb])
  }
  const [num, den] = (v?.r_frame_rate || '0/1').split('/').map(Number)
  return {
    path: file,
    name: path.basename(file),
    kind,
    durationMs: isImg ? null : Math.round(dur * 1000),
    width: v ? v.width : 0,
    height: v ? v.height : 0,
    fps: v && den ? num / den : null,
    hasVideo: !!v,
    hasAudio: !!a,
    hdr: (globalThis.__hdrFiles || []).includes(file),
    thumb,
    url: (globalThis.__badFiles || []).includes(file) ? `http://127.0.0.1:${MPORT}/bad?p=none` : urlOf(file),
  }
}

/** 注入到页面里的假 Tauri：invoke 按命令名分发，事件可以由测试触发。 */
function initScript(mport) {
  return `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  const listeners = {}
  window.__calls = []
  window.__dialogOpen = []; window.__dialogSave = []
  window.__files = {}      // 工程文件（edit_save / edit_open）
  window.__canceled = {}; window.__hold = false; window.__followReqs = []; window.__detectReqs = []
  window.__emit = (event, payload) => { for (const h of listeners[event] || []) cbs[h]?.({ event, id: 0, payload }) }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return 'http://127.0.0.1:${mport}/m?p=' + encodeURIComponent(p) },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': (listeners[args.event] ||= []).push(args.handler); return evId++
        case 'plugin:event|unlisten': return null
        case 'plugin:dialog|open': { const r = window.__dialogOpen.shift(); return r === undefined ? null : r }
        case 'plugin:dialog|save': { const r = window.__dialogSave.shift(); return r === undefined ? null : r }
        case 'edit_probe': return await window.__probe(args.path)
        case 'edit_thumb': return null
        case 'edit_subtitle_cues': return window.__cues || []
        case 'edit_stt_status': return window.__stt || { ready: true, engine: 'local', reason: '' }
        case 'edit_transcribe': (window.__trReqs ||= []).push(args.req); return window.__transcript
        case 'edit_beats': (window.__beatReqs ||= []).push(args); return window.__beats
        case 'edit_proxy': window.__proxied = (window.__proxied || 0) + 1; return window.__proxyUrl
        case 'edit_missing': return args.paths.filter((p) => !window.__exists(p))
        case 'edit_save': window.__files[args.path] = JSON.parse(JSON.stringify(args.project)); return null
        case 'edit_open': {
          const project = window.__files[args.path]
          if (!project) throw { kind: 'not_found', message: '找不到工程文件。' }
          const paths = [...project.clips.map((c) => c.path), ...(project.overlays || []).map((o) => o.path), ...project.audio.map((a) => a.path)]
          return { project, missing: paths.filter((p) => !window.__exists(p)) }
        }
        case 'edit_preview_start': window.__previewProject = args.project; return 77
        case 'edit_preview_url': return window.__previewUrl
        case 'media_job_start': window.__exported = args.job; return 88
        case 'media_job_cancel': return null
        case 'video_presets': return [{ id: 'compat', name: '通用兼容', desc: '', spec: { size: 'limit', width: 1920, height: 1080, shortSide: 1080, followOrientation: true, fps: 'auto', fpsValue: 30, hdr: true, fixColor: true, levels: 0, outRange: 'tv', denoise: 'off', matchColor: 0, matchTone: {}, matchShots: [], matchSkip: [], lut: null, lutStrength: 1, autocrop: false, audioRate: 0, audioChannels: 0, loudness: null, fixSync: true, codec: 'h264', quality: 'balanced' } }]
        case 'track_detect': window.__detectReqs.push(args); return await window.__trackDetect(args.path, args.atMs)
        case 'track_cancel': window.__canceled[args.id] = true; return null
        case 'track_follow': {
          const req = args.req
          window.__followReqs.push(req)
          window.__emit('edit://track', { id: req.id, percent: 20 })
          for (let i = 0; i < 400 && window.__hold && !window.__canceled[req.id]; i++) await new Promise((r) => setTimeout(r, 25))
          if (window.__canceled[req.id]) throw { kind: 'invalid', message: 'canceled' }
          const res = await window.__trackFollow(req)
          window.__emit('edit://track', { id: req.id, percent: 100 })
          return res
        }
        default: return null
      }
    },
  }
})()
`
}

async function launch() {
  const srv = await startMediaServer()
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--autoplay-policy=no-user-gesture-required'] })
  const ctx = await browser.newContext({ viewport: { width: 1480, height: 1000 } })
  const page = await ctx.newPage()
  const errors = []
  page.on('pageerror', (e) => errors.push('pageerror: ' + e.message))
  page.on('console', (m) => {
    if (m.type() === 'error' || m.type() === 'warning') errors.push(m.type() + ': ' + m.text() + ' @' + (m.location().url || '').slice(0, 120))
  })
  await page.exposeFunction('__probe', (p) => {
    try {
      return probe(p)
    } catch (e) {
      throw { kind: 'invalid', message: String(e.message || e) }
    }
  })
  await page.exposeFunction('__exists', (p) => fs.existsSync(p))
  await page.exposeFunction('__trackDetect', (p, at) => (globalThis.__detectImpl ? globalThis.__detectImpl(p, at) : { candidates: [], camera: 0, note: null }))
  await page.exposeFunction('__trackFollow', (req) => (globalThis.__followImpl ? globalThis.__followImpl(req) : { points: [], lostMs: 0, note: null }))
  await page.addInitScript(initScript(MPORT))
  return { srv, browser, page, errors }
}

const m = (f) => path.join(MEDIA, f)

/** 打开只含剪辑页的测试页面；等不到时把页面上的报错和内容打印出来，方便看 CI 日志。 */
async function openHarness(page, errors) {
  await page.goto('http://127.0.0.1:5199/ui-harness.html')
  try {
    await page.waitForSelector('.ed', { timeout: 60000 })
  } catch (e) {
    console.error('剪辑页没有出现。页面报错：\n' + errors.slice(0, 15).join('\n'))
    console.error('页面内容：' + (await page.locator('body').innerText().catch(() => '')).slice(0, 500))
    throw e
  }
}

/** 工具栏的“添加素材”：主轨就点按钮本身，其余在箭头菜单里。 */
async function addMenu(page, item) {
  const dd = page.locator('.bar .el-dropdown', { hasText: '添加素材' })
  if (item === '添加到主轨…') return dd.locator('.el-button-group > .el-button').first().click()
  await dd.locator('.el-dropdown__caret-button').click()
  await page.getByRole('menuitem', { name: item, exact: true }).click()
}

/** 取一个像素的 RGB（用 ffmpeg 解码截图）。 */
function pixel(buf, x, y) {
  const f = path.join(OUT, 'px.png')
  fs.writeFileSync(f, buf)
  const out = execFileSync('ffmpeg', ['-v', 'error', '-i', f, '-vf', `crop=1:1:${x}:${y}`, '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-'])
  return [out[0], out[1], out[2]]
}

module.exports = { launch, openHarness, m, pixel, OUT, MEDIA, urlOf, addMenu }
