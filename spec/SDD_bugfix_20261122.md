# SDD：2026-11-22 用户反馈 bugfix 规格总览

> 来源：`bugfix/20261122_bug_feedback_001.docx`（11 条反馈 + 5 张截图）。
> 本目录只包含**规格**，不包含实施代码。按 superpowers 纪律：每条 spec 先写会红的测试，再实现；规格未确认前不动手。
> 确认方式：逐条对照 `app-v2` 前后端代码、截图与 git 历史（核对日期 2026-11-22，分支 `feat/p4`）。

## 规格清单

| 编号 | 文件 | 问题 | 确认 | 用户优先级 | 实施依赖 |
|---|---|---|---|---|---|
| BUG-001 | `BUG-001-summary-truncation.md` | 摘要截断且无展开入口 | 属实 | P0 | — |
| BUG-002 | `BUG-002-directory-default-fold.md` | 目录默认折叠前两层 | 属实 | P1 | helper 与 BUG-010 共用 |
| BUG-003 | `BUG-003-dir-health-scan.md` | AI 扫描错/重复目录 + 修正建议 | 属实 | P1 | 扫描底座与 BUG-006 共用 |
| BUG-004 | `BUG-004-tag-quality.md` | 标签伪词与 AI 弱标签 | 部分属实 | P1 | — |
| BUG-005 | `BUG-005-confirmed-card-layout.md` | 已确认卡片扭曲 | 按未修复写完整规格 | P0 | 与 BUG-008 尺度 token 对齐 |
| BUG-006 | `BUG-006-semantic-dir-merge.md` | 同语义目录合并建议 | 属实 | P1 | 复用 BUG-003 底座 + P4 recategorize/undo |
| BUG-007 | `BUG-007-workspace-delete.md` | 整理台删除功能 | 属实 | P0 | 复用隔离区三段式与批次撤销 |
| BUG-008 | `BUG-008-ui-consistency.md` | 全局 UI 风格不一致 | 属实 | P1 | token 供 BUG-005/009 引用 |
| BUG-009 | `BUG-009-browser-font-size.md` | 浏览区字体偏小 | 属实 | P1 | 依赖 BUG-008 的 token |
| BUG-010 | `BUG-010-settings-tree-distortion.md` | 设置页目录畸变 | 属实 | P0 | 复用 BUG-002 的 helper |
| BUG-011 | `BUG-011-glassmorphism.md` | 苹果毛玻璃未实现 | 属实 | P0（用户最重要） | 建议在 BUG-008/009/010 之后 |

## 用户裁决记录（写规格前已确认）

1. **文件组织**：每问题一个独立 spec 文件 + 本 SDD 索引。
2. **BUG-004 范围**：只治理 2-gram 伪词与 AI 弱标签；`Word` / `2026` / `2026-Q4` 是刻意设计的基础标签，保留。
3. **BUG-005**：虽然代码中已有 `flex:0 0 auto`（提交 `c215bf1d`，2026-11-20），仍按未修复写完整规格；实施前先复测，不得因此省略加固项。
4. **BUG-011**：用户明确选择**全局苹果风重做，接受 WebView2 每帧合成的性能成本**，推翻 `tokens.css:26-30` 的旧性能取舍。

## 实施批次建议

| 批次 | 内容 | 理由 |
|---|---|---|
| A · 硬缺陷 | BUG-001、007、010、005 | 影响判定正确性与核心操作，且改动边界相对清晰 |
| B · 可用性 | BUG-002、004、008、009 | 尺度 token 先落位（008），002/010 共用 helper 注意顺序 |
| C · 智能 | BUG-003、006 | 共用扫描/建议底座；LLM 与 ModelGate 调度需统一设计 |
| D · 视觉 | BUG-011 | 最后上材质，避免前序批次的截图基线反复失效 |

## 跨规格约束（不可违反）

- **技术栈**：Tauri 2 + Rust + Vue 3 + TypeScript + Vite，未经用户明确许可不得更换。
- **安全删除**：永不直删、永不覆盖、可撤销；隔离区布局固定为 `<归档根>/.filebutler/quarantine/<批次id>/<源区相对路径>`（BUG-007/006）。
- **启动路径**：任何同步 HTTP / 探活不得挂进 `refreshBoot()` 默认路径；模型调用走 `ModelGate`（BUG-003/006/011 涉及后台任务时）。
- **测量纪律**：UI 间距一律用 CDP 读 `getComputedStyle` / `getBoundingClientRect`，选择作用域全路径；不靠截图像素猜。
- **界面入口**：一次性配置页面必须保留后门入口。
- **测试基线**：当前 `feat/p4` 为 cargo 215 / vitest 208 / e2e 125 全过；每批实施后须保持全绿。

## 待实施前复测项

- BUG-005：当前版本是否已消除截图 3 的挤压（决定是完整修复还是纯加固 + 回归）。
- BUG-001：确认 200–700 字区间的样例文件在真机上确实无展开按钮（静态推导已明确，真机补证）。
- BUG-011：WebView2 当前版本的 `backdrop-filter` 支持度与粗测帧率。
