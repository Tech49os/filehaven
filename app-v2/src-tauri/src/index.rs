//! 唯一执行 SQL 的模块 —— 评审红线 1：其他模块不许自己 `prepare/execute/query_row`。
//!
//! 每次数据访问都只做一件事，事务边界见详设 §5.4。

use crate::error::{CmdError, BAD_TRANSITION, DIR_INVALID};
use crate::model::{
    ArchiveDir, Assignment, CloseAck, DecideAck, DecideReq, DirNode, NewAssignment, ScannedFile,
    SourceRoot, UpsertStats,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// P1 一次建全的 10 张表
pub const TABLES: &[&str] = &[
    "settings",
    "source_roots",
    "archive_dirs",
    "files",
    "tags",
    "file_tags",
    "batches",
    "assignments",
    "migrations",
    "migration_items",
];

// ---------------------------------------------------------------- 状态机

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Queued,
    Labeled,
    PendingConfirm,
    Confirmed,
    Migrated,
    Parked,
    Skipped,
    Missing,
}

impl Status {
    pub const ALL: [Status; 8] = [
        Status::Queued,
        Status::Labeled,
        Status::PendingConfirm,
        Status::Confirmed,
        Status::Migrated,
        Status::Parked,
        Status::Skipped,
        Status::Missing,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Labeled => "labeled",
            Status::PendingConfirm => "pending_confirm",
            Status::Confirmed => "confirmed",
            Status::Migrated => "migrated",
            Status::Parked => "parked",
            Status::Skipped => "skipped",
            Status::Missing => "missing",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        Status::ALL.iter().copied().find(|x| x.as_str() == s)
    }
}

/// 详设 §5.3 的转移表（不在表内一律非法；同状态转换也非法，调用方需先判断）
pub static ALLOWED: &[(Status, &[Status])] = &[
    (Status::Queued, &[Status::Labeled, Status::Missing]),
    (Status::Labeled, &[Status::PendingConfirm, Status::Missing]),
    (
        Status::PendingConfirm,
        &[
            Status::Confirmed,
            Status::Queued,
            Status::Parked,
            Status::Missing,
        ],
    ),
    (
        Status::Parked,
        &[
            Status::PendingConfirm,
            Status::Confirmed,
            Status::Skipped,
            Status::Missing,
        ],
    ),
    (
        Status::Confirmed,
        &[Status::Migrated, Status::Queued, Status::Missing],
    ),
    (Status::Migrated, &[Status::Confirmed]),
    (Status::Missing, &[Status::Queued]),
    (Status::Skipped, &[]),
];

pub fn allowed(from: Status, to: Status) -> bool {
    ALLOWED
        .iter()
        .find(|(f, _)| *f == from)
        .map(|(_, tos)| tos.contains(&to))
        .unwrap_or(false)
}

// ---------------------------------------------------------------- 连接

pub fn open(path: &Path) -> Result<Connection, CmdError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA synchronous=NORMAL;",
    )?;
    conn.execute_batch(include_str!("schema.sql"))?;
    Ok(conn)
}

/// 仅供测试与诊断用
pub fn table_names(conn: &Connection) -> Result<Vec<String>, CmdError> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master
         WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- settings

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, CmdError> {
    let v = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    Ok(v)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), CmdError> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- source_roots

