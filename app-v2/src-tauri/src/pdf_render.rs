//! 扫描件 PDF 第 1 页 → PNG（P4 M2）
//!
//! 用 Windows.Data.Pdf：Windows 10 1809+ 系统自带，纯编译免 DLL 分发。
//! （P3 曾计划 pdfium：+8MB 原生依赖还要管 DLL 分发，弃。）
//!
//! 失败一律返回 None（加密件 / 损坏件 / 渲染失败），调用方按 P3 的 Skip 语义
//! 「明确不解读」处理，绝不报错打断队列。

/// 目标分辨率：150 DPI 对标签判定足够（页面缩放 150/96 ≈ 1.5625）
const TARGET_DPI: f64 = 150.0;
/// 长边上限：视觉模型吃得住的尺寸，再大也判不出更多内容
const MAX_SIDE_PX: f64 = 1344.0;

/// 渲染第 1 页为 PNG；失败返回 None。任何 WinRT 错误都不冒泡。
#[cfg(windows)]
pub fn render_first_page(pdf_path: &str) -> Option<Vec<u8>> {
    use windows::Data::Pdf::{IPdfPage, PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::{BitmapDecoder, BitmapEncoder};
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Storage::StorageFile;

    // GetFileFromPathAsync 不吃 `..` 相对段，先规范化成干净绝对路径
    // （dunce 出来的不带 \\?\ 前缀，WinRT 才认）
    let Ok(clean) = dunce::canonicalize(pdf_path) else {
        return None;
    };
    let inner = || -> Result<Vec<u8>, windows::core::Error> {
        let path = windows::core::HSTRING::from(clean.to_string_lossy().to_string());
        let file = StorageFile::GetFileFromPathAsync(&path)?.get()?;
        let doc = PdfDocument::LoadFromFileAsync(&file)?.get()?;
        if doc.PageCount()? == 0 {
            return Err(windows::core::Error::empty());
        }
        let page = doc.GetPage(0)?;

        // 目标尺寸：150 DPI（MediaBox 是 PDF 点，1 pt = 1/72 英寸），长边封顶 1344
        let media = page.Dimensions()?.MediaBox()?;
        let mut scale = TARGET_DPI / 72.0;
        let long = media.Width.max(media.Height) * scale as f32;
        if long > MAX_SIDE_PX as f32 {
            scale *= MAX_SIDE_PX / long as f64;
        }
        let w = ((media.Width * scale as f32).round() as u32).max(64);
        let h = ((media.Height * scale as f32).round() as u32).max(64);

        let rendered = InMemoryRandomAccessStream::new()?;
        let opts = PdfPageRenderOptions::new()?;
        opts.SetDestinationWidth(w)?;
        opts.SetDestinationHeight(h)?;

        // windows 0.58 只生成了 1 参的 RenderToStreamAsync 安全包装；
        // 带分辨率选项的 RenderWithOptionsToStreamAsync 在同一个 vtable 里，手动调
        let ipage: IPdfPage = windows::core::Interface::cast(&page)?;
        let action: windows::Foundation::IAsyncAction = unsafe {
            let vtbl = windows::core::Interface::vtable(&ipage);
            let mut result__ = core::mem::zeroed();
            (vtbl.RenderWithOptionsToStreamAsync)(
                windows::core::Interface::as_raw(&ipage),
                windows::core::Interface::as_raw(&rendered),
                windows::core::Interface::as_raw(&opts),
                &mut result__,
            )
            .and_then(|| windows::core::Type::from_abi(result__))
        }?;
        action.get()?;

        // RenderToStreamAsync 的输出要经 BitmapDecoder 解码再转 PNG
        let decoded = BitmapDecoder::CreateAsync(&rendered)?.get()?;
        let bmp = decoded.GetSoftwareBitmapAsync()?.get()?;

        let png = InMemoryRandomAccessStream::new()?;
        let encoder =
            BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId()?, &png)?.get()?;
        encoder.SetSoftwareBitmap(&bmp)?;
        encoder.FlushAsync()?.get()?;

        // 读出 PNG 字节
        let size = png.Size()? as u32;
        let reader = DataReader::CreateDataReader(&png)?;
        reader.LoadAsync(size)?.get()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf)?;
        Ok(buf)
    };

    match inner() {
        Ok(b) if b.starts_with(&[0x89, b'P', b'N', b'G']) => Some(b),
        Ok(_) => {
            eprintln!("[pdf_render] {pdf_path}: 渲染产物不是 PNG");
            None
        }
        Err(e) => {
            eprintln!("[pdf_render] {pdf_path}: {e}");
            None
        }
    }
}

/// 非 Windows 编译占位：永远 None（应用只在 Windows 发布）
#[cfg(not(windows))]
pub fn render_first_page(_pdf_path: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    /// 真夹具（pymupdf 从真实语料图合成的无文字层 PDF）整条链路
    #[cfg(windows)]
    #[test]
    fn renders_first_page_of_real_scanned_pdf() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test/fixtures/scanned-sample.pdf");
        if !std::path::Path::new(path).is_file() {
            eprintln!("夹具缺失（生成脚本还没跑），跳过");
            return;
        }
        let png = super::render_first_page(path).expect("真实扫描件必须能渲染");
        assert!(png.len() > 10_000, "PNG 太小说明渲染了个空白：{}", png.len());
        // 解开确认尺寸合理（W 冒号后 4 字节大端）
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        assert!(w >= 400 && w <= 2000, "宽度离谱：{w}");
    }

    #[cfg(windows)]
    #[test]
    fn missing_or_broken_file_returns_none() {
        assert!(super::render_first_page("Z:/不存在/无此文件.pdf").is_none());
        let junk = std::env::temp_dir().join("fb-not-a-pdf.pdf");
        std::fs::write(&junk, b"this is not a pdf at all").unwrap();
        assert!(super::render_first_page(&junk.to_string_lossy()).is_none());
        let _ = std::fs::remove_file(&junk);
    }
}
