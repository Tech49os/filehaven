//! 深度标签 + 单句解读（P2 M2）
//!
//! 设计文档 §6.3 / §6.5 的执行口径，外加三条工程约束：
//!
//! 1. **只问两个字段**：`{"tags":[…],"summary":"…"}`。每多一个字段约 +1.3s（实测），
//!    而标签的收益只需要这两项。
//! 2. **模型必须能说「不知道」**：实测 24 份样本里模型 0 次主动说不知道，
//!    所以提示词里用硬话压（不确定就返回空数组），代码里也不给「必须凑够 3 个」的压力。
//! 3. **队列任何失败都不许冒泡成界面错误**，只能让该文件停在 `basic`。

use crate::error::{CmdError, MODEL_BAD_OUTPUT, MODEL_BUSY};
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
    /// `vision=true` 走视觉通道（图片），false 走文本
    Send { vision: bool },
    Skip(&'static str),
}

/// 送模型白名单（计划 §3.2）
///
/// * 图片走视觉分支（P3 才做），`code` / `other` 读不出有意义正文
/// * **老版二进制 Office**（.doc/.xls/.ppt/.wps）解不出 XML，别浪费时间
/// * 正文 < 30 字 = 没有可解读的内容，硬送模型只会得到编造
pub const VISION_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// 原图 ≤8MB（超了不做缩放 —— 没有图像库，硬加一个只为缩图不值）
pub const VISION_MAX_BYTES: u64 = 8 * 1024 * 1024;

pub fn should_deep_tag(kind: &str, ext: &str, text_len: usize, size: u64) -> Decision {
    let ext = ext.trim_start_matches('.').to_lowercase();
    if kind == "code" || kind == "other" {
        return Decision::Skip("非文本可解读类型");
    }
    if kind == "image" {
        // 视觉分支（P3 T4.1）：只送 Ollama 吃得住的四类；bmp/gif 送进去只会得到胡编
        if !VISION_EXTS.contains(&ext.as_str()) {
            return Decision::Skip("图片格式不支持");
        }
        if size > VISION_MAX_BYTES {
            return Decision::Skip("图片过大");
        }
        return Decision::Send { vision: true };
    }
    if matches!(ext.as_str(), "doc" | "xls" | "ppt" | "wps" | "et" | "dps") {
        return Decision::Skip("老版二进制 Office，读不出正文");
    }
    if text_len < MIN_TEXT {
        if ext == "pdf" {
            // P3 T4.4：明确「不解析」，不装读。栅格化要引 pdfium（+8MB 原生依赖），留 P4
            return Decision::Skip("扫描件：本机不解析图像页");
        }
        return Decision::Skip("无正文");
    }
    Decision::Send { vision: false }
}

/// 取正文（给队列用）。抽不出就是空串，不 panic、不冒泡
pub fn text_of(path: &str, ext: &str, kind: &str, size: u64) -> String {
    let (text, _, _) = crate::extract::extract(Path::new(path), ext, kind, size);
    let s: String = text.chars().take(TEXT_CAP).collect();
    s.trim().to_string()
}

// ---------------------------------------------------------------- 模型闸（P3 §3.4）

use crate::model_gate::{ModelGate, Owner};

/// RAII 闸守卫：拿到闸之后，无论后面成功失败都自动释放，不许手动管。
///
/// 手动 release 的问题在 `?` 和早返回——漏一条路径就是永久占闸，
/// 队列从此哑掉。Drop 保证不漏。
struct GateGuard<'a> {
    gate: &'a ModelGate,
    who: Owner,
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        self.gate.release(self.who);
    }
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
/// 视觉提示词（P3 T4.2）：三段式，明写「看不清就给空数组，不要猜」。
/// 图片不产 `type`/`time` 标签 —— parse 出来的 tags 落库时一律进 `kind='topic'`，天然满足。
pub fn build_vision_prompt(name: &str) -> String {
    format!(
        "看这张图片（文件名：{name}），回答：\n\
         1. 这是一张什么图（照片/截图/图表/扫描件）；\n\
         2. 与什么项目或事项相关；\n\
         3. 给 3~5 个主题标签（2~6 字的通用名词短语，如「现场照片」「设备验收」），\n\
         并用一句话说明这张图是什么（20~40 字）。\n\
         **看不清或没有依据就不要猜：返回空数组 []，summary 填空字符串。**\n\
         只输出 JSON：{{\"tags\":[\"…\"],\"summary\":\"…\"}}"
    )
}

