//! 深度标签 + 单句解读（P2 M2）
//!
//! 设计文档 §6.3 / §6.5 的执行口径，外加三条工程约束：
//!
//! 1. **只问两个字段**：`{"tags":[…],"summary":"…"}`。每多一个字段约 +1.3s（实测），
//!    而标签的收益只需要这两项。
//! 2. **模型必须能说「不知道」**：实测 24 份样本里模型 0 次主动说不知道，
//!    所以提示词里用硬话压（不确定就返回空数组），代码里也不给「必须凑够 3 个」的压力。
//! 3. **队列任何失败都不许冒泡成界面错误**，只能让该文件停在 `basic`。

use crate::error::{CmdError, MODEL_BAD_OUTPUT};
use crate::tag;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 正文截断长度（字符）。4000 字约 2000~3000 token，留足输出余量
pub const TEXT_CAP: usize = 4000;
/// 单份超时
pub const LLM_TIMEOUT_SECS: u64 = 60;
/// 队列串行：上一份完成到下一份之间的间隔（Ollama 并发实测无效，见 README）
pub const GAP_MS: u64 = 200;
/// 正文短于这个长度视为「无正文」，不送模型硬猜
pub const MIN_TEXT: usize = 30;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepTag {
    pub tags: Vec<String>,
    pub summary: Option<String>,
}

// ---------------------------------------------------------------- 提示词

/// 类型的中文说法（提示词里用，别把 `word` 这种内部枚举喂给模型）
fn kind_label(kind: &str) -> &'static str {
    match kind {
        "word" => "文字文档",
        "excel" => "表格",
        "ppt" => "演示文稿",
        "pdf" => "PDF 文档",
        "image" => "图片",
        "diagram" => "架构图/流程图",
        _ => "文档",
    }
}

pub fn build_prompt(name: &str, kind: &str, text: &str) -> String {
    let body: String = text.chars().take(TEXT_CAP).collect();
    format!(
        "读下面这份文件的正文，给它打 3~5 个主题标签，并用一句话说明它是什么。\n\
         \n\
         文件名：{name}\n\
         类型：{kind}\n\
         \n\
         要求：\n\
         1. 标签写「这件事属于哪一类工作或事项」，2~6 个字的通用名词短语，例如「安全检查」「人才培训」「招标采购」。\n\
         2. **不要泛词**：文档、资料、文件、附件、材料、汇报、通知 这类不算标签。\n\
         3. **看不出来就不要猜**。正文里没有依据的内容一律不要写。真的判断不了就返回空数组 []。\n\
         4. summary 写清「是什么 + 什么时候的 + 和谁/哪个项目有关」，20~40 字。写不出就填空字符串。\n\
         5. 只输出 JSON，不要解释、不要 Markdown 代码块：\n\
         {{\"tags\":[\"…\"],\"summary\":\"…\"}}\n\
         \n\
         正文：\n\
         {body}",
        name = name,
        kind = kind_label(kind),
        body = body
    )
}

// ---------------------------------------------------------------- 解析

/// 通用词：模型很爱给，但对推荐毫无区分度
const GENERIC: &[&str] = &[
    "文档", "文件", "资料", "材料", "附件", "汇报", "通知", "报告", "方案", "表格", "内容", "其他",
];

fn clean_tags(raw: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t = t.trim().trim_matches(|c: char| {
            c == '"' || c == '「' || c == '」' || c == '《' || c == '》' || c == '#'
        });
        if t.is_empty() || t.chars().count() > 12 {
            continue;
        }
        if GENERIC.contains(&t) {
            continue;
        }
        if !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
        if out.len() >= 5 {
            break;
        }
    }
    out
}

