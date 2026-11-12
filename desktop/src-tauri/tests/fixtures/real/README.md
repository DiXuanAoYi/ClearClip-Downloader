# 真实响应样本

这里放从真实网站录下来的响应（已脱敏），解析代码的测试用它们，比手写的样本更接近网站现在的样子。

怎么加：设置里打开“录制样本”，解析一次出问题的链接，然后

```bash
cd desktop
node scripts/import-sample.mjs <数据目录>/samples/<平台>/<样本>.txt [名字]
```

脚本会去掉手机号、邮箱、内网地址，写出 `<平台>-<名字>.json|html` 和 `.meta.json`；
再在对应平台的解析代码里 `include_str!("../../tests/fixtures/real/…")`，写一个断言解析结果的测试。
`cargo test` 里的 `real_fixtures_are_clean` 会检查这个目录里没有 Cookie、令牌、手机号、邮箱，JSON 样本能解析。
