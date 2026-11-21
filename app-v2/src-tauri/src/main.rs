//! 文件管家 v2 · 本地智能文件归档客户端
//!
//! P1 阶段：只做元数据扫描 + 人工确认整理，不抽正文；只有「目录建议」用本地模型。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

mod batch;
mod deep_tag;
mod error;
// 正文抽取：P0 写好后一直没接线，整理台的「看内容」才真正用上它
#[allow(dead_code)]
mod extract;
mod index;
mod llm;
mod migrate;
mod migration;
mod model;
mod model_gate;
mod plan;
mod preview;
mod recommend;
mod scan;
mod state;
mod suggest_dirs;
mod tag;
mod watch;

#[cfg(test)]
mod tests;

use error::CmdError;
use model::{
    ArchiveDir, BatchView, BootState, Counts, DirNode, MigrationResult, MigrationSummary,
    ModelStatus, RecatOutcome, ScanReport, SourceRoot, UndoReport,
};
use state::AppState;

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, CmdError> {
    state
        .db
        .lock()
        .map_err(|_| CmdError::new(error::DB, "数据库锁已损坏，请重启应用"))
}

/// 监听线程专用的取锁：抢不到就退避 200ms 再试一次（T3.6）。
///
/// 界面线程随时可能拿着锁在渲染，监听不能因此丢事件；但也不能死等 ——
/// 等不到就丢这一批，下一轮启动扫描会补上（**不丢数据，只丢时机**）。
/// 单独抽出来是为了能单测「主线程持锁时监听侧 ≤1.2s 就放弃且不 panic」。
fn lock_db_retry(
    db: &std::sync::Mutex<rusqlite::Connection>,
    backoff: Duration,
) -> Option<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    if let Ok(g) = db.try_lock() {
        return Some(g);
    }
    std::thread::sleep(backoff);
    db.try_lock().ok()
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

/// explorer 的「定位并选中」参数
///
/// 2026-11-20 实测反馈：「定位会打开文档目录，不会打开文件所在的文件夹」。
/// 真因是把它当成**一个普通参数**交给 `Command::arg`：std 只在参数含空格时才加引号，
/// 于是 explorer 收到的是 `/select,C:\a b\x.docx` 或 `"/select,C:\a b\x.docx"` ——
/// 两种都不是 explorer 认的写法（它要 `/select,"路径"`，路径单独成引号段），
/// 解析失败后 explorer 不报错，只是回落到默认目录（文档）。
///
/// 所以这里返回**原始命令行片段**，配合 `raw_arg` 绕开 std 的引号规则。
fn explorer_select_arg(path: &str) -> String {
    format!("/select,\"{path}\"")
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
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .raw_arg(explorer_select_arg(&p.to_string_lossy()))
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

/// 直接问内核，**绝不起子进程**。
///
/// 早先的实现是 `Command::new("powershell").output()` —— 本程序是
/// `windows_subsystem = "windows"` 的 GUI 进程，起控制台程序会连带弹出黑框，
/// 一闪即逝。凡是查空间的入口都会闪：设置页一进去（磁盘可用）、选归档区、
/// 预演迁移、扫完刷新。改成 `GetDiskFreeSpaceExW` 后是微秒级、无副作用。
fn free_gb(path: &str) -> Option<f64> {
    let p = PathBuf::from(path);
    let target = p.ancestors().find(|a| a.exists())?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let mut wide: Vec<u16> = target.as_os_str().encode_wide().collect();
        wide.push(0);
        let (mut to_caller, mut total, mut total_free) = (0u64, 0u64, 0u64);
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut to_caller,
                &mut total,
                &mut total_free,
            )
        };
        if ok == 0 {
            return None;
        }
        let _ = (to_caller, total);
        Some(total_free as f64 / 1024.0 / 1024.0 / 1024.0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        None
    }
}

// ---------------------------------------------------------------- 启动状态

/// 启动第一调：决定前端路由（引导 / 整理台）
///
/// `probe_model` 只在**启动首调**传 true。模型探活是同步 HTTP + 3s 超时，
/// 而 `boot_state` 又被「每次判定后刷新计数」高频调用 —— 挂在别的路径上，
/// 连按回车就会一顿一顿的（8GB 显存机器上尤其明显）。
#[tauri::command]
async fn boot_state(app: AppHandle, probe_model: Option<bool>) -> Result<BootState, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        boot_state_inner(&app, probe_model.unwrap_or(false))
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("启动检查线程异常：{e}")))?
}

fn boot_state_inner(app: &AppHandle, probe_model: bool) -> Result<BootState, CmdError> {
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
    let watch_enabled = setting(&conn, "watch_enabled")
        .map(|v| v != "0")
        .unwrap_or(true);
    drop(conn);

    // 模型没起不能挡住任何别的功能（验收 14）：探活失败只是标记不可达
    let reachable = if probe_model {
        let r = llm::reachable(&base, Duration::from_secs(3));
        state.model_reachable.store(r, Ordering::Relaxed);
        r
    } else {
        state.model_reachable.load(Ordering::Relaxed)
    };

    Ok(BootState {
        configured: archive_root.is_some() && !source_roots.is_empty(),
        archive_root,
        source_roots,
        counts,
        model: ModelStatus {
            reachable,
            model: Some(model),
        },
        watch_enabled,
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
    let root = index::add_source_root(&conn, &path)?;
    drop(conn);
    // 新源区也要能显示图片缩略图（asset 协议白名单见 setup）
    let _ = app.asset_protocol_scope().allow_directory(&path, true);
    Ok(root)
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

    {
        // 记下「上次扫描是什么时候」，启动增量扫描靠它做 10 分钟节流（T3.2）
        let state = app.state::<AppState>();
        let wrote = match lock_db(&state) {
            Ok(conn) => index::set_setting(&conn, "last_scan_at", &index::now_ms().to_string()),
            Err(e) => Err(e),
        };
        let _ = wrote;
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

/// 按**现在的**目录集合重算指定文件的候选（不换批次、不动状态、不动焦点）
/// 起因（2026-11-20 反馈）：候选是开批时算一次的，之后新建/用过的目录不会回流，
/// 「我新建了目录，后面那些文件该归哪」就没人管了。重算只依赖目录特征与文件名，
/// 纯 CPU、毫秒级，所以每次新建目录后直接重跑一遍即可。
#[tauri::command]
async fn rescore_files(
    app: AppHandle,
    file_ids: Vec<i64>,
) -> Result<Vec<model::BatchItem>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        let scored = scored_pool(&conn)?;
        let wanted: std::collections::HashSet<i64> = file_ids.into_iter().collect();
        Ok(scored
            .items
            .into_iter()
            .filter(|(r, _, _)| wanted.contains(&r.id))
            .map(|(file, candidates, needs_you)| model::BatchItem {
                file,
                candidates,
                needs_you,
            })
            .collect())
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("重算线程异常：{e}")))?
}

#[tauri::command]
async fn accept_dirs(app: AppHandle, names: Vec<String>) -> Result<Vec<ArchiveDir>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::accept_dirs(&conn, &names)
}

// ---------------------------------------------------------------- 待定区（P2 M4）

