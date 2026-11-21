# 项目长期约定（文件管家 / 本地智能文件归档客户端）

## 当前阶段：P3 全部完成，已出包 v0.5.0-alpha.1（2026-11-21，分支 `feat/p3`）

P1（骨架闭环）→ P2（推荐变准，v0.4.0-alpha.1）→ **P3（会读会查，当前）** 全部交付。
P3 功能链：评测口径重建（留一集带标签 29 条 + `--no-tags` 开关）→ 标签可见可编辑
（FileRow.tags / kind 体系 / tag_locked / 词表管理）→ 浏览页（archived_path + 回填 + 未归类分组）
→ 检索三入口（Ctrl+K / 高级筛选 / 目录内筛选，一套查询层 5000 份 p95 8ms）
→ 视觉通道（9b，闸门 8/10 裁决默认开）→ 待定区「让 AI 说明」（插队异步）→ 图片解读开关。
**ModelGate 三向互斥**（model_gate.rs，目录建议/文本深读/视觉深读共用一把；MODEL_BUSY 不记失败；
目录建议等闸 70s 拿不到就报错放弃，绝不硬上——8GB 显存两个模型并发会打崩 llama-server）。
发布件：`发布/文件管家_v0.5.0-alpha.1.exe` + NSIS。
最终基线：`cargo test` **207** / `vitest` **206**（25 文件）/ `vue-tsc` 0 / e2e **115** 条全过
（p1 42 / p2 25 / p2r 4 / p3 44）/ contrast 过。
台账：`P3_执行状态.md`（逐里程碑证据）、`P3_推荐评测报告.md`（A/B：标签路不动 top1、top3 全集 +2.4pp）。

**P3 新硬经验（都栽过）**：
- **release exe 只编 Rust 不重打前端 dist**：`cargo build --release` 后界面还是旧版 ——
  改完前端必须 `npm run build` 再编 Rust；评测/验收产物要对输出字段，不能只看跑通了。
- **e2e 抓到 Search.vue watch 漏 tagIds**（点 chip 只亮不重搜）：旧 vitest 侥幸绿是因为
  先点标签再点类型，类型点击的请求顺带带了 tags。单交互单断言，别让用例里混多个触发源。
- **happy-dom 有 matchMedia 且默认窗宽 1024**：useNarrow 误判窄窗会藏 DOM ——
  测试 beforeEach stub matchMedia 成宽窗。
- **events mock 必须与真实模块同形**（Promise→UnlistenFn），同步返回函数会让组件 `.then` 炸掉。
- **Stats.currentId 完成后立刻清空再 emit**，否则前端「正在读」永远转圈。
- **vim 插队队列**：Queue.push_front FIFO+去重；explains 完成态收敛靠 done 前进 OR !running。
- **git bash 的 PATH 里 `C:/...` 的冒号会砸分隔符**，要用 `/c/...` 形式（tauri build 找不到 cargo 栽过）。
- **视觉裁决必须人工看原图**（ViewImage），9b 输出「看起来像对的」不算数。

**三条铁律**：① 任何时候出现「不明黑框」，先 grep `Command::new` ——
`windows_subsystem="windows"` 的 GUI 进程起任何控制台子进程都会弹框；
② 别把同步 HTTP / 探活挂进 `refreshBoot()` 的默认路径，它在每次判定后都会被调；
③ 界面里任何「配置完就消失」的一次性页面都要留后门入口（目录建议栽过一次）。

**另外四条硬经验（都栽过）**：
- **别用文件总大小决定要不要读正文**：42MB 的 pptx 内部 XML 只有几 KB。体积闸只给「整份读进内存」的路。
- **用户说「间距不对」先量再改**：CDP 读 `getComputedStyle` / `getBoundingClientRect`（`test/measure-card.py`），
  别拿截图像素猜（我猜错过一次，差了 17px）。
