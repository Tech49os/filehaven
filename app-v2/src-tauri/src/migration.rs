//! 迁移编排（详设 §7.7 / §5.4）
//!
//! 职责：把库里的 `confirmed` 变成磁盘上的「已归档 + 已隔离」，以及反向往回。
//! 这里只做编排与落库顺序，文件系统细节在 `migrate.rs`，SQL 在 `index.rs`。
//!
//! 事务粒度：**按文件一个事务**（`migration_items` 一行 + `status=migrated`）。
//! 一批 43 份失败 2 份就 41 成功 2 失败，绝不整批回滚。

use crate::error::{CmdError, MIGRATE_FAILED};
use crate::index::{self, Status};
use crate::migrate::{self, ItemResult, UndoOutcome};
use crate::model::{
    ChangedOut, FileRow, MigrationFailure, MigrationPreview, MigrationResult, NameConflictOut,
    PreviewGroup, UndoReport,
};
use crate::plan::{self, PlanInput, PlanItem};
use rusqlite::Connection;
use std::path::Path;

/// 空间判定：算不出剩余就不拦（宁可不保证，也别凭空拦住用户）
pub fn space_ok(free_gb: Option<f64>, needed_bytes: u64) -> bool {
    match free_gb {
        None => true,
        Some(g) => ((g.max(0.0) * 1024.0 * 1024.0 * 1024.0) as u64) >= needed_bytes,
    }
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// `abs` 去掉源区前缀 → 源区相对路径（`/` 分隔）。剥不掉就退化成文件名。
fn rel_in_source(abs: &str, root: &str) -> String {
    let a = abs.replace('\\', "/");
    let r = root.replace('\\', "/");
    let r = r.trim_end_matches('/');
    let hit = a.len() >= r.len()
        && a.is_char_boundary(r.len())
        && a[..r.len()].eq_ignore_ascii_case(r);
    if !hit {
        return file_name_of(abs);
    }
    let rest = a[r.len()..].trim_start_matches('/');
    if rest.is_empty() {
        file_name_of(abs)
    } else {
        rest.to_string()
    }
}

/// 库里「已确认」的完整快照：计划输入 + 用于展示的行
fn snapshot(conn: &Connection) -> Result<Vec<(PlanInput, FileRow)>, CmdError> {
    let rows = index::confirmed_for_migration(conn)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let rel = match r.file.source_root_id {
            Some(rid) => match index::get_source_root(conn, rid)? {
                Some(root) => rel_in_source(&r.file.abs_path, &root.path),
                None => file_name_of(&r.file.abs_path),
            },
            None => file_name_of(&r.file.abs_path),
        };
        let input = PlanInput {
            file_id: r.file.id,
            name: r.file.name.clone(),
            src_path: r.file.abs_path.clone(),
            dir_id: r.dir_id,
            dir_rel_path: r.dir_rel_path.clone(),
            rel_in_source: rel,
            size: r.file.size,
            mtime: r.file.mtime,
        };
        out.push((input, r.file));
    }
    Ok(out)
}

/// 迁移前预览：分组 / 冲突 / 已改拦截 / 空间校验（R7）
pub fn preview(
    conn: &Connection,
    archive_root: &Path,
    free_gb: Option<f64>,
) -> Result<MigrationPreview, CmdError> {
    let snap = snapshot(conn)?;
    let inputs: Vec<PlanInput> = snap.iter().map(|(p, _)| p.clone()).collect();
    let built = plan::build_plans(&inputs, archive_root);

    let mut groups: Vec<PreviewGroup> = Vec::new();
    for p in &built.plans {
        let row = snap
            .iter()
            .find(|(i, _)| i.file_id == p.file_id)
            .map(|(_, f)| f.clone());
        let Some(row) = row else { continue };
        let bytes = row.size.max(0) as u64;
        match groups.iter_mut().find(|g| g.dir_id == p.dir_id) {
            Some(g) => {
                g.bytes += bytes;
                g.files.push(row);
            }
            None => groups.push(PreviewGroup {
                dir_id: p.dir_id,
                rel_path: p.dir_rel_path.clone(),
                is_new: p.is_new_dir,
                files: vec![row],
                bytes,
            }),
        }
    }

    let total_files = built.plans.len();
    let total_bytes: u64 = built.plans.iter().map(|p| p.size.max(0) as u64).sum();

    Ok(MigrationPreview {
        groups,
        name_conflicts: built
            .conflicts
            .into_iter()
            .map(|c| NameConflictOut {
                file_id: c.file_id,
                target_name: c.target_name,
            })
            .collect(),
        changed_since_label: built
            .changed
            .into_iter()
            .map(|c| ChangedOut {
                file_id: c.file_id,
                name: c.name,
                why: c.why,
            })
            .collect(),
        total_files,
        total_bytes,
        free_gb,
        enough_space: space_ok(free_gb, total_bytes),
    })
}

