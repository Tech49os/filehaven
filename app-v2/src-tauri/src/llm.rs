//! 本地模型（Ollama）HTTP 调用
//!
//! 为什么走 Rust 而不是前端 fetch：Windows 上 Tauri 页面 origin 是
//! `http://tauri.localhost`，不在 Ollama 默认 CORS 白名单里，浏览器直连会被 403。
//! 从本地 Rust 进程发请求不带 Origin 头，天然没有这个问题。

use crate::error::{CmdError, MODEL_BAD_OUTPUT, MODEL_DOWN, MODEL_TIMEOUT};
use std::time::Duration;

pub const DEFAULT_BASE: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "qwen3.5:9b";

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).build()
}

/// 探活：`GET /api/tags` 能不能通
pub fn reachable(base: &str, timeout: Duration) -> bool {
    agent(timeout)
        .get(&format!("{}/api/tags", base.trim_end_matches('/')))
        .call()
        .is_ok()
}

/// 思考模型判定：只有这几个家族能接受 `think` 参数
///
/// 对非思考模型显式传 `think` 会被 Ollama 以 400 拒掉（"does not support thinking"），
/// 那会变成一次莫名其妙的功能不可用 —— 所以按模型名白名单传，而不是一律传。
pub fn supports_think(model: &str) -> bool {
    let m = model.to_lowercase();
    ["qwen3", "deepseek-r1", "magistral", "gpt-oss", "qwq", "glm-4.5"]
        .iter()
        .any(|k| m.contains(k))
}

/// 一次非流式生成。`format=json` 让模型只吐 JSON。
pub fn generate(
    base: &str,
    model: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String, CmdError> {
    let url = format!("{}/api/generate", base.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "format": "json",
        "options": { "num_ctx": 8192, "temperature": 0.3 }
    });
    if supports_think(model) {
        // 思考模型开着 think 会把 JSON 任务拖成 10 倍耗时（实测 9b 超 180s 全超时），
        // 且思考内容会挤占输出额度。只要结论，不要推理过程。
        body["think"] = serde_json::json!(false);
    }

    let res = agent(timeout)
        .post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());

    match res {
        Ok(r) => {
            let text = r
                .into_string()
                .map_err(|e| CmdError::new(MODEL_BAD_OUTPUT, format!("读取模型响应失败：{e}")))?;
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| CmdError::new(MODEL_BAD_OUTPUT, format!("响应不是 JSON：{e}")))?;
            v["response"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| CmdError::new(MODEL_BAD_OUTPUT, "响应里没有 response 字段"))
        }
        Err(ureq::Error::Status(code, r)) => {
            let text: String = r.into_string().unwrap_or_default();
            Err(CmdError::new(
                MODEL_BAD_OUTPUT,
                format!("模型服务返回 {code}：{}", text.chars().take(200).collect::<String>()),
            ))
        }
        Err(e) => Err(classify_transport_error(&e)),
    }
}

fn classify_transport_error(e: &ureq::Error) -> CmdError {
    let msg = e.to_string();
    let low = msg.to_lowercase();
    // 读超时和连不上必须分开：前者是「模型太慢」，后者是「模型没起」，
    // 前端文案与用户该做的事完全不同（等一会儿重试 vs 去开 Ollama）。
    // Windows 上读超时是 WSAETIMEDOUT(10060)，报错文案是中文「没有正确答复」，
    // 拿英文关键词判会把它误判成 MODEL_DOWN —— 实测踩过。
    let timeout_markers = [
        "timed out",
        "timeout",
        "10060",
        "没有正确答复",
        "超时",
    ];
    if timeout_markers.iter().any(|m| low.contains(m)) {
        CmdError::new(MODEL_TIMEOUT, format!("模型响应超时：{msg}"))
    } else {
        CmdError::new(MODEL_DOWN, format!("无法连接模型服务：{msg}"))
    }
}

/// 已加载模型列表（`GET /api/ps`）
pub fn loaded_models(base: &str, timeout: Duration) -> Vec<String> {
    let res = agent(timeout)
        .get(&format!("{}/api/ps", base.trim_end_matches('/')))
        .call();
    let Ok(r) = res else { return Vec::new() };
    let Ok(text) = r.into_string() else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// 显存互斥：把「不是 keep」的已加载模型卸掉（`keep_alive:0`）。
///
/// 8GB 显存装不下 9b + 4b 同时加载，会让 llama-server 崩。
/// **不要用 `ollama stop`** —— 它会把 serve 进程一起带崩（历史坑）。
pub fn unload_others(base: &str, keep: &str, timeout: Duration) {
    for m in loaded_models(base, timeout) {
        if m == keep {
            continue;
        }
        let body = serde_json::json!({ "model": m, "prompt": "", "keep_alive": 0 });
        let _ = agent(timeout)
            .post(&format!("{}/api/generate", base.trim_end_matches('/')))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
    }
}
