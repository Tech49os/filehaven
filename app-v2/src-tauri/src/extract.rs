//! 正文抽取：Office OpenXML / draw.io / 纯文本
//!
//! draw.io 解析是本产品的差异化能力（市面同类工具都不理解架构图内容），
//! 支持三种存储格式：未压缩 XML、base64 + raw deflate、URI 编码明文。

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use flate2::read::DeflateDecoder;
use regex::Regex;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::OnceLock;
use zip::ZipArchive;

const TEXT_CAP: usize = 6000;
const PLAIN_CAP: u64 = 200_000;

/// 需要**整份读进内存**的文件才能读正文的最大体积（目前只有 draw.io）
///
/// 注意：这个闸**不能**套在 Office 上。2026-11-20 实测反馈「42MB 的 pptx 读不出正文」，
/// 真因就是这里按总大小一刀切放弃了 —— 而 42MB 几乎全是被图片/视频/嵌入对象占掉的，
/// 真正有用的 slide XML 只有几 KB。Office 的读法是一个 entry 一个 entry 解压，
/// 内存上限交给 `MAX_ENTRY_BYTES` 管就够。
pub const MAX_PARSE_BYTES: u64 = 30 * 1024 * 1024;

/// 单个 zip entry 解压后最多收多少字节
///
/// 上限防的是「一个 XML 解出 500MB」这种解压炸弹，不是防大文件本身：
/// 正常的 document.xml / sheet1.xml 都在几百 KB 量级，8MB 已经很宽松。
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

fn re_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]*>").unwrap())
}

fn re_ws() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn re_value_attr() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?s)\bvalue="([^"]*)""#).unwrap())
}

fn re_label_attr() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?s)\blabel="([^"]*)""#).unwrap())
}

fn re_diagram_body() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<diagram[^>]*>(.*?)</diagram>").unwrap())
}

fn re_slide() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ppt/slides/slide(\d+)\.xml$").unwrap())
}

