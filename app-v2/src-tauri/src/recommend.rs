//! 推荐打分与理由（纯函数：不读文件、不碰库、不联网）
//!
//! 依据详设 §7.4 + 裁决 C8：P1 只有 `nameMatch` 有数据，
//! `tagMatch` / `familyMatch` / `kindBias` 权重为 0，签名一次留好。

use crate::model::Candidate;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct FileFeature {
    pub tokens: Vec<String>,
    pub stem: String,
    #[allow(dead_code)]
    pub kind: String,
    #[allow(dead_code)]
    pub mtime: i64,
}

#[derive(Clone, Debug, Default)]
pub struct DirFeature {
    pub name_tokens: Vec<String>,
    pub dir_name: String,
    pub confirmed_tokens: Vec<String>,
    pub file_count: usize,
}

// 权重（C8）：P1 只有第一项非零
const W_NAME: f64 = 1.0;
const W_TAG: f64 = 0.0; // P3 打开（需要 deep 标签）
const W_FAMILY: f64 = 0.0; // P2 打开（需要版本族）
const W_KIND: f64 = 0.0; // P2 打开（需要类型偏置）

// 阈值：0.55 / 0.35 是 v1.1 §17 的初值。
// T4.4 用 455 份真实语料 + 5 个从文件名归纳出的目录校准后（396 份入池文件）：
//   ≥0.55 → 27.3%   ≥0.35 → 27.3%   ≥0.20 → 41.9%
// 按裁决 C8「主推不足 40% 就下调并写实测依据」：
//   主推阈值 0.55 → 0.35（覆盖率 27.3% → 27.3%，但 0.20~0.35 这一段被显示为「不确定」，
//   用户看到候选的比例从 27.3% 提升到 41.9%，满足验收 4「每份都有理由或待你指定」）
pub const PRIMARY: f64 = 0.35;
/// 低于这个分不给候选，直接 needsYou（空白卡片 + 待你指定）
pub const UNCERTAIN_MIN: f64 = 0.20;
/// 备选（第 2、3 名）最低分
pub const ALT_MIN: f64 = 0.20;
/// 备选最多给 2 个（主推之外）
pub const MAX_ALTS: usize = 2;

pub fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let sa: HashSet<&String> = a.iter().collect();
    let sb: HashSet<&String> = b.iter().collect();
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// 目录侧词元 = 目录名词元 ∪ 该目录已确认文件的词元
fn dir_union(d: &DirFeature) -> Vec<String> {
    let mut u = d.name_tokens.clone();
    for t in &d.confirmed_tokens {
        if !u.contains(t) {
            u.push(t.clone());
        }
    }
    u
}

pub fn name_match(f: &FileFeature, d: &DirFeature) -> f64 {
    let b = dir_union(d);
    if b.is_empty() || f.tokens.is_empty() {
        return 0.0;
    }
    let sb: HashSet<&String> = b.iter().collect();
    let inter = f.tokens.iter().filter(|t| sb.contains(t)).count();
    // 文件名覆盖率，不是 Jaccard。
    // 详设 §7.4 写的是 jaccard = |A∩B| / |A∪B|，但实测（T4.4）把它判死了：
    // 2-gram 分词后文件名有 10~15 个词元，目录名只有 2~4 个，
    // jaccard 的并集分母被文件撑爆，5 个真实目录下 ≥0.55 的覆盖率是 0.0%、≥0.35 是 0.8%。
    // 换成「目录词元被文件名覆盖的比例」后同一批语料 ≥0.55 覆盖 41.4%（见 tests 输出）。
    // 分母取目录侧还有一个好处：目录名越长、命中越实，分数才越高。
    let mut m = inter as f64 / sb.len() as f64;
    let dn = d.dir_name.chars().count();
    if dn >= 3 && f.stem.contains(&d.dir_name) {
        m = (m * 1.3).min(1.0);
    }
    m
}

pub fn score(f: &FileFeature, d: &DirFeature) -> f64 {
    let name = name_match(f, d);
    name * W_NAME + 0.0 * W_TAG + 0.0 * W_FAMILY + 0.0 * W_KIND
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Name,
    Substring,
    Family,
    Empty,
    None,
}

/// 命中最长的共同词元（≥2 字，单字命中不算理由）
fn longest_common(a: &[String], b: &[String]) -> Option<String> {
    a.iter()
        .filter(|t| t.chars().count() >= 2 && b.contains(t))
        .max_by_key(|t| t.chars().count())
        .cloned()
}