- **位置列不要写死偏移**：`SELECT {FILE_COLS}, d.id` 后面接的列，偏移必须由列数算；加一列就全错位。
- **跑 e2e 别把输出接管道**：应用是常驻子进程，`| tail` 会一直不返回；用 `test/run-e2e.py`（落日志 + 时间戳数据目录）。
- **同名类选择器要限定作用域**：侧栏和待迁移底部都叫 `.foot`，`querySelector('.foot')` 量到的是侧栏，
  白追过一轮「重叠 25px」。CDP 测量一律写全路径（`.mg > .foot`）。
- **`min-height:0` 在滚动 flex 列里是陷阱**：只有一个子项允许压缩时，超高后只有它被压扁，
  里面的内容不缩、溢出画到别处（待迁移就这么糊的）。要么 `flex:0 0 auto`，要么单独开滚动容器。
- **Rust serde camelCase ↔ TS 类型必须对齐**：`pending_confirm` vs `pendingConfirm` → `undefined` → 界面显示 NaN，
  编译期零提示。测试要喂「线上真实形状」的数据。

**隔离验证**：`FILEBUTLER_DATA_DIR=<空目录>` + `WEBVIEW2_USER_DATA_FOLDER=<独立目录>`
可开一个不碰用户真实索引库的实例（`APPDATA` 改不动，`dirs` 走 `SHGetKnownFolderPath`）。

**驱动界面必须用 CDP，不要用 SendInput/PostMessage**：这台机器上键盘事件送不进 WebView2（鼠标能进）。
启动前设 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"`，用 `app-v2/test/cdp.py`
读 DOM 文本做断言、发真键盘事件、截 PNG。截图用 `test/shotp.py`（BMP 一张 14MB，会吃光读图预算）。
细节见 `.workbuddy/memory/2026-11-18.md` 末节（M8 五件事 + 3 处偏差 + 4 个新坑）。


## 历史：2026-11-18 傍晚转向重启

v0.2.1（规则+模型混合 → 涌现式分类）用户实测判定「还是不尽人意」。
旧文档全部归入 `历史归档/`，旧上下文只读 `历史归档/README.md`。

## 硬约束：技术栈不可擅改

**技术栈已定：Tauri 2 + Rust（后端）+ Vue 3 + TypeScript + Vite（前端）＋ SQLite/本地模型（后续）。**
未经用户明确许可，不得更换框架、不得替换为 Electron / Web 应用 / 低代码等替代方案。
即使遇到环境缺失（如缺 Rust），也应先安装所需工具链，而不是换栈。

> 教训：2026-11-16 因机器缺 Rust，曾擅自改为 Electron 方案并开始安装，
> 被用户当场纠正「该下载 rust 就下载 rust，后续没有我的许可不能改技术栈」。
> Electron 依赖已全部卸载。

## 其他已确认决策

- 产品形态：本地桌面 exe（不是 Web 应用），双击即用
- 归档根目录：单个固定目录，优先 D 盘（当前实际使用 `D:\文档归档`）
- 清理策略：三段式「复制 → SHA-256 校验 → 源文件入隔离区」，永不覆盖、永不直删、可撤销
- 扫描范围：全盘/指定目录，但系统目录强制黑名单不可放开
- 分类方式：AI 自动归纳为主 + 人工微调兜底；低置信不硬猜，进待确认队列
- 处理范围：文档 + 图片 + draw.io；音视频暂不做
- 平台：仅 Windows；跨平台仅做技术储备
- 定时整理：仅空闲时增量整理

## 环境事实

- Node v22.22.2 / npm 10.9.7
- Rust 1.100.0（stable-x86_64-pc-windows-msvc），rustup 装在 `%USERPROFILE%\.rustup`
- MSVC：Visual Studio 2022 Build Tools（`C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`）+ Windows SDK 10.0.26100
- 网络：`static.rust-lang.org` 极慢（26KB/s），**必须用镜像** `rsproxy.cn`（2.4MB/s）；
  cargo 已在 `~/.cargo/config.toml` 配置 rsproxy sparse 源
