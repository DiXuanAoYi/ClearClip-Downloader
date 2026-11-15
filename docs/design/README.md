# 清影界面设计稿（归档索引）

这里按类别保存了清影 ClearClip 的全部界面设计图，方便对照程序里的实现。所有设计图来自 Claude 设计画布里的 artifact（`.dc.html` 页面），**导出日期 2026-11-15**；主画布“清影界面设计”导出的是线上最新版（含所有者 11-15 在画布里做的 4 次编辑，共 15 个画板与旧仓库版本不同，已用线上版覆盖）。

- 主画布：<https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa>（artifact 列表里同一画布显示为 <https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP>，两个地址读到的是同一个版本）。
- 实现基线：桌面端 1.16.0（扩展 1.25.0）。“已实现 / 未实现”只据代码和 `desktop/CHANGELOG.md` 判断，判断不了的标“待核对”。

## 怎么打开 `.dc.html`

`.dc.html` 是设计画布的页面源文件（普通 HTML 加 `<x-dc>` 标记），一个文件一张画板。

1. **直接用浏览器打开**（拖进 Chrome / Edge 即可）：版式、颜色、字体、图标都会显示，样式靠 `_shared/` 里的 CSS，路径已在归档时改成相对路径。**限制**：文件头引用的 `./support.js`（画布运行时）不在仓库里，所以画板里用 `{{...}}` 填充的数据和 `<sc-for>` 循环的列表（例如解析页的清晰度卡片、下载方案里的数字）会显示成原始占位符或空着。已用 headless Chrome 打开 `02-主界面各页/解析.dc.html` 确认过这一点；其余画板未逐个验证。
2. **看渲染图**：`07-渲染图/*.png`（深色，1360 宽），或用浏览器打开 `docs/design-reference.html` 一页看完。注意这些 png 是旧版本，没有包含 11-15 的 4 次画布编辑。
3. **回到画布**：每行“来源”里有 artifact 链接，在 Claude 设计画布里能完整交互、切换深浅色。
4. 没有 `support.js` 时想看完整数据，可以直接读 `.dc.html` 文本：占位符 `{{x}}` 的取值写在文件末尾 `<script type="text/x-dc">` 的 `renderVals()` 里。

## 归档时做的改动（只有这几项，其余文件与画布导出内容逐字节一致）

- 旧仓库的 `docs/design/source/`、`png/` 用 `git mv` 移进对应类别，画板文件按语义改名（原画布键名记在“来源”列，`_shared/清影界面设计.canvas.json` 里仍是旧键名）。
- `<link rel="stylesheet" href="clearclip.css">` 与 `v2.css` 的路径改成指向 `_shared/` 的相对路径。
- 画板里引用的截图（artifact 的 `/_blob/<id>` 素材）下载后放在所在目录的 `assets/`，引用改成 `assets/<id>.png`。
- 其它 artifact 的画板保留原文件名（画板之间用 `<dc-import name="…">` 按名字互相引用，改名会断），每个 artifact 一个文件夹，文件夹里的 `canvas.json` 是该 artifact 的画布索引。
- 画板里的 Google Fonts 链接保持原样（离线时回退到系统字体）。

## 与旧版 README 的差异

旧版 README 写“设计系统 v2（待确认，尚未实现）”。按 `desktop/CHANGELOG.md` 1.16.0，设计令牌、统一组件和 V2 里的多数页面改版已经上线，所以 V2 各稿现在标为“已在 1.16.0 实现（逐项细节待核对）”。生成这些画板的 Python 脚本仍在 `gen/`，它们写入的是一个 `../project/` 目录（作者的工作目录），不是这里的任何子目录。


## 目录与文件数

| 目录 | 内容 | 文件数 |
|---|---|---|
| `01-设计系统与令牌/` | 设计令牌（度量、动效、色彩）、组件（Cc*）、硬编码色清理。 | 6 |
| `02-主界面各页/` | 解析、下载队列、订阅、直播、屏幕翻译、媒体库、文件翻译、生词与摘录、工具箱（含剪辑）和设置 13 个分类。 | 24 |
| `03-子窗口与浮层/` | 浮层与小窗（悬浮窗、迷你窗、截图译文窗、命令面板、锁屏）、小窗口与悬浮条、清影写作窗。 | 7 |
| `04-手机互传与手机互聊/` | 电脑端与手机网页的互传、互聊，含两次重设计的历史稿。 | 14 |
| `05-浏览器扩展/` | 扩展弹窗、设置页、页内浮层、字幕层、视频摘要面板。 | 12 |
| `06-专题设计稿/` | 针对某个功能 / 问题的专题稿：设计系统 v2 的页面改版稿、解析结果页重构、剪辑多轨、文件翻译、工具箱文件篮、批量下载、术语分层等。 | 62 |
| `07-渲染图/` | 主画布各画板的深色渲染图（仓库原有，比画板旧）。 | 50 |
| `_shared/` | 画板用到的 CSS 与主画布的 canvas.json。 | 3 |
| `gen/` | 生成旧画板 / V2 画板的 Python 脚本（原位保留）。 | 11 |
| 合计（不含本 README） | | 189 |

