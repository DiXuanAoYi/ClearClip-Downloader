// 新版解析结果页：两栏（作品 / 下载内容）、清晰度列表、声音二选一、字幕、范围、附带下载、底部摘要和“下载所选”；
// 图集（选图、背景音乐、合成视频）和合集（选条目、订阅更新）；亮色和暗色主题。假后端按命令名回答。
const fs = require('fs')
const path = require('path')
const { chromium } = require('playwright')
const PORT = 5199
let pass = 0
let fail = 0
const ok = (name, cond, extra = '') => {
  if (cond) pass++
  else fail++
  console.log(`${cond ? '  ok ' : 'FAIL '} ${name}${cond ? '' : '  ← ' + extra}`)
}

const settings = JSON.parse(fs.readFileSync(path.join(__dirname, 'merge-settings.json'), 'utf8'))
Object.assign(settings, { downloadDir: 'D:\\下载\\清影', subfolderByPlatform: true, subtitleMode: 'file', qualityPreset: 'best', audioFormat: 'mp3', skipExisting: true, watchClipboard: true })

const A = (o) => ({ url: 'https://cdn.test/' + o.id, ext: 'mp4', width: null, height: null, index: null, protocol: 'http', headers: [], quality: null, vcodec: null, acodec: null, bitrate: null, filesize: null, fps: null, hasAudio: null, pairAudio: null, formatId: null, extra: null, ...o })
const video = {
  platform: 'youtube', platformName: 'YouTube', id: 'abc', sourceUrl: 'https://youtube.test/watch?v=abc', title: 'How Rust achieves memory safety', author: 'Tech Explained', cover: null, durationMs: 1395000, kind: 'video', width: 3840, height: 2160, publishedAt: 1730000000, entries: [], series: null, extractor: 'yt-dlp', chapters: [{ title: '开场', startMs: 0, endMs: 62000 }, { title: '所有权', startMs: 62000, endMs: 480000 }, { title: '借用', startMs: 480000, endMs: 1100000 }, { title: '总结', startMs: 1100000, endMs: 1395000 }],
  assets: [
    A({ id: 'v1', kind: 'video', label: '1080p', width: 1920, height: 1080, vcodec: 'avc1', fps: 30, filesize: 420000000 }),
    A({ id: 'v0', kind: 'video', label: '2160p 4K', width: 3840, height: 2160, vcodec: 'vp9', fps: 60, filesize: 1450000000 }),
    A({ id: 'v2', kind: 'video', label: '720p', width: 1280, height: 720, vcodec: 'avc1', fps: 30, filesize: 210000000 }),
    A({ id: 'a1', kind: 'audio', label: '音频 m4a 128k', ext: 'm4a', bitrate: 128, filesize: 22000000 }),
    A({ id: 'c1', kind: 'cover', label: '封面', ext: 'jpg' }),
    A({ id: 's1', kind: 'subtitle', label: 'English', ext: 'vtt' }),
    A({ id: 's2', kind: 'subtitle', label: '中文（简体）', ext: 'vtt' }),
  ],
}
const images = {
  platform: 'douyin', platformName: '抖音', id: 'img', sourceUrl: 'https://douyin.test/x', title: '秋天的第一组照片', author: '小林的相机', cover: null, durationMs: null, kind: 'images', width: null, height: null, publishedAt: 1730000000, entries: [], series: null, extractor: 'native', chapters: [],
  assets: [...Array(9)].map((_, i) => A({ id: 'i' + i, kind: 'image', label: `图片 ${i + 1}`, ext: 'jpg', index: i, filesize: 1000000 })).concat([A({ id: 'm1', kind: 'audio', label: '背景音乐', ext: 'mp3', filesize: 3100000 })]),
}
const playlist = {
  platform: 'bilibili', platformName: 'B站', id: 'pl', sourceUrl: 'https://bili.test/list', title: 'Rust 合集', author: '编程老张', cover: null, durationMs: null, kind: 'playlist', width: null, height: null, publishedAt: null, series: null, extractor: 'native', chapters: [], assets: [],
  entries: [...Array(10)].map((_, i) => ({ id: 'e' + i, index: i + 1, title: `第 ${i + 1} 集`, url: 'https://x/' + i, durationMs: 600000, thumbnail: null })),
}

