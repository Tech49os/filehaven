//! BUG-003 目录体检 · 规则层（确定性预筛）
//!
//! 纯函数：只吃目录事实快照，不碰数据库、不发事件、不发模型请求。
//! 先按确定性规则筛出候选，模型复核（若有）只是二次确认，绝不静默执行。
//! 设计依据：`spec/BUG-003-dir-health-scan.md`、`spec/PLAN_BUG-003-dir-health-scan.md`。

use crate::model::{DirHealthIssue, DirHealthReport};
use std::time::Instant;
use crate::suggest_dirs::{
    is_filler_composed, strip_version_suffix, FAN_WORDS, FILLER_WORDS,
};

/// 括号包裹的版本/草稿词：`报告（最终版）` / `方案(最新版)`。
/// 不含年份/编码，避免误伤 `资料（2026）` 这类合法目录。
const BRACKET_VERSION_WORDS: &[&str] = &[
    "最终版", "最新版", "定稿", "终版", "初稿", "初版", "草稿", "副本", "打印版",
];

/// 一个目录的体检事实（由命令层从 DB 一次性投影出来，锁内取、锁外算）。
#[derive(Clone, Debug)]
pub struct DirFact {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub depth: i64,
    pub file_count: usize,
    pub child_count: usize,
}

/// 输出上限（spec §6：≤20 条，按置信度截断）。
const MAX_ISSUES: usize = 20;
/// 超过此层级视为过深（depth 从 1 起，5 层已明显难浏览）。
const MAX_DEPTH: i64 = 5;

fn issue(
    kind: &str,
    fact: &DirFact,
    target: Option<String>,
    reason: impl Into<String>,
    confidence: f64,
) -> DirHealthIssue {
    DirHealthIssue {
        id: format!("{kind}:{}", fact.id),
        kind: kind.to_string(),
        dir_id: fact.id,
        dir_name: fact.name.clone(),
        rel_path: fact.rel_path.clone(),
        target_path: target,
        reason: reason.into(),
        confidence,
        affected_files: fact.file_count,
        source: "rule".to_string(),
    }
}

/// 上提一层：把 rel_path 去掉最后一段；顶层无父可提则返回 None。
fn lift_one_level(rel_path: &str) -> Option<String> {
    let mut segs: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() <= 1 {
        return None;
    }
    segs.pop();
    Some(segs.join("/"))
}

/// 剥掉括号包裹的版本词：`报告（最终版）` → `报告`。剥不动就原样返回。
fn strip_bracket_version(s: &str) -> String {
    for (open, close) in [('（', '）'), ('(', ')')] {
        if s.ends_with(close) {
            if let Some(pos) = s.rfind(open) {
                let inner = s[pos + open.len_utf8()..s.len() - close.len_utf8()].trim();
                if BRACKET_VERSION_WORDS.iter().any(|w| *w == inner) {
                    return s[..pos].trim_end_matches([' ', '_', '-']).to_string();
                }
            }
        }
    }
    s.to_string()
}

/// 名字是否看起来是「年份/编码」这类刻意设计的基础目录（`2026`、`2026-Q4`、`GB-T1`）。
/// 这类目录剥掉数字后是空的，`is_filler_composed` 会误判成泛词，必须豁免。
fn looks_like_code_or_year(s: &str) -> bool {
    let has_digit = s.chars().any(|c| c.is_ascii_digit());
    let letters: String = s.chars().filter(|c| c.is_alphabetic()).collect();
    // 无字母或仅少量 ASCII 字母 + 数字/连字符：当作编码/年份
    has_digit
        && letters.len() <= 4
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ' | '·' | 'Q' | 'q'))
}

/// 该名字的「实际内容」是否确实由泛词/修饰词拼成。
/// 先排除年份编码，再要求至少命中一个已知泛词（否则 `智能工厂平台` 这种会被误伤）。
fn is_filler_name(s: &str) -> bool {
    if looks_like_code_or_year(s) {
        return false;
    }
    let hits_filler = FAN_WORDS
        .iter()
        .chain(FILLER_WORDS.iter())
        .any(|w| s.contains(w));
    hits_filler && is_filler_composed(s)
}

