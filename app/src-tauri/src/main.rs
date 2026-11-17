//! 文件管家 · 本地智能文件归档客户端
//!
//! 前端：Vue 3（打包进 dist/）
//! 后端：Rust（本文件及同 crate 模块），负责文件系统、正文抽取、安全迁移
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod extract;
mod migrate;
mod model;
mod scan;

#[cfg(test)]
mod tests;

use model::{MigrateReport, Plan, Record, ScanSummary, UndoReport};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 选择目录（系统原生对话框）
#[tauri::command]
async fn pick_directory(title: Option<String>, initial: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new();
        if let Some(t) = title {
            if !t.is_empty() {
                d = d.set_title(&t);
            }
        }
        if let Some(i) = initial {
            if !i.is_empty() && Path::new(&i).is_dir() {
                d = d.set_directory(&i);
            }
        }
        d.pick_folder().map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// 扫描目录并抽取正文特征（耗时操作，放到阻塞线程池）
#[tauri::command]
async fn scan_directory(
    app: AppHandle,
    root: String,
    archive_root: Option<String>,
    max_size_mb: Option<u64>,
    limit: Option<usize>,
) -> Result<ScanSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scan::scan(
            Some(&app),
            &root,
            archive_root,
            max_size_mb.unwrap_or(2048),
            limit.unwrap_or(20000),
        )
    })
    .await
    .map_err(|e| format!("扫描线程异常：{e}"))?
}

/// 执行安全迁移（三段式）
#[tauri::command]
async fn migrate_files(
    app: AppHandle,
    archive_root: String,
    batch_id: String,
    plans: Vec<Plan>,
) -> Result<MigrateReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        migrate::migrate(Some(&app), &archive_root, &batch_id, plans)
    })
    .await
    .map_err(|e| format!("迁移线程异常：{e}"))?
}

/// 撤销一个批次
#[tauri::command]
async fn undo_migration(
    archive_root: String,
    batch_id: String,
    records: Vec<Record>,
) -> Result<UndoReport, String> {
    tauri::async_runtime::spawn_blocking(move || migrate::undo(&archive_root, &batch_id, records))
        .await
        .map_err(|e| format!("撤销线程异常：{e}"))?
}

/// 准备归档根目录（创建目录 + .filemanager 结构）
#[tauri::command]
fn prepare_archive_root(path: String) -> Result<String, String> {
    let root = PathBuf::from(&path);
    fs::create_dir_all(&root).map_err(|e| format!("无法创建目录：{e}"))?;
    for sub in [".filemanager", ".filemanager/trash", ".filemanager/ledger"] {
        let _ = fs::create_dir_all(root.join(sub));
    }
    Ok(root.to_string_lossy().to_string())
}

/// 路径是否存在及类型
#[tauri::command]
fn path_info(path: String) -> serde_json::Value {
    let p = Path::new(&path);
    serde_json::json!({
        "exists": p.exists(),
        "isDir": p.is_dir(),
        "isFile": p.is_file(),
    })
}

/// 在资源管理器中打开
#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("路径不存在".into());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(p.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("仅支持 Windows".into())
    }
}

/// 代理本地大模型的 HTTP 请求
///
/// 为什么要绕到 Rust 发请求：Windows 上 Tauri 页面 origin 是 `http://tauri.localhost`，
/// 而 Ollama 默认的 CORS 白名单只放行 `tauri://localhost` 与 `localhost:端口`，
/// 浏览器直连会被挡成 403。从本地 Rust 进程发请求不带 Origin 头，天然没有这个问题。
#[tauri::command]
async fn llm_request(
    method: String,
    url: String,
    body: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(60000));
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        let res = if method.eq_ignore_ascii_case("GET") {
            agent.get(&url).call()
        } else {
            agent
                .post(&url)
                .set("Content-Type", "application/json")
                .send_string(body.as_deref().unwrap_or("{}"))
        };
        match res {
            Ok(r) => r.into_string().map_err(|e| format!("读取模型响应失败：{e}")),
            Err(ureq::Error::Status(code, r)) => {
                let text: String = r.into_string().unwrap_or_default();
                Err(format!(
                    "模型服务返回 {code}：{}",
                    text.chars().take(200).collect::<String>()
                ))
            }
            Err(e) => Err(format!("无法连接模型服务：{e}")),
        }
    })
    .await
    .map_err(|e| format!("请求线程异常：{e}"))?
}

