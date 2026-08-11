//! 渲染引擎：编排排版 + 笔画扰动，输出整页图像。
//!
//! 对应 Python 版 `HandwritingEngine`（默认 fast 后端）的接口：
//! `render_preview` / `generate` / `save_all`。
//! 支持双路径：纯文本路径（`text`）与段落路径（`paragraphs` 非空时启用，
//! 对齐/缩进/右对齐/跨页，见 layout::layout_paragraphs）。
//! 预览对超大背景（宽 > `PREVIEW_MAX_WIDTH`）做降采样并等比缩放空间参数，导出始终全分辨率。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{RgbImage, RgbaImage};
use rand::{rngs::StdRng, SeedableRng};

use crate::core::font::FontFace;
use crate::core::layout;
use crate::core::models::{HandwritingParams, ParamsError};
use crate::core::perturb;

/// 引擎错误。
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("参数校验失败：{0}")]
    Params(#[from] ParamsError),
    #[error("字体加载失败：{0}")]
    Font(String),
    #[error("背景图片加载失败：{0}")]
    Background(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("图像处理失败：{0}")]
    Image(String),
    #[error("PDF 导出失败：{0}")]
    Pdf(String),
    #[error("页面区域过小，无法排版任何文字（请检查边距 / 字号 / 背景尺寸）")]
    TextAreaTooSmall,
}

/// 预览降采样的最大背景宽度阈值。
/// 2048 宽对 ~800px 的预览区已远超显示精度；比 4096 减少 4 倍渲染/内存开销
/// （千万像素级背景逐页缓存 4096 宽时每页可达 ~95MB）。
const PREVIEW_MAX_WIDTH: u32 = 1280;

/// 预览背景缓存（路径+修改时间 → (原始宽度, 缩略图)）。
///
/// 32MP 级背景解码+降采样需数秒；每次预览（换 seed 重画笔画）都重新处理
/// 是最大的重复开销，缓存后只有首次预览慢，后续亚秒级。最多保留 2 份，
/// 超限清空（预览场景通常只有一个背景）。
type PreviewBgEntry = Arc<(u32, RgbImage)>;
type PreviewBgMap = std::collections::HashMap<(String, u64), PreviewBgEntry>;
type PreviewBgCache = std::sync::OnceLock<std::sync::Mutex<PreviewBgMap>>;

static PREVIEW_BG_CACHE: PreviewBgCache = std::sync::OnceLock::new();

/// 背景缓存键：路径 + 文件修改时间（秒），文件被替换时自动失效。
fn bg_cache_key(path: &str) -> (String, u64) {
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (path.to_string(), mtime)
}

/// 按 `src_w`（原始背景宽度）等比缩放空间参数；无需降采样时原样返回。
fn scaled_params_for(params: &HandwritingParams, src_w: u32) -> HandwritingParams {
    if src_w <= PREVIEW_MAX_WIDTH {
        return params.clone();
    }
    let scale = PREVIEW_MAX_WIDTH as f32 / src_w as f32;
    let mut scaled = params.clone();
    for f in [
        &mut scaled.font_size, &mut scaled.line_spacing, &mut scaled.word_spacing,
        &mut scaled.left_margin, &mut scaled.right_margin,
        &mut scaled.top_margin, &mut scaled.bottom_margin,
        &mut scaled.word_spacing_sigma, &mut scaled.line_spacing_sigma,
        &mut scaled.font_size_sigma, &mut scaled.perturb_x_sigma,
        &mut scaled.perturb_y_sigma,
    ] {
        *f *= scale;
    }
    scaled.font_size = scaled.font_size.max(1.0);
    // 段落首行缩进同为空间参数，必须随预览降采样等比缩放，
    // 否则大背景预览中缩进相对字号偏大（如 2 字宽缩进显示为 4 字宽）
    for p in scaled.paragraphs.iter_mut() {
        p.first_line_indent *= scale;
    }
    scaled
}

/// 调试日志：`HANDWRITE_DEBUG=1` 时输出到 stderr（带耗时毫秒），否则静默。
/// 用于排查预览/导出卡死时的环节定位，不参与任何业务逻辑。
pub fn dbg_log(stage: &str, elapsed_ms: u128) {
    if std::env::var_os("HANDWRITE_DEBUG").is_some() {
        eprintln!("[引擎] {stage}：{elapsed_ms}ms");
    }
}

