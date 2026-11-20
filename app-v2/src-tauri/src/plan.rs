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

/// 文件名清洗：归档前顺手改名用
///
/// 2026-11-20 实测反馈：「有些文件我想在确认迁移后顺便改个名字，让他更明确」。
/// 规则在目录名那套之上多一件事 —— **扩展名不能丢也不能叠**：
/// - 输入「周报」→ `周报.pptx`（补回原扩展名，否则落盘就变成没有类型的文件）
/// - 输入「周报.pptx」→ `周报.pptx`（不叠成 `周报.pptx.pptx`）
/// - 输入「周报.final.pptx」→ `周报.final.pptx`（中间的点是名字的一部分，保留）
pub fn clean_file_name(input: &str, orig_ext: &str) -> DirNamePreview {
    let raw = input.trim();
    if raw.is_empty() {
        return DirNamePreview {
            cleaned: String::new(),
            valid: false,
            reason: Some("文件名不能为空".into()),
        };
    }
    let p = clean_dir_name(raw);
    if !p.valid {
        return p;
    }
    let ext = orig_ext.trim().trim_start_matches('.').to_lowercase();
    if ext.is_empty() {
        return p;
    }
    if p.cleaned.to_lowercase().ends_with(&format!(".{ext}")) {
        return p;
    }
    DirNamePreview {
        cleaned: format!("{}.{ext}", p.cleaned),
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

// ---------------------------------------------------------------- 迁移计划（T7.1）

use std::collections::HashMap;
use std::fs::Metadata;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

/// 文件系统 mtime → 毫秒（扫描与迁移必须用同一个换算，否则每份都会被判「改过」）
pub fn mtime_ms(meta: &Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 迁移计划的一条输入 —— 由 `index.rs` 从库里读出，本模块不碰 SQL
#[derive(Clone, Debug, PartialEq)]
pub struct PlanInput {
    pub file_id: i64,
    pub name: String,
    pub src_path: String,
    pub dir_id: i64,
    pub dir_rel_path: String,
    /// 源区相对路径：隔离区按它保留目录结构（C3）
    pub rel_in_source: String,
    /// 标注时落库的值，用来判断「标注后有没有被改」
    pub size: i64,
    pub mtime: i64,
}

/// 一条可执行的迁移计划
#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub file_id: i64,
    pub name: String,
    pub src_path: String,
    /// 预览时算好的目标路径（执行时若已存在会再错开，绝不覆盖）
    pub dst_path: String,
    pub dir_id: i64,
    pub dir_rel_path: String,
    pub rel_in_source: String,
    pub size: i64,
    pub mtime: i64,
    /// 归档目录磁盘上还不存在 → 前端显示「（新建目录）」
    pub is_new_dir: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NameConflict {
    pub file_id: i64,
    /// 错开之后的名字（如 `报告_2.docx`）
    pub target_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChangedFile {
    pub file_id: i64,
    pub name: String,
    /// mtime | size | missing
    pub why: String,
}

#[derive(Clone, Debug, Default)]
pub struct BuildResult {
    pub plans: Vec<PlanItem>,
    pub conflicts: Vec<NameConflict>,
    pub changed: Vec<ChangedFile>,
}

/// `rel_path` （统一 `/` 分隔）→ 归档根下的绝对目录
pub fn archive_dir_path(archive_root: &Path, rel_path: &str) -> PathBuf {
    let mut cur = archive_root.to_path_buf();
    for seg in rel_path.split(['/', '\\']) {
        let seg = seg.trim();
        if seg.is_empty() || seg == "." || seg == ".." {
            continue;
        }
        cur = cur.join(seg);
    }
    cur
}

/// 隔离区路径：`<归档根>/.filebutler/quarantine/<迁移id>/<源区相对路径>`
pub fn quarantine_path(archive_root: &Path, migration_id: i64, rel_in_source: &str) -> PathBuf {
    archive_dir_path(
        archive_root,
        &format!(".filebutler/quarantine/{migration_id}/{rel_in_source}"),
    )
}

/// 生成迁移计划。
///
/// 排序固定为「目录 → 文件名 → id」：同名错开的 `_2/_3` 必须每次都一样，
/// 否则预览看到的和真搬进去的不是同一个名字。
pub fn build_plans(inputs: &[PlanInput], archive_root: &Path) -> BuildResult {
    let mut sorted: Vec<&PlanInput> = inputs.iter().collect();
    sorted.sort_by(|a, b| {
        a.dir_rel_path
            .cmp(&b.dir_rel_path)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.file_id.cmp(&b.file_id))
    });

    let mut out = BuildResult::default();
    // key = 小写目录 + 0 + 小写文件名。Windows 不分大小写，计划内部也得先查重，
    // 否则同一批里两份 `报告.docx` 会拿到同一个名字（磁盘上都还不存在，查不出来）
    let mut used: HashMap<String, ()> = HashMap::new();
    let mut new_dir_cache: HashMap<String, bool> = HashMap::new();

    for inp in sorted {
        let src = Path::new(&inp.src_path);
        let meta = match std::fs::metadata(src) {
            Ok(m) => m,
            Err(_) => {
                out.changed.push(ChangedFile {
                    file_id: inp.file_id,
                    name: inp.name.clone(),
                    why: "missing".into(),
                });
                continue;
            }
        };
        if meta.len() as i64 != inp.size {
            out.changed.push(ChangedFile {
                file_id: inp.file_id,
                name: inp.name.clone(),
                why: "size".into(),
            });
            continue;
        }
        if mtime_ms(&meta) != inp.mtime {
            out.changed.push(ChangedFile {
                file_id: inp.file_id,
                name: inp.name.clone(),
                why: "mtime".into(),
            });
            continue;
        }

        let dir_abs = archive_dir_path(archive_root, &inp.dir_rel_path);
        let is_new_dir = *new_dir_cache
            .entry(inp.dir_rel_path.clone())
            .or_insert_with(|| !dir_abs.exists());

        let mut candidate = inp.name.clone();
        let mut conflicted = false;
        if !inp.dir_rel_path.is_empty() {
            let (stem, ext) = split_ext(&inp.name);
            let mut n = 2u32;
            loop {
                let key = format!(
                    "{}\u{0}{}",
                    inp.dir_rel_path.to_lowercase(),
                    candidate.to_lowercase()
                );
                if used.contains_key(&key) || dir_abs.join(&candidate).exists() {
                    candidate = format!("{stem}_{n}{ext}");
                    n += 1;
                    conflicted = true;
                    if n > 999 {
                        candidate = format!("{stem}_{}{ext}", crate::index::now_ms());
                        break;
                    }
                } else {
                    break;
                }
            }
            used.insert(
                format!(
                    "{}\u{0}{}",
                    inp.dir_rel_path.to_lowercase(),
                    candidate.to_lowercase()
                ),
                (),
            );
        }
        if conflicted {
            out.conflicts.push(NameConflict {
                file_id: inp.file_id,
                target_name: candidate.clone(),
            });
        }

        out.plans.push(PlanItem {
            file_id: inp.file_id,
            name: inp.name.clone(),
            src_path: inp.src_path.clone(),
            dst_path: dir_abs
                .join(&candidate)
                .to_string_lossy()
                .to_string(),
            dir_id: inp.dir_id,
            dir_rel_path: inp.dir_rel_path.clone(),
            rel_in_source: inp.rel_in_source.clone(),
            size: inp.size,
            mtime: inp.mtime,
            is_new_dir,
        });
    }

    out
}
