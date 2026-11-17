//! 跨 IPC 的数据结构（camelCase 与前端 TS 对齐）

use serde::{Deserialize, Serialize};

/// 扫描结果中的一个文件
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedFile {
    pub name: String,
    pub path: String,
    pub ext: String,
    pub kind: String,
    pub size: u64,
    pub mtime: i64,
    pub text_summary: String,
    pub drawio_nodes: Vec<String>,
    pub parse_error: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub processed: usize,
    pub total: usize,
    pub current_path: String,
    pub skipped: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub files: Vec<ScannedFile>,
    pub skipped: usize,
    pub truncated: bool,
}

/// 迁移计划的一条
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub file_id: String,
    pub file_name: String,
    #[serde(default)]
    pub source_path: String,
    pub target_dir: String,
    pub target_name: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub file_id: String,
    pub file_name: String,
    pub source_path: String,
    pub target_path: String,
    pub status: String,
    pub size: u64,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateProgress {
    pub done: usize,
    pub total: usize,
    pub name: String,
    pub stage: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateReport {
    pub batch_id: String,
    pub records: Vec<Record>,
    pub migrated: usize,
    pub failed: usize,
    pub bytes: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub restored: usize,
    pub failed: usize,
    pub records: Vec<Record>,
}