/// 确定性预筛。输入是目录事实快照，输出按置信度降序、截断到 20 条。
///
/// 六条规则互不叠加（一个目录只取置信度最高的那条），避免同一目录重复刷屏。
pub fn analyze(facts: &[DirFact]) -> Vec<DirHealthIssue> {
    let mut out: Vec<DirHealthIssue> = Vec::new();

    // 同父分组，用于「高度相似路径」检测
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<i64>, Vec<&DirFact>> = HashMap::new();
    for f in facts {
        by_parent.entry(f.parent_id).or_default().push(f);
    }

    for f in facts {
        // 规则 1：空目录（无文件、无子目录）
        if f.file_count == 0 && f.child_count == 0 {
            out.push(issue(
                "empty",
                f,
                None,
                "空目录：既没有文件也没有子目录",
                0.9,
            ));
            continue;
        }

        // 规则 2：超深层级
        if f.depth >= MAX_DEPTH {
            if let Some(target) = lift_one_level(&f.rel_path) {
                out.push(issue(
                    "move",
                    f,
                    Some(target),
                    format!("层级过深（第 {} 层），建议上提一层", f.depth),
                    0.7,
                ));
                continue;
            }
        }

        // 规则 3：同父下高度相似路径（剥掉版本后缀后同名）。
        // 必须先于 rename：像「项目资料」+「项目资料(1)」这样的一对，
        // 单看前者会被泛词规则判成 rename，但两者放一起显然是同义重复，归并更贴切。
        let normalized = strip_version_suffix(&strip_bracket_version(&f.name));
        if !looks_like_code_or_year(&f.name) && !normalized.trim().is_empty() {
            if let Some(siblings) = by_parent.get(&f.parent_id) {
                let dup = siblings.iter().find(|g| {
                    g.id != f.id
                        && strip_version_suffix(&strip_bracket_version(&g.name)) == normalized
                });
                if let Some(other) = dup {
                    out.push(issue(
                        "merge",
                        f,
                        Some(other.rel_path.clone()),
                        format!("与同层目录「{}」含义高度相似", other.name),
                        0.6,
                    ));
                    continue;
                }
            }
        }

        // 规则 4：版本/草稿后缀、纯泛词拼装名
        let unbracketed = strip_bracket_version(&f.name);
        let clean = strip_version_suffix(&unbracketed);
        let suffix_changed = clean.trim() != f.name.trim() && !clean.trim().is_empty();
        if suffix_changed || is_filler_name(&f.name) {
            let new_name = if suffix_changed { clean } else { f.name.clone() };
            let target = match f.rel_path.rfind('/') {
                Some(i) => format!("{}/{}", &f.rel_path[..i], new_name),
                None => new_name,
            };
            let reason = if suffix_changed {
                "名称含版本/草稿后缀，建议去掉后缀"
            } else {
                "名称由泛词拼装，缺少实际含义"
            };
            out.push(issue("rename", f, Some(target), reason, 0.85));
        }
    }

    // 同一目录只保留置信度最高的一条
    let mut best: std::collections::HashMap<i64, DirHealthIssue> = std::collections::HashMap::new();
    for it in out {
        match best.get(&it.dir_id) {
            Some(cur) if cur.confidence >= it.confidence => {}
            _ => {
                best.insert(it.dir_id, it);
            }
        }
    }

    let mut issues: Vec<DirHealthIssue> = best.into_values().collect();
    issues.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.dir_id.cmp(&b.dir_id))
    });
    issues.truncate(MAX_ISSUES);
    issues
}

/// 模型复核回调：吃规则候选，吐经模型确认/筛过的候选；失败返回原因（触发降级）。
pub type Reviewer<'a> = dyn Fn(&[DirHealthIssue]) -> Result<Vec<DirHealthIssue>, String> + 'a;