/// 执行迁移。
///
/// `exclude_file_ids` 是前端取消勾选的那些 —— 它们回池（等待重新归类），本次不搬。
/// `progress(done, total, name, stage)` 每文件三次（copy / verify / quarantine）。
pub fn apply(
    conn: &mut Connection,
    archive_root: &Path,
    exclude_file_ids: &[i64],
    progress: &mut dyn FnMut(usize, usize, &str, &str),
) -> Result<MigrationResult, CmdError> {
    let snap = snapshot(conn)?;
    let inputs: Vec<PlanInput> = snap.iter().map(|(p, _)| p.clone()).collect();
    let built = plan::build_plans(&inputs, archive_root);

    for id in exclude_file_ids {
        if index::get_file_status(conn, *id)? == Some(Status::Confirmed) {
            index::set_status(conn, *id, Status::Queued)?;
        }
    }

    let plans: Vec<PlanItem> = built
        .plans
        .into_iter()
        .filter(|p| !exclude_file_ids.contains(&p.file_id))
        .collect();
    let total = plans.len();
    let migration_id = index::insert_migration(conn, total as i64)?;

    if total == 0 {
        index::finish_migration(conn, migration_id, "done", 0)?;
        return Ok(MigrationResult {
            migration_id,
            copied: 0,
            verified: 0,
            quarantined: 0,
            failed: Vec::new(),
        });
    }

    let mut copied = 0usize;
    let mut verified = 0usize;
    let mut quarantined = 0usize;
    let mut bytes = 0i64;
    let mut failed: Vec<MigrationFailure> = Vec::new();

    for (i, p) in plans.iter().enumerate() {
        let mut stage = |s: &str| progress(i, total, &p.name, s);
        let item = migrate::execute_one(archive_root, migration_id, p, &mut stage);

        // 每文件一个事务：台账一行 + 成功才推状态
        {
            let tx = conn.transaction()?;
            index::insert_migration_item(
                &tx,
                migration_id,
                item.file_id,
                &item.src_path,
                &item.dst_path,
                item.sha256.as_deref(),
                if item.result == ItemResult::Ok {
                    "ok"
                } else {
                    "failed"
                },
                item.error.as_deref(),
            )?;
            if item.result == ItemResult::Ok {
                index::set_status(&tx, item.file_id, Status::Migrated)?;
            }
            tx.commit()?;
        }

        if item.result == ItemResult::Ok {
            copied += 1;
            verified += 1;
            if item.quarantined {
                quarantined += 1;
            }
            bytes += item.bytes as i64;
        } else {
            failed.push(MigrationFailure {
                file_id: item.file_id,
                name: item.name.clone(),
                reason: item.error.unwrap_or_else(|| "未知原因".into()),
            });
        }
    }

    let status = if failed.is_empty() {
        "done"
    } else if copied == 0 {
        "failed"
    } else {
        "partial"
    };
    index::finish_migration(conn, migration_id, status, bytes)?;

    Ok(MigrationResult {
        migration_id,
        copied,
        verified,
        quarantined,
        failed,
    })
}

/// 撤销一次迁移：逆序还原，能救几份救几份（R8）
pub fn undo(
    conn: &mut Connection,
    archive_root: &Path,
    migration_id: i64,
    progress: &mut dyn FnMut(usize, usize, &str, &str),
) -> Result<UndoReport, CmdError> {
    let mig = index::get_migration(conn, migration_id)?
        .ok_or_else(|| CmdError::new(MIGRATE_FAILED, format!("找不到迁移记录：{migration_id}")))?;
    if mig.status == "undone" {
        return Err(CmdError::new(
            MIGRATE_FAILED,
            "这次迁移已经撤销过了，不能重复撤销",
        ));
    }

    let items: Vec<(index::MigrationItemRow, String)> = index::migration_items(conn, migration_id)?
        .into_iter()
        .filter(|(_, result)| result == "ok")
        .collect();
    if items.is_empty() {
        return Err(CmdError::new(MIGRATE_FAILED, "这次迁移没有可撤销的条目"));
    }

    let total = items.len();
    let mut restored = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<MigrationFailure> = Vec::new();

    // 逆序：后搬的先退，和搬的顺序正好相反
    let ordered: Vec<(usize, (index::MigrationItemRow, String))> = items.into_iter().enumerate().collect();
    for (i, (item, _)) in ordered.into_iter().rev() {
        let Some(file_id) = item.file_id else {
            skipped += 1;
            continue;
        };
        let name = file_name_of(&item.src_path);

        // 已经不处于 migrated（比如被重新扫描或手工改过）——不碰它，也不算失败
        let status = index::get_file_status(conn, file_id)?;
        if status != Some(Status::Migrated) {
            skipped += 1;
            continue;
        }

        let rel = match index::get_source_root_of_file(conn, file_id)? {
            Some(root) => rel_in_source(&item.src_path, &root),
            None => file_name_of(&item.src_path),
        };

        let mut stage = |s: &str| progress(i, total, &name, s);
        match migrate::undo_one(
            archive_root,
            migration_id,
            &item.src_path,
            &item.dst_path,
            item.sha256.as_deref(),
            &rel,
            &mut stage,
        ) {
            UndoOutcome::Restored => {
                let tx = conn.transaction()?;
                index::set_status(&tx, file_id, Status::Confirmed)?;
                tx.commit()?;
                restored += 1;
            }
            UndoOutcome::Failed(reason) => {
                failed.push(MigrationFailure {
                    file_id,
                    name,
                    reason,
                });
            }
        }
    }

    // 全部还原成功才把这次迁移标记为已撤销（还有失败就留着，允许再试一次）
    if failed.is_empty() {
        index::finish_migration(conn, migration_id, "undone", mig.bytes)?;
    }

    Ok(UndoReport {
        restored,
        skipped,
        failed,
    })
}
