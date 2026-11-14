# 清影界面设计稿

- `source/`：设计稿源文件。`*.dc.html` 是 Claude 设计画布里的页面（一个文件一张图），`clearclip.css` 是共用样式，`canvas.json` 是画布上每张图的位置和大小。
- `gen/`：生成这些 `.dc.html` 的 Python 脚本（`build.py` 是入口，`lib.py` 是小部件，`pages_*.py` 是各页内容）。改内容改这里，再重新生成。
- `png/`：每张图的深色渲染图（Chromium，1360 宽）。`../design-reference.html` 把它们排成一页。

程序里的实现：`desktop/src/styles/theme.css`（令牌）、`desktop/src/styles/skin.css`（Element Plus 控件和共用版式）。

## 设计系统 v2（待确认，尚未实现）

- `source/V2-*.dc.html` + `source/v2.css`：18 张新画板（总览、实施清单、设计系统 2 张、组件 2 张、C1–C12 页面）。`v2.css` 里带 NEW 标记的是建议新增的 `--cc-*` 令牌。
- `gen/v2lib.py`、`v2_spec.py`、`v2_pages.py`：生成脚本；`build_v2.py [heights.json]` 只写 `V2-*` 画板并追加进 `canvas.json`，不动旧画板；`measure_v2.cjs`（Playwright）量高度并截图到 `shots/`。
- `png/V2-*.png`：各画板整图（深浅色都在图里）。