/// 待定区列表：每份带**现算**的候选（T4.1）
///
/// 空列表也要能正常返回 —— 「待定区是空的」是好消息，不是错误。
#[tauri::command]
async fn list_parked(app: AppHandle) -> Result<Vec<model::ParkedItem>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        let rows = index::rows_by_status(&conn, "parked")?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let (scored, _) = score_rows(&conn, &rows)?;
        Ok(scored
            .into_iter()
            .map(|(file, candidates, needs_you)| model::ParkedItem {
                file,
                candidates,
                needs_you,
            })
            .collect())
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("待定区线程异常：{e}")))?
}

/// 捞回：`parked → pending_confirm`，下批就会出现（T4.2）
#[tauri::command]
async fn unpark(app: AppHandle, file_id: i64) -> Result<(), CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::unpark_file(&mut conn, file_id)
}

/// 直接指定：`parked → confirmed` + `user_picked` 的 assignment（T4.2）
///
/// `origin` 默认 `user_picked`：待定区里点主推 = 人自己挑的，不是系统推的。
#[tauri::command]
async fn assign_parked(
    app: AppHandle,
    file_id: i64,
    dir_id: i64,
    origin: Option<String>,
) -> Result<(), CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::assign_parked_file(
        &mut conn,
        file_id,
        dir_id,
        origin.as_deref().unwrap_or("user_picked"),
    )
}

/// 移出：`parked → skipped`（不当成丢失，也不再出现）
#[tauri::command]
async fn drop_file(app: AppHandle, file_id: i64) -> Result<(), CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::drop_file(&mut conn, file_id)
}

// ---------------------------------------------------------------- 标签（P3 M1）

/// 人加一个标签（一律 `kind='user'`）
#[tauri::command]
async fn add_tag(
    app: AppHandle,
    file_id: i64,
    name: String,
) -> Result<model::TagRef, CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::add_tag(&mut conn, file_id, &name)
}

/// 删一个标签（`type` / `time` 会被 `TAG_LOCKED` 拒掉）
#[tauri::command]
async fn remove_tag(app: AppHandle, file_id: i64, tag_id: i64) -> Result<(), CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::remove_tag(&mut conn, file_id, tag_id)
}

/// 标签词表（设置页）：user / topic + 使用份数
#[tauri::command]
async fn list_tags(app: AppHandle) -> Result<Vec<model::TagCount>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::list_tags(&conn)
}

#[tauri::command]
async fn delete_tag(app: AppHandle, tag_id: i64) -> Result<usize, CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::delete_tag(&mut conn, tag_id)
}

/// 改名，撞名则合并；返回最终留下的标签 id
#[tauri::command]
async fn rename_tag(app: AppHandle, tag_id: i64, name: String) -> Result<i64, CmdError> {
    let state = app.state::<AppState>();
    let mut conn = lock_db(&state)?;
    index::rename_tag(&mut conn, tag_id, &name)
}

/// 浏览页目录树（P3 T2.4）：归档目录 + 直属已迁移份数
#[tauri::command]
async fn browse_tree(app: AppHandle) -> Result<Vec<model::DirNode>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::browse_tree(&conn)
    })
    .await
    .map_err(|e| CmdError::new("JOIN_ERROR", e.to_string()))?
}

/// 浏览页列表（P3 T2.3）：`dirId=None` 是「未归类」分组
#[tauri::command]
async fn browse_dir(
    app: AppHandle,
    dir_id: Option<i64>,
    include_subdirs: bool,
) -> Result<Vec<model::FileRow>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::browse_dir(&conn, dir_id, include_subdirs)
    })
    .await
    .map_err(|e| CmdError::new("JOIN_ERROR", e.to_string()))?
}

/// 整理池全量（P3 T2.5：浏览页「待整理」虚拟视图，只读）
#[tauri::command]
async fn pool_rows(app: AppHandle) -> Result<Vec<model::FileRow>, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::pool_rows(&conn)
    })
    .await
    .map_err(|e| CmdError::new("JOIN_ERROR", e.to_string()))?
}

/// 检索请求入参（camelCase，§3.1 的 SearchReq）
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchReqIn {
    #[serde(default)]
    q: String,
    #[serde(default)]
    tags: Vec<i64>,
    /// and | or（默认 and）
    #[serde(default = "default_tag_mode")]
    tag_mode: String,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    from: Option<i64>,
    #[serde(default)]
    to: Option<i64>,
    /// archive | pool | parked | all（默认 all）
    #[serde(default = "default_scope")]
    scope: String,
    #[serde(default)]
    dir_id: Option<i64>,
    #[serde(default)]
    include_subdirs: bool,
    /// mtime | name | size（默认 mtime）
    #[serde(default = "default_sort")]
    sort: String,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}
fn default_tag_mode() -> String {
    "and".into()
}
fn default_scope() -> String {
    "all".into()
}
fn default_sort() -> String {
    "mtime".into()
}
fn default_limit() -> i64 {
    50
}

/// 检索结果：`total` 与 `items` 同源（服务端算，前端不许重算）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    items: Vec<model::FileRow>,
    total: i64,
}

/// 检索（P3 T3.1）：三个入口共用这一条查询层
#[tauri::command]
async fn search_files(app: AppHandle, req: SearchReqIn) -> Result<SearchResult, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        let r = index::SearchReq {
            q: req.q,
            tags: req.tags,
            tag_and: req.tag_mode != "or",
            kinds: req.kinds,
            from: req.from,
            to: req.to,
            scope: index::Scope::parse(&req.scope),
            dir_id: req.dir_id,
            include_subdirs: req.include_subdirs,
            sort: index::Sort::parse(&req.sort),
            limit: req.limit,
            offset: req.offset,
        };
        let (items, total) = index::search_files(&conn, &r)?;
        Ok(SearchResult { items, total })
    })
    .await
    .map_err(|e| CmdError::new("JOIN_ERROR", e.to_string()))?
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

/// P4：重新归类——归档区内二次迁移（浏览页右键）
#[tauri::command]
async fn recategorize_file(
    app: AppHandle,
    file_id: i64,
    dir_id: i64,
) -> Result<RecatOutcome, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut conn = lock_db(&state)?;
        let archive = archive_root_of(&conn)?;
        let root = PathBuf::from(&archive);
        migration::recategorize(&mut conn, &root, file_id, dir_id)
    })
    .await
    .map_err(|e| CmdError::new(error::DB, format!("重新归类线程异常：{e}")))?
}

