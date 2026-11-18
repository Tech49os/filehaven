//! 文件管家 v2 · 本地智能文件归档客户端
//!
//! P1 阶段：只做元数据扫描 + 人工确认整理，不抽正文；只有「目录建议」用本地模型。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

mod batch;
mod error;
mod index;
mod llm;
mod migrate;
mod migration;
mod model;
mod plan;
mod recommend;
mod scan;
mod state;
mod suggest_dirs;
mod tag;

#[cfg(test)]
mod tests;

use error::CmdError;
use model::{
    ArchiveDir, BatchView, BootState, Counts, DirNode, MigrationResult, MigrationSummary,
    ModelStatus, ScanReport, SourceRoot, UndoReport,
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

// ---------------------------------------------------------------- 批次

fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

#[tauri::command]
async fn next_batch(app: AppHandle, size: Option<i64>) -> Result<BatchView, CmdError> {
    tauri::async_runtime::spawn_blocking(move || next_batch_inner(&app, size.unwrap_or(12)))
        .await
        .map_err(|e| CmdError::new(error::DB, format!("批次线程异常：{e}")))?
}

/// 一次人工判定（单事务）
#[tauri::command]
async fn decide(app: AppHandle, req: model::DecideReq) -> Result<model::DecideAck, CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::apply_decide(&mut conn, &req)
}

/// Ctrl+Z：撤销本批最近一次确认
#[tauri::command]
async fn undo_decide(
    app: AppHandle,
    batch_id: i64,
) -> Result<Option<model::BatchItem>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || undo_decide_inner(&app, batch_id))
        .await
        .map_err(|e| CmdError::new(error::DB, format!("撤销线程异常：{e}")))?
}

fn undo_decide_inner(app: &AppHandle, batch_id: i64) -> Result<Option<model::BatchItem>, CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    let Some(file) = index::undo_last_decide(&mut conn, batch_id)? else {
        return Ok(None);
    };
    // 回填要带候选，否则前端恢复的卡片没有可选项 —— 复用挑批那套打分
    let scored = scored_pool(&conn)?;
    let (cands, needs) = scored
        .items
        .iter()
        .find(|(r, _, _)| r.id == file.id)
        .map(|(_, c, n)| (c.clone(), *n))
        .unwrap_or((Vec::new(), true));
    Ok(Some(model::BatchItem {
        file,
        candidates: cands,
        needs_you: needs,
    }))
}

#[tauri::command]
async fn close_batch(app: AppHandle, batch_id: i64) -> Result<model::CloseAck, CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::close_batch_cmd(&mut conn, batch_id)
}

#[tauri::command]
async fn accept_dirs(app: AppHandle, names: Vec<String>) -> Result<Vec<ArchiveDir>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::accept_dirs(&conn, &names)
}

// ---------------------------------------------------------------- 迁移

/// 归档根（没有就 NOT_CONFIGURED —— 迁移类命令一律靠它）
fn archive_root_of(conn: &rusqlite::Connection) -> Result<String, CmdError> {
    setting(conn, "archive_root")
        .ok_or_else(|| CmdError::new(error::NOT_CONFIGURED, "还没选择归档区，先去设置里选一个"))
}

/// 剩余空间：先看诊断用的环境变量，再问系统。
///
/// `FILEBUTLER_FAKE_FREE_GB` 只为验 R7（空间不足）留的后门，与 `FILEBUTLER_TIMEOUT` 同一路数。
fn free_gb_for(archive: &str) -> Option<f64> {
    if let Ok(v) = std::env::var("FILEBUTLER_FAKE_FREE_GB") {
        if let Ok(g) = v.parse::<f64>() {
            return Some(g);
        }
    }
    free_gb(archive)
}

/// 迁移预览：分组 / 冲突 / 已改拦截 / 空间（T7.2）
#[tauri::command]
async fn migration_preview(app: AppHandle) -> Result<model::MigrationPreview, CmdError> {
    tauri::async_runtime::spawn_blocking(move || migration_preview_inner(&app))
        .await
        .map_err(|e| CmdError::new(error::DB, format!("预览线程异常：{e}")))?
}

