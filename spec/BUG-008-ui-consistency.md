# BUG-008 全局 UI 风格不一致（待定区 vs 整理台待迁移）

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 8 条 |
| 确认结论 | **属实**：字号、内边距、底色、图标在各组件硬编码；token 只统一颜色，未统一尺寸/排版 |
| 优先级 | P1（一致性问题，影响整体观感与信任） |
| 主要代码 | `app-v2/src/styles/tokens.css`；`app-v2/src/components/FileCard.vue:360-416, 646-681`；`app-v2/src/pages/parked/Parked.vue:232-299`；`app-v2/src/pages/migration/PendingMigration.vue:372-422`；`.btn` 重复定义于 FileCard/Workspace/Settings/PendingMigration/Browser |
| 测试落点 | 组件单测 + CDP `getComputedStyle` 对比脚本 |

## 1. 问题陈述

用户原话：「全局 UI 风格不一致，待定区明显与整理台待迁移不一致，要保持一致性。」

核对到的具体差异：

| 维度 | 整理台待确认卡 | 待定区行 | 待迁移 |
|---|---|---|---|
| 底色 | `--c-surface` (#fff) | `--c-field` (#f5f5f7) | `--c-surface` |
| 内边距 | `20px 22px` | `12px 14px` | `9px 14px` / `4px 6px` |
| 文件名 | `15px / 600` | `13px / 500` | `12px` |
| 摘要 | `12.5px / 1.7` | `12px / 1.6` | — |
| 标签 chip | `11px / max 180px` | `10.5px / max 150px` | `10px` |
| 图标 | 34px 圆角盒 | 14px 裸图标 | 14px folder |
| 间距 | `gap:12px` | `gap:6px` | `gap:10px` |

`.btn` 在 5 个组件里各写一遍，是风格漂移的结构性来源。

## 2. 目标 / 非目标

**目标**
- 文件条目在整理台、待定区、待迁移页使用同一套视觉规范：字号、行高、间距、底色、圆角、图标尺寸。
- 尺寸与排版尺度进入 `tokens.css`，新组件优先引用 token，不再硬编码。

**非目标**
- 不重排功能入口、不改变任何按钮行为与权限。
- 不调整信息层级（待确认仍可展示摘要与候选，待定区仍展示备注与捞回）。
- 毛玻璃材质由 BUG-011 负责，本 spec 只管尺度与色彩一致性。

## 3. 行为规格

1. **Given** 任一文件条目视图，**When** 渲染，**Then** 文件名、元信息、标签 chip 的 `font-size` / `line-height` 取自统一 token，三处 computedStyle 一致。
2. **Given** 待定区文件行，**When** 与整理台待迁移行并排比较，**Then** 卡片底色、边框、圆角、左右内边距一致（待定区从灰底大行改为规范条目）。
3. **Given** 任一主操作按钮，**When** 渲染，**Then** padding / 圆角 / 字号来自共享样式而非各组件私有定义；5 份 `.btn` 重复定义收敛为 1 份。
4. **Given** 用户增删标签、捞回、勾选，**When** 操作，**Then** 功能与当前完全一致（无回归）。

## 4. 变更点

- `tokens.css` 新增尺寸/排版 token：如 `--fs-file-name`、`--fs-meta`、`--fs-chip`、`--lh-file`、`--pad-row-x/y`、`--gap-row`、`--r-item`。
- 抽取共享条目样式/组件（建议 `src/components/FileRowShell.vue` 或全局 `file-row` 类），待定区、待迁移、整理台卡片统一引用。
- 清理 5 处重复 `.btn`，收敛到全局样式；组件内只保留场景特化覆盖。
- BUG-009 的字号提升在本 spec 的 token 中一并落位，避免两处标准打架。

## 5. 验收标准（先写红测）

- [ ] 组件单测：三处文件条目使用同一 token 类名；待定区改名/捞回/勾选行为不变。
- [ ] CDP 对比：整理台、待定区、待迁移各取一条文件条目，读取 `getComputedStyle` 的 `fontSize` / `lineHeight` / `padding` / `backgroundColor` / `borderRadius`，差异在约定阈值内。
- [ ] 静态检查：`grep -r "\.btn\s*{" src/components src/pages` 只剩全局定义（允许 1 处）。
- [ ] 回归：现有 vitest / e2e 全绿（不减少任何操作入口）。

## 6. 风险与边界

- 测量必须用全路径选择器（如 `.mg > .foot`），项目曾因同名 `.foot` 选错元素白追一轮。
- 用户说「间距不对」时先量再改：用 `test/measure-card.py` 读 `getComputedStyle` / `getBoundingClientRect`，不要拿截图像素猜（历史猜错 17px）。
- 一致性改造不要顺手改交互顺序，避免破坏肌肉记忆与 e2e 断言。
