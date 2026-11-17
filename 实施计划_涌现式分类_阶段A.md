# 实施计划 · 涌现式分类（阶段 A：引擎）

> 规格见 `文档分类方案_规格_v1.0.md`。本计划只做**引擎**，不碰界面（按约定先引擎后界面）。
> 纪律：每个任务先写会失败的测试（RED），再写最少实现（GREEN），跑通才进下一个。

## 前置

- 分支 `feat/topic-emergence`（已建，基线已提交）
- `vitest@2.1.9` 已装（vite 5 兼容版；不升 vite 6，避免波及 Tauri 构建）
- 测试命令 `npm test`（在 `app/` 下）

## 任务清单

### A1 语言级词表 `src/core/emergence/lexicon.ts`
- `DOC_KIND_WORDS`：文档性质词（方案/汇报/报告/合同/协议/清单/报价/模板/纪要/议程/申请/审批…）
- `ORG_SUFFIXES`：组织后缀（项目组/技术室/工作组/专班/团队/公司/部门/中心/事业部/研究院/分公司/办公室）
- `GENERIC_WORDS`：公文套话（关于/有关/相关/工作/项目/建设/管理/技术/系统/平台…）
- **禁止**出现任何行业词。测试：断言表中不含 `钻采|油化厂|智能工厂|海油|信科`
- 验收：`lexicon.test.ts` 全绿

### A2 清洗与切段 `src/core/emergence/normalize.ts`
- `stripNoise(name)`：去扩展名、`附件N：`、`副本/最终版/修订版`、`V1.0`、`20260908`、`2026.8.4`、裸数字
- `segments(name)`：按 `- _ 空格 + · 、 ， ： （）【】` 切段，返回原样段
- 测试用例：附件前缀剥离、版本号剥离、纯数字文件名返回空、`（二期）` 这类中文括号内容必须保留
- 验收：`normalize.test.ts` 全绿

### A3 候选抽取 `src/core/emergence/candidates.ts`
- `collectCandidates(files: {name:string; path:string}[], opts)` → `Candidate[]`
- `Candidate { term: string; df: number; isOrg: boolean; samples: string[]; score: number }`
- 候选 = 整段（≤12 字）+ 段内 3–8 字 n-gram
- 过滤：DF ≥ 2；DF ≤ max(3, N×0.08)；命中停用词剔除；组织后缀标记 `isOrg`
- 测试：构造 5 份小样本能抽出 `技术谱系` 且 df=3；部门名被标 `isOrg`；DF 超限被丢
- 验收：`candidates.test.ts` 全绿

### A4 去重与排序（同文件）
- 最长匹配去重：A ⊂ B 且 DF(B) ≥ DF(A)×0.85 → 丢 A
- `score = df × (1 - isOrg×0.6) × positionBonus`（首段 1.0 / 中段 0.7 / 末段 0.5）
- Top-K（默认 80）
- 测试：长串与短串不同时存在；组织名排在真事项之后；K 截断生效
- 验收：`candidates.test.ts` 追加用例全绿

### A5 真实数据回归 `test/emergence-real.test.ts`
- 读 `test/real-classify.json` 的 450 个 `name`
- 断言：至少出现 5 个真实事项（`技术谱系`/`人工智能推进会`/`梯度培育`/`逐浪数字蓝海`/`油化厂`）
- 断言：`智能工厂技术室`、`生产智能化项目组`、`信科公司` 均 `isOrg=true`
- 断言：`技术谱系` 与 `中海油信息科技有限公司科技技术谱系` 不同时存在
- 验收：验收标准 1/2/3/8 通过

### A6 模型归并 `src/core/emergence/merge.ts`
- `buildMergePrompt(cands)` → string（纯函数，可测：必须包含全部 alias、要求 JSON、禁止发明新词）
- `parseMergeResult(raw, cands)` → `MergedTopic[]`，校验 alias 必须来自候选，越界即丢弃并记录
- `fallbackMerge(cands, k=12)` → 降级：非组织候选直接当类目名
- 测试：合法 JSON 解析；含幻觉 alias 时被剔除；模型输出非法 JSON 时不抛异常而是走降级
- 验收：`merge.test.ts` 全绿

