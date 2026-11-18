//! 三段式安全迁移（详设 §7.7）
//!
//! ① 复制 → ② SHA-256 校验 → ③ 源文件移入隔离区，逐文件执行。
//! 铁律：永不覆盖既有文件；校验不一致立即删副本、源文件不动；
//! 隔离区保留源目录相对结构（C3），撤销靠它按原路还原。
//!
//! 本模块只做文件系统，不碰 SQL、不碰 Tauri：落库与事件由 `migration.rs` / `main.rs` 负责，
//! 这样每个分支都能用临时目录单测。

use crate::error::FILE_CHANGED;
use crate::plan::{self, PlanItem};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

/// 隔离区布局由 `plan` 定义，这里转出去给调用方与测试用（一处定义，不许两处算）
pub use crate::plan::quarantine_path;

/// 进度阶段（与前端 `MigrateProgress.stage` 一致）
pub const STAGE_COPY: &str = "copy";
pub const STAGE_VERIFY: &str = "verify";
pub const STAGE_QUARANTINE: &str = "quarantine";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemResult {
    Ok,
    Failed,
}

/// 一份文件的迁移结果（对应 migration_items 一行）
#[derive(Clone, Debug)]
pub struct MigrateItem {
    pub file_id: i64,
    pub name: String,
    pub src_path: String,
    /// 实际落盘的目标路径（可能因同名而错开）
    pub dst_path: String,
    pub sha256: Option<String>,
    pub result: ItemResult,
    /// 失败的错误码（FILE_CHANGED / IO …），成功为 None
    pub code: Option<&'static str>,
    pub error: Option<String>,
    /// 源文件是否真的进了隔离区 —— 撤销的前提
    pub quarantined: bool,
    pub bytes: u64,
}

pub fn sha256_of(path: &Path) -> std::io::Result<String> {
    let f = File::open(path)?;
    let mut reader = BufReader::with_capacity(1 << 20, f);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 复制并保留 mtime（撤销/浏览都按时间排序，丢了时间戳等于丢信息）
fn copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    if let Ok(meta) = fs::metadata(src) {
        if let Ok(mtime) = meta.modified() {
            if let Ok(f) = fs::OpenOptions::new().write(true).open(dst) {
                let _ = f.set_modified(mtime);
            }
        }
    }
    Ok(())
}

/// 移动文件：先试 rename（同盘瞬间完成），跨盘再退化成「复制 + 校验 + 删源」
fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_file(src, dst)?;
    let a = sha256_of(src)?;
    let b = sha256_of(dst)?;
    if a != b {
        let _ = fs::remove_file(dst);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "跨盘搬移后哈希不一致，已回滚",
        ));
    }
    fs::remove_file(src)?;
    Ok(())
}

/// 执行前二次兜底：目标已存在就换名，绝不覆盖
fn resolve_dst(plan: &PlanItem) -> PathBuf {
    let desired = PathBuf::from(&plan.dst_path);
    if !desired.exists() {
        return desired;
    }
    let parent = desired.parent().unwrap_or_else(|| Path::new("."));
    let name = desired
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| plan.name.clone());
    parent.join(plan::unique_name(parent, &name))
}