/// 最近一次迁移 —— 撤销入口（重启后也还撤得回来）
#[tauri::command]
async fn latest_migration(app: AppHandle) -> Result<Option<MigrationSummary>, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    Ok(index::latest_migration(&conn)?.map(|m| MigrationSummary {
        id: m.id,
        status: m.status,
        kind: m.kind,
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

/// 写备注（整理台双击「先放着」那一行）。空白等于清空。
#[tauri::command]
async fn save_note(app: AppHandle, file_id: i64, note: String) -> Result<(), CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    index::set_note(&conn, file_id, &note)
}

/// 归档前顺手改名（只改库里记的目标名，磁盘上的源文件一动不动）
///
/// 返回清洗后的名字（已补回扩展名）；传空串表示改回原名。
/// 迁移时 `migration::snapshot` 会把这个名字当成目标名，落盘就是新名字；
/// 源文件原名留在隔离区的相对路径里，撤销照样能还原。
#[tauri::command]
async fn rename_file(app: AppHandle, file_id: i64, new_name: String) -> Result<String, CmdError> {
    let state = app.state::<AppState>();
    let conn = lock_db(&state)?;
    if new_name.trim().is_empty() {
        index::set_target_name(&conn, file_id, None)?;
        return Ok(String::new());
    }
    let row = index::file_row(&conn, file_id)?
        .ok_or_else(|| CmdError::new("NOT_FOUND", format!("文件不存在：{file_id}")))?;
    let p = plan::clean_file_name(&new_name, &row.ext);
    if !p.valid {
        return Err(CmdError::new(
            error::DIR_INVALID,
            p.reason.unwrap_or_else(|| "文件名无效".into()),
        ));
    }
    index::set_target_name(&conn, file_id, Some(&p.cleaned))?;
    Ok(p.cleaned)
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
/// `hint` 是用户写的补充要求（「按客户分」「主要做招投标」这类）——
/// 有了它，「再来一版」才不是同一套逻辑空转。空白/缺省一律当没提。
///
/// 失败一律返回 `CmdError`（详设 §6.3 第 12 条），前端按 `code` 降级到手写。
/// 整个流程（含最长 240s 等待）都**不持数据库锁**（铁律见 `state.rs`）。
#[tauri::command]
async fn suggest_dirs(
    app: AppHandle,
    force: Option<bool>,
    hint: Option<String>,
) -> Result<suggest_dirs::DirSuggestion, CmdError> {
    let hint = hint
        .as_deref()
        .and_then(suggest_dirs::normalize_hint)
        .unwrap_or_default();
    let state = app.state::<AppState>();
    let (base, model, files, existing) = {
        let conn = lock_db(&state)?;
        let (base, model) = llm_cfg(&conn);
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

    // 显存仲裁（P3 §3.4 ModelGate）：目录建议 / 文本深读 / 视觉深读 三向互斥。
    // 先按住队列（paused_for_suggest 语义不动），再占模型闸 —— 最多等 70s
    //（单份超时 60s + 余量）。等不到就放弃这次建议，**绝不硬上**：
    // 两个请求同压 8GB 显存会把 llama-server 打崩。
    let got = {
        let q = app.state::<Arc<deep_tag::Queue>>().inner().clone();
        q.paused_for_suggest.store(true, Ordering::Relaxed);
        q.gate.acquire(model_gate::Owner::Suggest, 70_000)
    };
    if !got {
        app.state::<Arc<deep_tag::Queue>>()
            .paused_for_suggest
            .store(false, Ordering::Relaxed);
        return Err(CmdError::new(
            error::MODEL_BUSY,
            "深度解读正在占用模型（等了 70 秒还没空闲），稍后再试",
        ));
    }

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
                hint: if hint.trim().is_empty() {
                    None
                } else {
                    Some(hint.trim())
                },
                timeout: Duration::from_secs(SUGGEST_TIMEOUT_SECS),
            },
            &mut on_stage,
        )
    })
    .await;

    stop.store(true, Ordering::Relaxed);
    let _ = hb.join();
    // 无论成功失败都要放闸、放开队列，否则一次超时就把深度标签永久按停
    {
        let q = app.state::<Arc<deep_tag::Queue>>();
        q.gate.release(model_gate::Owner::Suggest);
        q.paused_for_suggest.store(false, Ordering::Relaxed);
    }

    req.map_err(|e| CmdError::new(error::DB, format!("目录建议线程异常：{e}")))?
}

// ---------------------------------------------------------------- 正文预览

/// 整理台「看一眼内容」：返回正文摘要（长文截断）/ 图片标记 / 读不出的原因
///
/// 抽正文要解 zip，属于阻塞 IO，必须扔给 blocking 线程池 —— 否则一张 20MB 的 pptx
/// 卡住的是整个界面。前端按 fileId 缓存结果，同一份文件只读一次。
#[tauri::command]
async fn preview_file(app: AppHandle, file_id: i64) -> Result<preview::Preview, CmdError> {
    let state = app.state::<AppState>();
    let row = {
        let conn = lock_db(&state)?;
        index::file_row(&conn, file_id)?.ok_or_else(|| {
            CmdError::new(error::DB, format!("找不到这份文件的记录（id={file_id}）"))
        })?
    };

    tauri::async_runtime::spawn_blocking(move || {
        preview::make(
            // 已归档的文件源路径已进隔离区，读归档后的真实路径（P3 T2.1）
            Path::new(row.live_path()),
            &row.kind,
            &row.ext,
            row.size.max(0) as u64,
        )
    })
    .await
    .map_err(|e| CmdError::new(error::IO, format!("预览线程异常：{e}")))
}

/// 池内每份文件算一次候选（挑批与撤销回填共用）
struct ScoredPool {
    items: Vec<(model::FileRow, Vec<model::Candidate>, bool)>,
    dir_count: usize,
}

/// 目录行 → `DirFeature`。评测（`--eval`）与真实池**必须走同一个函数**，
/// 否则「评测说变准了、线上没变」这种事会看不出来。
///
/// `rel_path` 拆成各段：nameMatch 认的就是「路径上每一段」，只取叶子段会漏掉
/// 真实线索（详见 `recommend.rs` 头部第 5 条）。
fn dir_feature_of(
    rel_path: &str,
    name: &str,
    confirmed_names: &[String],
    kind_hist: &HashMap<String, i64>,
    topic_tags: &[String],
) -> recommend::DirFeature {
    let seg_names: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    let seg_names = if seg_names.is_empty() { vec![name] } else { seg_names };
    let segments: Vec<recommend::DirSeg> = seg_names
        .iter()
        .map(|s| recommend::DirSeg {
            name: (*s).to_string(),
            tokens: tag::tokenize(s),
        })
        .collect();

    let mut confirmed_tokens: Vec<String> = Vec::new();
    let mut confirmed_files: Vec<Vec<String>> = Vec::new();
    for n in confirmed_names {
        let toks = tag::tokenize(stem_of(n));
        for t in &toks {
            if !confirmed_tokens.contains(t) {
                confirmed_tokens.push(t.clone());
            }
        }
        confirmed_files.push(toks);
    }

    recommend::DirFeature {
        segments,
        dir_name: name.to_string(),
        confirmed_tokens,
        confirmed_files,
        file_count: confirmed_names.len(),
        kind_hist: kind_hist.clone(),
        topic_tags: topic_tags.to_vec(),
    }
}

fn file_feature_of(name: &str, kind: &str, mtime: i64, topic_tags: Vec<String>) -> recommend::FileFeature {
    let stem = stem_of(name).to_string();
    recommend::FileFeature {
        tokens: tag::tokenize(&stem),
        stem,
        kind: kind.to_string(),
        mtime,
        topic_tags,
    }
}

fn scored_pool(conn: &rusqlite::Connection) -> Result<ScoredPool, CmdError> {
    let (items, dir_count) = score_rows(conn, &index::pool_rows(conn)?)?;
    Ok(ScoredPool { items, dir_count })
}

/// 给**指定的一批文件**算候选（挑批 / 重算 / 待定区共用这一条路径）
///
/// 待定区必须走这里而不是缓存：躺在待定区的文件可能压了几周，
/// 这期间目录长了、别的文件归档过了，几周前的候选早就不作数（T4.1）。
fn score_rows(
    conn: &rusqlite::Connection,
    rows: &[model::FileRow],
) -> Result<
    (
        Vec<(model::FileRow, Vec<model::Candidate>, bool)>,
        usize,
    ),
    CmdError,
