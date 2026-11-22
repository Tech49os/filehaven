//! BUG-006 同义目录合并 · 编排层。
//!
//! 复用底座：
//! - 规则预筛 / 降级报告 / 阶段事件骨架 ← `dir_health.rs::scan_report`
//! - 三段式搬移与 kind 机制 ← `migration.rs::recategorize`，台账 kind=`dir_merge`
//! - 隔离区布局 ← `plan::quarantine_path`（一处定义，不许另造）
//!
//! 纪律：只识别与建议，绝不自动合并；每份文件独立事务；失败显式记录；
//! 永不直删、永不覆盖；源目录只标记 empty，保留记录与磁盘空目录。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::dir_norm::signature;
use crate::error::{self, CmdError, BAD_INPUT, MIGRATE_FAILED};
use crate::index::{self, Status};
use crate::migrate;
use crate::model::{
    DirMergeApplyAck, DirMergeCandidate, DirMergeReport, MigrationFailure, UndoReport,
};
use crate::plan;

/// 单组文件数上限（spec §6：N 次三段式搬移，耗时/失败概率随 N 增长）
pub const MAX_GROUP_FILES: usize = 50;

/// 一个目录的事实（命令层锁内投影，锁外只做纯计算）
#[derive(Clone, Debug)]
pub struct MergeFact {
    pub id: i64,
    pub rel_path: String,
    /// 原始路径段数：越多代表层级语义越清晰
    pub depth: i64,
    pub file_count: usize,
    pub created_at: i64,
}

/// 模型复核回调：只做「确认/否决」，不给模型改路径的权力。
pub type MergeReviewer<'a> =
    dyn Fn(&[DirMergeCandidate]) -> Result<Vec<DirMergeCandidate>, String> + 'a;

/// 规则预筛：签名相同才成组；推荐目标 = 层级更清晰 → 文件更多 → 创建更早。
pub fn analyze_groups(facts: &[MergeFact]) -> Vec<DirMergeCandidate> {
    let mut by_sig: HashMap<String, Vec<&MergeFact>> = HashMap::new();
    for f in facts {
        let sig = signature(&f.rel_path);
        if !sig.is_empty() {
            by_sig.entry(sig).or_default().push(f);
        }
    }

    let mut out = Vec::new();
    for (sig, mut members) in by_sig {
        if members.len() < 2 {
            continue;
        }
        let total_files: usize = members.iter().map(|m| m.file_count).sum();
        if total_files > MAX_GROUP_FILES {
            // 超限不静默丢：由 scan_report 的 note 汇总报告
            continue;
        }
        members.sort_by_key(|m| m.id);
        let best = members
            .iter()
            .max_by(|a, b| {
                a.depth
                    .cmp(&b.depth)
                    .then_with(|| a.file_count.cmp(&b.file_count))
                    // 创建更早者优先：a 更小 → a 胜出
                    .then_with(|| b.created_at.cmp(&a.created_at))
            })
            .expect("members 至少两个");

        let dir_ids: Vec<i64> = members.iter().map(|m| m.id).collect();
        let dir_paths: Vec<String> = members.iter().map(|m| m.rel_path.clone()).collect();
        let source_dir_ids: Vec<i64> = dir_ids.iter().copied().filter(|id| *id != best.id).collect();
        let source_dir_paths: Vec<String> = members
            .iter()
            .filter(|m| m.id != best.id)
            .map(|m| m.rel_path.clone())
            .collect();
        let reason = format!(
            "归一化签名「{sig}」；差异点：{}",
            members
                .iter()
                .map(|m| m.rel_path.as_str())
                .collect::<Vec<_>>()
                .join(" ↔ ")
        );

        out.push(DirMergeCandidate {
            id: format!("dir_merge:{sig}"),
            signature: sig,
            dir_ids,
            dir_paths,
            source_dir_ids,
            source_dir_paths,
            target_dir_id: best.id,
            target_path: best.rel_path.clone(),
            suggested_target_id: best.id,
            reason,
            file_count: total_files,
            confidence: 0.75,
            source: "rule".to_string(),
        });
    }

    out.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.signature.cmp(&b.signature))
    });
    out
}

