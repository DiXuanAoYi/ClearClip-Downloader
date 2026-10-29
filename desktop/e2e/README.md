# 界面测试

剪辑界面（`EditorTools.vue`）的自动化测试，不需要启动 Tauri：`ui-harness.html` 只挂载剪辑页，`lib.cjs` 在页面里注入一个假的 Tauri 后端（`invoke` 按命令名分发，用真的 `ffprobe` 读素材），再用 Chromium 操作界面。

```bash
npm run test:unit   # utils/edit.ts 的纯函数（布局、裁剪、波纹删除……），不需要浏览器
npm run test:e2e    # 起 vite，依次跑 ops.cjs / edit.cjs / region.cjs
npm test            # 两者都跑
node e2e/run.mjs ops.cjs   # 只跑一个脚本
```

需要：

- 本机有 `ffmpeg` 和 `ffprobe`
- Chromium：`npx playwright install chromium`；已有浏览器时用环境变量 `CHROMIUM_PATH` 指向它
- 端口 5199（vite）和 5301（素材服务）空闲

脚本说明：

| 文件 | 内容 |
| --- | --- |
| `ops.cjs` | 添加素材菜单、向左 / 向右裁剪、波纹删除、分离音频、剪辑点与逐帧、复制、应用到全部、最近的工程、吸附、缩放 |
| `edit.cjs` | 空白状态、添加 / 选择 / 分割 / 排序 / 裁剪、转场、文字、配乐、预览、导出、工程文件、素材丢失、英文界面 |
| `region.cjs` | 叠加素材的摆放和拖动、区域（马赛克 / 局部调色 / 跟随聚焦）、手动点、重新追踪 |

`media/` 里是测试用的小素材（几百 KB，直接提交，不现场生成）。新增界面功能时，在对应脚本里加 `ok('说明', 条件)`；用到新的后端命令，在 `lib.cjs` 的 `invoke` 里补一个分支。