## 逐文件索引

列：文件 / 标题（取自画布里的标题）/ 对应的程序页面或功能 / 来源（artifact 链接与导出日期）/ 实现状态（只据代码和 `desktop/CHANGELOG.md` 判断，判断不了标“待核对”）。

### 01 设计系统与令牌

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `01-设计系统与令牌/V2-设计系统-度量与动效.dc.html` | 设计系统 · 度量与动效 | 设计令牌：字号 / 圆角 / 间距 / 阴影 / 动效 / z-index / 焦点环（desktop/src/styles/theme.css） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-DS-1.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `01-设计系统与令牌/V2-设计系统-色彩与令牌清单.dc.html` | 设计系统 · 色彩与令牌清单 | 设计令牌：色彩与完整令牌清单（theme.css） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-DS-2.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `01-设计系统与令牌/V2-组件-按钮分段链接选择.dc.html` | 组件 · 按钮 分段 链接 选择 | 组件 CcButton / CcSegmented / CcLink / CcSelect（desktop/src/components/ui/） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-CMP-1.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `01-设计系统与令牌/V2-组件-字段空状态骨架对话框提示.dc.html` | 组件 · 字段 空状态 骨架 对话框 提示 | 组件 CcField / CcSwitch / CcEmpty / CcSkeleton / CcDialog / CcToast（desktop/src/components/ui/） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-CMP-2.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `01-设计系统与令牌/V2-C12-硬编码色对照.dc.html` | C12 硬编码色对照 | 硬编码色清理对照表与 lint 规则（npm run lint:colors） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C12.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `01-设计系统与令牌/component-api.md` | 组件 API 一览（CcButton 等，desktop/src/components/ui/） | 组件库说明（文档） | 仓库原有（docs/design/component-api.md，git mv 移入） | 已实现（随 1.16.0 组件库） |

### 02 主界面各页

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `02-主界面各页/解析.dc.html` | 解析 | 解析页（ParseView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Main.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/解析-列表与批量.dc.html` | 解析 · 列表与批量 | 解析页：列表 / 批量模式 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `ParseList.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/下载队列.dc.html` | 下载队列 | 下载队列（QueueView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Queue.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；队列行图标按钮、“更多”菜单见 1.16.0 |
| `02-主界面各页/订阅.dc.html` | 订阅 | 订阅（SubsView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Subs.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/直播.dc.html` | 直播 | 直播（LiveView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Live.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/屏幕翻译.dc.html` | 屏幕翻译 | 屏幕翻译（ScreenView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Screen.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/媒体库.dc.html` | 媒体库 | 媒体库（LibraryView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Library.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；筛选弹层、空 / 加载 / 失败重试态见 1.16.0 |
| `02-主界面各页/文件翻译.dc.html` | 文件翻译 | 文件翻译（FileTransView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `FileTrans.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/生词与摘录.dc.html` | 生词与摘录 | 生词与摘录（VocabView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Vocab.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/工具箱.dc.html` | 工具箱 | 工具箱首页（ToolboxView） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Toolbox.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；“常用工具”快捷卡见 1.16.0 |
| `02-主界面各页/工具箱-剪辑.dc.html` | 工具箱 · 剪辑 | 工具箱 › 剪辑（时间线） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Editor.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `02-主界面各页/设置/设置-下载.dc.html` | 设置 · 下载 | 设置 › 下载 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-解析.dc.html` | 设置 · 解析 | 设置 › 解析 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-parse.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-BT与ed2k.dc.html` | 设置 · BT 与 ed2k | 设置 › BT 与 ed2k | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-p2p.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-网络.dc.html` | 设置 · 网络 | 设置 › 网络 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-network.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-账号与Cookie.dc.html` | 设置 · 账号与 Cookie | 设置 › 账号与Cookie | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-accounts.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-手机与扩展.dc.html` | 设置 · 手机与扩展 | 设置 › 手机与扩展 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-phone.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-自动化.dc.html` | 设置 · 自动化 | 设置 › 自动化 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-auto.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-AI.dc.html` | 设置 · AI | 设置 › AI | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-ai.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-写作.dc.html` | 设置 · 写作 | 设置 › 写作 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-writing.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-安全与隐私.dc.html` | 设置 · 安全与隐私 | 设置 › 安全与隐私 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-security.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-组件.dc.html` | 设置 · 组件 | 设置 › 组件 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-components.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-诊断.dc.html` | 设置 · 诊断 | 设置 › 诊断 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-diag.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |
| `02-主界面各页/设置/设置-通用.dc.html` | 设置 · 通用 | 设置 › 通用 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Settings-general.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；设置页重做（即时保存、长说明折叠、快捷键录入）见 1.16.0 |