pub fn process_one(
    job: &Job,
    base: &str,
    model: &str,
    timeout: std::time::Duration,
    gate: &ModelGate,
) -> Result<DeepTag, CmdError> {
    let text = text_of(&job.abs_path, &job.ext, &job.kind, job.size);
    match should_deep_tag(&job.kind, &job.ext, text.chars().count(), job.size) {
        Decision::Skip(_) => Ok(DeepTag::default()),
        Decision::Send { vision } => {
            // 模型闸（P3 §3.4）：发请求前必须占位。拿不到（目录建议/另一通道在跑）
            // 返回 MODEL_BUSY，队列跳过这一轮，不死等。timeout=0：立刻判定。
            let owner = if vision { Owner::Vision } else { Owner::Text };
            if !gate.acquire(owner, 0) {
                return Err(CmdError::new(
                    MODEL_BUSY,
                    "模型正被其他任务占用（目录建议/视觉解读），本轮跳过",
                ));
            }
            let _guard = GateGuard { gate, who: owner };
            let raw = if vision {
                // 视觉：读原图 → base64 直送（§3.4）。没有图像库，不做任何缩放
                let bytes = std::fs::read(&job.abs_path)?;
                use std::io::Write as _;
                let mut b64 = String::with_capacity((bytes.len() + 2) / 3 * 4 + 4);
                let mut enc = Base64Writer { out: &mut b64 };
                // 标准库没有 base64；手写一个小编码器（只在测试与真机用，性能足够）
                let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
                let b = &bytes;
                let mut i = 0;
                while i + 2 < b.len() {
                    let n = ((b[i] as u32) << 16) | ((b[i + 1] as u32) << 8) | b[i + 2] as u32;
                    let _ = enc.write_all(&[
                        table[(n >> 18) as usize & 63],
                        table[(n >> 12) as usize & 63],
                        table[(n >> 6) as usize & 63],
                        table[n as usize & 63],
                    ]);
                    i += 3;
                }
                let rem = b.len() - i;
                if rem == 1 {
                    let n = (b[i] as u32) << 16;
                    let _ = enc.write_all(&[table[(n >> 18) as usize & 63], table[(n >> 12) as usize & 63], b'=', b'=']);
                } else if rem == 2 {
                    let n = ((b[i] as u32) << 16) | ((b[i + 1] as u32) << 8);
                    let _ = enc.write_all(&[table[(n >> 18) as usize & 63], table[(n >> 12) as usize & 63], table[(n >> 6) as usize & 63], b'=']);
                }
                drop(enc);
                let prompt = build_vision_prompt(&job.name);
                crate::llm::generate_vision(base, model, &prompt, &b64, timeout)?
            } else {
                let prompt = build_prompt(&job.name, &job.kind, &text);
                crate::llm::generate(base, model, &prompt, timeout)?
            };
            parse(&raw)
        }
    }
}

