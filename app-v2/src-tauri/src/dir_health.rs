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