fn row_to_root(r: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRoot> {
    Ok(SourceRoot {
        id: r.get(0)?,
        path: r.get(1)?,
        enabled: r.get::<_, i64>(2)? != 0,
    })
}

/// 重复路径返回已有记录，不报错
pub fn add_source_root(conn: &Connection, path: &str) -> Result<SourceRoot, CmdError> {
    conn.execute(
        "INSERT OR IGNORE INTO source_roots(path, enabled, added_at) VALUES(?1, 1, ?2)",
        params![path, now_ms()],
    )?;
    let root = conn.query_row(
        "SELECT id, path, enabled FROM source_roots WHERE path=?1",
        [path],
        row_to_root,
    )?;
    Ok(root)
}

pub fn list_source_roots(conn: &Connection) -> Result<Vec<SourceRoot>, CmdError> {
    let mut stmt = conn.prepare("SELECT id, path, enabled FROM source_roots ORDER BY id")?;
    let rows = stmt.query_map([], row_to_root)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_source_root(conn: &Connection, id: i64) -> Result<Option<SourceRoot>, CmdError> {
    let r = conn
        .query_row(
            "SELECT id, path, enabled FROM source_roots WHERE id=?1",
            [id],
            row_to_root,
        )
        .optional()?;
    Ok(r)
}

/// 移除源区：该源区文件置 missing（已迁移/已跳过的除外）。
/// 删除前先把 `source_root_id` 置空 —— 否则外键约束会拒绝删除，
/// 而这些文件本身还要留着（它们是 missing，不是被遗忘）。
pub fn remove_source_root(conn: &mut Connection, id: i64) -> Result<usize, CmdError> {
    let tx = conn.transaction()?;
    let n = tx.execute(
        "UPDATE files SET status='missing'
         WHERE source_root_id=?1 AND status NOT IN ('migrated','skipped','missing')",
        [id],
    )?;
    tx.execute(
        "UPDATE files SET source_root_id=NULL WHERE source_root_id=?1",
        [id],
    )?;
    tx.execute("DELETE FROM source_roots WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(n)
}

// ---------------------------------------------------------------- archive_dirs

fn row_to_dir(r: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveDir> {
    Ok(ArchiveDir {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        name: r.get(2)?,
        rel_path: r.get(3)?,
        depth: r.get(4)?,
        origin: r.get(5)?,
        created_at: r.get(6)?,
        last_used_at: r.get(7)?,
    })
}

const DIR_COLS: &str = "id, parent_id, name, rel_path, depth, origin, created_at, last_used_at";

/// 建目录节点（内部算 rel_path 与 depth）。同名同父已存在时返回 DIR_EXISTS。
pub fn insert_dir(
    conn: &Connection,
    parent_id: Option<i64>,
    name: &str,
    origin: &str,
) -> Result<ArchiveDir, CmdError> {
    let (rel_path, depth) = match parent_id {
        Some(pid) => {
            let parent = conn
                .query_row(
                    &format!("SELECT {DIR_COLS} FROM archive_dirs WHERE id=?1"),
                    [pid],
                    row_to_dir,
                )
                .optional()?
                .ok_or_else(|| CmdError::new("DIR_PARENT_MISSING", format!("父目录不存在：{pid}")))?;
            (format!("{}/{}", parent.rel_path, name), parent.depth + 1)
        }
        None => (name.to_string(), 1),
    };

    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM archive_dirs WHERE rel_path=?1",
            [&rel_path],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_some() {
        return Err(CmdError::new(
            crate::error::DIR_EXISTS,
            format!("目录已存在：{rel_path}"),
        ));
    }

    conn.execute(
        "INSERT INTO archive_dirs(parent_id, name, rel_path, depth, origin, created_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![parent_id, name, rel_path, depth, origin, now_ms()],
    )?;
    let dir = conn.query_row(
        &format!("SELECT {DIR_COLS} FROM archive_dirs WHERE rel_path=?1"),
        [&rel_path],
        row_to_dir,
    )?;
    Ok(dir)
}

/// 采纳一组目录方案：逐段建（父不存在一并建），已存在的直接复用。
///
/// 换一批后方案与已有目录重叠是常态，所以这里必须幂等，不许报 DIR_EXISTS。
pub fn accept_dirs(conn: &Connection, names: &[String]) -> Result<Vec<ArchiveDir>, CmdError> {
    let mut out = Vec::new();
    for raw in names {
        let clean = crate::plan::clean_path(raw);
        if !clean.valid {
            return Err(CmdError::new(
                DIR_INVALID,
                clean.reason.unwrap_or_else(|| "目录名无效".into()),
            ));
        }
        let mut parent: Option<i64> = None;
        let mut last: Option<ArchiveDir> = None;
        for seg in clean.cleaned.split('/') {
            let existing = match parent {
                Some(pid) => {
                    let p = get_dir(conn, pid)?.ok_or_else(|| {
                        CmdError::new("DIR_PARENT_MISSING", format!("父目录不存在：{pid}"))
                    })?;
                    dir_by_rel_path(conn, &format!("{}/{}", p.rel_path, seg))?
                }
                None => dir_by_rel_path(conn, seg)?,
            };
            let d = match existing {
                Some(d) => d,
                None => insert_dir(conn, parent, seg, "model_approved")?,
            };
            parent = Some(d.id);
            last = Some(d);
        }
        if let Some(d) = last {
            out.push(d);
        }
    }
    Ok(out)
}

pub fn dir_by_rel_path(conn: &Connection, rel_path: &str) -> Result<Option<ArchiveDir>, CmdError> {
    let d = conn
        .query_row(
            &format!("SELECT {DIR_COLS} FROM archive_dirs WHERE rel_path=?1"),
            [rel_path],
            row_to_dir,
        )
        .optional()?;
    Ok(d)
}

pub fn get_dir(conn: &Connection, id: i64) -> Result<Option<ArchiveDir>, CmdError> {
    let d = conn
        .query_row(
            &format!("SELECT {DIR_COLS} FROM archive_dirs WHERE id=?1"),
            [id],
            row_to_dir,
        )
        .optional()?;
    Ok(d)
}

pub fn list_dirs(conn: &Connection) -> Result<Vec<ArchiveDir>, CmdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {DIR_COLS} FROM archive_dirs ORDER BY depth, id"
    ))?;
    let rows = stmt.query_map([], row_to_dir)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 一次查全表 + 内存组树（不许 N+1 递归查）
pub fn list_dir_nodes(conn: &Connection) -> Result<Vec<DirNode>, CmdError> {
    let mut counts: HashMap<i64, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT a.target_dir_id, COUNT(*)
             FROM assignments a JOIN files f ON f.id = a.file_id
             WHERE f.status IN ('confirmed','migrated')
               AND a.target_dir_id IS NOT NULL
               AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = a.file_id)
             GROUP BY a.target_dir_id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (k, v) = row?;
            counts.insert(k, v);
        }
    }

    let mut flat: Vec<(i64, Option<i64>, DirNode)> = Vec::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT {DIR_COLS} FROM archive_dirs ORDER BY depth, id"
        ))?;
        let rows = stmt.query_map([], row_to_dir)?;
        for r in rows {
            let d = r?;
            flat.push((
                d.depth,
                d.parent_id,
                DirNode {
                    file_count: counts.get(&d.id).copied().unwrap_or(0),
                    id: d.id,
                    name: d.name,
                    rel_path: d.rel_path,
                    depth: d.depth,
                    origin: d.origin,
                    children: Vec::new(),
                },
            ));
        }
    }

    // 深的一层先挂到父上，浅的后挂；同层保持 id 顺序
    let mut by_id: HashMap<i64, DirNode> = HashMap::new();
    let mut order: Vec<(i64, i64, Option<i64>)> = Vec::new();
    for (depth, parent_id, node) in flat {
        order.push((depth, node.id, parent_id));
        by_id.insert(node.id, node);
    }
    order.sort_by(|a, b| b.0.cmp(&a.0));

    let mut roots: Vec<DirNode> = Vec::new();
    for (_, id, parent_id) in order {
        let Some(node) = by_id.remove(&id) else {
            continue;
        };
        match parent_id {
            Some(pid) if by_id.contains_key(&pid) => {
                by_id.get_mut(&pid).unwrap().children.push(node);
            }
            _ => roots.push(node),
        }
    }
    roots.sort_by_key(|n| n.id);
    Ok(roots)
}

