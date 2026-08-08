//! 排版：逐字符绘制、行/字/字号高斯扰动、end_chars/start_chars 换行规则。
//!
//! 对应 Python 版 `engine_fast._layout_page` 的逻辑翻译。
//! 随机数消耗顺序保持与 Python 版一致（逐字符：行扰动 → 字号扰动 → 字距扰动），
//! 便于未来用 golden 样本做迁移验收。

use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::core::font::FontFace;
use crate::core::models::HandwritingParams;

/// 一页排版结果。
pub struct LayoutResult {
    /// `width * height` 的前景掩码（逐行）。
    pub mask: Vec<bool>,
    /// 本页消费的字符数（含换行符）。
    pub consumed: usize,
}

/// 排版一页文字，返回前景掩码与本页消费的字符数。
///
/// 坐标约定：`y` 为行**顶部**坐标（与 Python 版 `_layout_page` 一致），
/// 光栅化时通过 `ascent` 换算为基线坐标。
pub fn layout_page(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    text: &str,
    start: usize,
    width: usize,
    height: usize,
) -> LayoutResult {
    let width_f = width as f32;
    let mut mask = vec![false; width * height];

    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();
    let line_spacing = params.total_line_spacing();
    let end_chars = params.end_chars.as_str();
    let start_chars = params.start_chars.as_str();

    let normal_line = Normal::new(0.0, f64::from(params.line_spacing_sigma)).unwrap();
    let normal_word = Normal::new(0.0, f64::from(params.word_spacing_sigma)).unwrap();
    let normal_font = Normal::new(0.0, f64::from(params.font_size_sigma)).unwrap();

    let mut i = start;
    let mut y = params.first_line_y();

    while y <= height as f32 - params.bottom_margin - params.font_size {
        let mut x = params.left_margin;
        loop {
            if i >= text_len {
                return LayoutResult { mask, consumed: i };
            }
            let ch = chars[i];
            if ch == '\n' {
                i += 1;
                break;
            }
            // 换行规则：末尾禁止字符 / 行首禁止字符（与 Python 版一致）
            if x > width_f - params.right_margin - 2.0 * params.font_size
                && start_chars.contains(ch)
            {
                break;
            }
            if x > width_f - params.right_margin - params.font_size && !end_chars.contains(ch) {
                break;
            }

            // 行纵向扰动（每字符独立，与 Python 版 rand.gauss 顺序一致）
            let yj = y + normal_line.sample(rng) as f32;

            // 字号扰动
            let mut size = params.font_size;
            if params.font_size_sigma > 0.0 {
                size = (params.font_size + normal_font.sample(rng) as f32).round().max(0.0);
            }
            let size = size.max(1.0);

            let offset = font.glyph_width(ch, size);
            let baseline_y = yj + font.ascent(size);
            font.rasterize(ch, size, x.round(), baseline_y, &mut mask, width, height);

            // 字距推进（含字形宽度与扰动）
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;

            i += 1;
            if i >= text_len {
                return LayoutResult { mask, consumed: i };
            }
        }
        y += line_spacing;
    }
    LayoutResult { mask, consumed: i }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use std::path::PathBuf;

    fn system_font() -> Option<PathBuf> {
        const CANDIDATES: &[&str] = &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
            r"/System/Library/Fonts/PingFang.ttc",
            r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        CANDIDATES.iter().map(|p| PathBuf::from(p.trim())).find(|p| p.is_file())
    }

    fn params() -> HandwritingParams {
        HandwritingParams {
            text: "你好世界，测试排版。".into(),
            ..HandwritingParams::default()
        }
    }

    #[test]
    fn layout_produces_foreground_and_consumes() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let p = params();
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let result = layout_page(&p, &font, &mut rng, &p.text, 0, 600, 400);
        assert!(result.consumed > 0, "应消费至少一个字符");
        assert!(result.consumed <= p.text.chars().count());
        assert!(result.mask.iter().any(|&b| b), "排版应产生前景像素");
    }

    #[test]
    fn layout_respects_end_chars_wrap() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 40.0).unwrap();
        // 极窄画布：每行最多一个字符
        let mut p = params();
        p.end_chars = "".into(); // 全部字符都不可放行尾 → 每行一个字符
        p.font_size = 40.0;
        p.word_spacing = 0.0;
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.left_margin = 2.0;
        p.right_margin = 2.0;
        p.top_margin = 2.0;
        p.bottom_margin = 2.0;
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let result = layout_page(&p, &font, &mut rng, "甲乙丙丁戊己", 0, 60, 600);
        // 6 个字符应全部消费（每个字符占一行）
        assert_eq!(result.consumed, 6);
    }

    #[test]
    fn same_seed_same_layout() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let p = params();
        let a = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(42), &p.text, 0, 600, 400);
        let b = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(42), &p.text, 0, 600, 400);
        assert_eq!(a.mask, b.mask);
        assert_eq!(a.consumed, b.consumed);
    }
}