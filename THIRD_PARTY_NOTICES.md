# 第三方许可说明

清影 ClearClip 自身以 [MIT 许可证](LICENSE) 发布。它使用了下列开源库，各自保留原有许可证；完整的依赖树和版本见 `desktop/package-lock.json` 与 `desktop/src-tauri/Cargo.lock`。

## 前端（npm）

| 库 | 许可证 |
| --- | --- |
| vue、pinia、vite、@vitejs/plugin-vue | MIT |
| element-plus、@element-plus/icons-vue | MIT |
| @tauri-apps/api、@tauri-apps/plugin-dialog、@tauri-apps/cli | MIT 或 Apache-2.0 |
| typescript | Apache-2.0 |
| vue-tsc | MIT |
| playwright（仅测试） | Apache-2.0 |

## 后端（Rust crate）

| 库 | 许可证 |
| --- | --- |
| tauri 及 tauri-plugin-*（clipboard-manager、notification、global-shortcut、dialog、opener、single-instance、log、autostart、updater） | MIT 或 Apache-2.0 |
| serde、serde_json、reqwest、tokio、futures-util、regex、url、thiserror、chrono、log、tar、zip、hex、fs4、sha2、md-5、aes、cbc、aes-gcm、argon2、async-trait | MIT 或 Apache-2.0 |
| rusqlite（含内置 SQLite） | MIT（SQLite 为公有领域） |
| lzma-rs | MIT |
| qrcode | MIT 或 Apache-2.0 |
| keyring | MIT 或 Apache-2.0 |
| windows-sys | MIT 或 Apache-2.0 |

间接依赖的许可证以各自仓库为准，可用 `cargo license` / `npm ls` 查看。

## 运行时下载的外部工具

下面的程序**不随本仓库分发**，由用户在应用内按需下载，遵循各自的许可证：

- [FFmpeg](https://ffmpeg.org/)：默认下载 LGPL 构建；若用户选择完整构建则为 GPL。
- [yt-dlp](https://github.com/yt-dlp/yt-dlp)：Unlicense。

## 名称与商标

抖音、快手等名称属于各自的权利人，本项目与其无隶属关系。