/// 体检编排（纯逻辑，不碰 DB / 事件总线 / 模型客户端）。
///
/// 阶段序列固定 `scan → rule → model → done`：即使模型不可用，`model` 阶段也会发，
/// 只是 note 里写明跳过原因，前端不必为「没发第三个事件」写特例。
///
/// 降级纪律（spec §3.2）：模型不可用 / 闸被占用 / 复核失败，一律返回规则候选 +
/// `model_reviewed=false` + note 含「未经模型复核」，**绝不返回 Err、绝不阻塞**。
pub fn scan_report(
    facts: &[DirFact],
    reviewer: &Reviewer,
    can_review: bool,
    skip_reason: &str,
    on_stage: &mut dyn FnMut(&str, u64, &str),
) -> DirHealthReport {
    let t0 = Instant::now();
    let mut report = DirHealthReport::default();

    on_stage("scan", 0, &format!("扫描到 {} 个归档目录", facts.len()));

    let mut issues = analyze(facts);
    on_stage("rule", t0.elapsed().as_millis() as u64, &format!("规则初筛 {} 条候选", issues.len()));

    if !can_review {
        report.note = if skip_reason.is_empty() {
            "模型不可用，仅规则初筛（未经模型复核）".to_string()
        } else {
            format!("{skip_reason}，仅规则初筛（未经模型复核）")
        };
        on_stage("model", t0.elapsed().as_millis() as u64, "跳过模型复核");
    } else {
        on_stage("model", t0.elapsed().as_millis() as u64, "模型复核中");
        match reviewer(&issues) {
            Ok(reviewed) => {
                issues = reviewed;
                report.model_reviewed = true;
                report.note = "规则初筛 + 模型复核".to_string();
            }
            Err(reason) => {
                report.note = format!("模型复核失败（{reason}），保留规则初筛（未经模型复核）");
            }
        }
    }

    report.issues = issues;
    report.elapsed_ms = t0.elapsed().as_millis() as u64;
    on_stage("done", report.elapsed_ms, &report.note);
    report
}

/// 从 DB 一次性投影目录事实（锁内调用）。两个 COUNT 用子查询，避免 N+1。
pub fn facts_from_db(conn: &rusqlite::Connection) -> Result<Vec<DirFact>, crate::error::CmdError> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.parent_id, d.name, d.rel_path, d.depth,
                (SELECT COUNT(*) FROM files f
                  WHERE f.status='migrated' AND f.archive_dir_id=d.id) AS file_count,
                (SELECT COUNT(*) FROM archive_dirs c WHERE c.parent_id=d.id) AS child_count
         FROM archive_dirs d ORDER BY d.rel_path",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DirFact {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            name: r.get(2)?,
            rel_path: r.get(3)?,
            depth: r.get(4)?,
            file_count: r.get::<_, i64>(5)? as usize,
            child_count: r.get::<_, i64>(6)? as usize,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::error::CmdError::from)
}

/// 模型复核 prompt：只让模型判「哪几条候选是真问题」，不让它自由发挥改目录名。
/// 复用项目既有教训：绝不采信模型给出的新归口/新路径，只用它做「确认/否决」。
fn review_prompt(issues: &[DirHealthIssue]) -> String {
    let mut lines = String::new();
    for x in issues {
        lines.push_str(&format!(
            "- id={} 类型={} 目录={} 依据={}\n",
            x.id, x.kind, x.rel_path, x.reason
        ));
    }
    format!(
        "你是归档目录质检员。下面是规则初筛出的可疑目录，请判断每条是否确实是问题。\
仅返回 JSON：{{\"keep\":[\"id1\",\"id2\"]}}，keep 里放你认可的 id。\
不确定的不要放进去。不要新增条目，不要修改任何目录名。\n\n候选：\n{lines}"
    )
}

/// 从模型响应里容错提取 keep 列表（兼容 ```json 围栏、前后废话）。
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
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
    )
}

/// 真实模型复核闭包工厂：捕获 base/model/timeout，返回一个可供 `scan_report` 用的 Reviewer。
///
/// 语义：模型只做「确认」——它 keep 的才保留；拒绝的候选直接丢弃（模型说不是问题）。
/// 这样即使模型把项目背景当归口，最坏也只是少报，不会静默改动任何目录。
pub fn make_reviewer<'a>(
    base: &'a str,
    model: &'a str,
    timeout: std::time::Duration,
) -> impl Fn(&[DirHealthIssue]) -> Result<Vec<DirHealthIssue>, String> + 'a {
    move |issues| {
        let prompt = review_prompt(issues);
        let raw = crate::llm::generate(base, model, &prompt, timeout)
            .map_err(|e| e.message)?;
        let keep = parse_keep(&raw).ok_or_else(|| "返回不是合法 JSON".to_string())?;
        let kept: Vec<DirHealthIssue> = issues
            .iter()
            .filter(|x| keep.contains(&x.id))
            .cloned()
            .collect();
        Ok(kept)
    }
}