fn migration_preview_inner(app: &AppHandle) -> Result<model::MigrationPreview, CmdError> {
    let state = app.state::<AppState>();
    let archive = {
        let conn = lock_db(&state)?;
        archive_root_of(&conn)?
    };
    // 剩余空间要起进程去问，放锁外
    let free = free_gb_for(&archive);
    let conn = lock_db(&state)?;
    migration::preview(&conn, Path::new(&archive), free)
}

/// 应用迁移（长任务，每文件一事务，三次进度事件）
#[tauri::command]
async fn apply_migration(
    app: AppHandle,
    exclude_file_ids: Vec<i64>,
) -> Result<MigrationResult, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut conn = lock_db(&state)?;
        let archive = archive_root_of(&conn)?;
        let root = PathBuf::from(&archive);

        // R7：预览可能放了一阵子，真搬之前再确认一次空间
        let free = free_gb_for(&archive);
        let pre = migration::preview(&conn, &root, free)?;
        if !pre.enough_space {
            return Err(CmdError::new(
                error::DISK_FULL,
                format!(
                    "归档盘剩余空间不够（要 {:} MB，只剩 {:.1} GB）",
                    pre.total_bytes / 1024 / 1024,
                    free.unwrap_or(0.0)
                ),
            ));
        }

        let emitter = app.clone();
        let mut on_progress = |done: usize, total: usize, name: &str, stage: &str| {
            let _ = emitter.emit(
                "migrate-progress",
                model::MigrateProgress {
                    done,
                    total,
                    name: name.to_string(),
                    stage: stage.to_string(),
                },
            );
        };
        migration::apply(&mut conn, &root, &exclude_file_ids, &mut on_progress)
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("迁移线程异常：{e}")))?
}

/// 撤销一次迁移（逆序还原，能救几份救几份）
#[tauri::command]
async fn undo_migration(app: AppHandle, migration_id: i64) -> Result<UndoReport, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut conn = lock_db(&state)?;
        let archive = archive_root_of(&conn)?;
        let root = PathBuf::from(&archive);
        let emitter = app.clone();
        let mut on_progress = |done: usize, total: usize, name: &str, stage: &str| {
            let _ = emitter.emit(
                "undo-progress",
                model::MigrateProgress {
                    done,
                    total,
                    name: name.to_string(),
                    stage: stage.to_string(),
                },
            );
        };
        migration::undo(&mut conn, &root, migration_id, &mut on_progress)
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("撤销线程异常：{e}")))?
}

/// 最近一次迁移 —— 撤销入口（重启后也还撤得回来）
#[tauri::command]
async fn latest_migration(app: AppHandle) -> Result<Option<MigrationSummary>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    Ok(index::latest_migration(&conn)?.map(|m| MigrationSummary {
        id: m.id,
        status: m.status,
        file_count: m.file_count,
        bytes: m.bytes,
        finished_at: m.finished_at,
    }))
}

/// 重新确认：标注后被改过的文件退回整理台，并按磁盘现状刷新 size/mtime
#[tauri::command]
async fn requeue_confirmed(app: AppHandle, file_id: i64) -> Result<(), CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let path = {
            let conn = lock_db(&state)?;
            index::file_row(&conn, file_id)?
                .ok_or_else(|| CmdError::new("NOT_FOUND", format!("文件不存在：{file_id}")))?
                .abs_path
        };
        // stat 在锁外
        let meta = std::fs::metadata(&path)?;
        let conn = lock_db(&state)?;
        index::requeue_confirmed(&conn, file_id, meta.len() as i64, plan::mtime_ms(&meta))
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("重新确认线程异常：{e}")))?
}

// ---------------------------------------------------------------- 目录建议