> {
    let dir_rows = index::dir_feature_rows(conn)?;
    let mut rel_paths: HashMap<i64, String> = HashMap::new();
    let mut dirs: Vec<(i64, recommend::DirFeature)> = Vec::new();
    for row in &dir_rows {
        rel_paths.insert(row.id, row.rel_path.clone());
        dirs.push((
            row.id,
            dir_feature_of(
                &row.rel_path,
                &row.name,
                &row.confirmed_names,
                &row.kind_hist,
                &row.topic_tags,
            ),
        ));
    }

    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let tags = index::topic_tags_for(conn, &ids)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let feat = file_feature_of(
            &row.name,
            &row.kind,
            row.mtime,
            tags.get(&row.id).cloned().unwrap_or_default(),
        );
        let (cands, needs) = recommend::candidates(&feat, &dirs, &rel_paths);
        items.push((row.clone(), cands, needs));
    }
    Ok((items, dirs.len()))
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

    // 告诉深度标签队列「用户正在看的就是这几份」（T2.6 优先级第一档）。
    // 没有这一步，tagMatch 在整批上都是 0，等于 M1 白做。
    let batch_ids: Vec<i64> = items.iter().map(|i| i.file.id).collect();
    app.state::<Arc<deep_tag::Queue>>().set_batch(batch_ids);

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
            // 诊断入口（--suggest）不吃用户补充要求：它是用来对拍 prompt 的
            hint: None,
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

/// `--eval <cases.json> <out.json> [--no-tags]`：不开 GUI、不碰库、不联网，只算推荐排名
///
/// 输入来自 `test/export-eval.py` 导出的留一集；输出每条 case 的**完整排名**，
/// 阈值网格搜索放在 `test/eval_recommend.py` 里做 —— 改阈值不需要重编译。
///
/// 留一法在这里落地：算某一条 case 的分数前，把它自己的文件名从
/// `expectDir` 那个目录的已确认列表里摘掉，否则等于把答案喂给 familyMatch。
///
/// `--no-tags`（P3 T0.3）把 case 与目录两侧的 `topicTags` 全部清空后再打分，
/// 等价于「标签路关掉」—— A/B 的对照组。**只影响这个子命令**，
/// `recommend::rank` 的线上调用完全不受影响。
fn run_eval_cli(cases: &str, out: &str, no_tags: bool) -> i32 {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RawDir {
        rel_path: String,
        name: String,
        file_names: Vec<String>,
        #[serde(default)]
        kinds: Vec<String>,
        #[serde(default)]
        topic_tags: Vec<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RawCase {
        file_name: String,
        kind: String,
        mtime: i64,
        expect_dir: String,
        #[serde(default)]
        has_deep_tags: bool,
        #[serde(default)]
        topic_tags: Vec<String>,
        #[serde(default)]
        tag_level: String,
    }
    #[derive(serde::Deserialize)]
    struct RawSet {
        dirs: Vec<RawDir>,
        cases: Vec<RawCase>,
    }

    let text = match std::fs::read_to_string(cases) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("读评测集失败：{e}");
            return 1;
        }
    };
    let set: RawSet = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("评测集不是预期 JSON：{e}");
            return 1;
        }
    };

    let mut out_cases: Vec<serde_json::Value> = Vec::with_capacity(set.cases.len());
    for c in &set.cases {
        // 每个 case 重建一次目录特征：留一法要摘掉自己，不能共用一份
        let mut dirs: Vec<(i64, recommend::DirFeature)> = Vec::new();
        let mut rel_paths: HashMap<i64, String> = HashMap::new();
        for (i, d) in set.dirs.iter().enumerate() {
            let id = i as i64 + 1;
            let mut names: Vec<String> = d.file_names.clone();
            let mut kinds: Vec<String> = d.kinds.clone();
            if d.rel_path == c.expect_dir {
                if let Some(p) = names.iter().position(|n| *n == c.file_name) {
                    names.remove(p);
                    if p < kinds.len() {
                        kinds.remove(p);
                    }
                }
            }
            let mut kind_hist: HashMap<String, i64> = HashMap::new();
            for k in &kinds {
                *kind_hist.entry(k.clone()).or_insert(0) += 1;
            }
            rel_paths.insert(id, d.rel_path.clone());
            dirs.push((
                id,
                dir_feature_of(
                    &d.rel_path,
                    &d.name,
                    &names,
                    &kind_hist,
                    // 对照组：标签路关掉
                    if no_tags { &[] } else { &d.topic_tags },
                ),
            ));
        }

        let feat = file_feature_of(
            &c.file_name,
            &c.kind,
            c.mtime,
            if no_tags {
                Vec::new()
            } else {
                c.topic_tags.clone()
            },
        );
        let ranked = recommend::rank(&feat, &dirs);
        let rows: Vec<serde_json::Value> = ranked
            .iter()
            .take(10)
            .map(|(id, s, r)| {
                serde_json::json!({
                    "relPath": rel_paths.get(id).cloned().unwrap_or_default(),
                    "score": s,
                    "reason": r,
                })
            })
            .collect();
        let pos = ranked
            .iter()
            .position(|(id, _, _)| rel_paths.get(id).map(|p| p == &c.expect_dir).unwrap_or(false));
        let (cands, needs) = recommend::candidates(&feat, &dirs, &rel_paths);
        out_cases.push(serde_json::json!({
            "fileName": c.file_name,
            "kind": c.kind,
            "expectDir": c.expect_dir,
            "hasDeepTags": c.has_deep_tags,
            "tagLevel": c.tag_level,
            "topicTags": c.topic_tags,
            // 1 起；null = 没进有理由的排名
            "rank": pos.map(|p| p + 1),
            "ranked": rows,
            "needsYou": needs,
            "primaryHits": cands.first().map(|x| x.rel_path == c.expect_dir).unwrap_or(false),
        }));
    }

    let payload = serde_json::json!({
        "dirCount": set.dirs.len(),
        "caseCount": set.cases.len(),
        "noTags": no_tags,
        // 阈值随产出一起写出来：Python 汇总不能拿自己的一套默认值，
        // 否则「脚本说 7.9%、代码实际 2.6%」这种对不上会一直存在
        "thresholds": {
            "primary": recommend::PRIMARY,
            "uncertainMin": recommend::UNCERTAIN_MIN,
            "altMin": recommend::ALT_MIN,
        },
        "cases": out_cases,
    });
    match std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("写出失败：{e}");
            1
        }
    }
}

// ---------------------------------------------------------------- 深度标签队列（M2）

/// 模型与地址（都从 settings 读，缺省用 llm 的默认值）
///
/// 键名是 `ollama_base` / `ollama_model` —— 设置页写的就是这两个。
/// 曾经有一版写成 `base` / `model`，结果设置页改了模型、队列还在用默认值，
/// 而且不报错（读不到就走缺省），只表现为「怎么变慢了」。别改这里的键名。
fn llm_cfg(conn: &rusqlite::Connection) -> (String, String) {
    let base = setting(conn, "ollama_base").unwrap_or_else(|| llm::DEFAULT_BASE.to_string());
    let model = setting(conn, "ollama_model").unwrap_or_else(|| llm::DEFAULT_MODEL.to_string());
    (base, model)
}

fn emit_deep(app: &AppHandle, q: &deep_tag::Queue) {
    let _ = app.emit("deep-tag-progress", q.snapshot());
}

