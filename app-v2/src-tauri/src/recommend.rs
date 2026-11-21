//! 推荐打分与理由（纯函数：不读文件、不碰库、不联网）
//!
//! 依据详设 §5.4 + P2 计划 §3.1。四条不变量（比权重数字更重要）：
//!
//! 1. **权重和恒为 1.0**。缺某一项时按比例重分配，不是置 0 ——
//!    否则无标签文件的分数被系统性压低，等于歧视新文件。
//! 2. **`nameMatch` 的分母只有目录侧**（目录各段名），绝不并上已确认文件的词元。
//!    P1 的 `dir_union()` 把两者并成一个集合，目录用得越久分母越大，
//!    同一份文件的 name 分数会自己往下掉 —— 那正是「越用越不准」的隐藏 bug。
//!    已确认文件的词元归 `familyMatch` 管，两路各司其职。
//! 3. **`familyMatch` = 与该目录已确认文件的名称相似度 top3 平均**（不是最大，
//!    避免一个近重名文件把整目录带上去）。
//! 4. **`kindBias` 向 0.5 收缩**：`(该类型份数 + 2) / (目录总份数 + 4)`。
//!
//! 另有一条 P2 新增（计划 §8 偏差 4 的记录对象）：目录在层级里，
//! 「目录名」= **完整相对路径的每一段**。只取最后一段会漏掉真实线索 ——
//! 实测 38 份留一集里 11 条漏判都是这一类：文件名词对得上祖先段
//! （`智能工厂项目`），却因为目录名只叫 `验收` 而拿 0 分，
//! 反过来名字叫 `汇报` 的浅目录白拿 1.00。

use crate::model::Candidate;
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct FileFeature {
    pub tokens: Vec<String>,
    pub stem: String,
    pub kind: String,
    pub mtime: i64,
    /// 该文件的 topic 标签（P2 深度标签产出；没有就是空）
    pub topic_tags: Vec<String>,
}

/// 目录路径上的一段（`信科` / `智能工厂项目` / `验收` …）
#[derive(Clone, Debug, Default)]
pub struct DirSeg {
    pub name: String,
    pub tokens: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DirFeature {
    /// 从根到叶子，每段的（名字, 词元）
    pub segments: Vec<DirSeg>,
    pub dir_name: String,
    /// 已确认文件的词元**并集**：只给理由文案用，不参与 name 打分
    pub confirmed_tokens: Vec<String>,
    /// 每个已确认文件各自的词元：familyMatch 用（top3 平均）
    pub confirmed_files: Vec<Vec<String>>,
    pub file_count: usize,
    /// 类型分布（word/excel/ppt/pdf/…）
    pub kind_hist: HashMap<String, i64>,
    /// 该目录已确认文件的 topic 标签
    pub topic_tags: Vec<String>,
}

impl DirFeature {
    /// 目录自身（叶子）那一段的词元
    pub fn name_tokens(&self) -> &[String] {
        self.segments
            .last()
            .map(|s| s.tokens.as_slice())
            .unwrap_or(&[])
    }
}

/// 四路权重（设计文档 §5.4 原值：0.40 / 0.35 / 0.20 / 0.05）
///
/// **P3 T0.2 抽检结论：维持不动，并把「降权会怎样」也量了一遍**（2026-11-21）：
///
/// 一致性抽检（`test/tag-consistency.json`，真实库随机 20 份 deep、已确认）：
/// 一致 **9** / 沾边 5 / 无关 0 / 不适用 6（该目录只有它一份带标签，无从比较）。
/// 落在计划定的 8~11 区间 → 维持 `wt=0.35`，理由：
///
/// * **0 条「无关」**：标签从没把人往错目录带，只是命中率低；
/// * 5 条「沾边」全是同一个现象 —— 同一件事被模型写成不同词
///   （`数字化转型`/`数智化转型`、`系统上线`/`系统实施`、`项目方案`/`专题方案`），
///   而 `tag_match` 是**词面覆盖率**，对同义词零容忍；
/// * 该修的是**词表收敛**（T1.5 的改名/合并），不是下调权重。
///
/// 对照实验（同留一集，`test/eval-wt*.json`）：把 wt 降到 0.20 后四格三降一平 ——
/// | 配置 | 全集 top1 | 全集 top3 | 标签子集 top1 | 标签子集 top3 |
/// |---|---|---|---|---|
/// | wt=0.35（现行） | **41.2%** | 62.4% | **34.5%** | **65.5%** |
/// | wt=0.20（实验） | 38.8% | 62.4% | 27.6% | 62.1% |
/// 所以下调被数据否掉了，已回退。词表收敛之后再谈调权重。
pub const W_NAME: f64 = 0.40;
pub const W_TAG: f64 = 0.35;
pub const W_FAMILY: f64 = 0.20;
pub const W_KIND: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    pub name: f64,
    pub tag: f64,
    pub family: f64,
    pub kind: f64,
}