const init = (result, dark) => `
(() => {
  const cbs = {}; let cbId = 1; let evId = 1
  window.__calls = []
  window.__result = ${JSON.stringify(result)}
  window.__dark = ${dark}
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) { const id = cbId++; cbs[id] = cb; return id },
    unregisterCallback(id) { delete cbs[id] },
    convertFileSrc(p) { return p },
    async invoke(cmd, args) {
      window.__calls.push({ cmd, args })
      switch (cmd) {
        case 'plugin:event|listen': return evId++
        case 'get_settings': return ${JSON.stringify(settings)}
        case 'preferred_subtitles': return ['s1']
        case 'downloaded_entries': return ['e0', 'e1', 'e2']
        case 'detect_links': return []
        case 'list_history': return []
        case 'enqueue': return { tasks: [{ id: 1 }], alreadyQueued: 0, alreadyDownloaded: 0 }
        case 'enqueue_entries': return args.entryIds.length
        case 'make_slideshow': return '/tmp/x.mp4'
        case 'subs_add': return { id: 1 }
        default: return null
      }
    },
  }
})()
`

;(async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
  const errors = []
  const open = async (result, dark = false) => {
    const page = await browser.newPage({ viewport: { width: 1100, height: 1000 } })
    page.on('pageerror', (e) => errors.push(e.message))
    await page.addInitScript(init(result, dark))
    await page.goto(`http://127.0.0.1:${PORT}/merge-harness.html#parse`)
    await page.waitForTimeout(1200)
    return page
  }
  const lastCall = (page, cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).at(-1)?.args, cmd)
  const sum = (page) => page.locator('.foot .sum').innerText()

  // 1. 视频
  {
    const page = await open(video)
    const work = await page.locator('.work').boundingBox()
    const dl = await page.locator('.dl').boundingBox()
    ok('两栏：左边作品卡、右边下载内容', work && dl && work.x < dl.x && Math.abs(work.y - dl.y) < 2 && work.width <= 321, JSON.stringify({ work, dl }))
    const cover = await page.locator('.cover').boundingBox()
    ok('横屏视频封面按 16:9', cover && Math.abs(cover.width / cover.height - 16 / 9) < 0.05, JSON.stringify(cover))
    ok('封面角标 4K、时长 23:15', (await page.locator('.cover .badge').innerText()) === '4K' && (await page.locator('.cover .dur').innerText()) === '23:15')
    const heads = await page.locator('.dl .pz-st').allInnerTexts()
    ok('区块顺序：清晰度、声音、字幕、范围、附带下载', ['清晰度', '声音', '字幕', '范围', '附带下载'].every((h, i) => heads[i]?.startsWith(h)), heads.join(' | '))
    const rows = await page.locator('.frow').allInnerTexts()
    ok('清晰度一行一种，第一种标“默认”、最高的标“最高”', rows.length === 3 && /默认/.test(rows[0]) && /最高/.test(rows[1]), rows.join(' | '))
    ok('默认选中第一种', (await page.locator('.frow.on').count()) === 1 && /1080p/.test(await page.locator('.frow.on').innerText()))
    ok('偏好语言的字幕默认选中', (await page.locator('.subs .pz-chip.on').count()) === 1)
    ok('底部摘要：视频 + 字幕 + 大小 + 保存位置', /1080p 视频/.test(await sum(page)) && /1 条字幕（单独文件）/.test(await sum(page)) && /约 401 MB/.test(await sum(page)) && /D:\\下载\\清影\\YouTube/.test(await sum(page)), await sum(page))
    // 换清晰度
    await page.locator('.frow', { hasText: '720p' }).click()
    ok('换成 720p：只有一行选中，摘要跟着变', (await page.locator('.frow.on').count()) === 1 && /720p 视频/.test(await sum(page)), await sum(page))
    // 字幕处理方式
    await page.locator('.subs .pz-seg button', { hasText: '内嵌到视频' }).click()
    ok('字幕处理方式改成内嵌，摘要跟着变', /（内嵌到视频）/.test(await sum(page)))
    // 只要音频：换成单独的音频轨，清晰度和字幕变灰
    await page.locator('.pz-seg button', { hasText: '只要音频' }).click()
    await page.waitForTimeout(100)
    ok('只要音频：清晰度不能选、字幕变灰', (await page.locator('.frow:disabled').count()) === 3 && (await page.locator('.subs .pz-chip:disabled').count()) === 2)
    ok('只要音频：摘要是“音频（转 MP3）”，范围那块说明下载完整声音', /音频（转 MP3）/.test(await sum(page)) && /只要音频时下载完整的声音/.test(await page.locator('.dl').innerText()), await sum(page))
    await page.locator('.foot .el-button').click()
    await page.waitForTimeout(200)
    const eq = await lastCall(page, 'enqueue')
    ok('只要音频下载：只下音频轨、转 mp3、不带字幕', eq && JSON.stringify(eq.assetIds) === '["a1"]' && eq.post.extractAudio === 'mp3' && !eq.post.subMode, JSON.stringify(eq))
    await page.locator('.pz-seg button', { hasText: '视频（含声音）' }).click()
    ok('切回视频：恢复刚才选的 720p', /720p/.test(await page.locator('.frow.on').innerText()) && !(await page.locator('.chips .pz-chip.on', { hasText: '音频 m4a' }).count()))
    // 范围：只下载一段 / 选章节
    await page.locator('.clip .pz-seg button', { hasText: '只下载一段' }).click()
    await page.locator('.clip .t input').first().fill('1:00')
    await page.locator('.clip .t input').nth(1).fill('2:00')
    await page.waitForTimeout(100)
    ok('只下载一段：摘要显示时间段', /只下载 1:00–2:00/.test(await sum(page)), await sum(page))
    await page.locator('.clip .bar .seg').nth(1).click()
    await page.waitForTimeout(100)
    ok('点章节色条：切到“选章节”并选中这一章', (await page.locator('.clip .chap.on').innerText()).includes('所有权') && /只下载 1:02–8:00/.test(await sum(page)), await sum(page))
    await page.locator('.chips .pz-chip', { hasText: '封面' }).click()
    await page.locator('.foot .el-button').click()
    await page.waitForTimeout(200)
    const eq2 = await lastCall(page, 'enqueue')
    ok('下载所选：720p + 字幕（内嵌）+ 封面 + 这一章', eq2 && ['v2', 's1', 'c1'].every((id) => eq2.assetIds.includes(id)) && eq2.post.subMode === 'soft' && eq2.post.clip.startMs === 62000 && eq2.post.clip.endMs === 480000, JSON.stringify(eq2))
    // 主按钮颜色：亮色是深一档的橙，白字（先把鼠标移开，免得量到悬停色）
    await page.mouse.move(5, 5)
    await page.waitForTimeout(300)
    const btn = await page.locator('.foot .el-button').evaluate((b) => [getComputedStyle(b).backgroundColor, getComputedStyle(b).color])
    ok('主按钮：#B4561A 底、白字', btn[0] === 'rgb(180, 86, 26)' && btn[1] === 'rgb(255, 255, 255)', btn.join(' '))
    await page.close()
  }

  // 2. 暗色主题
  {
    const page = await open(video, true)
    const c = await page.evaluate(() => {
      const g = (sel, p) => getComputedStyle(document.querySelector(sel))[p]
      return { card: g('.dl', 'backgroundColor'), btn: g('.foot .el-button', 'backgroundColor'), txt: g('.foot .el-button', 'color'), on: g('.frow.on', 'borderTopColor'), foot: g('.foot', 'backgroundColor') }
    })
    ok('暗色：卡片用暗色底、主按钮亮橙配深色字、底栏是暗色', c.card === 'rgb(34, 41, 44)' && c.btn === 'rgb(240, 138, 60)' && c.txt === 'rgb(29, 36, 32)' && c.foot === 'rgb(30, 36, 39)', JSON.stringify(c))
    await page.close()
  }

  // 3. 图集
  {
    const page = await open(images)
    const cover = await page.locator('.cover').boundingBox()
    ok('图集封面框按 4:3（竖图居中不裁切）', cover && Math.abs(cover.width / cover.height - 4 / 3) < 0.05, JSON.stringify(cover))
    ok('图片 5 列、默认全选', (await page.locator('.ph').count()) === 9 && (await page.locator('.ph.sel').count()) === 9)
    const cols = await page.locator('.ph').evaluateAll((els) => new Set(els.slice(0, 6).map((e) => Math.round(e.getBoundingClientRect().top))).size)
    ok('一行 5 张', cols === 2)
    await page.locator('.ph').nth(3).click()
    await page.waitForTimeout(250)
    const dim = await page.locator('.ph').nth(3).locator('img').evaluate((i) => getComputedStyle(i).opacity)
    const head = await page.locator('.dl .pz-st').first().innerText()
    ok('点一张取消：已选 8 / 9，没选的变淡', /已选 8 \/ 9/.test(head) && dim === '0.45', head + ' ' + dim)
    await page.locator('.pz-link', { hasText: '反选' }).click()
    ok('反选：只剩 1 张', (await page.locator('.ph.sel').count()) === 1)
    await page.locator('.pz-link', { hasText: '全选' }).first().click()
    await page.locator('.pz-chip', { hasText: '一起下载' }).click()
    ok('背景音乐“一起下载”：摘要加上背景音乐', /9 张图片 \+ 背景音乐/.test(await sum(page)), await sum(page))
    await page.getByRole('button', { name: '生成视频' }).click()
    await page.waitForTimeout(200)
    const sl = await lastCall(page, 'make_slideshow')
    ok('合成视频直接在页面上：用选中的图片、配背景音乐、每张 3 秒', sl && sl.imageIds.length === 9 && sl.musicId === 'm1' && sl.seconds === 3, JSON.stringify(sl))
    ok('图集没有清晰度、声音、范围', !(await page.locator('.frow').count()) && !/声音/.test((await page.locator('.dl .pz-st').allInnerTexts()).join()))
    await page.close()
  }

  // 4. 合集
  {
    const page = await open(playlist)
    await page.waitForTimeout(300)
    ok('合集：默认只选没下载过的（7 / 10），按钮写数量', /已选 7 \/ 10/.test(await page.locator('.dl').innerText()) && /下载所选 7 个/.test(await page.locator('.foot .el-button').innerText()))
    ok('已下载的标“已下载”', (await page.locator('.entry .pz-tag.ok').count()) === 3)
    ok('“每个视频怎么下”显示默认设置', /按设置（最高清晰度）/.test(await page.locator('.fields').innerText()) && /偏好语言 · 单独文件/.test(await page.locator('.fields').innerText()))
    ok('保存到合集名的文件夹', /D:\\下载\\清影\\B站\\Rust 合集/.test(await sum(page)), await sum(page))
    await page.locator('.foot .el-button').click()
    await page.waitForTimeout(200)
    ok('下载所选：7 个条目', (await lastCall(page, 'enqueue_entries'))?.entryIds.length === 7)
    await page.getByRole('button', { name: '订阅更新' }).click()
    await page.waitForTimeout(200)
    const sub = await lastCall(page, 'subs_add')
    ok('订阅更新：用合集地址和默认设置订阅', sub && sub.url === 'https://bili.test/list' && sub.settings.intervalHours === 6, JSON.stringify(sub))
    await page.close()
  }

  ok('没有页面错误', !errors.length, errors.join(' | '))
  await browser.close()
  console.log(`\n${pass} 通过，${fail} 失败`)
  process.exit(fail ? 1 : 0)
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
