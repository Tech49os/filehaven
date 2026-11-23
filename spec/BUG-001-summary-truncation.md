# BUG-001 文档摘要显示不全、无法滚动、没有展开入口

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 1 条 + 截图 1（整理台待确认卡片红框） |
| 确认结论 | **属实**：前端 CSS 裁到 4 行，后端 700 字才标记 `truncated`，中间区间既无展开按钮也不可滚动 |
| 优先级 | P0（摘要是归档决策依据，看不到等于盲判） |
| 主要代码 | `app-v2/src/components/FileCard.vue:282-294, 763-773`；`app-v2/src-tauri/src/preview.rs:20-24, 108-119`；`app-v2/src/core/ipc.ts:113-121` |
| 测试落点 | `app-v2/src/components/FileCard.test.ts`（已有用例 `:79-88`） |

## 1. 问题陈述

用户原话：「文档摘要文本显示不全，详见截图部分，内容无法正常展示，且无法滚动，没有按钮展示更多。」

截图 1 中红框内的正文在 4 行处被切断，卡片上没有任何滚动条或展开按钮。静态核对证实：

```css
/* FileCard.vue:763-768 */
.pv-text { display:-webkit-box; -webkit-line-clamp:4; overflow:hidden; }
```
```rust
// preview.rs:24 / :112
pub const PREVIEW_CAP: usize = 700;
let truncated = normalized.chars().count() > PREVIEW_CAP;
```
按钮 `v-if="preview.kind === 'text' && preview.truncated"`（`:288`）。4 行约容纳 150–200 字，落在 200–700 字区间的文本已被裁切，但 `truncated=false`，按钮不渲染，非展开态也没有 `overflow:auto`。

## 2. 目标 / 非目标

**目标**
- 任何被 CSS 裁掉的正文，都必须有明确、可达的查看方式（展开或滚动）。
- 展开按钮的出现条件与「实际是否溢出」一致，不与后端字数阈值绑定。

**非目标**
- 不改后端 `PREVIEW_CAP=700`、不改正文抽取与深读逻辑。
- 不改图片预览和「读不出的原因」两种分支。

## 3. 行为规格

1. **Given** 文本预览已渲染，**When** 元素实际内容高度大于可见高度（被裁），**Then** 显示「展开全文」按钮。
2. **Given** 文本未溢出（哪怕后端 `truncated=true`），**Then** 不显示展开按钮，避免误导。
3. **Given** 处于展开态，**When** 点击「收起」，**Then** 回到 4 行裁切态；展开态允许在 `max-height` 内滚动到底。
4. **Given** 切换当前文件/换一批，**When** 新卡片挂载，**Then** 展开态复位为收起，不残留上一份文件的状态。
5. 图片预览与读不出原因分支保持现状。

## 4. 变更点

- `FileCard.vue`：给 `.pv-text` 加模板 ref，`onMounted` + `nextTick` 后比较 `scrollHeight > clientHeight`；监听窗口 resize / 容器宽度变化（`ResizeObserver`）重算；用本地 `textOverflowed` 状态控制按钮显示（与 `preview.truncated` 取或，但以实测为准）。
- 单测环境 happy-dom 无真实布局：测试通过 mock `scrollHeight` / `clientHeight` 描述符驱动，不依赖真实排版。
- 不改 `preview.rs`；`FilePreview.truncated` 继续表示「后端还有更多正文」，可在 UI 上作为附加说明但不再是按钮的唯一条件。

## 5. 验收标准（先写红测）

- [ ] `FileCard.test.ts`：mock 溢出时按钮存在；不溢出时按钮不存在（即使 `truncated=true`）。
- [ ] `FileCard.test.ts`：点击展开后 class 含 `full`，文案变「收起」；再点恢复。
- [ ] `FileCard.test.ts`：换文件后展开态复位。
- [ ] `cargo test` 原有 preview 用例保持通过（700 字契约不变）。
- [ ] e2e：打开整理台，选一份 300–500 字正文的文件，断言红框区域可见「展开全文」，点击后可见末尾文字。

## 6. 风险与边界

- `ResizeObserver` 在 happy-dom 不存在，需在组件内做存在性判断，避免单测崩溃。
- 展开态 `max-height:220px` 可能仍不够长文本，必须保证可滚动（不是再次裁掉）。
- 首屏批量渲染 12 张卡片时测量开销需可接受；只在文本预览卡片上挂监听。