fn decode_entity(ent: &str) -> Option<char> {
    match ent {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = ent.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

/// 反转义 XML 实体。
///
/// 逐字符推进，识别失败时原样输出并前进一个字符——避免未知实体（如 `&foo;`）
/// 或未闭合的 `&` 造成死循环或字符重复。
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < s.len() {
        if bytes[i] == b'&' {
            if let Some(rel) = s[i..].find(';') {
                let ent = &s[i + 1..i + rel];
                if let Some(c) = decode_entity(ent) {
                    out.push(c);
                    i += rel + 1;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap_or('&');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 去掉标签、反转义、压缩空白
pub fn strip_tags(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let no_br = input.replace("<br>", " ").replace("<br/>", " ").replace("<br />", " ");
    let no_tag = re_tag().replace_all(&no_br, " ");
    let text = unescape(&no_tag);
    re_ws().replace_all(&text, " ").trim().to_string()
}

/// 从 Office OpenXML 里抽正文
fn office_text(path: &Path, kind: &str) -> String {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut zip = match ZipArchive::new(BufReader::new(file)) {
        Ok(z) => z,
        Err(_) => return String::new(),
    };

    let read_entry = |zip: &mut ZipArchive<BufReader<File>>, name: &str| -> Option<String> {
        let e = zip.by_name(name).ok()?;
        let mut s = String::new();
        // 限量读：entry 声称解压后 500MB 也只收 8MB，剩下的留给 TEXT_CAP 去截
        e.take(MAX_ENTRY_BYTES).read_to_string(&mut s).ok()?;
        Some(s)
    };

    match kind {
        "doc" => {
            let xml = match read_entry(&mut zip, "word/document.xml") {
                Some(x) => x,
                None => return String::new(),
            };
            let with_breaks = xml.replace("</w:p>", "\n");
            strip_tags(&with_breaks).chars().take(TEXT_CAP).collect()
        }
        "xls" => {
            let mut text = String::new();
            if let Some(xml) = read_entry(&mut zip, "xl/sharedStrings.xml") {
                text = strip_tags(&xml.replace("</si>", "\n"));
            }
            if text.is_empty() {
                if let Some(xml) = read_entry(&mut zip, "xl/worksheets/sheet1.xml") {
                    text = strip_tags(&xml);
                }
            }
            text.chars().take(TEXT_CAP).collect()
        }
        "ppt" => {
            let mut names: Vec<(u32, String)> = zip
                .file_names()
                .filter_map(|n| {
                    re_slide()
                        .captures(n)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u32>().ok())
                        .map(|i| (i, n.to_string()))
                })
                .collect();
            names.sort_by_key(|(i, _)| *i);
            let mut parts: Vec<String> = Vec::new();
            for (_, n) in names.into_iter().take(20) {
                if let Some(xml) = read_entry(&mut zip, &n) {
                    parts.push(strip_tags(&xml));
                }
            }
            parts.join(" ").chars().take(TEXT_CAP).collect()
        }
        _ => String::new(),
    }
}

/// PDF 文字层抽取（T2.3）
///
/// 为什么放在独立线程里：`pdf-extract` 是纯 Rust（底层 lopdf），没有取消语义，
/// 碰上畸形 xref 表会卡很久。队列是串行的，一份卡住整条队列就停了 ——
/// 所以给它一个硬超时，超时就当「没有文字层」返回空串，文件标「未解读」。
/// （线程本身会自己跑完退出，不 join，泄漏一个短命线程比卡住队列便宜得多。）
pub fn pdf_text(path: &Path) -> String {
    let p = path.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let t = pdf_extract::extract_text(&p).unwrap_or_default();
        let _ = tx.send(t);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(PDF_TIMEOUT_SECS)) {
        Ok(t) => t.chars().take(TEXT_CAP).collect(),
        Err(_) => String::new(),
    }
}

/// PDF 抽取硬超时（秒）
pub const PDF_TIMEOUT_SECS: u64 = 8;

fn dedupe_keys(raw: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for s in raw {
        let k = s.trim().to_string();
        if k.is_empty() || !seen.insert(k.clone()) {
            continue;
        }
        out.push(k);
    }
    out
}

/// 从一段 draw.io XML 里收节点文字
fn collect_nodes(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    for caps in re_value_attr().captures_iter(xml) {
        out.push(caps[1].to_string());
    }
    for caps in re_label_attr().captures_iter(xml) {
        out.push(caps[1].to_string());
    }
    let cleaned: Vec<String> = out
        .into_iter()
        .map(|v| strip_tags(&unescape(&v)))
        .filter(|t| {
            let n = t.chars().count();
            n >= 1 && n <= 60 && !t.chars().all(|c| c.is_ascii_digit() || c.is_whitespace())
        })
        .collect();
    dedupe_keys(cleaned)
}

fn inflate_raw(b64: &str) -> Option<String> {
    let compact: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = B64.decode(compact.as_bytes()).ok()?;
    let mut dec = DeflateDecoder::new(&bytes[..]);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    Some(String::from_utf8_lossy(&out).to_string())
}

fn uri_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.to_string())
}

/// 解析 draw.io，返回（节点文字, 是否失败）
pub fn parse_drawio(xml: &str) -> (Vec<String>, bool) {
    // 1) 未压缩 XML
    let direct = collect_nodes(xml);
    if !direct.is_empty() {
        return (direct, false);
    }

    // 2) URI 编码的明文
    if let Some(idx) = xml.find("%3C") {
        let decoded = uri_decode(&xml[idx..]);
        let n = collect_nodes(&decoded);
        if !n.is_empty() {
            return (n, false);
        }
    }

    // 3) base64 + raw deflate（draw.io 默认压缩格式）
    let mut nodes: Vec<String> = Vec::new();
    for caps in re_diagram_body().captures_iter(xml) {
        let raw = caps[1].trim();
        if raw.is_empty() {
            continue;
        }
        if let Some(inflated) = inflate_raw(raw) {
            let payload = if inflated.contains('%') {
                uri_decode(&inflated)
            } else {
                inflated
            };
            nodes.extend(collect_nodes(&payload));
        }
    }
    if !nodes.is_empty() {
        return (dedupe_keys(nodes), false);
    }

    (Vec::new(), true)
}

/// 扩展名 → 文件类型（与前端 extractor.ts 保持一致）
pub fn kind_of(ext: &str) -> &'static str {
    match ext {
        "doc" | "docx" | "wps" => "doc",
        "xls" | "xlsx" | "csv" => "xls",
        "ppt" | "pptx" => "ppt",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif" => "image",
        "drawio" => "drawio",
        "txt" | "md" | "log" | "json" | "yml" | "yaml" | "xml" => "text",
        "ts" | "js" | "py" | "java" | "go" | "rs" | "vue" | "sql" => "code",
        _ => "other",
    }
}

/// 明确不是「要归档的文档」的扩展名。
///
/// 与前端 src/core/skip.ts 的 NON_DOC_EXT 保持一致（改一处必须改两处）。
/// 判断标准是「读不出正文 / 判断不了事项」：快捷方式、程序、压缩包、系统文件。
/// 真实数据里这些能占到未归类文件的一半以上，收进来只会淹掉真正要处理的文件。
const NON_DOC_EXT: &[&str] = &[
    // 快捷方式与可执行
    "lnk", "exe", "msi", "dll", "sys", "com", "scr", "bat", "cmd", "ps1", "reg", "vbs",
    // 系统与临时
    "ini", "tmp", "temp", "thumbcache", "db", "dat", "bak1",
    // 压缩包：读不出正文，归类只能靠猜
    "zip", "rar", "7z", "gz", "tar", "iso", "cab",
    // 二进制 / 编译产物
    "pyc", "class", "jar", "so", "dylib", "obj", "o", "bin", "apk", "dmg",
];

/// 系统自己生成的固定文件名
const NON_DOC_NAMES: &[&str] = &["thumbs.db", "ehthumbs.db", "desktop.ini"];

/// 无扩展名的文件不武断排除：可能是不带后缀的正文文件
pub fn is_non_document(name: &str, ext: &str) -> bool {
    let lower = name.to_lowercase();
    if NON_DOC_NAMES.contains(&lower.as_str()) {
        return true;
    }
    if ext.is_empty() {
        return false;
    }
    NON_DOC_EXT.contains(&ext)
}

fn is_plain_text(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "md" | "csv" | "log" | "json" | "yml" | "yaml" | "xml" | "ini" | "ts" | "js"
            | "py" | "java" | "go" | "rs" | "vue" | "sql" | "html" | "css"
    )
}