/// 按可用项重分配权重（不变量 1）
///
/// 缺项时不是把那一路置 0，而是把它的份额**按剩余项的原始比例**摊过去：
/// 无标签 → 0.40 : 0.20 : 0.05 归一 = 0.615 / 0.308 / 0.077。
/// 这样「有没有标签」只改变各路的相对话语权，不改变总权重。
pub fn weights(has_tag: bool, has_family: bool, has_kind: bool) -> Weights {
    let name = W_NAME;
    let tag = if has_tag { W_TAG } else { 0.0 };
    let family = if has_family { W_FAMILY } else { 0.0 };
    let kind = if has_kind { W_KIND } else { 0.0 };
    let sum = name + tag + family + kind;
    // name 恒可用，sum 不可能为 0；仍然写防御分支，免得将来加「无 name」场景时除零
    if sum <= 0.0 {
        return Weights {
            name: 1.0,
            tag: 0.0,
            family: 0.0,
            kind: 0.0,
        };
    }
    Weights {
        name: name / sum,
        tag: tag / sum,
        family: family / sum,
        kind: kind / sum,
    }
}

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

/// `needle` 被 `hay` 覆盖的比例（分母取 hay）
///
/// 词元表都很短（一份文件名 10 来个、一段目录名 2~8 个），
/// 建 HashSet 的分配比线性比较更贵 —— 实测 455×50 从 398ms 降到预算内就是这个改动，
/// 所以阈值以上才走哈希。
fn coverage(needle: &[String], hay: &[String]) -> f64 {
    if needle.is_empty() || hay.is_empty() {
        return 0.0;
    }
    let inter = if hay.len() <= 32 {
        needle.iter().filter(|t| hay.contains(t)).count()
    } else {
        let hs: HashSet<&String> = hay.iter().collect();
        needle.iter().filter(|t| hs.contains(t)).count()
    };
    inter as f64 / needle.len() as f64
}

/// 单段命中分 = 覆盖率 × 特异性
///
/// 特异性 `n/(n+2)` 是 P2 加的：没有它，名字只有两个字的浅目录
/// （`汇报` / `验收`）命中一个词元就拿 1.00，比真实目录里
/// 6 个字的 `智能工厂项目` 命中 5/6 还高 —— 实测就是这么错的。
/// 段名越长，被命中越说明「这份文件确实属于这一类」。
fn seg_score(seg_tokens: &[String], file_tokens: &[String]) -> f64 {
    let c = coverage(seg_tokens, file_tokens);
    if c <= 0.0 {
        return 0.0;
    }
    let n = seg_tokens.len() as f64;
    c * (n / (n + 2.0))
}