// ---------------------------------------------------------------- files

fn rel_path_of(root_path: &str, abs_path: &str, fallback: &str) -> String {
    Path::new(abs_path)
        .strip_prefix(root_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| fallback.to_string())
}

/// 按 abs_path 冲突更新 size/mtime/last_seen_at；本次未见到且之前存在的置 missing。
/// 每 500 行一个事务，避免 455 份一个长事务。
pub fn upsert_files(
    conn: &mut Connection,
    root_id: i64,
    files: &[ScannedFile],
) -> Result<UpsertStats, CmdError> {
    let root_path: String = conn.query_row(
        "SELECT path FROM source_roots WHERE id=?1",
        [root_id],
        |r| r.get(0),
    )?;
    // 本次扫描的时间戳必须严格大于库里已有的 last_seen_at，
    // 否则同一毫秒内跑两次扫描会把「本次没见到的」判不出来（missing 永远是 0）。
    let prev_max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(last_seen_at), 0) FROM files WHERE source_root_id=?1",
        [root_id],
        |r| r.get(0),
    )?;
    let now = now_ms().max(prev_max + 1);
    let mut stats = UpsertStats::default();

    for chunk in files.chunks(500) {
        let tx = conn.transaction()?;
        for f in chunk {
            let rel = rel_path_of(&root_path, &f.path, &f.name);
            // 出生状态就分两种：非资料型（压缩包/程序/空文件）直接 skipped，不进池
            let pool = crate::tag::should_pool(&f.name, &f.kind, f.size);
            let born = if pool { "queued" } else { "skipped" };
            let existed: Option<i64> = tx
                .query_row("SELECT id FROM files WHERE abs_path=?1", [&f.path], |r| {
                    r.get(0)
                })
                .optional()?;
            tx.execute(
                "INSERT INTO files
                   (source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,
                    tag_level, status, defer_count, discovered_at, last_seen_at, missing)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'basic',?10,0,?9,?9,0)
                 ON CONFLICT(abs_path) DO UPDATE SET
                   source_root_id = excluded.source_root_id,
                   rel_path       = excluded.rel_path,
                   name           = excluded.name,
                   ext            = excluded.ext,
                   kind           = excluded.kind,
                   size           = excluded.size,
                   mtime          = excluded.mtime,
                   last_seen_at   = excluded.last_seen_at,
                   -- 文件回来了：missing → queued（资料型）/ skipped（非资料型），
                   -- 这是详设 §5.3 里唯一允许的回流
                   status         = CASE
                                      WHEN files.status='missing' AND ?11=1 THEN 'queued'
                                      WHEN files.status='missing' AND ?11=0 THEN 'skipped'
                                      ELSE files.status END",
                params![
                    root_id,
                    f.path,
                    rel,
                    f.name,
                    f.ext,
                    f.kind,
                    f.size as i64,
                    f.mtime,
                    now,
                    born,
                    if pool { 1 } else { 0 }
                ],
            )?;
            if existed.is_some() {
                stats.updated += 1;
            } else {
                stats.inserted += 1;
            }
        }
        tx.commit()?;
    }

    let missing = conn.execute(
        "UPDATE files SET status='missing'
         WHERE source_root_id=?1 AND last_seen_at < ?2
           AND status NOT IN ('migrated','skipped','missing')",
        params![root_id, now],
    )?;
    stats.missing = missing;
    Ok(stats)
}

