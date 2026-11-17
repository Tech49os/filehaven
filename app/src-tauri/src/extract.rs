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

/// 需要读正文的最大文件体积，超过只按文件名判断（防内存爆掉）
pub const MAX_PARSE_BYTES: u64 = 30 * 1024 * 1024;

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
        let mut e = zip.by_name(name).ok()?;
        let mut s = String::new();
        e.read_to_string(&mut s).ok()?;
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

fn is_plain_text(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "md" | "csv" | "log" | "json" | "yml" | "yaml" | "xml" | "ini" | "ts" | "js"
            | "py" | "java" | "go" | "rs" | "vue" | "sql" | "html" | "css"
    )
}

/// 抽取一份文件的正文特征
pub fn extract(path: &Path, ext: &str, kind: &str, size: u64) -> (String, Vec<String>, bool) {
    if size > MAX_PARSE_BYTES {
        return (String::new(), Vec::new(), false);
    }

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

    (String::new(), Vec::new(), false)
}
