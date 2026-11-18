//! 文件管家 v2 · 本地智能文件归档客户端
//!
//! P1 阶段：只做元数据扫描 + 人工确认整理，不抽正文；只有「目录建议」用本地模型。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

mod error;
mod index;
mod llm;
mod model;
mod plan;
mod scan;
mod state;
mod tag;

#[cfg(test)]
mod tests;

use error::CmdError;
use model::{
    ArchiveDir, BootState, Counts, DirNode, ModelStatus, ScanReport, SourceRoot,
};
use state::AppState;

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, CmdError> {
    state
        .db
        .lock()
        .map_err(|_| CmdError::new(error::DB, "数据库锁已损坏，请重启应用"))
}

fn setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    index::get_setting(conn, key).ok().flatten()
}

// ---------------------------------------------------------------- 基础命令

/// 选择目录（系统原生对话框）
#[tauri::command]
async fn pick_directory(title: Option<String>, initial: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new();
        if let Some(t) = title {
            if !t.is_empty() {
                d = d.set_title(&t);
            }
        }
        if let Some(i) = initial {
            if !i.is_empty() && Path::new(&i).is_dir() {
                d = d.set_directory(&i);
            }
        }
        d.pick_folder().map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// 在资源管理器中打开
#[tauri::command]
fn open_path(path: String) -> Result<(), CmdError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(CmdError::new(error::IO, "路径不存在"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(p.as_os_str())
            .spawn()?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(CmdError::new(error::IO, "仅支持 Windows"))
    }
}

/// 在资源管理器中定位并选中该文件
#[tauri::command]
fn reveal_path(path: String) -> Result<(), CmdError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(CmdError::new(error::IO, "路径不存在"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.to_string_lossy()))
            .spawn()?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(CmdError::new(error::IO, "仅支持 Windows"))
    }
}

/// 本机磁盘剩余空间（GB）
#[tauri::command]
async fn disk_free_gb(path: String) -> Option<f64> {
    tauri::async_runtime::spawn_blocking(move || free_gb(&path))
        .await
        .ok()
        .flatten()
}

fn free_gb(path: &str) -> Option<f64> {
    let p = PathBuf::from(path);
    let target = p.ancestors().find(|a| a.exists())?;
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-PSDrive -Name '{}').Free",
                    target.components().next().and_then(|c| match c {
                        std::path::Component::Prefix(p) =>
                            Some(p.as_os_str().to_string_lossy().trim_end_matches(':').to_string()),
                        _ => None,
                    })?
                ),
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let bytes: f64 = s.trim().parse().ok()?;
        Some(bytes / 1024.0 / 1024.0 / 1024.0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        None
    }
}

// ---------------------------------------------------------------- 启动状态

/// 启动第一调：决定前端路由（引导 / 整理台）
#[tauri::command]
async fn boot_state(app: AppHandle) -> Result<BootState, CmdError> {
    tauri::async_runtime::spawn_blocking(move || boot_state_inner(&app))
        .await
        .map_err(|e| CmdError::new(error::DB, format!("启动检查线程异常：{e}")))?
}

fn boot_state_inner(app: &AppHandle) -> Result<BootState, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;

    let archive_root = setting(&conn, "archive_root");
    let source_roots = index::list_source_roots(&conn)?;

    let mut counts = Counts::default();
    for (status, n) in index::count_by_status(&conn)? {
        match status.as_str() {
            "queued" => counts.queued = n,
            "labeled" => counts.labeled = n,
            "pending_confirm" => counts.pending_confirm = n,
            "confirmed" => counts.confirmed = n,
            "migrated" => counts.migrated = n,
            "parked" => counts.parked = n,
            "skipped" => counts.skipped = n,
            "missing" => counts.missing = n,
            _ => {}
        }
    }

    let base = setting(&conn, "ollama_base").unwrap_or_else(|| llm::DEFAULT_BASE.to_string());
    let model = setting(&conn, "ollama_model").unwrap_or_else(|| llm::DEFAULT_MODEL.to_string());
    drop(conn);

    // 探活一次，3s 超时。模型没起不能挡住任何别的功能（验收 14）。
    let reachable = llm::reachable(&base, Duration::from_secs(3));

    Ok(BootState {
        configured: archive_root.is_some() && !source_roots.is_empty(),
        archive_root,
        source_roots,
        counts,
        model: ModelStatus {
            reachable,
            model: Some(model),
        },
    })
}

