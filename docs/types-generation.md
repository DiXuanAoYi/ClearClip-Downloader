# 从 Rust 生成 TypeScript 类型

`desktop/src/types.ts` 以前全部是照着 Rust 结构手抄的（2600 多行），一个字段要改三处：Rust、`types.ts`、测试里的假后端。现在开始一个结构一个结构地改成由 Rust 生成。

## 怎么接入一个结构

1. Rust 里给结构加：

   ```rust
   #[derive(Serialize, Deserialize, ts_rs::TS)]
   #[serde(rename_all = "camelCase")]
   #[ts(export, export_to = "../../src/generated/")]
   pub struct Foo { … }
   ```

   它里面用到的其他结构也要加同样的派生；枚举同理（serde 的 `rename_all`、`tag`、`default` 等属性 ts-rs 会认）。
2. 在 `desktop` 目录运行 `npm run gen:types`（跑 `cargo test export_bindings`，把结果写到 `src/generated/`，并把 64 位整数的 `bigint` 改成 `number`，因为 Tauri 传的是 JSON 数字）。
3. 在 `types.ts` 里删掉手写的定义，改成 `import type { Foo } from './generated/Foo'` 再 `export type { Foo }`。其他文件继续从 `types.ts` 导入，不用改。
4. 提交 `src/generated/` 里的文件。CI 的“Generated TypeScript types are up to date”会重新生成并和提交的比较，不一致就失败。

## 现状

已接入：`model.rs` 里的 `MediaKind`、`Protocol`、`AssetKind`、`Asset`、`PlaylistEntry`、`SeriesInfo`、`MediaInfo`、`Clip`、`Chapter`（共 9 个）。其余仍是手写，对应关系记在 `types.ts` 顶部的注释里。

## 已知的限制

- 生成的字段是“必有”的：Rust 里 `#[serde(default)]` 的字段在 TypeScript 里是 `T`，不是 `T?`。界面代码里构造这类对象时要把字段写全（试点的两个类型 `vue-tsc` 没有报错）。
- 只生成数据结构，不生成命令的调用绑定（约 390 个命令仍按字符串名字调用）。命令绑定要不要用 tauri-specta 以后再评估。
- `serde_json::Value` 字段不要直接生成（会带出一个放在 `src-tauri/bindings/` 的依赖文件），用 `#[ts(type = "Record<string, unknown> | null")]` 之类指定（`Asset.extra` 就是这样）。
