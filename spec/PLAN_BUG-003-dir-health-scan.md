# 实施计划：BUG-003 目录体检（TDD）

> 分支：`bugfix/BUG-003-dir-health-scan`（基线 `chore/real-fixtures-from-20261122` = 67ff220）
> 规格：`spec/BUG-003-dir-health-scan.md`
> 基线：vitest 214 / cargo 215（perf 抖动单跑正常）/ vue-tsc 0

## 0. 范围裁决（spec 内部矛盾消解）

spec 行为 3 列 `kind = rename|merge|move|empty`，但非目标第 29 行明确「不直接删除磁盘目录或库记录，语义等价合并由 BUG-006 覆盖」，验收第 4 条只针对「重命名/移动」。据此裁决：

| kind | 规则层识别 | 执行 + 撤销 | 理由 |
|---|---|---|---|
| `rename` | ✅ | ✅ 本轮实现 | 验收第 4 条；只改 rel_path 及后代，写批次可撤销 |
| `move` | ✅ | ✅ 本轮实现 | 层级调整，同上（目标层级调整本质是 rel_path 前缀改写） |
| `empty` | ✅ | ❌ 只报告 | 删除空目录违反非目标；标注「需你手工确认后删除」 |
| `merge` | ✅ | ❌ 只报告 | 语义合并归 BUG-006；标注「由语义合并功能处理」 |

- 报告可解释、可拒绝；**绝不静默执行**。
- 绝不把扫描挂进 `refreshBoot()`（项目铁律，SDD 跨规格约束）。
- 模型复核复用 `ModelGate`；占用/不可用时**降级**为规则候选 + 标注「未经模型复核」，不报错不阻塞。

