# BUG-011 苹果毛玻璃风未实现（全局重做）

| 项 | 值 |
|---|---|
| 来源 | `bugfix/20261122_bug_feedback_001.docx` 第 11 条（「最重要的…苹果毛玻璃风没出来」） |
| 确认结论 | **属实**：`glass.css` 工具类未被任何组件引用，大面积实色覆盖；旧设计决策明确为性能放弃毛玻璃 |
| 用户裁决 | **全局苹果风重做，接受 WebView2 每帧合成的性能成本**（推翻 `tokens.css:26-30` 的旧性能取舍） |
| 优先级 | P0（用户自述最重要）／实施批次建议放视觉批次最后，避免与 BUG-008/009/010 打架 |
| 主要代码 | `app-v2/src/styles/glass.css:1-47`；`app-v2/src/styles/tokens.css:24-31`；`app-v2/src/components/AppWindow.vue:42-63`；`app-v2/src/App.vue:61-72` |
| 测试落点 | CDP 截图对比 + `getComputedStyle` 断言；对比度检测脚本 |

## 1. 问题陈述

用户原话：「最重要的我想要的苹果毛玻璃风没出来。」

现状：
```css
/* glass.css:6-10 / 20-26：定义了但没人用 */
.glass { background:var(--c-sidebar); backdrop-filter:blur(30px) saturate(180%); }
.glass-panel { background:rgba(255,255,255,.82); backdrop-filter:blur(24px) saturate(160%); }
```
全项目 grep 显示这些类只在 `glass.css` 内出现；`AppWindow.vue:47` `.win{background:var(--c-bg)}`、`:59` `.titlebar{background:var(--c-titlebar)}`、侧栏与面板全是实色。`App.vue:66` 虽设 `body{background:transparent}`，但被 `.win` 实色完全盖住。`tokens.css:26-30` 与 `glass.css:35-40` 的注释明确记录「为性能放弃毛玻璃」的旧决策。

## 2. 目标 / 非目标

**目标**
- 标题栏、侧栏、主面板、浮层统一半透明材质 + `backdrop-filter`，形成连贯的 Apple 风格（不是只在某个弹窗上贴一层）。
- 在引入材质的同时保证文字可读性（对比度 ≥4.5:1）与功能辨识度。
- 材质参数集中在 token，便于统一调参。

**非目标**
- 不做完整 macOS 窗口仿真（红绿灯按钮只是视觉装饰，行为按现有窗口控制）。
- 不引入图片壁纸资源；底层用渐变/纯色即可产生足够的模糊内容差异。
- 不降低 BUG-008/009 已定的字号与尺度标准。

## 3. 行为规格

1. **Given** 应用主窗口，**When** 渲染，**Then** 标题栏、侧栏、主内容区背景为半透明材质并带 `backdrop-filter: blur(...) saturate(...)`；其中至少两处模糊半径不同以形成层次。
2. **Given** 浮层/弹窗/命令面板，**When** 显示，**Then** 使用更高不透明度与阴影，文字底下有 `.glass-text` 底衬或等效对比保障。
3. **Given** 任意文本区域，**When** 材质生效，**Then** 正文与背景的对比度 ≥4.5:1（可用 CDP 取计算色后按 WCAG 公式校验）。
4. **Given** WebView2 不支持 `backdrop-filter`，**When** 渲染，**Then** 回退到实色方案（`@supports not (backdrop-filter: blur(1px))`），功能与可读性不降级。
5. **Given** 用户在低性能设备上运行，**When** 需要，**Then** 可通过设置项关闭材质回到实色（本项目的可选防御项；默认开启，用户裁决为全局重做）。
6. **Given** 窗口从 1000px 缩放到 1600px，**When** 材质渲染，**Then** 无接缝、无实色闪块、无滚动残影。

## 4. 变更点

- `tokens.css`：删除/改写「放弃毛玻璃」的旧注释，新增材质 token（如 `--glass-blur-title`、`--glass-blur-panel`、`--glass-bg-sidebar`、`--glass-bg-panel`、`--glass-border`）；保留色板变量供回退。
- `AppWindow.vue`、`Sidebar.vue`、`App.vue`、浮层组件：实际挂上材质类；确保 `.win` 不再用不透明 `--c-bg` 覆盖底层。
- `glass.css`：完善 `@supports` 回退与 `.glass-text` 底衬规则；列表卡片是否保留实色由 BUG-008 的条目规范决定，但整页仍处材质体系中。
- 设置页新增「材质效果」开关（可选，默认开），写入 settings 表。
- 旧测试截图基线需更新。

## 5. 验收标准（先写红测）

- [ ] CDP：`getComputedStyle` 断言标题栏/侧栏/主面板的 `backdropFilter` 非空且含 `blur`，背景 alpha < 1。
- [ ] CDP：正文区域抽样文字色与合成背景的对比度 ≥4.5:1。
- [ ] 截图：1000×700 / 1440×900 / 1600×900 三档窗口截图，无固定实色块突兀拼接。
- [ ] 回归：关闭材质开关后回退实色，所有交互与 BUG-008/009 的 computedStyle 断言仍通过。
- [ ] e2e 全量回归通过（材质不得遮挡按钮命中区域）。

## 6. 风险与边界

- 用户明确接受性能成本，但 WebView2 合成在旧机器可能明显掉帧；需做一次粗测并记录，不以此为阻断。
- 半透明叠加会改变既有颜色对比，必须逐页检查禁用态/错误态提示是否仍可辨。
- 不要把材质接到列表每一行（历史教训：`glass.css:35-40` 明确「列表卡片不用毛玻璃」，性能与可读性双输）；用户要的「全局风」指窗口框架层的连贯材质，而非逐行模糊。
- 与 BUG-008 同时实施时，先定字号/尺度再上材质，避免截图基线反复失效。
