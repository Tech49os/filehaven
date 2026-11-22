# 真实整理结果 → 保密测试语料

把一次真实整理的结果沉淀成可供测试使用的语料：一份结构化快照 JSON，加上按归档目录树重建的真实文件副本。数据规模大致为**数百份文件、上百个目录、数百个标签**，具体以快照为准。

## 文件职责

| 文件 | 作用 |
| --- | --- |
| `export_snapshot.py` | 以只读方式打开索引库，把各表导出为快照 JSON |
| `sync_files.py` | 从快照读取归档路径，复制真实文件副本并生成 sha256 清单 |
| `verify.py` | 校验快照行数、文件数量、sha256 以及 `_data/` 是否被 git 忽略 |
| `test_tools.py` | 用合成 sqlite 库与合成文件的单元测试 |
| `.gitignore` | 确保保密目录 `_data/` 永不进入 git |

## 用法

在本目录执行，所有真实路径都通过命令行参数传入，源码里不写死任何真实路径。

```bash
# 1. 只读导出快照
python export_snapshot.py <索引库路径> <输出snapshot.json路径>

# 2. 复制真实文件副本到 _data（按归档相对路径重建目录树）
python sync_files.py <snapshot.json路径> <目标目录，例如 _data>

# 3. 校验语料完整性
python verify.py <snapshot.json路径> <目标目录，例如 _data>

# 4. 运行合成测试（不接触真实数据）
python -m unittest -v test_tools
```

快照 JSON 顶层包含 `meta` 和 `tables`：`meta` 记录采集时间、各表行数统计，以及从 `settings` / `source_roots` 读出的 sourceRoot 与 archiveRoot；`tables` 是按 `pragma table_info` 动态读取列后导出的各表内容。所有真实名称、真实标签与真实路径只存在于 `meta` 和 `tables` 的具体取值里。

`sync_files.py` 只读取源文件（`shutil.copy2`），不移动、不删除、不改名。若目标文件已存在且 sha256 一致则跳过，重复执行是幂等的；若源文件缺失会列出并报错，不会静默跳过。清单 `checksums.sha256` 每行格式为 `<相对路径>  <sha256>`。

## 保密规则

1. `_data/` 已被 `.gitignore` 忽略，真实快照和真实文件副本只能放在这里，**绝不能提交进 git**。
2. 源码、测试、README 中只允许出现占位符和合成数据（如 `测试文件A.docx`、`测试目录`），不得出现真实文件名、真实目录名、真实标签内容或真实绝对路径。
3. 本目录内容属于用户保密文件，**不要外传、不要上传到外部服务**。
4. 导出脚本对索引库使用 `mode=ro` 只读打开；复制脚本只读源文件。请勿手动改动这两类源数据。
5. 需要新增示例时，一律使用合成名称；真实数据只通过运行上述脚本在本地生成。
