## 动机

<!-- 为什么要改：解决什么问题、对应哪个 Issue -->

## 改动

<!-- 改了什么（按文件或功能列出） -->

## 验证

<!-- 跑了哪些命令、结果如何；哪些没验证（真实网站、真实 Windows、真实浏览器里的扩展）要明说 -->

- [ ] `cd desktop && npx vue-tsc --noEmit && npm run test:unit`
- [ ] 相关界面测试脚本（`node e2e/run.mjs <脚本>.cjs`）
- [ ] Rust 改动：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`
- [ ] 用户可见的变化已写进 `desktop/CHANGELOG.md`

## 风险

<!-- 可能影响的地方、回滚方式 -->

## 截图

<!-- 界面改动请附前后对比；没有就删掉这一节 -->
