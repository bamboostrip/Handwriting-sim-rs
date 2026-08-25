//! 把 PDF / DOCX 渲染成「打印预览」图片，用作手写底图。
//!
//! 对应 Python 版 `core/doc_render.py`：
//! - PDF 用 pdfium 栅格化（`pdfium-render` 绑定；运行时需要 `pdfium.dll`，
//!   放在 exe 旁或系统 PATH 中。缺失时给出明确提示）
//! - DOCX 的忠实排版需要本机排版引擎：优先借助 Microsoft Word（COM 自动化，
//!   仅 Windows），其次 LibreOffice（`soffice --headless`），转成 PDF 后
//!   再走同一条栅格化链路。都没有时给出明确的安装提示。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// 文档渲染错误。
#[derive(Debug, thiserror::Error)]
pub enum DocRenderError {
    #[error("不支持的文档类型：{0}（支持 .pdf / .docx）")]
    UnsupportedExtension(String),
    #[error("PDF 没有可渲染的页面：{0}")]
    NoPages(String),
    #[error(
        "无法把 DOCX 转成打印预览：需要本机安装 Microsoft Word 或 LibreOffice。\n\
         也可以先在 Word 里把文档另存为 PDF，再直接导入 PDF。"
    )]
    DocxConversionUnavailable,
    #[error("pdfium 加载失败：{0}（请把 pdfium.dll 放到程序目录，或安装后加入 PATH）")]
    PdfiumUnavailable(String),
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for DocRenderError {
    fn from(e: std::io::Error) -> Self {
        DocRenderError::Other(e.to_string())
    }
}

/// 入口：PDF 直接渲染；DOCX 先转 PDF。返回逐页 PNG 路径（页序即列表序）。
pub fn document_to_page_images(
    path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<Vec<PathBuf>, DocRenderError> {
    let suffix = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match suffix.as_str() {
        "pdf" => pdf_to_images(path, out_dir, dpi),
        "docx" => {
            let pdf_path = docx_to_pdf(path, out_dir)?;
            pdf_to_images(&pdf_path, out_dir, dpi)
        }
        other => Err(DocRenderError::UnsupportedExtension(format!(".{other}"))),
    }
}

/// 把 PDF 逐页栅格化为 PNG，返回按页序排列的文件路径列表。
/// 对齐 Python 版 `pdf_to_images`（默认 200 DPI）。
pub fn pdf_to_images(pdf_path: &Path, out_dir: &Path, dpi: u32) -> Result<Vec<PathBuf>, DocRenderError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| DocRenderError::Other(format!("创建缓存目录失败：{e}")))?;
    let prefix = page_prefix(pdf_path);
    clear_stale_pages(out_dir, &prefix);

    let pdfium = open_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| DocRenderError::Other(format!("打开 PDF 失败：{e}")))?;
    let scale = dpi as f32 / 72.0;
    let mut paths = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
        // 目标像素尺寸：页面点数（1/72 英寸）× dpi/72
        let w_px = (page.width().value * scale).round().max(1.0) as i32;
        let h_px = (page.height().value * scale).round().max(1.0) as i32;
        let bitmap = page
            .render(w_px, h_px, None)
            .map_err(|e| DocRenderError::Other(format!("第 {} 页渲染失败：{e}", index + 1)))?;
        let image = bitmap.as_image();
        let rgb = image::DynamicImage::ImageRgb8(image.to_rgb8());
        let path = out_dir.join(format!("{prefix}_{index}.png"));
        rgb.save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| DocRenderError::Other(format!("保存 {} 失败：{e}", path.display())))?;
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(DocRenderError::NoPages(pdf_path.display().to_string()));
    }
    Ok(paths)
}

/// 页文件名前缀：文档名（不含扩展名），清理非法字符避免跨平台问题。
fn page_prefix(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_string())
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii() {
            c
        } else {
            '_'
        })
        .collect()
}

/// 清理同前缀的旧页文件，避免旧文档页数混入新导入结果。
fn clear_stale_pages(out_dir: &Path, prefix: &str) {
    if let Ok(rd) = std::fs::read_dir(out_dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(prefix) && name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// 打开 pdfium 动态库：优先 exe 目录下的 `pdfium.dll`，再尝试系统库。
fn open_pdfium(
) -> Result<pdfium_render::prelude::Pdfium, DocRenderError> {
    use pdfium_render::prelude::*;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = exe_dir {
        candidates.push(dir.join("pdfium.dll"));
        candidates.push(dir.join("libpdfium.so"));
    }
    for candidate in candidates {
        if candidate.is_file() {
            if let Ok(bindings) = Pdfium::bind_to_library(&candidate) {
                return Ok(Pdfium::new(bindings));
            }
        }
    }
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(e) => Err(DocRenderError::PdfiumUnavailable(e.to_string())),
    }
}

/// 把 DOCX 转成 PDF（Word COM 优先，LibreOffice 兜底）。对齐 Python 版 `docx_to_pdf`。
pub fn docx_to_pdf(docx_path: &Path, out_dir: &Path) -> Result<PathBuf, DocRenderError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| DocRenderError::Other(format!("创建缓存目录失败：{e}")))?;
    let stem = docx_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    let pdf_path = out_dir.join(format!("{stem}.pdf"));
    let _ = std::fs::remove_file(&pdf_path);

    #[cfg(target_os = "windows")]
    {
        let script = word_com_script(docx_path, &pdf_path);
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        match run_with_timeout(&mut cmd, Duration::from_secs(300)) {
            Ok(true) if pdf_path.is_file() => return Ok(pdf_path),
            _ => {} // Word 未安装或转换失败，继续尝试 LibreOffice
        }
    }

    let soffice = find_soffice();
    if let Some(soffice) = soffice {
        let mut cmd = Command::new(&soffice);
        cmd.args(["--headless", "--convert-to", "pdf"])
            .arg("--outdir")
            .arg(out_dir)
            .arg(docx_path);
        let _ = run_with_timeout(&mut cmd, Duration::from_secs(300));
        if pdf_path.is_file() {
            return Ok(pdf_path);
        }
    }

    Err(DocRenderError::DocxConversionUnavailable)
}

