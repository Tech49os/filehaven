# UI 块① 补充修复：窗口四角露出直角（白边突出）

| 项 | 值 |
|---|---|
| 分支 | `feat/ui-apple-glass-v1` |
| 报告 | 用户截图红圈标注：窗口左上角有白色突出部分 |
| 根因 | `--r-window: 14px` 太小 + 带 `backdrop-filter` 的子层不被祖先圆角裁剪 |
| 修复 | 圆角提到 24px（效果图原值）+ 贴边材质层各自带对应圆角 |

## 1. 现象

用户在真机截图里圈出窗口左上角：**圆角外侧多出一圈白色/浅色带子，沿上边与左边走成直角轮廓**，没有跟随圆弧弯曲。四角都有，左上最明显（因为该处背后桌面是浅色）。

## 2. 定位过程（都做了实测，没有靠肉眼猜）

1. **放大 12 倍看清形状**：圆弧外有一圈亮带，边界是**直角**而不是圆弧 → 说明窗口的 `border-radius` 没有真正裁到内容。
2. **消融实验**：
   - 页面底色调成洋红 → 溢出像素跟着变洋红 ⇒ 溢出的是**页面自身内容**，不是桌面透出；
   - 关掉所有 `backdrop-filter` → 计数**完全不变** ⇒ 不是模糊层直接画出来的。
   - 去掉 `.win` 的 border → 计数不变 ⇒ 不是描边。
3. **扫描圆角半径临界点**（同一张截图、同一套判定）：

   | `--r-window` | 圆外残留 |
   |---|---|
   | 14px（原值） | 大量 |
   | 16px | 2386 |
   | 18px | 850 |
   | 20px | 186 |
   | 22px | 2 |
   | **24px** | **0** |

4. **结论**：`.side-rail`（22px blur）与 `.titlebar`（30px blur）带 `backdrop-filter`，在 WebView2 里会**独立合成**，祖先 `.win` 的 `border-radius + overflow:hidden` 裁不掉它们。它们是直角矩形，14px 的圆弧盖不住，于是四角露出直角；`clip-path` 也无效（实测）。圆角足够大（24px）才能把直角盖住。

## 3. 改动

| 文件 | 改动 |
|---|---|
| `src/styles/tokens.css` | `--r-window: 14px → 24px`，并把实测过程写进注释 |
| `src/components/AppWindow.vue` | `.side-rail` 加左上/左下圆角；`.titlebar` 加右上圆角；新增 `.no-side` 类与 `.win.no-side .titlebar { border-top-left-radius }` |
| `src/App.vue` | `.main`（挂 `.glass-panel`）加右下圆角 |

**为什么两个都做**：只把 `--r-window` 调大是"盖住"问题，一旦有人改小就复发；让每层自己带同半径圆角才是根治。两者叠加后，四角在 14px 时也能自我校正（虽然 `--r-window` 现在是 24px）。

**无侧栏场景**：引导页 `route.meta.sidebar=false`，不传 `#side` slot → 无 rail，标题栏从窗口最左开始。此时只有右上圆角，左上会露直角（真机实测 `borderTopLeftRadius=0px`）。已用 `.win.no-side` 补上。

## 4. RED → GREEN

| 阶段 | 结果 |
|---|---|
| 写「窗口圆角必须 ≥24px」后首跑 | **RED**：`expected 14 to be greater than or equal to 24` |
| 写「顶部两层材质各自带对应圆角」后首跑 | **RED**：`.side-rail` 无 `border-top-left-radius` |
| 写「无侧栏时标题栏左上也要有圆角」后首跑 | **RED**：找不到 `.win.no-side .titlebar` 规则 |
| 转绿后 | `ui-window.test.ts` **12/12 通过** |
| 全量 `npx vitest run` | **32 文件 / 300 测试全过**（基线 289 → 块① 8 → 圆角 3 → 共 300） |
| `npx vue-tsc --noEmit` | **0 错误** |
| 真机引导页 | `.win.no-side` 生效，标题栏 `borderTopLeftRadius` 实测 **24px** |
| 真机整理台 | 放大 12 倍确认左上角平滑圆弧，白色突出消失 |

## 5. 排查工具（留在仓库，可复跑）

| 脚本 | 用途 |
|---|---|
| `test/corner-alpha.py` | 按 alpha 通道量四角圆外像素（**注意**：CDP 截图会把透明区填白，对 Tauri 透明窗口不适用，留作参考） |
| `test/ascii-map.py` | 把截图局部打成亮度字符图，快速判断形状（圆弧 vs 直角） |
| `test/crop-zoom.py` | 裁剪并最近邻放大，纯标准库 |
| `test/shotp.py`（既有） | 从屏幕 DC 抠窗口，**判断圆角必须用这个**，CDP 截图不可靠 |

**判定圆角的正确方法**（这次踩过）：CDP `Page.captureScreenshot` 对 Tauri 透明窗口会把窗口外渲染成白色/花屏，无法判断圆角外是否干净；必须用 `shotp.py` 从屏幕 DC 抠图，窗口外是真实桌面像素。

## 6. 边界

1. 24px 是设计文档 §2 里效果图的原始值（原文写"实现可用 `--r-window: 14px`，取平台安全值"）。这次实测证明 14px 会露直角，因此**采用效果图原值 24px**，等于把该条"实现可降级"的让步收回。
2. 窗口最大化时圆角是否可见由 DWM 决定；当前 24px 在最大化下不显示圆角是正常行为。
3. 左侧彩色条纹是透明窗口背后桌面内容的毛玻璃采样，属预期（用户已裁决接受合成成本）。
4. 未跑 e2e 全量；本轮验证为 vitest 300 + vue-tsc 0 + 真机 CDP/截图。
