#!/usr/bin/env python3
"""填写包管理器清单的版本和 SHA-256。

用法：python3 update-manifests.py 0.2.0
从 GitHub Release desktop-v<版本> 下载安装包计算校验值，输出到 dist/ 目录：
  dist/winget/   提交到 microsoft/winget-pkgs（manifests/d/DiXuanAoYi/ClearClip/<版本>/）
  dist/scoop/    放到自己的 Scoop bucket
  dist/homebrew/ 放到自己的 Homebrew tap 的 Casks/ 目录
"""
import hashlib
import pathlib
import sys
import urllib.request

REPO = "DiXuanAoYi/ClearClip-Downloader"
HERE = pathlib.Path(__file__).parent

ASSETS = {
    "sha_win_setup": "ClearClip_{v}_x64-setup.exe",
    "sha_win_portable": "ClearClip-{v}-portable-windows-x64.zip",
    "sha_mac_arm": "ClearClip_{v}_aarch64.dmg",
    "sha_mac_intel": "ClearClip_{v}_x64.dmg",
}


def sha256_of(url: str) -> str:
    h = hashlib.sha256()
    with urllib.request.urlopen(url) as r:
        while chunk := r.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    v = sys.argv[1].lstrip("v")
    values = {"version": v}
    for key, name in ASSETS.items():
        url = f"https://github.com/{REPO}/releases/download/desktop-v{v}/{name.format(v=v)}"
        try:
            values[key] = sha256_of(url)
            print(f"{name.format(v=v)}: {values[key]}")
        except Exception as e:  # 某个平台没有发布时保留占位
            print(f"跳过 {name.format(v=v)}：{e}")
            values[key] = "<missing>"
    out = HERE / "dist"
    for src in [*HERE.glob("winget/*.yaml"), *HERE.glob("scoop/*.json"), *HERE.glob("homebrew/*.rb")]:
        text = src.read_text(encoding="utf-8")
        for k, val in values.items():
            text = text.replace(f"@@{k}@@", val)
        dest = out / src.parent.name / src.name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(text, encoding="utf-8")
        print("写入", dest)


if __name__ == "__main__":
    main()