/// 队列线程：串行取件 → 抽正文 → 问模型 → 落库
///
/// 三条硬约束（违反任何一条都会让 P1 的能力被拖坏）：
/// 1. **持锁期间绝不发 HTTP** —— 模型单份可能 60s，锁住就是整个界面卡死；
/// 2. **失败不冒泡** —— 只记 `failed` 与日志，UI 照常用；
/// 3. **模型没起就停着**，每 20s 探一次，不把 400 份文件挨个试一遍。
fn spawn_deep_tag(app: AppHandle, queue: Arc<deep_tag::Queue>) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let mut note: Option<String> = None;
        loop {
            if queue.snapshot_stopped() {
                break;
            }
            if !queue.enabled() || queue.paused() || queue.paused_for_suggest.load(Ordering::Relaxed)
            {
                std::thread::sleep(Duration::from_millis(400));
                continue;
            }

            // 1) 取下一份（短锁）：插队列优先（T4.5「让 AI 说明」），否则按 SQL 优先级
            let picked = {
                let conn = match lock_db(&state) {
                    Ok(c) => c,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(800));
                        continue;
                    }
                };
                let job = match queue.pop_front() {
                    Some(id) => index::job_by_id(&conn, id).ok().flatten(),
                    None => {
                        let batch = queue.batch.lock().map(|b| b.clone()).unwrap_or_default();
                        index::next_deep_job(&conn, &batch).ok().flatten()
                    }
                };
                let remaining = index::deep_remaining(&conn).unwrap_or(0);
                let (base, model) = llm_cfg(&conn);
                (job, remaining, base, model)
            };

            let (job, remaining, base, model) = picked;
            {
                if let Ok(mut st) = queue.stats_lock() {
                    st.remaining = remaining;
                }
            }

            let Some(job) = job else {
                // 队列空：慢轮询，别空转烧 CPU
                if let Ok(mut st) = queue.stats_lock() {
                    st.running = false;
                    st.current.clear();
                    st.current_id = None;
                }
                emit_deep(&app, &queue);
                std::thread::sleep(Duration::from_millis(1500));
                continue;
            };

            // T4.6：图片任务的口径。kind=image 的送/跳只由 ext/size 决定，不用抽正文就能判
            let vision_job = job.kind == "image"
                && matches!(
                    deep_tag::should_deep_tag(&job.kind, &job.ext, 1, job.size),
                    deep_tag::Decision::Send { .. }
                );

            if job.kind == "image" && !queue.vision_enabled() {
                // 图片解读关着：不读、不占模型闸、不算这份文件失败（本轮跳过，
                // 重新打开时 clear_failed 会再给机会）。计一次 skipped 让设置页看得见
                queue.mark_failed(job.file_id);
                if let Ok(mut st) = queue.stats_lock() {
                    st.vision_skipped += 1;
                }
                emit_deep(&app, &queue);
                std::thread::sleep(Duration::from_millis(GAP_MS));
                continue;
            }

            if queue.already_failed(job.file_id) {
                // 本轮已经失败过：跳过它，别在原地打转（重启后会再给一次机会）
                std::thread::sleep(Duration::from_millis(GAP_MS));
                continue;
            }

            if let Ok(mut st) = queue.stats_lock() {
                st.running = true;
                st.current = job.name.clone();
                st.current_id = Some(job.file_id);
                st.note = note.clone();
            }
            emit_deep(&app, &queue);

            let mut result = deep_tag::process_one(
                &job,
                &base,
                &model,
                Duration::from_secs(deep_tag::LLM_TIMEOUT_SECS),
                &queue.gate,
            );
            // 失败重试 1 次（Ollama 偶发 500 / 半截 JSON）；闸被占不用重试，等下一轮
            if let Err(e) = &result {
                if e.code != error::MODEL_BUSY {
                    std::thread::sleep(Duration::from_millis(500));
                    result = deep_tag::process_one(
                        &job,
                        &base,
                        &model,
                        Duration::from_secs(deep_tag::LLM_TIMEOUT_SECS),
                        &queue.gate,
                    );
                }
            }

            match result {
                Ok(d) => {
                    note = None;
                    let wrote = {
                        let mut conn = match lock_db(&state) {
                            Ok(c) => c,
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(800));
                                continue;
                            }
                        };
                        index::apply_deep_tag(&mut conn, job.file_id, &d.tags, d.summary.as_deref())
                    };
                    match wrote {
                        Ok(skipped_tags) => {
                            if skipped_tags {
                                eprintln!(
                                    "deep tag skipped: user-locked（{} 的标签是人改过的，只写了 summary）",
                                    job.name
                                );
                            }
                            if let Ok(mut st) = queue.stats_lock() {
                                st.done += 1;
                                st.note = None;
                                if vision_job {
                                    st.vision_done += 1;
                                } else if job.kind == "image" {
                                    // 图片但被 Skip（格式不支持/超大）：模型根本没跑
                                    st.vision_skipped += 1;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("deep-tag: 落库失败 {}：{}", job.name, e.message);
                            queue.mark_failed(job.file_id);
                            if let Ok(mut st) = queue.stats_lock() {
                                st.failed += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    if e.code == error::MODEL_BUSY {
                        // 闸被别人占着（目录建议/视觉解读）：不是这份文件的问题，
                        // 不记失败、不进 failed_ids，歇一下等下一轮再取
                        eprintln!("deep-tag: {} 让路（模型闸被占），稍后重试", job.name);
                        std::thread::sleep(Duration::from_millis(GAP_MS));
                        continue;
                    }
                    let down = e.code == error::MODEL_DOWN || e.code == error::MODEL_TIMEOUT;
                    queue.mark_failed(job.file_id);
                    if let Ok(mut st) = queue.stats_lock() {
                        // 模型没起不算「这份文件失败」——换台机器起来还能读，别污染统计
                        if !down {
                            st.failed += 1;
                        }
                        st.note = Some(if down {
                            "队列已停：模型未响应".to_string()
                        } else {
                            format!("上一份没读出来：{}", e.message)
                        });
                    }
                    eprintln!("deep-tag: {} 失败 [{}] {}", job.name, e.code, e.message);
                    if down {
                        note = Some("队列已停：模型未响应".to_string());
                        emit_deep(&app, &queue);
                        // 20s 后再探一次（分片睡，保证能响应暂停/退出）
                        for _ in 0..10 {
                            if queue.snapshot_stopped() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(2000));
                        }
                        continue;
                    }
                }
            }

            // 完成后立刻清 currentId：待定区的「正在读…」按钮态靠它收敛，
            // 留着会让按钮永远转圈（T4.5）
            if let Ok(mut st) = queue.stats_lock() {
                st.current.clear();
                st.current_id = None;
                st.running = false;
            }
            emit_deep(&app, &queue);
            std::thread::sleep(Duration::from_millis(deep_tag::GAP_MS));
        }
    });
}

const GAP_MS: u64 = deep_tag::GAP_MS;

/// 待定区「让 AI 看看这是什么」（P3 T4.5）：把一份文件插到深读队列最前。
///
/// **不同步等待**——单份 60s，同步返回会让人以为卡死。立即回 `{queued:true}`，
/// 真正的结论走 `deep-tag-progress` 事件（带 currentId），前端按 id 收敛按钮态。
#[tauri::command]
async fn explain_file(app: AppHandle, file_id: i64) -> Result<serde_json::Value, CmdError> {
    let q = app.state::<Arc<deep_tag::Queue>>().inner().clone();
    // 深读关着时不静默失败：明确报 NOT_CONFIGURED，前端给「去开启」入口
    if !q.enabled() {
        return Err(CmdError::new(
            error::NOT_CONFIGURED,
            "深度解读未开启，请先到设置里打开",
        ));
    }
    {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::file_row(&conn, file_id)?.ok_or_else(|| {
            CmdError::new(error::DB, format!("找不到这份文件的记录（id={file_id}）"))
        })?;
    }
    q.push_front(file_id);
    Ok(serde_json::json!({ "queued": true }))
}