/// 本机磁盘剩余空间（GB）
#[tauri::command]
fn disk_free_gb(path: String) -> Option<f64> {
    let p = PathBuf::from(&path);
    let target = p.ancestors().find(|a| a.exists())?;
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-PSDrive -Name '{}').Free",
                    target.components().next().and_then(|c| match c {
                        std::path::Component::Prefix(p) => {
                            Some(p.as_os_str().to_string_lossy().trim_end_matches(':').to_string())
                        }
                        _ => None,
                    })?
                ),
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let bytes: f64 = s.trim().parse().ok()?;
        return Some(bytes / 1024.0 / 1024.0 / 1024.0);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        None
    }
}

/// 诊断模式：不加 GUI 直接扫描一个目录，把结果写成 JSON
/// 用法：file-manager.exe --selftest <目录> <输出json路径>
fn run_selftest(dir: &str, out: &str) -> i32 {
    use std::time::Instant;
    let t0 = Instant::now();
    match scan::scan(None, dir, None, 2048, 20000) {
        Ok(r) => {
            let ms = t0.elapsed().as_millis();
            let with_text = r.files.iter().filter(|f| !f.text_summary.is_empty()).count();
            let with_drawio = r.files.iter().filter(|f| !f.drawio_nodes.is_empty()).count();
            let bytes: u64 = r.files.iter().map(|f| f.size).sum();
            let payload = serde_json::json!({
                "dir": dir,
                "elapsedMs": ms,
                "count": r.files.len(),
                "skipped": r.skipped,
                "truncated": r.truncated,
                "bytes": bytes,
                "withText": with_text,
                "withDrawio": with_drawio,
                "files": r.files,
            });
            if let Err(e) = std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
                eprintln!("写出失败：{e}");
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!("扫描失败：{e}");
            1
        }
    }
}

/// 诊断模式：不加 GUI 直接测一次本地大模型调用，把结果写成 JSON
/// 用法：file-manager.exe --llmtest <输出json路径> [模型名]
fn run_llmtest(out: &str, model: &str) -> i32 {
    use std::time::Instant;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .build();

    let t0 = Instant::now();
    let tags: Result<String, String> = match agent.get("http://127.0.0.1:11434/api/tags").call() {
        Ok(r) => r.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let tags_ms = t0.elapsed().as_millis();

    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "format": {
            "type": "object",
            "properties": {
                "ws": { "type": "string", "enum": ["智能工厂平台","钻采院项目","商务采购",""] },
                "dt": { "type": "string", "enum": ["方案设计","成本报价",""] }
            },
            "required": ["ws", "dt"]
        },
        "options": { "temperature": 0, "num_predict": 48, "num_ctx": 2048 },
        "messages": [
            { "role": "system", "content": "你是文件归档助手，只输出 JSON。" },
            { "role": "user", "content": "文件名：钻采院高温高压设备技术方案v3.docx\n正文：本方案针对钻采院高温高压试验装置提出改造方案。" }
        ]
    });

    let t1 = Instant::now();
    let chat: Result<String, String> = match agent
        .post("http://127.0.0.1:11434/api/chat")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(r) => r.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let chat_ms = t1.elapsed().as_millis();

    let models: Vec<String> = tags
        .as_ref()
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| {
            v["models"].as_array().map(|a| {
                a.iter()
                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();

    let content = chat
        .as_ref()
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v["message"]["content"].as_str().map(|s| s.to_string()));

    let payload = serde_json::json!({
        "tagsOk": tags.is_ok(),
        "tagsMs": tags_ms,
        "models": models,
        "chatOk": chat.is_ok(),
        "chatMs": chat_ms,
        "content": content,
        "error": chat.as_ref().err(),
    });
    match std::fs::write(out, serde_json::to_string_pretty(&payload).unwrap()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("写出失败：{e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--selftest" {
        std::process::exit(run_selftest(&args[2], &args[3]));
    }
    if args.len() >= 3 && args[1] == "--llmtest" {
        let model = args.get(3).map(|s| s.as_str()).unwrap_or("qwen2.5:7b");
        std::process::exit(run_llmtest(&args[2], model));
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            pick_directory,
            scan_directory,
            migrate_files,
            undo_migration,
            prepare_archive_root,
            path_info,
            open_path,
            disk_free_gb,
            llm_request
        ])
        .run(tauri::generate_context!())
        .expect("应用启动失败");
}
