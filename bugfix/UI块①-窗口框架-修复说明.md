# UI 块① · Finder 式窗口框架 修复说明

> 分支：`feat/ui-apple-glass-v1`（从 BUG-011 分支 HEAD `0bdfb27` 切出）
> 范围：仅 `spec/PLAN_UI_APPLE_GLASS_v1.md` 的「块① 窗口框架 Finder 式改造」。
> 块②（浮层组件）、块③（页面级视觉）未处理；未切分支、未合并。

## 一、根因

BUG-011 落地的是「四层材质」，但窗口骨架仍是旧的纵向布局：

1. `.win` 为 `flex-direction: column`，标题栏横跨整窗、高 44px，灯的 `<i>` 只是装饰；
2. 窗口控制实际靠右上角 `.winbtns` 三个符号按钮，灯不可点；
3. Sidebar 自带 `.glass-side` 材质和 `200px/176px` 固定宽度（含窄窗 media query），
   与标题栏材质上下错位，无法做到「侧栏从 y=0 贯通到底、灯区与侧栏同一块玻璃」；
4. 没有集中的框架尺寸 token，侧栏宽/标题栏高散落在组件里，块②的浮层掩膜色也没有 token。

## 二、改动文件清单

| 文件 | 改动 |
|---|---|
| `app-v2/src/components/AppWindow.vue` | 横向 flex 骨架；新增 `side` slot + `.side-rail.glass-side`（贯通到底）；顶部 52px 灯区改为 3 个可点按钮；`.content` 内 52px `.titlebar.glass-title` + `#tail`；`.winbtns` 全部删除 |
| `app-v2/src/components/Sidebar.vue` | 根 `<aside class="side glass-side">` → `<div class="side">`；删自身 `width:200px/176px` 与 media query；导航项 `min-height 32→40`、圆角 12、选中态白 38% inset 描边；数量徽章 24×18 药丸 |
| `app-v2/src/App.vue` | `<Sidebar>` 通过 `#side` slot 传入；`.main.glass-panel` 留在默认 slot；删除多余 `.shell` 包裹；`#tail` 加「快速检索 Ctrl K」胶囊 |
| `app-v2/src/styles/tokens.css` | 新增 `--side-width:224px`、`--titlebar-h:52px`、`--c-mask:rgba(10,10,12,0.16)` |
| `app-v2/src/ui-window.test.ts` | **新建**：8 条源码契约 + mount 断言（RED 测试） |
| `app-v2/src/glass.test.ts` | 同步 1 条旧契约：`.glass-side` 由父级 rail 提供，Sidebar 根不再自带 |

未改动：`src/styles/glass.css`、`src/components/SearchPanel.vue`、router、任何 Rust / Tauri 配置、
`design/`、`spec/`、`.workbuddy/`。

## 三、关键决策

1. **`showSide` 判断方式：用 `$slots.side`，不加 prop。**
   `App.vue` 写 `<template v-if="showSidebar" #side><Sidebar /></template>`；
   引导页 `meta.sidebar=false` 时不提供该 slot，`AppWindow` 的 `v-if="$slots.side"` 使 rail 不渲染，
   标题栏自然横跨全宽。好处是 `showSidebar` 仍只有 App.vue 一个数据源，不会出现 prop 与 slot 不一致。
2. **检索胶囊触发方式：向 `window` 派发 `KeyboardEvent('keydown', { key:'k', ctrlKey:true })`。**
   `SearchPanel.vue` 已有挂在 window 捕获阶段的全局 Ctrl+K 监听，派发真实事件即可打开面板；
   比新增 `defineExpose`/模板 ref 更稳，也不用改属于块②的 SearchPanel。文案含 `Ctrl K`，
   激活态蓝描边按裁决留给块②，本块只保证可见可点。
3. **材质分工不回退**：rail 唯一挂 `.glass-side`，标题栏挂 `.glass-title`，主面板 `.main.glass-panel`
   在 App.vue 的默认 slot；`AppWindow` 的 `.body` 不再重复挂材质。
   `glass.css` 一行未改，`--glass-*` 参数、`html.no-glass` 与 `@supports` 双回退全部保留。