#[tauri::command]
async fn deep_tag_status(
    app: AppHandle,
    queue: tauri::State<'_, Arc<deep_tag::Queue>>,
) -> Result<deep_tag::Stats, CmdError> {
    let q = queue.inner().clone();
    let state = app.state::<AppState>();
    if let Ok(conn) = lock_db(&state) {
        if let Ok(n) = index::deep_remaining(&conn) {
            if let Ok(mut st) = q.stats_lock() {
                st.remaining = n;
            }
        }
    }
    Ok(q.snapshot())
}

/// 开关与暂停。`enabled` 写库，重启后还是用户的意愿
#[tauri::command]
async fn set_deep_tag(
    app: AppHandle,
    queue: tauri::State<'_, Arc<deep_tag::Queue>>,
    enabled: Option<bool>,
    paused: Option<bool>,
    vision_enabled: Option<bool>,
) -> Result<deep_tag::Stats, CmdError> {
    let q = queue.inner().clone();
    if let Some(e) = enabled {
        q.set_enabled(e);
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::set_setting(&conn, "deep_tag_enabled", if e { "1" } else { "0" })?;
    }
    if let Some(v) = vision_enabled {
        q.set_vision_enabled(v);
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::set_setting(&conn, "deep_vision_enabled", if v { "1" } else { "0" })?;
    }
    if let Some(p) = paused {
        q.set_paused(p);
    }
    Ok(q.snapshot())
}

/// `--extract <目录> <out.json>`：逐份跑正文抽取并报字数/耗时
///
/// 存在的理由只有一个：**PDF 抽取的闸门要靠实测**（计划 T2.3 —— 中文正文成功率 <60%
/// 就回滚依赖，PDF 一律「未解读」）。不能用「我觉得 pdf-extract 挺行」来替代测量。
fn run_extract_cli(dir: &str, out: &str) -> i32 {
    let t_start = Instant::now();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let walker = walkdir::WalkDir::new(dir).max_depth(3).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = scan::ext_of(&name);
        let kind = tag::kind_of_ext(&ext);
        if !matches!(kind, "pdf" | "word" | "excel" | "ppt" | "diagram") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let t0 = Instant::now();
        let (text, _, err) = extract::extract(entry.path(), &ext, kind, size);
        rows.push(serde_json::json!({
            "name": name,
            "kind": kind,
            "size": size,
            "chars": text.chars().count(),
            "ms": t0.elapsed().as_millis() as u64,
            "parseError": err,
        }));
    }
    let pdf = rows.iter().filter(|r| r["kind"] == "pdf").count();
    let ok = rows
        .iter()
        .filter(|r| r["kind"] == "pdf" && r["chars"].as_u64().unwrap_or(0) >= 100)
        .count();
    let payload = serde_json::json!({
        "dir": dir,
        "total": rows.len(),
        "pdf": pdf,
        "pdfWithText": ok,
        "elapsedMs": t_start.elapsed().as_millis() as u64,
        "files": rows,
    });
    eprintln!("抽取 {} 份，其中 PDF {pdf} 份、有文字层 {ok} 份", payload["total"]);
    match std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("写出失败：{e}");
            1
        }
    }
}

/// `--deeptag <目录> <out.json> [份数]`：不开 GUI 跑一遍深度标签
///
/// 与界面里队列走的是**同一套** `deep_tag::process_one` + `llm::generate`。
/// 存在的理由：验收 3 要求「12 份文本文件 3 分钟内完成、中位单份 ≤8s」，
/// 这个数只能真跑出来，不能靠估。
fn run_deeptag_cli(dir: &str, out: &str, limit: usize) -> i32 {
    let base = std::env::var("FILEBUTLER_BASE").unwrap_or_else(|_| llm::DEFAULT_BASE.to_string());
    let model = std::env::var("FILEBUTLER_MODEL").unwrap_or_else(|_| llm::DEFAULT_MODEL.to_string());
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut picked: Vec<deep_tag::Job> = Vec::new();
    for entry in walkdir::WalkDir::new(dir).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() || picked.len() >= limit {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = scan::ext_of(&name);
        let kind = tag::kind_of_ext(&ext);
        if !matches!(kind, "word" | "excel" | "ppt" | "pdf" | "diagram") {
            continue;
        }
        picked.push(deep_tag::Job {
            file_id: picked.len() as i64 + 1,
            name,
            abs_path: entry.path().to_string_lossy().to_string(),
            ext,
            kind: kind.to_string(),
            size: entry.metadata().map(|m| m.len()).unwrap_or(0),
        });
    }
    eprintln!("挑中 {} 份，模型 {model}", picked.len());

    let t_all = Instant::now();
    let mut times: Vec<u128> = Vec::new();
    for job in &picked {
        let t0 = Instant::now();
        let r = deep_tag::process_one(
            job,
            &base,
            &model,
            Duration::from_secs(deep_tag::LLM_TIMEOUT_SECS),
            // CLI 没有别的模型通路，闸永远空闲；仍走同一签名保持链路一致
            &model_gate::ModelGate::default(),
        );
        let ms = t0.elapsed().as_millis();
        times.push(ms);
        match r {
            Ok(d) => {
                eprintln!(
                    "  {}ms  {}  →  {:?}  {}",
                    ms,
                    job.name.chars().take(28).collect::<String>(),
                    d.tags,
                    d.summary.unwrap_or_else(|| "（无）".into())
                );
                rows.push(serde_json::json!({
                    "name": job.name, "kind": job.kind, "ms": ms,
                    "tags": d.tags, "summary": d.tags.len(),
                }));
            }
            Err(e) => {
                eprintln!("  {}ms  {}  →  [{}] {}", ms, job.name, e.code, e.message);
                rows.push(serde_json::json!({
                    "name": job.name, "kind": job.kind, "ms": ms, "error": e.code,
                }));
            }
        }
    }
    times.sort_unstable();
    let median = times.get(times.len() / 2).copied().unwrap_or(0);
    let payload = serde_json::json!({
        "dir": dir,
        "model": model,
        "count": picked.len(),
        "wallMs": t_all.elapsed().as_millis() as u64,
        "medianMs": median,
        "files": rows,
    });
    eprintln!(
        "共 {} 份 / 总耗 {}ms / 中位 {}ms",
        picked.len(),
        t_all.elapsed().as_millis(),
        median
    );
    match std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("写出失败：{e}");
            1
        }
    }
}

// ---------------------------------------------------------------- 增量与监听（M3）

