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
    // schema.sql 是 CREATE TABLE IF NOT EXISTS —— 老库不会因为改了建表语句就多出列，
    // 后加的列必须在这里补一次（幂等）
    ensure_column(&conn, "files", "target_name", "TEXT")?;
    Ok(conn)
}

/// 表里没有这列就加上；已经有就什么都不做。
///
/// 返回 true 表示这次真的执行了 ALTER，便于测试与诊断。
pub fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<bool, CmdError> {
    if has_column(conn, table, column)? {
        return Ok(false);
    }
    conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    Ok(true)
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, CmdError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let name: String = r.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
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
            // 先看「是不是真的变了」：size 与 mtime 都没动就是没变。
            // 只刷 last_seen_at（单列 UPDATE），不走整行 upsert ——
            // P1 每份都算一次 update，455 份从 0.3s 涨到 2s 以上，而且统计里
            // 「更新 N」永远等于「扫描 N」，用户根本看不出哪里变了。
            let existed: Option<(i64, i64, i64, String)> = tx
                .query_row(
                    "SELECT id, size, mtime, status FROM files WHERE abs_path=?1",
                    [&f.path],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            if let Some((id, size, mtime, status)) = existed.clone() {
                // 注意 `status != 'missing'`：文件删了又原样放回来时，
                // size/mtime 一模一样，但状态必须从 missing 恢复成 queued，
                // 走这条捷径就会永远留在 missing（实测过）
                if status != "missing" && size == f.size as i64 && mtime == f.mtime {
                    tx.execute(
                        "UPDATE files SET last_seen_at=?1 WHERE id=?2",
                        params![now, id],
                    )?;
                    stats.unchanged += 1;
                    continue;
                }
            }
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

/// 备注：空白一律写 NULL —— 库里不允许「没备注」有两种表示
pub fn set_note(conn: &Connection, file_id: i64, note: &str) -> Result<(), CmdError> {
    let v = note.trim();
    let affected = if v.is_empty() {
        conn.execute("UPDATE files SET note=NULL WHERE id=?1", [file_id])?
    } else {
        conn.execute("UPDATE files SET note=?1 WHERE id=?2", params![v, file_id])?
    };
    if affected == 0 {
        return Err(CmdError::new(
            "NOT_FOUND",
            format!("文件不存在：{file_id}"),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- 批次

const FILE_COLS: &str =
    "id, name, ext, kind, size, mtime, status, rel_path, abs_path, source_root_id, defer_count, note, target_name, summary, tag_level";

/// **按列名取值**，不用下标。
///
/// P1 栽过：`SELECT {FILE_COLS}, d.id` 这类拼接语句里，加一列就让后面所有
/// `r.get(13)` 全部错位，而且编译器一声不吭。按名取值后，「加列」这件事
/// 顶多多读一个字段，不会再静默地把 note 读成 rel_path。
fn row_to_file(r: &rusqlite::Row<'_>) -> rusqlite::Result<crate::model::FileRow> {
    Ok(crate::model::FileRow {
        id: r.get("id")?,
        name: r.get("name")?,
        ext: r.get("ext")?,
        kind: r.get("kind")?,
        size: r.get("size")?,
        mtime: r.get("mtime")?,
        status: r.get("status")?,
        rel_path: r.get("rel_path")?,
        abs_path: r.get("abs_path")?,
        source_root_id: r.get("source_root_id")?,
        defer_count: r.get("defer_count")?,
        note: r.get("note")?,
        target_name: r.get("target_name")?,
        summary: r.get("summary")?,
        tag_level: r.get("tag_level")?,
    })
}

/// 归档时用的名字：改过就用新名字，没改就是原名
pub fn effective_name(f: &crate::model::FileRow) -> String {
    f.target_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| f.name.clone())
}

/// 写入/清除「归档时改成的名字」（空串 = 恢复原名）
pub fn set_target_name(
    conn: &Connection,
    file_id: i64,
    name: Option<&str>,
) -> Result<(), CmdError> {
    let v = name.map(|s| s.trim()).filter(|s| !s.is_empty());
    let n = conn.execute(
        "UPDATE files SET target_name=?1 WHERE id=?2",
        params![v, file_id],
    )?;
    if n == 0 {
        return Err(CmdError::new(
            "NOT_FOUND",
            format!("文件不存在：{file_id}"),
        ));
    }
    Ok(())
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
            OR (status='queued' AND tag_level IN ('basic','deep'))
         ORDER BY mtime DESC, id"
    ))?;
    let rows = stmt.query_map([], row_to_file)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 目录建议的抽样输入：除 skipped / missing 之外的所有文件（详设 §7.6）
///
/// 已迁移的也算进来 —— 它们同样是「这个人在管什么」的证据，
/// 只是不再需要被整理。`source` 取路径最后两层，用来告诉模型文件原本散在哪。
pub fn suggest_input_rows(
    conn: &Connection,
) -> Result<Vec<crate::suggest_dirs::SampleInput>, CmdError> {
    let mut stmt = conn.prepare(
        "SELECT name, kind, abs_path, mtime FROM files
         WHERE status NOT IN ('skipped','missing')
         ORDER BY mtime DESC, id",
    )?;
    let rows = stmt.query_map([], |r| {
        let name: String = r.get(0)?;
        let kind: String = r.get(1)?;
        let abs: String = r.get(2)?;
        let mtime: i64 = r.get(3)?;
        Ok(crate::suggest_dirs::SampleInput {
            name,
            kind,
            source: crate::suggest_dirs::source_of(&abs),
            mtime,
        })
    })?;
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
            OR (status='queued' AND tag_level IN ('basic','deep'))",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// 推荐用的目录特征（P2 T1.1：一次查全，禁 N+1）
pub struct DirFeatureRow {
    pub id: i64,
    pub name: String,
    pub rel_path: String,
    /// 该目录下已确认文件名（最多 20 份，够 familyMatch 用）
    pub confirmed_names: Vec<String>,
    /// 类型分布：word/excel/ppt/pdf/…
    pub kind_hist: HashMap<String, i64>,
    /// 该目录已确认文件的 topic 标签
    pub topic_tags: Vec<String>,
}

pub fn dir_feature_rows(conn: &Connection) -> Result<Vec<DirFeatureRow>, CmdError> {
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

    // 一次查全：名字 + 类型（kind_hist 要算全量，不能只算前 20 份）
    let mut by_dir: HashMap<i64, (Vec<String>, HashMap<String, i64>)> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT a.target_dir_id, f.name, f.kind
             FROM assignments a JOIN files f ON f.id = a.file_id
             WHERE a.target_dir_id IS NOT NULL
               AND f.status IN ('confirmed','migrated')
               AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = a.file_id)
             ORDER BY a.decided_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (d, n, k) = row?;
            let e = by_dir.entry(d).or_default();
            if e.0.len() < 20 {
                e.0.push(n);
            }
            *e.1.entry(k).or_insert(0) += 1;
        }
    }

    // 一次查全：topic 标签（P1 从没写过，但通道要通，M2 会往里写）
    let mut tags_by_dir: HashMap<i64, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT a.target_dir_id, t.name
             FROM assignments a
             JOIN files f ON f.id = a.file_id
             JOIN file_tags ft ON ft.file_id = f.id
             JOIN tags t ON t.id = ft.tag_id AND t.kind = 'topic'
             WHERE a.target_dir_id IS NOT NULL
               AND f.status IN ('confirmed','migrated')
               AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = a.file_id)
             GROUP BY a.target_dir_id, t.name",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (d, t) = row?;
            tags_by_dir.entry(d).or_default().push(t);
        }
    }

    Ok(dirs
        .into_iter()
        .map(|(id, name, rel)| {
            let (names, kind_hist) = by_dir.remove(&id).unwrap_or_default();
            let topic_tags = tags_by_dir.remove(&id).unwrap_or_default();
            DirFeatureRow {
                id,
                name,
                rel_path: rel,
                confirmed_names: names,
                kind_hist,
                topic_tags,
            }
        })
        .collect())
}

