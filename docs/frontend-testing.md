# 前端测试

`desktop/` 前端有三层测试，都在 `desktop/` 目录下运行：

| 命令 | 内容 |
| --- | --- |
| `npm run test:unit` | `e2e/unit/*.test.mjs`：node 自带测试运行器，纯函数和扩展逻辑，不需要浏览器 |
| `npm run test:component` | `tests/component/*.test.ts`：vitest + @vue/test-utils + jsdom，测 pinia stores、api 层（mock `invoke`）、`Cc*` 组件（渲染 / 事件 / 无障碍属性）、`useTauriEvent`、`progressBatch` |
| `npm run test:e2e` | Chromium 里用模拟后端跑整个界面（见 `e2e/README.md`） |

其它：

- `npm run test:coverage`：跑组件测试并打印覆盖率文字摘要（只统计 api / stores / 组合函数 / 进度合并 / `components/ui`，没有硬阈值）。
- `npm run check:commands`：命令契约检查。解析 `src/api/*.ts` 里 `invoke('xxx')` 的命令名和 `src-tauri/src/lib.rs` 的 `generate_handler![]` 列表，前端调用了 Rust 没注册的命令就失败（只解析源码，不需要编译 Rust）。
- `npm run test:component:watch`：改文件自动重跑。

写组件测试的约定：

- 文件放 `tests/component/`，命名 `*.test.ts`（不在 `src/` 里，不参与 `vue-tsc` 和 `vite build`）。
- 用 `vi.mock('@tauri-apps/api/core')` / `'@tauri-apps/api/event'` 替换后端；不要连真实后端。
- jsdom 缺的浏览器接口（`matchMedia`、`ResizeObserver`）在 `tests/component/setup.ts` 里补。
- 已知的产品缺陷用 `it.fails` 记录（修好后会提示改成普通 `it`），不要为了测试改产品行为。

CI：`desktop-ci` 的 `frontend-component` 任务运行命令契约检查和带覆盖率的组件测试；它是独立的并行任务，目前不在必需检查里。
