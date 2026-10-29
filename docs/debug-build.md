# 调试版：不发布也能在真机上看效果

每次改完都合并、发版再下载，太慢也浪费构建资源。有两种办法，按需要选一个。

## 办法一：GitHub 上构建调试版（不用装任何开发环境）

只构建 Windows 便携包，不创建 Release、不改版本号、不需要合并分支，通常几分钟到十几分钟。

**自动**：往 `claude/**` 或 `debug/**` 分支推送时，提交说明里带 `[debug-build]`，就会自动构建。

**手动**：仓库页面 → Actions → `desktop-debug` → Run workflow，在 “Use workflow from” 里选要测试的分支，选好架构（绝大多数电脑是 x64，骁龙等 ARM 设备选 arm64）。这个入口要等 `desktop-debug.yml` 进入默认分支（main）后才会出现。

构建完成后，在这次运行的页面最下面的 **Artifacts** 里下载 `ClearClip-debug-…-windows-x64.zip`（需要登录 GitHub，保留 7 天），解压后双击 `clearclip.exe`：

- 会多出一个**控制台窗口**，程序日志实时显示在里面；关闭它等于退出程序
- 界面里**右键 → 检查**，可以打开开发者工具（看前端报错、网络请求）
- 数据、设置、日志都在解压目录的 `data` 文件夹（便携模式），不会碰已安装版本的数据；日志文件是 `data\logs\clearclip.log`
- 调试版没有优化，体积大、速度慢，只用来测试

**怎么确认运行的是这一版**：窗口标题是“清影 ClearClip 调试版 版本 · 提交号”，侧栏左下角写着“v版本 · 调试版 提交号”，提交号和压缩包名字里的那 7 位一致就对了。

**能不能和已安装的清影同时开**：能。清影只允许同一个程序标识开一个实例，再开一次只会把已经开着的那个调到前面。正式版和调试版用的是不同的标识（调试版是 `com.dixuanaoyi.clearclip.debug`，配置在 `desktop/src-tauri/tauri.debug.conf.json`），所以互不影响。但**两个调试包之间**用的是同一个标识：换新调试包之前，先把上一个从托盘（右下角图标）退出，否则新包一启动就被转给旧的，看到的还是旧界面。

## 办法二：在自己电脑上开发模式运行（改动立刻生效）

适合要反复试的情况。需要先装：[Node.js 22](https://nodejs.org/)、[Rust](https://rustup.rs/)（1.80 以上）、Visual Studio 生成工具里的“使用 C++ 的桌面开发”，Windows 11 自带 WebView2。

```powershell
git clone https://github.com/DiXuanAoYi/ClearClip-Downloader.git
cd ClearClip
git checkout <要测试的分支>
cd desktop
npm install
npm run tauri dev
```

第一次编译需要十几分钟，之后改前端代码界面立刻刷新，改 Rust 代码会自动重新编译重启。控制台里能看到日志，界面里右键 → 检查打开开发者工具。

## 反馈问题时带上什么

- 控制台里的日志（或 `data\logs\clearclip.log`，正式安装版在“设置 → 诊断 → 打开日志目录”）
- 开发者工具 Console 标签里的红色报错
- 一张截图
