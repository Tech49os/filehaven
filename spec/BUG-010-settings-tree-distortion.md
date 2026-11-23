# BUG-010 设置页目录结构变多时布局畸变

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 10 条 |
| 确认结论 | **属实**：flex 列中的 Card 可被压缩，叠加默认全展开，目录变多时被挤成嵌套滚动/重叠 |
| 优先级 | P0（设置页被挤坏会阻塞其他配置） |
| 主要代码 | `app-v2/src/pages/settings/Settings.vue:20, 265, 306-329, 451-458`；`app-v2/src/components/Card.vue:17-50`；对照已修复的 `app-v2/src/pages/migration/PendingMigration.vue:361-370` |
| 测试落点 | `app-v2/src/pages/settings/Settings.test.ts`；CDP 量测脚本 |

## 1. 问题陈述

用户原话：「设置部分，在目录结构内容变多时，目录畸变严重。」

现状：
```css
/* Settings.vue:452-458 */
.grid { display:flex; flex-direction:column; overflow:auto; min-height:0; }
```
其内 `<Card title="目录结构">`（`:306-329`）作为 flex item 没有 `flex:0 0 auto`；而 `Card.vue:17-50` 的 `.card{min-height:0;overflow:hidden}`、`.content{min-height:0;overflow:auto}` 正好允许它被压扁。同类问题在 `PendingMigration.vue:361-370` 已用 `flex:0 0 auto` 修复并留有注释。

## 2. 目标 / 非目标

**目标**
- 目录节点数量增长时，设置页各卡片保持可读、互不重叠；目录树区域独立滚动。
- 默认只展开前两层（与 BUG-002 共用 helper），降低触发概率。

**非目标**
- 不改设置项内容与保存逻辑。
- 不新增目录编辑能力（BUG-003 负责体检入口）。

## 3. 行为规格

1. **Given** 目录树有 100+ 节点，**When** 渲染设置页，**Then** 所有 Card 高度由内容决定，不因 flex 容器空间不足被压缩。
2. **Given** 目录树超出可视高度，**When** 查看，**Then** 目录区域出现独立滚动条（`max-height` 受限），滚动只影响该区域，页面其他卡片不跟着抖动或重叠。
3. **Given** 用户进入设置页，**When** 目录树加载，**Then** 第 3 层及更深节点默认折叠（复用 BUG-002 的 `collapsedBeyondDepth`）。
4. **Given** 窗口高度变小（如 700px），**When** 渲染，**Then** 卡片之间矩形不相交，底部操作按钮仍可达。
5. **Given** 用户展开多个深层节点，**When** 目录区域滚动，**Then** 不发生嵌套滚动导致的「滚动不动/跳回」现象。

## 4. 变更点

- `Settings.vue`：`.grid > .card`（或对应 class）加 `flex:0 0 auto`；目录树容器加 `max-height:min(360px, 40vh); overflow:auto; min-height:0`。
- `Settings.vue:20` 的 `collapsed` 初始化改用 BUG-002 的 `collapsedBeyondDepth` helper。
- 参考 `PendingMigration.vue:361-370` 的既有修复写法，保持两处一致。
- 不在 `Card.vue` 全局强制 `flex:0 0 auto`（可能影响真正需要弹性伸缩的场景），只在设置页局部约束。

## 5. 验收标准（先写红测）

- [ ] `Settings.test.ts`：目录节点 >100 时，卡片容器带抗压缩 class；`collapsed` 含第 3 层节点 id。
- [ ] CDP 量测：注入 100+ 节点后，设置页各 Card 的 `getBoundingClientRect` 两两不相交；目录区域 `scrollHeight > clientHeight` 且可滚动。
- [ ] CDP 量测：窗口 1000×700 与 1440×900 两档均满足第 4 条。
- [ ] 回归：设置页保存/改名/词表管理操作全部通过。

## 6. 风险与边界

- `min-height:0` 只加给真正的滚动容器；给被压缩子项加会导致内容溢出画到别处（项目既有踩坑，待迁移曾中招）。
- 目录树默认折叠与 BUG-002 共用 helper，但两处状态各自独立，不要求跨页同步。
- 目录区域 `max-height` 用 `min()` 需确认 WebView2 版本支持；否则退回固定 px + 媒体查询。