4. **拖拽热区**：`data-tauri-drag-region` 只放在整根 `.side-rail` 和 `.titlebar` 上；
   三个灯按钮是 `<button>` 且**不带**该属性，保证可点击/可聚焦（带 aria-label/title）。
5. **灯的位置**：灯区高 52px、`padding-left:22px` + `gap:8px` + 12px 圆点，
   左边缘依次为 x=22/42/62、上边缘 y=20，与标准一致；灯区与 rail 共用一块玻璃，
   不叠加材质、不加分隔线。
6. **不硬编码示意数字**：效果图的「38 份待整理」等仅为示意，本块未引入任何写死计数，
   侧栏徽章继续读 store 的真实数据。

## 四、RED → GREEN

先新建 `ui-window.test.ts` 并实际运行 `npx vitest run src/ui-window.test.ts`，
RED 结果：**7 失败 / 1 通过**（通过的是「无 side slot 时 rail 不渲染」，因旧代码压根没有 rail）。

实际失败项：

1. 三个灯是 button，分别调用 close / minimize / toggleMaximize
2. rail 与标题栏是拖拽区，灯按钮本身不是
3. rail 宽取 `--side-width`(224px)，标题栏高取 `--titlebar-h`(52px)
4. Sidebar 根不再自带 glass-side 与固定宽度/媒体查询
5. rail 唯一挂 glass-side，标题栏挂 glass-title，主面板由 App.vue 挂
6. 胶囊可见可点，文案含 Ctrl K，点击派发 Ctrl+K 键盘事件
7. `--c-mask` 已定义且 alpha 在 0.14~0.18

随后用最少代码改完四个业务文件，并同步 `glass.test.ts` 的 1 条旧契约断言；
新测试 **8/8 通过**，全量回归通过。`glass.test.ts` 的同步属于契约更新（旧断言要求 Sidebar 自带材质），
不涉及运行时逻辑变化。

## 五、验证

| 项 | 结果 |
|---|---|
| `cd app-v2 && npx vitest run` | **32 文件 / 297 测试全过**（基线 289 + 新增 8）；`Sidebar.test.ts` 5 条仍全过 |
| `cd app-v2 && npx vue-tsc --noEmit` | **0 错误**（exit 0） |
| `grep -rn 'winbtns\|data-tauri-drag-region' app-v2/src/components/AppWindow.vue` | 仅 3 行命中：rail、titlebar 各 1 处属性 + 1 行注释；**无 winbtns、灯按钮无拖拽属性** |
| `git diff --name-only -- glass.css SearchPanel.vue design spec` | 空，未触碰 |
| cargo / `cargo test` | **未跑**（本块为纯前端改动，按裁决不跑） |
| 真机 CDP 量测 / 截图 | **未跑**，由主代理执行；本文不声称做过 CDP 验证 |
| dev server | 未启动 |

## 六、边界与遗留风险

1. ~~`glass.css` 的 `.glass-side` 仍带 `border-right`~~ —— **主代理 CDP 复核后已修**：
   截图确认贯通侧栏右缘存在 1px 白线，与标准 §2「无竖向分割亮线」冲突。
   追加红测（`ui-window.test.ts`「侧栏无竖向分割亮线」，先跑见 RED：抓到材质态的 border-right），
   随后删除 `glass.css` 材质态与 `html.no-glass` 回退态两处的 `border-right`，复查 20/20 绿、
   vue-tsc 0。BUG-011 的四层 blur/saturate 与双回退未动，只去掉这一条线。
2. 测试为源码契约 + happy-dom mount，能证明 DOM/绑定/token 正确，**不能证明**真实窗口的
   拖拽手感、Tauri 拖拽区与灯点击的互斥行为、以及模糊层的实际观感，需真机 CDP 复核。
3. `SearchPanel` 的遮罩仍保留 `var(--c-mask, rgba(0,0,0,0.28))` 旧 fallback；
   新 token 已备好，替换 fallback 属于块②范围，未在本块处理。
4. 快速检索胶囊的激活态（蓝描边 + 10% 蓝底）按裁决属于块②，本块未实现。
5. 三键删除后，系统级快捷键（Alt+F4 等）仍可用；灯点击复用既有 `getCurrentWindow()` 与
   `isTauri` 守卫，纯 Vite dev 下点击不报错但无窗口行为。