- **GitHub 直连时通时断**，下载大文件优先 `winget`（~15MB/s），不要手动 curl GitHub
- Python 3.13（零第三方库，脚本均用标准库/ctypes）；另有 `C:/Python314/python.exe`（3.14，装了 pymupdf 用于 PDF）
- **Ollama 0.40.16**：`C:\Users\kingb\AppData\Local\Programs\Ollama\ollama.exe`，端口 11434，
  模型存 `C:\Users\kingb\.ollama\models\`，GPU = RTX 4060 Laptop 8GB + 32GB 内存
- 已装模型：qwen3.5:9b（6.6GB）、qwen3.5:4b（3.3GB）、qwen2.5:7b、qwen3:4b-q4_K_M
- 卸载驻留模型用 `POST /api/generate {"model":...,"prompt":"","keep_alive":0}`；9b 加载前必须先卸载其他模型（否则显存不够），**切勿 `ollama stop`**

## 当前状态：大模型 MVP 已落地（v0.2.1，2026-11-17）

`llm.ts` 不再是从未被调用的死代码：归纳主流程已走 `core/hybrid.ts`（规则×模型融合），
界面有实时进度 + 实时判定列表，设置页是真实 Ollama 配置。产物 `发布/文件管家_v0.2.1.exe`。

**融合策略（别再改）**：双路一致→0.93 自动归档；双路分歧→强待确认且**预填规则结论**（理由里写模型异议）；
模型补判→0.76 待确认；无正文、`code`/`other` 一律不送模型。
一句话：**模型负责把「对的」确认下来，不负责把「错的」改掉。**

真实 453 份实测（smart，上限 60）：送 45 份 / 155s / 一致 30 / 补判 6 / 分歧 6 / 待确认 280→262。

**新增技术坑（写代码前必读）**：
- 源码文件绝不能送模型：`components.js`、`tickets.html` 会被判成「部门管理·单据表单」（ticket→工单）
- 模型把**项目背景当归口**：正文出现「智能工厂平台」就把安全检查报告/费率清单全归该项目（真实 5/5、演示 4/4）；提示词第 5 条只能纠正一部分 → 分歧绝不采信模型
- **并发没意义**：9 份文件并发 1/2/4 都是 27s，Ollama 默认串行推理；要快只能减份数
- Tauri 页面 origin 是 `http://tauri.localhost`，不在 Ollama CORS 白名单，必须走 Rust `llm_request` 代理
- 每多输出一个 JSON 字段约 +10 token ≈ 1.3s，所以只要 ws/dt 两字段

## LLM 评测结论（历史，仍有效）

**结论**（详见 `LLM评测报告.md`，修正标注后 24 份样本）：

| 方案 | ws | dt |
|---|---|---|
| 规则 | 70.8% | **87.5%** |
| qwen2.5:7b | 79.2% | 79.2% |
| qwen3:4b-q4_K_M | 66.7% | 79.2% |
| qwen3.5:4b（新增，11-17 测） | 70.8% | 79.2% |
| 旧混合 conf≥0.85 直通 | 83.3% | 83.3% |
| **分工：ws=7b / dt 规则优先** | 79.2% | **91.7%** |
| 双路一致直通 | 覆盖 50%，自动部分 100% | — |

1. 无方案碾压，24 份样本量下 8 个点差异不显著
2. **类型判断规则更强**（87.5% vs 79.2%），类型词典必须保留
3. 模型无正文也硬猜，24 份 0 次说"不知道"
4. 旧混合不能用的真因：17 份高置信直通里 4 份判错、静默归档无提示
5. 可靠混合 = 双路一致才直通（12/12 = 100%）
6. 最优 = 分工：模型判工作域、规则判类型

