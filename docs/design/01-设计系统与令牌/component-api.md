# 组件 API 一览（desktop/src/components/ui/）

样式只用 `theme.css` 的 v2 令牌（`--cc-fs-* / --cc-r-* / --cc-sp-* / --cc-h-* / --cc-sh-* / --cc-dur-* / --cc-z-* / --cc-focus*`）。陈列页：`npm run dev` 后打开 `/ui-gallery.html`；测试：`e2e/ui-gallery.cjs`。

| 组件 | 属性 | 事件 / 插槽 | 备注 |
|---|---|---|---|
| CcButton | `kind` primary/secondary/text/danger；`size` s/m/l（28/36/44）；`iconOnly` + `ariaLabel`；`loading`；`disabled` + `disabledReason`；`type` | `click`；默认插槽 | 原生 `<button>`；loading 时 `aria-busy`、宽度不变、点击无效 |
| CcSegmented | `v-model`；`options[{value,label,disabled}]`；`size` s/m；`ariaLabel`；`disabled` | `update:modelValue` | `role=radiogroup`，漫游 tabindex，方向键/Home/End，跳过禁用项 |
| CcField | `label`；`desc`；`error`；`layout` row/col；`controlId` | 默认插槽 `{id, descId, errId, invalid}`；`label` / `desc` 插槽 | 说明最多 2 行，超出出现“了解更多/收起”（`aria-expanded`）；错误行 `role=alert` |
| CcSelect | `v-model`；`options`；`size`；`width` s/m/l/full；`placeholder`；`disabled`；`invalid`；`filterable`；`ariaLabel`；`id` | `update:modelValue`、`change` | 包 el-select；超过 8 项自动可搜索 |
| CcEmpty | `title`；`desc`；`compact` | 默认插槽（操作）、`art`、`desc` | 空状态：图 + 标题 + 一句说明 + 操作 |
| CcSkeleton | `width`；`height`；`radius`；`circle`；`lines`；`label` | — | `role=status`；reduced-motion 下不动 |
| CcDialog | `v-model`；`title`；`size` sm 400/md 560/lg 720/full；`danger`；`initialFocus`；`closeOnClickModal` | `update:modelValue`、`closed`；默认、`footer` 插槽 | 窄屏 `min(档宽, 100vw-32px)`；lg 正文内部滚动；危险确认让初始焦点落在“先不…” |
| CcLink | `href`；`external`；`disabled` | `click` | 外链走现有 `open_url`，带图标和读屏提示 |
| CcToast / `notify()` | `notify({kind, title, desc, detail, onRetry, duration})`；`notifyError(title, extra)`（`src/utils/notify.ts`） | — | 复用 ElMessage；错误不自动消失、`role=alert`，可“重试 / 查看详情（可复制）” |