### 03 子窗口与浮层

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `03-子窗口与浮层/浮层与小窗.dc.html` | 浮层与小窗 | 悬浮窗 / 迷你窗 / 截图译文窗 / 命令面板 / 锁屏等浮层 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Overlays.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」）；悬浮窗、迷你窗、截图译文窗同步改版见 1.16.0；命令面板、锁屏的细节待核对 |
| `03-子窗口与浮层/小窗口与悬浮条.dc.html` | 小窗口与悬浮条 | 小窗口与悬浮条 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Small.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `03-子窗口与浮层/清影写作.dc.html` | 清影写作 | 清影写作窗（WriteApp） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Write.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `03-子窗口与浮层/清影写作-帮我回/Dark.dc.html` | 帮我回 · 暗色（问清楚） | 清影写作窗：“帮我回”标签 | [清影写作 · 帮我回](https://claude.ai/artifact/Xogg9ZkWFxFxenzuu2TiUa)，导出 2026-11-15 | 已在 1.10.0 实现（CHANGELOG 1.10.0「清影写作：帮我回」） |
| `03-子窗口与浮层/清影写作-帮我回/Main.dc.html` | 帮我回 · 亮色（有条件答应） | 清影写作窗：“帮我回”标签 | [清影写作 · 帮我回](https://claude.ai/artifact/Xogg9ZkWFxFxenzuu2TiUa)，导出 2026-11-15 | 已在 1.10.0 实现（CHANGELOG 1.10.0「清影写作：帮我回」） |
| `03-子窗口与浮层/清影写作-帮我回/Spec.dc.html` | 规则与重组 | 清影写作窗：“帮我回”标签 | [清影写作 · 帮我回](https://claude.ai/artifact/Xogg9ZkWFxFxenzuu2TiUa)，导出 2026-11-15 | 已在 1.10.0 实现（CHANGELOG 1.10.0「清影写作：帮我回」） |
| `03-子窗口与浮层/清影写作-帮我回/canvas.json` | 画布索引：清影写作 · 帮我回 | 清影写作窗：“帮我回”标签 | [清影写作 · 帮我回](https://claude.ai/artifact/Xogg9ZkWFxFxenzuu2TiUa)，导出 2026-11-15 | —（画布布局，非设计图） |

### 04 手机互传与手机互聊

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `04-手机互传与手机互聊/手机互传.dc.html` | 手机互传 | 手机互传（TransferPanel + 手机网页） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Phone.dc.html` | 已在 1.16.0 实现（新主题“冷钢深空 + 镜头琥珀”，CHANGELOG 1.16.0「界面」） |
| `04-手机互传与手机互聊/手机互聊.dc.html` | 手机互聊 | 手机互聊（默认关闭） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `Chat.dc.html` | 已在 1.16.0 实现（CHANGELOG 1.16.0「手机互传」：新增手机互聊，默认关闭） |
| `04-手机互传与手机互聊/手机互传重设计/Main.dc.html` | 电脑端 · 媒体库 → 手机互传 | 手机互传（电脑端传输流 + 手机网页） | [手机互传重设计](https://claude.ai/artifact/F6e9fJYvm59pw9cATEyDhz)，导出 2026-11-15 | 已实现（1.11.0「手机互传（重设计）」）；其后 1.12.0 / 1.16.0 又改版，见第二版与主画布 |
| `04-手机互传与手机互聊/手机互传重设计/PhoneGet.dc.html` | 手机端 · 电脑发来的 | 手机互传（电脑端传输流 + 手机网页） | [手机互传重设计](https://claude.ai/artifact/F6e9fJYvm59pw9cATEyDhz)，导出 2026-11-15 | 已实现（1.11.0「手机互传（重设计）」）；其后 1.12.0 / 1.16.0 又改版，见第二版与主画布 |
| `04-手机互传与手机互聊/手机互传重设计/PhonePair.dc.html` | 手机端 · 首次配对 / 选择内容 | 手机互传（电脑端传输流 + 手机网页） | [手机互传重设计](https://claude.ai/artifact/F6e9fJYvm59pw9cATEyDhz)，导出 2026-11-15 | 已实现（1.11.0「手机互传（重设计）」）；其后 1.12.0 / 1.16.0 又改版，见第二版与主画布 |
| `04-手机互传与手机互聊/手机互传重设计/PhoneSend.dc.html` | 手机端 · 发送（对话流） | 手机互传（电脑端传输流 + 手机网页） | [手机互传重设计](https://claude.ai/artifact/F6e9fJYvm59pw9cATEyDhz)，导出 2026-11-15 | 已实现（1.11.0「手机互传（重设计）」）；其后 1.12.0 / 1.16.0 又改版，见第二版与主画布 |
| `04-手机互传与手机互聊/手机互传重设计/canvas.json` | 画布索引：手机互传重设计 | 手机互传（电脑端传输流 + 手机网页） | [手机互传重设计](https://claude.ai/artifact/F6e9fJYvm59pw9cATEyDhz)，导出 2026-11-15 | —（画布布局，非设计图） |
| `04-手机互传与手机互聊/手机互传第二版设计/Desk.dc.html` | 电脑端 · 亮色 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | 已实现（1.12.0「手机互传（第二版）」）；1.16.0 的主画布为最新稿 |
| `04-手机互传与手机互聊/手机互传第二版设计/DeskDark.dc.html` | 电脑端 · 暗色 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | 已实现（1.12.0「手机互传（第二版）」）；1.16.0 的主画布为最新稿 |
| `04-手机互传与手机互聊/手机互传第二版设计/Phone.dc.html` | 手机端 · 发送 · 亮色 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | 已实现（1.12.0「手机互传（第二版）」）；1.16.0 的主画布为最新稿 |
| `04-手机互传与手机互聊/手机互传第二版设计/PhoneGetDark.dc.html` | 手机端 · 电脑发来的 · 暗色 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | 已实现（1.12.0「手机互传（第二版）」）；1.16.0 的主画布为最新稿 |
| `04-手机互传与手机互聊/手机互传第二版设计/PhoneGetLight.dc.html` | 手机端 · 电脑发来的 · 亮色 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | 已实现（1.12.0「手机互传（第二版）」）；1.16.0 的主画布为最新稿 |
| `04-手机互传与手机互聊/手机互传第二版设计/PhoneSendDark.dc.html` | 手机端 · 发送 · 暗色 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | 已实现（1.12.0「手机互传（第二版）」）；1.16.0 的主画布为最新稿 |
| `04-手机互传与手机互聊/手机互传第二版设计/canvas.json` | 画布索引：手机互传 第二版设计 | 手机互传（暗色、同一资源一条、发送对象可勾选、图片缩略图） | [手机互传 第二版设计](https://claude.ai/artifact/7kqYV21HYoXVbATqwyTJA6)，导出 2026-11-15 | —（画布布局，非设计图） |

### 05 浏览器扩展

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `05-浏览器扩展/扩展-弹窗.dc.html` | 扩展 · 弹窗 | 浏览器扩展：弹窗 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `ExtPopup.dc.html` | 已在 1.16.0 实现（扩展 1.25.0 新主题） |
| `05-浏览器扩展/扩展-设置.dc.html` | 扩展 · 设置（横向导航） | 浏览器扩展：设置页（顶部横向导航） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `ExtSettings.dc.html` | 已在 1.16.0 实现（扩展 1.25.0：设置页改成顶部横向导航） |
| `05-浏览器扩展/扩展-页面内浮层.dc.html` | 扩展 · 页面内浮层 | 浏览器扩展：页面内浮层（视频胶囊 / 字幕层 / 划词 / 摘要面板 / 页面媒体总览 / 截图翻译） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `ExtPage.dc.html` | 已在 1.16.0 实现（扩展 1.25.0 新主题） |
| `05-浏览器扩展/字幕层重设计/Main.dc.html` | 视频字幕层 · 播放器内嵌胶囊 | 浏览器扩展：视频字幕层（播放器内嵌胶囊） | [字幕层重设计](https://claude.ai/artifact/GG2EoaTAcPiZm28ydX1NyK)，导出 2026-11-15 | 已实现（1.9.0「扩展：字幕层和视频摘要文稿」）；1.25.0 又换新主题 |
| `05-浏览器扩展/字幕层重设计/canvas.json` | 画布索引：字幕层重设计 | 浏览器扩展：视频字幕层（播放器内嵌胶囊） | [字幕层重设计](https://claude.ai/artifact/GG2EoaTAcPiZm28ydX1NyK)，导出 2026-11-15 | —（画布布局，非设计图） |
| `05-浏览器扩展/视频摘要-收到右边/Collapsed.dc.html` | ② 收起：只剩右边一个小标签 | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | 已实现（1.10.0「扩展：视频摘要面板收到右边」） |
| `05-浏览器扩展/视频摘要-收到右边/Dark.dc.html` | ④ 暗色：展开 | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | 已实现（1.10.0「扩展：视频摘要面板收到右边」） |
| `05-浏览器扩展/视频摘要-收到右边/Main.dc.html` | ① 展开：多一个“收起”按钮 | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | 已实现（1.10.0「扩展：视频摘要面板收到右边」） |
| `05-浏览器扩展/视频摘要-收到右边/Peek.dc.html` | ③ 预览：鼠标停在标签上，面板滑出来 | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | 已实现（1.10.0「扩展：视频摘要面板收到右边」） |
| `05-浏览器扩展/视频摘要-收到右边/Spec.dc.html` | ⑥ 规则 | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | 已实现（1.10.0「扩展：视频摘要面板收到右边」） |
| `05-浏览器扩展/视频摘要-收到右边/Tab.dc.html` | ⑤ 小标签的各种状态（亮 / 暗） | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | 已实现（1.10.0「扩展：视频摘要面板收到右边」） |
| `05-浏览器扩展/视频摘要-收到右边/canvas.json` | 画布索引：清影视频摘要 · 收到右边 | 浏览器扩展：视频摘要面板（收起 / 预览 / 小标签） | [清影视频摘要 · 收到右边](https://claude.ai/artifact/YaLM9aJC86JtjeNVLres6F)，导出 2026-11-15 | —（画布布局，非设计图） |

### 06 专题设计稿

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `06-专题设计稿/设计系统v2-界面提升/V2-总览与待拍板事项.dc.html` | 总览与待拍板事项 | 总览与 8 件待拍板事项 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-Overview.dc.html` | 规划文档（待拍板事项的最终结论以 CHANGELOG 1.16.0 为准，逐项待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-实施清单与验收.dc.html` | D 实施清单与验收 | 实施清单（18 项）与验收标准 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-D.dc.html` | 规划文档；各项落实情况待核对（1.16.0 已含令牌、组件、设置、队列、侧栏、媒体库、写作窗等） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C1-设置-AI.dc.html` | C1 设置 · AI | 设置 › AI | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C1.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C2-设置全局.dc.html` | C2 设置全局 | 设置页全局（排布开关移到“通用”） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C2.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C3-下载队列.dc.html` | C3 下载队列 | 下载队列 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C3.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C4-设置-安全.dc.html` | C4 设置 · 安全 | 设置 › 安全与隐私（老板键录入控件） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C4.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C5-写作窗.dc.html` | C5 写作窗 | 清影写作窗 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C5.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C6-工具箱首页.dc.html` | C6 工具箱首页 | 工具箱首页（常用工具快捷卡） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C6.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C7-侧栏.dc.html` | C7 侧栏 | 主侧栏（矮窗口滚动 / 仅图标） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C7.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C8-媒体库.dc.html` | C8 媒体库 | 媒体库 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C8.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C9-空状态统一.dc.html` | C9 空状态统一 | 订阅 / 生词 / 迷你窗 / 直播 / 媒体库 / 文件翻译空状态（CcEmpty） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C9.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C10-屏幕翻译.dc.html` | C10 屏幕翻译 | 屏幕翻译 | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C10.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/设计系统v2-界面提升/V2-C11-加载错误反馈.dc.html` | C11 加载 · 错误 · 反馈 | 加载 / 错误 / 反馈（骨架、动效、保存胶囊、错误 toast） | 主画布 [清影界面设计](https://claude.ai/artifact/9780225b-1b36-4a84-b79c-de186538e3fa)（同一画布也以 https://claude.ai/artifact/Ki4UkxGLP5xgTpatwNavmP 列出），导出 2026-11-15；画布键名 `V2-C11.dc.html` | 已在 1.16.0 实现（1.16.0「新增一套统一的界面组件和设计令牌」；与本稿逐项是否一致待核对） |
| `06-专题设计稿/术语分层设计/Dark.dc.html` | 编辑术语库 · 分层术语（暗色） | 设置 › AI › 编辑术语库（分层术语） | [清影术语分层设计](https://claude.ai/artifact/QetgTjfVCAL6TwFmV6wsLR)，导出 2026-11-15 | 已实现（1.9.0「AI 术语库：术语分层」） |
| `06-专题设计稿/术语分层设计/Main.dc.html` | 编辑术语库 · 分层术语（亮色） | 设置 › AI › 编辑术语库（分层术语） | [清影术语分层设计](https://claude.ai/artifact/QetgTjfVCAL6TwFmV6wsLR)，导出 2026-11-15 | 已实现（1.9.0「AI 术语库：术语分层」） |
| `06-专题设计稿/术语分层设计/Spec.dc.html` | 现状与规则 | 设置 › AI › 编辑术语库（分层术语） | [清影术语分层设计](https://claude.ai/artifact/QetgTjfVCAL6TwFmV6wsLR)，导出 2026-11-15 | 已实现（1.9.0「AI 术语库：术语分层」） |
| `06-专题设计稿/术语分层设计/States.dc.html` | 操作：搜索、右键菜单、拖动、删除 | 设置 › AI › 编辑术语库（分层术语） | [清影术语分层设计](https://claude.ai/artifact/QetgTjfVCAL6TwFmV6wsLR)，导出 2026-11-15 | 已实现（1.9.0「AI 术语库：术语分层」） |
| `06-专题设计稿/术语分层设计/assets/d6722339fd1110b46696718d5c48f2f3.png` | 截图素材（画板里引用） | 设置 › AI › 编辑术语库（分层术语） | [清影术语分层设计](https://claude.ai/artifact/QetgTjfVCAL6TwFmV6wsLR)，导出 2026-11-15 | 参考图（现状截图） |
| `06-专题设计稿/术语分层设计/canvas.json` | 画布索引：清影术语分层设计 | 设置 › AI › 编辑术语库（分层术语） | [清影术语分层设计](https://claude.ai/artifact/QetgTjfVCAL6TwFmV6wsLR)，导出 2026-11-15 | —（画布布局，非设计图） |
| `06-专题设计稿/解析结果页重构/Before.dc.html` | 现状与问题 · 新规范 | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | 已实现（1.9.0「解析结果页重做」）；合集页其后 1.14.0 又重新整理，细节待核对 |
| `06-专题设计稿/解析结果页重构/Images.dc.html` | 新 · 图集（图片 + 背景音乐） | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | 已实现（1.9.0「解析结果页重做」）；合集页其后 1.14.0 又重新整理，细节待核对 |
| `06-专题设计稿/解析结果页重构/Main.dc.html` | 新 · 视频（多清晰度、字幕、章节） | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | 已实现（1.9.0「解析结果页重做」）；合集页其后 1.14.0 又重新整理，细节待核对 |
| `06-专题设计稿/解析结果页重构/Playlist.dc.html` | 新 · 合集 / 播放列表 | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | 已实现（1.9.0「解析结果页重做」）；合集页其后 1.14.0 又重新整理，细节待核对 |
| `06-专题设计稿/解析结果页重构/assets/8a0d35e8258e7f8f23c6e13c0a4055ed.png` | 截图素材（画板里引用） | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | 参考图（现状截图） |
| `06-专题设计稿/解析结果页重构/assets/c6cc2743c272c21b8c6e9bc3fa90ca3f.png` | 截图素材（画板里引用） | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | 参考图（现状截图） |
| `06-专题设计稿/解析结果页重构/canvas.json` | 画布索引：清影解析结果页重构 | 解析结果页（视频 / 图集 / 合集） | [清影解析结果页重构](https://claude.ai/artifact/NwZpVQUio8Gf7BGh5NZt48)，导出 2026-11-15 | —（画布布局，非设计图） |
| `06-专题设计稿/剪辑多轨道设计/Features.dc.html` | 4 功能清单：对标达芬奇 / Premiere / 剪映 | 工具箱 › 剪辑（多轨时间线、混音器、自动避让、变速曲线） | [剪辑多轨道设计](https://claude.ai/artifact/KToqKZxJzKCxZJWVLApZUR)，导出 2026-11-15 | 第一批已实现（1.13.0「剪辑：多轨道」）；第二批（混音器 / 自动避让 / 变速曲线）代码里有对应模块，是否与稿完全一致待核对 |
| `06-专题设计稿/剪辑多轨道设计/Main.dc.html` | 1 多轨时间线：每一类都能有很多条轨道 | 工具箱 › 剪辑（多轨时间线、混音器、自动避让、变速曲线） | [剪辑多轨道设计](https://claude.ai/artifact/KToqKZxJzKCxZJWVLApZUR)，导出 2026-11-15 | 第一批已实现（1.13.0「剪辑：多轨道」）；第二批（混音器 / 自动避让 / 变速曲线）代码里有对应模块，是否与稿完全一致待核对 |
| `06-专题设计稿/剪辑多轨道设计/Mixer.dc.html` | 5 混音器、自动避让、变速曲线 | 工具箱 › 剪辑（多轨时间线、混音器、自动避让、变速曲线） | [剪辑多轨道设计](https://claude.ai/artifact/KToqKZxJzKCxZJWVLApZUR)，导出 2026-11-15 | 第一批已实现（1.13.0「剪辑：多轨道」）；第二批（混音器 / 自动避让 / 变速曲线）代码里有对应模块，是否与稿完全一致待核对 |
| `06-专题设计稿/剪辑多轨道设计/Ops.dc.html` | 2 怎么加轨、换轨、管理轨道 | 工具箱 › 剪辑（多轨时间线、混音器、自动避让、变速曲线） | [剪辑多轨道设计](https://claude.ai/artifact/KToqKZxJzKCxZJWVLApZUR)，导出 2026-11-15 | 第一批已实现（1.13.0「剪辑：多轨道」）；第二批（混音器 / 自动避让 / 变速曲线）代码里有对应模块，是否与稿完全一致待核对 |
| `06-专题设计稿/剪辑多轨道设计/Rules.dc.html` | 3 叠放规则、旧工程怎么变、需要你定的事 | 工具箱 › 剪辑（多轨时间线、混音器、自动避让、变速曲线） | [剪辑多轨道设计](https://claude.ai/artifact/KToqKZxJzKCxZJWVLApZUR)，导出 2026-11-15 | 第一批已实现（1.13.0「剪辑：多轨道」）；第二批（混音器 / 自动避让 / 变速曲线）代码里有对应模块，是否与稿完全一致待核对 |
| `06-专题设计稿/剪辑多轨道设计/canvas.json` | 画布索引：剪辑多轨道设计 | 工具箱 › 剪辑（多轨时间线、混音器、自动避让、变速曲线） | [剪辑多轨道设计](https://claude.ai/artifact/KToqKZxJzKCxZJWVLApZUR)，导出 2026-11-15 | —（画布布局，非设计图） |
| `06-专题设计稿/文件翻译版面工作台设计/Collapse.dc.html` | 7 左栏收起：三种状态 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Extract.dc.html` | 4 从原页取色、提取一组颜色和渐变 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Fix.dc.html` | 6 裁切、背景露边、换行、字号 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Harmony.dc.html` | 5 配色色轮、对比度与色盲 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Main.dc.html` | 1 版面工作台：选中一块（对比模式） | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Multi.dc.html` | 2 多选与批量 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Notes.dc.html` | 8 原因判断、weld_app 对照、需要你确认的事 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Page.dc.html` | 页面示意组件 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/Picker.dc.html` | 3 取色器（对照 weld_app 的取色对话框） | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | 已实现（1.13.0「文件翻译：版面工作台」）；“安全色域”等个别项待核对 |
| `06-专题设计稿/文件翻译版面工作台设计/canvas.json` | 画布索引：文件翻译版面工作台设计 | 文件翻译 › 版面工作台（块编辑、取色器、配色与对比度、左栏收起） | [文件翻译版面工作台设计](https://claude.ai/artifact/HcKASDsmgS3rvkiKRE2zAm)，导出 2026-11-15 | —（画布布局，非设计图） |
| `06-专题设计稿/工具箱分组与文件篮设计/Batch.dc.html` | 3 批量处理中（响度标准化） | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Dark.dc.html` | 4 暗色（生成字幕，不需要预览） | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Live.dc.html` | 2 视频规整（实时预览） | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Main.dc.html` | 1 压缩视频（试一小段预览 + 文件篮） | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Map.dc.html` | 7 旧位置 → 新位置，文件篮规则 | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Nav.dc.html` | 6 返回按钮与主导航图标条 | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Rail.dc.html` | 主导航图标条组件 | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Sidebar.dc.html` | 侧栏组件 | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/Tiers.dc.html` | 5 预览三档：每个工具属于哪一档 | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | 已实现（1.13.0「工具箱重新分组」、文件篮）；“试压预览”等细节待核对 |
| `06-专题设计稿/工具箱分组与文件篮设计/canvas.json` | 画布索引：工具箱分组与文件篮设计 | 工具箱（分组、文件篮、预览三档、主导航图标条） | [工具箱分组与文件篮设计](https://claude.ai/artifact/Ds71fqzYxYC3ZvwVxf5xmr)，导出 2026-11-15 | —（画布布局，非设计图） |
| `06-专题设计稿/批量下载并入主页/Batch.dc.html` | ② 主页 · 批量模式（粘贴多条链接后，就地预览和设置） | 解析页（单个解析 / 批量下载切换、批次管理） | [批量下载并入主页](https://claude.ai/artifact/MKNCo1dNcQ4PwJ4KJ3qM4G)，导出 2026-11-15 | 已实现（1.11.0「批量下载并入主页」） |
| `06-专题设计稿/批量下载并入主页/Main.dc.html` | ① 主页 · 默认（单个解析 + 进行中的批次） | 解析页（单个解析 / 批量下载切换、批次管理） | [批量下载并入主页](https://claude.ai/artifact/MKNCo1dNcQ4PwJ4KJ3qM4G)，导出 2026-11-15 | 已实现（1.11.0「批量下载并入主页」） |
| `06-专题设计稿/批量下载并入主页/Running.dc.html` | ③ 主页 · 批次管理（原“批量下载”页的内容） | 解析页（单个解析 / 批量下载切换、批次管理） | [批量下载并入主页](https://claude.ai/artifact/MKNCo1dNcQ4PwJ4KJ3qM4G)，导出 2026-11-15 | 已实现（1.11.0「批量下载并入主页」） |
| `06-专题设计稿/批量下载并入主页/canvas.json` | 画布索引：批量下载并入主页 | 解析页（单个解析 / 批量下载切换、批次管理） | [批量下载并入主页](https://claude.ai/artifact/MKNCo1dNcQ4PwJ4KJ3qM4G)，导出 2026-11-15 | —（画布布局，非设计图） |
| `06-专题设计稿/文件翻译-重新整理/Layout.dc.html` | ③ 版面：以前整页报错，现在按页显示 | 文件翻译（任务、版面、开始前确认、导出） | [清影文件翻译 · 重新整理](https://claude.ai/artifact/3rVyUraVG6a79J3mA7QC9e)，导出 2026-11-15 | 已实现（1.10.0「文件翻译：任务、版面、开始前确认」）；“开始前确认”1.15.0 并入翻译设置 |
| `06-专题设计稿/文件翻译-重新整理/Main.dc.html` | ① 整页：任务、设置、阅读器 | 文件翻译（任务、版面、开始前确认、导出） | [清影文件翻译 · 重新整理](https://claude.ai/artifact/3rVyUraVG6a79J3mA7QC9e)，导出 2026-11-15 | 已实现（1.10.0「文件翻译：任务、版面、开始前确认」）；“开始前确认”1.15.0 并入翻译设置 |
| `06-专题设计稿/文件翻译-重新整理/Spec.dc.html` | ⑤ 原因与规则 | 文件翻译（任务、版面、开始前确认、导出） | [清影文件翻译 · 重新整理](https://claude.ai/artifact/3rVyUraVG6a79J3mA7QC9e)，导出 2026-11-15 | 已实现（1.10.0「文件翻译：任务、版面、开始前确认」）；“开始前确认”1.15.0 并入翻译设置 |
| `06-专题设计稿/文件翻译-重新整理/Start.dc.html` | ④ 开始前确认 + 导出 | 文件翻译（任务、版面、开始前确认、导出） | [清影文件翻译 · 重新整理](https://claude.ai/artifact/3rVyUraVG6a79J3mA7QC9e)，导出 2026-11-15 | 已实现（1.10.0「文件翻译：任务、版面、开始前确认」）；“开始前确认”1.15.0 并入翻译设置 |
| `06-专题设计稿/文件翻译-重新整理/Tasks.dc.html` | ② 任务：每种状态有哪些按钮 | 文件翻译（任务、版面、开始前确认、导出） | [清影文件翻译 · 重新整理](https://claude.ai/artifact/3rVyUraVG6a79J3mA7QC9e)，导出 2026-11-15 | 已实现（1.10.0「文件翻译：任务、版面、开始前确认」）；“开始前确认”1.15.0 并入翻译设置 |
| `06-专题设计稿/文件翻译-重新整理/canvas.json` | 画布索引：清影文件翻译 · 重新整理 | 文件翻译（任务、版面、开始前确认、导出） | [清影文件翻译 · 重新整理](https://claude.ai/artifact/3rVyUraVG6a79J3mA7QC9e)，导出 2026-11-15 | —（画布布局，非设计图） |

### 07 渲染图（png）

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `07-渲染图/Chat.png` | 手机互聊（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Chat.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Editor.png` | 工具箱 · 剪辑（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Editor.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/ExtPage.png` | 扩展 · 页面内浮层（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 ExtPage.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/ExtPopup.png` | 扩展 · 弹窗（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 ExtPopup.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/ExtSettings.png` | 扩展 · 设置（横向导航）（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 ExtSettings.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/FileTrans.png` | 文件翻译（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 FileTrans.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Library.png` | 媒体库（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Library.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Live.png` | 直播（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Live.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Main.png` | 解析（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Main.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Overlays.png` | 浮层与小窗（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Overlays.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/ParseList.png` | 解析 · 列表与批量（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 ParseList.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Phone.png` | 手机互传（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Phone.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Queue.png` | 下载队列（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Queue.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Screen.png` | 屏幕翻译（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Screen.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-accounts.png` | 设置 · 账号与 Cookie（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-accounts.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-ai.png` | 设置 · AI（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-ai.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-auto.png` | 设置 · 自动化（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-auto.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-components.png` | 设置 · 组件（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-components.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-diag.png` | 设置 · 诊断（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-diag.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-general.png` | 设置 · 通用（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-general.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-network.png` | 设置 · 网络（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-network.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-p2p.png` | 设置 · BT 与 ed2k（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-p2p.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-parse.png` | 设置 · 解析（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-parse.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-phone.png` | 设置 · 手机与扩展（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-phone.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-security.png` | 设置 · 安全与隐私（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-security.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings-writing.png` | 设置 · 写作（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings-writing.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Settings.png` | 设置 · 下载（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Settings.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Small.png` | 小窗口与悬浮条（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Small.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Subs.png` | 订阅（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Subs.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Toolbox.png` | 工具箱（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Toolbox.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C1.png` | C1 设置 · AI（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C1.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C10.png` | C10 屏幕翻译（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C10.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C11.png` | C11 加载 · 错误 · 反馈（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C11.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C12.png` | C12 硬编码色对照（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C12.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C2.png` | C2 设置全局（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C2.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C3.png` | C3 下载队列（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C3.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C4.png` | C4 设置 · 安全（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C4.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C5.png` | C5 写作窗（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C5.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C6.png` | C6 工具箱首页（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C6.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C7.png` | C7 侧栏（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C7.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C8.png` | C8 媒体库（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C8.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-C9.png` | C9 空状态统一（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-C9.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-CMP-1.png` | 组件 · 按钮 分段 链接 选择（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-CMP-1.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-CMP-2.png` | 组件 · 字段 空状态 骨架 对话框 提示（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-CMP-2.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-D.png` | D 实施清单与验收（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-D.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-DS-1.png` | 设计系统 · 度量与动效（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-DS-1.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-DS-2.png` | 设计系统 · 色彩与令牌清单（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-DS-2.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/V2-Overview.png` | 总览与待拍板事项（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 V2-Overview.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Vocab.png` | 生词与摘录（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Vocab.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |
| `07-渲染图/Write.png` | 清影写作（渲染图） | 见同名画板 | 仓库原有（Chromium 深色渲染，1360 宽；画板 Write.dc.html 的旧版本渲染，未随 11-15 的 4 次画布编辑更新） | 渲染图，状态同对应画板 |

### 共用文件（样式与画布索引）

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `_shared/clearclip.css` | 旧画板共用样式 | — | 主画布 clearclip.css（导出 2026-11-15） | 设计稿样式；程序里的实现在 desktop/src/styles/theme.css、skin.css |
| `_shared/v2.css` | V2 画板共用样式（带 NEW 标记的是建议新增的 --cc-* 令牌） | — | 主画布 v2.css（导出 2026-11-15） | 设计稿样式 |
| `_shared/清影界面设计.canvas.json` | 主画布的画布索引（每张图的位置、大小、标题；键名是归档前的文件名） | — | 主画布 canvas.json（导出 2026-11-15） | —（画布布局，非设计图） |

### 生成脚本

| 文件 | 标题 | 程序页面 / 功能 | 来源 | 状态 |
|---|---|---|---|---|
| `gen/build.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/build_v2.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/lib.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/measure_v2.cjs` | 设计稿生成脚本（Playwright 量高度） | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/pages_a.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/pages_b.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/pages_c.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/pages_d.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/v2_pages.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/v2_spec.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |
| `gen/v2lib.py` | 设计稿生成脚本 | — | 仓库原有（未移动） | 生成脚本，非设计图 |

## 未收录 / 未能读取的设计

（本节见 PR 描述的最新版本）
