# 发布与上架教程

这个文件夹放的是**需要项目所有者自己的账号、证书或付费订阅才能完成**的事情的教程。代码和发布流水线已经准备好了：配好对应的密钥后，重新运行一次 `desktop-release` 工作流，安装包就会带上签名。

**这些都不是必须的**：不配也能正常发布和使用，只是用户安装时会多一两步确认。

| 事项 | 作用 | 费用 | 难度 | 现状 |
| --- | --- | --- | --- | --- |
| [1. 更新签名密钥](1-updater-key.md) | 程序内“检查更新 → 一键安装” | 免费 | 低（约 10 分钟） | 未配置：检查更新只提示有新版本，再跳到发布页手动下载 |
| [2. 浏览器扩展上架](2-extension-store.md) | 用户从商店一键安装扩展 | Chrome 一次性注册费（金额以后台为准）；Edge、Firefox 免费 | 中（审核 1–3 天） | 未上架：扩展 zip 在每个 Release 里，按说明手动加载 |
| [3. macOS 签名和公证](3-macos-signing.md) | 用户双击就能打开，不再被拦截 | Apple Developer Program 年费 | 中 | 未配置：流水线做临时签名，首次打开需要在“隐私与安全性”里点“仍要打开” |
| [4. Windows 代码签名](4-windows-signing.md) | 去掉 SmartScreen 的“未知发布者”警告 | 证书或签名服务按年收费 | 较高 | 未配置：点“更多信息 → 仍要运行” |

建议顺序：1 → 2 → 3 → 4（4 成本最高，也最不必要）。

## 其他材料

- [store-listing.md](store-listing.md)：扩展商店的名称、描述、单一用途说明和权限理由，可以直接复制。
- [privacy-policy.md](privacy-policy.md)：隐私政策，商店要求填写它的网址。

## 发布一个新版本

1. 修改版本号（`desktop/src-tauri/tauri.conf.json`、`desktop/src-tauri/Cargo.toml`、`desktop/package.json` 三处保持一致），合并到 `main`。
2. 打开 GitHub 的 **Actions → desktop-release → Run workflow**，分支选 `main`。
3. 等四个平台都构建完成（约 15 分钟）。Release 会自动创建，包含各平台安装包、Windows 便携版、浏览器扩展 zip；配置了更新签名时还会有 `latest.json`。

## 官方文档

下面的页面以官方当前内容为准（Tauri 页面可切换到中文）：

- [Tauri：Windows 代码签名](https://v2.tauri.app/distribute/sign/windows/)（更新插件和 macOS 签名在同站的 Distribute 与 Plugins 栏目）
- [Chrome 应用店：用户数据与隐私政策要求](https://developer.chrome.com/webstore/user_data)
- [Chrome 应用店：Limited Use 限制](https://developer.chrome.com/docs/webstore/program-policies/limited-use/)
- [Chrome 应用店：开发者协议](https://developer.chrome.com/docs/webstore/terms/)
- Edge 加载项和 Firefox 附加组件的发布说明，请在各自的开发者中心（上文链接）内查看。
