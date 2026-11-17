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

## 阶段 B（暂不做，待阶段 A 验收后）
- `types.ts`：废弃 `ws`/`dt`，改 `topicId`/`tags`
- `TopicConfirm.vue`：方案确认页（改名/删除/合并/秒级重算）
- `wizard.ts`、`InductionStep.vue`、`ReviewStep.vue`、`MigrateStep.vue` 接入
- 迁移路径 `categoryPathOf` 改为单层
