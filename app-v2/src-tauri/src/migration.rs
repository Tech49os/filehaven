//! 迁移编排（详设 §7.7 / §5.4）
//!
//! 职责：把库里的 `confirmed` 变成磁盘上的「已归档 + 已隔离」，以及反向往回。
//! 这里只做编排与落库顺序，文件系统细节在 `migrate.rs`，SQL 在 `index.rs`。
//!
//! 事务粒度：**按文件一个事务**（`migration_items` 一行 + `status=migrated`）。
//! 一批 43 份失败 2 份就 41 成功 2 失败，绝不整批回滚。

use crate::error::{self, CmdError, BAD_INPUT, MIGRATE_FAILED};
use crate::index::{self, Status};
use crate::migrate::{self, ItemResult, UndoOutcome};
use crate::model::{
    ChangedOut, FileRow, MigrationFailure, MigrationPreview, MigrationResult, NameConflictOut,
    PreviewGroup, RecatOutcome, UndoReport,
};
use crate::plan::{self, PlanInput, PlanItem};
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};

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
            // 归档用目标名：确认前顺手改过名的话，搬过去的就是新名字
            name: index::effective_name(&r.file),
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
                // 归档后的真实路径（P3 T2.1）：源文件进了隔离区，abs_path 已是死路。
                // 浏览页 / 打开 / 定位 / 预览全靠这两列。
                tx.execute(
                    "UPDATE files SET archived_path=?1, archive_dir_id=?2 WHERE id=?3",
                    rusqlite::params![item.dst_path, p.dir_id, item.file_id],
                )?;
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
    let is_recat = mig.kind == "recategorize";
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

        // 普通迁移的防呆：这份文件后来被重新归类挪走了（archived_path 已不是本批的落点）。
        // 继续撤会把归档区的另一份副本变成孤儿 —— 跳过，让用户先撤重新归类那批。
        if !is_recat {
            let cur_archived: Option<String> = conn
                .query_row(
                    "SELECT archived_path FROM files WHERE id=?1",
                    [file_id],
                    |r| r.get(0),
                )
                .unwrap_or(None);
            if cur_archived.is_some() && cur_archived.as_deref() != Some(item.dst_path.as_str()) {
                skipped += 1;
                continue;
            }
        }

        let mut stage = |s: &str| progress(i, total, &name, s);
        let outcome = if is_recat {
            undo_recategorize_one(
                conn,
                archive_root,
                migration_id,
                &item,
                &mut stage,
            )
        } else {
            let rel = match index::get_source_root_of_file(conn, file_id)? {
                Some(root) => rel_in_source(&item.src_path, &root),
                None => file_name_of(&item.src_path),
            };
            migrate::undo_one(
                archive_root,
                migration_id,
                &item.src_path,
                &item.dst_path,
                item.sha256.as_deref(),
                &rel,
                &mut stage,
            )
        };
        match outcome {
            UndoOutcome::Restored => {
                let tx = conn.transaction()?;
                if is_recat {
                    // 撤销重新归类：文件回到旧目录，仍处于归档态，不回池
                    tx.execute(
                        "UPDATE files SET archived_path=?1, archive_dir_id=COALESCE(?2, archive_dir_id) WHERE id=?3",
                        rusqlite::params![item.src_path, item.old_dir_id, file_id],
                    )?;
                } else {
                    index::set_status(&tx, file_id, Status::Confirmed)?;
                    // 文件回去了，归档路径就不再成立；不清掉浏览页会指向隔离区里的旧路径
                    tx.execute(
                        "UPDATE files SET archived_path=NULL, archive_dir_id=NULL WHERE id=?1",
                        rusqlite::params![file_id],
                    )?;
                }
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

// ================================================================ P4 M1：重新归类（归档区内二次迁移）

/// 重新归类一份已归档文件：从旧目录挪到新目录。
///
/// 三段式照旧（复制 → SHA-256 校验 → 旧归档副本进隔离区），永不覆盖：
/// 目标有同名就自动错开 `_2`。台账记成 `kind='recategorize'` 的独立批次，
/// 用户可以按批次撤销（撤销会把文件挪回旧目录，新位置副本清除）。
pub fn recategorize(
    conn: &mut Connection,
    archive_root: &Path,
    file_id: i64,
    new_dir_id: i64,
) -> Result<RecatOutcome, CmdError> {
    // ① 状态与存在性闸门
    let row: Option<(String, Option<String>, Option<i64>)> = conn
        .query_row(
            "SELECT status, archived_path, archive_dir_id FROM files WHERE id=?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((status, archived_path, cur_dir)) = row else {
        return Err(CmdError::new(BAD_INPUT, "找不到这份文件"));
    };
    if status != "migrated" {
        return Err(CmdError::new(
            MIGRATE_FAILED,
            "只有已归档的文件才能重新归类",
        ));
    }
    let Some(src_s) = archived_path else {
        return Err(CmdError::new(
            MIGRATE_FAILED,
            "这份文件没有归档路径记录，无法重新归类",
        ));
    };
    if cur_dir == Some(new_dir_id) {
        return Err(CmdError::new(BAD_INPUT, "文件已经在当前目录里，不需要重新归类"));
    }
    let dir = index::get_dir(conn, new_dir_id)?.ok_or_else(|| {
        CmdError::new(BAD_INPUT, "目标目录不存在，请先在目录方案里建好")
    })?;

    let src = PathBuf::from(&src_s);
    if !src.is_file() {
        return Err(CmdError::new(
            MIGRATE_FAILED,
            "归档文件已不在磁盘上（可能被手工挪动），先处理文件再重新归类",
        ));
    }

    // ② 目标路径：同名自动错开，绝不覆盖
    let dst_dir = plan::archive_dir_path(archive_root, &dir.rel_path);
    std::fs::create_dir_all(&dst_dir)
        .map_err(|e| CmdError::new(error::IO, format!("创建目标目录失败：{e}")))?;
    let name = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut dst = dst_dir.join(&name);
    let mut renamed = false;
    if dst.exists() {
        dst = dst_dir.join(plan::unique_name(&dst_dir, &name));
        renamed = true;
    }

    // ③ 复制 + SHA-256 校验（校验不过就删副本，原文件纹丝不动）
    let src_hash = migrate::sha256_of(&src)
        .map_err(|e| CmdError::new(error::IO, format!("读取归档文件失败：{e}")))?;
    if let Err(e) = migrate::copy_file(&src, &dst) {
        let _ = std::fs::remove_file(&dst);
        return Err(CmdError::new(error::IO, format!("复制失败：{e}")));
    }
    let dst_hash = migrate::sha256_of(&dst)
        .map_err(|e| CmdError::new(error::IO, format!("校验读取失败：{e}")))?;
    if dst_hash != src_hash {
        let _ = std::fs::remove_file(&dst);
        return Err(CmdError::new(
            MIGRATE_FAILED,
            "哈希校验不一致，已放弃本次重新归类，原文件未动",
        ));
    }

    // ④ 台账 + 旧副本进隔离区 + 更新 files
    let bytes = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0) as i64;
    let migration_id = index::insert_migration_with_kind(conn, 1, "recategorize")?;
    let q = plan::quarantine_path(
        archive_root,
        migration_id,
        &format!("{}/{}", dir.rel_path, name),
    );
    if let Err(e) = migrate::move_file(&src, &q) {
        // 旧副本挪不进隔离区：回滚磁盘（删掉刚复制的），原文件原地不动
        let _ = std::fs::remove_file(&dst);
        index::finish_migration(conn, migration_id, "failed", 0)?;
        return Err(CmdError::new(
            MIGRATE_FAILED,
            format!("旧归档副本没能移入隔离区（{e}），本次未改动"),
        ));
    }

    {
        let tx = conn.transaction()?;
        index::insert_migration_item_ext(
            &tx,
            migration_id,
            file_id,
            &src_s,
            &dst.to_string_lossy().to_string(),
            Some(&src_hash),
            "ok",
            None,
            cur_dir,
        )?;
        tx.execute(
            "UPDATE files SET archived_path=?1, archive_dir_id=?2 WHERE id=?3",
            rusqlite::params![dst.to_string_lossy().to_string(), new_dir_id, file_id],
        )?;
        index::finish_migration(&tx, migration_id, "done", bytes)?;
        tx.commit()?;
    }

    Ok(RecatOutcome {
        new_path: dst.to_string_lossy().to_string(),
        renamed,
    })
}

/// 撤销一条重新归类：隔离区旧原件 → 旧目录；新位置副本 → 隔离区（不直删）。
///
/// 两条硬闸：
/// ① 新位置副本的哈希必须与台账一致 —— 被改过就拒绝（撤销不该丢用户的修改）；
/// ② 旧位置已有同名文件就拒绝 —— 永不覆盖。
fn undo_recategorize_one(
    _conn: &Connection,
    archive_root: &Path,
    migration_id: i64,
    item: &index::MigrationItemRow,
    stage: &mut dyn FnMut(&str),
) -> UndoOutcome {
    let old_path = PathBuf::from(&item.src_path);
    let new_path = PathBuf::from(&item.dst_path);
    let name = file_name_of(&item.src_path);

    stage(migrate::STAGE_COPY);
    if !new_path.is_file() {
        return UndoOutcome::Failed("新位置的文件不见了，无法撤销".into());
    }
    if let Some(want) = &item.sha256 {
        match migrate::sha256_of(&new_path) {
            Ok(h) if h == *want => {}
            Ok(_) => {
                return UndoOutcome::Failed(
                    "新位置的文件被修改过，撤销会丢掉这些改动；请先确认内容".into(),
                )
            }
            Err(e) => return UndoOutcome::Failed(format!("读取新位置文件失败：{e}")),
        }
    }

    // 从隔离区找回旧原件（按名字 + 哈希，用户手工挪过也能救回来）
    let q_dir = plan::archive_dir_path(
        archive_root,
        &format!(".filebutler/quarantine/{migration_id}"),
    );
    let Some(from) = migrate::find_in_quarantine(&q_dir, &name, item.sha256.as_deref()) else {
        return UndoOutcome::Failed("隔离区找不到旧归档副本，无法撤销".into());
    };

    if old_path.exists() {
        return UndoOutcome::Failed("旧目录里已经出现了同名文件，未还原（避免覆盖）".into());
    }

    stage(migrate::STAGE_VERIFY);
    // 先还原件，再收新位置副本（顺序保证最坏情况是「两处都有」，绝不出现「两处都无」）
    if let Err(e) = migrate::move_file(&from, &old_path) {
        return UndoOutcome::Failed(format!("旧副本还原失败：{e}"));
    }
    let undo_q = plan::quarantine_path(archive_root, migration_id, &format!("_undo/{name}"));
    if let Err(e) = migrate::move_file(&new_path, &undo_q) {
        return UndoOutcome::Failed(format!(
            "旧位置已还原，但新位置副本没能移入隔离区（{e}）；请手工处理"
        ));
    }
    UndoOutcome::Restored
}