// ---------------------------------------------------------------- 源区 / 归档区

#[tauri::command]
async fn add_source_root(app: AppHandle, path: String) -> Result<SourceRoot, CmdError> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(CmdError::new(
            error::SOURCE_UNREADABLE,
            format!("目录不存在或不可访问：{path}"),
        ));
    }
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::add_source_root(&conn, &path)
}

#[tauri::command]
async fn list_source_roots(app: AppHandle) -> Result<Vec<SourceRoot>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::list_source_roots(&conn)
}

#[tauri::command]
async fn remove_source_root(app: AppHandle, id: i64) -> Result<(), CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::remove_source_root(&mut conn, id)?;
    Ok(())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInfo {
    pub path: String,
    pub free_gb: Option<f64>,
    pub quarantine_path: String,
}

fn in_source(archive: &str, sources: &[SourceRoot]) -> bool {
    let a = archive.replace('\\', "/").to_lowercase();
    sources.iter().any(|s| {
        let r = s.path.replace('\\', "/").to_lowercase();
        a == r || a.starts_with(&format!("{r}/"))
    })
}

#[tauri::command]
async fn set_archive_root(app: AppHandle, path: String) -> Result<ArchiveInfo, CmdError> {
    let state = app.state::<AppState>();
    let sources = {
        let conn = lock_db(&state)?;
        index::list_source_roots(&conn)?
    };
    if in_source(&path, &sources) {
        return Err(CmdError::new(
            error::ARCHIVE_IN_SOURCE,
            "归档区不能放在源区里，否则会被重复扫描",
        ));
    }

    let root = PathBuf::from(&path);
    std::fs::create_dir_all(&root)?;
    let quarantine = root.join(".filebutler").join("quarantine");
    std::fs::create_dir_all(&quarantine)?;

    {
        let conn = lock_db(&state)?;
        index::set_setting(&conn, "archive_root", &path)?;
    }

    Ok(ArchiveInfo {
        free_gb: free_gb(&path),
        quarantine_path: quarantine.to_string_lossy().to_string(),
        path,
    })
}

// ---------------------------------------------------------------- 扫描

/// 扫描全部源区（长任务，spawn_blocking）
#[tauri::command]
async fn scan_all(app: AppHandle) -> Result<ScanReport, CmdError> {
    tauri::async_runtime::spawn_blocking(move || scan_all_inner(&app))
        .await
        .map_err(|e| CmdError::new(error::SOURCE_UNREADABLE, format!("扫描线程异常：{e}")))?
}

fn scan_all_inner(app: &AppHandle) -> Result<ScanReport, CmdError> {
    let state = app.state::<AppState>();
    let (roots, archive) = {
        let conn = lock_db(&state)?;
        (index::list_source_roots(&conn)?, setting(&conn, "archive_root"))
    };
    if roots.is_empty() {
        return Err(CmdError::new(error::NOT_CONFIGURED, "还没有选择源区"));
    }

    let t0 = Instant::now();
    // 锁外扫描：这一段时间不持有数据库连接
    let scanned = scan::scan_roots(
        &roots,
        archive.as_deref().map(Path::new),
        Some(app),
    );

    let mut report = ScanReport {
        scanned: scanned.len(),
        ..Default::default()
    };

    {
        let mut conn = lock_db(&state)?;
        for root in &roots {
            let files: Vec<model::ScannedFile> = scanned
                .iter()
                .filter(|(rid, _)| *rid == root.id)
                .map(|(_, f)| f.clone())
                .collect();
            if files.is_empty() {
                continue;
            }
            let st = index::upsert_files(&mut conn, root.id, &files)?;
            report.inserted += st.inserted;
            report.updated += st.updated;
            report.missing += st.missing;

            // 基础标签：type / time / keyword，写完 queued → labeled
            let queued = index::list_queued(&conn, Some(root.id))?;
            let items: Vec<(i64, Vec<(String, String, f64)>)> = queued
                .iter()
                .map(|(id, name, kind, mtime)| (*id, tag::basic_tags(name, kind, *mtime)))
                .collect();
            index::apply_basic_labels(&mut conn, &items)?;
        }

        for (status, n) in index::count_by_status(&conn)? {
            if status == "skipped" {
                report.skipped_by_rule = n as usize;
            }
        }
    }

    let _ = app.emit(
        "scan-progress",
        model::ScanProgress {
            root_path: String::new(),
            processed: report.scanned,
            total: report.scanned,
            current_path: String::new(),
            skipped: report.skipped_by_rule,
        },
    );

    report.elapsed_ms = t0.elapsed().as_millis() as u64;
    Ok(report)
}

