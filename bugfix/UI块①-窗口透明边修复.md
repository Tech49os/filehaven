# UI 块① · 窗口四周 8px 透明边修复

分支：`feat/ui-apple-glass-v1`
前置：块①窗口框架（`6aa2a12`）、窗口圆角修复（`c94d380`）

## 一、现象

用户反馈「还是有透明的边，很明显」。放大窗口左边缘 8 倍后可见：窗口内容外侧有一条约 8px 的灰色渐变带（外侧 `227,227,227` → 内侧 `172,172,172`），上下右同理（上方 1px）。

## 二、根因

不是 CSS 问题，是**窗口矩形比 WebView2 客户区大一整圈**：

| 量测项 | 实测值 |
|---|---|
| `GetWindowRect` | 1176 × 749 |
| `GetClientRect` | 1160 × 740 |
| 外扩 (左/上/右/下) | **8 / 1 / 8 / 8** |

`tauri.conf.json` 里 `"shadow": true`。tao 为了在无边框窗口上画 DWM 投影，把窗口各向外扩 8px，阴影画在这圈上；而 WebView2 只覆盖客户区。窗口是 `transparent: true`，这圈没有内容，就直接显示为桌面 + 阴影的灰色过渡 —— 也就是用户看到的「透明的边」。

> 上一轮把 `--r-window` 提到 24px，只解决了四角的直角露出，没有动这条边。两个问题叠加，这条边一直都在。

## 三、排除过程（全部运行时实测，未改代码）

| 实验 | 结果 |
|---|---|
| `DwmExtendFrameIntoClientArea(MARGINS{0,0,0,0})` | 外扩不变 (8/1/8/8) |
| 移除 `WS_THICKFRAME` + `SWP_FRAMECHANGED` | 外扩不变 |
| 整套样式换 `WS_POPUP` | 外扩不变 |
| `DwmSetWindowAttribute(NCRENDERING_POLICY, DISABLED)` | 外扩不变，仅 `ExtendedFrameBounds` 变成整个窗口 |

四种 Win32 手段都改不动，说明外扩是**窗口创建时**由 tao 决定的，运行时改样式无效。

## 四、改动

1. `src-tauri/tauri.conf.json`：`"shadow": true` → `"shadow": false`
2. `src/ui-window.test.ts` 新增契约测试：断言 `transparent === true`、`decorations === false`、**`shadow === false`**，注释写明 8px 来源，防止以后有人为「想要投影」改回去。

红测验证：临时改回 `true` 跑一次 → `expected true to be false` 失败；改回 `false` → 通过。

## 五、验证

**几何**（真机，重启实例后）

| 项 | 修复前 | 修复后 |
|---|---|---|
| WindowRect | 1176×749 | **1160×740** |
| ClientRect | 1160×740 | 1160×740 |
| pad | (8,1,8,8) | **(0,0,0,0)** |
| 截图尺寸 | 1176×749 | **1160×740** |

**像素**（真实屏幕截图 `edge-fixed.png`）

- 左边缘：`x=0` 为 1px 抗锯齿过渡色，`x=1` 起即内容色 `232,239,249` —— 8px 渐变带消失
- 上边缘：`y=0` 过渡，`y=1` 起内容色
- 右/下边缘：同样只剩 1px 过渡

**视觉**：左上角放大 10 倍为平滑圆弧，圆弧外直接是桌面，无灰/白色带子；左边缘竖直段只剩 1px 描边。

**回归**：vitest 全量、`vue-tsc --noEmit` 见提交时输出。

## 六、边界与取舍

1. **窗口失去 DWM 投影**。`shadow:false` 后窗口边缘直接切桌面，没有外发光过渡。这是消除透明边的直接代价，与「边缘必须干净」的要求一致。
2. **缩放能力未丢**：样式仍为 `0x14CF0000`（含 `WS_THICKFRAME`），Tauri 的 `WM_NCCALCSIZE` 处理保留边缘命中区，鼠标仍可从边缘拖拽改大小；绿键最大化/还原不受影响。
3. 本次只改配置 + 测试，未动任何 CSS 材质规则（BUG-011 参数、`html.no-glass` 与 `@supports` 双回退保持原样）。

## 七、可复用工具

- `app-v2/test/shotp.py`：从屏幕 DC 抠窗口，透明窗口下唯一可信的截图方式（CDP 截图会把透明区渲染成白色）
- `app-v2/test/crop-zoom.py`：裁剪 + 整数倍最近邻放大，看边缘层次
- `app-v2/test/ascii-map.py`：亮度字符图，快速判断角/边形状
