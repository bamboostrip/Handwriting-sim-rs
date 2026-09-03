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
use crate::core::models::{Align, HandwritingParams, ParamsError, Paragraph, TextRegion};
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
    #[error("文档底图渲染失败：{0}")]
    Doc(String),
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
    // 框选区域同样按比例缩放到预览坐标（对齐 Python 版 `_scale_params_for_preview`：
    // 区域矩形 × scale、区域字号 × scale；深拷贝不污染原始参数）
        for r in scaled.regions.iter_mut() {
            r.x = (r.x as f64 * scale as f64).round() as i32;
            r.y = (r.y as f64 * scale as f64).round() as i32;
            r.w = ((r.w as f64 * scale as f64).round() as i32).max(1);
            r.h = ((r.h as f64 * scale as f64).round() as i32).max(1);
            if r.font_size > 0 {
                r.font_size = ((r.font_size as f64 * scale as f64).round() as i32).max(1);
            }
            for p in r.paragraphs.iter_mut() {
                p.first_line_indent *= scale;
            }
            // 检测行距同为空间参数（PDF 像素），必须随预览降采样等比缩放，
            // 否则预览中区域行距相对字号偏大、与导出行数不一致
            if let Some(m) = r.line_spacing.as_mut() {
                *m *= scale;
            }
            if let Some(m) = r.margin_top.as_mut() { *m *= scale; }
            if let Some(m) = r.margin_bottom.as_mut() { *m *= scale; }
            if let Some(m) = r.margin_left.as_mut() { *m *= scale; }
            if let Some(m) = r.margin_right.as_mut() { *m *= scale; }
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

    // ------------------------------------------------------------------
    // 多页背景与区域合成
    // ------------------------------------------------------------------

    /// 第 index 页（0 基）背景文件路径：多页文档超出自身页数时复用最后一页；
    /// 无多页背景时恒为 `background_path`。对齐 Python 版 `_page_background` 取路逻辑。
    fn background_page_path(params: &HandwritingParams, index: usize) -> String {
        if params.background_pages.is_empty() {
            params.background_path.clone()
        } else {
            params.background_pages[index.min(params.background_pages.len() - 1)].clone()
        }
    }

    /// 把背景图缩放到统一页面尺寸（某页与首页尺寸不同时对齐首页，保证排版坐标一致）。
    fn resize_to_size(img: RgbImage, width: u32, height: u32) -> RgbImage {
        if img.width() == width && img.height() == height {
            return img;
        }
        image::imageops::resize(&img, width, height, image::imageops::FilterType::Lanczos3)
    }

    /// 构造区域局部的渲染参数：独立字体/字号；逐区域排版/扰动/错字覆盖项；
    /// 支持继承关联的 HandwritingRole 属性；
    /// 打印体关闭全部扰动（覆盖项之后应用，打印语义始终占优）。
    /// 区域以矩形自身为界，不再叠加整页边距（对齐 Python 版 `_region_params`）。
    fn region_local_params(params: &HandwritingParams, region: &TextRegion) -> HandwritingParams {
        let mut rp = params.clone();
        rp.text = region.text.clone();
        rp.paragraphs = Vec::new();
        rp.regions = Vec::new();
        rp.left_margin = region.margin_left.unwrap_or(0.0);
        rp.right_margin = region.margin_right.unwrap_or(0.0);
        rp.top_margin = region.margin_top.unwrap_or(0.0);
        rp.bottom_margin = region.margin_bottom.unwrap_or(0.0);

        let mut is_printed = region.printed;

        // 1. 继承 HandwritingRole (匹配 region.role_id)
        let role = params.roles.iter().find(|r| r.id == region.role_id && r.id != 0);
        if let Some(role) = role {
            if !role.font_path.is_empty() && region.font_path.is_empty() {
                rp.font_path = role.font_path.clone();
            }
            if let Some(fill) = role.fill {
                if region.fill.is_none() {
                    rp.fill = fill;
                }
            }
            if role.printed || role.id == 1 {
                is_printed = true;
            }
            if let Some(fs) = role.font_size {
                if region.font_size == 0 {
                    rp.font_size = fs;
                }
            }
            if let Some(ws) = role.word_spacing {
                if region.word_spacing.is_none() {
                    rp.word_spacing = ws;
                }
            }
            if let Some(s) = role.font_size_sigma {
                if region.font_size_sigma.is_none() {
                    rp.font_size_sigma = s;
                }
            }
            if let Some(s) = role.word_spacing_sigma {
                if region.word_spacing_sigma.is_none() {
                    rp.word_spacing_sigma = s;
                }
            }
            if let Some(s) = role.line_spacing_sigma {
                if region.line_spacing_sigma.is_none() {
                    rp.line_spacing_sigma = s;
                }
            }
            if let Some(s) = role.perturb_x_sigma {
                if region.perturb_x_sigma.is_none() {
                    rp.perturb_x_sigma = s;
                }
            }
            if let Some(s) = role.perturb_y_sigma {
                if region.perturb_y_sigma.is_none() {
                    rp.perturb_y_sigma = s;
                }
            }
            if let Some(s) = role.perturb_theta_sigma {
                if region.perturb_theta_sigma.is_none() {
                    rp.perturb_theta_sigma = s;
                }
            }
            if let Some(r) = role.miswrite_rate {
                if region.miswrite_rate.is_none() {
                    rp.miswrite_rate = r;
                }
            }
            if let Some(st) = role.miswrite_strikeout_style {
                if region.miswrite_strikeout_style.is_none() {
                    rp.miswrite_strikeout_style = st;
                }
            }
        }

        // 2. 应用区域级显式覆盖项
        if !region.font_path.is_empty() {
            rp.font_path = region.font_path.clone();
        }
        if region.font_size > 0 {
            rp.font_size = region.font_size as f32;
        }

        // 行间距处理：
        // - 若 region.line_spacing 为 Some(v)，则 rp.line_spacing = v；
        // - 若 region.line_spacing 为 None：
        //   - 若关联 role 指定了 line_spacing，则继承 role.line_spacing；
        //   - 若为多行区域 (h > font_size * 2.0)，自动设置自然行间距 (font_size * 0.35).round()；
        //   - 若为单行区域 (h <= font_size * 2.0)，行间距为 0.0。
        if let Some(v) = region.line_spacing {
            rp.line_spacing = v;
        } else if let Some(role_ls) = role.and_then(|r| r.line_spacing) {
            rp.line_spacing = role_ls;
        } else if region.h as f32 > rp.font_size * 2.0 {
            rp.line_spacing = (rp.font_size * 0.35).round();
        } else {
            rp.line_spacing = 0.0;
        }
        if let Some(v) = region.word_spacing_sigma {
            rp.word_spacing_sigma = v;
        }
        if let Some(v) = region.line_spacing_sigma {
            rp.line_spacing_sigma = v;
        }
        if let Some(v) = region.font_size_sigma {
            rp.font_size_sigma = v;
        }
        if let Some(v) = region.perturb_x_sigma {
            rp.perturb_x_sigma = v;
        }
        if let Some(v) = region.perturb_y_sigma {
            rp.perturb_y_sigma = v;
        }
        if let Some(v) = region.perturb_theta_sigma {
            rp.perturb_theta_sigma = v;
        }
        if let Some(v) = region.miswrite_rate {
            rp.miswrite_rate = v;
        }
        if let Some(s) = region.miswrite_strikeout_style {
            rp.miswrite_strikeout_style = s;
        }
        if let Some(c) = region.fill {
            rp.fill = c;
        }
        if region.printed {
            is_printed = true;
        }

        // 段落/对齐/首行缩进：
        if !region.paragraphs.is_empty() {
            rp.paragraphs = region.paragraphs.clone();
            rp.text.clear();
        } else if region.align != 0 || region.indent_em > 0.0 {
            rp.paragraphs = vec![Paragraph {
                text: region.text.clone(),
                align: match region.align {
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Left,
                },
                first_line_indent: region.indent_em * rp.font_size,
                font_family: None,
                runs: Vec::new(),
            }];
        } else if region.text.contains('\n') {
            rp.paragraphs = region
                .text
                .split('\n')
                .map(|t| Paragraph {
                    text: t.to_string(),
                    align: Align::Left,
                    first_line_indent: 0.0,
                    font_family: None,
                    runs: Vec::new(),
                })
                .collect();
        } else {
            rp.text = region.text.clone();
        }

        // 打印体：零扰动、零错字（优先于任何覆盖项）
        if is_printed {
            rp.word_spacing_sigma = 0.0;
            rp.line_spacing_sigma = 0.0;
            rp.font_size_sigma = 0.0;
            rp.perturb_x_sigma = 0.0;
            rp.perturb_y_sigma = 0.0;
            rp.perturb_theta_sigma = 0.0;
            rp.miswrite_rate = 0.0;
        }

        // 噪声类参数按区域字号相对主字号的比例缩放：sigma 是绝对像素值，在主
        // 文字字号下调出来的数值直接套到小字号区域会相对放大数倍（视觉上字被
        // "摇散"）。均值类（字间距/行距）与旋转（弧度量）保持原值不缩放。
        let k = rp.font_size / params.font_size.max(1.0);
        if (k - 1.0).abs() > 1e-3 {
            rp.word_spacing_sigma *= k;
            rp.line_spacing_sigma *= k;
            rp.font_size_sigma *= k;
            rp.perturb_x_sigma *= k;
            rp.perturb_y_sigma *= k;
        }

        // 行距按盒高收敛：多行区域（带换行的提取文本）若按检测行距放不下，
        // 会把末尾行挤出盒外被裁掉。估计行数需计入行内换行余量（逐行 ceil），
        // 以"每行均分盒高"为上限收紧行距，保证内容完整优先于行距还原。
        let split_lines: Vec<&str> = region.text.split('\n').collect();
        if split_lines.len() >= 2 {
            let char_budget =
                ((region.w as f32 - rp.left_margin - rp.right_margin) / rp.font_size * 0.95)
                    .floor()
                    .max(1.0);
            let est_lines: usize = split_lines
                .iter()
                .map(|l| ((l.chars().count() as f32 / char_budget).ceil() as usize).max(1))
                .sum();
            let ls_fit = region.h as f32 / est_lines as f32 - rp.font_size;
            if ls_fit > 0.0 && rp.line_spacing > ls_fit {
                rp.line_spacing = ls_fit;
            }
        }
        rp
    }

    /// 每区域独立排版随机源：由主 seed 派生的确定性字符串种子
    /// （对齐 Python 版 `random.Random(f"{seed}|region{index}")` 的派生方式，
    /// 经哈希映射到 u64；相同 seed 下预览与导出完全一致）。
    fn region_seed(seed: u64, index: usize) -> u64 {
        use std::hash::{Hash, Hasher};
        let text = format!("{seed}|region{index}");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// 笔画扰动的独立随机源（区域路径专用，避免与排版流交叉影响）。
    fn perturb_seed(seed: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let text = format!("{seed}|perturb");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// 预加载参数中涉及的所有字体文件。
    fn preload_fonts(
        params: &HandwritingParams,
    ) -> Result<std::collections::HashMap<String, Arc<FontFace>>, EngineError> {
        let mut map = std::collections::HashMap::new();
        let mut to_load: Vec<(String, f32)> = Vec::new();
        if !params.font_path.is_empty() {
            to_load.push((params.font_path.clone(), params.font_size));
        }
        for role in &params.roles {
            if !role.font_path.is_empty() {
                to_load.push((
                    role.font_path.clone(),
                    role.font_size.unwrap_or(params.font_size),
                ));
            }
        }
        for para in &params.paragraphs {
            for run in para.effective_runs() {
                if let Some(font_path) = &run.style.font_path {
                    if !font_path.is_empty() {
                        to_load.push((
                            font_path.clone(),
                            run.style.font_size.unwrap_or(params.font_size),
                        ));
                    }
                }
            }
        }
        for region in &params.regions {
            let role = params.roles.iter().find(|r| r.id == region.role_id && r.id != 0);
            let font_path = if !region.font_path.is_empty() {
                Some(region.font_path.clone())
            } else if let Some(role) = role {
                if !role.font_path.is_empty() {
                    Some(role.font_path.clone())
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(path) = font_path {
                let size = if region.font_size > 0 {
                    region.font_size as f32
                } else if let Some(fs) = role.and_then(|r| r.font_size) {
                    fs
                } else {
                    params.font_size
                };
                to_load.push((path, size));
            }
            for para in &region.paragraphs {
                for run in para.effective_runs() {
                    if let Some(font_path) = &run.style.font_path {
                        if !font_path.is_empty() {
                            to_load.push((
                                font_path.clone(),
                                run.style.font_size.unwrap_or(params.font_size),
                            ));
                        }
                    }
                }
            }
        }
        for (path, size) in to_load {
            if let std::collections::hash_map::Entry::Vacant(e) = map.entry(path) {
                let font = FontFace::load(Path::new(e.key()), size).map_err(EngineError::Font)?;
                e.insert(Arc::new(font));
            }
        }
        Ok(map)
    }

    /// 主文字（text 或 paragraphs）的逐页墨迹分层掩码；无文字返回空。
    /// 返回值第二项为「排版停滞」标志：纯文本路径某页一个字都没消费
    /// （文本区过小）时为 true，调用方据此报 `TextAreaTooSmall` 而不是
    /// 无限追加空白页（保持既有回归语义）。
    fn main_page_masks(
        params: &HandwritingParams,
        font: &FontFace,
        font_map: Option<&std::collections::HashMap<String, Arc<FontFace>>>,
        rng: &mut StdRng,
        width: usize,
        height: usize,
    ) -> (Vec<layout::StyledPage>, bool) {
        if !params.paragraphs.is_empty() {
            let resolver = |p: &str| font_map.and_then(|m| m.get(p)).map(|f| f.as_ref());
            let pages = layout::layout_paragraphs_styled(
                params,
                font,
                Some(&resolver),
                rng,
                &params.paragraphs,
                width,
                height,
            );
            return (pages, false);
        }

        if params.text.trim().is_empty() {
            return (Vec::new(), false);
        }
        let mut pages = Vec::new();
        let total_chars = params.text.chars().count();
        let mut start = 0usize;
        let mut stalled = false;
        let default_style = layout::PerturbStyle::new(
            params.fill,
            false,
            params.perturb_x_sigma,
            params.perturb_y_sigma,
            params.perturb_theta_sigma,
        );
        loop {
            let result =
                layout::layout_text(params, font, rng, &params.text, start, width, height, false);
            let no_progress = result.consumed <= start;
            start = result.consumed;
            pages.push(layout::StyledPage {
                layers: vec![layout::StyledLayer {
                    style: default_style.clone(),
                    mask: result.mask,
                }],
            });
            if start >= total_chars {
                break;
            }
            if no_progress {
                stalled = true;
                break;
            }
        }
        (pages, stalled)
    }
}

/// 一个框选区域的预计算条目：局部参数、页面偏移/尺寸、单页掩码、目标页。
struct RegionEntry {
    local_params: HandwritingParams,
    ox: usize,
    oy: usize,
    rw: usize,
    rh: usize,
    mask: Vec<bool>,
    /// 目标渲染页索引（0 基）。
    target_page: usize,
}

/// 统一的多页生成器：预览（降采样参数 + 缩略底图）与导出（原始参数 +
/// 全分辨率底图）共用同一段编排逻辑，保证同 seed 输出一致。
///
/// `first_background` 为第 1 页背景（决定画布尺寸），`page_background(index)`
/// 返回第 index 页已缩放到统一尺寸的背景。
fn generate_pages_with(
    params: &HandwritingParams,
    font: Option<&FontFace>,
    seed: u64,
    first_background: RgbImage,
    page_background: &dyn Fn(usize) -> Result<RgbImage, EngineError>,
) -> Result<Vec<RgbaImage>, EngineError> {
    let width = first_background.width() as usize;
    let height = first_background.height() as usize;

    let font_map = DefaultEngine::preload_fonts(params)?;

    // ---- 主文字逐页掩码（无文字时为空 → 纯背景路径） ----
    let main_rng = &mut StdRng::seed_from_u64(seed);
    let (main_pages, stalled) = match font {
        Some(f) => {
            DefaultEngine::main_page_masks(params, f, Some(&font_map), main_rng, width, height)
        }
        None => (Vec::new(), false),
    };
    if stalled {
        return Err(EngineError::TextAreaTooSmall);
    }

    // ---- 区域条目：每区域独立字体/参数/随机源（对齐 `_pages_with_regions`） ----
    let mut entries: Vec<RegionEntry> = Vec::new();
    for (index, region) in params.regions.iter().enumerate() {
        if region.text.trim().is_empty() {
            continue;
        }
        let ox = (region.x.max(0) as usize).min(width.saturating_sub(1));
        let oy = (region.y.max(0) as usize).min(height.saturating_sub(1));
        let rw = (region.w.max(1) as usize).min(width.saturating_sub(ox)).max(1);
        let rh = (region.h.max(1) as usize).min(height.saturating_sub(oy)).max(1);
        let rp = DefaultEngine::region_local_params(params, region);
        let font_r = if let Some(cached) = font_map.get(&rp.font_path) {
            cached.clone()
        } else {
            Arc::new(
                FontFace::load(Path::new(&rp.font_path), rp.font_size)
                    .map_err(EngineError::Font)?,
            )
        };
        let rrand = &mut StdRng::seed_from_u64(DefaultEngine::region_seed(seed, index));
        // 区域排版：仅排版在所属单页内（超出框选区域的内容直接截断不跨页延伸）。
        // 排版画布高度给足 4 倍盒高，避免"最后一行放不下被分到下一页"而被
        // next() 整行丢弃——越界墨迹最终仍由 to_combined_mask 裁剪到盒高。
        let mask: Vec<bool> = if !rp.paragraphs.is_empty() {
            let resolver = |p: &str| font_map.get(p).map(|f| f.as_ref());
            layout::layout_paragraphs_styled(
                &rp,
                &font_r,
                Some(&resolver),
                rrand,
                &rp.paragraphs,
                rw,
                rh.saturating_mul(4),
            )
            .into_iter()
            .next()
            .map(|p| p.to_combined_mask(rw, rh))
            .unwrap_or_else(|| vec![false; rw * rh])
        } else {
            let result = layout::layout_text(&rp, &font_r, rrand, &region.text, 0, rw, rh, true);
            result.mask
        };
        entries.push(RegionEntry {
            local_params: rp,
            ox,
            oy,
            rw,
            rh,
            mask,
            target_page: region.page.max(1) as usize - 1,
        });
    }

    // ---- 总页数 = 主文字 / 背景页数 / 各区域所在页的最大值（至少 1 页） ----
    let n_pages = main_pages
        .len()
        .max(params.background_pages.len())
        .max(entries.iter().map(|e| e.target_page + 1).max().unwrap_or(0))
        .max(1);

    let perturb_rng = &mut StdRng::seed_from_u64(DefaultEngine::perturb_seed(seed));
    let mut out: Vec<RgbaImage> = Vec::with_capacity(n_pages);
    for page_index in 0..n_pages {
        let bg = page_background(page_index)?;
        let mut canvas = bg.as_raw().clone();
        // 区域先合成（仅在指定 target_page 渲染，重叠时主文字在上）
        for e in &entries {
            if page_index != e.target_page {
                continue;
            }
            perturb::perturb_region_into(
                &e.mask,
                e.rw,
                e.rh,
                &e.local_params,
                perturb_rng,
                e.ox,
                e.oy,
                &mut canvas,
                width,
                height,
            );
        }
        // 主文字后合成：以当前画布为底（含区域墨迹），分层扰动/打印写入
        if page_index < main_pages.len() {
            for layer in &main_pages[page_index].layers {
                if layer.mask.iter().any(|&b| b) {
                    perturb::perturb_styled_layer_into(
                        &layer.mask,
                        width,
                        height,
                        &layer.style,
                        perturb_rng,
                        &mut canvas,
                    );
                }
            }
        }
        out.push(rgba_from_rgb(&canvas, width, height));
    }
    Ok(out)
}


impl Engine for DefaultEngine {
    fn render_preview(&self, params: &HandwritingParams) -> Result<RgbaImage, EngineError> {
        params.validate_with(false)?;
        let pages = preview_pages_impl(self.seed, params)?;
        pages
            .into_iter()
            .next()
            .ok_or_else(|| EngineError::Image("未生成任何页面".into()))
    }

    fn render_pages(&self, params: &HandwritingParams) -> Result<Vec<RgbaImage>, EngineError> {
        let t0 = std::time::Instant::now();
        params.validate_with(false)?;
        let has_content = has_renderable_content(params);
        let font = load_font_if_needed(params, has_content)?;
        let background = Self::load_background(&params.background_path)?;
        dbg_log(
            &format!("字体+背景加载完成（背景 {}x{}）", background.width(), background.height()),
            t0.elapsed().as_millis(),
        );
        let first_w = background.width();
        let first_h = background.height();
        let bg_for_closure = background.clone();
        let page_background = move |index: usize| -> Result<RgbImage, EngineError> {
            if index == 0 {
                return Ok(bg_for_closure.clone());
            }
            let path = Self::background_page_path(params, index);
            let img = Self::load_background(&path)?;
            Ok(Self::resize_to_size(img, first_w, first_h))
        };
        let pages = generate_pages_with(params, font.as_ref(), self.seed, background, &page_background)?;
        dbg_log(&format!("全部 {} 页渲染完成", pages.len()), t0.elapsed().as_millis());
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

/// 是否存在可渲染内容（主文字 / 段落 / 非空区域文字）——决定是否需要加载主字体。
fn has_renderable_content(params: &HandwritingParams) -> bool {
    !params.text.trim().is_empty()
        || !params.paragraphs.is_empty()
        || params.regions.iter().any(|r| !r.text.trim().is_empty())
}

/// 有内容时加载主字体；纯背景预览允许 font_path 为空（一个字都不画）。
fn load_font_if_needed(
    params: &HandwritingParams,
    has_content: bool,
) -> Result<Option<FontFace>, EngineError> {
    if !has_content {
        return Ok(None);
    }
    FontFace::load(Path::new(&params.font_path), params.font_size)
        .map(Some)
        .map_err(EngineError::Font)
}

/// 预览全页实现：降采样背景 + 等比缩放参数后走统一生成器。
fn preview_pages_impl(seed: u64, params: &HandwritingParams) -> Result<Vec<RgbaImage>, EngineError> {
    let t0 = std::time::Instant::now();
    params.validate_with(false)?;
    dbg_log(
        &format!(
            "参数校验通过（文本 {} 字 / 段落 {} 段 / 区域 {} 个）",
            params.text.chars().count(),
            params.paragraphs.len(),
            params.regions.len()
        ),
        t0.elapsed().as_millis(),
    );
    let font = load_font_if_needed(params, has_renderable_content(params))?;
    dbg_log("字体加载完成", t0.elapsed().as_millis());
    let (background, scaled) = DefaultEngine::load_background_for_preview(params)?;
    let (width, height) = (background.width() as usize, background.height() as usize);
    dbg_log(&format!("画布 {width}x{height}，开始排版"), t0.elapsed().as_millis());
    let first_w = background.width();
    let first_h = background.height();
    let bg_for_closure = background.clone();
    let page_background = move |index: usize| -> Result<RgbImage, EngineError> {
        if index == 0 {
            return Ok(bg_for_closure.clone());
        }
        // 预览路径的后续文档页：加载原图后缩放到首页缩略图尺寸
        let path = DefaultEngine::background_page_path(params, index);
        let img = DefaultEngine::load_background(&path)?;
        Ok(DefaultEngine::resize_to_size(img, first_w, first_h))
    };
    let pages = generate_pages_with(&scaled, font.as_ref(), seed, background, &page_background)?;
    dbg_log(&format!("预览渲染完成（共 {} 页）", pages.len()), t0.elapsed().as_millis());
    Ok(pages)
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
    preview_pages_impl(seed, params)
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
    use crate::core::models::{Align, HandwritingRole, Paragraph, TextRegion, TextRun, TextRunStyle};
    use image::Rgb;
    use std::fs;


    fn system_fonts() -> Vec<PathBuf> {
        const CANDIDATES: &[&str] = &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
            r"C:\Windows\Fonts\simsun.ttc",
            r"C:\Windows\Fonts\simkai.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            r"/System/Library/Fonts/PingFang.ttc",
            r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        CANDIDATES
            .iter()
            .map(|p| PathBuf::from(p.trim()))
            .filter(|p| p.is_file())
            .collect()
    }

    fn system_font() -> Option<PathBuf> {
        system_fonts().into_iter().next()
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
                font_family: None,
                runs: Vec::new(),
            },
            Paragraph {
                text: "第二段文字，右对齐。".into(),
                align: Align::Right,
                first_line_indent: 0.0,
                font_family: None,
                runs: Vec::new(),
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
        // 纯背景预览合法后（require_text=false），默认参数在「无背景」处被拦下
        let params = HandwritingParams::default();
        assert!(matches!(
            render_preview(&params, 1),
            Err(EngineError::Params(ParamsError::NoBackground))
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
            font_family: None,
            runs: Vec::new(),
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

    // =====================================================================
    // 框选文字区域（对应 Python 版 tests/test_regions.py）
    // =====================================================================

    /// 白底黑字的墨迹掩码。
    fn region_ink_mask(page: &RgbaImage) -> Vec<bool> {
        page.as_raw()
            .chunks_exact(4)
            .map(|px| ((px[0] as u16 + px[1] as u16 + px[2] as u16) / 3) < 128)
            .collect()
    }

    /// 统计矩形区域内的墨迹像素数。
    fn inner_ink(ink: &[bool], w: usize, x: usize, y: usize, rw: usize, rh: usize) -> usize {
        let mut count = 0;
        for yy in y..(y + rh) {
            for xx in x..(x + rw) {
                if ink[yy * w + xx] {
                    count += 1;
                }
            }
        }
        count
    }

    fn region_test_params(font: &Path, dir: &tempfile::TempDir) -> HandwritingParams {
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(400, 300);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();
        HandwritingParams {
            text: String::new(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            font_size: 30.0,
            line_spacing: 40.0,
            word_spacing: 5.0,
            ..HandwritingParams::default()
        }
    }

    #[test]
    fn printed_region_ink_near_box() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let params = region_test_params(&font, &dir);
        let (bx, by, bw, bh) = (60usize, 50usize, 200usize, 120usize);
        let mut p = params;
        p.regions = vec![TextRegion {
            x: bx as i32, y: by as i32, w: bw as i32, h: bh as i32,
            text: "打印体测试文字".into(), printed: true, ..TextRegion::default()
        }];
        let image = DefaultEngine::new(7).render_preview(&p).unwrap();
        let ink = region_ink_mask(&image);
        assert!(ink.iter().any(|&b| b));
        let slack = (p.font_size * 2.0) as usize;
        let (mut min_x, mut min_y) = (usize::MAX, usize::MAX);
        let (mut max_x, mut max_y) = (0usize, 0usize);
        for (i, &v) in ink.iter().enumerate() {
            if v {
                let (x, y) = (i % 400, i / 400);
                min_x = min_x.min(x); max_x = max_x.max(x);
                min_y = min_y.min(y); max_y = max_y.max(y);
            }
        }
        assert!(min_x + slack >= bx && min_y + slack >= by, "墨迹不应远离矩形左上：{min_x},{min_y}");
        assert!(max_x <= bx + bw + slack && max_y <= by + bh + slack, "墨迹不应溢出矩形太远");
        assert!(inner_ink(&ink, 400, bx, by, bw, bh) > 0, "墨迹应与矩形相交");
    }

    #[test]
    fn handwritten_region_renders() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 200, text: "手写体区域内容".into(), ..TextRegion::default()
        }];
        let image = render_preview(&params, 42).unwrap();
        assert!(region_ink_mask(&image).iter().any(|&b| b), "手写体区域应有前景");
    }

    #[test]
    fn region_align_changes_layout() {
        // 同 seed 同文本：居中区域与左对齐区域的墨迹水平质心应明显不同
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let ink_centroid_x = |align: i32| -> f32 {
            let mut params = region_test_params(&font, &dir);
            params.regions = vec![TextRegion {
                x: 40, y: 40, w: 300, h: 120, text: "对齐测试".into(),
                align, ..TextRegion::default()
            }];
            let img = render_preview(&params, 42).unwrap();
            let mask = region_ink_mask(&img);
            let (mut sum, mut n) = (0.0f64, 0usize);
            for (i, &b) in mask.iter().enumerate() {
                if b { sum += (i % 400) as f64; n += 1; }
            }
            assert!(n > 0, "应有墨迹");
            (sum / n as f64) as f32
        };
        let left = ink_centroid_x(0);
        let center = ink_centroid_x(1);
        assert!(
            (center - left).abs() > 10.0,
            "居中与左对齐的墨迹质心应不同：left={left} center={center}"
        );
    }

    #[test]
    fn region_multi_paragraph_alignment() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 200,
            text: "你好\n张三".into(),
            paragraphs: vec![
                Paragraph { text: "你好".into(), align: Align::Center, first_line_indent: 0.0, font_family: None, runs: Vec::new() },
                Paragraph { text: "张三".into(), align: Align::Right, first_line_indent: 0.0, font_family: None, runs: Vec::new() },
            ],
            ..TextRegion::default()
        }];
        let img = render_preview(&params, 42).unwrap();
        assert!(region_ink_mask(&img).iter().any(|&b| b), "多段区域应成功渲染");
    }

    #[test]
    fn region_overrides_change_output() {
        // 同 seed 下，设置了覆盖项（字距/颜色/错字率）的区域输出应与默认不同；
        // 打印体 + 扰动覆盖应保持零扰动语义（与不带覆盖的打印体一致）。
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();

        let mut base = region_test_params(&font, &dir);
        base.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 200, text: "覆盖测试文字内容".into(),
            font_size: 28, ..TextRegion::default()
        }];
        let base_img = render_preview(&base, 42).unwrap();

        let mut overridden = region_test_params(&font, &dir);
        overridden.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 200, text: "覆盖测试文字内容".into(),
            font_size: 28,
            word_spacing: Some(24.0),
            line_spacing: Some(64.0),
            perturb_theta_sigma: Some(0.3),
            miswrite_rate: Some(0.5),
            fill: Some([200, 30, 30]),
            ..TextRegion::default()
        }];
        let over_img = render_preview(&overridden, 42).unwrap();

        assert_ne!(
            base_img.as_raw(), over_img.as_raw(),
            "设置覆盖项后渲染结果应当不同"
        );

        // 打印体忽略扰动类覆盖：带扰动覆盖的打印体 == 不带覆盖的打印体
        let mut printed_plain = region_test_params(&font, &dir);
        printed_plain.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 200, text: "打印体覆盖".into(),
            printed: true, font_size: 28, ..TextRegion::default()
        }];
        let mut printed_overridden = region_test_params(&font, &dir);
        printed_overridden.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 200, text: "打印体覆盖".into(),
            printed: true, font_size: 28,
            perturb_theta_sigma: Some(0.5),
            miswrite_rate: Some(0.9),
            ..TextRegion::default()
        }];
        assert_eq!(
            render_preview(&printed_plain, 42).unwrap().as_raw(),
            render_preview(&printed_overridden, 42).unwrap().as_raw(),
            "打印体应忽略扰动/错字类覆盖项"
        );
    }

    #[test]
    fn region_and_main_text_coexist() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.text = "这是主文字，铺满页面边距区域。".repeat(10);
        let (bx, by, bw, bh) = (150usize, 100usize, 160usize, 90usize);
        params.regions = vec![TextRegion {
            x: bx as i32, y: by as i32, w: bw as i32, h: bh as i32,
            text: "区域文字".into(), printed: true, ..TextRegion::default()
        }];
        let image = render_preview(&params, 42).unwrap();
        let ink = region_ink_mask(&image);
        assert!(inner_ink(&ink, 400, bx, by, bw, bh) > 0, "区域内应有墨迹");
        // 区域外左上角应有主文字墨迹（首行基线在 top_margin + line_spacing ≈ 70）
        assert!(inner_ink(&ink, 400, 31, 71, 109, 29) > 0, "区域外应有主文字墨迹");
    }

    #[test]
    fn region_overflow_does_not_create_extra_pages() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        let (bx, by, bw, bh) = (50usize, 40usize, 180usize, 80usize);
        params.regions = vec![TextRegion {
            x: bx as i32, y: by as i32, w: bw as i32, h: bh as i32,
            text: "很长的一段区域文字。".repeat(30),
            ..TextRegion::default()
        }];
        let pages = render_all_pages_preview(&params, 7).unwrap();
        assert_eq!(pages.len(), 1, "超出的区域文字直接截断，不应创建额外页面");
        let ink = region_ink_mask(&pages[0]);
        assert!(inner_ink(&ink, 400, bx, by, bw, bh) > 0, "第一页框内应有墨迹");
    }

    #[test]
    fn region_margin_shifts_text() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut p1 = region_test_params(&font, &dir);
        p1.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 100, text: "边距测试文本".into(),
            margin_left: Some(0.0), ..TextRegion::default()
        }];
        let img1 = render_preview(&p1, 42).unwrap();
        let mut p2 = region_test_params(&font, &dir);
        p2.regions = vec![TextRegion {
            x: 40, y: 40, w: 300, h: 100, text: "边距测试文本".into(),
            margin_left: Some(50.0), ..TextRegion::default()
        }];
        let img2 = render_preview(&p2, 42).unwrap();
        let m1 = region_ink_mask(&img1);
        let m2 = region_ink_mask(&img2);
        assert_ne!(m1, m2, "设置左边距后墨迹分布应发生改变");
    }

    #[test]
    fn region_same_seed_preview_matches_export() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.text = "主文字内容。".repeat(20);
        params.regions = vec![
            TextRegion { x: 30, y: 30, w: 200, h: 100, text: "区域一".into(), ..TextRegion::default() },
            TextRegion {
                x: 240, y: 150, w: 130, h: 110, text: "区域二".into(),
                printed: true, font_size: 24, ..TextRegion::default()
            },
        ];
        let preview_pages = render_all_pages_preview(&params, 99).unwrap();
        let out = dir.path().join("export");
        let files = DefaultEngine::new(99).save_all(&params, &out).unwrap();
        assert_eq!(files.len(), preview_pages.len());
        for (path, page) in files.iter().zip(preview_pages.iter()) {
            let saved = image::open(path).unwrap().to_rgba8();
            assert_eq!(saved.as_raw(), page.as_raw(), "同 seed 预览与导出应逐像素一致");
        }
    }

    #[test]
    fn region_only_passes_validation() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.regions = vec![TextRegion {
            x: 10, y: 10, w: 100, h: 60, text: "仅区域".into(), ..TextRegion::default()
        }];
        assert!(params.validate_with(true).is_ok(), "只有区域没有主文字也应通过校验");
    }

    #[test]
    fn region_missing_font_fails_validation() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.regions = vec![TextRegion {
            x: 10, y: 10, w: 100, h: 60, text: "字".into(),
            font_path: dir.path().join("nope.ttf").to_string_lossy().into_owned(),
            ..TextRegion::default()
        }];
        assert!(matches!(
            params.validate_with(true),
            Err(ParamsError::RegionFontMissing { .. })
        ));
    }

    #[test]
    fn region_bad_rect_and_page_fail_validation() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut p = region_test_params(&font, &dir);
        p.regions = vec![TextRegion {
            x: 10, y: 10, w: 0, h: 60, text: "字".into(), ..TextRegion::default()
        }];
        assert!(matches!(p.validate_with(true), Err(ParamsError::RegionSize { index: 1 })));
        p.regions = vec![TextRegion {
            x: 10, y: 10, w: 100, h: 60, text: "字".into(), page: 0, ..TextRegion::default()
        }];
        assert!(matches!(p.validate_with(true), Err(ParamsError::RegionPage { index: 1 })));
    }

    #[test]
    fn region_on_requested_page() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        let box1 = (40usize, 40usize, 220usize, 100usize);
        let box2 = (200usize, 150usize, 170usize, 110usize);
        params.regions = vec![
            TextRegion {
                x: box1.0 as i32, y: box1.1 as i32, w: box1.2 as i32, h: box1.3 as i32,
                text: "第一页区域".into(), printed: true, ..TextRegion::default()
            },
            TextRegion {
                x: box2.0 as i32, y: box2.1 as i32, w: box2.2 as i32, h: box2.3 as i32,
                text: "第二页区域".into(), printed: true, page: 2, ..TextRegion::default()
            },
        ];
        let pages = render_all_pages_preview(&params, 7).unwrap();
        assert!(pages.len() >= 2);
        let ink0 = region_ink_mask(&pages[0]);
        let ink1 = region_ink_mask(&pages[1]);
        assert!(inner_ink(&ink0, 400, box1.0, box1.1, box1.2, box1.3) > 0, "第一页区域应出现在第一页");
        assert_eq!(inner_ink(&ink0, 400, box2.0, box2.1, box2.2, box2.3), 0, "第二页区域不应提前出现");
        assert!(inner_ink(&ink1, 400, box2.0, box2.1, box2.2, box2.3) > 0, "第二页区域应出现在第二页");
        assert_eq!(inner_ink(&ink1, 400, box1.0, box1.1, box1.2, box1.3), 0, "第一页单页区域不应延续");
    }

    #[test]
    fn region_empty_text_skipped() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.regions = vec![
            TextRegion { x: 10, y: 10, w: 100, h: 60, text: "   ".into(), ..TextRegion::default() },
            TextRegion { x: 10, y: 100, w: 100, h: 100, text: "有效区域".into(), ..TextRegion::default() },
        ];
        let image = render_preview(&params, 42).unwrap();
        assert!(region_ink_mask(&image).iter().any(|&b| b));
    }

    #[test]
    fn region_clamped_to_page() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.regions = vec![
            TextRegion { x: 350, y: 250, w: 200, h: 150, text: "越界区域".into(), ..TextRegion::default() },
            TextRegion { x: 0, y: 0, w: 5000, h: 5000, text: "超大区域".into(), ..TextRegion::default() },
        ];
        let image = render_preview(&params, 42).unwrap();
        assert_eq!((image.width(), image.height()), (400, 300), "越界区域应被钳制不崩溃");
    }

    /// 纯背景预览：无文字、无区域时输出空白背景页而不是报「未输入文字」。
    #[test]
    fn pure_background_preview_allowed() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let params = region_test_params(&font, &dir);
        let pages = render_all_pages_preview(&params, 42).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!((pages[0].width(), pages[0].height()), (400, 300));
        // 无墨迹
        assert!(pages[0].as_raw().chunks_exact(4).all(|px| px[0] == 255));
    }

    /// 多页文档背景：background_pages 提供逐页底图，主文字只占首页，
    /// 其余页输出纯背景便于翻页浏览后再框选；尺寸不同的页面被统一缩放。
    #[test]
    fn multi_page_backgrounds_rendered_as_tail_pages() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let bg1 = dir.path().join("doc_0.png");
        let bg2 = dir.path().join("doc_1.png");
        let mut img1 = RgbImage::new(400, 300);
        for px in img1.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img1.save(&bg1).unwrap();
        // 第二页尺寸故意不同：应被统一缩放到首页尺寸
        let mut img2 = RgbImage::new(800, 600);
        for px in img2.pixels_mut() {
            *px = Rgb([250, 250, 250]);
        }
        img2.save(&bg2).unwrap();

        let mut params = region_test_params(&font, &dir);
        params.background_path = bg1.to_string_lossy().into_owned();
        params.background_pages = vec![
            bg1.to_string_lossy().into_owned(),
            bg2.to_string_lossy().into_owned(),
        ];
        params.text = "首页文字。".into();
        let pages = render_all_pages_preview(&params, 7).unwrap();
        assert_eq!(pages.len(), 2, "文档底图第二页也应输出，实际 {} 页", pages.len());
        assert_eq!((pages[1].width(), pages[1].height()), (400, 300), "第二页应统一到首页尺寸");
        // 第二页应是纯背景（浅色），没有黑字
        let ink1 = region_ink_mask(&pages[1]);
        assert!(!ink1.iter().any(|&b| b), "第二页应为纯背景无墨迹");
    }

    #[test]
    fn test_render_mixed_runs_printed_zero_jitter_and_colors() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.font_size = 28.0;
        params.line_spacing = 40.0;
        params.paragraphs = vec![Paragraph {
            text: String::new(),
            align: Align::Left,
            first_line_indent: 0.0,
            font_family: None,
            runs: vec![
                TextRun::new(
                    "印刷标题：",
                    TextRunStyle {
                        role_id: 1,
                        printed: true,
                        fill: Some([0, 0, 0]),
                        ..Default::default()
                    },
                ),
                TextRun::new(
                    "手写答案内容",
                    TextRunStyle {
                        role_id: 0,
                        printed: false,
                        fill: Some([0, 0, 255]),
                        ..Default::default()
                    },
                ),
            ],
        }];

        // 使用两个不同 seed 渲染
        let page1 = DefaultEngine::new(1).render_preview(&params).unwrap();
        let page2 = DefaultEngine::new(2).render_preview(&params).unwrap();

        let raw1 = page1.as_raw();
        let raw2 = page2.as_raw();

        // 提取两张图上的黑色像素（印刷体）
        let black_pixels1: Vec<usize> = raw1
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 0 { Some(idx) } else { None })
            .collect();
        let black_pixels2: Vec<usize> = raw2
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 0 { Some(idx) } else { None })
            .collect();

        assert!(!black_pixels1.is_empty(), "应存在印刷黑字");
        assert_eq!(
            black_pixels1, black_pixels2,
            "印刷体在不同 seed 下应具有完全一致的零抖动/零位移像素坐标！"
        );

        // 提取两张图上的蓝色像素（手写体）
        let blue_pixels1: Vec<usize> = raw1
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 255 { Some(idx) } else { None })
            .collect();
        let blue_pixels2: Vec<usize> = raw2
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 255 { Some(idx) } else { None })
            .collect();

        assert!(!blue_pixels1.is_empty(), "应存在手写蓝字");
        assert_ne!(
            blue_pixels1, blue_pixels2,
            "手写体在不同 seed 下应受到随机笔画扰动，像素坐标应当不同！"
        );
    }

    #[test]
    fn test_multi_role_rendering_with_custom_fonts_and_colors() {
        let Some(font) = system_font() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&font, &dir);
        params.font_size = 28.0;
        params.line_spacing = 40.0;
        params.roles = vec![
            crate::core::models::HandwritingRole {
                id: 1,
                name: "印刷体".into(),
                font_path: font.to_string_lossy().into_owned(),
                printed: true,
                font_size: Some(28.0),
                fill: Some([0, 0, 0]),
                ..Default::default()
            },
            crate::core::models::HandwritingRole {
                id: 2,
                name: "红笔批注".into(),
                font_path: font.to_string_lossy().into_owned(),
                printed: false,
                font_size: Some(24.0),
                fill: Some([255, 0, 0]),
                ..Default::default()
            },
        ];
        params.paragraphs = vec![Paragraph {
            text: String::new(),
            align: Align::Left,
            first_line_indent: 0.0,
            font_family: None,
            runs: vec![
                TextRun::new("【题目】", TextRunStyle { role_id: 1, ..Default::default() }),
                TextRun::new("优秀回答", TextRunStyle { role_id: 0, fill: Some([0, 0, 255]), ..Default::default() }),
                TextRun::new("（批注：满分）", TextRunStyle { role_id: 2, ..Default::default() }),
            ],
        }];

        let page = DefaultEngine::new(42).render_preview(&params).unwrap();
        let raw = page.as_raw();

        let has_black = raw.chunks_exact(4).any(|px| px[0] == 0 && px[1] == 0 && px[2] == 0);
        let has_blue = raw.chunks_exact(4).any(|px| px[0] == 0 && px[1] == 0 && px[2] == 255);
        let has_red = raw.chunks_exact(4).any(|px| px[0] == 255 && px[1] == 0 && px[2] == 0);

        assert!(has_black, "应渲染黑色印刷体");
        assert!(has_blue, "应渲染蓝色手写体");
        assert!(has_red, "应渲染红色批注");
    }

    #[test]
    fn test_multi_printed_fonts_in_mixed_mode() {
        let fonts = system_fonts();
        if fonts.is_empty() {
            return;
        }
        let font_a = fonts[0].to_string_lossy().into_owned();
        let font_b = if fonts.len() > 1 {
            fonts[1].to_string_lossy().into_owned()
        } else {
            fonts[0].to_string_lossy().into_owned()
        };
        let font_hand = fonts[0].to_string_lossy().into_owned();

        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&fonts[0], &dir);
        params.font_path = font_hand;
        params.font_size = 28.0;
        params.line_spacing = 40.0;
        params.word_spacing = 5.0;
        params.perturb_x_sigma = 1.0;
        params.perturb_y_sigma = 1.0;

        params.paragraphs = vec![Paragraph {
            text: String::new(),
            align: Align::Left,
            first_line_indent: 0.0,
            font_family: None,
            runs: vec![
                TextRun::new(
                    "【印刷标题】",
                    TextRunStyle {
                        font_path: Some(font_a),
                        printed: true,
                        role_id: 1,
                        fill: Some([0, 0, 0]),
                        ..Default::default()
                    },
                ),
                TextRun::new(
                    "说明文字：",
                    TextRunStyle {
                        font_path: Some(font_b),
                        printed: true,
                        role_id: 1,
                        fill: Some([100, 100, 100]),
                        ..Default::default()
                    },
                ),
                TextRun::new(
                    "手写动态扰动内容",
                    TextRunStyle {
                        font_path: None,
                        printed: false,
                        role_id: 0,
                        fill: Some([0, 0, 255]),
                        ..Default::default()
                    },
                ),
            ],
        }];

        // 验证排版与渲染无错误
        let page1 = DefaultEngine::new(10).render_preview(&params).expect("应成功渲染页面1");
        let page2 = DefaultEngine::new(20).render_preview(&params).expect("应成功渲染页面2");

        let raw1 = page1.as_raw();
        let raw2 = page2.as_raw();

        // 验证黑字（Run 1）、灰字（Run 2）和蓝字（Run 3）均有像素
        let has_black = raw1.chunks_exact(4).any(|px| px[0] == 0 && px[1] == 0 && px[2] == 0);
        let has_gray = raw1.chunks_exact(4).any(|px| px[0] == 100 && px[1] == 100 && px[2] == 100);
        let has_blue = raw1.chunks_exact(4).any(|px| px[0] == 0 && px[1] == 0 && px[2] == 255);

        assert!(has_black, "应渲染黑色印刷体 Run 1");
        assert!(has_gray, "应渲染灰色印刷体 Run 2");
        assert!(has_blue, "应渲染蓝色手写体 Run 3");

        // 验证印刷体零扰动（同坐标），手写体存在扰动
        let black_px1: Vec<usize> = raw1
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 0 { Some(idx) } else { None })
            .collect();
        let black_px2: Vec<usize> = raw2
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 0 { Some(idx) } else { None })
            .collect();
        assert_eq!(black_px1, black_px2, "印刷标题 Run 1 跨 seed 应保持零扰动像素一致");

        let gray_px1: Vec<usize> = raw1
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 100 && px[1] == 100 && px[2] == 100 { Some(idx) } else { None })
            .collect();
        let gray_px2: Vec<usize> = raw2
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 100 && px[1] == 100 && px[2] == 100 { Some(idx) } else { None })
            .collect();
        assert_eq!(gray_px1, gray_px2, "印刷说明 Run 2 跨 seed 应保持零扰动像素一致");

        let blue_px1: Vec<usize> = raw1
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 255 { Some(idx) } else { None })
            .collect();
        let blue_px2: Vec<usize> = raw2
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(idx, px)| if px[0] == 0 && px[1] == 0 && px[2] == 255 { Some(idx) } else { None })
            .collect();
        assert_ne!(blue_px1, blue_px2, "手写体 Run 3 跨 seed 应受扰动影响具有不同像素坐标");
    }

    #[test]
    fn test_region_inherits_role_attributes() {
        let fonts = system_fonts();
        if fonts.is_empty() {
            return;
        }
        let role_font = fonts[0].to_string_lossy().into_owned();
        let main_font = if fonts.len() > 1 {
            fonts[1].to_string_lossy().into_owned()
        } else {
            fonts[0].to_string_lossy().into_owned()
        };

        let dir = tempfile::tempdir().unwrap();
        let mut params = region_test_params(&fonts[0], &dir);
        params.font_path = main_font;
        params.fill = [0, 0, 0];
        params.roles = vec![HandwritingRole {
            id: 2,
            name: "高亮角色2".into(),
            highlight: Some("yellow".into()),
            font_path: role_font.clone(),
            printed: false,
            font_size: Some(26.0),
            fill: Some([255, 0, 0]),
            word_spacing: Some(5.0),
            line_spacing: Some(30.0),
            ..Default::default()
        }];

        let region = TextRegion {
            x: 10,
            y: 10,
            w: 200,
            h: 40,
            text: "继承测试".into(),
            role_id: 2,
            font_path: String::new(), // 未显式指定字体，应继承 role.font_path
            fill: None, // 未显式指定颜色，应继承 role.fill
            font_size: 0, // 未显式指定字号，应继承 role.font_size
            ..Default::default()
        };

        let local_params = DefaultEngine::region_local_params(&params, &region);
        assert_eq!(local_params.font_path, role_font);
        assert_eq!(local_params.fill, [255, 0, 0]);
        assert_eq!(local_params.font_size, 26.0);
        assert_eq!(local_params.word_spacing, 5.0);
        assert_eq!(local_params.line_spacing, 30.0);
    }
}