/// 从模型原文里抠 JSON。
///
/// 复用 `suggest_dirs` 的容错思路（去 ``` 围栏、取首尾花括号），但**字段缺失按空处理**：
/// 报错会让队列空转重试，而「模型少给一个字段」根本不值得重试。
pub fn parse(raw: &str) -> Result<DeepTag, CmdError> {
    let body = strip_fence(raw);
    let start = body.find('{').ok_or_else(|| {
        CmdError::new(MODEL_BAD_OUTPUT, "模型输出里没有 JSON 对象")
    })?;
    let end = body.rfind('}').ok_or_else(|| {
        CmdError::new(MODEL_BAD_OUTPUT, "模型输出的 JSON 没有闭合")
    })?;
    if end <= start {
        return Err(CmdError::new(MODEL_BAD_OUTPUT, "模型输出的 JSON 不完整"));
    }
    let slice = &body[start..=end];
    // 容忍尾逗号：`{"a":1,}` 这种模型常见手滑
    let cleaned = strip_trailing_commas(slice);

    let v: serde_json::Value = serde_json::from_str(&cleaned)
        .map_err(|e| CmdError::new(MODEL_BAD_OUTPUT, format!("模型输出不是合法 JSON：{e}")))?;

    let mut tags: Vec<String> = Vec::new();
    match v.get("tags") {
        Some(serde_json::Value::Array(a)) => {
            for x in a {
                if let Some(s) = x.as_str() {
                    tags.push(s.to_string());
                }
            }
        }
        // 模型偶尔给成字符串：`"tags":"安全检查、消防"` —— 按分隔符拆开，别整个丢掉
        Some(serde_json::Value::String(s)) => {
            for part in s.split(|c| c == '、' || c == ',' || c == '，' || c == ';' || c == '；') {
                tags.push(part.to_string());
            }
        }
        _ => {}
    }

    let summary = v
        .get("summary")
        .and_then(|s| s.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() >= 4);

    Ok(DeepTag {
        tags: clean_tags(tags),
        summary,
    })
}

fn strip_fence(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    // ```json\n…\n```
    let after = t.trim_start_matches("```");
    let after = match after.find('\n') {
        Some(i) => &after[i + 1..],
        None => after,
    };
    after.trim_end_matches("```").trim().to_string()
}

fn strip_trailing_commas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == ',' {
            let next = chars[i + 1..]
                .iter()
                .find(|x| !x.is_whitespace())
                .copied()
                .unwrap_or('\0');
            if next == '}' || next == ']' {
                continue;
            }
        }
        out.push(*c);
    }
    out
}

// ---------------------------------------------------------------- 送不送

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Send,
    Skip(&'static str),
}

/// 送模型白名单（计划 §3.2）
///
/// * 图片走视觉分支（P3 才做），`code` / `other` 读不出有意义正文
/// * **老版二进制 Office**（.doc/.xls/.ppt/.wps）解不出 XML，别浪费时间
/// * 正文 < 30 字 = 没有可解读的内容，硬送模型只会得到编造
pub fn should_deep_tag(kind: &str, ext: &str, text_len: usize) -> Decision {
    let ext = ext.trim_start_matches('.').to_lowercase();
    if kind == "code" || kind == "other" {
        return Decision::Skip("非文本可解读类型");
    }
    if kind == "image" {
        return Decision::Skip("图片走视觉分支（P3）");
    }
    if matches!(ext.as_str(), "doc" | "xls" | "ppt" | "wps" | "et" | "dps") {
        return Decision::Skip("老版二进制 Office，读不出正文");
    }
    if text_len < MIN_TEXT {
        return Decision::Skip("无正文");
    }
    Decision::Send
}

/// 取正文（给队列用）。抽不出就是空串，不 panic、不冒泡
pub fn text_of(path: &str, ext: &str, kind: &str, size: u64) -> String {
    let (text, _, _) = crate::extract::extract(Path::new(path), ext, kind, size);
    let s: String = text.chars().take(TEXT_CAP).collect();
    s.trim().to_string()
}

// ---------------------------------------------------------------- 优先级（T2.6）

/// 取件优先级：当前批 → 待定区 → 池内 → 已迁移
///
/// 纯函数，故意抽出来单测：SQL 里那个 CASE 必须与它逐条对应，
/// 两边一漂移就是「用户正在看的那批没有标签」。
pub fn priority(status: &str, in_batch: bool) -> u8 {
    if in_batch {
        return 0;
    }
    match status {
        "parked" => 1,
        "queued" | "labeled" | "pending_confirm" => 2,
        "migrated" | "confirmed" => 3,
        _ => 9,
    }
}