/// 把「两种 kind 词汇表」统一到 extract 内部这一套（doc/xls/ppt/pdf/…）
///
/// 全仓有两套 kind：`tag::kind_of_ext` 给库和界面用（word/excel/ppt），
/// `extract::kind_of` 给解析器用（doc/xls/ppt）。2026-11-20 接深度标签时踩到：
/// 直接把库里的 `word` 传给 `extract()`，Office 分支一个都不匹配，
/// **所有 docx/xlsx 都返回空串**，而且不报错 —— 表现为「这些文件读了但没结论」。
/// 所以映射放在 extract 入口，谁来调都不会静默失效。
fn norm_kind(kind: &str, ext: &str) -> &'static str {
    match kind {
        "word" | "text" => "doc",
        "excel" => "xls",
        "ppt" => "ppt",
        "pdf" => "pdf",
        "image" => "image",
        "diagram" | "drawio" => "drawio",
        _ => kind_of(ext),
    }
}

/// 抽取一份文件的正文特征
pub fn extract(path: &Path, ext: &str, kind: &str, size: u64) -> (String, Vec<String>, bool) {
    let kind = norm_kind(kind, ext);
    if is_plain_text(ext) {
        let f = match File::open(path) {
            Ok(f) => f,
            Err(_) => return (String::new(), Vec::new(), false),
        };
        let mut buf = Vec::new();
        if f.take(PLAIN_CAP).read_to_end(&mut buf).is_err() {
            return (String::new(), Vec::new(), false);
        }
        let text = String::from_utf8_lossy(&buf).to_string();
        let s: String = text.chars().take(TEXT_CAP).collect();
        return (s, Vec::new(), false);
    }

    if kind == "drawio" {
        // 这一路是把整个文件当字符串读，体积闸留在这里
        if size > MAX_PARSE_BYTES {
            return (String::new(), Vec::new(), false);
        }
        let mut s = String::new();
        if let Ok(mut f) = File::open(path) {
            if f.read_to_string(&mut s).is_ok() {
                let (nodes, err) = parse_drawio(&s);
                return (nodes.join(" "), nodes, err);
            }
        }
        return (String::new(), Vec::new(), true);
    }

    if kind == "doc" || kind == "xls" || kind == "ppt" {
        return (office_text(path, kind), Vec::new(), false);
    }

    if kind == "pdf" {
        // 扫描件没有文字层：返回空串，交给「未解读」，绝不硬猜
        return (pdf_text(path), Vec::new(), false);
    }

    (String::new(), Vec::new(), false)
}