/// 批量取 topic 标签：`file_id → [标签名]`（一次一条 SQL，禁 N+1）
pub fn topic_tags_for(
    conn: &Connection,
    ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>, CmdError> {
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for chunk in ids.chunks(500) {
        if chunk.is_empty() {
            continue;
        }
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT ft.file_id, t.name
             FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
             WHERE t.kind='topic' AND ft.file_id IN ({marks})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (fid, tag) = row?;
            out.entry(fid).or_default().push(tag);
        }
    }
    Ok(out)
}


/// 深度标签落库（T2.5）：单事务写 tags + file_tags + summary + tag_level
///
/// **失败不动 `tag_level`** —— 写不进去就是没读过，下一轮队列会再试。
/// `summary` 为空写 NULL 而不是空串：NULL 表示「读过但没有可说的」，
/// 与「没读过」（`tag_level='basic'`）是两件事，界面文案也不同。
pub fn apply_deep_tag(
    conn: &mut Connection,
    file_id: i64,
    tags: &[String],
    summary: Option<&str>,
) -> Result<(), CmdError> {
    let tx = conn.transaction()?;
    // 重跑时不留脏数据：只清 topic 类，基础标签（type/time/keyword）不能动
    tx.execute(
        "DELETE FROM file_tags WHERE file_id=?1
           AND tag_id IN (SELECT id FROM tags WHERE kind='topic')",
        params![file_id],
    )?;
    for name in tags {
        tx.execute(
            "INSERT OR IGNORE INTO tags(name, kind) VALUES(?1, 'topic')",
            params![name],
        )?;
        let tid: i64 = tx.query_row(
            "SELECT id FROM tags WHERE name=?1 AND kind='topic'",
            params![name],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO file_tags(file_id, tag_id, confidence) VALUES(?1,?2,0.8)",
            params![file_id, tid],
        )?;
    }
    let sum = summary.map(|s| s.trim()).filter(|s| !s.is_empty());
    tx.execute(
        "UPDATE files SET summary=?1, tag_level='deep' WHERE id=?2",
        params![sum, file_id],
    )?;
    // 刚扫描进来、还没来得及打基础标签的文件：标成 labeled 才留得住池内资格
    // （`pool_rows` 的 queued 那一支只在有标签时才算池内）
    tx.execute(
        "UPDATE files SET status='labeled' WHERE id=?1 AND status='queued'",
        params![file_id],
    )?;
    tx.commit()?;
    Ok(())
}