/// 模型请求超时
///
/// 详设 §7.6 写的是 180s，M8 按实测改成 240s（偏差记录见 `P1_执行状态.md`）：
/// T5.8 三次实跑 81.6s / 164.4s / 114.3s，164s 那次离 180s 只剩 16 秒。
/// 耗时抖动的真因不是 prompt 长度，而是模型加载与显存分层 —— 上限给宽一点，
/// 换来的是「本来能成的一次不会白等 3 分钟还被降级」。超时仍走 `MODEL_TIMEOUT` 降级。
pub const SUGGEST_TIMEOUT_SECS: u64 = 240;
/// 等待期心跳间隔：只报「已等多少秒」，不报假百分比
const SUGGEST_HEARTBEAT_MS: u64 = 5_000;

/// 目录建议：抽样 → 本地模型 → 五条门槛
///
/// 失败一律返回 `CmdError`（详设 §6.3 第 12 条），前端按 `code` 降级到手写。
/// 整个流程（含最长 240s 等待）都**不持数据库锁**（铁律见 `state.rs`）。
#[tauri::command]
async fn suggest_dirs(
    app: AppHandle,
    force: Option<bool>,
) -> Result<suggest_dirs::DirSuggestion, CmdError> {
    let state = app.state::<AppState>();
    let (base, model, files, existing) = {
        let conn = lock_db(&state)?;
        let base = setting(&conn, "ollama_base").unwrap_or_else(|| llm::DEFAULT_BASE.to_string());
        let model = setting(&conn, "ollama_model").unwrap_or_else(|| llm::DEFAULT_MODEL.to_string());
        let files = index::suggest_input_rows(&conn)?;
        let existing: Vec<String> = index::list_dirs(&conn)?
            .into_iter()
            .map(|d| d.rel_path)
            .collect();
        (base, model, files, existing)
    };

    // 模型没起要立刻说清，别让用户干等 240 秒
    if !llm::reachable(&base, Duration::from_secs(3)) {
        return Err(CmdError::new(
            error::MODEL_DOWN,
            format!("本地模型未启动（{base} 连不上），先打开 Ollama，或者直接自己写目录"),
        ));
    }

    let total = files.len();
    let _ = app.emit(
        "suggest-stage",
        model::SuggestStage {
            stage: "sampling".into(),
            elapsed_ms: 0,
            note: format!("共 {total} 份文件，抽样后送模型"),
        },
    );

    // 心跳线程：阻塞请求期间每 5s 补报一次，界面才不会像卡死
    let stop = Arc::new(AtomicBool::new(false));
    let hb = {
        let app = app.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut ticks: u32 = 0;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                ticks += 1;
                if ticks as u64 % (SUGGEST_HEARTBEAT_MS / 250) == 0 {
                    let _ = app.emit(
                        "suggest-stage",
                        model::SuggestStage {
                            stage: "waiting".into(),
                            elapsed_ms: t0.elapsed().as_millis() as u64,
                            note: String::new(),
                        },
                    );
                }
            }
        })
    };

    let force = force.unwrap_or(false);
    let stage_app = app.clone();
    let req = tauri::async_runtime::spawn_blocking(move || {
        let mut on_stage = |stage: &str, ms: u64| {
            let _ = stage_app.emit(
                "suggest-stage",
                model::SuggestStage {
                    stage: stage.to_string(),
                    elapsed_ms: ms,
                    note: String::new(),
                },
            );
        };
        suggest_dirs::run(
            suggest_dirs::RunInput {
                base: &base,
                model: &model,
                files: &files,
                existing_dirs: &existing,
                oldest_first: force,
                timeout: Duration::from_secs(SUGGEST_TIMEOUT_SECS),
            },
            &mut on_stage,
        )
    })
    .await;

    stop.store(true, Ordering::Relaxed);
    let _ = hb.join();

    req.map_err(|e| CmdError::new(error::DB, format!("目录建议线程异常：{e}")))?
}

/// 池内每份文件算一次候选（挑批与撤销回填共用）
struct ScoredPool {
    items: Vec<(model::FileRow, Vec<model::Candidate>, bool)>,
    dir_count: usize,
}