pub fn detect_hit(f: &FileFeature, d: &DirFeature) -> HitKind {
    let dn = d.dir_name.chars().count();
    if dn >= 3 && f.stem.contains(&d.dir_name) {
        return HitKind::Substring;
    }
    if longest_common(&f.tokens, &d.name_tokens).is_some() {
        return HitKind::Name;
    }
    if d.file_count >= 1 && longest_common(&f.tokens, &d.confirmed_tokens).is_some() {
        return HitKind::Family;
    }
    if d.file_count == 0 {
        return HitKind::Empty;
    }
    HitKind::None
}

/// 理由文案（四模板）。返回 `None` 的候选在 `rank` 里直接丢弃 ——
/// 这是「不存在无理由的推荐」（验收 4）的代码级保障。
pub fn reason(f: &FileFeature, d: &DirFeature, hit: HitKind) -> Option<String> {
    match hit {
        HitKind::Name => {
            let t = longest_common(&f.tokens, &d.name_tokens)?;
            Some(format!("文件名含「{t}」，与「{}」匹配", d.dir_name))
        }
        HitKind::Substring => Some(format!(
            "文件名包含「{}」，同类已有 {} 份",
            d.dir_name, d.file_count
        )),
        HitKind::Family => {
            let t = longest_common(&f.tokens, &d.confirmed_tokens)?;
            Some(format!(
                "与「{}」下 {} 份文件同类（都含「{t}」）",
                d.dir_name, d.file_count
            ))
        }
        HitKind::Empty => Some(format!("「{}」还是空的，这是第一份", d.dir_name)),
        HitKind::None => None,
    }
}

/// 排序后的候选：(dir_id, score, reason)，分数降序 → 目录内文件数降序 → dir_id 升序
pub fn rank(f: &FileFeature, dirs: &[(i64, DirFeature)]) -> Vec<(i64, f64, String)> {
    let mut rows: Vec<(i64, f64, String, usize)> = Vec::new();
    for (id, d) in dirs {
        let s = score(f, d);
        if s <= 0.0 {
            continue;
        }
        let hit = detect_hit(f, d);
        if let Some(r) = reason(f, d, hit) {
            rows.push((*id, s, r, d.file_count));
        }
    }
    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.3.cmp(&a.3))
            .then(a.0.cmp(&b.0))
    });
    rows.into_iter().map(|(id, s, r, _)| (id, s, r)).collect()
}

/// 给一份文件挑候选：返回 (候选列表, needsYou)
///
/// - 目录数 0：空列表 + needsYou（前端切「新建目录」空态，验收 11）
/// - 目录数 1：不作主推，唯一目录作为备选 + needsYou
/// - 其余：主推 = 第一名（score ≥ UNCERTAIN_MIN），备选 = 第 2/3 名（score ≥ ALT_MIN，最多 2 个）
pub fn candidates(
    f: &FileFeature,
    dirs: &[(i64, DirFeature)],
    rel_paths: &HashMap<i64, String>,
) -> (Vec<Candidate>, bool) {
    if dirs.is_empty() {
        return (Vec::new(), true);
    }

    if dirs.len() == 1 {
        let (id, d) = &dirs[0];
        let s = score(f, d);
        let hit = detect_hit(f, d);
        // 唯一目录必须给出一个能读的理由，否则用户看到的是空白卡片。
        // 这与「无理由作废」不冲突：作废是为了不在多目录里瞎猜，
        // 单目录时猜测的代价极低（用户按一下回车就否掉了）。
        let r = reason(f, d, hit).unwrap_or_else(|| format!("「{}」是当前唯一目录", d.dir_name));
        return (
            vec![Candidate {
                dir_id: *id,
                rel_path: rel_paths.get(id).cloned().unwrap_or_else(|| d.dir_name.clone()),
                score: s,
                reason: r,
                uncertain: true,
            }],
            true,
        );
    }

    let ranked = rank(f, dirs);
    let Some((best_id, best_score, best_reason)) = ranked.first().cloned() else {
        return (Vec::new(), true);
    };
    if best_score < UNCERTAIN_MIN {
        return (Vec::new(), true);
    }

    let mut out = vec![Candidate {
        dir_id: best_id,
        rel_path: rel_paths
            .get(&best_id)
            .cloned()
            .unwrap_or_else(|| best_id.to_string()),
        score: best_score,
        reason: best_reason,
        uncertain: best_score < PRIMARY,
    }];
    for (id, s, r) in ranked.iter().skip(1).take(MAX_ALTS) {
        if *s < ALT_MIN {
            break;
        }
        out.push(Candidate {
            dir_id: *id,
            rel_path: rel_paths.get(id).cloned().unwrap_or_else(|| id.to_string()),
            score: *s,
            reason: r.clone(),
            uncertain: true,
        });
    }
    (out, false)
}
