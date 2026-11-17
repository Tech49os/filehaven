//! 目录扫描：递归遍历 + 系统目录强制排除 + 正文特征抽取

use crate::extract::{extract, kind_of};
use crate::model::{ScanProgress, ScanSummary, ScannedFile};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

/// 系统目录黑名单：强制排除，用户不可放开
const SYSTEM_DIRS: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "appdata",
    "application data",
    "$recycle.bin",
    "system volume information",
    "recovery",
    "perflogs",
    "node_modules",
    ".git",
    ".cache",
    ".workbuddy",
    "boot",
    "drivers",
    "msocache",
    "windows.old",
    "$windows.~bt",
    "$windows.~ws",
];

pub fn is_system_dir(name: &str) -> bool {
    let low = name.to_lowercase();
    SYSTEM_DIRS.contains(&low.as_str())
}

fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 扫描入口。返回已抽取正文的候选文件列表。
///
/// `app` 为 None 时不发进度事件（单元测试用）。
pub fn scan(
    app: Option<&AppHandle>,
    root: &str,
    archive_root: Option<String>,
    max_size_mb: u64,
    limit: usize,
) -> Result<ScanSummary, String> {
    let root_path = Path::new(root);
    if !root_path.is_dir() {
        return Err(format!("目录不存在或不可访问：{root}"));
    }

    let archive = archive_root.map(PathBuf::from);
    let max_bytes = max_size_mb.saturating_mul(1024 * 1024);
    let mut files: Vec<ScannedFile> = Vec::new();
    let mut skipped = 0usize;
    let mut processed = 0usize;
    let mut truncated = false;

    let archive_cmp = archive.as_ref().map(|p| p.to_string_lossy().to_lowercase());

    let walker = WalkDir::new(root_path)
        .max_depth(14)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            if e.file_type().is_dir() && is_system_dir(&name) {
                return false;
            }
            if let Some(ac) = &archive_cmp {
                let p = e.path().to_string_lossy().to_lowercase();
                if p.starts_with(ac.as_str()) {
                    return false;
                }
            }
            true
        });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if files.len() >= limit {
            truncated = true;
            break;
        }

        let path: &Path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        if meta.len() > max_bytes {
            skipped += 1;
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        // 跳过 Office 临时锁文件与系统隐藏文件
        if name.starts_with("~$") || name.starts_with(".") {
            skipped += 1;
            continue;
        }

        let ext = ext_of(&name);
        let kind = kind_of(&ext);
        let size = meta.len();
        let path_str = path.to_string_lossy().to_string();

        processed += 1;
        if (processed % 15 == 0 || processed == 1) && app.is_some() {
            let _ = app.unwrap().emit(
                "scan-progress",
                ScanProgress {
                    processed,
                    total: processed,
                    current_path: path_str.clone(),
                    skipped,
                },
            );
        }

        let (text, nodes, err) = extract(path, &ext, kind, size);
        files.push(ScannedFile {
            name,
            path: path_str,
            ext,
            kind: kind.to_string(),
            size,
            mtime: mtime_ms(&meta),
            text_summary: text.chars().take(2000).collect(),
            drawio_nodes: nodes,
            parse_error: err,
        });
    }

    let _ = app.map(|a| {
        a.emit(
            "scan-progress",
            ScanProgress {
                processed: files.len(),
                total: files.len(),
                current_path: String::new(),
                skipped,
            },
        )
    });

    Ok(ScanSummary {
        files,
        skipped,
        truncated,
    })
}