/// 扫描编排（纯逻辑，不碰 DB / 事件总线 / 模型客户端）。
///
/// 阶段固定 `scan → rule → model → done`：模型不可用时 model 阶段照样发，note 标明跳过原因。
/// 降级纪律：绝不返回 Err、绝不阻塞，永远给确定性规则候选 + `model_reviewed=false`。
pub fn scan_report(
    facts: &[MergeFact],
    reviewer: &MergeReviewer,
    can_review: bool,
    skip_reason: &str,
    on_stage: &mut dyn FnMut(&str, u64, &str),
) -> DirMergeReport {
    let t0 = Instant::now();
    let mut report = DirMergeReport::default();

    on_stage("scan", 0, &format!("扫描到 {} 个归档目录", facts.len()));
    let mut candidates = analyze_groups(facts);
    let mut note_extra = String::new();
    let raw_total: usize = facts.iter().map(|f| f.file_count).sum();
    if raw_total > 0 && candidates.iter().map(|c| c.file_count).sum::<usize>() < raw_total {
        note_extra = format!("；已跳过超过 {MAX_GROUP_FILES} 份文件的超大组");
    }
    on_stage(
        "rule",
        t0.elapsed().as_millis() as u64,
        &format!("规则初筛 {} 组同义目录", candidates.len()),
    );

    if !can_review {
        let base = if skip_reason.is_empty() {
            "模型不可用，仅规则初筛（未经模型复核）".to_string()
        } else {
            format!("{skip_reason}，仅规则初筛（未经模型复核）")
        };
        report.note = format!("{base}{note_extra}");
        on_stage("model", t0.elapsed().as_millis() as u64, "跳过模型复核");
    } else {
        on_stage("model", t0.elapsed().as_millis() as u64, "模型复核中");
        match reviewer(&candidates) {
            Ok(reviewed) => {
                let mut reviewed = reviewed;
                for c in &mut reviewed {
                    c.source = "model".to_string();
                }
                candidates = reviewed;
                report.model_reviewed = true;
                report.note = format!("规则初筛 + 模型复核{note_extra}");
            }
            Err(reason) => {
                report.note =
                    format!("模型复核失败（{reason}），保留规则初筛（未经模型复核）{note_extra}");
            }
        }
    }

    report.candidates = candidates;
    report.elapsed_ms = t0.elapsed().as_millis() as u64;
    on_stage("done", report.elapsed_ms, &report.note);
    report
}

/// 锁内投影目录事实。已标记 empty 的源目录不再参与扫描（避免合并后反复自我建议）。
pub fn facts_from_db(conn: &rusqlite::Connection) -> Result<Vec<MergeFact>, CmdError> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.rel_path, d.depth,
                (SELECT COUNT(*) FROM files f
                  WHERE f.status='migrated' AND f.archive_dir_id=d.id) AS file_count,
                d.created_at
         FROM archive_dirs d
         WHERE COALESCE(d.is_empty,0)=0
         ORDER BY d.rel_path",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MergeFact {
            id: r.get(0)?,
            rel_path: r.get(1)?,
            depth: r.get(2)?,
            file_count: r.get::<_, i64>(3)? as usize,
            created_at: r.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::CmdError::from)
}

/// 模型复核 prompt：只让模型判「哪几组确实是同义」，不许改路径。
fn review_prompt(candidates: &[DirMergeCandidate]) -> String {
    let mut lines = String::new();
    for c in candidates {
        lines.push_str(&format!("- id={} 签名={} 目录={:?}\n", c.id, c.signature, c.dir_paths));
    }
    format!(
        "你是归档目录质检员。下面是规则初筛的同义目录候选，请判断哪些确实含义相同。\
仅返回 JSON：{{\"keep\":[\"id1\"]}}，keep 里放你认可的候选 id。\
不确定或层级顺序不同的不要放进去。不要新增条目，不要修改任何目录名。\n\n候选：\n{lines}"
    )
}

/// 容错提取 keep 列表（兼容 ```json 围栏与前后废话）
fn parse_keep(raw: &str) -> Option<Vec<String>> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&raw[start..=end]).ok()?;
    let arr = json.get("keep")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

/// 真实模型复核闭包工厂（沿用 `dir_health::make_reviewer` 骨架）
pub fn make_reviewer<'a>(
    base: &'a str,
    model: &'a str,
    timeout: std::time::Duration,
) -> impl Fn(&[DirMergeCandidate]) -> Result<Vec<DirMergeCandidate>, String> + 'a {
    move |candidates| {
        let prompt = review_prompt(candidates);
        let raw = crate::llm::generate(base, model, &prompt, timeout).map_err(|e| e.message)?;
        let keep = parse_keep(&raw).ok_or_else(|| "返回不是合法 JSON".to_string())?;
        Ok(candidates
            .iter()
            .filter(|c| keep.contains(&c.id))
            .cloned()
            .collect())
    }
}

