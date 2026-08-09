//! 排版：逐字符绘制、行/字/字号高斯扰动、end_chars/start_chars 换行规则。
//!
//! 对应 Python 版 `engine_fast._layout_page` 的逻辑翻译。
//! 随机数消耗顺序保持与 Python 版一致（逐字符：行扰动 → 字号扰动 → 字距扰动），
//! 便于未来用 golden 样本做迁移验收。

use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::core::font::FontFace;
use crate::core::models::{Align, HandwritingParams, MiswriteMode, Paragraph};

/// 一页排版结果。
pub struct LayoutResult {
    /// `width * height` 的前景掩码（逐行）。
    pub mask: Vec<bool>,
    /// 本页消费的字符数（含换行符）。
    pub consumed: usize,
}

/// 在掩码中画一条带厚度的线段（删除线用）。逐行 bool 掩码，越界自动忽略。
#[allow(clippy::too_many_arguments)]
fn draw_thick_line(
    mask: &mut [bool],
    width: usize,
    height: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
) {
    let r = thickness / 2.0;
    let steps = ((x1 - x0).abs().max((y1 - y0).abs())).ceil().max(1.0) as usize;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let cx = x0 + (x1 - x0) * t;
        let cy = y0 + (y1 - y0) * t;
        let x_lo = (cx - r).floor() as isize;
        let x_hi = (cx + r).ceil() as isize;
        let y_lo = (cy - r).floor() as isize;
        let y_hi = (cy + r).ceil() as isize;
        for yy in y_lo..=y_hi {
            for xx in x_lo..=x_hi {
                if xx >= 0 && yy >= 0 && (xx as usize) < width && (yy as usize) < height {
                    mask[(yy as usize) * width + (xx as usize)] = true;
                }
            }
        }
    }
}

/// 对错字字符绘制删除线（与可选的上方小字重写）。
/// `y_top` 为该字符的行顶坐标（同 layout_page/placed 的 y 语义），`angle` 为删除线倾角（rad）；
/// `draw_small` 为 true 时在正上方略偏右补画小一号重写（Above 模式）。
#[allow(clippy::too_many_arguments)]
fn draw_miswrite(
    mask: &mut [bool],
    width: usize,
    height: usize,
    font: &FontFace,
    ch: char,
    x: f32,
    y_top: f32,
    size: f32,
    angle: f32,
    draw_small: bool,
) {
    let advance = font.glyph_width(ch, size);
    // 删除线：跨字形宽度的旋转粗线，位于字符竖直中线
    let mid = y_top + font.ascent(size) * 0.45;
    let (ct, st) = (angle.cos(), angle.sin());
    let half = advance * 0.45;
    let (rx, ry) = (half * ct, half * st);
    let thickness = (size / 8.0).max(2.0);
    draw_thick_line(mask, width, height, x + rx, mid - ry, x - rx, mid + ry, thickness);
    if draw_small {
        // 小一号重写：正上方略偏右，基线不低于 0（首行避免裁掉）
        let small = (size * 0.6).max(1.0);
        let small_x = x + size * 0.15;
        let small_baseline = (y_top - size * 0.85 + font.ascent(small)).max(font.ascent(small));
        font.rasterize(ch, small, small_x, small_baseline, mask, width, height);
    }
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
    let normal_strike = Normal::new(0.0, 0.15).unwrap();

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

            // 字距推进（含字形宽度与扰动）——先记录字符起始位置供错字效果使用
            let char_x = x;
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;

            i += 1;

            // 写错字模拟：判定只影响渲染，不参与换行；rate=0 时不消耗 RNG（零回归）
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                let angle = normal_strike.sample(rng) as f32;
                match params.miswrite_rewrite_mode {
                    MiswriteMode::Above => {
                        // 删除线 + 正上方小字重写
                        draw_miswrite(&mut mask, width, height, font, ch, char_x, yj, size, angle, true);
                    }
                    MiswriteMode::Rewrite => {
                        // 仅删除线；后文正常位置重写：紧邻错字之后以正常字号重写，再推进一个字形宽度给后续字符
                        draw_miswrite(&mut mask, width, height, font, ch, char_x, yj, size, angle, false);
                        font.rasterize(ch, size, x.round(), baseline_y, &mut mask, width, height);
                        x += font.glyph_width(ch, size) + params.word_spacing;
                    }
                }
            }

            if i >= text_len {
                return LayoutResult { mask, consumed: i };
            }
        }
        y += line_spacing;
    }
    LayoutResult { mask, consumed: i }
}