/// 渲染引擎接口。
pub trait Engine {
    /// 渲染第一页（预览用）。
    fn render_preview(&self, params: &HandwritingParams) -> Result<RgbaImage, EngineError>;
    /// 渲染全部页。
    fn render_pages(&self, params: &HandwritingParams) -> Result<Vec<RgbaImage>, EngineError>;
    /// 导出全部页到目录，返回文件路径列表。
    fn save_all(&self, params: &HandwritingParams, out_dir: &Path) -> Result<Vec<PathBuf>, EngineError>;
}

/// 默认引擎：纯 Rust 实现，seed 驱动，预览与导出同 seed 时逐像素一致。
pub struct DefaultEngine {
    seed: u64,
}

impl DefaultEngine {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl DefaultEngine {
    fn load_background(path: &str) -> Result<RgbImage, EngineError> {
        image::open(path)
            .map_err(|e| EngineError::Background(format!("{path}: {e}")))
            .map(|img| img.to_rgb8())
    }

    /// 预览专用降采样加载：宽 > `PREVIEW_MAX_WIDTH` 时重采样 + 空间参数等比缩放。
    ///
    /// 重采样用 `fast_image_resize`（SIMD + rayon 多线程）：实测 32MP 背景 Lanczos3
    /// 降采样 79s → ~2s；`image::imageops::resize` 对千万像素级输入极慢（单线程朴素实现）。
    /// 结果按路径+修改时间缓存：首次预览后，后续预览（换 seed）直接复用缩略图。
    fn load_background_for_preview(
        params: &HandwritingParams,
    ) -> Result<(RgbImage, HandwritingParams), EngineError> {
        let t0 = std::time::Instant::now();
        let key = bg_cache_key(&params.background_path);

        // 缓存命中：直接复用缩略图，仅重算缩放参数（毫秒级）
        if let Some(entry) = PREVIEW_BG_CACHE
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
            .lock()
            .unwrap()
            .get(&key)
        {
            let (src_w, thumb) = (entry.0, &entry.1);
            let scaled = scaled_params_for(params, src_w);
            dbg_log(
                &format!("背景命中缓存（原始宽 {src_w}px，缩略图 {}x{}）", thumb.width(), thumb.height()),
                t0.elapsed().as_millis(),
            );
            return Ok((thumb.clone(), scaled));
        }

        let bg = Self::load_background(&params.background_path)?;
        let src_w = bg.width();
        let scaled = scaled_params_for(params, src_w);
        let thumb = if bg.width() <= PREVIEW_MAX_WIDTH {
            bg
        } else {
            let scale = PREVIEW_MAX_WIDTH as f32 / bg.width() as f32;
            let new_height = (bg.height() as f32 * scale).round().max(1.0) as u32;
            let (src_w, src_h) = (bg.width(), bg.height());
            let src = fast_image_resize::images::Image::from_vec_u8(
                src_w,
                src_h,
                bg.into_raw(),
                fast_image_resize::PixelType::U8x3,
            )
            .map_err(|e| EngineError::Image(format!("构造重采样源图失败：{e}")))?;
            let mut dst = fast_image_resize::images::Image::new(
                PREVIEW_MAX_WIDTH,
                new_height,
                fast_image_resize::PixelType::U8x3,
            );
            let mut resizer = fast_image_resize::Resizer::new();
            resizer
                .resize(&src, &mut dst, &fast_image_resize::ResizeOptions::new())
                .map_err(|e| EngineError::Image(format!("背景降采样失败：{e}")))?;
            RgbImage::from_raw(PREVIEW_MAX_WIDTH, new_height, dst.into_vec())
                .ok_or_else(|| EngineError::Image("背景降采样输出尺寸异常".into()))?
        };
        dbg_log(
            &format!("背景加载+降采样（{}x{} → {}x{}）", src_w, thumb.height() * src_w / thumb.width().max(1), thumb.width(), thumb.height()),
            t0.elapsed().as_millis(),
        );
        {
            let mut map = PREVIEW_BG_CACHE
                .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
                .lock()
                .unwrap();
            if map.len() >= 2 {
                map.clear();
            }
            map.insert(key, Arc::new((src_w, thumb.clone())));
        }
        Ok((thumb, scaled))
    }

