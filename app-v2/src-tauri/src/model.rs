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
}
