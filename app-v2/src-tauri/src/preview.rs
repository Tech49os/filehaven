//! 整理台的「看一眼内容」：正文摘要 / 图片交给缩略图 / 读不出来就说清为什么
//!
//! 起因（2026-11-20 实测反馈）：「分拣文件时，我只能看到文件名称，他的路径和内容我看不到」。
//! 文件名经常是 `IMG_2043.jpg`、`扫描件001.pdf`、`2024(2).docx` 这种，光看名字只能瞎猜，
//! 逼得用户「先打开文件看一眼 → 切回来 → 再判断」，一趟两三次切换。
//!
//! 这一层只做**读**，不做渲染决定：
//! - 有正文 → `kind = "text"`，前端按段落显示；
//! - 图片   → `kind = "image"`，前端走 asset 协议把原图当缩略图显示（不在这里解码，
//!            免得为了一个 300px 的预览把 4000px 的照片读进内存再 downscale）；
//! - 其余   → `kind = "none"` + 一句人话，说清是「PDF 不解析」还是「文件不在了」。
//!
//! 抽取本身复用 `extract`（P0 就写好、P1 一直没接线的模块），所以支持
//! docx/xlsx/pptx、纯文本、draw.io。PDF 与扫描件不解析——假的正文摘要比没有更误导。

use crate::extract;
use serde::Serialize;
use std::path::Path;

/// 卡片里显示的正文上限（字）
///
/// 700 字在卡片里约 10 行，够看出「这是哪一单业务」；再多就会把候选目录和按钮挤到屏幕外，
/// 而整理台的目的是判定，不是阅读。要看全文就点「打开」。
pub const PREVIEW_CAP: usize = 700;

/// 一份文件的预览结果（前端 `FilePreview`）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub text: String,
    /// 正文超长被截断了（前端据此给「展开 / 打开看全文」）
    pub truncated: bool,
    /// 没有正文时的说明；有正文时为空串
    pub reason: String,
    /// `text` / `image` / `none`
    pub kind: String,
}

impl Preview {
    fn none(reason: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            truncated: false,
            reason: reason.into(),
            kind: "none".into(),
        }
    }
}

/// 把扫描时记下的 `kind`（word/excel/ppt/pdf/image/diagram/other）翻成抽取器认的值
fn extract_kind(kind: &str) -> &'static str {
    match kind {
        "word" => "doc",
        "excel" => "xls",
        "ppt" => "ppt",
        "diagram" => "drawio",
        _ => "other",
    }
}

/// 老版二进制 Office（97-2003）与 WPS 二进制格式的扩展名
///
/// 这些格式不是 zip 包，我们没有解析器。**必须单独说清楚**，
/// 否则会掉进「读不出正文（可能是扫描件、空文档或加密文件）」那句里 ——
/// 那是把「我们不会」说成「你的文件有问题」，用户会去翻文件、重装 Office。
const LEGACY_EXT: &[&str] = &["doc", "ppt", "xls", "wps", "et", "dps"];

/// 读一份文件的预览
///
/// `kind` / `ext` / `size` 都来自索引库（`FileRow`），不现场再猜一遍：
/// 扫描时怎么判的，这里就怎么用，避免两处规则漂移。
pub fn make(path: &Path, kind: &str, ext: &str, size: u64) -> Preview {
    if !path.exists() {
        return Preview::none("文件不在了（可能被移走或删除）");
    }
    if kind == "image" {
        // 缩略图由前端用 absPath 直接喂给 <img>，这里不需要文字
        return Preview {
            text: String::new(),
            truncated: false,
            reason: String::new(),
            kind: "image".into(),
        };
    }
    if kind == "pdf" {
        return Preview::none("PDF 这一版不抽正文，直接打开看最快");
    }
    if LEGACY_EXT.contains(&ext.to_ascii_lowercase().as_str()) {
        return Preview::none(format!(
            "老版 .{ext} 是二进制格式，这版还解析不了，直接打开看"
        ));
    }

    let (raw, _, _) = extract::extract(path, ext, extract_kind(kind), size);
    let text = raw.trim();

    // PPT/Excel 抽出来常常是一串空格分隔的词，压一下更易读；纯文本保留换行
    let normalized = if ext.eq_ignore_ascii_case("txt")
        || ext.eq_ignore_ascii_case("md")
        || ext.eq_ignore_ascii_case("csv")
        || ext.eq_ignore_ascii_case("log")
    {
        text.to_string()
    } else {
        collapse_spaces(text)
    };

    if normalized.is_empty() {
        return Preview::none("这类文件读不出正文（可能是扫描件、空文档或加密文件）");
    }

    let truncated = normalized.chars().count() > PREVIEW_CAP;
    let body: String = normalized.chars().take(PREVIEW_CAP).collect();
    Preview {
        text: body,
        truncated,
        reason: String::new(),
        kind: "text".into(),
    }
}

/// 多行压成一行（Office 抽出来的正文本来就没有段落信息，留着换行反而断断续续）
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim_end().to_string()
}