    /// 渲染一页（纯文本路径）。
    fn render_page_from(
        &self,
        params: &HandwritingParams,
        font: &FontFace,
        rng: &mut StdRng,
        text: &str,
        start: usize,
        background: &RgbImage,
    ) -> Result<(RgbaImage, usize), EngineError> {
        let (width, height) = (background.width() as usize, background.height() as usize);
        let t0 = std::time::Instant::now();
        let result = layout::layout_page(params, font, rng, text, start, width, height);
        let t1 = t0.elapsed().as_millis();
        let canvas =
            perturb::perturb_mask(&result.mask, width, height, params, rng, background.as_raw());
        let t2 = t0.elapsed().as_millis();
        dbg_log(
            &format!("单页排版 {t1}ms + 扰动 {t2}ms（消费 {}/{} 字）", result.consumed, text.chars().count()),
            t2,
        );
        Ok((rgba_from_rgb(&canvas, width, height), result.consumed))
    }
}

impl Engine for DefaultEngine {
    fn render_preview(&self, params: &HandwritingParams) -> Result<RgbaImage, EngineError> {
        params.validate()?;
        let font =
            FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
        let (background, scaled) = Self::load_background_for_preview(params)?;
        let mut rng = StdRng::seed_from_u64(self.seed);
        if !scaled.paragraphs.is_empty() {
            let pages = layout::layout_paragraphs(
                &scaled, &font, &mut rng, &scaled.paragraphs,
                background.width() as usize, background.height() as usize,
            );
            let canvas = perturb::perturb_mask(
                &pages[0], background.width() as usize, background.height() as usize,
                &scaled, &mut rng, background.as_raw(),
            );
            return Ok(rgba_from_rgb(&canvas, background.width() as usize, background.height() as usize));
        }
        let (page, _) = self.render_page_from(&scaled, &font, &mut rng, &params.text, 0, &background)?;
        Ok(page)
    }

    fn render_pages(&self, params: &HandwritingParams) -> Result<Vec<RgbaImage>, EngineError> {
        let t0 = std::time::Instant::now();
        params.validate()?;
        let font =
            FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
        let background = Self::load_background(&params.background_path)?;
        dbg_log(&format!("字体+背景加载完成（背景 {}x{}）", background.width(), background.height()), t0.elapsed().as_millis());
        let mut rng = StdRng::seed_from_u64(self.seed);
        if !params.paragraphs.is_empty() {
            let pages = layout::layout_paragraphs(
                params, &font, &mut rng, &params.paragraphs,
                background.width() as usize, background.height() as usize,
            );
            dbg_log(&format!("段落排版完成（{} 页，等待逐页扰动）", pages.len()), t0.elapsed().as_millis());
            let mut out = Vec::with_capacity(pages.len());
            let mut canvas = Vec::with_capacity(background.as_raw().len());
            for (index, mask) in pages.into_iter().enumerate() {
                let t1 = std::time::Instant::now();
                perturb::perturb_mask_into(
                    &mask, background.width() as usize, background.height() as usize,
                    params, &mut rng, background.as_raw(), &mut canvas,
                );
                dbg_log(&format!("第 {} 页扰动完成", index + 1), t1.elapsed().as_millis());
                out.push(rgba_from_rgb(&canvas, background.width() as usize, background.height() as usize));
            }
            return Ok(out);
        }
        let mut pages = Vec::new();
        let mut start = 0;
        let total_chars = params.text.chars().count();
        let mut page_no = 0;
        loop {
            page_no += 1;
            let (page, consumed) = self.render_page_from(params, &font, &mut rng, &params.text, start, &background)?;
            pages.push(page);
            dbg_log(&format!("第 {page_no} 页完成（消费 {consumed}/{total_chars} 字）"), t0.elapsed().as_millis());
            if consumed >= total_chars {
                break;
            }
            if consumed <= start {
                // 一页未消费任何字符（文本区过小等）：再渲染只会无限追加空白页
                return Err(EngineError::TextAreaTooSmall);
            }
            start = consumed;
        }
        dbg_log(&format!("全部 {page_no} 页渲染完成"), t0.elapsed().as_millis());
        Ok(pages)
    }

