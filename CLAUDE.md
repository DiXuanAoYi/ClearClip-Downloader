# 清影 ClearClip：维护说明

给在这个仓库里工作的 Claude 看的。用户文档在 `README.md`、`desktop/README.md` 和 `docs/`；当前规划在 [docs/roadmap.md](docs/roadmap.md) 第九节。

## 仓库

- `desktop/`：桌面程序（Tauri 2 + Vue 3 + Rust）。`desktop/src-tauri/` 是 Rust，`desktop/src/` 是界面。
- `desktop/extension/`：浏览器扩展（MV3，Chrome / Edge / Firefox），和桌面程序一起发布。
- `docs/`：功能说明和规划；`docs/release-guide/` 是需要所有者自己账号的事（签名、上架）。

## 和所有者的约定

- 用中文汇报。说清楚哪些没验证：真实网站、真实 Windows、真实浏览器里的扩展，在开发环境里都验证不了。
- 文档、CI、测试这类不改变程序行为的改动：可以自己开 PR，CI 通过后合并。
- 改程序行为的改动：推到分支（推送会触发 CI），问所有者要不要开 PR、合并。
- 发版每次由所有者决定。
- 所有者说“先出设计图”时，先出设计稿等确认，不要直接写代码；说“全部做完再告诉我”时，中间不汇报，做完一次说清楚。
- 面向中文和英文用户，目前只有所有者自己在用。主线是**下载与解析、屏幕翻译、扩展里的字幕和音效**，验证优先覆盖这三块。

## 常用命令

在 `desktop/` 下：

```bash
npm ci
npm run build                 # vue-tsc 类型检查 + vite 构建
npm run test:unit             # e2e/unit/*.test.mjs
node e2e/run.mjs parse-ui.cjs # 跑一个界面测试脚本；不带参数跑默认套件（需要 ffmpeg）
npm test                      # 单元测试 + 默认套件（CI 跑的就是这个）
npm run release:check         # 各处版本号是否一致
```

在 `desktop/src-tauri/` 下：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

机器内存或磁盘紧时，cargo 命令前加 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`。没有 Playwright 自带的浏览器时，用 `CHROMIUM_PATH` 指向已有的 Chromium（所有界面测试脚本都认）。

## 容易踩的坑

- 最低 Rust 版本是 1.96（`Cargo.toml` 的 `rust-version`）：不要用之后才稳定的 API，比如 `Option::is_none_or`。
- `cfg(windows)` 的代码在 Linux 上不编译。CI 的 cross-check 只编译 Windows ARM64，x64 要到发版时才第一次编译；Windows 专有的测试 CI 不跑。改这类代码时在 PR 和汇报里写明没有编译或运行验证。
- 单元测试用 `node --experimental-strip-types` 跑，测试里相对导入 `.ts` 文件要写扩展名。
- 新的界面测试脚本要加进 `e2e/run.mjs` 的默认列表，否则 CI 不跑；自己启动浏览器的要写 `chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })`。
- 解析器的测试样本（`src-tauri/tests/fixtures/`）是按页面结构写的，不访问线上；改解析时尽量用“录制样本”存下的真实响应。
- 界面文字是中文，英文在 `src/i18n/en.ts`；`npm run i18n` 列出缺的（目前不强制）。

## 更新记录和发版

- 改动写进 `desktop/CHANGELOG.md` 最上面。开发中的版本标题写 `## x.y.z（开发中，未发版）`，没做的部分也写明。
- 发版（所有者同意后）：
  1. `npm run release:version -- x.y.z [--ext a.b.c]`（在 `desktop/` 下）：改齐 7 处版本号，把更新记录标题改成 `## x.y.z（扩展 a.b.c）`。浏览器扩展有改动时才升扩展版本。
  2. 提交、开 PR，CI 通过后合并到 `main`。
  3. 在 `main` 上手动运行 `desktop-release`（约 25–30 分钟）。最后的 `verify` 任务核对安装包是否齐全；Release 页面最上面是这一版的更新记录。
- 提交说明用中文。
