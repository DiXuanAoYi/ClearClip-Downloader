# 界面测试

不需要启动 Tauri：vite 起几个测试页面（`ui-harness.html` 只挂载剪辑页，`merge-harness.html` 按 `#parse`、`#transfer` 等挂载各个页面），脚本在页面里注入一个假的 Tauri 后端（`invoke` 按命令名分发），再用 Chromium 操作界面。浏览器扩展的脚本把内容脚本加载进真实页面，后台消息用假的代替。

```bash
npm run test:unit                  # e2e/unit/*.test.mjs：纯函数和扩展逻辑，不需要浏览器
npm run test:e2e                   # 起 vite，依次跑默认套件（见 run.mjs）
npm test                           # 两者都跑（CI 跑的就是这个）
node e2e/run.mjs parse-ui.cjs      # 只跑一个或几个脚本
```

需要：

- 本机有 `ffmpeg` 和 `ffprobe`
- Chromium：`npx playwright install chromium`；已有浏览器时用环境变量 `CHROMIUM_PATH` 指向它（所有脚本都认）
- 端口 5199（vite）和 5301（素材服务）空闲

## 默认套件

| 文件 | 内容 |
| --- | --- |
| `ops.cjs` | 剪辑：添加素材菜单、向左 / 向右裁剪、波纹删除、分离音频、剪辑点与逐帧、复制、应用到全部、最近的工程、吸附、缩放 |
| `edit.cjs` | 剪辑：空白状态、添加 / 选择 / 分割 / 排序 / 裁剪、转场、文字、配乐、预览、导出、工程文件、素材丢失、英文界面 |
| `region.cjs` | 剪辑：叠加素材的摆放和拖动、区域（马赛克 / 局部调色 / 跟随聚焦）、手动点、重新追踪 |
| `hdr.cjs` | 剪辑：HDR 素材提示，一键转成普通画面并替换 |
| `subtitle.cjs` | 剪辑：导入字幕文件，每条字幕变成时间线上的一条文字 |
| `proxy.cjs` | 剪辑：系统播放器解不了的素材换成预览用的小文件 |
| `transcript.cjs` | 剪辑：文稿生成、选词、剪掉、找口头禅、按词生成字幕 |
| `keyframes.cjs` | 剪辑：关键帧动画、变速曲线、定格、倒放 |
| `beats.cjs` | 剪辑：绿幕抠像、节拍分析、按节拍切换、配乐适配 |
| `highlights.cjs` | 剪辑：精彩片段 |
| `background.cjs` | 剪辑：换背景（自动识别绿幕、纯色 / 模糊 / 图片） |
| `vocals.cjs` | 剪辑：人声分离 |
| `tracks-ui.cjs` | 剪辑：多轨道、轨道头、调整图层、刀片、标记、混音器、轨道模板 |
| `vsum-panel.cjs` | 扩展：视频摘要面板 |
| `sublayer-session.cjs` | 扩展：摘要面板和字幕层共用字幕和译文、共用词卡 |
| `video-dock.cjs` | 扩展：视频胶囊（下载、播放增强、字幕层、视频摘要的统一入口） |
| `webtrans.cjs` | 扩展：网页翻译 |
| `bili-sub.cjs` | 扩展：B站字幕跨域读取 |
| `audio.cjs` | 扩展：实时音效（用正弦波当视频声音，量处理后的输出） |
| `overview-naming.cjs` | 扩展：页面媒体总览的命名栏 |
| `ext-smoke.cjs` | 扩展：把真正的 `extension/` 加载进 Chromium（真的后台脚本和内容脚本，假的清影后端）：后台脚本启动、整页翻译出现译文、弹窗音效页默认“奥懿”并能存多个自定义预设 |
| `merge-ui.cjs` | 桌面：悬浮窗、摘录 |
| `parse-ui.cjs` | 桌面：解析结果页、合集、创作者主页分栏和列表展开 |
| `home-batch.cjs` | 桌面：主页的单个解析 / 批量下载切换 |
| `batch-ui.cjs` | 桌面：批量添加面板、队列按批次显示 |
| `write-ui.cjs` | 桌面：写作窗 |
| `pdfcanvas.cjs` | 桌面：PDF 版面画布 |
| `filetr-ui.cjs` | 桌面：文件翻译页 |
| `toolbox-ui.cjs` | 桌面：工具箱 |
| `transfer-ui.cjs` | 桌面：手机互传（电脑端） |
| `phone-page.cjs` | 手机网页：配对页、对话流、电脑发来的卡片 |

## 不在默认套件里的

- `ext-real.cjs`：排查整页翻译用的，只打印、不判断对错（有判断的版本是默认套件里的 `ext-smoke.cjs`）。需要显示环境：`xvfb-run -a node e2e/ext-real.cjs`。

## 写新用例

`media/` 里是测试用的小素材（几百 KB，直接提交，不现场生成）。新增界面功能时，在对应脚本里加 `ok('说明', 条件)`；用到新的后端命令，在脚本的假后端（剪辑页在 `lib.cjs` 的 `invoke`）里补一个分支。新脚本要加进 `run.mjs` 的默认列表，否则 CI 不会跑；自己启动浏览器的脚本要写 `chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })`。
