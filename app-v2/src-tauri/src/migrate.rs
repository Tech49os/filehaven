//! 三段式安全迁移：① 复制 → ② SHA-256 校验 → ③ 源文件入隔离区
//!
//! 铁律：永不覆盖既有文件；校验不一致立即回滚副本；源文件只移动不删除。

use crate::model::{MigrateProgress, MigrateReport, Plan, Record, UndoReport};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

fn sha256_of(path: &Path) -> std::io::Result<String> {
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

pub fn sanitize_segment(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    cleaned.trim().trim_end_matches('.').to_string()
}

/// 同名加序号，永不覆盖
fn unique_target(dir: &Path, name: &str) -> (PathBuf, bool) {
    let direct = dir.join(name);
    if !direct.exists() {
        return (direct, false);
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut i = 1;
    loop {
        let cand = dir.join(format!("{stem}_{i}{ext}"));
        if !cand.exists() {
            return (cand, true);
        }
        i += 1;
        if i > 500 {
            return (dir.join(format!("{stem}_{}{ext}", chrono_stamp())), true);
        }
    }
}

fn chrono_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn build_target_dir(archive_root: &Path, target_dir: &str) -> PathBuf {
    let mut cur = archive_root.to_path_buf();
    for seg in target_dir.split(['\\', '/']).filter(|s| !s.is_empty()) {
        let safe = sanitize_segment(seg);
        if safe.is_empty() || safe == "." || safe == ".." {
            continue;
        }
        cur = cur.join(safe);
    }
    cur
}

fn copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    // 保时间戳，便于后续按修改时间排序
    if let Ok(meta) = fs::metadata(src) {
        if let Ok(mtime) = meta.modified() {
            if let Ok(f) = fs::OpenOptions::new().write(true).open(dst) {
                let _ = f.set_modified(mtime);
            }
        }
    }
    Ok(())
}

fn emit_progress(app: Option<&AppHandle>, payload: MigrateProgress) {
    if let Some(a) = app {
        let _ = a.emit("migrate-progress", payload);
    }
}

/// 给「字节完全相同的副本」用的批量哈希。
///
/// 前端只把「同一个大小、不止一份」的文件送进来（真实数据里 455 份只有 26 份），
/// 所以这一步很便宜——不做全盘哈希。算不出来的文件直接跳过，不影响扫描结果。
#[tauri::command]
pub fn hash_files(paths: Vec<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in paths {
        let path = PathBuf::from(&p);
        if let Ok(h) = sha256_of(&path) {
            out.push((p, h));
        }
    }
    out
}

pub fn migrate(
    app: Option<&AppHandle>,
    archive_root: &str,
    batch_id: &str,
    plans: Vec<Plan>,
) -> Result<MigrateReport, String> {
    let started = Instant::now();
    let root = PathBuf::from(archive_root);
    fs::create_dir_all(&root).map_err(|e| format!("无法创建归档根目录：{e}"))?;

    let trash_dir = root.join(".filemanager").join("trash").join(sanitize_segment(batch_id));
    fs::create_dir_all(&trash_dir).map_err(|e| format!("无法创建隔离区：{e}"))?;

    let total = plans.len();
    let mut records: Vec<Record> = Vec::with_capacity(total);
    let mut migrated = 0usize;
    let mut failed = 0usize;
    let mut bytes = 0u64;

    for (idx, plan) in plans.iter().enumerate() {
        let src = PathBuf::from(&plan.source_path);
        let mut rec = Record {
            file_id: plan.file_id.clone(),
            file_name: plan.file_name.clone(),
            source_path: plan.source_path.clone(),
            target_path: String::new(),
            status: "failed".into(),
            size: 0,
            error: None,
        };

        emit_progress(
            app,
            MigrateProgress {
                done: idx,
                total,
                name: plan.file_name.clone(),
                stage: "① 复制中".into(),
            },
        );

        macro_rules! fail {
            ($msg:expr) => {{
                rec.error = Some($msg.to_string());
                failed += 1;
                records.push(rec);
                continue;
            }};
        }

        if !src.is_file() {
            fail!("源文件已不存在");
        }
        let size = match fs::metadata(&src) {
            Ok(m) => m.len(),
            Err(e) => fail!(format!("读取源文件失败：{e}")),
        };
        rec.size = size;

        let src_hash = match sha256_of(&src) {
            Ok(h) => h,
            Err(e) => fail!(format!("计算源文件哈希失败：{e}")),
        };

        // ---- ① 复制 ----
        let target_dir = build_target_dir(&root, &plan.target_dir);
        if let Err(e) = fs::create_dir_all(&target_dir) {
            fail!(format!("创建目标目录失败：{e}"));
        }
        let (dest, _renamed) = unique_target(&target_dir, &plan.target_name);
        if let Err(e) = copy_file(&src, &dest) {
            let _ = fs::remove_file(&dest);
            fail!(format!("复制失败：{e}"));
        }
        rec.target_path = dest.to_string_lossy().to_string();

        // ---- ② 校验 ----
        emit_progress(
            app,
            MigrateProgress {
                done: idx,
                total,
                name: plan.file_name.clone(),
                stage: "② 校验完整性".into(),
            },
        );
        let dest_hash = match sha256_of(&dest) {
            Ok(h) => h,
            Err(e) => {
                let _ = fs::remove_file(&dest);
                fail!(format!("校验读取失败：{e}"));
            }
        };
        if dest_hash != src_hash {
            let _ = fs::remove_file(&dest);
            fail!("哈希校验不一致，已删除副本并保留原文件");
        }

        // ---- ③ 源文件入隔离区 ----
        emit_progress(
            app,
            MigrateProgress {
                done: idx,
                total,
                name: plan.file_name.clone(),
                stage: "③ 源文件移入隔离区".into(),
            },
        );
        let trash_name = format!(
            "{}_{}",
            sanitize_segment(&plan.file_id),
            sanitize_segment(&plan.file_name)
        );
        let trash_path = trash_dir.join(trash_name);
        let isolated = match copy_file(&src, &trash_path) {
            Ok(()) => match sha256_of(&trash_path) {
                Ok(h) if h == src_hash => match fs::remove_file(&src) {
                    Ok(()) => true,
                    Err(_) => false,
                },
                _ => false,
            },
            Err(_) => false,
        };

        if isolated {
            rec.status = "recycled".into();
        } else {
            // 隔离失败不算迁移失败：副本已就位且校验通过，源文件保留在原处
            rec.status = "verified".into();
            rec.error = Some("源文件已保留（未能移入隔离区）".into());
        }

        migrated += 1;
        bytes += size;
        records.push(rec);

        emit_progress(
            app,
            MigrateProgress {
                done: idx + 1,
                total,
                name: plan.file_name.clone(),
                stage: "完成".into(),
            },
        );
    }

    // 台账
    write_ledger(&root, batch_id, &records);

    Ok(MigrateReport {
        batch_id: batch_id.to_string(),
        records,
        migrated,
        failed,
        bytes,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// 迁移台账（append-only JSONL），与 Python 试点脚本同格式
fn write_ledger(root: &Path, batch_id: &str, records: &[Record]) {
    let dir = root.join(".filemanager").join("ledger");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("migrations.jsonl");
    let entry = serde_json::json!({
        "batch": batch_id,
        "time": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "archive_root": root.to_string_lossy(),
        "source": "tauri-app",
        "records": records.iter().map(|r| serde_json::json!({
            "fileId": r.file_id,
            "fileName": r.file_name,
            "src": r.source_path,
            "dest": r.target_path,
            "status": r.status,
            "size": r.size,
            "error": r.error,
        })).collect::<Vec<_>>(),
    });
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", entry);
    }
}

/// 撤销一个批次：从隔离区还原到原位置，并删除归档副本
pub fn undo(archive_root: &str, batch_id: &str, records: Vec<Record>) -> Result<UndoReport, String> {
    let root = PathBuf::from(archive_root);
    let trash_dir = root.join(".filemanager").join("trash").join(sanitize_segment(batch_id));
    let mut restored = 0usize;
    let mut failed = 0usize;
    let mut out: Vec<Record> = Vec::new();

    for mut r in records {
        if r.status != "recycled" && r.status != "verified" {
            continue;
        }
        let trash_path = trash_dir.join(format!(
            "{}_{}",
            sanitize_segment(&r.file_id),
            sanitize_segment(&r.file_name)
        ));
        if !trash_path.is_file() {
            r.status = "reverted".into();
            r.error = Some("隔离区找不到原件".into());
            failed += 1;
            out.push(r);
            continue;
        }
        let src = PathBuf::from(&r.source_path);
        let (dest, _) = unique_target(
            src.parent().unwrap_or_else(|| Path::new(".")),
            src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().as_str(),
        );
        match copy_file(&trash_path, &dest) {
            Ok(()) => {
                let ok = match (sha256_of(&trash_path), sha256_of(&dest)) {
                    (Ok(a), Ok(b)) => a == b,
                    _ => false,
                };
                if ok {
                    // 归档副本可以删了（原件已经回到原位且校验通过）
                    let target = PathBuf::from(&r.target_path);
                    if target.is_file() {
                        let _ = fs::remove_file(&target);
                    }
                    let _ = fs::remove_file(&trash_path);
                    r.status = "reverted".into();
                    r.error = None;
                    restored += 1;
                } else {
                    let _ = fs::remove_file(&dest);
                    r.status = "failed".into();
                    r.error = Some("还原后哈希不一致，已回滚".into());
                    failed += 1;
                }
            }
            Err(e) => {
                r.status = "failed".into();
                r.error = Some(format!("还原失败：{e}"));
                failed += 1;
            }
        }
        out.push(r);
    }

    Ok(UndoReport {
        restored,
        failed,
        records: out,
    })
}
