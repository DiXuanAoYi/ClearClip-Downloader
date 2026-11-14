# 安全政策

## 支持的版本

只维护 [最新的正式版本](https://github.com/DiXuanAoYi/ClearClip-Downloader/releases/latest)（`desktop-v*`）。安全修复只发在最新版里，旧版本请先升级；程序内“检查更新”和发布页都能拿到最新版。

## 报告漏洞

请**不要**在公开的 Issue 里贴漏洞细节。通过 GitHub 的私密渠道报告：

1. 打开本仓库的 **Security** 标签页，点 **Report a vulnerability**（GitHub Security Advisories，仅仓库所有者可见）。
2. 写清楚：影响的版本和系统、复现步骤、可能的影响，有的话附上概念验证。

收到后会尽快确认并评估；修复发布之后，会在更新记录里说明，并按你的意愿署名。

## 范围

在范围内：

- 清影桌面程序（`desktop/`）：下载与解析、本地加密保险箱、应用锁、手机互传、内置 MCP 服务、自动更新、命令与权限边界。
- 浏览器扩展（`desktop/extension/`）：权限使用、与桌面程序的通信。
- 发布流程和更新包签名（`.github/workflows/`）。

不在范围内：

- 目标网站本身的漏洞、被网站反爬或下线的接口。
- 需要先取得本机管理员权限或已经能读写用户数据目录才能成立的问题。
- 依赖库里已公开、且已经在 `desktop/src-tauri/deny.toml` / `.cargo/audit.toml` 里写明理由暂时忽略的通告（欢迎指出理由不成立）。

安全与隐私的设计说明见 [docs/security-and-privacy.md](docs/security-and-privacy.md)。