/// 极简 base64 写出器（std::io::Write 适配，只为上面的手写编码器收口）
struct Base64Writer<'a> {
    out: &'a mut String,
}
impl<'a> std::io::Write for Base64Writer<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.out.push_str(std::str::from_utf8(buf).map_err(|_| std::io::ErrorKind::InvalidData)?);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
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
    /// 正在处理哪一份的 id（T4.5）：待定区「正在读…」按钮态靠它收敛
    pub current_id: Option<i64>,
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
    /// 目录建议要独占显存时置 true，队列让路（P2 语义不动；占闸期间队列也会让路，双保险）
    pub paused_for_suggest: Arc<AtomicBool>,
    /// 模型闸（P3 §3.4）：目录建议 / 文本深读 / 视觉深读 三向互斥。
    /// 取代了旧的 `busy` + `wait_idle` 双人锁（同一职责两套机制并存必然漂移）
    pub gate: Arc<ModelGate>,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
    /// 当前批快照：用户正在看的那批必须先有标签
    pub batch: Arc<Mutex<Vec<i64>>>,
    /// 本轮已失败的文件，别在原地打转
    failed_ids: Arc<Mutex<Vec<i64>>>,
    /// 用户点「让 AI 说明」插队的 id（T4.5）：worker 取件前先看这里，FIFO
    explain_front: Arc<Mutex<Vec<i64>>>,
}