/// 文件名 vs 目录：取路径上**各段命中的最大值**
pub fn name_match(f: &FileFeature, d: &DirFeature) -> f64 {
    if f.tokens.is_empty() || d.segments.is_empty() {
        return 0.0;
    }
    let mut m: f64 = 0.0;
    for seg in &d.segments {
        m = m.max(seg_score(&seg.tokens, &f.tokens));
    }
    let dn = d.dir_name.chars().count();
    if dn >= 3 && f.stem.contains(&d.dir_name) {
        m = (m * 1.3).min(1.0);
    }
    m
}

/// 标签覆盖率：分母取目录侧（与 name 同口径）
pub fn tag_match(f: &FileFeature, d: &DirFeature) -> f64 {
    coverage(&d.topic_tags, &f.topic_tags)
}

/// 同族相似度：与每个已确认文件名比，取 **top3 平均**
///
/// 分母取已确认文件那一侧的词元数 —— 与 name_match 一致（分母是参照物）。
/// 空目录返回 0，且 `has_family=false`，交给权重重分配。
pub fn family_match(f: &FileFeature, d: &DirFeature) -> (f64, bool) {
    if d.confirmed_files.is_empty() || f.tokens.is_empty() {
        return (0.0, false);
    }
    let mut scores: Vec<f64> = d
        .confirmed_files
        .iter()
        .map(|cf| coverage(cf, &f.tokens))
        .collect();
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let take = scores.len().min(3);
    let sum: f64 = scores.iter().take(take).sum();
    let avg = sum / take as f64;
    // 全无交集 = 没有同族证据，等同于该项不可用（交给重分配），
    // 而不是「有证据说明不像」—— 后者会把整个目录的分数一起压下去
    (avg, avg > 0.0)
}

/// 类型偏置：向 0.5 收缩
pub fn kind_bias(f: &FileFeature, d: &DirFeature) -> (f64, bool) {
    let total: i64 = d.kind_hist.values().sum();
    if total == 0 {
        return (0.5, false);
    }
    let hit = d.kind_hist.get(&f.kind).copied().unwrap_or(0);
    ((hit + 2) as f64 / (total + 4) as f64, true)
}

/// 四路各自的原始分 + 可用性
#[derive(Clone, Copy, Debug)]
pub struct Parts {
    pub name: f64,
    pub tag: f64,
    pub family: f64,
    pub kind: f64,
    pub has_tag: bool,
    pub has_family: bool,
    pub has_kind: bool,
}

pub fn parts(f: &FileFeature, d: &DirFeature) -> Parts {
    let (family, has_family) = family_match(f, d);
    let (kind, has_kind) = kind_bias(f, d);
    Parts {
        name: name_match(f, d),
        tag: tag_match(f, d),
        family,
        kind,
        has_tag: !d.topic_tags.is_empty() && !f.topic_tags.is_empty(),
        has_family,
        has_kind,
    }
}

pub fn score(f: &FileFeature, d: &DirFeature) -> f64 {
    score_of(&parts(f, d))
}

fn score_of(p: &Parts) -> f64 {
    let w = weights(p.has_tag, p.has_family, p.has_kind);
    p.name * w.name + p.tag * w.tag + p.family * w.family + p.kind * w.kind
}