### A7 归属判定 `src/core/emergence/assign.ts`
- `scoreFile(file, topic)` → 0–1（文件名命中 alias 0.90 / 路径命中 0.80 / 正文命中 0.72 / 正文 ≥2 关键词 0.68）
- `assign(files, topics, threshold=0.85)` → `{ assigned, pending }`
- 测试：文件名命中→自动；仅正文命中→待确认；都不命中→待确认；**不存在任何默认归属**
- 验收：验收标准 7 通过

### A8 提案装配 `src/core/emergence/proposal.ts`
- `buildProposal(files, topics, assignResult)` → `TopicProposal`
- 每个 topic 带 3–5 个代表文件
- 守恒断言：`topics 内文件数 + ungrouped 数 === 文件总数`
- 验收：验收标准 4/6/7 通过

## 阶段 A 完成标志

```
npm test          # 全绿
npx tsx test/emergence-real.ts   # 在 450 份真实文件上输出提案，人工看质量
```

输出一份 `test/proposal-real.json`，我会把类目 + 代表文件列给你看，你确认后进入界面改造。

---

## 实测结论（2026-11-17，别重走这些路）

### 已经跑通的

- `normalize` / `candidates` / `grouping` 三个模块，43 个测试全绿
- 在真实 450 份文件上能涌现出真事项：技术谱系(18)、人工智能推进会(14)、梯度培育培训(12)、
  钻采院高温高压项目(10)、智擎海发版(8)、AI编程(8)、智造领航逐浪数字蓝海(7)、中期工作会(7)、
  全栈工程师(6)、AI大赛(5)、安环大屏(4)、境内差旅费(4)……
- 碎片剔除有效：`中海油信息科技有` 那类 9 个滑动窗口碎片已被清掉

### 走不通的（重要，别再试）

**1. 模型归并（qwen2.5:7b）判断力不够，在两个极端之间摆动**

| prompt 策略 | 结果 |
|---|---|
| 「请合并同一件事的不同叫法」 | 40 个候选压成 **5 个笼统大类**（智能工厂推进/数字化建设/AI应用赋能…），把钻采院、差旅费、AI大赛全丢进噪音 |
| 「逐个判断是不是具体的事」 | 40 条里 **36 条判 null**，包括技术谱系、AI大赛、中期工作会 |
| 「没把握就单独成组，别标 null」 | **38 个词各自成组**，0 噪音，连 `ppt`、`work`、`演讲稿` 都当事项 |

耗时：7b 每次 70–79 秒。**结论：这个任务需要的是稳定语义判断，7b 不具备。**

**2. qwen3.5:9b 在本机跑不动这个 prompt**

5000 字 prompt（40 个候选）→ 199 秒后连接断开。8GB 显存 + 9b 模型 + 4096 上下文，
prefill 阶段就撑不住。（单份文件 5.5 秒的评测数据是短文场景，不能套用到长 prompt。）

**3. 纯算法分组的质量是「草稿」不是「成品」**

60 个候选 → 43 组 → 初筛后 42 项。其中约 25 项是真事项，其余是用
`演讲稿`、`申请表`、`操作手册`、`ppt修改`、`中国海油`、`生产管理`、`技术沉淀` 这类词凑的。

所以「系统自动给出高质量分类方案」在当前硬件上达不到。
能达到的是：**给出一份需要人工删改的初稿**。

### 下一步的三种走法（待用户选择）

| 方案 | 做法 | 代价 |
|---|---|---|
| A 草稿模式 | 42 项全给用户，界面上支持多选批量删除 + 合并，调完记住方案 | 首轮约 10 分钟人工调整 |
| B 高置信先行 | 只自动建权重 ≥8 的 15 个事项，其余进「未归类」让用户拖 | 自动部分更准，但覆盖面小 |
| C 轻量二分类 | 用 4b 做「是不是部门名/格式词」的逐条二分类（比开放式归并简单得多），先清垃圾再分组 | 多一次模型调用，但任务简单、可能稳 |

## 阶段 B（暂不做，待阶段 A 验收后）
- `types.ts`：废弃 `ws`/`dt`，改 `topicId`/`tags`
- `TopicConfirm.vue`：方案确认页（改名/删除/合并/秒级重算）
- `wizard.ts`、`InductionStep.vue`、`ReviewStep.vue`、`MigrateStep.vue` 接入
- 迁移路径 `categoryPathOf` 改为单层