impl Queue {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            paused: Arc::new(AtomicBool::new(false)),
            paused_for_suggest: Arc::new(AtomicBool::new(false)),
            gate: Arc::new(ModelGate::default()),
            stop: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(Stats::default())),
            batch: Arc::new(Mutex::new(Vec::new())),
            failed_ids: Arc::new(Mutex::new(Vec::new())),
            explain_front: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// T4.5：把一份文件插到深读队列最前。重复点同一份去重，返回是否真的排上了
    pub fn push_front(&self, id: i64) -> bool {
        if let Ok(mut v) = self.explain_front.lock() {
            if !v.contains(&id) {
                v.push(id);
                return true;
            }
        }
        false
    }

    /// 取出下一个插队件（先插先读）。没有就 None，worker 回落到 SQL 取件
    pub fn pop_front(&self) -> Option<i64> {
        let mut v = self.explain_front.lock().ok()?;
        if v.is_empty() {
            None
        } else {
            Some(v.remove(0))
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
        assert_eq!(should_deep_tag("word", "docx", 500, 100), Decision::Send { vision: false });
        assert_eq!(should_deep_tag("excel", "xlsx", 500, 100), Decision::Send { vision: false });
        assert_eq!(should_deep_tag("pdf", "pdf", 500, 100), Decision::Send { vision: false });
        assert_eq!(should_deep_tag("other", "js", 500, 100), Decision::Skip("非文本可解读类型"));
        assert_eq!(should_deep_tag("other", "zip", 500, 100), Decision::Skip("非文本可解读类型"));
        assert_eq!(should_deep_tag("word", "doc", 500, 100), Decision::Skip("老版二进制 Office，读不出正文"));
        assert_eq!(should_deep_tag("word", "docx", 5, 100), Decision::Skip("无正文"));
        // P3 T4.4：扫描件明确「不解析」，不许有人顺手改成「识别中」
        assert_eq!(should_deep_tag("pdf", "pdf", 5, 100), Decision::Skip("扫描件：本机不解析图像页"));
        // P3 T4.1：图片走视觉分支（白名单 + 8MB 闸）
        assert_eq!(should_deep_tag("image", "png", 0, 100), Decision::Send { vision: true });
        assert_eq!(should_deep_tag("image", "jpg", 0, 100), Decision::Send { vision: true });
        assert_eq!(should_deep_tag("image", "webp", 0, 100), Decision::Send { vision: true });
        assert_eq!(should_deep_tag("image", "bmp", 0, 100), Decision::Skip("图片格式不支持"));
        assert_eq!(should_deep_tag("image", "gif", 0, 100), Decision::Skip("图片格式不支持"));
        assert_eq!(
            should_deep_tag("image", "png", 0, 9 * 1024 * 1024),
            Decision::Skip("图片过大")
        );
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

    // T2.4：整条链路（抽正文 → 拼提示词 → HTTP → 解析）在桩上跑通；结束闸必须已释放
    #[test]
    fn process_one_end_to_end_on_stub() {
        let base = stub_server(
            r#"{"response":"{\"tags\":[\"安全检查\",\"消防验收\"],\"summary\":\"2026年度安全检查通知。\"}"}"#,
        );
        let (_d, job) = write_txt("关于开展2026年度安全检查的通知，请各部门于11月底前完成自查并上报。");
        let gate = ModelGate::default();
        let d = process_one(&job, &base, "test-model", std::time::Duration::from_secs(5), &gate)
            .unwrap();
        assert_eq!(d.tags, vec!["安全检查", "消防验收"]);
        assert!(d.summary.unwrap().contains("安全检查"));
        assert_eq!(gate.owner(), None, "成功路径也要放闸，不许漏");
    }

    // T2.4：模型返回垃圾 → BAD_OUTPUT，不 panic（队列靠这个判失败，进程要活着）；失败也要放闸
    #[test]
    fn process_one_bad_json_is_error_not_panic() {
        let base = stub_server(r#"{"response":"我不会，你自己看着办"}"#);
        let (_d, job) = write_txt("这是一份足够长的正文，用来确保它会被送去模型而不是被当成无正文跳过。");
        let gate = ModelGate::default();
        let e = process_one(&job, &base, "test-model", std::time::Duration::from_secs(5), &gate)
            .unwrap_err();
        assert_eq!(e.code, MODEL_BAD_OUTPUT);
        assert_eq!(gate.owner(), None, "失败路径也要放闸（Drop 守卫兜底）");
    }

    // §3.4：闸被别人占着（目录建议在跑）→ MODEL_BUSY，绝不发 HTTP
    #[test]
    fn process_one_skips_model_when_gate_is_taken() {
        let base = stub_server(r#"{"response":"不该被调到"}"#);
        let (_d, job) = write_txt("关于开展2026年度安全检查的通知，请各部门于11月底前完成自查并上报。");
        let gate = ModelGate::default();
        assert!(gate.acquire(Owner::Suggest, 0));
        let e = process_one(&job, &base, "test-model", std::time::Duration::from_secs(5), &gate)
            .unwrap_err();
        assert_eq!(e.code, MODEL_BUSY, "拿不到闸要立刻报 MODEL_BUSY，不死等");
        assert_eq!(gate.owner(), Some(Owner::Suggest), "别人的闸不许被释放");
        // 队列对 MODEL_BUSY 只跳过不记失败 —— 文件本身没问题
    }

    // §3.4：闸被占时，图片任务同样拿不到（视觉通道 owner=Vision）
    #[test]
    fn process_one_vision_job_respects_gate() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("现场照片.png");
        // 最小 PNG 头 + 任意字节即可：反正不会真的发出请求
        std::fs::write(&p, [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        let job = Job {
            file_id: 2,
            name: "现场照片.png".into(),
            abs_path: p.to_string_lossy().to_string(),
            ext: "png".into(),
            kind: "image".into(),
            size: 8,
        };
        let base = stub_server(r#"{"response":"不该被调到"}"#);
        let gate = ModelGate::default();
        assert!(gate.acquire(Owner::Suggest, 0));
        let e = process_one(&job, &base, "test-model", std::time::Duration::from_secs(5), &gate)
            .unwrap_err();
        assert_eq!(e.code, MODEL_BUSY);
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

    // T2.6 的 wait_idle 双人锁已被 §3.4 的 ModelGate 取代：
    // 等待/超时/让位的等价断言在 model_gate.rs（second_owner_waits_and_times_out /
    // waiter_gets_gate_after_holder_releases），不再保留两套机制。

    // T4.5：插队队列——去重、先插先读
    #[test]
    fn push_front_dedups_and_drains_fifo() {
        let q = Queue::new(true);
        assert!(q.push_front(7));
        assert!(!q.push_front(7), "同一个 id 重复点不许排两次");
        assert!(q.push_front(9));
        assert_eq!(q.pop_front(), Some(7), "先插的先读");
        assert_eq!(q.pop_front(), Some(9));
        assert_eq!(q.pop_front(), None);
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