fn scored_pool(conn: &rusqlite::Connection) -> Result<ScoredPool, CmdError> {
    let dir_rows = index::dir_feature_rows(conn)?;
    let mut rel_paths: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    let mut dirs: Vec<(i64, recommend::DirFeature)> = Vec::new();
    for (id, name, rel, confirmed_names) in &dir_rows {
        rel_paths.insert(*id, rel.clone());
        let mut confirmed: Vec<String> = Vec::new();
        for n in confirmed_names {
            for t in tag::tokenize(stem_of(n)) {
                if !confirmed.contains(&t) {
                    confirmed.push(t);
                }
            }
        }
        dirs.push((
            *id,
            recommend::DirFeature {
                name_tokens: tag::tokenize(name),
                dir_name: name.clone(),
                confirmed_tokens: confirmed,
                file_count: confirmed_names.len(),
            },
        ));
    }

    let pool = index::pool_rows(conn)?;
    let mut items = Vec::with_capacity(pool.len());
    for row in &pool {
        let stem = stem_of(&row.name);
        let feat = recommend::FileFeature {
            tokens: tag::tokenize(stem),
            stem: stem.to_string(),
            kind: row.kind.clone(),
            mtime: row.mtime,
        };
        let (cands, needs) = recommend::candidates(&feat, &dirs, &rel_paths);
        items.push((row.clone(), cands, needs));
    }
    Ok(ScoredPool {
        items,
        dir_count: dirs.len(),
    })
}

