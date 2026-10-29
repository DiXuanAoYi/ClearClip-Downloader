# 包管理器清单

| 渠道 | 文件 | 发布方式 |
| --- | --- | --- |
| winget | `winget/*.yaml` | 用 `update-manifests.py` 生成后，向 [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) 提交 PR（目录 `manifests/d/DiXuanAoYi/ClearClip/<版本>/`），或使用 `wingetcreate submit` |
| Scoop | `scoop/clearclip.json` | 放到自己的 bucket 仓库；`checkver` / `autoupdate` 已配置，可用 Scoop 的 `checkver.ps1 -u` 自动更新 |
| Homebrew | `homebrew/clearclip.rb` | 放到自己的 tap（如 `DiXuanAoYi/homebrew-tap` 的 `Casks/`），`brew install --cask dixuanaoyi/tap/clearclip` |

发布新版本（推送 `desktop-v*` 标签、Release 发布后）：

```bash
python3 update-manifests.py 0.2.0   # 下载安装包计算 SHA-256，输出到 dist/
```

Scoop 使用便携版 zip，数据保存在安装目录的 `data`（已设置 persist，更新时保留）。