/// 一条待办
#[derive(Clone, Debug)]
pub struct Job {
    pub file_id: i64,
    pub name: String,
    pub abs_path: String,
    pub ext: String,
    pub kind: String,
    pub size: u64,
}

// ---------------------------------------------------------------- 单份处理

/// 一份文件走完「抽正文 → 判断送不送 → 问模型 → 解析」，**不碰数据库**
///
/// 抽出来是为了能单测：真正落库在 `index::apply_deep_tag`，
/// 这里只负责「把这份文件读成一段结构化结论」。
pub fn process_one(
    job: &Job,
    base: &str,
    model: &str,
    timeout: std::time::Duration,
) -> Result<DeepTag, CmdError> {
    let text = text_of(&job.abs_path, &job.ext, &job.kind, job.size);
    match should_deep_tag(&job.kind, &job.ext, text.chars().count()) {
        Decision::Skip(_) => Ok(DeepTag::default()),
        Decision::Send => {
            let prompt = build_prompt(&job.name, &job.kind, &text);
            let raw = crate::llm::generate(base, model, &prompt, timeout)?;
            parse(&raw)
        }
    }
}

// ---------------------------------------------------------------- 队列

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// 本次运行已完成（含「读过但没结论」）
    pub done: usize,
    /// 本次运行失败（本轮会跳过，重启后再试）
    pub failed: usize,
    /// 还剩多少份没深读（数据库现算）
    pub remaining: i64,
    /// 正在处理哪一份
    pub current: String,
    pub running: bool,
    pub paused: bool,
    pub enabled: bool,
    /// 队列为什么停着（模型未响应 / 用户关掉）
    pub note: Option<String>,
}

/// 队列句柄（放进 AppState，前端与目录建议都要摸它）
pub struct Queue {
    enabled: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    /// 目录建议要独占显存时置 true，队列让路
    pub paused_for_suggest: Arc<AtomicBool>,
    /// 正在跑一份模型请求（目录建议要等它跑完）
    pub busy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
    /// 当前批快照：用户正在看的那批必须先有标签
    pub batch: Arc<Mutex<Vec<i64>>>,
    /// 本轮已失败的文件，别在原地打转
    failed_ids: Arc<Mutex<Vec<i64>>>,
}