/// 采纳一条 rename/move：把 `dir_id` 的 rel_path 改成 `new_rel_path`，
/// 连同所有后代目录与受影响文件的 archived_path 一起改，磁盘目录同步改名，写批次可撤销。
///
/// 安全纪律：
/// - 磁盘目标已存在 → unique_name 错开 `_2`，绝不覆盖；
/// - 全部 DB 改动在一个事务里，磁盘改名失败则事务不提交；
/// - 台账 kind='dir_health'，每个受影响目录一条 item（src=old_rel, dst=new_rel）。
pub fn apply_rename(
    conn: &mut rusqlite::Connection,
    archive_root: &std::path::Path,
    dir_id: i64,
    new_rel_path: &str,
    _source: &str,
) -> Result<crate::model::DirHealthApplyAck, crate::error::CmdError> {
    use crate::error::CmdError;

    let dir = crate::index::get_dir(conn, dir_id)?
        .ok_or_else(|| CmdError::new(crate::error::BAD_INPUT, "目录不存在"))?;
    let old_rel = dir.rel_path.clone();
    if old_rel == new_rel_path {
        return Err(CmdError::new(crate::error::BAD_INPUT, "目标路径与当前相同"));
    }

    // 磁盘目标撞名错开（对目标目录的父目录做 unique_name）
    let new_clean = new_rel_path.trim_end_matches('/').to_string();
    let new_path_disk = archive_root.join(&new_clean);
    let mut actual = new_clean.clone();
    let mut renamed = false;
    if new_path_disk.exists() {
        let parent = new_path_disk.parent().unwrap_or(archive_root);
        let base = new_clean.rsplit('/').next().unwrap_or(&new_clean);
        let unique = crate::plan::unique_name(parent, base);
        actual = match parent.strip_prefix(archive_root) {
            Ok(rel) if !rel.as_os_str().is_empty() => {
                format!("{}/{}", rel.to_string_lossy().replace('\\', "/"), unique)
            }
            _ => unique,
        };
        renamed = true;
    }

    // 一次性查出所有受影响目录（目标 + 后代），按 depth 升序（先改浅的）
    let descendants: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, rel_path FROM archive_dirs
             WHERE id=?1 OR substr(rel_path,1,length(?2)+1)=?2||'/'
             ORDER BY depth, id",
        )?;
        let rows = stmt.query_map(rusqlite::params![dir_id, old_rel], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let prefix_len = old_rel.len();
    let mapped: Vec<(i64, String, String)> = descendants
        .iter()
        .map(|(id, rel)| {
            let new = format!("{}{}", actual, &rel[prefix_len..]);
            (*id, rel.clone(), new)
        })
        .collect();

    // 受影响文件数（按目录 id 集合）
    let affected_files: usize = {
        let ids: Vec<i64> = mapped.iter().map(|(id, _, _)| *id).collect();
        if ids.is_empty() {
            0
        } else {
            let marks = vec!["?"; ids.len()].join(",");
            let sql = format!(
                "SELECT COUNT(*) FROM files WHERE archive_dir_id IN ({marks}) AND archived_path IS NOT NULL"
            );
            let params: Vec<&i64> = ids.iter().collect();
            conn.query_row(&sql, rusqlite::params_from_iter(params), |r| r.get::<_, i64>(0))? as usize
        }
    };

    // 磁盘改名（先做，失败则不动 DB）
    let src_disk = archive_root.join(&old_rel);
    let dst_disk = archive_root.join(&actual);
    if src_disk.exists() {
        if let Some(p) = dst_disk.parent() {
            std::fs::create_dir_all(p).map_err(|e| CmdError::new(crate::error::IO, format!("创建目标目录失败：{e}")))?;
        }
        std::fs::rename(&src_disk, &dst_disk)
            .map_err(|e| CmdError::new(crate::error::IO, format!("目录改名失败（目标已存在则不会覆盖）：{e}")))?;
    }

    let result = (|| -> Result<crate::model::DirHealthApplyAck, CmdError> {
        let tx = conn.transaction()?;
        // 目录 rel_path：按映射逐个改（每个 rel_path 唯一，无中间碰撞）
        for (id, _, new_rel) in &mapped {
            tx.execute(
                "UPDATE archive_dirs SET rel_path=?1 WHERE id=?2",
                rusqlite::params![new_rel, id],
            )?;
        }
        // 文件 archived_path 是「归档根 + rel_path」的绝对路径，前缀要用绝对目录路径替换，
        // 不能用相对 rel_path（会漏掉盘符/反斜杠）。substr 精确匹配开头，避免误替换路径中段的同名段。
        for (id, old, new) in &mapped {
            let old_prefix = archive_root.join(old).to_string_lossy().to_string();
            let new_prefix = archive_root.join(new).to_string_lossy().to_string();
            tx.execute(
                "UPDATE files SET archived_path=?1 || substr(archived_path, ?2)
                 WHERE archive_dir_id=?3 AND archived_path IS NOT NULL
                   AND substr(archived_path,1,?4)=?5",
                rusqlite::params![new_prefix, old_prefix.chars().count() + 1, id, old_prefix.chars().count(), old_prefix],
            )?;
        }
        let mig_id = crate::index::insert_migration_with_kind(&tx, affected_files as i64, "dir_health")?;
        for (id, old, new) in &mapped {
            crate::index::insert_migration_item_ext(
                &tx, mig_id, 0, old, new, None, "ok", None, Some(*id),
            )?;
        }
        crate::index::finish_migration(&tx, mig_id, "done", 0)?;
        tx.commit()?;
        Ok(crate::model::DirHealthApplyAck {
            migration_id: mig_id,
            new_rel_path: actual.clone(),
            renamed,
            affected_dirs: mapped.len(),
            affected_files,
        })
    })();

    if result.is_err() {
        // DB 失败：磁盘已改名，尝试改回（尽力而为，不覆盖用户后续改动）
        if src_disk.exists() == false && dst_disk.exists() {
            let _ = std::fs::rename(&dst_disk, &src_disk);
        }
    }
    result
}

