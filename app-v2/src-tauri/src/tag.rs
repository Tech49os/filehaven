//! 分词 / 停用词 / kind 归并 / 基础标签（纯函数，不读文件、不碰库）
//!
//! 依据详设 §7.1 §7.2。
//!
//! 关于 §7.1 规则 4 与实际期望的两处冲突，已按工程判断记录：
//! - 规则 4 只说「长度 ≥4 的中文段产出 2-gram + 整段」，那么 `安全检查通知` 拆不出
//!   `安全检查`（3 字）这种整词，`04生产工艺设备…` 也拆不出 `硬件设备`（4 字）。
//!   整词要么靠词典，要么靠 3/4-gram；引词典会把 P1 拖成词库维护工程，3/4-gram 会让
//!   平均 token 数从 8 涨到 50+。所以这里**严格按规则 4 实现**：整段 + 2-gram。
//!   推荐是双向对称分词（文件与目录名走同一个函数），2-gram 交集足以匹配，
//!   `生产工艺` 与 `硬件设备` 会以 `生产`/`工艺`/`硬件`/`设备` 的形式两边同时出现。
//! - 因此 T2.1 的样例断言按「规则 4 的真实产出」写，不按理想整词写（见 tests.rs）。

/// 停用词：版本/客套/流程类噪声，与业务无关
pub const STOPWORDS: &[&str] = &[
    "附件", "副本", "扫描", "最终", "最新", "定稿", "初稿", "初版", "草稿", "修改", "新建",
    "文档", "文件", "资料", "v1", "v2", "v3", "完", "版", "终版", "打印", "盖章",
    // 文件名噪声：相机/截图默认名，455 份语料里成片出现，留着会让整类图片互相似
    "img", "dsc", "dscn", "dcim", "未命名", "无标题",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Han,
    Digit,
    Letter,
}

fn class_of(c: char) -> Option<Class> {
    if ('\u{4e00}'..='\u{9fa5}').contains(&c) {
        Some(Class::Han)
    } else if c.is_ascii_digit() {
        Some(Class::Digit)
    } else if c.is_ascii_alphabetic() {
        Some(Class::Letter)
    } else {
        None
    }
}

/// 全角转半角 + 大写转小写
fn normalize(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let c = if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
        } else if c == '\u{3000}' {
            ' '
        } else {
            c
        };
        for l in c.to_lowercase() {
            out.push(l);
        }
    }
    out
}

/// 切成「同类字符连续段」的序列：中文段 / 数字段 / 字母段
fn segments(chars: &[char]) -> Vec<(Class, String)> {
    let mut out: Vec<(Class, String)> = Vec::new();
    let mut cur: Option<(Class, String)> = None;
    for &c in chars {
        match class_of(c) {
            None => {
                if let Some(s) = cur.take() {
                    out.push(s);
                }
            }
            Some(cl) => match cur {
                Some((c0, ref mut buf)) if c0 == cl => buf.push(c),
                _ => {
                    if let Some(s) = cur.take() {
                        out.push(s);
                    }
                    cur = Some((cl, c.to_string()));
                }
            },
        }
    }
    if let Some(s) = cur.take() {
        out.push(s);
    }
    out
}

/// 停用词判定：
/// - 多字停用词按「包含」过滤 —— 否则 `初版草稿` 这种整段会因为不是精确停用词而留下来
/// - 单字停用词（`版`/`完`）只做精确匹配 —— 否则 `版本管理` 会被误杀
fn is_stopword(t: &str) -> bool {
    STOPWORDS.iter().any(|w| {
        if w.chars().count() >= 2 {
            t.contains(w)
        } else {
            t == *w
        }
    })
}

