# BUG-002 归档目录默认折叠，只展示前两层

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 2 条 + 截图 2（浏览页树全展开，滚动条很长） |
| 确认结论 | **属实**：`collapsed` 初始为空集合，递归树无深度上限，且无批量折叠入口 |
| 优先级 | P1（直接影响浏览页可用性） |
| 主要代码 | `app-v2/src/pages/browser/Browser.vue:37-38, 57-62`；`app-v2/src/components/DirTree.vue:25-62`；`app-v2/src/pages/settings/Settings.vue:20` |
| 测试落点 | `app-v2/src/components/DirTree.test.ts`、`app-v2/src/pages/browser/Browser.test.ts` |

## 1. 问题陈述

用户原话：「归档目录内容很多，应默认折叠，只展示前两层。」

截图 2 中「归档目录」从第 1 层一直展开到深层节点，滚动条滑块很短，找目录困难。

现状：
```ts
// Browser.vue:38
const collapsed = ref<Set<number>>(new Set())  // 默认全部展开
```
```vue
<!-- DirTree.vue:52-59：递归渲染，只看 collapsed，不看深度 -->
<DirTree v-if="hasChildren(n) && !collapsed.has(n.id)" ... />
```

## 2. 目标 / 非目标

**目标**
- 浏览页打开时，第 1、2 层可见，第 3 层及更深节点默认折叠。
- 用户可逐节点展开/折叠，可一键「全部折叠到前两层」。
- 设置页目录树复用同一默认深度策略（与 BUG-010 联动）。

**非目标**
- 不改目录数据模型、不持久化折叠状态（刷新后回到默认策略）。
- 不改右侧文件列表行为。

## 3. 行为规格

1. **Given** 用户进入浏览页，**When** 目录树加载完成，**Then** 所有 `depth >= 3` 且有子节点的目录在 `collapsed` 集合中，界面只显示到第 2 层。
2. **Given** 用户点击某节点 caret，**When** 该节点原为折叠，**Then** 仅展开该节点子树；再次点击恢复折叠。
3. **Given** 已手动展开深层节点，**When** 点击「仅看前两层」，**Then** 折叠集合重置为初始策略，手动展开状态清空。
4. **Given** 用户从文件列表或其他入口选中深层目录，**When** 该目录不在可见层级，**Then** 展开其祖先链使其可见并高亮（可选增强；若实现成本过高，可降级为「选中后提示所在路径」）。
5. 折叠状态在本次页面会话内保持，刷新页面恢复默认。

## 4. 变更点

- 新增纯函数（建议 `app-v2/src/core/dir-tree.ts`）：`collapsedBeyondDepth(nodes: DirNode[], depth = 2): Set<number>`，递归收集超过深度阈值的有子节点 id。
- `Browser.vue` 初始化 `collapsed` 时调用该函数；设置页 `Settings.vue` 复用（与 BUG-010 同一 helper，避免两处逻辑漂移）。
- 浏览页树头部加「仅看前两层」按钮（沿用既有按钮样式，与 BUG-008 的尺寸 token 对齐）。
- `DirTree.vue` 保持受控组件语义，不改递归结构；`aria-expanded` 继续反映折叠状态。

## 5. 验收标准（先写红测）

- [ ] `dir-tree.test.ts`（新建）：3 层样例树，函数返回第 3 层节点 id；叶子节点不进集合；`depth=1` 边界正确。
- [ ] `Browser.test.ts`：加载树后断言 `aria-expanded="true"` 的 treeitem 只属于第 1、2 层。
- [ ] `Browser.test.ts`：点击 caret 后目标节点展开，点击「仅看前两层」后恢复默认。
- [ ] e2e：截图对照——打开浏览页不需滚动即可看到顶层全貌；CDP 读取 `role=treeitem` 数量与预期一致。

## 6. 风险与边界

- `DirNode.depth` 从 1 起（见 `index.rs`）；「前两层」按 `depth <= 2` 实现，不要用 0 起算。
- 若目录树只有 1 层，按钮点击应无可见变化，不报错。
- 折叠只影响展示，不改变 `browse_dir` 查询与已选目录。