    fn save_all(&self, params: &HandwritingParams, out_dir: &Path) -> Result<Vec<PathBuf>, EngineError> {
        std::fs::create_dir_all(out_dir)?;
        let pages = self.render_pages(params)?;
        // 按页并行编码 PNG（各页独立 buffer，保存顺序仍按 index 保证文件名稳定）
        let mut files: Vec<Option<PathBuf>> = vec![None; pages.len()];
        let out_dir = out_dir.to_path_buf();
        let mut first_err: Option<EngineError> = None;
        std::thread::scope(|s| {
            let handles: Vec<_> = pages
                .into_iter()
                .enumerate()
                .map(|(index, page)| {
                    let out_dir = &out_dir;
                    s.spawn(move || {
                        let path = out_dir.join(format!("{index}.png"));
                        page.save(&path)
                            .map_err(|e| EngineError::Image(format!("保存 {path:?} 失败：{e}")))?;
                        Ok::<PathBuf, EngineError>(path)
                    })
                })
                .collect();
            for (index, handle) in handles.into_iter().enumerate() {
                let result = match handle.join() {
                    Ok(result) => result,
                    Err(_) => {
                        Err(EngineError::Image(format!("保存 {index}.png 失败：线程异常")))
                    }
                };
                match result {
                    Ok(path) => files[index] = Some(path),
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                }
            }
        });
        if let Some(e) = first_err {
            return Err(e);
        }
        let files = files.into_iter().map(|p| p.unwrap()).collect::<Vec<_>>();
        if files.is_empty() {
            return Err(EngineError::Image("未生成任何图片".into()));
        }
        Ok(files)
    }
}

/// 把 RGB 缓冲包装为 RGBA 图像（前景不透明）。
fn rgba_from_rgb(rgb: &[u8], width: usize, height: usize) -> RgbaImage {
    let mut buf = Vec::with_capacity(rgb.len() / 3 * 4);
    for px in rgb.chunks_exact(3) {
        buf.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    RgbaImage::from_raw(width as u32, height as u32, buf)
        .expect("RGB->RGBA 尺寸应一致")
}

/// 便捷入口：渲染预览并返回 RGBA 图像（供 GUI 调用）。
pub fn render_preview(params: &HandwritingParams, seed: u64) -> Result<RgbaImage, EngineError> {
    DefaultEngine::new(seed).render_preview(params)
}

/// 便捷入口：导出到目录（供 CLI 调用）。
pub fn export(params: &HandwritingParams, out_dir: &Path, seed: u64) -> Result<Vec<PathBuf>, EngineError> {
    DefaultEngine::new(seed).save_all(params, out_dir)
}

/// 便捷入口：预览全部页（与 `render_preview` 同降采样路径，供 GUI 翻页显示）。
pub fn render_all_pages_preview(
    params: &HandwritingParams,
    seed: u64,
) -> Result<Vec<RgbaImage>, EngineError> {
    let t0 = std::time::Instant::now();
    params.validate()?;
    dbg_log(&format!("参数校验通过（文本 {} 字 / 段落 {} 段）", params.text.chars().count(), params.paragraphs.len()), t0.elapsed().as_millis());
    let font =
        FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
    dbg_log("字体加载完成", t0.elapsed().as_millis());
    let (background, scaled) = DefaultEngine::load_background_for_preview(params)?;
    let (width, height) = (background.width() as usize, background.height() as usize);
    dbg_log(&format!("画布 {width}x{height}，开始排版"), t0.elapsed().as_millis());
    let mut rng = StdRng::seed_from_u64(seed);
    let engine = DefaultEngine::new(seed);

    if !scaled.paragraphs.is_empty() {
        let pages = layout::layout_paragraphs(&scaled, &font, &mut rng, &scaled.paragraphs, width, height);
        dbg_log(&format!("段落排版完成（{} 页，等待逐页扰动）", pages.len()), t0.elapsed().as_millis());
        let mut out = Vec::with_capacity(pages.len());
        let mut canvas = Vec::with_capacity(background.as_raw().len());
        for (index, mask) in pages.into_iter().enumerate() {
            let t1 = std::time::Instant::now();
            perturb::perturb_mask_into(&mask, width, height, &scaled, &mut rng, background.as_raw(), &mut canvas);
            dbg_log(&format!("第 {} 页扰动完成", index + 1), t1.elapsed().as_millis());
            out.push(rgba_from_rgb(&canvas, width, height));
        }
        dbg_log(&format!("预览渲染完成（共 {} 页）", out.len()), t0.elapsed().as_millis());
        return Ok(out);
    }
    let mut pages = Vec::new();
    let mut start = 0;
    let total_chars = params.text.chars().count();
    let mut page_no = 0;
    loop {
        page_no += 1;
        let (page, consumed) = engine.render_page_from(&scaled, &font, &mut rng, &params.text, start, &background)?;
        pages.push(page);
        dbg_log(&format!("第 {page_no} 页完成（消费 {consumed}/{total_chars} 字）"), t0.elapsed().as_millis());
        if consumed >= total_chars {
            break;
        }
        if consumed <= start {
            // 一页未消费任何字符（文本区过小等）：再渲染只会无限追加空白页
            return Err(EngineError::TextAreaTooSmall);
        }
        start = consumed;
    }
    let done_ms = t0.elapsed().as_millis();
    dbg_log(&format!("预览渲染完成（共 {page_no} 页）"), done_ms);
    Ok(pages)
}

/// 便捷入口：导出 PDF（位图层方案，300 DPI）。
///
/// 复用 `render_pages` 全分辨率渲染，逐页嵌入位图；
/// 页物理尺寸 = 像素 @ 300 DPI（A4 扫描背景 2480×3508 → 恰好 A4 页）。
pub fn export_pdf(
    params: &HandwritingParams,
    out_path: &Path,
    seed: u64,
) -> Result<(), EngineError> {
    let pages = DefaultEngine::new(seed).render_pages(params)?;
    let mut doc = printpdf::PdfDocument::new("handwrite-sim");
    let mut pdf_pages = Vec::with_capacity(pages.len());
    for page in pages {
        let (w, h) = page.dimensions();
        let raw = printpdf::RawImage::from_dynamic_image(image::DynamicImage::ImageRgba8(page))
            .map_err(EngineError::Pdf)?;
        let id = doc.add_image(&raw);
        let ops = vec![printpdf::Op::UseXobject {
            id,
            transform: printpdf::XObjectTransform {
                dpi: Some(300.0),
                ..Default::default()
            },
        }];
        pdf_pages.push(printpdf::PdfPage::new(
            printpdf::Mm(w as f32 * 25.4 / 300.0),
            printpdf::Mm(h as f32 * 25.4 / 300.0),
            ops,
        ));
    }
    doc.with_pages(pdf_pages);
    let mut warnings = Vec::new();

    // 配置 PDF 保存选项：
    // 1. 禁用图片最大尺寸限制（默认 2MB），防止大图被 printpdf 内部的最近邻算法强行降采样导致画质严重受损（产生大量锯齿）
    // 2. 强制使用 Flate 无损压缩，既保证画质 100% 不受损，又能有效压缩 PDF 体积
    let image_opt = printpdf::ImageOptimizationOptions {
        max_image_size: None,
        auto_optimize: Some(false),
        format: Some(printpdf::ImageCompression::Flate),
        ..Default::default()
    };
    let save_options = printpdf::PdfSaveOptions {
        image_optimization: Some(image_opt),
        ..Default::default()
    };

    let bytes = doc.save(&save_options, &mut warnings);

    // 启用 PDF 图像插值滤波（抗锯齿），防止 PDF 阅读器中出现严重的像素锯齿
    let mut lopdf_doc = lopdf::Document::load_mem(&bytes)
        .map_err(|e| EngineError::Pdf(format!("解析 PDF 字节失败：{e}")))?;

    for (_, object) in lopdf_doc.objects.iter_mut() {
        if let lopdf::Object::Stream(ref mut stream) = object {
            if let Ok(subtype) = stream.dict.get(b"Subtype") {
                if subtype == &lopdf::Object::Name(b"Image".to_vec()) {
                    stream.dict.set("Interpolate", lopdf::Object::Boolean(true));
                }
            }
        }
    }

    lopdf_doc.save(out_path)
        .map_err(|e| EngineError::Pdf(format!("写入 PDF 文件失败：{e}")))?;

    Ok(())
}

/// 预览专用：边界提示叠加（对齐 Python 版 `workers._bounds_overlay`）。
///
/// 非渲染区域（边距外）以 `color` 半透明着色（alpha 40），渲染区内侧画边距框线
/// （alpha 230，线宽 `max(2, w/900)`）。仅预览使用，导出不叠加。
pub fn overlay_bounds(img: &RgbaImage, params: &HandwritingParams, color: [u8; 3]) -> RgbaImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let key = bg_cache_key(&params.background_path);
    let src_w = if let Some(entry) = PREVIEW_BG_CACHE
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|g| g.get(&key).cloned())
    {
        entry.0
    } else {
        img.width()
    };
    let scaled = scaled_params_for(params, src_w);
    let left = scaled.left_margin.max(0.0) as usize;
    let top = scaled.top_margin.max(0.0) as usize;
    let right = (w as f32 - scaled.right_margin).max(0.0) as usize;
    let bottom = (h as f32 - scaled.bottom_margin).max(0.0) as usize;
    let (right, bottom) = (right.min(w), bottom.min(h));
    if right <= left || bottom <= top {
        return img.clone(); // 异常边距（渲染区为空）时原样返回
    }
    let mut out = img.clone();
    let raw = out.as_mut();
    let line_w = (2usize).max(w / 900);
    // 半透明合成（对齐 PIL alpha_composite 语义，alpha 通道保持不透明）
    let blend = |dst: &mut [u8], c: [u8; 3], a: u8| {
        let alpha = a as u32;
        dst[0] = ((dst[0] as u32 * (255 - alpha) + c[0] as u32 * alpha) / 255) as u8;
        dst[1] = ((dst[1] as u32 * (255 - alpha) + c[1] as u32 * alpha) / 255) as u8;
        dst[2] = ((dst[2] as u32 * (255 - alpha) + c[2] as u32 * alpha) / 255) as u8;
        dst[3] = 255;
    };
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let px = &mut raw[idx * 4..idx * 4 + 4];
            let in_inner = x >= left && y >= top && x < right && y < bottom;
            if !in_inner {
                blend(px, color, 40); // 非渲染区半透明着色
            }
            // 边距框线：紧贴渲染区内侧的四条边
            let on_border = (x >= left && x < right && (y < top + line_w || y >= bottom.saturating_sub(line_w)))
                || (y >= top && y < bottom && (x < left + line_w || x >= right.saturating_sub(line_w)));
            if on_border {
                blend(px, color, 230);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{Align, Paragraph};
    use image::Rgb;
    use std::fs;

    fn system_font() -> Option<PathBuf> {
        const CANDIDATES: &[&str] = &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
            r"/System/Library/Fonts/PingFang.ttc",
            r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        CANDIDATES.iter().map(|p| PathBuf::from(p.trim())).find(|p| p.is_file())
    }

    fn make_params(font: &Path, bg: &Path) -> HandwritingParams {
        HandwritingParams {
            text: "手写模拟器 Rust 引擎测试。".into(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            font_size: 30.0,
            line_spacing: 40.0,
            ..HandwritingParams::default()
        }
    }

    #[test]
    fn preview_matches_export_with_same_seed() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(400, 300);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let params = make_params(&font, &bg);

        // 预览（渲染全部页的第一页语义 = render_pages 首页）
        let pages = DefaultEngine::new(42).render_pages(&params).unwrap();
        assert!(!pages.is_empty());
        assert_eq!(pages[0].width(), 400);
        assert_eq!(pages[0].height(), 300);

        // 导出（同 seed）应逐像素一致
        let out = dir.path().join("out");
        let files = DefaultEngine::new(42).save_all(&params, &out).unwrap();
        assert_eq!(files.len(), pages.len());
        for (path, page) in files.iter().zip(pages.iter()) {
            let saved = image::open(path).unwrap().to_rgba8();
            assert_eq!(saved.as_raw(), page.as_raw());
        }

        // 存在有效前景（黑字白底）
        let gray_min = pages[0]
            .as_raw()
            .chunks_exact(4)
            .map(|px| (px[0] as u16 + px[1] as u16 + px[2] as u16) / 3)
            .min()
            .unwrap();
        assert!(gray_min < 128, "应有深色前景：{gray_min}");

        fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn paragraph_path_preview_matches_export_with_same_seed() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(400, 300);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let mut params = make_params(&font, &bg);
        params.paragraphs = vec![
            Paragraph {
                text: "第一段文字，居中。".into(),
                align: Align::Center,
                first_line_indent: 40.0,
            },
            Paragraph {
                text: "第二段文字，右对齐。".into(),
                align: Align::Right,
                first_line_indent: 0.0,
            },
        ];
        params.text = String::new();

        let pages = DefaultEngine::new(42).render_pages(&params).unwrap();
        assert!(!pages.is_empty());
        let out = dir.path().join("out");
        let files = DefaultEngine::new(42).save_all(&params, &out).unwrap();
        assert_eq!(files.len(), pages.len());
        for (path, page) in files.iter().zip(pages.iter()) {
            let saved = image::open(path).unwrap().to_rgba8();
            assert_eq!(saved.as_raw(), page.as_raw());
        }
        assert!(
            pages[0].as_raw().chunks_exact(4).any(|px| (px[0] as u16 + px[1] as u16 + px[2] as u16) / 3 < 128),
            "段落路径应有深色前景"
        );
        fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn render_preview_downsample_only_for_huge_background() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        // 5000px 宽背景（> 4096 阈值）；背景高度需足够容纳一行，
        // 否则降采样后（scale≈0.82）首行超出可绘制区导致空白页
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(5000, 600);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let params = make_params(&font, &bg);
        let page = DefaultEngine::new(1).render_preview(&params).unwrap();
        // 降采样后预览输出缩略背景尺寸（PREVIEW_MAX_WIDTH 宽）
        assert_eq!(page.width(), PREVIEW_MAX_WIDTH, "降采样后预览应输出缩略背景尺寸");
        // 预览仍应正确渲染（有深色前景）
        let gray_min = page.as_raw().chunks_exact(4).map(|px| (px[0] as u16 + px[1] as u16 + px[2] as u16) / 3).min().unwrap();
        assert!(gray_min < 128, "降采样预览应有深色前景：{gray_min}");
        fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn render_preview_no_downsample_for_normal_background() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        // 1000px 宽背景（≤ PREVIEW_MAX_WIDTH 阈值）：不应降采样，输出应与背景同尺寸
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(1000, 300);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let params = make_params(&font, &bg);
        let page = DefaultEngine::new(1).render_preview(&params).unwrap();
        assert_eq!(page.width(), 1000, "≤PREVIEW_MAX_WIDTH 背景不应降采样");
        assert_eq!(page.height(), 300);
        // 预览仍应正确渲染（有深色前景）
        let gray_min = page
            .as_raw()
            .chunks_exact(4)
            .map(|px| (px[0] as u16 + px[1] as u16 + px[2] as u16) / 3)
            .min()
            .unwrap();
        assert!(gray_min < 128, "预览应有深色前景：{gray_min}");
        fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn render_preview_rejects_invalid_params() {
        let params = HandwritingParams::default();
        assert!(matches!(
            render_preview(&params, 1),
            Err(EngineError::Params(ParamsError::NoText))
        ));
    }

    #[test]
    fn render_all_pages_preview_first_matches_render_preview() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(400, 200);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        // 长文本：必然跨多页
        let mut params = make_params(&font, &bg);
        params.text = "这是第一页的内容，需要足够长才能触发换页。第二行继续。第三行再来一些。第四行补充。".into();
        params.font_size = 36.0;
        params.line_spacing = 40.0;

        let preview = render_preview(&params, 7).unwrap();
        let pages = render_all_pages_preview(&params, 7).unwrap();
        assert!(pages.len() >= 2, "长文本应产生多页，实际 {}", pages.len());
        assert_eq!(pages[0].as_raw(), preview.as_raw(), "首帧应与 render_preview 逐像素一致");
        for p in &pages {
            assert_eq!(p.dimensions(), preview.dimensions());
        }
        fs::remove_dir_all(dir.path()).ok();
    }

    /// 文本区过小（小背景 + 默认边距）时，多页循环必须终止而不是无限追加空白页。
    /// 回归：点击预览导致 GUI 卡死（渲染线程死循环）。
    #[test]
    fn render_pages_terminates_on_tiny_text_area() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bg = dir.path().join("bg.png");
        // 64x64 小背景：默认边距(30) + 行距(48) + 字号(36) 下文本区为空
        let mut img = RgbImage::new(64, 64);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let params = make_params(&font, &bg);
        // 不允许死循环：若死循环此处会挂起（配合 cargo test 超时观察）
        let pages = DefaultEngine::new(42).render_pages(&params);
        assert!(
            matches!(pages, Err(EngineError::TextAreaTooSmall)),
            "导出路径应明确报错而不是无限追加空白页，实际 {pages:?}"
        );

        let preview = render_all_pages_preview(&params, 42);
        assert!(
            matches!(preview, Err(EngineError::TextAreaTooSmall)),
            "预览路径应明确报错，实际 {preview:?}"
        );
        fs::remove_dir_all(dir.path()).ok();
    }