/// 启动后自动补一轮扫描（T3.2）
///
/// 两条硬规矩：
/// * **距离上次扫描 < 10 分钟就跳过** —— 用户点着玩重启几次，不该每次都全盘走一遍；
/// * **后台跑、不挡首屏** —— 首屏要的是计数和批次，不是「扫描完成」。
fn spawn_auto_scan(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let due = {
            let Ok(conn) = lock_db(&state) else { return };
            let last = setting(&conn, "last_scan_at")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let configured = !index::list_source_roots(&conn).unwrap_or_default().is_empty();
            let has_archive = setting(&conn, "archive_root").is_some();
            (configured && has_archive, index::now_ms() - last)
        };
        let (ok, since) = due;
        if !ok {
            return;
        }
        if since < 10 * 60 * 1000 {
            eprintln!("auto-scan: skip（{}s 前刚扫过）", since / 1000);
            return;
        }
        // 给首屏一点时间：窗口先出来、计数先渲染，再开始走盘
        std::thread::sleep(Duration::from_millis(1200));
        match scan_all_inner(&app) {
            Ok(r) => {
                eprintln!(
                    "auto-scan: 扫了 {} 份，新增 {}，更新 {}，丢失 {}",
                    r.scanned, r.inserted, r.updated, r.missing
                );
                // 前端据此出「发现 N 份新文件」（T3.7），中性提示，不闪不红
                let _ = app.emit("files-found", serde_json::json!({ "added": r.inserted }));
            }
            Err(e) => eprintln!("auto-scan: 失败 [{}] {}", e.code, e.message),
        }
    });
}

/// 监听批次落库（T3.4/T3.5/T3.6）
///
/// 返回真正新增的份数。**写库失败只记日志、丢这一批** ——
/// 下一轮启动扫描会补上，绝不为了写一行数据去等 UI，也绝不 panic。
fn apply_watch_batch(app: &AppHandle, paths: Vec<std::path::PathBuf>) -> usize {
    if paths.is_empty() {
        return 0;
    }
    let state = app.state::<AppState>();
    if !state.watch_enabled.load(Ordering::Relaxed) {
        return 0;
    }
    let mut guard = match lock_db_retry(&state.db, Duration::from_millis(200)) {
        Some(g) => g,
        None => {
            eprintln!("watch: db busy, drop batch（{} 条）", paths.len());
            return 0;
        }
    };

    let mut added = 0usize;
    for p in paths.iter().take(500) {
        let abs = p.to_string_lossy().to_string();
        if p.exists() {
            let Some(meta) = scan::meta_of(p) else { continue };
            let Ok(Some(root_id)) = index::root_for_path(&guard, &abs) else {
                continue;
            };
            let one = [meta];
            // 必须用 watched 版：普通 upsert 会做「本轮未见即丢失」的全局判定，
            // 而这一批只有刚变的几份 —— 用它等于把库里其余文件全打成 missing
            match index::upsert_files_watched(&mut guard, root_id, &one) {
                Ok(st) => {
                    if st.inserted > 0 {
                        // 新文件要立刻有基础标签，否则进不了池
                        if let Ok(queued) = index::list_queued(&guard, Some(root_id)) {
                            let items: Vec<(i64, Vec<(String, String, f64)>)> = queued
                                .iter()
                                .filter(|(_, name, _, _)| name == &one[0].name)
                                .map(|(id, name, kind, mtime)| {
                                    (*id, tag::basic_tags(name, kind, *mtime))
                                })
                                .collect();
                            let _ = index::apply_basic_labels(&mut guard, &items);
                        }
                        added += 1;
                    }
                }
                Err(e) => eprintln!("watch: 入库失败 {abs}：{}", e.message),
            }
        } else {
            // 删除/改名：只对 3 个状态生效（T3.5 守卫）
            match index::mark_missing_if_pending(&guard, &abs) {
                Ok(true) => eprintln!("watch: {abs} 不见了 → missing"),
                Ok(false) => {}
                Err(e) => eprintln!("watch: 标丢失失败 {abs}：{}", e.message),
            }
        }
    }
    added
}

/// 监听 guard 的插槽。设置页关掉监听时把它置空（drop 即停线程），
/// 重新打开时再 start 一个 —— 否则开关只能写数据库，得重启才生效。
type WatchSlot = std::sync::Mutex<Option<watch::WatchGuard>>;

/// 从库里读出「该听哪些目录、归档区在哪」
fn watch_config_from_db(app: &AppHandle) -> watch::WatchConfig {
    let state = app.state::<AppState>();
    let Ok(conn) = lock_db(&state) else {
        return watch::WatchConfig::default();
    };
    let roots: Vec<std::path::PathBuf> = index::list_source_roots(&conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| std::path::PathBuf::from(r.path))
        .collect();
    let archive_root = setting(&conn, "archive_root").map(std::path::PathBuf::from);
    watch::WatchConfig {
        roots,
        archive_root,
    }
}

/// 起监听（已在跑就先停掉旧的，避免同一目录挂两个 watcher）
fn start_watch(app: &AppHandle) -> Result<(), CmdError> {
    let cfg = watch_config_from_db(app);
    if cfg.roots.is_empty() {
        return Ok(());
    }
    let handle = app.clone();
    let g = watch::start(
        cfg,
        Duration::from_millis(watch::DEBOUNCE_MS),
        move |paths| {
            let n = apply_watch_batch(&handle, paths);
            if n > 0 {
                // 前端据此出「+N 新文件」（T3.7）
                let _ = handle.emit("files-found", serde_json::json!({ "added": n }));
            }
        },
    )?;
    let slot = app.state::<WatchSlot>();
    // 先换出旧的再赋值：旧 guard 在这里 drop，线程退出
    *slot.lock().map_err(|_| CmdError::new(error::DB, "监听插槽锁已损坏"))? = Some(g);
    Ok(())
}

/// 停监听：置空插槽即可（guard 的 Drop 会 join 线程）
fn stop_watch(app: &AppHandle) {
    let slot = app.state::<WatchSlot>();
    let taken = slot.lock().map(|mut s| s.take());
    // 显式落到语句里：让 MutexGuard 在 slot 之前析构
    drop(taken);
}

