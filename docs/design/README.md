# 清影界面设计稿

- `source/`：设计稿源文件。`*.dc.html` 是 Claude 设计画布里的页面（一个文件一张图），`clearclip.css` 是共用样式，`canvas.json` 是画布上每张图的位置和大小。
- `gen/`：生成这些 `.dc.html` 的 Python 脚本（`build.py` 是入口，`lib.py` 是小部件，`pages_*.py` 是各页内容）。改内容改这里，再重新生成。
- `png/`：每张图的深色渲染图（Chromium，1360 宽）。`../design-reference.html` 把它们排成一页。

程序里的实现：`desktop/src/styles/theme.css`（令牌）、`desktop/src/styles/skin.css`（Element Plus 控件和共用版式）。