pub fn get_file_status(conn: &Connection, file_id: i64) -> Result<Option<Status>, CmdError> {
    let s: Option<String> = conn
        .query_row("SELECT status FROM files WHERE id=?1", [file_id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(s.and_then(|s| Status::parse(&s)))
}

/// 状态写入的唯一入口：内部查表校验，非法直接 BAD_TRANSITION
pub fn set_status(conn: &Connection, file_id: i64, to: Status) -> Result<(), CmdError> {
    let from = get_file_status(conn, file_id)?
        .ok_or_else(|| CmdError::new("NOT_FOUND", format!("文件不存在：{file_id}")))?;
    if !allowed(from, to) {
        return Err(CmdError::new(
            BAD_TRANSITION,
            format!("不允许的状态转换：{} → {}", from.as_str(), to.as_str()),
        ));
    }
    conn.execute(
        "UPDATE files SET status=?1 WHERE id=?2",
        params![to.as_str(), file_id],
    )?;
    Ok(())
}

pub fn count_by_status(conn: &Connection) -> Result<Vec<(String, i64)>, CmdError> {
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM files GROUP BY status")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- 标签

/// 待打基础标签的文件（queued）：(id, name, kind, mtime)
pub fn list_queued(
    conn: &Connection,
    root_id: Option<i64>,
) -> Result<Vec<(i64, String, String, i64)>, CmdError> {
    let sql = match root_id {
        Some(_) => "SELECT id, name, kind, mtime FROM files WHERE status='queued' AND source_root_id=?1",
        None => "SELECT id, name, kind, mtime FROM files WHERE status='queued'",
    };
    let mut stmt = conn.prepare(sql)?;
    let mapper = |r: &rusqlite::Row<'_>| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    };
    let rows: Vec<_> = match root_id {
        Some(id) => stmt.query_map([id], mapper)?.collect::<Result<_, _>>()?,
        None => stmt.query_map([], mapper)?.collect::<Result<_, _>>()?,
    };
    Ok(rows)
}

/// 写基础标签并把 queued 推到 labeled（单事务）。
/// 返回实际写入标签的文件数。
pub fn apply_basic_labels(
    conn: &mut Connection,
    items: &[(i64, Vec<(String, String, f64)>)],
) -> Result<usize, CmdError> {
    let tx = conn.transaction()?;
    let mut done = 0usize;
    for (file_id, tags) in items {
        for (name, kind, conf) in tags {
            tx.execute(
                "INSERT OR IGNORE INTO tags(name, kind) VALUES(?1, ?2)",
                params![name, kind],
            )?;
            let tag_id: i64 = tx.query_row(
                "SELECT id FROM tags WHERE name=?1 AND kind=?2",
                params![name, kind],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO file_tags(file_id, tag_id, confidence) VALUES(?1,?2,?3)",
                params![file_id, tag_id, conf],
            )?;
        }
        tx.execute(
            "UPDATE files SET tag_level='basic', status='labeled' WHERE id=?1 AND status='queued'",
            [file_id],
        )?;
        done += 1;
    }
    tx.commit()?;
    Ok(done)
}

// ---------------------------------------------------------------- assignments

pub fn insert_assignment(conn: &Connection, a: &NewAssignment) -> Result<i64, CmdError> {
    conn.execute(
        "INSERT INTO assignments
           (file_id, batch_id, target_dir_id, origin, confidence, reason, decision, decided_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            a.file_id,
            a.batch_id,
            a.target_dir_id,
            a.origin,
            a.confidence,
            a.reason,
            a.decision,
            a.decided_at
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

const ASSIGN_COLS: &str =
    "id, file_id, batch_id, target_dir_id, origin, confidence, reason, decision, decided_at";

fn row_to_assignment(r: &rusqlite::Row<'_>) -> rusqlite::Result<Assignment> {
    Ok(Assignment {
        id: r.get(0)?,
        file_id: r.get(1)?,
        batch_id: r.get(2)?,
        target_dir_id: r.get(3)?,
        origin: r.get(4)?,
        confidence: r.get(5)?,
        reason: r.get(6)?,
        decision: r.get(7)?,
        decided_at: r.get(8)?,
    })
}

pub fn last_assignment(conn: &Connection, file_id: i64) -> Result<Option<Assignment>, CmdError> {
    let a = conn
        .query_row(
            &format!(
                "SELECT {ASSIGN_COLS} FROM assignments WHERE file_id=?1
                 ORDER BY decided_at DESC, id DESC LIMIT 1"
            ),
            [file_id],
            row_to_assignment,
        )
        .optional()?;
    Ok(a)
}

/// 本批已确认（accepted/changed）的份数
pub fn count_assigned(conn: &Connection, batch_id: i64) -> Result<i64, CmdError> {
    let n = conn.query_row(
        "SELECT COUNT(*) FROM assignments
         WHERE batch_id=?1 AND decision IN ('accepted','changed')",
        [batch_id],
        |r| r.get(0),
    )?;
    Ok(n)
}

// ---------------------------------------------------------------- 判定落库

/// 一次人工判定的落库（单事务）：推状态 + 写 assignment（+ 暂缓计数 / 备注 / 目录最近使用）
///
/// 状态推进一律走 `set_status`，非转移表内的组合直接 `BAD_TRANSITION`；
/// 事务里任何一步失败都不许留下半条记录。
pub fn apply_decide(conn: &mut Connection, req: &DecideReq) -> Result<DecideAck, CmdError> {
    let target = match req.decision.as_str() {
        "accepted" | "changed" => Status::Confirmed,
        "deferred" => Status::Queued,
        "parked" => Status::Parked,
        other => {
            return Err(CmdError::new(
                BAD_TRANSITION,
                format!("未知的判定：{other}"),
            ))
        }
    };
    // 采纳/改判必须落到具体目录；暂缓与先放着不该带目标
    if target == Status::Confirmed && req.dir_id.is_none() {
        return Err(CmdError::new(DIR_INVALID, "没有指定目标目录"));
    }
    if target != Status::Confirmed && req.dir_id.is_some() {
        return Err(CmdError::new(DIR_INVALID, "暂缓/先放着不该带目标目录"));
    }

    let tx = conn.transaction()?;
    // 同状态转换在转移表里非法，正好用来挡住重复确认
    set_status(&tx, req.file_id, target)?;

    insert_assignment(
        &tx,
        &NewAssignment {
            file_id: req.file_id,
            batch_id: Some(req.batch_id),
            target_dir_id: req.dir_id,
            origin: req.origin.clone(),
            confidence: req.confidence,
            reason: req.reason.clone(),
            decision: req.decision.clone(),
            decided_at: now_ms(),
        },
    )?;

    if req.decision == "deferred" {
        tx.execute(
            "UPDATE files SET defer_count = defer_count + 1 WHERE id=?1",
            [req.file_id],
        )?;
    }
    if let Some(note) = req.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        tx.execute(
            "UPDATE files SET note=?1 WHERE id=?2",
            params![note, req.file_id],
        )?;
    }
    if let Some(dir_id) = req.dir_id {
        tx.execute(
            "UPDATE archive_dirs SET last_used_at=?1 WHERE id=?2",
            params![now_ms(), dir_id],
        )?;
    }

    let confirmed = count_assigned(&tx, req.batch_id)?;
    let target_rel_path = match req.dir_id {
        Some(d) => get_dir(&tx, d)?.map(|x| x.rel_path),
        None => None,
    };
    let remaining = pool_count(&tx)?;
    tx.commit()?;

    Ok(DecideAck {
        file_id: req.file_id,
        status: target.as_str().to_string(),
        confirmed,
        target_rel_path,
        remaining_in_pool: remaining,
    })
}

/// Ctrl+Z：撤销本批最近一条 accepted/changed，文件回到池里
///
/// 暂缓与先放着不产生「已确认」，不在撤销范围内（返回 None）。
pub fn undo_last_decide(
    conn: &mut Connection,
    batch_id: i64,
) -> Result<Option<crate::model::FileRow>, CmdError> {
    let tx = conn.transaction()?;
    let row: Option<(i64, i64)> = tx
        .query_row(
            "SELECT id, file_id FROM assignments
             WHERE batch_id=?1 AND decision IN ('accepted','changed')
             ORDER BY id DESC LIMIT 1",
            [batch_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((aid, file_id)) = row else {
        return Ok(None);
    };
    tx.execute("DELETE FROM assignments WHERE id=?1", [aid])?;
    set_status(&tx, file_id, Status::Queued)?;
    let file = file_row(&tx, file_id)?;
    tx.commit()?;
    Ok(file)
}

/// 关批次并告知剩余池量（前端完成提示条要用）
pub fn close_batch_cmd(conn: &mut Connection, batch_id: i64) -> Result<CloseAck, CmdError> {
    let closed = close_batch(conn, batch_id)?;
    let remaining = pool_count(conn)?;
    Ok(CloseAck {
        closed,
        remaining_in_pool: remaining,
    })
}

// ---------------------------------------------------------------- 批次

const FILE_COLS: &str = "id, name, ext, kind, size, mtime, status, rel_path, abs_path, source_root_id, defer_count, note";

fn row_to_file(r: &rusqlite::Row<'_>) -> rusqlite::Result<crate::model::FileRow> {
    Ok(crate::model::FileRow {
        id: r.get(0)?,
        name: r.get(1)?,
        ext: r.get(2)?,
        kind: r.get(3)?,
        size: r.get(4)?,
        mtime: r.get(5)?,
        status: r.get(6)?,
        rel_path: r.get(7)?,
        abs_path: r.get(8)?,
        source_root_id: r.get(9)?,
        defer_count: r.get(10)?,
        note: r.get(11)?,
    })
}

/// 当前未关闭的批次：(id, seq, size)
pub fn open_batch(conn: &Connection) -> Result<Option<(i64, i64, i64)>, CmdError> {
    let b = conn
        .query_row(
            "SELECT id, seq, size FROM batches WHERE closed_at IS NULL ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(b)
}

pub fn insert_batch(conn: &Connection, seq: i64, size: i64) -> Result<i64, CmdError> {
    conn.execute(
        "INSERT INTO batches(seq, size, created_at) VALUES(?1, ?2, ?3)",
        params![seq, size, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn next_batch_seq(conn: &Connection) -> Result<i64, CmdError> {
    let n: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM batches", [], |r| {
        r.get(0)
    })?;
    Ok(n)
}

/// 关批次：未确认的文件回池（pending_confirm → queued，合法转换）
pub fn close_batch(conn: &mut Connection, batch_id: i64) -> Result<usize, CmdError> {
    let tx = conn.transaction()?;
    let n = tx.execute(
        "UPDATE files SET status='queued' WHERE status='pending_confirm'",
        [],
    )?;
    tx.execute(
        "UPDATE batches SET closed_at=?1 WHERE id=?2 AND closed_at IS NULL",
        params![now_ms(), batch_id],
    )?;
    tx.commit()?;
    Ok(n)
}

/// 候选池。
///
/// 详设 §7.5 写的是 `status ∈ {labeled, pending_confirm}`，但 §5.3 里
/// 「S 暂缓」是 `pending_confirm → queued`。两边一合，暂缓过的文件就永远回不了池
/// —— 与 §8.4「暂缓后回池」和验收 13 直接矛盾。
/// 所以这里把「已打过基础标签的 queued」也算进池：暂缓回来的文件正好落在这一格。
pub fn pool_rows(conn: &Connection) -> Result<Vec<crate::model::FileRow>, CmdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FILE_COLS} FROM files
         WHERE status IN ('labeled','pending_confirm')
            OR (status='queued' AND tag_level='basic')
         ORDER BY mtime DESC, id"
    ))?;
    let rows = stmt.query_map([], row_to_file)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn file_rows(conn: &Connection, ids: &[i64]) -> Result<Vec<crate::model::FileRow>, CmdError> {
    let mut out = Vec::new();
    let mut stmt = conn.prepare(&format!("SELECT {FILE_COLS} FROM files WHERE id=?1"))?;
    for id in ids {
        if let Some(r) = stmt.query_row([id], row_to_file).optional()? {
            out.push(r);
        }
    }
    Ok(out)
}

pub fn file_row(conn: &Connection, id: i64) -> Result<Option<crate::model::FileRow>, CmdError> {
    let r = conn
        .query_row(
            &format!("SELECT {FILE_COLS} FROM files WHERE id=?1"),
            [id],
            row_to_file,
        )
        .optional()?;
    Ok(r)
}

/// 池内还有多少份（与 `pool_rows` 同一口径）
pub fn pool_count(conn: &Connection) -> Result<i64, CmdError> {
    let n = conn.query_row(
        "SELECT COUNT(*) FROM files
         WHERE status IN ('labeled','pending_confirm')
            OR (status='queued' AND tag_level='basic')",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// 推荐用的目录特征：(id, name, rel_path, 该目录下已确认文件名)
pub fn dir_feature_rows(conn: &Connection) -> Result<Vec<(i64, String, String, Vec<String>)>, CmdError> {
    let mut dirs: Vec<(i64, String, String)> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT id, name, rel_path FROM archive_dirs ORDER BY depth, id")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for r in rows {
            dirs.push(r?);
        }
    }

    let mut by_dir: HashMap<i64, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT a.target_dir_id, f.name
             FROM assignments a JOIN files f ON f.id = a.file_id
             WHERE a.target_dir_id IS NOT NULL
               AND f.status IN ('confirmed','migrated')
               AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = a.file_id)
             ORDER BY a.decided_at DESC
             LIMIT 2000",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (d, n) = row?;
            let v = by_dir.entry(d).or_default();
            if v.len() < 20 {
                v.push(n);
            }
        }
    }

    Ok(dirs
        .into_iter()
        .map(|(id, name, rel)| {
            let names = by_dir.remove(&id).unwrap_or_default();
            (id, name, rel, names)
        })
        .collect())
}

pub fn count_status(conn: &Connection, status: Status) -> Result<i64, CmdError> {
    let n = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE status=?1",
        [status.as_str()],
        |r| r.get(0),
    )?;
    Ok(n)
}

// ---------------------------------------------------------------- 测试辅助
// 只在测试里用：直接摆一个文件行 / 直接改状态，绕开状态机以便测状态机本身。
// 放在 index.rs 里是为了守住「只有 index.rs 执行 SQL」这条红线。

#[cfg(test)]
pub fn insert_test_file(
    conn: &Connection,
    root_id: i64,
    name: &str,
    status: Status,
) -> Result<i64, CmdError> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO files
           (source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,
            tag_level, status, defer_count, discovered_at, last_seen_at, missing)
         VALUES (?1, ?2, ?2, ?3, 'docx', 'word', 100, ?4, 'basic', ?5, 0, ?4, ?4, 0)",
        params![root_id, format!("C:/t/{name}"), name, now, status.as_str()],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
pub fn force_status(conn: &Connection, file_id: i64, status: Status) -> Result<(), CmdError> {
    conn.execute(
        "UPDATE files SET status=?1 WHERE id=?2",
        params![status.as_str(), file_id],
    )?;
    Ok(())
}

#[cfg(test)]
pub fn file_size(conn: &Connection, abs_path: &str) -> Result<i64, CmdError> {
    let n = conn.query_row("SELECT size FROM files WHERE abs_path=?1", [abs_path], |r| {
        r.get(0)
    })?;
    Ok(n)
}