// ---------------------------------------------------------------- 阈值（T1.7）
//
// 2026-11-20 用留一集（38 份 / 47 目录，真实索引库）对 UNCERTAIN_MIN 做 0.05 步长网格。
// **排名与阈值无关**，阈值只决定「给不给候选」，所以判据是「覆盖率 × 覆盖率内的 top1」：
//
//   UNCERTAIN_MIN | 有候选 | 无候选 | 有候选时 top1
//   ------------- | ------ | ------ | -------------
//   0.15          | 97.4%  |  2.6%  | 37.8%
//   0.20          | 92.1%  |  7.9%  | 40.0%   ← 选它
//   0.25          | 86.8%  | 13.2%  | 36.4%
//   0.30          | 86.8%  | 13.2%  | 36.4%
//   0.35          | 84.2%  | 15.8%  | 34.4%
//   0.40          | 71.1%  | 28.9%  | 37.0%   （无候选已破 25% 上限，淘汰）
//
// 0.20 同时是覆盖率与「给出去的候选里对不对」的双高点，且满足计划「无候选 ≤25%」；
// 0.15 覆盖率只多 5pp，top1 反而低 2.2pp —— 多给出去的那两份是错的。
// 与 P1 的 0.20 数值相同，但**分数刻度已经变了**（name 有特异性折扣、缺项按比例重分配），
// 所以这是重新校准后的巧合，不是照抄。
pub const PRIMARY: f64 = 0.30;
/// 低于这个分不给候选，直接 needsYou（空白卡片 + 待你指定）
pub const UNCERTAIN_MIN: f64 = 0.20;
/// 备选（第 2、3 名）最低分
pub const ALT_MIN: f64 = 0.20;
/// 备选最多给 2 个（主推之外）
pub const MAX_ALTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Name,
    Substring,
    Tag,
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

fn common_list(a: &[String], b: &[String]) -> Vec<String> {
    a.iter()
        .filter(|t| t.chars().count() >= 2 && b.contains(t))
        .cloned()
        .collect()
}

fn hit_of(f: &FileFeature, d: &DirFeature, p: &Parts) -> HitKind {
    let dn = d.dir_name.chars().count();
    if dn >= 3 && f.stem.contains(&d.dir_name) {
        return HitKind::Substring;
    }
    // 名字这一路：任一段有 ≥2 字的共同词元才算「说得清」
    let any_name = d
        .segments
        .iter()
        .any(|s| longest_common(&f.tokens, &s.tokens).is_some());
    if any_name {
        return HitKind::Name;
    }
    if p.tag > 0.0 && !common_list(&d.topic_tags, &f.topic_tags).is_empty() {
        return HitKind::Tag;
    }
    if d.file_count >= 1 && p.family > 0.0 {
        return HitKind::Family;
    }
    if d.file_count == 0 {
        return HitKind::Empty;
    }
    HitKind::None
}

pub fn detect_hit(f: &FileFeature, d: &DirFeature) -> HitKind {
    hit_of(f, d, &parts(f, d))
}

/// 理由文案（五模板）。返回 `None` 的候选在 `rank` 里直接丢弃 ——
/// 这是「不存在无理由的推荐」（验收 4）的代码级保障。
pub fn reason(f: &FileFeature, d: &DirFeature, hit: HitKind) -> Option<String> {
    match hit {
        HitKind::Name => {
            // 命中的是叶子段，还是某个祖先段？文案要说清，不然用户看不懂为什么推荐它
            let mut best: Option<(f64, &DirSeg)> = None;
            for seg in &d.segments {
                let s = seg_score(&seg.tokens, &f.tokens);
                if s > 0.0 && best.map(|(b, _)| s > b).unwrap_or(true) {
                    best = Some((s, seg));
                }
            }
            let (_, seg) = best?;
            let t = longest_common(&f.tokens, &seg.tokens)?;
            if seg.name == d.dir_name {
                Some(format!("文件名含「{t}」，与「{}」匹配", d.dir_name))
            } else {
                Some(format!(
                    "文件名含「{t}」，与上级目录「{}」一致",
                    seg.name
                ))
            }
        }
        HitKind::Substring => Some(format!(
            "文件名包含「{}」，同类已有 {} 份",
            d.dir_name, d.file_count
        )),
        HitKind::Tag => {
            let c = common_list(&d.topic_tags, &f.topic_tags);
            if c.is_empty() {
                return None;
            }
            let shown: Vec<String> = c.iter().take(2).cloned().collect();
            Some(format!(
                "标签「{}」与「{}」下 {} 份文件一致",
                shown.join("、"),
                d.dir_name,
                d.file_count
            ))
        }
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
        let p = parts(f, d);
        let s = score_of(&p);
        if s <= 0.0 {
            continue;
        }
        let hit = hit_of(f, d, &p);
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
