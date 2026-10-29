# 浏览器扩展上架

> 这是 [发布前的准备](README.md) 的一部分。费用、界面和审核要求会变，步骤以各官方页面为准。


扩展的打包文件 `ClearClip-extension-<版本>.zip` 会由发布流水线自动放进每个 Release，可以直接上传到各商店。上架材料已经准备好：

- 商店文案和权限说明：[store-listing.md](store-listing.md)（复制粘贴即可）
- 隐私政策：[privacy-policy.md](privacy-policy.md)，填商店要求的“隐私政策链接”时用  
  `https://github.com/DiXuanAoYi/ClearClip-Downloader/blob/main/docs/release-guide/privacy-policy.md`

### 要准备的截图

商店都要求截图，我无法替你截。建议 3–5 张，在清影和浏览器里实际操作后截取：

1. 在视频网页上点击扩展图标，弹窗里发现的视频流列表和“发送所选”；
2. 右键菜单里的“用清影下载”；
3. “接管浏览器下载”的设置（类型、最小大小、网站过滤）和“最近接管的下载”；
4. 清影下载队列里出现被接管的下载（“浏览器下载”分类）；
5. 扩展里的“同步此网站的登录 Cookie”和自动同步开关。

Chrome 要求 1280×800 或 640×400（PNG 或 JPEG）；Edge、Firefox 对尺寸要求较宽松，用同样的图即可。

### 1. Chrome 网上应用店

1. 打开 [Chrome 开发者控制台](https://chrome.google.com/webstore/devconsole)，用 Google 账号登录，接受开发者协议并缴纳一次性注册费。
2. **添加新内容 → 上传** `ClearClip-extension-<版本>.zip`。
3. 填写“商店列表”（文案见 [store-listing.md](store-listing.md)）、上传图标（`icons/128.png`）和截图。
4. 在“隐私权”页签填写：
   - **单一用途**：见 [store-listing.md](store-listing.md)；
   - **权限理由**：对 `downloads`、`cookies`、`webRequest`、`tabs`、`contextMenus`、`storage`、`activeTab`、`notifications`、主机权限 `<all_urls>` 和内容脚本分别填写理由（[store-listing.md](store-listing.md) 里有现成文字）；
   - **隐私政策链接**：上面的链接；
   - 数据使用披露：勾选“身份验证信息”（Cookie）、“网站内容”和“网页历史记录”（说明见 [store-listing.md](store-listing.md)），并勾选三项合规声明。
5. 提交审核。申请 `<all_urls>` 这类宽范围主机权限的扩展审核会更久，可能被要求补充说明；如果被拒，按邮件意见修改后重新提交。

> 新增的 `downloads`、`webRequest`、`tabs` 权限会让审核更严格，权限理由要写清“只在用户开启 / 操作时使用、只发往本机”（[store-listing.md](store-listing.md) 里已写好）。想加快审核、减少被拒的风险，可以以后把 `<all_urls>` 改成“按需授权”（`optional_host_permissions`，同步某个网站的 Cookie 时再请求该网站的权限）。这需要改扩展代码，需要时告诉我。

### 2. Microsoft Edge 加载项

1. 打开 [Partner Center 的 Edge 页面](https://partner.microsoft.com/dashboard/microsoftedge)，用微软账号注册开发者（免费）。
2. **创建新扩展 → 上传** 同一个 zip。
3. 填写属性（类别选“生产力”）、商店列表和隐私政策链接（同上）。
4. 在“Notes for certification”里说明扩展只连接本机 `127.0.0.1`，测试需要先安装清影桌面版，并给出下载链接。
5. 提交，通常几个工作日审核。

### 3. Firefox 附加组件（AMO）

1. 打开 [Firefox 附加组件开发者中心](https://addons.mozilla.org/developers/)，用 Firefox 账号登录（免费）。
2. **提交新附加组件 → 在 Mozilla 上发布**（上架到商店），上传同一个 zip。扩展 ID 已写在 `manifest.json` 的 `browser_specific_settings.gecko.id`，不要改动，否则会被当成新扩展。
3. 源代码：扩展没有打包或压缩，不需要另外提交源代码。
4. 填写简介、分类、隐私政策链接和截图，提交。
5. 审核通过前，用户仍可以按 `desktop/extension/README.md` 手动加载。

### 以后更新扩展

改 `desktop/extension/manifest.json` 里的 `version`（必须比已上架的大），发布流水线会生成新 zip，再到各商店后台上传新版本。

---