/// 绝对路径 → 归档根相对路径（`/` 分隔），用于隔离区保留源目录结构。
fn rel_from_root(abs: &str, root: &Path) -> String {
    let a = abs.replace('\\', "/");
    let r = root.to_string_lossy().replace('\\', "/");
    let r = r.trim_end_matches('/');
    if a.len() > r.len() && a.is_char_boundary(r.len()) && a[..r.len()].eq_ignore_ascii_case(r) {
        let rest = a[r.len()..].trim_start_matches('/');
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    Path::new(abs)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 采纳一组同义目录合并：逐份走三段式（复制 → SHA-256 校验 → 旧副本入隔离区）。
///
/// - 单批一个 `migrations` 记录，kind=`dir_merge`；每份一个独立事务，失败显式记账；
/// - 目标同名按 `_2` 错开，绝不覆盖；
/// - 完成后源目录标记 `is_empty=1`，**不删目录记录、不删磁盘空目录**。
pub fn apply_merge(
    conn: &mut rusqlite::Connection,
    archive_root: &Path,
    source_dir_ids: &[i64],
    target_dir_id: i64,
    progress: &mut dyn FnMut(&str, u64, &str),
) -> Result<DirMergeApplyAck, CmdError> {
    let t0 = Instant::now();
    let target = index::get_dir(conn, target_dir_id)?
        .ok_or_else(|| CmdError::new(BAD_INPUT, "目标目录不存在"))?;
    if source_dir_ids.is_empty() {
        return Err(CmdError::new(BAD_INPUT, "没有要合并的源目录"));
    }
    for id in source_dir_ids {
        if *id == target_dir_id {
            return Err(CmdError::new(BAD_INPUT, "源目录不能同时是目标目录"));
        }
        index::get_dir(conn, *id)?
            .ok_or_else(|| CmdError::new(BAD_INPUT, format!("源目录不存在：{id}")))?;
    }

    let marks = vec!["?"; source_dir_ids.len()].join(",");
    let sql = format!(
        "SELECT id, archived_path, archive_dir_id, name FROM files
         WHERE status='migrated' AND archive_dir_id IN ({marks}) AND archived_path IS NOT NULL
         ORDER BY archive_dir_id, name, id"
    );
    let params: Vec<&i64> = source_dir_ids.iter().collect();
    let mut stmt = conn.prepare(&sql)?;
    let files: Vec<(i64, String, i64, String)> = stmt
        .query_map(rusqlite::params_from_iter(params), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt); // 事务要独占可变借用，先释放查询结果句柄
    if files.len() > MAX_GROUP_FILES {
        return Err(CmdError::new(
            BAD_INPUT,
            format!(
                "单组 {} 份文件超过上限 {MAX_GROUP_FILES}，请拆分后再合并",
                files.len()
            ),
        ));
    }

    let migration_id = index::insert_migration_with_kind(conn, files.len() as i64, "dir_merge")?;
    let target_disk = plan::archive_dir_path(archive_root, &target.rel_path);
    std::fs::create_dir_all(&target_disk)
        .map_err(|e| CmdError::new(error::IO, format!("创建目标目录失败：{e}")))?;

    let mut copied = 0usize;
    let mut renamed = 0usize;
    let mut bytes = 0i64;
    let mut failed: Vec<MigrationFailure> = Vec::new();

    for (i, (file_id, src_s, old_dir_id, name)) in files.iter().enumerate() {
        progress("copy", (i * 3) as u64, name);
        let src = PathBuf::from(src_s);
        if !src.is_file() {
            failed.push(MigrationFailure {
                file_id: *file_id,
                name: name.clone(),
                reason: "归档文件已不在磁盘上，未搬移".to_string(),
            });
            continue;
        }
        let src_hash = match migrate::sha256_of(&src) {
            Ok(h) => h,
            Err(e) => {
                failed.push(MigrationFailure {
                    file_id: *file_id,
                    name: name.clone(),
                    reason: format!("读取源文件失败：{e}"),
                });
                continue;
            }
        };

        // 目标同名错开，绝不覆盖
        let dst_name = plan::unique_name(&target_disk, name);
        if dst_name != *name {
            renamed += 1;
        }
        let dst = target_disk.join(&dst_name);

        progress("verify", (i * 3 + 1) as u64, name);
        if let Err(e) = migrate::copy_file(&src, &dst) {
            let _ = std::fs::remove_file(&dst);
            failed.push(MigrationFailure {
                file_id: *file_id,
                name: name.clone(),
                reason: format!("复制失败：{e}"),
            });
            continue;
        }
        match migrate::sha256_of(&dst) {
            Ok(h) if h == src_hash => {}
            Ok(_) => {
                let _ = std::fs::remove_file(&dst);
                failed.push(MigrationFailure {
                    file_id: *file_id,
                    name: name.clone(),
                    reason: "哈希校验不一致，已放弃该文件，原文件未动".to_string(),
                });
                continue;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&dst);
                failed.push(MigrationFailure {
                    file_id: *file_id,
                    name: name.clone(),
                    reason: format!("校验读取失败：{e}"),
                });
                continue;
            }
        }

        progress("quarantine", (i * 3 + 2) as u64, name);
        let q = plan::quarantine_path(archive_root, migration_id, &rel_from_root(src_s, archive_root));
        if let Err(e) = migrate::move_file(&src, &q) {
            // 副本已校验通过但源文件进不了隔离区：清掉副本，原文件原地不动
            let _ = std::fs::remove_file(&dst);
            failed.push(MigrationFailure {
                file_id: *file_id,
                name: name.clone(),
                reason: format!("旧副本没能移入隔离区（{e}），本次未改动"),
            });
            continue;
        }

        {
            let tx = conn.transaction()?;
            index::insert_migration_item_ext(
                &tx,
                migration_id,
                *file_id,
                src_s,
                &dst.to_string_lossy().to_string(),
                Some(&src_hash),
                "ok",
                None,
                Some(*old_dir_id),
            )?;
            tx.execute(
                "UPDATE files SET archived_path=?1, archive_dir_id=?2 WHERE id=?3",
                rusqlite::params![dst.to_string_lossy().to_string(), target_dir_id, file_id],
            )?;
            tx.commit()?;
        }
        copied += 1;
        bytes += std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0) as i64;
    }

    // 只把真正搬空的源目录标为 empty（部分失败时仍留有文件的目录保持原状）
    for id in source_dir_ids {
        if !index::dir_has_archived_files(conn, *id)? {
            index::set_dir_empty(conn, *id, true)?;
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
    progress(
        "done",
        t0.elapsed().as_millis() as u64,
        &format!("合并完成：{copied} 份，失败 {} 份", failed.len()),
    );

    Ok(DirMergeApplyAck {
        migration_id,
        affected_files: copied,
        renamed,
        failed,
        source_dir_ids: source_dir_ids.to_vec(),
        target_dir_id,
    })
}

/// 撤销一批 `dir_merge`：隔离区原件按哈希校验后还原，新位置副本进隔离区 `_undo`（不直删）。
///
/// 两条硬闸（与 `undo_recategorize_one` 同纪律）：
/// ① 新位置副本或隔离区原件的哈希与台账不一致 → 拒绝（不丢用户改动）；
/// ② 旧位置已有同名文件 → 拒绝（永不覆盖）。
pub fn undo_merge(
    conn: &mut rusqlite::Connection,
    archive_root: &Path,
    migration_id: i64,
) -> Result<UndoReport, CmdError> {
    let mig = index::get_migration(conn, migration_id)?
        .ok_or_else(|| CmdError::new(MIGRATE_FAILED, "找不到合并批次"))?;
    if mig.kind != "dir_merge" {
        return Err(CmdError::new(MIGRATE_FAILED, "这不是同义目录合并批次"));
    }
    if mig.status == "undone" {
        return Err(CmdError::new(MIGRATE_FAILED, "这批合并已经撤销过了"));
    }

    let items: Vec<_> = index::migration_items(conn, migration_id)?
        .into_iter()
        .filter(|(_, result)| result == "ok")
        .collect();
    if items.is_empty() {
        return Err(CmdError::new(MIGRATE_FAILED, "这批合并没有可撤销的条目"));
    }

    let mut restored = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<MigrationFailure> = Vec::new();
    let mut touched_dirs: Vec<i64> = Vec::new();

    // 逆序：后搬的先退，与合并顺序正好相反
    for (item, _) in items.into_iter().rev() {
        let Some(file_id) = item.file_id else {
            skipped += 1;
            continue;
        };
        let name = Path::new(&item.src_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if index::get_file_status(conn, file_id)? != Some(Status::Migrated) {
            skipped += 1;
            continue;
        }

        let new_path = PathBuf::from(&item.dst_path);
        if !new_path.is_file() {
            failed.push(MigrationFailure {
                file_id,
                name,
                reason: "新位置的文件不见了，无法撤销".to_string(),
            });
            continue;
        }
        if let Some(want) = &item.sha256 {
            match migrate::sha256_of(&new_path) {
                Ok(h) if h == *want => {}
                Ok(_) => {
                    failed.push(MigrationFailure {
                        file_id,
                        name,
                        reason: "新位置的文件被修改过，撤销会丢失改动；请先确认".to_string(),
                    });
                    continue;
                }
                Err(e) => {
                    failed.push(MigrationFailure {
                        file_id,
                        name,
                        reason: format!("读取新位置文件失败：{e}"),
                    });
                    continue;
                }
            }
        }

        let q_dir = plan::archive_dir_path(
            archive_root,
            &format!(".filebutler/quarantine/{migration_id}"),
        );
        let expected = plan::quarantine_path(
            archive_root,
            migration_id,
            &rel_from_root(&item.src_path, archive_root),
        );
        let from = if expected.is_file() {
            expected
        } else {
            match migrate::find_in_quarantine(&q_dir, &name, item.sha256.as_deref()) {
                Some(p) => p,
                None => {
                    failed.push(MigrationFailure {
                        file_id,
                        name,
                        reason: "隔离区找不到旧归档副本，无法撤销".to_string(),
                    });
                    continue;
                }
            }
        };
        if let Some(want) = &item.sha256 {
            match migrate::sha256_of(&from) {
                Ok(h) if h == *want => {}
                Ok(_) => {
                    failed.push(MigrationFailure {
                        file_id,
                        name,
                        reason: "隔离区原件与台账哈希不一致，拒绝撤销".to_string(),
                    });
                    continue;
                }
                Err(e) => {
                    failed.push(MigrationFailure {
                        file_id,
                        name,
                        reason: format!("读取隔离区原件失败：{e}"),
                    });
                    continue;
                }
            }
        }

        let old_path = PathBuf::from(&item.src_path);
        if old_path.exists() {
            failed.push(MigrationFailure {
                file_id,
                name,
                reason: "旧目录已出现同名文件，未还原（避免覆盖）".to_string(),
            });
            continue;
        }

        // 先还原原件，再把新位置副本收进隔离区：最坏是「两处都有」，绝不「两处都无」
        if let Err(e) = migrate::move_file(&from, &old_path) {
            failed.push(MigrationFailure {
                file_id,
                name,
                reason: format!("旧副本还原失败：{e}"),
            });
            continue;
        }
        let undo_dir = q_dir.join("_undo");
        if let Err(e) = std::fs::create_dir_all(&undo_dir) {
            failed.push(MigrationFailure {
                file_id,
                name,
                reason: format!("创建撤销隔离目录失败：{e}"),
            });
            continue;
        }
        let undo_q = undo_dir.join(plan::unique_name(&undo_dir, &name));
        if let Err(e) = migrate::move_file(&new_path, &undo_q) {
            failed.push(MigrationFailure {
                file_id,
                name,
                reason: format!(
                    "旧位置已还原，但新位置副本没能移入隔离区（{e}）；请手工处理"
                ),
            });
            continue;
        }

        {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE files SET archived_path=?1, archive_dir_id=COALESCE(?2, archive_dir_id) WHERE id=?3",
                rusqlite::params![item.src_path, item.old_dir_id, file_id],
            )?;
            tx.commit()?;
        }
        if let Some(d) = item.old_dir_id {
            if !touched_dirs.contains(&d) {
                touched_dirs.push(d);
            }
        }
        restored += 1;
    }

    for d in touched_dirs {
        index::set_dir_empty(conn, d, false)?;
    }
    if failed.is_empty() {
        index::finish_migration(conn, migration_id, "undone", mig.bytes)?;
    }
    Ok(UndoReport {
        restored,
        skipped,
        failed,
    })
}
