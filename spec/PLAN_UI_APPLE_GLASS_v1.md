# 实施计划：Apple 毛玻璃 UI 标准 v1.0 落地

> 来源：`design/UI设计标准_苹果毛玻璃_v1.0.md` + 8 张效果图 + 用户裁决（2026-11-23）
> 分支：`feat/ui-apple-glass-v1`（从 `bugfix/BUG-011-glassmorphism` HEAD `0bdfb27` 切出）
> 纪律：每块红→绿 TDD、独立提交、单独说明文档；每块完成后停下给用户真机验证。

## 用户裁决（写码前锁定）

1. **窗口灯**：红=关闭、黄=最小化、绿=最大化/还原，三者可点击。删掉右上三键。保留系统快捷键（Alt+F4 等）。
2. **交付节奏**：分三块提交，每块给用户验一次，确认后再进下一块。
3. **效果图示意数字**（38 份待整理、1284 份已归档等）不硬编码，显示真实数据。
4. 材质开关、`@supports` 回退、对比度红线、BUG-008/009 字号尺度全部继承，不回退。

## 块① 窗口框架 Finder 式改造

**目标文件**：`src/components/AppWindow.vue`、`src/components/Sidebar.vue`、`src/App.vue`、`src/main.ts`（如需新增样式）、`src/styles/tokens.css`（`--c-mask` 等）、新建 `src/ui-window.test.ts`。

**改动清单**：
1. `AppWindow.vue` 模板重排为横向 `.frame`：
   - 左侧 `.side-rail.glass-side`（224px，从 y=0 贯通到底），内含 52px 灯区 + `<slot name="side">`
   - 灯改为三个 `<button class="lamp r/y/g">`（可点击、可聚焦、aria-label），复用现有 `onClose/onMin/onMax`
   - 右侧 `.content` 内含 `<header class="titlebar glass-title">`（52px，只盖内容区，标题居中，无三键）+ `<slot name="tail">` + `<main class="body glass-panel">`
   - 拖拽热区：`.side-rail` 灯区 + `.titlebar` 均带 `data-tauri-drag-region`；灯按钮上单独排除拖拽
2. `Sidebar.vue`：根元素从 `<aside class="side glass-side">` 改为内部容器（不再自带材质与固定宽度，由父级 `.side-rail` 统一提供）；删掉自身 `width:200px/176px` 与 `border-right`；导航项 `min-height:32→40px`、圆角 12、选中态补白 38% 描边；数量徽章改 24×18 药丸。
3. `App.vue`：把 `<Sidebar>` 塞进 AppWindow 的 `#side` slot，`.main` 留在默认 slot；右上角 `#tail` 放「快速检索 ⌘K」胶囊（点击打开 SearchPanel）。
4. `tokens.css`：新增 `--c-mask: rgba(10,10,12,.16)`（标准 14~18%），替换 SearchPanel 的 fallback。
5. 保持 `tauri.conf.json` 的 `decorations:false / transparent:true` 不变（纯前端 DOM 改造，无需动 Rust）。

**RED 测试**（`ui-window.test.ts`，源码级契约 + 组件挂载）：
- 灯是 button 且分别绑定关闭/最小化/最大化回调，无 `winbtns` 三键
- `.side-rail` 与 `.titlebar` 均带拖拽属性，灯按钮不在拖拽区内
- 侧栏宽度 224px、标题栏 52px（从源码/CSS 变量断言）
- Sidebar 根不再自带 glass-side、固定宽度
- 快速检索胶囊存在且写着 Ctrl K
- `--c-mask` 已定义且 alpha 在 0.14~0.18

**GREEN**：最少代码转绿，保留既有导航/路由/材质开关行为。

**验证**：`npx vitest run` + `npx vue-tsc --noEmit`；真机 CDP 量侧栏/标题栏矩形、灯可点击（CDP click 后窗口尺寸变化或进程退出需谨慎——关窗测试放最后）、三档截图。**停下给用户验**。

## 块② 全局浮层组件升级

**目标文件**：`SearchPanel.vue`、`Browser.vue`（右键菜单）、新建 `ConfirmDialog.vue`、`Settings.vue`（开关）、相关 test。

1. **Ctrl+K 面板**：宽 560→600px；输入 14→17px；结果分「最佳匹配 / 目录」两组；底部状态栏左「N 个结果」右「↑↓ 选择 · ↵ 打开 · Esc 关闭」；遮罩用 `--c-mask`。
2. **右键菜单**：`.menu` 挂 `glass-float`，宽 224px，圆角 12；hover 蓝实底白字；「移入隔离区」红 `#FF3B30`；右侧快捷键（打开 O / 定位 L / 重新归类 R / 移入隔离区 Del）；分组 1px 分隔线。
3. **重新归类确认弹窗**：新建 `ConfirmDialog.vue`（600px、玻璃浮层、压暗 16%、标题 18 SemiBold → 副标 12 → 分隔线 → 三步胶囊「复制到新目录 → SHA-256 校验 → 旧副本入隔离区」→ 安全说明 → 取消/确认右对齐）；Browser.vue 点目录先弹窗、确认后才调 `doRecat`。
4. **材质开关**：30×18 胶囊，开=主色、钮 14px 白右偏 2px。

**RED 测试**：菜单项文案/快捷键/危险色、弹窗结构与「确认后调用 recategorize」、面板分组与状态栏、开关尺寸。

**验证**：vitest + vue-tsc + CDP（Ctrl+K 面板、右键菜单、重新归类弹窗截图与量测）。**停下给用户验**。

## 块③ 页面级视觉组件

**目标文件**：新建 `SafetyCard.vue` / `AiSummaryCard.vue` / `ProgressBar.vue` / `StatusBadge.vue`；改 `Workspace.vue`、`FileCard.vue`、`Browser.vue`、`PendingMigration.vue`、`Parked.vue`、`Settings.vue`、`tokens.css`（`--r-card-lg:18px`）。

1. **安全保证卡**：success 6% 底 + 10% 描边、标题 12 SemiBold `#248A3D`、行 11px `#3A3A3C`；四条承诺固定文案（撞名 `_2`、隔离区可找回、按批次撤销、校验失败自动暂停）；放迁移页与设置页。
2. **AI 摘要卡**：indigo 7% 底 + 10% 描边、圆角 13~14、AI 星芒 + 「AI 摘要」12 SemiBold `#575AD8` + 正文 12；双路分歧标题写「AI 摘要 · 双路不一致」；替换 FileCard / Parked / Browser 详情条的平铺 summary。
3. **进度条**：高 5px、全圆、底轨 `#E5E5EA`、填充主色；替换迁移页 `.prog` 文本与设置页文本进度。
4. **全圆语义徽章**：`--r-badge:13px` 或全圆；绿/蓝/橙/红语义底 10~12% + 深色文字；状态词按标准固定。
5. **页面大卡圆角 18**：新增 `--r-card-lg`，只给页面级主卡/队列卡，不动控件级。

**RED 测试**：四组件存在性与 props、各页面实际挂载、文案完整、圆角/颜色走 token。

**验证**：vitest + vue-tsc + cargo（若未动 Rust 则跳过）+ CDP 全场景截图。**停下给用户验**。

## 收尾

三块全绿后：全量 vitest / vue-tsc / cargo、CDP 三档窗口与对比度、逐块说明文档核对、汇总交付清单。

## 已知边界

- 不改 Tauri 窗口配置与 Rust；灯的控制走现有 `getCurrentWindow()` 调用。
- 每个 CDP 量测都用全路径选择器（项目踩过同名 `.foot` 的坑）。
- 真机 e2e 全量不声称通过，除非实际跑过。