// ---------------------------------------------------------------- 目录

#[tauri::command]
async fn list_dirs(app: AppHandle) -> Result<Vec<DirNode>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::list_dir_nodes(&conn)
}

#[tauri::command]
fn preview_dir_name(input: String) -> plan::DirNamePreview {
    plan::clean_path(&input)
}

#[tauri::command]
async fn create_dir(app: AppHandle, input: String) -> Result<ArchiveDir, CmdError> {
    let clean = plan::clean_path(&input);
    if !clean.valid {
        return Err(CmdError::new(
            error::DIR_INVALID,
            clean.reason.unwrap_or_else(|| "目录名无效".into()),
        ));
    }

    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;

    // 逐段建：父不存在就一并建
    let mut parent_id: Option<i64> = None;
    let mut last: Option<ArchiveDir> = None;
    for seg in clean.cleaned.split('/') {
        let existing = match parent_id {
            Some(pid) => {
                let p = index::get_dir(&conn, pid)?
                    .ok_or_else(|| CmdError::new(error::DB, "父目录丢失"))?;
                index::dir_by_rel_path(&conn, &format!("{}/{}", p.rel_path, seg))?
            }
            None => index::dir_by_rel_path(&conn, seg)?,
        };
        match existing {
            Some(d) => {
                parent_id = Some(d.id);
                last = Some(d);
            }
            None => {
                let d = index::insert_dir(&conn, parent_id, seg, "user")?;
                parent_id = Some(d.id);
                last = Some(d);
            }
        }
    }
    last.ok_or_else(|| CmdError::new(error::DIR_INVALID, "目录名无效"))
}

// ---------------------------------------------------------------- 诊断入口

/// `--scan <目录> <输出json>`：不开 GUI 跑一次扫描并写报告（验收 2 计时用）
fn run_scan_cli(dir: &str, out: &str) -> i32 {
    let t0 = Instant::now();
    match scan::scan_dir(dir) {
        Ok(files) => {
            let ms = t0.elapsed().as_millis();
            let mut pooled = 0usize;
            for f in &files {
                if tag::should_pool(&f.name, &f.kind, f.size) {
                    pooled += 1;
                }
            }
            let payload = serde_json::json!({
                "dir": dir,
                "elapsedMs": ms,
                "scanned": files.len(),
                "pooled": pooled,
                "skippedByRule": files.len() - pooled,
            });
            match std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("写出失败：{e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("扫描失败：{e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--scan" {
        std::process::exit(run_scan_cli(&args[2], &args[3]));
    }

    tauri::Builder::default()
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            std::fs::create_dir_all(&dir)?;
            let conn = index::open(&dir.join("index.db")).map_err(|e| e.to_string())?;
            app.manage(AppState::new(conn, dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_directory,
            open_path,
            reveal_path,
            disk_free_gb,
            boot_state,
            add_source_root,
            list_source_roots,
            remove_source_root,
            set_archive_root,
            scan_all,
            list_dirs,
            preview_dir_name,
            create_dir
        ])
        .run(tauri::generate_context!())
        .expect("应用启动失败");
}