/// 下一条待深读的文件（T2.6 取件顺序）
///
/// 顺序与 `deep_tag::priority` 逐条对应：当前批 → 待定区 → 池内 → 已迁移。
/// `batch_ids` 由调用方从 AppState 的当前批快照里取，用 i64 拼进 SQL（不经过用户输入）。
pub fn next_deep_job(
    conn: &Connection,
    batch_ids: &[i64],
) -> Result<Option<crate::deep_tag::Job>, CmdError> {
    let in_batch = if batch_ids.is_empty() {
        "-1".to_string()
    } else {
        batch_ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let sql = format!(
        "SELECT id, name, abs_path, ext, kind, size FROM files
         WHERE tag_level='basic' AND status NOT IN ('missing','skipped')
         ORDER BY
           CASE
             WHEN id IN ({in_batch}) THEN 0
             WHEN status='parked' THEN 1
             WHEN status IN ('queued','labeled','pending_confirm') THEN 2
             WHEN status IN ('migrated','confirmed') THEN 3
             ELSE 9
           END,
           mtime DESC, id
         LIMIT 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let job = stmt
        .query_row([], |r| {
            Ok(crate::deep_tag::Job {
                file_id: r.get(0)?,
                name: r.get(1)?,
                abs_path: r.get(2)?,
                ext: r.get(3)?,
                kind: r.get(4)?,
                size: r.get::<_, i64>(5)? as u64,
            })
        })
        .optional()?;
    Ok(job)
}

/// 某个绝对路径属于哪个源区（取最长前缀，多个源区嵌套时不会挑错）
pub fn root_for_path(conn: &Connection, path: &str) -> Result<Option<i64>, CmdError> {
    let low = path.replace('\\', "/").to_lowercase();
    let mut best: Option<(usize, i64)> = None;
    for root in list_source_roots(conn)? {
        if !root.enabled {
            continue;
        }
        let r = root.path.replace('\\', "/").to_lowercase();
        if low.starts_with(&r) && best.map(|(n, _)| r.len() > n).unwrap_or(true) {
            best = Some((r.len(), root.id));
        }
    }
    Ok(best.map(|(_, id)| id))
}

/// 监听看到「文件不见了」时调用（T3.5 守卫）
///
/// **只对 3 个状态生效**：`queued` / `labeled` / `pending_confirm`。
/// `confirmed` / `migrated` / `parked` / `skipped` 一律不动 ——
/// 迁移就是把源文件搬进隔离区，监听必然会看到删除；
/// 按「未见即 missing」处理会把刚迁移的整批文件打成丢失（计划 T3.5）。
/// 返回 true 表示这次真的标了。
pub fn mark_missing_if_pending(conn: &Connection, abs_path: &str) -> Result<bool, CmdError> {
    let n = conn.execute(
        "UPDATE files SET status='missing'
         WHERE abs_path=?1 AND status IN ('queued','labeled','pending_confirm')",
        [abs_path],
    )?;
    Ok(n > 0)
}

/// 队列进度：还剩多少份没深读
pub fn deep_remaining(conn: &Connection) -> Result<i64, CmdError> {
    let n = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE tag_level='basic' AND status NOT IN ('missing','skipped')",
        [],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// 当前批快照（给队列排优先级用）
pub fn batch_file_ids(conn: &Connection, batch_id: i64) -> Result<Vec<i64>, CmdError> {
    let mut stmt = conn.prepare(
        "SELECT file_id FROM assignments WHERE batch_id=?1 ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([batch_id], |r| r.get::<_, i64>(0))?;
    let mut out: Vec<i64> = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn count_status(conn: &Connection, status: Status) -> Result<i64, CmdError> {    let n = conn.query_row(
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
    insert_test_file_kind(conn, root_id, name, "docx", "word", status)
}

/// 同上，但能指定扩展名与类型（kind_hist 的测试要混 word/pdf）
#[cfg(test)]
pub fn insert_test_file_kind(
    conn: &Connection,
    root_id: i64,
    name: &str,
    ext: &str,
    kind: &str,
    status: Status,
) -> Result<i64, CmdError> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO files
           (source_root_id, abs_path, rel_path, name, ext, kind, size, mtime,
            tag_level, status, defer_count, discovered_at, last_seen_at, missing)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, 100, ?6, 'basic', ?7, 0, ?6, ?6, 0)",
        params![
            root_id,
            format!("C:/t/{name}"),
            name,
            ext,
            kind,
            now,
            status.as_str()
        ],
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

/// 测试用：给一份文件挂一个 topic 标签（深度标签的完整流程走 `apply_deep_tag`）
#[cfg(test)]
pub fn insert_test_topic_tag(
    conn: &Connection,
    file_id: i64,
    tag: &str,
) -> Result<(), CmdError> {
    conn.execute(
        "INSERT OR IGNORE INTO tags(name, kind) VALUES(?1, 'topic')",
        params![tag],
    )?;
    let tid: i64 = conn.query_row(
        "SELECT id FROM tags WHERE name=?1 AND kind='topic'",
        params![tag],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO file_tags(file_id, tag_id, confidence) VALUES(?1, ?2, 0.8)",
        params![file_id, tid],
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

// ---------------------------------------------------------------- 迁移台账

/// 待迁移的一份文件 + 目标目录（一次 join 拿全，不做 N+1）
#[derive(Clone, Debug)]
pub struct ConfirmedRow {
    pub file: crate::model::FileRow,
    pub dir_id: i64,
    pub dir_rel_path: String,
}

/// 现役判定：该文件最新一条 assignment 指向的目录
pub fn confirmed_for_migration(conn: &Connection) -> Result<Vec<ConfirmedRow>, CmdError> {
    let cols = FILE_COLS
        .split(',')
        .map(|c| format!("f.{}", c.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {cols}, d.id, d.rel_path
         FROM files f
         JOIN assignments a ON a.file_id = f.id AND a.target_dir_id IS NOT NULL
         JOIN archive_dirs d ON d.id = a.target_dir_id
         WHERE f.status='confirmed'
           AND a.id = (SELECT MAX(a2.id) FROM assignments a2 WHERE a2.file_id = f.id)
         ORDER BY d.rel_path, f.name, f.id"
    );
    let mut stmt = conn.prepare(&sql)?;
    // 目录两列紧跟在 FILE_COLS 后面：偏移量由列数算出来，别写死
    // （写死过一次：加 target_name 之后 12/13 全错位，读成 Null 才炸出来）
    let off = FILE_COLS.split(',').count();
    let rows = stmt.query_map([], move |r| {
        Ok(ConfirmedRow {
            file: row_to_file(r)?,
            dir_id: r.get(off)?,
            dir_rel_path: r.get(off + 1)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct MigrationRow {
    pub id: i64,
    /// running|done|partial|failed|undone
    pub status: String,
    pub file_count: i64,
    pub bytes: i64,
    pub finished_at: Option<i64>,
}

const MIG_COLS: &str = "id, status, file_count, bytes, finished_at";

fn row_to_migration(r: &rusqlite::Row<'_>) -> rusqlite::Result<MigrationRow> {
    Ok(MigrationRow {
        id: r.get(0)?,
        status: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        file_count: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
        bytes: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
        finished_at: r.get(4)?,
    })
}

pub fn insert_migration(conn: &Connection, file_count: i64) -> Result<i64, CmdError> {
    conn.execute(
        "INSERT INTO migrations(batch_id, started_at, status, file_count, bytes)
         VALUES(NULL, ?1, 'running', ?2, 0)",
        params![now_ms(), file_count],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_migration(
    conn: &Connection,
    id: i64,
    status: &str,
    bytes: i64,
) -> Result<(), CmdError> {
    conn.execute(
        "UPDATE migrations SET finished_at=?1, status=?2, bytes=?3 WHERE id=?4",
        params![now_ms(), status, bytes, id],
    )?;
    Ok(())
}

pub fn get_migration(conn: &Connection, id: i64) -> Result<Option<MigrationRow>, CmdError> {
    let r = conn
        .query_row(
            &format!("SELECT {MIG_COLS} FROM migrations WHERE id=?1"),
            [id],
            row_to_migration,
        )
        .optional()?;
    Ok(r)
}

/// 最近一次迁移（撤销入口用它，重启后也能撤）
pub fn latest_migration(conn: &Connection) -> Result<Option<MigrationRow>, CmdError> {
    let r = conn
        .query_row(
            &format!("SELECT {MIG_COLS} FROM migrations ORDER BY id DESC LIMIT 1"),
            [],
            row_to_migration,
        )
        .optional()?;
    Ok(r)
}

#[derive(Clone, Debug)]
pub struct MigrationItemRow {
    pub file_id: Option<i64>,
    pub src_path: String,
    pub dst_path: String,
    pub sha256: Option<String>,
}

pub fn insert_migration_item(
    conn: &Connection,
    migration_id: i64,
    file_id: i64,
    src_path: &str,
    dst_path: &str,
    sha256: Option<&str>,
    result: &str,
    error: Option<&str>,
) -> Result<(), CmdError> {
    conn.execute(
        "INSERT INTO migration_items(migration_id, file_id, src_path, dst_path, sha256, result, error)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![migration_id, file_id, src_path, dst_path, sha256, result, error],
    )?;
    Ok(())
}

/// 一次迁移的全部条目（按 id 正序 = 迁移顺序，撤销要逆着走）
pub fn migration_items(
    conn: &Connection,
    migration_id: i64,
) -> Result<Vec<(MigrationItemRow, String)>, CmdError> {
    let mut stmt = conn.prepare(
        "SELECT file_id, src_path, dst_path, sha256, COALESCE(result,'')
         FROM migration_items WHERE migration_id=?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([migration_id], |r| {
        Ok((
            MigrationItemRow {
                file_id: r.get(0)?,
                src_path: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                dst_path: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                sha256: r.get(3)?,
            },
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 重新确认：退回池里，同时按磁盘现在的状态刷新 size/mtime
///
/// 不刷新的话，这份文件会被迁移前检查永远判成「标注后已改」，卡在原地出不去。
pub fn requeue_confirmed(
    conn: &Connection,
    file_id: i64,
    size: i64,
    mtime: i64,
) -> Result<(), CmdError> {
    set_status(conn, file_id, Status::Queued)?;
    conn.execute(
        "UPDATE files SET size=?1, mtime=?2 WHERE id=?3",
        params![size, mtime, file_id],
    )?;
    Ok(())
}

/// 文件所属源区的路径（撤销时要用它算隔离区里的相对路径）
pub fn get_source_root_of_file(conn: &Connection, file_id: i64) -> Result<Option<String>, CmdError> {
    let p: Option<Option<String>> = conn
        .query_row(
            "SELECT r.path FROM files f
             LEFT JOIN source_roots r ON r.id = f.source_root_id
             WHERE f.id=?1",
            [file_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(p.flatten())
}
