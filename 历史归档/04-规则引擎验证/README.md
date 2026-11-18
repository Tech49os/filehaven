# AI 归纳分类 验证原型

进入完整开发前，用来验证「AI 自动归纳分类 + draw.io 内容理解」是否可行的零依赖 Python 原型。

## 运行

```bash
cd D:\filemanager\ai_validation
python ai_induction_validation.py
```

无需安装任何第三方库（仅用 Python 标准库），Python 3.8+ 均可。

### 可选参数

```bash
python ai_induction_validation.py --k 6          # 指定聚类数
python ai_induction_validation.py --no-auto-k    # 关闭自动 K，用真实类别数 6
python ai_induction_validation.py --seed 42      # 指定随机种子
```

### 真实 LLM 复测（可选）

原型已预留 LLM 接口。配置环境变量后用真实 LLM 做簇命名：

```bash
set AI_VAL_LLM=1
set OPENAI_API_KEY=sk-你的key
set OPENAI_BASE_URL=https://api.openai.com/v1
python ai_induction_validation.py
```

未配置时自动用离线基线命名，不影响主流程。

## 它验证了什么

| 实验 | 说明 |
|---|---|
| draw.io 解析 | 抽取架构图的节点文字，验证图内文字能否作为分类/检索输入 |
| 实验 A vs B | 仅文件名 vs 文件名+内容+draw.io，量化内容抽取的价值 |
| 阶段二A | 纯统计归类准确率（20 轮留出法，防数据泄漏） |
| 阶段二B | 语义理解（模拟 LLM）归类准确率，与 85% 目标对比 |

## 关键结论

- draw.io 节点解析 **100%** 成功
- 纯统计归类 **66.2%**（不达标）→ 语义理解 **86.2%**（达标）
- **结论：必须用 LLM 做语义分类，纯规则/TF-IDF 不够**

详见 `验证报告.md`。

## 文件

- `ai_induction_validation.py` — 主程序（分词、draw.io 解析、TF-IDF、K-Means、准确率评估）
- `sample_data.py` — 58 份内置样本集（模拟真实杂乱命名 + 人工标注分类）
- `验证报告.md` — 完整验证结论
- `result.json` — 运行时生成的结构化结果

## 如何接入你的真实文件

1. 在 `sample_data.py` 中新增 `load_real_docs(root_dir)`，遍历目录读取文件。
2. 对文档抽取文本（docx/xlsx/pdf），对 `.drawio` 复用 `parse_drawio_nodes()`。
3. 为每份文件标注 `true_category`（ground truth），替换 `load_sample_docs()` 的返回值。
4. 重新运行，即可得到你真实数据上的准确率。

> 提示：真实 LLM 复测是关键一步——词表模拟只是近似，真实 Qwen2.5-3B 的语义理解能力需要实测确认。