/// 撤销一批 dir_health 修正：按台账逆序把目录 rel_path 与文件 archived_path 改回。
pub fn undo_dir_health(
    conn: &mut rusqlite::Connection,
    archive_root: &std::path::Path,
    migration_id: i64,
) -> Result<crate::model::UndoReport, crate::error::CmdError> {
    use crate::error::CmdError;
    use crate::model::{MigrationFailure, UndoReport};

    let mig = crate::index::get_migration(conn, migration_id)?
        .ok_or_else(|| CmdError::new(crate::error::MIGRATE_FAILED, "找不到修正批次"))?;
    if mig.status == "undone" {
        return Err(CmdError::new(crate::error::MIGRATE_FAILED, "这批修正已经撤销过了"));
    }
    let items: Vec<(crate::index::MigrationItemRow, String)> = crate::index::migration_items(conn, migration_id)?
        .into_iter()
        .filter(|(_, r)| r == "ok")
        .collect();

    let total = items.len();
    let mut restored = 0usize;
    let mut failed = Vec::new();

    // 逆序：深层先还原，避免前缀碰撞
    for (item, _) in items.into_iter().rev() {
        let dir_id = item.old_dir_id.unwrap_or(0);
        let old_rel = item.src_path.clone(); // 要还原成的路径
        let cur_rel = item.dst_path.clone(); // 当前路径
        let cur_disk = archive_root.join(&cur_rel);
        let old_disk = archive_root.join(&old_rel);
        // 磁盘：当前位置在、旧位置空，才能安全改回
        if cur_disk.exists() && !old_disk.exists() {
            if let Some(p) = old_disk.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if let Err(e) = std::fs::rename(&cur_disk, &old_disk) {
                failed.push(MigrationFailure {
                    file_id: 0,
                    name: old_rel.clone(),
                    reason: format!("磁盘还原失败：{e}"),
                });
                continue;
            }
        }
        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(e) => {
                failed.push(MigrationFailure { file_id: 0, name: old_rel.clone(), reason: e.to_string() });
                continue;
            }
        };
        let r = (|| -> Result<(), rusqlite::Error> {
            tx.execute("UPDATE archive_dirs SET rel_path=?1 WHERE id=?2", rusqlite::params![old_rel, dir_id])?;
            let cur_prefix = archive_root.join(&cur_rel).to_string_lossy().to_string();
            let old_prefix = archive_root.join(&old_rel).to_string_lossy().to_string();
            tx.execute(
                "UPDATE files SET archived_path=?1 || substr(archived_path, ?2)
                 WHERE archive_dir_id=?3 AND archived_path IS NOT NULL
                   AND substr(archived_path,1,?4)=?5",
                rusqlite::params![old_prefix, cur_prefix.chars().count() + 1, dir_id, cur_prefix.chars().count(), cur_prefix],
            )?;
            Ok(())
        })();
        match r {
            Ok(()) => { let _ = tx.commit(); restored += 1; }
            Err(e) => failed.push(MigrationFailure { file_id: 0, name: old_rel.clone(), reason: e.to_string() }),
        }
    }

    if failed.is_empty() {
        crate::index::finish_migration(conn, migration_id, "undone", mig.bytes)?;
    }
    Ok(UndoReport { restored, skipped: 0, failed })
}