**qwen3.5:4b 裁决（11-17）**：不换。比 qwen3:4b 有进步（ws 66.7%→70.8%）但全面不敌 7b，无正文"说不知道"仅 9%（比 7b 15% 更会硬猜），维持原方案。详见 `LLM评测报告.md` 第九节。脚本判模型名含 `qwen3` 就自动传 `think:false`，qwen3.5 通用，无需改脚本。

**qwen3.5:9b 裁决（11-17）**：文本 ws79.2%（追平7b，含规则最高）/ dt75%（全场最低，模型越大类型越差），不适合作主分类；但**视觉四模型最强**——图片 4/4、扫描件PDF 2/2 全对（4b 是 3/4、1/2），哈希名小图和扫描件合同两处 4b 错 9b 对。速度慢（文本5.5s/份、视觉15-33s/图）、需独占显存。建议：主分类维持 7b+规则，视觉分支用 9b。详见 `qwen3.5-9B整体评测报告.md`。

**悬而未决（下次先做）**：我自己的 ground truth 有 5 处错，见 `app/test/ground-truth-review.md`，等用户裁决。
其中 3 份「梯度培育培训项目」（正文写「继续教育学院 制」）我标成智能工厂平台、应为人事培训，
直接导致一开始误判「7b 碾压规则」。

**技术坑（写代码前必读）**：
- qwen3 必须传 `"think": false`（26s → 3s）
- 8GB 显存装不下 7b+4b 同时加载，会让 llama-server 崩溃
- 不要 `ollama stop <model>`，会把 serve 进程一起带崩
- zip 格式不要按总大小限正文抽取（有 689MB pptx，内部 XML 很小）
- pptx slide 要按数字排序，`sorted()` 按字符串会错

## 语义归并的时间账（2026-11-18 实测，已推翻旧结论）

旧结论「9b 在 8GB 显存上跑 5000 字 prompt 会失败、归并不可行」**是错的**，
真因是 num_ctx 4096 只剩 900 输出余量 + 客户端默认 60s 超时，不是显存。
实测：40 候选 / 5039 字 / 3179 token，9b 约 **150 秒**返回合法 JSON；4b 吞吐高但输出更啰嗦，墙钟一样。
结论改写为：**一次性归并可行（150s），179 候选分块归并 12min 不可行**。
NPU（Ultra 9 185H 的 AI Boost 11 TOPS）帮不上：Ollama 只带 cuda/rocm/vulkan 后端，
且无运行时支持 NPU+GPU 拆同一模型，NPU 带宽 89.6GB/s 只有显存的 1/3。

## 文档索引

**2026-11-18 晚重整：全部旧文档已归入 `历史归档/`，新上下文先读 `历史归档/README.md`（一页讲清三条死路 + 可复用结论 + 可照搬代码），不要再翻根目录。**

| 文件 | 作用 |
|---|---|
| `历史归档/README.md` | **唯一入口**：三条路线死因、模型/环境实测边界、工程红线、代码资产清单 |
| `历史归档/01-需求与设计/` | 需求 v0.3 定稿、技术设计 v1.0、UI 设计约束 v1.0（视觉唯一参照） |
| `历史归档/02-评测与实测/` | LLM 评测、9b 全量、视觉评测、真实桌面测试、试点迁移、8GB 显存边界 |
| `历史归档/03-方案演进/` | 涌现式规格与实施计划（死路留证）、同一文件多版本方案、任务总账（至 11-18，前六节已过时） |
| `历史归档/04-规则引擎验证/`、`05-模型实验原始数据/` | 历史实验脚本与原始 JSON |
| `app/test/ground-truth-review.md` | **我标错的 5 处标注，仍未裁决** |
| `app/test/{desktop-scan,bench}.json` | 455 份真实扫描 / 91 份人工标注基准（唯一真实语料，重做时留着） |
| `app/README.md` | 桌面客户端实现说明 |