#[tauri::command]
async fn set_watch_enabled(app: AppHandle, enabled: bool) -> Result<(), CmdError> {
    {
        let state = app.state::<AppState>();
        let conn = lock_db(&state)?;
        index::set_setting(&conn, "watch_enabled", if enabled { "1" } else { "0" })?;
        state.watch_enabled.store(enabled, Ordering::Relaxed);
    }
    // 开关要当场生效，不能等下次启动
    if enabled {
        start_watch(&app)?;
    } else {
        stop_watch(&app);
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--deeptag" {
        let limit = args
            .get(4)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(12);
        std::process::exit(run_deeptag_cli(&args[2], &args[3], limit));
    }
    if args.len() >= 4 && args[1] == "--extract" {
        std::process::exit(run_extract_cli(&args[2], &args[3]));
    }
    if args.len() >= 4 && args[1] == "--eval" {
        let no_tags = args.iter().skip(2).any(|a| a == "--no-tags");
        std::process::exit(run_eval_cli(&args[2], &args[3], no_tags));
    }
    if args.len() >= 3 && args[1] == "--perf-search" {
        std::process::exit(run_perf_search_cli(&args[2]));
    }
    if args.len() >= 4 && args[1] == "--scan" {
        std::process::exit(run_scan_cli(&args[2], &args[3]));
    }
    if args.len() >= 4 && args[1] == "--suggest" {
        let force = args.get(4).map(|a| a == "force").unwrap_or(false);
        std::process::exit(run_suggest_cli(&args[2], &args[3], force));
    }

    tauri::Builder::default()
        .setup(|app| {
            // `FILEBUTLER_DATA_DIR` 只为端到端验证用：把库指到一个空目录，
            // e2e 就不会碰到用户真实的索引库（与 FILEBUTLER_FAKE_FREE_GB 同一路数）。
            let dir = match std::env::var("FILEBUTLER_DATA_DIR") {
                Ok(d) if !d.trim().is_empty() => PathBuf::from(d),
                _ => app
                    .path()
                    .app_data_dir()
                    .unwrap_or_else(|_| PathBuf::from(".")),
            };
            std::fs::create_dir_all(&dir)?;
            let mut conn = index::open(&dir.join("index.db")).map_err(|e| e.to_string())?;

            // 老数据回填（P3 T2.2）：archived_path 列是 P3 才有的，已迁移的行从
            // migration_items 反推一次。只跑一次（settings 标记位），失败不挡启动。
            match index::backfill_archived_paths(&mut conn) {
                Ok(0) => {}
                Ok(n) => eprintln!("backfill archived_path: {n} rows"),
                Err(e) => eprintln!("backfill archived_path failed: {e}"),
            }

            // 图片缩略图走 asset 协议：把已配置的源区（和归档区）加进读取白名单。
            // 不写死 `**` —— 白名单越小，将来前端万一被注入也有边界。
            // 新增源区时由 `add_source_root` 再补一条。
            {
                let scope = app.asset_protocol_scope();
                let mut roots: Vec<String> = index::list_source_roots(&conn)
                    .map(|v| v.into_iter().map(|r| r.path).collect())
                    .unwrap_or_default();
                if let Some(a) = index::get_setting(&conn, "archive_root").ok().flatten() {
                    roots.push(a);
                }
                for r in roots {
                    let _ = scope.allow_directory(&r, true);
                }
            }

            app.manage(AppState::new(conn, dir));

            // 深度标签队列：默认开（用户可在设置页关掉）
            let enabled = index::get_setting(&app.state::<AppState>().db.lock().unwrap(), "deep_tag_enabled")
                .ok()
                .flatten()
                .map(|v| v != "0")
                .unwrap_or(true);
            let queue = Arc::new(deep_tag::Queue::new(enabled));
            // 图片解读开关（T4.6）：默认值由 T4.3 视觉闸门裁决，用户可改（写库）
            let vision = index::get_setting(
                &app.state::<AppState>().db.lock().unwrap(),
                "deep_vision_enabled",
            )
            .ok()
            .flatten()
            .map(|v| v != "0")
            .unwrap_or(deep_tag::VISION_DEFAULT_ON);
            queue.set_vision_enabled(vision);
            app.manage(queue.clone());
            spawn_deep_tag(app.handle().clone(), queue);

            // 启动增量 + 运行时监听（M3）。两者都不挡首屏：各自一个后台线程。
            spawn_auto_scan(app.handle().clone());

            // 监听的 guard 得能被设置页的开关摘下来，所以用 Option 装在插槽里
            app.manage(std::sync::Mutex::new(None::<watch::WatchGuard>));
            let watch_on = {
                let state = app.state::<AppState>();
                let on = lock_db(&state)
                    .ok()
                    .and_then(|conn| setting(&conn, "watch_enabled"))
                    .map(|v| v != "0")
                    .unwrap_or(true);
                state.watch_enabled.store(on, Ordering::Relaxed);
                on
            };
            if watch_on {
                if let Err(e) = start_watch(app.handle()) {
                    eprintln!("watch: 启动失败 [{}] {}", e.code, e.message);
                }
            }
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
            list_parked,
            unpark,
            assign_parked,
            drop_file,
            undo_decide,
            close_batch,
            migration_preview,
            apply_migration,
            undo_migration,
            latest_migration,
            requeue_confirmed,
            save_note,
            preview_file,
            rename_file,
            rescore_files,
            deep_tag_status,
            set_deep_tag,
            recategorize_file,
            explain_file,
            set_watch_enabled,
            add_tag,
            remove_tag,
            list_tags,
            delete_tag,
            rename_tag,
            browse_tree,
            browse_dir,
            pool_rows,
            search_files
        ])
        .run(tauri::generate_context!())
        .expect("应用启动失败");
}

// ---------------------------------------------------------------- 检索性能基准（P3 T3.7）

/// 临时库造 N 份（默认 5000），量三类查询的 p95。不开 GUI、不碰真实库。
fn run_perf_search_cli(count_arg: &str) -> i32 {
    let n: usize = count_arg.parse().unwrap_or(5000);
    let dir = std::env::temp_dir().join(format!("fb-perf-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let started = std::time::Instant::now();
    let conn = match index::open(&dir.join("perf.db")) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("open: {e}");
            return 1;
        }
    };
    {
        index::add_source_root(&conn, "C:/perf").ok();
        let root: i64 =
            conn.query_row("SELECT id FROM source_roots LIMIT 1", [], |r| r.get(0)).unwrap();
        let mut stmt = conn
            .prepare("INSERT INTO files(source_root_id, abs_path, rel_path, name, ext, kind, size, mtime, status, discovered_at, last_seen_at) VALUES(?1,?2,?2,?3,?4,?5,100,1700000000000,'labeled',0,0)")
            .unwrap();
        let kinds = ["word", "excel", "ppt", "pdf", "image"];
        let exts = ["docx", "xlsx", "pptx", "pdf", "png"];
        for i in 0..n {
            let k = i % 5;
            let ext = exts[k];
            let kind = kinds[k];
            let _ = stmt.execute(rusqlite::params![
                root,
                format!("C:/perf/材料{i:05}.{ext}"),
                format!("材料{i:05}"),
                ext,
                kind
            ]);
        }
        let mut t = conn
            .prepare("INSERT OR IGNORE INTO tags(name, kind) VALUES(?1,'topic')")
            .unwrap();
        for g in 0..(n / 20).max(1) {
            let _ = t.execute(rusqlite::params![format!("话题{g}")]);
        }
        let _ = conn.execute(
            "INSERT INTO file_tags(file_id, tag_id)
             SELECT f.id, t.id FROM files f JOIN tags t ON (t.id % 20) = (f.id % 20) WHERE f.id % 5 = 0",
            [],
        );
    }
    let insert_ms = started.elapsed().as_millis();

    let run = |req: index::SearchReq| -> Vec<u128> {
        let mut ms = Vec::new();
        for _ in 0..30 {
            let t0 = std::time::Instant::now();
            let _ = index::search_files(&conn, &req).unwrap();
            ms.push(t0.elapsed().as_millis());
        }
        ms.sort();
        ms
    };
    let p95 = |v: &[u128]| -> u128 { v[v.len() * 95 / 100] };

    let q_kw = run(index::SearchReq {
        q: "材料000".into(),
        scope: index::Scope::All,
        limit: 200,
        ..Default::default()
    });
    let tag_ids: i64 = conn
        .query_row("SELECT id FROM tags LIMIT 1", [], |r| r.get(0))
        .unwrap_or(0);
    let q_tag = run(index::SearchReq {
        tags: vec![tag_ids],
        scope: index::Scope::All,
        limit: 200,
        ..Default::default()
    });
    let q_mix = run(index::SearchReq {
        q: "材料00".into(),
        tags: vec![tag_ids],
        tag_and: false,
        kinds: vec!["docx".into()],
        from: Some(0),
        to: Some(1_800_000_000_000),
        scope: index::Scope::All,
        limit: 200,
        ..Default::default()
    });

    let out = serde_json::json!({
        "count": n,
        "insertMs": insert_ms,
        "keywordP95Ms": p95(&q_kw),
        "tagP95Ms": p95(&q_tag),
        "mixedP95Ms": p95(&q_mix),
    });
    println!("{out}");
    let _ = std::fs::remove_dir_all(&dir);
    0
}