/// 行带分组：把行聚合 bool 数组按连续段分组，返回 [start, end) 列表。
fn split_text_rows(rows: &[bool]) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, &v) in rows.iter().enumerate() {
        if v && start.is_none() {
            start = Some(idx);
        } else if !v && start.is_some() {
            groups.push((start.unwrap(), idx));
            start = None;
        }
    }
    if let Some(s) = start {
        groups.push((s, rows.len()));
    }
    groups
}

/// 按文本行测量非零 x 范围，逐行水平居中（对齐 Python `_center_text_lines`）。
fn center_text_lines(mask: &mut [bool], width: usize) {
    let rows: Vec<bool> = mask.chunks(width).map(|r| r.iter().any(|&b| b)).collect();
    if !rows.iter().any(|&b| b) {
        return;
    }
    let mut result = vec![false; mask.len()];
    for (y0, y1) in split_text_rows(&rows) {
        let band = &mask[y0 * width..y1 * width];
        let mut min_x = usize::MAX;
        let mut max_x = 0usize;
        for (x, &b) in band.iter().enumerate() {
            if b {
                min_x = min_x.min(x % width);
                max_x = max_x.max(x % width);
            }
        }
        let line_w = max_x - min_x + 1;
        if line_w >= width {
            result[y0 * width..y1 * width].copy_from_slice(band);
            continue;
        }
        let shift = (width - line_w) / 2 - min_x;
        for (idx, &b) in band.iter().enumerate() {
            if b {
                let x = idx % width;
                let y = y0 + idx / width;
                let nx = x as isize + shift as isize;
                if nx >= 0 && (nx as usize) < width {
                    result[y * width + nx as usize] = true;
                }
            }
        }
    }
    mask.copy_from_slice(&result);
}