## 1. 数据模型（新增，model.rs）

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirHealthIssue {
    pub id: String,            // 稳定 id：kind:dirId，前端逐条采纳键
    pub kind: String,          // rename|merge|move|empty
    pub dir_id: i64,
    pub dir_name: String,
    pub rel_path: String,
    pub target_path: Option<String>,  // rename/move 的建议目标 rel_path
    pub reason: String,        // 中文依据
    pub confidence: f64,       // 0..1
    pub affected_files: usize, // 影响文件数
    pub source: String,        // rule|model
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DirHealthReport {
    pub issues: Vec<DirHealthIssue>,
    pub model_reviewed: bool,  // false = 降级
    pub note: String,          // 降级说明 / 统计
    pub elapsed_ms: u64,
}
```

## 2. 任务拆分（每任务 2-5 分钟，红→绿→提交）

### T1 · 规则层纯函数 `dir_health.rs`（RED-GREEN）
- 新建 `src-tauri/src/dir_health.rs`，`main.rs` 加 `mod dir_health;`。
- `pub struct DirFact { id, parent_id, name, rel_path, depth, file_count, child_count }`
- `pub fn analyze(facts: &[DirFact]) -> Vec<DirHealthIssue>` 纯函数，规则：
  1. **空目录**：`file_count==0 && child_count==0` → `empty`，conf 0.9
  2. **超深层级**：`depth >= 5` → `move`，target 建议上提一层（去掉首段），conf 0.7
  3. **版本/草稿后缀**：`rel_path` 任一段经 `suggest_dirs::strip_version_suffix` 变化，或末段 `is_filler_composed` → `rename`，target 为清洗后路径，conf 0.85
  4. **高度相似路径**：同 parent 下两段 `strip_version_suffix` 后相同、或编辑距离 ≤1 且长度 ≥3 → `merge`，target 指向先建者，conf 0.6
- 上限 ≤20 条，按 confidence 降序截断。
- **红测**（`tests.rs` 新 `mod dir_health`）：空目录/超深/版本后缀/相似各识别；**正常树无误报**（`2026`、`2026-Q4`、`智能工厂平台`、`合同` 等基础目录不得中招）。

### T2 · 降级 + 阶段事件（RED-GREEN）
- `dir_health.rs`：`pub fn scan(app_state, emit_stage)` 编排：取 facts（锁内快照，锁外算）→ 发 `scan` → 发 `rule` → 尝试模型复核：
  - `llm::reachable(base, 3s)` 失败 → `model_reviewed=false, note="本地模型未启动，以下为规则初筛（未经模型复核）"`
  - `ModelGate.acquire(Suggest, 0)` 失败 → 同上，note「模型正忙」
  - 可用 → 发 `model`，喂目录名清单给 LLM 复核（复用 JSON 容错解析），成功则把 model 确认的 issue `source=model`，失败也降级不报错
  - 发 `done`。**任何分支都不返回 Err**（除非 DB 读失败）。
- 命令 `dir_health_scan(app) -> DirHealthReport`：注册进 `generate_handler!`。
- **红测**：纯函数 `with_gate_busy` 版本注入 gate 占用 → 返回降级报告而非 Err；闸占用不死锁（超时返回）。

### T3 · 执行与撤销（RED-GREEN）
- 命令 `apply_dir_health(app, req: {issue, decision}) -> ApplyAck`：
  - 只接受 `rename`/`move` 且 `decision=accept`；其余返回拒绝说明。
  - 事务：更新 `archive_dirs.rel_path`（目标 + 所有后代前缀替换，注意**闭包索引**：先查后代再逐个 UPDATE，避免前缀碰撞）→ 更新 `files.archived_path`（受影响前缀替换）→ 写批次：`migrations(kind='dir_health', ...)` + `migration_items` 记录 old/new rel_path（src_path/dst_path 借用字段，file_id=NULL，与实际列语义对齐）。
  - 磁盘目录改名：`std::fs::rename` 源目录→目标（归档根下），失败回滚 DB 事务。
  - 同名冲突：`plan::unique_name` 错开，target 实际值回写 `ApplyAck.new_rel_path`。
- 命令 `undo_dir_health(app, migration_id) -> UndoReport`：按批次逆序还原 rel_path/archived_path/磁盘目录名；被手工改动过则拒绝（复用 recat 的哈希/存在性思路，这里用路径存在性防呆）。
- **红测**（fresh_db + 隔离归档根 tempdir）：采纳后 DB 路径与 archived_path 一致；撤销完全还原；同名冲突不覆盖（错开 `_2`）。

### T4 · 前端接线 + DirHealth 组件（RED-GREEN）
- `types.ts` 加 `DirHealthIssue` / `DirHealthReport` / `DirHealthStage` / `DirHealthApplyReq`。
- `ipc.ts` 加 `dirHealthScan()` / `applyDirHealth(req)` / `undoDirHealth(migrationId)`。
- `events.ts` 加 `onDirHealthStage`（`dir-health-stage` 事件）。
- 新建 `src/pages/settings/DirHealth.vue`：按钮「目录体检」→ 扫描 → 建议列表（kind 徽章、理由、影响文件数、置信度、`源:规则/模型`）；每条「采纳/忽略」；采纳调 `applyDirHealth` 后 `dirs.load()`；忽略项本地标记不进执行；「撤销上次修正」按钮。滚动约束遵循 BUG-010（`.list{max-height;overflow:auto}`）。
- `Settings.vue`「目录结构」卡片 `#action` 加体检入口（复用 DirHealth 组件）。
- **红测** `DirHealth.test.ts`（mock ipc/events 照搬 Settings.test.ts）：逐条采纳/忽略，忽略项不进入 apply 批次；降级标注显示；进度阶段显示。

### T5 · 收尾
- 全量 vitest + vue-tsc + cargo test 全绿（perf 抖动单跑复证）。
- 派新子代理两段式评审（规格符合度 + 代码质量）。
- 写工作日志；给用户合并/PR/保留/丢弃选择。

## 3. 铁律检查
- [ ] 扫描只手动触发，不进 refreshBoot
- [ ] 模型不可用/闸占用 → 降级不报错
- [ ] 不自动改动、不静默迁移
- [ ] 不删磁盘目录/库记录（empty/merge 只报告）
- [ ] 永不覆盖（unique_name 错开）
- [ ] 可撤销（批次 + undo）
- [ ] 阶段事件 scan→rule→model→done