    /// 行距+字号为 0 时 layout_page 的 y 不再推进，必须被校验拦截。
    #[test]
    fn validate_rejects_zero_total_line_spacing() {
        let dir = tempfile::tempdir().unwrap();
        let font = dir.path().join("font.ttf");
        let bg = dir.path().join("bg.png");
        std::fs::write(&font, b"dummy").unwrap();
        std::fs::write(&bg, b"dummy").unwrap();
        let p = HandwritingParams {
            text: "你好".into(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            font_size: 0.0,
            line_spacing: 0.0,
            ..HandwritingParams::default()
        };
        assert!(matches!(p.validate(), Err(ParamsError::NoLineSpacing)));
    }

    /// 预览降采样必须等比缩放段落首行缩进：
    /// 未缩放时大背景预览中「2 字宽」缩进会显示为约 4 字宽（缩进像素值不随字号缩小）。
    #[test]
    fn scaled_params_scales_paragraph_first_line_indent() {
        let mut params = HandwritingParams {
            text: "你好".into(),
            font_size: 140.0,
            ..HandwritingParams::default()
        };
        params.paragraphs = vec![Paragraph {
            text: "第一段".into(),
            align: Align::Left,
            first_line_indent: 280.0, // 2 字宽（2 × font_size 140）
        }];
        // 背景宽 2560（> PREVIEW_MAX_WIDTH 1280）→ scale = 0.5
        let scaled = scaled_params_for(&params, 2560);
        assert_eq!(scaled.font_size, 70.0);
        assert_eq!(
            scaled.paragraphs[0].first_line_indent, 140.0,
            "缩进应随字号等比缩放，保持 2 字宽"
        );
        // 未超阈值：不缩放
        let unscaled = scaled_params_for(&params, 1024);
        assert_eq!(unscaled.paragraphs[0].first_line_indent, 280.0);
    }

    #[test]
    fn overlay_bounds_tints_outside_and_draws_border() {
        let mut img = RgbaImage::new(120, 100);
        for px in img.pixels_mut() {
            *px = image::Rgba([255, 255, 255, 255]);
        }
        let params = HandwritingParams {
            left_margin: 10.0,
            top_margin: 10.0,
            right_margin: 10.0,
            bottom_margin: 10.0,
            ..HandwritingParams::default()
        };
        let out = overlay_bounds(&img, &params, [0, 200, 0]);
        let raw = out.as_raw();
        let px = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * 120 + x) * 4;
            [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]
        };
        // 非渲染区（边距外）浅着色：alpha 40 → 绿色分量略升
        let corner = px(3, 3);
        assert!(corner[1] > 240, "边距外应被浅着色：{corner:?}");
        // 渲染区中心保持白色
        let center = px(60, 50);
        assert_eq!(center, [255, 255, 255, 255], "渲染区应保持原色");
        // 边框线强着色：alpha 230 → 更接近提示色（绿色分量明显低于浅着色区）
        let border = px(60, 10);
        assert!(border[1] < 220, "边框线应强着色：{border:?}");
        assert!(corner[1] > border[1], "边框线应比边距外更接近提示色");
        // alpha 通道保持不透明
        assert_eq!(out.as_raw()[3], 255);
    }

    #[test]
    fn export_pdf_produces_valid_multipage_pdf() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(400, 200);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let mut params = make_params(&font, &bg);
        params.text = "这是第一页的内容，需要足够长才能触发换页。第二行继续。第三行再来一些。第四行补充。".into();
        params.font_size = 36.0;
        params.line_spacing = 40.0;

        let pages = DefaultEngine::new(7).render_pages(&params).unwrap();
        assert!(pages.len() >= 2, "长文本应产生多页，实际 {}", pages.len());

        let out = dir.path().join("out.pdf");
        export_pdf(&params, &out, 7).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "应以 %PDF- 开头");
        assert!(bytes.len() > 1000, "PDF 应包含图像数据");

        // 用 printpdf 读回验证页数与页尺寸
        let mut warnings = Vec::new();
        let doc = printpdf::PdfDocument::parse(&bytes, &printpdf::PdfParseOptions::default(), &mut warnings)
            .unwrap_or_else(|e| panic!("PDF 解析失败：{e}"));
        assert_eq!(doc.page_count(), pages.len(), "PDF 页数应与 render_pages 一致");

        // 验证生成的 PDF 中所有 image 对象的 Interpolate 都被设为 true，且尺寸未被压缩降采样
        let lopdf_doc = lopdf::Document::load_mem(&bytes).unwrap();
        let mut image_count = 0;
        let (w, h) = pages[0].dimensions();
        for (_, object) in lopdf_doc.objects.iter() {
            if let lopdf::Object::Stream(ref stream) = object {
                if let Ok(subtype) = stream.dict.get(b"Subtype") {
                    if subtype == &lopdf::Object::Name(b"Image".to_vec()) {
                        image_count += 1;
                        let interpolate = stream.dict.get(b"Interpolate").unwrap();
                        assert_eq!(interpolate, &lopdf::Object::Boolean(true), "图像的 Interpolate 标志应为 true");
                        
                        let width = stream.dict.get(b"Width").unwrap().as_i64().unwrap();
                        let height = stream.dict.get(b"Height").unwrap().as_i64().unwrap();
                        assert_eq!(width, w as i64, "图像宽度应与原始页面宽度一致");
                        assert_eq!(height, h as i64, "图像高度应与原始页面高度一致");
                    }
                }
            }
        }
        assert!(image_count > 0, "PDF 应包含至少一个图像对象");

        // 页物理尺寸 ≈ 像素 @ 300 DPI。printpdf 0.12 的 PdfPage 无 width/height
        // 字段，页尺寸在 media_box: Rect 中，单位 Pt（1/72 英寸）
        let (w, h) = pages[0].dimensions();
        let page = doc.pages.first().expect("至少一页");
        let expect_w_pt = w as f32 * 72.0 / 300.0;
        let expect_h_pt = h as f32 * 72.0 / 300.0;
        assert!(
            (page.media_box.width.0 - expect_w_pt).abs() < 0.1,
            "页宽 {} vs {expect_w_pt}",
            page.media_box.width.0
        );
        assert!(
            (page.media_box.height.0 - expect_h_pt).abs() < 0.1,
            "页高 {} vs {expect_h_pt}",
            page.media_box.height.0
        );
        fs::remove_dir_all(dir.path()).ok();
    }
}
