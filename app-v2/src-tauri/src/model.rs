//! 跨 IPC 的数据结构（`serde(rename_all = "camelCase")` 与前端 TS 一一对应）

use serde::{Deserialize, Serialize};

/// 扫描到的文件元数据（P1 不抽正文，只有元信息）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedFile {
    pub name: String,
    /// 绝对路径
    pub path: String,
    pub ext: String,
    /// 6 类：word|excel|ppt|pdf|image|diagram|other
    pub kind: String,
    pub size: u64,
    /// 毫秒
    pub mtime: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRoot {
    pub id: i64,
    pub path: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveDir {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    /// imported|model_approved|user|inline
    pub origin: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirNode {
    pub id: i64,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    pub origin: String,
    pub file_count: i64,
    pub children: Vec<DirNode>,
}

/// 扫描落库统计
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertStats {
    pub inserted: usize,
    pub updated: usize,
    pub missing: usize,
    /// size/mtime 都没动：只刷了 last_seen_at，没有整行重写（T3.1）
    pub unchanged: usize,
}

/// 一条人工判定（assignments 表）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub id: i64,
    pub file_id: i64,
    pub batch_id: Option<i64>,
    pub target_dir_id: Option<i64>,
    pub origin: String,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    /// accepted|changed|deferred|parked
    pub decision: String,
    pub decided_at: i64,
}

/// 待写入的一条判定
#[derive(Clone, Debug)]
pub struct NewAssignment {
    pub file_id: i64,
    pub batch_id: Option<i64>,
    pub target_dir_id: Option<i64>,
    pub origin: String,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub decision: String,
    pub decided_at: i64,
}

/// 扫描进度事件载荷（`scan-progress`）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub root_path: String,
    pub processed: usize,
    pub total: usize,
    pub current_path: String,
    pub skipped: usize,
}

/// 目录建议阶段事件载荷（`suggest-stage`）
///
/// 只报阶段名与已等毫秒数 —— 模型什么时候返回说不准，报百分比就是撒谎。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestStage {
    /// sampling | waiting | validating
    pub stage: String,
    pub elapsed_ms: u64,
    pub note: String,
}

/// 一次扫描的汇总（前端用它显示「已跳过 N 份非资料型文件」，不许静默）
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scanned: usize,
    pub inserted: usize,
    pub updated: usize,
    pub missing: usize,
    pub skipped_by_rule: usize,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub queued: i64,
    pub labeled: i64,
    pub pending_confirm: i64,
    pub confirmed: i64,
    pub migrated: i64,
    pub parked: i64,
    pub skipped: i64,
    pub missing: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub reachable: bool,
    pub model: Option<String>,
}

/// 启动第一调，决定前端路由
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootState {
    pub configured: bool,
    pub archive_root: Option<String>,
    pub source_roots: Vec<SourceRoot>,
    pub counts: Counts,
    pub model: ModelStatus,
    /// 运行时监听是否开着（设置页那个开关，T3.7）
    pub watch_enabled: bool,
}

/// 一个标签（界面上要显示、要能点、要能删）
///
/// `kind` 决定样式与可删性：`type`/`time` 是基础标签不许删，
/// `topic` 是模型给的，`user` 是人自己加的。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRef {
    pub id: i64,
    pub name: String,
    /// type|time|topic|keyword|user
    pub kind: String,
}

/// 标签词表里的一行（T1.5 设置页）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// 挂了多少份文件
    pub file_count: i64,
}

/// 列表/卡片里的一份文件
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub id: i64,
    pub name: String,
    pub ext: String,
    pub kind: String,
    pub size: i64,
    /// 毫秒
    pub mtime: i64,
    pub status: String,
    pub rel_path: String,
    pub abs_path: String,
    pub source_root_id: Option<i64>,
    pub defer_count: i32,
    pub note: Option<String>,
    /// 归档时改成的名字（NULL = 保持原名）
    pub target_name: Option<String>,
    /// 深度标签产出的一句话说明（NULL = 读过但没结论 / 还没读，用 `tag_level` 区分）
    pub summary: Option<String>,
    /// basic | deep
    pub tag_level: String,
    /// 人改过标签：队列重跑只写 summary，不覆盖标签（T1.4）
    pub tag_locked: bool,
    /// 标签，按 `type → time → topic → keyword → user` 排好（T1.1，批量补，不进主查询）
    pub tags: Vec<TagRef>,
}

