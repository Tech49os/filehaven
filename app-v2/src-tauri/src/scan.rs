//! 目录扫描：递归遍历 + 系统目录强制排除（P1 只读元数据，不抽正文）
//!
//! 与旧版的差别：不再调用 `extract()`。455 份实测从 1.3s 降到 0.3s 左右，
//! 而且 P1 完全不需要 `textSummary` 落库（详设 §1.2）。

use crate::error::CmdError;
use crate::model::{ScanProgress, ScannedFile, SourceRoot};
use crate::tag;
use std::path::Path;
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
    ".filebutler",
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

pub fn ext_of(name: &str) -> String {
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

/// 扫多个根目录，返回 `(root_id, 文件元数据)` 列表。
///
/// `app` 为 None 时不发进度事件（单测用）。
pub fn scan_roots(
    roots: &[SourceRoot],
    archive_root: Option<&Path>,
    app: Option<&AppHandle>,
) -> Vec<(i64, ScannedFile)> {
    let archive_cmp = archive_root.map(|p| p.to_string_lossy().to_lowercase());
    let mut out: Vec<(i64, ScannedFile)> = Vec::new();

    for root in roots.iter().filter(|r| r.enabled) {
        let root_path = Path::new(&root.path);
        if !root_path.is_dir() {
            continue;
        }
        let mut processed = 0usize;
        let mut skipped = 0usize;

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
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let name = entry.file_name().to_string_lossy().to_string();
            // Office 锁文件与隐藏文件：连元数据都不该进库
            if name.starts_with("~$") || name.starts_with('.') {
                skipped += 1;
                continue;
            }

            let ext = ext_of(&name);
            let kind = tag::kind_of_ext(&ext).to_string();
            processed += 1;

            if let Some(a) = app {
                if processed % 50 == 0 {
                    let _ = a.emit(
                        "scan-progress",
                        ScanProgress {
                            root_path: root.path.clone(),
                            processed,
                            total: processed,
                            current_path: path.to_string_lossy().to_string(),
                            skipped,
                        },
                    );
                }
            }

            out.push((
                root.id,
                ScannedFile {
                    name,
                    path: path.to_string_lossy().to_string(),
                    ext,
                    kind,
                    size: meta.len(),
                    mtime: mtime_ms(&meta),
                },
            ));
        }

        if let Some(a) = app {
            let _ = a.emit(
                "scan-progress",
                ScanProgress {
                    root_path: root.path.clone(),
                    processed,
                    total: processed,
                    current_path: String::new(),
                    skipped,
                },
            );
        }
    }

    out
}

/// 单个路径 → 文件元数据（监听用它把「刚落的文件」变成一条可入库的记录）
pub fn meta_of(path: &Path) -> Option<ScannedFile> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_string();
    let ext = ext_of(&name);
    Some(ScannedFile {
        name: name.clone(),
        path: path.to_string_lossy().to_string(),
        ext: ext.clone(),
        kind: tag::kind_of_ext(&ext).to_string(),
        size: meta.len(),
        mtime: mtime_ms(&meta),
    })
}

/// 扫一个具体目录（`--selftest` 与单测用），不查库
pub fn scan_dir(dir: &str) -> Result<Vec<ScannedFile>, CmdError> {
    if !Path::new(dir).is_dir() {
        return Err(CmdError::new(
            crate::error::SOURCE_UNREADABLE,
            format!("目录不存在或不可访问：{dir}"),
        ));
    }
    let root = SourceRoot {
        id: 0,
        path: dir.to_string(),
        enabled: true,
    };
    Ok(scan_roots(&[root], None, None)
        .into_iter()
        .map(|(_, f)| f)
        .collect())
}