fn next_batch_inner(app: &AppHandle, size: i64) -> Result<BatchView, CmdError> {
    use index::Status;

    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;

    // 上一个没关的批次先关掉：未确认的回池
    if let Some((id, _, _)) = index::open_batch(&conn)? {
        index::close_batch(&mut conn, id)?;
    }
    let seq = index::next_batch_seq(&conn)?;
    let batch_id = index::insert_batch(&conn, seq, size)?;

    // 池内每份文件算一次候选，挑批次时直接复用，不重算
    let scored_all = scored_pool(&conn)?;
    let pool_count = scored_all.items.len() as i64;

    let pool_items: Vec<batch::PoolItem> = scored_all
        .items
        .iter()
        .map(|(row, cands, needs)| batch::PoolItem {
            file_id: row.id,
            primary_dir: if *needs {
                None
            } else {
                cands.first().map(|c| c.dir_id)
            },
            score: cands.first().map(|c| c.score).unwrap_or(0.0),
            defer_count: row.defer_count,
            mtime: row.mtime,
            has_deep_tag: false,
        })
        .collect();
    let picked = batch::pick(&pool_items, size as usize);

    let mut items: Vec<model::BatchItem> = Vec::new();
    let mut deferred_badge: Vec<i64> = Vec::new();
    for id in &picked {
        let Some((file, cands, needs)) = scored_all.items.iter().find(|(r, _, _)| r.id == *id) else {
            continue;
        };
        let mut file = file.clone();
        // 暂缓回来的文件是 queued，必须先回到 labeled 才能进批次（详设 §5.3）
        if file.status == "queued" {
            index::set_status(&conn, file.id, Status::Labeled)?;
            file.status = "labeled".into();
        }
        if file.status != "pending_confirm" {
            index::set_status(&conn, file.id, Status::PendingConfirm)?;
            file.status = "pending_confirm".into();
        }
        if file.defer_count >= batch::DEFER_BADGE_AT {
            deferred_badge.push(file.id);
        }
        items.push(model::BatchItem {
            file,
            candidates: cands.clone(),
            needs_you: *needs,
        });
    }

    let remaining = pool_count - (picked.len() as i64);

    Ok(model::BatchView {
        batch_id,
        seq,
        size,
        items,
        remaining_in_pool: remaining.max(0),
        dir_count: scored_all.dir_count as i64,
        deferred_badge,
    })
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

/// `--suggest <样本json> <输出json> [force]`：不开 GUI 跑一次目录建议（T5.8 验收计时用）
///
/// 走的是与界面**完全同一条**管线（`suggest_dirs::run`），
/// 区别只在于样本来自 fixture 而不是数据库、进度打印到 stderr。
fn run_suggest_cli(samples: &str, out: &str, force: bool) -> i32 {
    #[derive(serde::Deserialize)]
    struct Raw {
        name: String,
        kind: String,
        #[serde(default)]
        size: u64,
        mtime: i64,
        path: Option<String>,
    }

    let text = match std::fs::read_to_string(samples) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("读样本失败：{e}");
            return 1;
        }
    };
    let raws: Vec<Raw> = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("样本不是 JSON 数组：{e}");
            return 1;
        }
    };

    let files: Vec<suggest_dirs::SampleInput> = raws
        .into_iter()
        // 与扫描入库同一口径：压缩包/程序/空文件在库里就是 skipped，
        // 诊断入口必须一起过滤，否则「系统工具软件」这种目录会凭 ZIP/EXE 混进来
        .filter(|r| {
            let kind = tag::normalize_kind(&r.kind);
            tag::should_pool(&r.name, kind, r.size)
        })
        .map(|r| suggest_dirs::SampleInput {
            name: r.name,
            kind: tag::normalize_kind(&r.kind).to_string(),
            source: r.path.as_deref().map(suggest_dirs::source_of).unwrap_or_default(),
            mtime: r.mtime,
        })
        .collect();

    let base = std::env::var("FILEBUTLER_BASE").unwrap_or_else(|_| llm::DEFAULT_BASE.to_string());
    let model = std::env::var("FILEBUTLER_MODEL").unwrap_or_else(|_| llm::DEFAULT_MODEL.to_string());
    // 只在诊断入口留的超时旋钮：用来验「超时降级」这一支（界面恒定 SUGGEST_TIMEOUT_SECS）
    let timeout_secs = std::env::var("FILEBUTLER_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(SUGGEST_TIMEOUT_SECS);

    let t0 = Instant::now();
    let mut on_stage = |stage: &str, ms: u64| eprintln!("[{stage}] {ms}ms");
    let res = suggest_dirs::run(
        suggest_dirs::RunInput {
            base: &base,
            model: &model,
            files: &files,
            existing_dirs: &[],
            oldest_first: force,
            timeout: Duration::from_secs(timeout_secs),
        },
        &mut on_stage,
    );

    let payload = match res {
        Ok(s) => {
            let names: Vec<String> = s.option_a.iter().map(|d| d.name.clone()).collect();
            let names_b: Vec<String> = s.option_b.iter().map(|d| d.name.clone()).collect();
            eprintln!(
                "成功：{} 份样本 / {}ms\nA({})：{}\nB({})：{}",
                s.sample_count,
                s.elapsed_ms,
                s.dimension_a,
                names.join("、"),
                s.dimension_b,
                names_b.join("、")
            );
            serde_json::json!({
                "ok": true,
                "wallMs": t0.elapsed().as_millis() as u64,
                "model": s.model,
                "sampleCount": s.sample_count,
                "elapsedMs": s.elapsed_ms,
                "suggestion": s,
            })
        }
        Err(e) => {
            eprintln!("失败：[{}] {}", e.code, e.message);
            serde_json::json!({
                "ok": false,
                "wallMs": t0.elapsed().as_millis() as u64,
                "model": model,
                "error": { "code": e.code, "message": e.message },
            })
        }
    };

    match std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("写出失败：{e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--scan" {
        std::process::exit(run_scan_cli(&args[2], &args[3]));
    }
    if args.len() >= 4 && args[1] == "--suggest" {
        let force = args.get(4).map(|a| a == "force").unwrap_or(false);
        std::process::exit(run_suggest_cli(&args[2], &args[3], force));
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
            create_dir,
            accept_dirs,
            suggest_dirs,
            next_batch,
            decide,
            undo_decide,
            close_batch,
            migration_preview,
            apply_migration,
            undo_migration,
            latest_migration,
            requeue_confirmed
        ])
        .run(tauri::generate_context!())
        .expect("应用启动失败");
}