/// 一条候选目录（reason 非空，服务端保证）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub dir_id: i64,
    pub rel_path: String,
    pub score: f64,
    pub reason: String,
    /// 0.35 ≤ score < 0.55：橙色徽标
    pub uncertain: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub file: FileRow,
    /// 0~2 个备选：主推在 [0]；needsYou 时为空
    pub candidates: Vec<Candidate>,
    pub needs_you: bool,
}

/// 待定区的一行（T4.1）
///
/// 候选是**每次打开现算的**：待定区里的文件可能躺了几周，
/// 这期间目录长了、别的文件归档过了，几周前算的候选早就不作数。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParkedItem {
    pub file: FileRow,
    pub candidates: Vec<Candidate>,
    pub needs_you: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchView {
    pub batch_id: i64,
    pub seq: i64,
    pub size: i64,
    pub items: Vec<BatchItem>,
    pub remaining_in_pool: i64,
    /// 0 时前端切空态（验收 11）
    pub dir_count: i64,
    /// defer_count ≥ 3 的 fileId（排最后并标注）
    pub deferred_badge: Vec<i64>,
}

/// 一次人工判定（`decide` 入参）
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideReq {
    pub file_id: i64,
    pub batch_id: i64,
    pub dir_id: Option<i64>,
    /// accepted | changed | deferred | parked
    pub decision: String,
    /// recommended | user_changed | user_picked | new_dir
    pub origin: String,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub note: Option<String>,
}

/// 判定回执：前端只靠它更新计数与卡片，不重拉整批
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideAck {
    pub file_id: i64,
    pub status: String,
    /// 本批已确认份数（accepted + changed）
    pub confirmed: i64,
    pub target_rel_path: Option<String>,
    pub remaining_in_pool: i64,
}

/// 关批次回执
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseAck {
    /// 本次回池的份数
    pub closed: usize,
    pub remaining_in_pool: i64,
}

/// 启动/写操作后刷新用的空忙态占位（前端不显示，只用于命令出参一致）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OkAck {
    pub ok: bool,
}

// ---------------------------------------------------------------- 迁移（M7）

/// 待迁移预览（详设 §6.3）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreview {
    pub groups: Vec<PreviewGroup>,
    /// 将加 `_2` 的冲突（目标已有同名，或本批里两份同名）
    pub name_conflicts: Vec<NameConflictOut>,
    /// 标注后被改过，必须拦下的
    pub changed_since_label: Vec<ChangedOut>,
    pub total_files: usize,
    pub total_bytes: u64,
    /// 归档盘剩余（GB）。拿不到就是 None —— 不拦，只是不保证
    pub free_gb: Option<f64>,
    pub enough_space: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGroup {
    pub dir_id: i64,
    pub rel_path: String,
    /// 归档目录磁盘上还不存在 → 显示「（新建目录）」
    pub is_new: bool,
    pub files: Vec<FileRow>,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameConflictOut {
    pub file_id: i64,
    pub target_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedOut {
    pub file_id: i64,
    pub name: String,
    /// mtime | size | missing
    pub why: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationFailure {
    pub file_id: i64,
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub migration_id: i64,
    pub copied: usize,
    pub verified: usize,
    pub quarantined: usize,
    pub failed: Vec<MigrationFailure>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub restored: usize,
    /// 已经不在 migrated 状态（或记录缺失）而跳过的
    pub skipped: usize,
    pub failed: Vec<MigrationFailure>,
}

/// 最近一次迁移（撤销入口）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSummary {
    pub id: i64,
    /// running|done|partial|failed|undone
    pub status: String,
    pub file_count: i64,
    pub bytes: i64,
    pub finished_at: Option<i64>,
}

/// 迁移/撤销进度事件（`migrate-progress` / `undo-progress`）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateProgress {
    pub done: usize,
    pub total: usize,
    pub name: String,
    /// copy | verify | quarantine
    pub stage: String,
}