/// 渲染单个段落，返回逐行 [(该行墨迹裁剪掩码, 相对该行绘制基线的偏移)]。
/// 空行对应 (None, 0.0)。画布按段落自身高度创建（不受页高裁剪）。
pub fn layout_paragraph(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    paragraph: &Paragraph,
    width: usize,
) -> Vec<(Option<Vec<bool>>, f32)> {
    let width_f = width as f32;
    let line_spacing = params.total_line_spacing();
    let end_chars = params.end_chars.as_str();
    let start_chars = params.start_chars.as_str();
    let text = paragraph.text.as_str();
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();

    let normal_line = Normal::new(0.0, f64::from(params.line_spacing_sigma)).unwrap();
    let normal_word = Normal::new(0.0, f64::from(params.word_spacing_sigma)).unwrap();
    let normal_font = Normal::new(0.0, f64::from(params.font_size_sigma)).unwrap();
    let normal_strike = Normal::new(0.0, 0.15).unwrap();

    // 阶段一：纯排版（不绘制），随机数消耗顺序与纯文本路径一致
    #[derive(Clone, Copy)]
    struct Placed {
        ch: char,
        x: f32,
        y: f32,
        size: f32,
        line: usize,
        miswrite: bool,
        angle: f32,
    }
    let mut placed: Vec<Placed> = Vec::new();
    let mut line_x_ends: Vec<f32> = Vec::new();
    let mut line_ys: Vec<f32> = Vec::new();
    let mut i = 0;
    let mut y = line_spacing - params.font_size;
    while i < text_len {
        line_ys.push(y);
        let mut x = params.left_margin + (if i == 0 { paragraph.first_line_indent } else { 0.0 });
        while i < text_len {
            let ch = chars[i];
            if ch == '\n' {
                i += 1;
                break;
            }
            if x > width_f - params.right_margin - 2.0 * params.font_size
                && start_chars.contains(ch)
            {
                break;
            }
            if x > width_f - params.right_margin - params.font_size && !end_chars.contains(ch) {
                break;
            }
            let yj = y + normal_line.sample(rng) as f32;
            let mut size = params.font_size;
            if params.font_size_sigma > 0.0 {
                size = (params.font_size + normal_font.sample(rng) as f32).round().max(0.0);
            }
            let size = size.max(1.0);
            let offset = font.glyph_width(ch, size);
            placed.push(Placed { ch, x, y: yj, size, line: line_ys.len() - 1, miswrite: false, angle: 0.0 });
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;
            i += 1;
            // 错字判定（RNG 消费顺序与文本路径一致：字符扰动之后）；rate=0 不消耗
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                if let Some(last) = placed.last_mut() {
                    last.miswrite = true;
                    last.angle = normal_strike.sample(rng) as f32;
                }
            }
        }
        line_x_ends.push(x);
        y += line_spacing;
    }
    if line_ys.is_empty() {
        return Vec::new();
    }

    // 右对齐：按每行逻辑宽度（含尾部空格）平移到右边距
    let shifts: Option<Vec<f32>> = if paragraph.align == Align::Right {
        let right_x = width_f - params.right_margin;
        Some(line_x_ends.iter().map(|xe| right_x - xe).collect())
    } else {
        None
    };

    // 阶段二：按段落实际高度创建画布并绘制（不被页高裁剪）
    let canvas_h = (y + params.font_size + 4.0 * params.line_spacing_sigma + 4.0).max(1.0);
    let canvas_h = canvas_h as usize;
    let mut mask = vec![false; width * canvas_h];
    for item in &placed {
        let dx = match &shifts {
            Some(s) => item.x + s[item.line],
            None => item.x,
        };
        let baseline_y = item.y + font.ascent(item.size);
        font.rasterize(item.ch, item.size, dx, baseline_y, &mut mask, width, canvas_h);
        if item.miswrite {
            draw_miswrite(&mut mask, width, canvas_h, font, item.ch, dx, item.y, item.size, item.angle, params.miswrite_rewrite_mode == MiswriteMode::Above);
            if params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                let dx2 = dx + font.glyph_width(item.ch, item.size) + params.word_spacing;
                font.rasterize(item.ch, item.size, dx2.round(), baseline_y, &mut mask, width, canvas_h);
            }
        }
    }
    if paragraph.align == Align::Center {
        center_text_lines(&mut mask, width);
    }

    // 按行提取墨迹：行带分组 → 归属各行，空行补 (None, 0.0)。
    // 一行可能包含多个行带（Above 模式的小字重写悬浮在行顶上方），
    // 全部归入该行合并提取，避免多余行带被丢弃。
    let rows: Vec<bool> = mask.chunks(width).map(|r| r.iter().any(|&b| b)).collect();
    let bands = split_text_rows(&rows);
    let mut bi = 0usize;
    let off_min = -0.25 * line_spacing;
    let off_max = 0.8 * line_spacing;
    let mut lines: Vec<(Option<Vec<bool>>, f32)> = Vec::new();
    for &yk in &line_ys {
        if bi < bands.len() && (bands[bi].0 as f32) < yk + line_spacing / 2.0 {
            let s0 = bands[bi].0;
            let mut s_main = s0;
            let mut e = bands[bi].1;
            bi += 1;
            while bi < bands.len() && (bands[bi].0 as f32) < yk + line_spacing / 2.0 {
                s_main = bands[bi].0;
                e = bands[bi].1;
                bi += 1;
            }
            // 对齐基准取主行带（最后一个）起点；小字带悬浮其上，行主体仍对齐行网格
            let off = ((s_main as f32 - yk).max(off_min)).min(off_max);
            lines.push((Some(mask[s0 * width..e * width].to_vec()), off));
        } else {
            lines.push((None, 0.0));
        }
    }
    lines
}

