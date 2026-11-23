# BUG-009 浏览页文件展示区字体偏小、不够美观

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 9 条 |
| 确认结论 | **属实**：字号 10–12.5px，低于项目已有 13px 可读性基线；另有重复死样式 |
| 优先级 | P1（可读性，改动小、收益直接） |
| 主要代码 | `app-v2/src/components/FileList.vue:94-152`；`app-v2/src/pages/browser/Browser.vue:562-642`（重复死样式）；`app-v2/src/styles/glass.css:42-47`（`.readable` 基线） |
| 测试落点 | `app-v2/src/components/FileList.test.ts`（随实现建）；CDP 量测脚本 |

## 1. 问题陈述

用户原话：「浏览功能中，文件展示区域，视觉上字体偏小，不够美观。」

现状：
```css
/* FileList.vue */
.row { height:32px; font-size:12.5px; }      /* :94-104 */
.cell.kind/.size/.time/.st { font-size:11.5px; }  /* :126-132 */
.tchip { font-size:10px; }                   /* :137-152 */
```
对照 `glass.css:42-47` 已定义的可读性底线「正文 ≥13px / 行高 1.6」。此外 `Browser.vue:562-642` 有一份重复的 `.list/.row/.cell/.tchip` 样式，因 scoped 属性不作用于子组件而成为死代码。

## 2. 目标 / 非目标

**目标**
- 浏览页文件行正文 ≥13px、行高 ≥1.6；次要列 ≥12px；标签 chip ≥11px。
- 行高由内容决定且不低于 36px，视觉密度与整理台文件条目一致（与 BUG-008 共用 token）。
- 删除重复死样式，避免两套标准。

**非目标**
- 不改变 6 列信息结构与排序、筛选行为。
- 不改文件打开/定位/右键菜单逻辑。

## 3. 行为规格

1. **Given** 浏览页文件列表，**When** 渲染，**Then** 文件名 computedStyle `font-size >= 13px`、`line-height >= 1.6`。
2. **Given** 类型/大小/时间/状态列，**When** 渲染，**Then** `font-size >= 12px`，不因列宽变窄继续缩小字号。
3. **Given** 标签 chip，**When** 渲染，**Then** `font-size >= 11px`，超长按省略处理而非缩字号。
4. **Given** 窗口宽度较窄，**When** 6 列放不下，**Then** 采用横向滚动或隐藏次要列（二选一，实施时定），不得通过压缩字号硬塞。
5. **Given** 用户打开文件/右键操作，**When** 字号调整后，**Then** 行为不变。

## 4. 变更点

- `FileList.vue` 字号与行高改引用 BUG-008 新增的 token（如 `--fs-file-name`、`--fs-meta`、`--fs-chip`、`--lh-file`），不写死像素。
- 删除 `Browser.vue:562-642` 重复样式块，避免后续改错文件。
- 窄窗策略在 spec 实施时明确并记录到 `tokens.css` 注释。

## 5. 验收标准（先写红测）

- [ ] `FileList.test.ts`：渲染后断言文件名/次要列/chip 使用 token 类而非硬编码字号。
- [ ] CDP：在 1000 / 1280 / 1600 px 窗口读取 `.row` 与文件名单元格的 `getComputedStyle`，满足第 1–3 条阈值。
- [ ] 静态检查：`Browser.vue` 不再含 `.tchip` / `.row` 重复样式块。
- [ ] e2e：浏览页打开、定位、右键重新归类全部回归通过。

## 6. 风险与边界

- 字号变大后 6 列在 1000px 宽度可能溢出；必须先定窄窗策略再改字号，避免横向滚动条破坏布局。
- 与 BUG-008 共用 token，若两者由不同批次实施，后实施者需回头核对前者已落位的数值。
- 不需要改后端 `browse_dir` 分页或返回字段。