/// 文件名主名 → 词元列表（顺序稳定，已去重、已去停用词）
///
/// 停用词在**切段之前**就从整串里摘掉（换成空格分隔符），而不是只做整词比对。
/// 原因：`初版草稿` 这种段如果先切 2-gram，会留下 `版草` 这种纯噪声，
/// 单靠整词比对拦不住。先摘词再切段，`初版草稿` 直接变空段。
/// 单字停用词（`版`/`完`）不做这种摘除，否则 `版本管理` 会被连坐。
pub fn tokenize(stem: &str) -> Vec<String> {
    let mut s: String = normalize(stem).into_iter().collect();
    for w in STOPWORDS {
        if w.chars().count() >= 2 {
            s = s.replace(w, " ");
        }
    }

    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: String, out: &mut Vec<String>| {
        if t.is_empty() || is_stopword(&t) || out.contains(&t) {
            return;
        }
        out.push(t);
    };

    for (cl, seg) in segments(&chars) {
        match cl {
            Class::Digit => { /* 规则 3：纯数字段丢弃 */ }
            Class::Letter => {
                // 规则 5：英文整词，长度 <3 丢弃
                if seg.chars().count() >= 3 {
                    push(seg, &mut out);
                }
            }
            Class::Han => {
                let c: Vec<char> = seg.chars().collect();
                let n = c.len();
                // 规则 3：单汉字段丢弃
                if n == 1 {
                    continue;
                }
                if n <= 3 {
                    // 规则 4 后半：2~3 字直接保留整段
                    push(seg.clone(), &mut out);
                } else {
                    // 规则 4 前半：整段优先，2-gram 兜底
                    push(seg.clone(), &mut out);
                    for i in 0..n - 1 {
                        push(c[i..i + 2].iter().collect::<String>(), &mut out);
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- kind

/// 旧 9 类 → 新 6 类 + other（详设 §7.2 映射表）
pub fn normalize_kind(old: &str) -> &'static str {
    match old {
        "doc" | "word" => "word",
        "xls" | "excel" => "excel",
        "ppt" => "ppt",
        "pdf" => "pdf",
        "image" => "image",
        "drawio" | "diagram" => "diagram",
        "text" => "word",
        "code" | "other" => "other",
        _ => "other",
    }
}

/// 扫描时由扩展名直接归 6 类（与 `normalize_kind` 结果一致）
pub fn kind_of_ext(ext: &str) -> &'static str {
    let ext = ext.trim_start_matches('.').to_lowercase();
    match ext.as_str() {
        "doc" | "docx" | "wps" | "txt" | "md" | "log" | "rtf" => "word",
        "xls" | "xlsx" | "csv" | "et" => "excel",
        "ppt" | "pptx" | "dps" => "ppt",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif" | "tif" | "tiff" => "image",
        "drawio" => "diagram",
        _ => "other",
    }
}

/// 入池判定：非资料型/临时/空文件不入池（详设 §7.2）
pub fn should_pool(name: &str, kind: &str, size: u64) -> bool {
    if kind == "other" {
        return false;
    }
    let lower = name.to_lowercase();
    if name.starts_with("~$") || lower.ends_with(".lnk") {
        return false;
    }
    for ext in [".tmp", ".crdownload", ".part"] {
        if lower.ends_with(ext) {
            return false;
        }
    }
    if size == 0 {
        return false;
    }
    true
}

// ---------------------------------------------------------------- 基础标签

/// 1970-01-01 起的天数 → (年, 月, 日)，Howard Hinnant 算法
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// mtime(毫秒) → (年, 季度)。按 UTC 计算（年边界的 8 小时误差对粗粒度标签可接受）
pub fn year_quarter(mtime_ms: i64) -> (i64, u32) {
    let days = mtime_ms.div_euclid(86_400_000);
    let (y, m, _) = civil_from_days(days);
    (y, (m - 1) / 3 + 1)
}

/// 6 类 kind → 展示用中文类型名
pub fn type_name(kind: &str) -> &'static str {
    match kind {
        "word" => "Word",
        "excel" => "Excel",
        "ppt" => "PPT",
        "pdf" => "PDF",
        "image" => "图片",
        "diagram" => "draw.io",
        _ => "其他",
    }
}

/// 基础标签：type + time（年 / 年-季度）+ keyword（top 3）
/// 返回 (标签名, 标签类别, 置信度)
pub fn basic_tags(name: &str, kind: &str, mtime_ms: i64) -> Vec<(String, String, f64)> {
    let mut out: Vec<(String, String, f64)> = Vec::new();
    out.push((type_name(kind).to_string(), "type".to_string(), 1.0));

    let (y, q) = year_quarter(mtime_ms);
    out.push((y.to_string(), "time".to_string(), 1.0));
    if y > 1970 {
        out.push((format!("{y}-Q{q}"), "time".to_string(), 1.0));
    }

    let stem = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    for (i, t) in tokenize(stem).iter().take(3).enumerate() {
        out.push((t.clone(), "keyword".to_string(), 0.9 - 0.1 * i as f64));
    }
    out
}