/// 全部段落 → 逐行流式分页，返回各页前景掩码（对齐 Python `_paragraph_pages`）。
pub fn layout_paragraphs(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    paragraphs: &[Paragraph],
    width: usize,
    height: usize,
) -> Vec<Vec<bool>> {
    let line_spacing = params.total_line_spacing();
    let lead = line_spacing - params.font_size;
    let limit = height as f32 - params.bottom_margin - params.font_size;

    // 所有段落共用同一 rng 流，先全部排版为逐行列表
    let mut all_lines: Vec<(Option<Vec<bool>>, f32)> = Vec::new();
    for para in paragraphs {
        let mut lines = layout_paragraph(params, font, rng, para, width);
        if lines.is_empty() {
            lines.push((None, 0.0)); // 空段保留一行空行
        }
        all_lines.extend(lines);
    }

    let mut pages: Vec<Vec<bool>> = Vec::new();
    let mut page_canvas = vec![false; width * height];
    let mut draw_y = params.top_margin + lead;
    for (band, off) in all_lines {
        if draw_y > limit && page_canvas.iter().any(|&b| b) {
            pages.push(std::mem::take(&mut page_canvas));
            page_canvas = vec![false; width * height];
            draw_y = params.top_margin + lead;
        }
        if let Some(band) = band {
            let row0 = (draw_y + off).round() as isize;
            for (by, row) in band.chunks(width).enumerate() {
                let ty = row0 + by as isize;
                if ty < 0 || ty >= height as isize {
                    continue;
                }
                let dst = &mut page_canvas[ty as usize * width..(ty as usize + 1) * width];
                for (x, &b) in row.iter().enumerate() {
                    if b {
                        dst[x] = true;
                    }
                }
            }
        }
        draw_y += line_spacing;
    }
    if page_canvas.iter().any(|&b| b) || pages.is_empty() {
        pages.push(page_canvas);
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::MiswriteMode;
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

    fn para() -> Paragraph {
        Paragraph {
            text: "第一行文字，第二行测试。".into(),
            align: Align::Left,
            first_line_indent: 0.0,
        }
    }

    #[test]
    fn layout_paragraph_produces_lines() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let lines = layout_paragraph(&p, &font, &mut rng, &para(), 600);
        assert!(!lines.is_empty(), "应产生至少一行");
        assert!(lines.iter().any(|(m, _)| m.is_some()), "应存在非空行墨迹");
    }

    #[test]
    fn layout_paragraph_first_line_indent_only() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        let mut pa = para();
        pa.first_line_indent = 50.0;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let lines = layout_paragraph(&p, &font, &mut rng, &pa, 600);
        let first = lines[0].0.as_ref().expect("首行应有墨迹");
        let first_min_x = first
            .chunks(600)
            .enumerate()
            .filter(|(_, row)| row.iter().any(|&b| b))
            .flat_map(|(_, row)| row.iter().position(|&b| b))
            .min()
            .unwrap();
        assert!(first_min_x >= 50, "首行应缩进：{first_min_x}");
    }

    #[test]
    fn layout_paragraph_right_align_pushes_to_right_edge() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.right_margin = 30.0;
        let mut pa = para();
        pa.align = Align::Right;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let lines = layout_paragraph(&p, &font, &mut rng, &pa, 600);
        let first = lines[0].0.as_ref().expect("首行应有墨迹");
        let (mut max_x, mut min_x) = (0usize, usize::MAX);
        for row in first.chunks(600) {
            for (x, &b) in row.iter().enumerate() {
                if b {
                    max_x = max_x.max(x);
                    min_x = min_x.min(x);
                }
            }
        }
        assert!(
            max_x >= 600 - 30 - 40,
            "右对齐行应贴近右缘，实际 max_x={max_x}"
        );
        assert!(min_x > 0, "右对齐行不应从左边距开始：min_x={min_x}");
    }

    #[test]
    fn layout_paragraph_empty_text_yields_no_lines() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let p = params();
        let mut pa = para();
        pa.text = String::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        assert!(layout_paragraph(&p, &font, &mut rng, &pa, 600).is_empty());
    }

    #[test]
    fn layout_paragraphs_streams_across_pages() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        let mut paras = vec![para(), para()];
        paras[1].text = "第二段内容，足够长以触发跨页。".into();
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let pages = layout_paragraphs(&p, &font, &mut rng, &paras, 300, 250);
        // 跨平台字形度量差异可能导致行数浮动，页数也因此可能浮动，故放宽为至少两页
        assert!(pages.len() >= 2, "矮画布应产生至少两页，实际 {}", pages.len());
        assert!(pages[0].iter().any(|&b| b), "首页应有墨迹");
        assert!(pages[1].iter().any(|&b| b), "第二页应有墨迹");
    }

    /// 错字率>0 时输出应比关闭时产生更多前景（删除线/重写墨迹），
    /// 且消费额外 RNG 后同 seed 应逐像素稳定复现。错字率=0 不消耗额外 RNG
    /// 由 rate=0 短路 random_bool 结构性保证；本测试断言墨迹增量与确定性。
    #[test]
    fn miswrite_adds_ink_and_rate_zero_is_stable() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.miswrite_rate = 0.5;
        p.miswrite_rewrite_mode = MiswriteMode::Above;
        let text = "今天天气很好，我们去公园散步。".to_string();
        let a = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        let b = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        assert_eq!(a.consumed, b.consumed);
        assert_eq!(a.mask, b.mask, "同 seed 应逐像素一致");
        let ink_a = a.mask.iter().filter(|&&v| v).count();
        assert!(ink_a > 0, "应存在前景");

        // 错字率=0 时 rate=0 短路 random_bool，不消耗额外 RNG（结构性保证）；
        // 此处仅断言墨迹增量：0.5 的墨迹量大于 0.0 的墨迹量（删除线/重写增加前景）。
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let zero = layout_page(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        assert!(
            ink_a > zero.mask.iter().filter(|&&v| v).count(),
            "错字效果应增加前景像素"
        );
    }

    /// Rewrite 模式：重写字符画在错字右侧（x 推进），墨迹分布明显更宽。
    #[test]
    fn miswrite_rewrite_mode_draws_extra_glyph() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.miswrite_rate = 1.0; // 全部字符错字 → 每个字符都重写一遍
        p.miswrite_rewrite_mode = MiswriteMode::Rewrite;
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        let text = "甲乙丙".to_string();
        let r = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        assert_eq!(r.consumed, 3);
        // Rewrite 模式不画上方小字：首行行顶之上不应有墨迹（只有删除线+内联重写）
        let top_ink_row = r.mask
            .chunks(600)
            .enumerate()
            .find(|(_, row)| row.iter().any(|&b| b))
            .map(|(i, _)| i)
            .unwrap();
        assert!(
            top_ink_row >= p.first_line_y() as usize,
            "Rewrite 不应在行顶上方画小字：首个墨迹行 {top_ink_row}"
        );
        // 3 个错字 + 3 个重写 = 6 个字形宽度（约 36px 每字），比只排 3 字明显更宽
        let last_ink_x = r.mask.chunks(600).flat_map(|row| row.iter().rposition(|&b| b)).max().unwrap();
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let r0 = layout_page(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        let last_ink_x0 = r0.mask.chunks(600).flat_map(|row| row.iter().rposition(|&b| b)).max().unwrap();
        assert!(last_ink_x > last_ink_x0 + 30, "Rewrite 应把最右墨迹推到更远处：{last_ink_x} vs {last_ink_x0}");
    }

    /// 段落路径：错字率>0 增加前景；同 seed 两次渲染逐像素一致。
    #[test]
    fn paragraph_miswrite_adds_ink_and_is_deterministic() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.miswrite_rate = 0.8;
        p.miswrite_rewrite_mode = MiswriteMode::Above;
        let mut pa = para();
        pa.text = "今天天气很好，我们去公园散步。".into();
        let a = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        let b = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        assert_eq!(a.len(), b.len());
        for ((ma, _), (mb, _)) in a.iter().zip(b.iter()) {
            assert_eq!(ma, mb, "同 seed 应逐像素一致");
        }
        let ink_a: usize = a.iter().filter_map(|(m, _)| m.as_ref()).map(|m| m.iter().filter(|&&v| v).count()).sum();
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let z = layout_paragraph(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        let ink_z: usize = z.iter().filter_map(|(m, _)| m.as_ref()).map(|m| m.iter().filter(|&&v| v).count()).sum();
        assert!(ink_a > ink_z, "错字效果应增加前景像素：{ink_a} vs {ink_z}");
    }
}
