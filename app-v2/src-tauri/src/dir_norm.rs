//! BUG-006 同语义目录归一化（纯函数）。
//!
//! 目标：把「`2026年9月`」与「`2026年/9月`」这类**层级写法不同但语义相同**的路径
//! 归一成同一个可比较签名；同时绝不抹掉层级顺序（`9月/2026年` ≠ `2026年/9月`）。
//!
//! 只做确定性规则，不碰数据库、不发事件、不调模型。模型复核只是二次确认（见 `dir_merge.rs`）。
//! 复用 `suggest_dirs::strip_version_suffix`，不另造版本后缀规则。

use crate::suggest_dirs::strip_version_suffix;

/// 括号包裹的版本/草稿词：`报告（最终版）` → `报告`。
/// 全角括号已在 `fold_width` 里折成半角，这里只认半角。
const BRACKET_VERSION_WORDS: &[&str] = &[
    "最终版", "最新版", "定稿", "终版", "初稿", "初版", "草稿", "副本", "打印版",
];

/// 全角 → 半角 + 空格归一（全角空格 → 普通空格，后续按空白统一剔除）。
fn fold_width(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if u == 0x3000 {
                ' '
            } else if (0xFF01..=0xFF5E).contains(&u) {
                char::from_u32(u - 0xFEE0).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// 剥掉尾部 `(最终版)` / `(草稿)` 这类括号词。`strip_version_suffix` 只认数字括号与裸后缀，
/// 这里补上括号写法；剥不动就原样返回。
fn strip_bracket_version(s: &str) -> String {
    if !s.ends_with(')') {
        return s.to_string();
    }
    if let Some(pos) = s.rfind('(') {
        let inner = s[pos + 1..s.len() - 1].trim();
        if BRACKET_VERSION_WORDS.iter().any(|w| *w == inner) {
            return s[..pos].trim_end_matches(['_', '-', ' ']).to_string();
        }
    }
    s.to_string()
}

/// 剥掉段内所有空白（含全角空格归一化后的普通空格），空格差异不影响语义。
fn strip_spaces(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 单段归一：全半角 → 去空白 → 括号版本词 → 版本/草稿后缀。
fn normalize_segment(seg: &str) -> Option<String> {
    let folded = fold_width(seg);
    let no_space = strip_spaces(&folded);
    let unbracketed = strip_bracket_version(&no_space);
    let clean = strip_version_suffix(&unbracketed);
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

/// 日期层级归一：`2026年9月` → `2026年` + `9月`（月去掉前导零）。
/// 只认「年 + 月」都在同一段的写法；`9月/2026年` 的两段顺序保持不变，
/// 因此不会与 `2026年/9月` 误判同义。
fn expand_date_segment(seg: String, out: &mut Vec<String>) {
    let chars: Vec<char> = seg.chars().collect();
    if let Some(yi) = seg.find('年') {
        let after = &seg[yi + '年'.len_utf8()..];
        if let Some(mi) = after.find('月') {
            let y = &seg[..yi];
            let m = &after[..mi];
            let valid = !y.is_empty()
                && y.chars().all(|c| c.is_ascii_digit())
                && y.chars().count() == 4
                && !m.is_empty()
                && m.chars().all(|c| c.is_ascii_digit())
                && m.chars().count() <= 2
                && mi + '月'.len_utf8() == after.len()
                && chars
                    .iter()
                    .all(|c| c.is_ascii_digit() || *c == '年' || *c == '月');
            if valid {
                let month: u32 = m.parse().unwrap_or(0);
                if (1..=12).contains(&month) {
                    out.push(format!("{y}年"));
                    out.push(format!("{month}月"));
                    return;
                }
            }
        }
    }
    out.push(seg);
}

/// 归一化后的层级段序列。保留顺序，`/` 是层级分隔符。
pub fn normalized_segments(rel_path: &str) -> Vec<String> {
    // 先整串折全半角、去空白，**再**按 `/` 切层级 —— 否则全角斜杠 `／` 会被当成段内字符，
    // `项目_草稿／2026年 9月` 就切不开，日期层级与版本后缀都归一不了。
    let folded = fold_width(rel_path);
    let no_space = strip_spaces(&folded);
    let mut out = Vec::new();
    for raw in no_space.split(['/', '\\']) {
        if let Some(seg) = normalize_segment(raw) {
            expand_date_segment(seg, &mut out);
        }
    }
    out
}

/// 可比较签名。签名相同才允许成组；签名里保留层级顺序。
pub fn signature(rel_path: &str) -> String {
    normalized_segments(rel_path).join("/")
}

/// 理由文本：归一化签名 + 原路径差异点。绝不输出「系统觉得你错了」这类不可解释的话。
pub fn difference_note(a: &str, b: &str) -> String {
    let sa = signature(a);
    let sb = signature(b);
    let flag = if sa == sb { "签名一致" } else { "签名不同" };
    format!("归一化签名「{sa}」↔「{sb}」（{flag}）；差异点：{a} ↔ {b}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_level_and_fullwidth_normalize() {
        assert_eq!(signature("2026年9月"), signature("2026年/9月"));
        assert_eq!(signature("项目_草稿／2026年 9月"), signature("项目/2026年9月"));
    }

    #[test]
    fn hierarchy_order_is_preserved() {
        assert_ne!(signature("2026年/9月"), signature("9月/2026年"));
        assert_ne!(signature("2026年9月"), signature("2026年10月"));
    }
}