impl Queue {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            paused: Arc::new(AtomicBool::new(false)),
            paused_for_suggest: Arc::new(AtomicBool::new(false)),
            busy: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(Stats::default())),
            batch: Arc::new(Mutex::new(Vec::new())),
            failed_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn snapshot(&self) -> Stats {
        let mut s = self.stats.lock().map(|g| g.clone()).unwrap_or_default();
        s.paused = self.paused.load(Ordering::Relaxed);
        s.enabled = self.enabled.load(Ordering::Relaxed);
        s
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    /// 等「当前这一份跑完」，最多等 `max_ms`（目录建议抢显存时用）
    pub fn wait_idle(&self, max_ms: u64) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_ms);
        while self.is_busy() {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        true
    }

    pub fn set_batch(&self, ids: Vec<i64>) {
        if let Ok(mut b) = self.batch.lock() {
            *b = ids;
        }
    }

    pub fn snapshot_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// 统计区的短锁（只用来读改写几个数字，别在里面做 IO）
    pub fn stats_lock(&self) -> Result<std::sync::MutexGuard<'_, Stats>, ()> {
        self.stats.lock().map_err(|_| ())
    }

    pub fn already_failed(&self, id: i64) -> bool {
        self.failed_ids
            .lock()
            .map(|v| v.contains(&id))
            .unwrap_or(false)
    }

    pub fn mark_failed(&self, id: i64) {
        if let Ok(mut v) = self.failed_ids.lock() {
            if !v.contains(&id) {
                v.push(id);
            }
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_carries_the_three_hard_rules() {
        let p = build_prompt("安全检查通知.docx", "word", &"正文".repeat(3000));
        assert!(p.contains("安全检查通知.docx"));
        assert!(p.contains("文字文档"));
        assert!(p.contains("空数组"), "必须允许模型说不知道");
        assert!(p.contains("只输出 JSON"));
        // 正文必须截断到 4000 字
        assert!(p.chars().count() < 4000 + 800, "prompt 过长：{}", p.chars().count());
    }

    #[test]
    fn parse_plain_json() {
        let d = parse(r#"{"tags":["安全检查","消防验收"],"summary":"2026年度安全检查的通知，涉及生产部。"}"#).unwrap();
        assert_eq!(d.tags, vec!["安全检查", "消防验收"]);
        assert!(d.summary.unwrap().contains("安全检查"));
    }

    #[test]
    fn parse_fenced_json() {
        let d = parse("```json\n{\"tags\":[\"人才培训\"],\"summary\":\"继续教育学院的培训安排。\"}\n```").unwrap();
        assert_eq!(d.tags, vec!["人才培训"]);
    }

    #[test]
    fn parse_missing_summary_is_null_not_error() {
        let d = parse(r#"{"tags":["招标采购"]}"#).unwrap();
        assert_eq!(d.tags, vec!["招标采购"]);
        assert!(d.summary.is_none(), "缺 summary 按空处理，不许报错让队列空转");

        let d = parse(r#"{"tags":[],"summary":""}"#).unwrap();
        assert!(d.tags.is_empty());
        assert!(d.summary.is_none());
    }

    #[test]
    fn parse_tags_as_string() {
        let d = parse(r#"{"tags":"安全检查、消防验收、培训","summary":"一份综合检查材料。"}"#).unwrap();
        assert_eq!(d.tags, vec!["安全检查", "消防验收", "培训"]);
    }

    #[test]
    fn parse_garbage_is_bad_output() {
        let e = parse("我不知道这份文件是什么").unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        let e = parse("{\"tags\": [1,2,3]").unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
    }

    #[test]
    fn generic_tags_are_dropped_and_capped() {
        let d = parse(
            r#"{"tags":["文档","资料","安全检查","消防验收","人才培训","招标采购","合同","附件"],"summary":"一份综合材料。"}"#,
        )
        .unwrap();
        assert!(!d.tags.iter().any(|t| t == "文档" || t == "资料" || t == "附件"));
        assert_eq!(d.tags.len(), 5, "最多 5 个：{:?}", d.tags);
    }

    #[test]
    fn trailing_comma_is_tolerated() {
        let d = parse(r#"{"tags":["安全检查",],"summary":"一份检查材料。",}"#).unwrap();
        assert_eq!(d.tags, vec!["安全检查"]);
        assert!(d.summary.is_some());
    }

    #[test]
    fn whitelist_decisions() {
        assert_eq!(should_deep_tag("word", "docx", 500), Decision::Send);
        assert_eq!(should_deep_tag("excel", "xlsx", 500), Decision::Send);
        assert_eq!(should_deep_tag("pdf", "pdf", 500), Decision::Send);
        assert_eq!(should_deep_tag("other", "js", 500), Decision::Skip("非文本可解读类型"));
        assert_eq!(should_deep_tag("other", "zip", 500), Decision::Skip("非文本可解读类型"));
        assert_eq!(should_deep_tag("word", "doc", 500), Decision::Skip("老版二进制 Office，读不出正文"));
        assert_eq!(should_deep_tag("word", "docx", 5), Decision::Skip("无正文"));
        assert_eq!(should_deep_tag("image", "png", 0), Decision::Skip("图片走视觉分支（P3）"));
    }

    /// 起一个假 Ollama：返回固定响应体。
    ///
    /// 必须把请求**整份读完**（头 + 按 Content-Length 的体）再回写，否则
    /// 客户端还在写请求体、服务端已经 close，Windows 上会抛
    /// `os error 10053 (连接被本机软件中止)`，测试就随机红。
    fn stub_server(body: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut sock, _)) = listener.accept() else {
                    return;
                };
                let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(5)));

                // 1) 读到空行为止，拿到头
                let mut req: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                let header_end = loop {
                    match sock.read(&mut chunk) {
                        Ok(0) => break None,
                        Ok(n) => {
                            req.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = find_subslice(&req, b"\r\n\r\n") {
                                break Some(pos + 4);
                            }
                        }
                        Err(_) => break None,
                    }
                };
                let Some(header_end) = header_end else { continue };

                // 2) 按 Content-Length 把请求体读完，不读完不回复
                let head = String::from_utf8_lossy(&req[..header_end]).to_string();
                let want = head
                    .lines()
                    .find_map(|l| {
                        let low = l.to_ascii_lowercase();
                        low.strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while req.len() < header_end + want {
                    match sock.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => req.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }

                // 3) 回写并半关闭，让客户端先看到完整响应再断开
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes());
                let _ = sock.flush();
                let _ = sock.shutdown(std::net::Shutdown::Write);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || hay.len() < needle.len() {
            return None;
        }
        hay.windows(needle.len()).position(|w| w == needle)
    }

    fn write_txt(text: &str) -> (tempfile::TempDir, Job) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("安全检查通知.txt");
        std::fs::write(&p, text).unwrap();
        let job = Job {
            file_id: 1,
            name: "安全检查通知.txt".into(),
            abs_path: p.to_string_lossy().to_string(),
            ext: "txt".into(),
            kind: "word".into(),
            size: text.len() as u64,
        };
        (dir, job)
    }

    // T2.4：整条链路（抽正文 → 拼提示词 → HTTP → 解析）在桩上跑通
    #[test]
    fn process_one_end_to_end_on_stub() {
        let base = stub_server(
            r#"{"response":"{\"tags\":[\"安全检查\",\"消防验收\"],\"summary\":\"2026年度安全检查通知。\"}"}"#,
        );
        let (_d, job) = write_txt("关于开展2026年度安全检查的通知，请各部门于11月底前完成自查并上报。");
        let d = process_one(&job, &base, "test-model", std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(d.tags, vec!["安全检查", "消防验收"]);
        assert!(d.summary.unwrap().contains("安全检查"));
    }

    // T2.4：模型返回垃圾 → BAD_OUTPUT，不 panic（队列靠这个判失败，进程要活着）
    #[test]
    fn process_one_bad_json_is_error_not_panic() {
        let base = stub_server(r#"{"response":"我不会，你自己看着办"}"#);
        let (_d, job) = write_txt("这是一份足够长的正文，用来确保它会被送去模型而不是被当成无正文跳过。");
        let e = process_one(&job, &base, "test-model", std::time::Duration::from_secs(5)).unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
    }

    // T2.4：同一份文件本轮失败后不再重取（否则队列原地打转）
    #[test]
    fn queue_skips_files_that_already_failed() {
        let q = Queue::new(true);
        assert!(!q.already_failed(7));
        q.mark_failed(7);
        assert!(q.already_failed(7));
        assert!(!q.already_failed(8));
    }

    // T2.6：目录建议要等「当前这一份跑完」，但不能死等
    #[test]
    fn wait_idle_returns_when_queue_finishes_current() {
        let q = std::sync::Arc::new(Queue::new(true));
        q.busy.store(true, Ordering::Relaxed);
        let q2 = q.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            q2.busy.store(false, Ordering::Relaxed);
        });
        let t0 = std::time::Instant::now();
        assert!(q.wait_idle(3000), "应该在超时前等到空闲");
        assert!(t0.elapsed().as_millis() >= 250, "不许立刻返回，那是没等");
    }

    #[test]
    fn wait_idle_gives_up_when_queue_is_stuck() {
        let q = Queue::new(true);
        q.busy.store(true, Ordering::Relaxed);
        let t0 = std::time::Instant::now();
        assert!(!q.wait_idle(200), "超时要返回 false，让目录建议自己决定是否硬上");
        assert!(t0.elapsed().as_millis() >= 190);
    }

    #[test]
    fn priority_order() {
        assert_eq!(priority("migrated", true), 0, "当前批永远最先");
        assert_eq!(priority("parked", false), 1);
        assert_eq!(priority("pending_confirm", false), 2);
        assert_eq!(priority("queued", false), 2);
        assert_eq!(priority("migrated", false), 3);
        assert!(priority("labeled", false) < priority("migrated", false));
    }
}
