# macOS 签名和公证

> 这是 [发布前的准备](README.md) 的一部分。费用、界面和审核要求会变，步骤以各官方页面为准。


## 现在的状态

- **0.1.0** 的 macOS 安装包完全没有签名，在 Apple 芯片的 Mac 上会提示“**ClearClip.app 已损坏，无法打开**”。临时解决办法：在终端执行 `xattr -cr /Applications/ClearClip.app` 后再打开。
- **0.1.1 起**，发布流水线在没有配置 Apple 证书时会自动做**临时（ad-hoc）签名**，并在构建时用 `codesign --verify --deep --strict` 校验。0.1.1 的两个 macOS 安装包（Apple 芯片和 Intel）都通过了校验（`Signature=adhoc`，`valid on disk`）。
  - 按 macOS 的规则，签过名的应用被隔离后，提示应是“无法验证开发者”而不是“已损坏”：到“系统设置 → 隐私与安全性”，在页面下方点“仍要打开”（macOS 15 起右键“打开”不再能绕过拦截）。
  - **没有在真机上验证过**。如果仍然提示“已损坏”，用上面的 `xattr -cr` 办法，并到 Issue 里告诉我们 macOS 的版本和芯片类型。
- 要让用户**双击直接打开**，需要下面的 Developer ID 签名和公证。

没有 Apple 开发者账号也能正常发布和使用，只是首次打开多一步。

### 前提

- 加入 [Apple Developer Program](https://developer.apple.com/programs/)（个人或组织，年费）。
- 一台 Mac（导出证书用，不需要长期使用）。

### 步骤

1. **创建证书**：登录 [developer.apple.com → Certificates](https://developer.apple.com/account/resources/certificates/list)，新建 **Developer ID Application** 证书（按页面说明在 Mac 的“钥匙串访问”里生成证书请求文件上传，再下载 `.cer` 双击安装）。
2. **导出 .p12**：钥匙串访问 → 我的证书 → 右键该证书 → 导出，格式选 `.p12`，设一个导出密码。
3. **转成 base64**：

   ```bash
   base64 -i certificate.p12 | pbcopy
   ```

4. **记下签名身份**：终端执行 `security find-identity -v -p codesigning`，复制形如 `Developer ID Application: 你的名字 (TEAMID)` 的整行。
5. **创建应用专用密码**：登录 [appleid.apple.com](https://appleid.apple.com) → 登录与安全 → 应用专用密码，新建一个。
6. **Team ID**：在 [developer.apple.com → Membership details](https://developer.apple.com/account) 里查看。
7. 在 GitHub 仓库 **Settings → Secrets and variables → Actions** 添加：

   | 名称 | 内容 |
   | --- | --- |
   | `APPLE_CERTIFICATE` | 第 3 步复制的 base64 文本 |
   | `APPLE_CERTIFICATE_PASSWORD` | 第 2 步的导出密码 |
   | `APPLE_SIGNING_IDENTITY` | 第 4 步的整行文字 |
   | `APPLE_ID` | Apple ID 邮箱 |
   | `APPLE_PASSWORD` | 第 5 步的应用专用密码（不是 Apple ID 登录密码） |
   | `APPLE_TEAM_ID` | 第 6 步的 Team ID |

8. 重新运行 `desktop-release` 工作流。流水线只会把已配置（非空）的 Secrets 传给打包工具，所以没配的时候不会报错；配好后会自动签名并提交公证（公证需要几分钟到几十分钟）。
9. 验证：下载 `.dmg` 安装后直接双击打开，不再被拦截。可以在终端用 `spctl -a -vv /Applications/ClearClip.app` 检查，显示 `accepted` 和 `source=Notarized Developer ID`。

---