/// 查找 LibreOffice 可执行文件（PATH + Windows 常见安装目录）。
fn find_soffice() -> Option<PathBuf> {
    if let Some(p) = which("soffice") {
        return Some(p);
    }
    if let Some(p) = which("libreoffice") {
        return Some(p);
    }
    #[cfg(target_os = "windows")]
    for base in [
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
    ] {
        let p = PathBuf::from(base);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 极简 PATH 查找（避免引入 which crate）。
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let ext = cfg!(target_os = "windows").then_some(".exe");
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if let Some(ext) = ext {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 带超时的进程运行（返回是否成功退出）。Rust 标准库无内置超时，轮询实现。
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<bool, String> {
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("转换超时".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// 生成调用 Word COM 另存为 PDF 的 PowerShell 脚本（17 = wdFormatPDF）。
/// 对齐 Python 版 `_word_com_script`。
#[cfg(target_os = "windows")]
fn word_com_script(docx_path: &Path, pdf_path: &Path) -> String {
    let src = docx_path.display().to_string().replace('\'', "''");
    let dst = pdf_path.display().to_string().replace('\'', "''");
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $word = New-Object -ComObject Word.Application\n\
         $word.Visible = $false\n\
         try {{\n\
         \x20 $doc = $word.Documents.Open('{src}', $false, $true)\n\
         \x20 $doc.SaveAs([ref]'{dst}', [ref]17)\n\
         \x20 $doc.Close($false)\n\
         }} finally {{\n\
         \x20 $word.Quit()\n\
         }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_extension_rejected() {
        let err = document_to_page_images(Path::new("a.txt"), Path::new("out"), 200)
            .unwrap_err();
        assert!(matches!(err, DocRenderError::UnsupportedExtension(_)));
    }

    #[test]
    fn page_prefix_sanitized() {
        assert_eq!(page_prefix(Path::new("我的 文档:v2.pdf")), "我的_文档_v2");
    }

    #[test]
    fn pdf_to_images_renders_all_pages() {
        // 本机有 pdfium.dll（exe 旁 / 当前目录 / PATH）时验证完整栅格化链路；
        // 无 dll 的环境（CI）优雅跳过。
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("two_page.pdf");
        {
            use printpdf::PdfDocument;
            let mut doc = PdfDocument::new("handwrite-sim-test");
            let empty_ops: Vec<printpdf::Op> = Vec::new();
            doc.with_pages(vec![
                printpdf::PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), empty_ops.clone()),
                printpdf::PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), empty_ops),
            ]);
            let mut warnings = Vec::new();
            let bytes = doc.save(&printpdf::PdfSaveOptions::default(), &mut warnings);
            std::fs::write(&pdf_path, bytes).unwrap();
        }
        let out_dir = dir.path().join("pages");
        match pdf_to_images(&pdf_path, &out_dir, 100) {
            Ok(paths) => {
                assert_eq!(paths.len(), 2, "两页 PDF 应输出两张 PNG");
                for p in &paths {
                    assert!(p.is_file(), "{} 应存在", p.display());
                    let img = image::open(p).unwrap();
                    // 100 DPI 下 A4 ≈ 827×1169 px
                    assert!(
                        (img.width(), img.height()).0 > 500 && (img.width(), img.height()).1 > 700,
                        "页面尺寸异常：{:?}",
                        (img.width(), img.height())
                    );
                }
            }
            Err(DocRenderError::PdfiumUnavailable(_)) => {
                eprintln!("跳过：未找到 pdfium.dll");
            }
            Err(e) => panic!("PDF 栅格化意外失败：{e}"),
        }
    }

    #[test]
    fn docx_conversion_never_panics() {
        // 结果依赖本机是否装有 Word/LibreOffice（Word 的文本恢复甚至能把
        // 伪 docx 当纯文本打开转出 PDF），因此只验证：不 panic；
        // 成功时产物存在；失败时报错信息可读。
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.docx");
        std::fs::write(&fake, b"not a real docx").unwrap();
        match docx_to_pdf(&fake, dir.path()) {
            Ok(path) => assert!(path.is_file(), "成功时 PDF 应存在：{}", path.display()),
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}
