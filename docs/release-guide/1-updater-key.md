# 更新签名密钥

> 这是 [发布前的准备](README.md) 的一部分。费用、界面和审核要求会变，步骤以各官方页面为准。


程序内更新用一对密钥校验更新包：**私钥**留在 GitHub 的 Secrets 里，用来给安装包签名；**公钥**编译进程序，用来验证。这和操作系统的代码签名无关，不需要购买证书。

### 步骤

1. 在自己电脑上（需要装 Node.js），进入仓库的 `desktop` 目录，生成密钥：

   ```bash
   cd desktop
   npm install
   npm run tauri signer generate -- -w ~/.tauri/clearclip.key
   ```

   按提示设置一个密码（可以留空，但建议设置）。生成两个文件：`clearclip.key`（私钥）和 `clearclip.key.pub`（公钥）。

2. 打开 GitHub 仓库 → **Settings → Secrets and variables → Actions → New repository secret**，添加三个 Secret：

   | 名称 | 内容 |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | `clearclip.key` 文件的全部内容 |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 第 1 步设置的密码（没设就不用添加） |
   | `TAURI_UPDATER_PUBKEY` | `clearclip.key.pub` 文件的全部内容 |

3. 发布新版本时，在 **Actions → desktop-release → Run workflow**（选 `main` 分支）。流水线检测到私钥后会自动：
   - 额外生成更新包（`.sig` 签名文件）和 `latest.json`，上传到 Release；
   - 把公钥编译进程序。

4. 验证：用**这一版及以后**的安装包，在“设置 → 通用 → 关于”点“检查更新”，有新版本时会出现“下载并安装”。

### 注意

- **私钥务必备份**（密码管理器、加密网盘）。丢了私钥，已安装的用户就无法再自动更新，只能手动下载新版安装包。
- **私钥不要提交到仓库**，也不要贴到聊天或 Issue 里。
- 已经发布的 0.1.0 没有编译进公钥，**不能**程序内更新，用户需要手动下载一次新版；之后的版本就可以自动更新了。
- 更新检查地址写在 `desktop/src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints`，指向最新 Release 里的 `latest.json`。

---

## 迁移仓库或新建仓库后

仓库的 Secrets 不会跟着迁移。新仓库里没有配置 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_UPDATER_PUBKEY` 时，发布依然成功，但 Release 里没有 `latest.json` 和 `.sig` 文件，程序内只能“检查到新版本并打开发布页”，不能一键安装；流水线日志里会出现黄色警告提醒。按上面的步骤把两个 Secret 补上，再发布下一个版本即可。注意：之前发布的、没有编译进公钥的版本（例如 0.8.0）无法程序内更新，需要手动安装一次带公钥的新版，之后才能自动更新。
