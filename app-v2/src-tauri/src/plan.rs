//! 目录名清洗 / 同名加序号（纯函数）
//!
//! 依据详设 §7.3。TS 侧 `src/core/dirname.ts` 必须与本文件逐字一致，
//! 两侧共跑 `test/fixtures/dirname-cases.json`（评审红线 2）。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 目录名清洗结果（IPC 直接回传前端）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirNamePreview {
    pub cleaned: String,
    pub valid: bool,
    pub reason: Option<String>,
}

/// 单段目录名清洗（不含 `/`，路径拆分在 `clean_path` 里做）
pub fn clean_dir_name(input: &str) -> DirNamePreview {
    let original = input;

    // 1. 非法字符与控制字符 → _
    let mut s: String = input
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();

    // 2. 首尾空格 + 尾部点号循环去除 + 首部点号去除
    s = s.trim().to_string();
    while s.ends_with('.') {
        s.pop();
    }
    while s.starts_with('.') {
        s.remove(0);
    }
    s = s.trim().to_string();

    // 3. 连续 _ 折叠
    let mut folded = String::with_capacity(s.len());
    let mut last_us = false;
    for c in s.chars() {
        if c == '_' {
            if last_us {
                continue;
            }
            last_us = true;
        } else {
            last_us = false;
        }
        folded.push(c);
    }
    let mut s = folded;

    if s.is_empty() {
        return DirNamePreview {
            cleaned: String::new(),
            valid: false,
            reason: Some("目录名不能为空".into()),
        };
    }

    // 4. 超长截断：前 51 字符 + _ + md5(清洗后全名) 前 8 位
    if s.chars().count() > 60 {
        let head: String = s.chars().take(51).collect();
        let hash = format!("{:x}", md5::compute(s.as_bytes()));
        s = format!("{head}_{}", &hash[..8]);
    }

    // 5. Windows 保留名 → 前置 _
    let stem_upper = match s.rfind('.') {
        Some(i) if i > 0 => s[..i].to_uppercase(),
        _ => s.to_uppercase(),
    };
    if is_reserved(&stem_upper) {
        s = format!("_{s}");
    }

    let _ = original;
    DirNamePreview {
        cleaned: s,
        valid: true,
        reason: None,
    }
}

fn is_reserved(upper_stem: &str) -> bool {
    const FIXED: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    if FIXED.contains(&upper_stem) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper_stem.strip_prefix(prefix) {
            if rest.len() == 1 && ('1'..='9').contains(&rest.chars().next().unwrap()) {
                return true;
            }
        }
    }
    false
}

/// 多段相对路径清洗：`父/子` → 逐段清洗后用 `/` 拼回
pub fn clean_path(input: &str) -> DirNamePreview {
    let raw = input.replace('\\', "/");
    let mut parts: Vec<String> = Vec::new();
    for seg in raw.split('/') {
        if seg.trim().is_empty() {
            continue;
        }
        let p = clean_dir_name(seg);
        if !p.valid {
            return p;
        }
        parts.push(p.cleaned);
    }
    if parts.is_empty() {
        return DirNamePreview {
            cleaned: String::new(),
            valid: false,
            reason: Some("目录名不能为空".into()),
        };
    }
    DirNamePreview {
        cleaned: parts.join("/"),
        valid: true,
        reason: None,
    }
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// 同目录已存在则 `_2`、`_3` 递增（起始 2）；仅同扩展名冲突
pub fn unique_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    let mut i = 2u32;
    loop {
        let cand = format!("{stem}_{i}{ext}");
        if !dir.join(&cand).exists() {
            return cand;
        }
        i += 1;
        if i > 999 {
            return format!("{stem}_{}{ext}", crate::index::now_ms());
        }
    }
}
