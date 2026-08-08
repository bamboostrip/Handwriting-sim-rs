//! 渲染引擎：编排排版 + 笔画扰动，输出整页图像。
//!
//! 对应 Python 版 `HandwritingEngine`（默认 fast 后端）的接口：
//! `render_preview` / `generate` / `save_all`。
//! 当前实现对齐 `FastEngine` 的**纯文本路径**；段落路径（paragraphs）
//! 作为后续迭代（见 docs/02-migration-plan.md 的迁移阶段划分）。

use std::path::{Path, PathBuf};

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
        let result = layout::layout_page(params, font, rng, text, start, width, height);
        let canvas =
            perturb::perturb_mask(&result.mask, width, height, params, rng, &background.as_raw());
        Ok((rgba_from_rgb(&canvas, width, height), result.consumed))
    }
}

impl Engine for DefaultEngine {
    fn render_preview(&self, params: &HandwritingParams) -> Result<RgbaImage, EngineError> {
        params.validate()?;
        let font =
            FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
        let background = Self::load_background(&params.background_path)?;
        let mut rng = StdRng::seed_from_u64(self.seed);
        let (page, _) = self.render_page_from(params, &font, &mut rng, &params.text, 0, &background)?;
        Ok(page)
    }

    fn render_pages(&self, params: &HandwritingParams) -> Result<Vec<RgbaImage>, EngineError> {
        params.validate()?;
        let font =
            FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
        let background = Self::load_background(&params.background_path)?;
        let mut rng = StdRng::seed_from_u64(self.seed);
        let mut pages = Vec::new();
        let mut start = 0;
        loop {
            let (page, consumed) = self.render_page_from(params, &font, &mut rng, &params.text, start, &background)?;
            pages.push(page);
            if consumed >= params.text.chars().count() {
                break;
            }
            start = consumed;
        }
        Ok(pages)
    }

    fn save_all(&self, params: &HandwritingParams, out_dir: &Path) -> Result<Vec<PathBuf>, EngineError> {
        std::fs::create_dir_all(out_dir)?;
        let pages = self.render_pages(params)?;
        let mut files = Vec::new();
        for (index, page) in pages.into_iter().enumerate() {
            let path = out_dir.join(format!("{index}.png"));
            page.save(&path)
                .map_err(|e| EngineError::Image(format!("保存 {path:?} 失败：{e}")))?;
            files.push(path);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn render_preview_rejects_invalid_params() {
        let params = HandwritingParams::default();
        assert!(matches!(
            render_preview(&params, 1),
            Err(EngineError::Params(ParamsError::NoText))
        ));
    }
}