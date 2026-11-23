# BUG-005 已确认状态卡片显示扭曲、文字挤在一起

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 5 条 + 截图 3 |
| 确认结论 | 按用户裁决 **按未修复写完整规格**；说明：代码中已有 `flex:0 0 auto`（提交 `c215bf1d`，11-20），截图可能为修复前版本，实施前必须先复测核对，但不得因此省略本规格的加固项 |
| 优先级 | P0（确认结果是整理台的主要反馈） |
| 主要代码 | `app-v2/src/components/FileCard.vue:153, 297-315, 351, 391-395, 514-552, 841-853`；`app-v2/src/pages/workspace/Workspace.vue:438-469, 702-719, 743-755` |
| 测试落点 | `app-v2/src/components/FileCard.test.ts`；`app-v2/test/measure-card.py`（CDP 量测，注意作用域全路径） |

## 1. 问题陈述

用户原话：「已确认状态的卡片显示扭曲在了一起。」

截图 3 中整理台中央「已确认 2 份」区域，卡片内文字与绿色确认行挤在一起、行距消失。

现状证据与风险点：
- `Workspace.vue:751-755` 已有 `.mid :deep(.card){flex:0 0 auto}`，注释明确是为「已确认内容少、拉满会挤」而加。
- `FileCard.vue:391-395` `.hd` 用 `align-items:flex-start`，`:545-552` `.state` 用 `flex-wrap:wrap; justify-content:flex-end`——徽章多时会换行挤压标题。
- `:514-524` `.meta` 用 `white-space:nowrap; overflow:hidden`，长路径缺少 `min-width:0` 时可能顶破 flex 容器。
- 右侧已确认栏 `.drow`（grid）与 `.prow`（flex-column）本身无畸形写法，但受外层高度约束影响。

## 2. 目标 / 非目标

**目标**
- 已确认卡片在「本批列表」和「右侧已确认栏」中都保持文字完整、行距稳定、互不重叠。
- 长文件名、长路径、多徽章、窄窗口四种压力条件下仍稳定。

**非目标**
- 不改变已确认状态的判定逻辑与撤销链路。
- 不重做整理台整体布局（风格统一交给 BUG-008）。

## 3. 行为规格

1. **Given** 卡片进入 `confirmed` 状态，**When** 渲染，**Then** 卡片高度由内容决定：既不被 flex 拉伸铺满，也不被压缩（`flex:0 0 auto`，保留既有修复）。
2. **Given** 文件名很长，**When** 渲染，**Then** 文件名单行省略并显示 `title` 完整值，不得换成多行挤压徽章区。
3. **Given** 状态徽章数量多或窗口变窄，**When** 换行，**Then** 徽章区换行不得覆盖文件名与路径；标题与徽章之间保留最小间距。
4. **Given** 归档路径很长，**When** 渲染「已归到 <路径>」结果行，**Then** 最多两行后省略，`title` 保留完整路径。
5. **Given** 窗口宽度 1000 / 1280 / 1600 px，显示缩放 100% / 125% / 150%，**When** 渲染已确认卡片，**Then** 任意两个文本块的实际矩形（CDP `getBoundingClientRect`）不相交。
6. **Given** 右侧已确认条目数增加，**When** 列表变长，**Then** 行高恒定，滚动容器生效，不出现重叠或整体压缩。

## 4. 变更点

- `FileCard.vue`：`.hd` 改为 grid 或给标题/徽章区各自 `min-width:0`；`.state` 换行时增加行高与 `row-gap`；`.meta` 与 `.result` 补 `min-width:0` 和 `text-overflow:ellipsis`。
- `Workspace.vue`：保留 `.mid :deep(.card){flex:0 0 auto}`；检查右侧已确认栏滚动容器是否具备明确高度与 `overflow:auto`。
- 测距脚本用全路径选择器（如 `.mg > .foot`），避免同名 `.foot` 选错元素——项目已有踩坑。

## 5. 验收标准（先写红测）

- [ ] `FileCard.test.ts`：confirmed 状态下 className 含 `done`，候选区不渲染；长路径以省略形式出现且 `title` 为完整值。
- [ ] CDP 量测：三种窗口宽度 × 三档缩放下，卡片内文本块矩形两两不相交，行距 ≥ 基线值。
- [ ] CDP 量测：右侧已确认栏 10 条记录时行高与 2 条时一致。
- [ ] 截图对照：与截图 3 同一数据重现，绿条、文件名、路径三块区域清晰分离。

## 6. 风险与边界

- 实施前先跑一次当前版本复测：若 `c215bf1d` 已完全消除现象，则本 spec 降级为纯加固 + 回归测试，不再动布局。
- `min-height:0` 只给真正的滚动容器，不给被压缩的子项（项目既有踩坑：待迁移曾被压扁溢出）。
- 不要通过减少字号解决挤压——字号标准由 BUG-008 / BUG-009 统一。