/// 迁移一份文件（三段式，逐文件原子）
///
/// 调用方拿到结果后**立刻**写一个事务重：`migration_items` 一行 + 成功则 `status=migrated`。
pub fn execute_one(
    archive_root: &Path,
    migration_id: i64,
    plan: &PlanItem,
    stage: &mut dyn FnMut(&str),
) -> MigrateItem {
    let src = PathBuf::from(&plan.src_path);
    let mut item = MigrateItem {
        file_id: plan.file_id,
        name: plan.name.clone(),
        src_path: plan.src_path.clone(),
        dst_path: String::new(),
        sha256: None,
        result: ItemResult::Failed,
        code: None,
        error: None,
        quarantined: false,
        bytes: 0,
    };

    macro_rules! fail {
        ($code:expr, $msg:expr) => {{
            item.code = Some($code);
            item.error = Some(String::from($msg));
            return item;
        }};
    }

    // ---- ① 复制 ----
    stage(STAGE_COPY);
    let meta = match fs::metadata(&src) {
        Ok(m) => m,
        Err(_) => fail!(crate::error::IO, "源文件已不存在"),
    };
    // 迁移前 preview 已拦过，这里是二次兜底：万一预览之后又被改，宁可不搬
    if meta.len() as i64 != plan.size {
        fail!(
            FILE_CHANGED,
            format!(
                "标注之后文件体积变了（{} → {} 字节），请重新确认",
                plan.size,
                meta.len()
            )
        );
    }
    if plan::mtime_ms(&meta) != plan.mtime {
        fail!(
            FILE_CHANGED,
            "标注之后文件被修改过（修改时间变了），请重新确认"
        );
    }
    item.bytes = meta.len();

    let src_hash = match sha256_of(&src) {
        Ok(h) => h,
        Err(e) => fail!(crate::error::IO, format!("读取源文件失败：{e}")),
    };
    item.sha256 = Some(src_hash.clone());

    let dst = resolve_dst(plan);
    item.dst_path = dst.to_string_lossy().to_string();
    if let Err(e) = copy_file(&src, &dst) {
        let _ = fs::remove_file(&dst);
        fail!(crate::error::IO, format!("复制失败：{e}"));
    }

    // ---- ② 校验 ----
    stage(STAGE_VERIFY);
    let dst_hash = match sha256_of(&dst) {
        Ok(h) => h,
        Err(e) => {
            let _ = fs::remove_file(&dst);
            fail!(crate::error::IO, format!("校验读取失败：{e}"));
        }
    };
    if dst_hash != src_hash {
        let _ = fs::remove_file(&dst);
        fail!(
            crate::error::MIGRATE_FAILED,
            "哈希校验不一致，已删除副本并保留原文件"
        );
    }

    // ---- ③ 源文件进隔离区 ----
    stage(STAGE_QUARANTINE);
    let q = plan::quarantine_path(archive_root, migration_id, &plan.rel_in_source);
    if let Err(e) = move_file(&src, &q) {
        // 副本已校验通过，保留它；源文件还在原地 —— 记 failed，用户能再试一次
        item.result = ItemResult::Failed;
        item.code = Some(crate::error::MIGRATE_FAILED);
        item.error = Some(format!(
            "归档副本已就位，但源文件没能移入隔离区（{e}）；请手工处理原文件"
        ));
        return item;
    }

    item.result = ItemResult::Ok;
    item.quarantined = true;
    item
}

/// 撤销一份文件的结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoOutcome {
    Restored,
    Failed(String),
}

/// 隔离区里找不到按相对路径算出来的那一份时，按文件名再找一次（R8：用户手工挪过）
fn find_in_quarantine(root: &Path, name: &str, sha: Option<&str>) -> Option<PathBuf> {
    let mut by_name: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name().to_string_lossy() != name {
            continue;
        }
        if let Some(want) = sha {
            if sha256_of(entry.path()).map(|h| h == want).unwrap_or(false) {
                return Some(entry.path().to_path_buf());
            }
        }
        by_name.push(entry.path().to_path_buf());
    }
    // 名字对上了但哈希对不上的，只在唯一时兜底用 —— 否则宁可报失败
    if by_name.len() == 1 {
        return by_name.pop();
    }
    None
}

/// 撤销一份文件：隔离区 → 原路径，然后删掉归档副本
pub fn undo_one(
    archive_root: &Path,
    migration_id: i64,
    src_path: &str,
    dst_path: &str,
    sha: Option<&str>,
    rel_in_source: &str,
    stage: &mut dyn FnMut(&str),
) -> UndoOutcome {
    let src = PathBuf::from(src_path);
    let q_dir = plan::archive_dir_path(archive_root, &format!(".filebutler/quarantine/{migration_id}"));
    let expected = plan::quarantine_path(archive_root, migration_id, rel_in_source);

    stage(STAGE_COPY);
    let from = if expected.is_file() {
        expected
    } else {
        let name = src
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        match find_in_quarantine(&q_dir, &name, sha) {
            Some(p) => p,
            None => return UndoOutcome::Failed("隔离区找不到原件".into()),
        }
    };

    // 隔离区里的原件必须和迁移时记下的哈希一致，否则宁可不还原
    if let Some(want) = sha {
        match sha256_of(&from) {
            Ok(h) if h == want => {}
            Ok(_) => return UndoOutcome::Failed("隔离区原件与记录不一致，未还原".into()),
            Err(e) => return UndoOutcome::Failed(format!("读取隔离区原件失败：{e}")),
        }
    }

    if src.exists() {
        return UndoOutcome::Failed("原位置已经有同名文件，未还原（避免覆盖）".into());
    }

    stage(STAGE_VERIFY);
    if let Err(e) = move_file(&from, &src) {
        return UndoOutcome::Failed(format!("还原失败：{e}"));
    }

    // 原件回来了，归档副本就可以删了
    let dst = PathBuf::from(dst_path);
    if dst.is_file() {
        if let Err(e) = fs::remove_file(&dst) {
            return UndoOutcome::Failed(format!("原件已还原，但归档副本删不掉：{e}"));
        }
    }
    UndoOutcome::Restored
}
