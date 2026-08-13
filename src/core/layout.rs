//! 排版：逐字符绘制、行/字/字号高斯扰动、end_chars/start_chars 换行规则。
//!
//! 对应 Python 版 `engine_fast._layout_page` 的逻辑翻译。
//! 随机数消耗顺序保持与 Python 版一致（逐字符：行扰动 → 字号扰动 → 字距扰动），
//! 便于未来用 golden 样本做迁移验收。

use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::core::font::FontFace;
use crate::core::models::{Align, HandwritingParams, MiswriteMode, Paragraph, StrikeoutStyle};

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

const COMMON_CHINESE_CHARS: &[char] = &[
    '的', '一', '是', '在', '了', '不', '和', '有', '大', '这', '主', '中', '人', '国', '为', '以', '我', '分', '们', '行',
    '产', '作', '本', '经', '发', '社', '工', '己', '等', '均', '部', '样', '出', '门', '家', '理', '学', '对', '里', '后',
    '小', '多', '下', '心', '然', '事', '资', '力', '么', '得', '之', '都', '平', '因', '起', '只', '没', '生', '量', '建',
    '长', '现', '前', '性', '那', '系', '各', '进', '最', '及', '外', '治', '与', '公', '向', '情', '老', '正', '路', '解',
    '问', '反', '政', '化', '无', '其', '期', '高', '强', '使', '教', '定', '重', '社', '特', '立', '体', '政', '代', '通',
    '度', '意', '见', '指', '表', '命', '战', '民', '保', '机', '关', '各', '党', '建', '议', '写', '论', '设', '合', '名',
    '同', '由', '接', '收', '改', '新', '想', '打', '放', '儿', '加', '用', '及', '那', '此', '实', '决', '求', '美', '品',
    '书', '样', '要', '治', '法', '务', '制', '度', '清', '楚', '确', '认', '真', '实', '各', '部', '委', '局', '厅', '所'
];

pub(crate) fn get_wrong_char(ch: char, rng: &mut impl Rng) -> char {
    if ch.is_ascii_uppercase() {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            wrong_ch = (b'A' + rng.random_range(0..26)) as char;
        }
        wrong_ch
    } else if ch.is_ascii_lowercase() {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            wrong_ch = (b'a' + rng.random_range(0..26)) as char;
        }
        wrong_ch
    } else if ch.is_ascii_digit() {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            wrong_ch = (b'0' + rng.random_range(0..10)) as char;
        }
        wrong_ch
    } else if ('\u{4e00}'..='\u{9fa5}').contains(&ch) {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            let idx = rng.random_range(0..COMMON_CHINESE_CHARS.len());
            wrong_ch = COMMON_CHINESE_CHARS[idx];
        }
        wrong_ch
    } else {
        ch
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_bezier_line(
    mask: &mut [bool],
    width: usize,
    height: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    waviness: f32,
    rng: &mut impl Rng,
) {
    let mx = (x0 + x1) / 2.0;
    let my = (y0 + y1) / 2.0;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let nx = -dy / len;
    let ny = dx / len;

    let offset = if waviness <= 0.0 { 0.0 } else { rng.random_range(-waviness..=waviness) };
    let cx = mx + nx * offset;
    let cy = my + ny * offset;

    let mut prev_x = x0;
    let mut prev_y = y0;
    for step in 1..=5 {
        let t = step as f32 / 5.0;
        let mt = 1.0 - t;
        let curr_x = mt * mt * x0 + 2.0 * mt * t * cx + t * t * x1;
        let curr_y = mt * mt * y0 + 2.0 * mt * t * cy + t * t * y1;
        draw_thick_line(mask, width, height, prev_x, prev_y, curr_x, curr_y, thickness);
        prev_x = curr_x;
        prev_y = curr_y;
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
    wrong_ch: char,
    correct_ch: char,
    x: f32,
    y_top: f32,
    size: f32,
    angle: f32,
    draw_small: bool,
    style: StrikeoutStyle,
    rng: &mut impl Rng,
) {
    use rand::SeedableRng;
    let seed = rng.random::<u64>();
    let mut local_rng = rand::rngs::StdRng::seed_from_u64(seed);
    let rng = &mut local_rng;

    let wrong_advance = font.glyph_width(wrong_ch, size);
    let mid_x = x + wrong_advance / 2.0; // Centered on typo character
    let mid_y = y_top + font.ascent(size) * 0.45;
    
    let (ct, st) = (angle.cos(), angle.sin());
    let half_w = wrong_advance * 0.55;
    let half_h = size * 0.4;
    let thickness = (size * 0.035).max(1.5);
    let waviness = size * 0.08;

    match style {
        StrikeoutStyle::Line => {
            let rx = half_w * ct;
            let ry = half_w * st;
            draw_bezier_line(mask, width, height, mid_x - rx, mid_y - ry, mid_x + rx, mid_y + ry, thickness, waviness, rng);
        }
        StrikeoutStyle::DoubleLine => {
            let rx = half_w * ct;
            let ry = half_w * st;
            
            // Draw top parallel line
            let offset_y = size * 0.1;
            draw_bezier_line(mask, width, height, mid_x - rx, mid_y - ry - offset_y, mid_x + rx, mid_y + ry - offset_y, thickness, waviness, rng);
            // Draw bottom parallel line
            draw_bezier_line(mask, width, height, mid_x - rx, mid_y - ry + offset_y, mid_x + rx, mid_y + ry + offset_y, thickness, waviness, rng);
        }
        StrikeoutStyle::Slash => {
            // Draw single diagonal slash
            let x0 = mid_x + half_w * 0.7;
            let y0 = mid_y - half_h;
            let x1 = mid_x - half_w * 0.7;
            let y1 = mid_y + half_h;
            draw_bezier_line(mask, width, height, x0, y0, x1, y1, thickness, waviness, rng);
        }
        StrikeoutStyle::Cross => {
            // Diagonal line 1
            let x0_1 = mid_x - half_w * 0.7;
            let y0_1 = mid_y - half_h;
            let x1_1 = mid_x + half_w * 0.7;
            let y1_1 = mid_y + half_h;
            draw_bezier_line(mask, width, height, x0_1, y0_1, x1_1, y1_1, thickness, waviness, rng);
            
            // Diagonal line 2
            let x0_2 = mid_x + half_w * 0.7;
            let y0_2 = mid_y - half_h;
            let x1_2 = mid_x - half_w * 0.7;
            let y1_2 = mid_y + half_h;
            draw_bezier_line(mask, width, height, x0_2, y0_2, x1_2, y1_2, thickness, waviness, rng);
        }
    }

    if draw_small {
        // Rewrite height adjusted closer to y_top with slight random perturbation
        let small = (size * 0.6).max(1.0);
        let rx_offset = rng.random_range(-size * 0.03..=size * 0.03);
        let ry_offset = rng.random_range(-size * 0.03..=size * 0.03);
        let small_x = x + wrong_advance * 0.45 + rx_offset;
        let small_baseline = (y_top - size * 0.45 + font.ascent(small) + ry_offset).max(font.ascent(small));
        font.rasterize(correct_ch, small, small_x, small_baseline, mask, width, height);
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

            let word_noise = normal_word.sample(rng) as f32;

            // 写错字模拟：判定只影响渲染，不参与换行；rate=0 时不消耗 RNG（零回归）
            let mut is_miswrite = false;
            let mut wrong_ch = ch;
            let mut angle = 0.0;
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                is_miswrite = true;
                wrong_ch = get_wrong_char(ch, rng);
                angle = normal_strike.sample(rng) as f32;
            }

            let offset = font.glyph_width(wrong_ch, size);
            let baseline_y = yj + font.ascent(size);
            font.rasterize(wrong_ch, size, x.round(), baseline_y, &mut mask, width, height);

            let char_x = x;
            x += params.word_spacing + offset + word_noise;

            i += 1;

            if is_miswrite {
                match params.miswrite_rewrite_mode {
                    MiswriteMode::Above => {
                        draw_miswrite(&mut mask, width, height, font, wrong_ch, ch, char_x, yj, size, angle, true, params.miswrite_strikeout_style, rng);
                    }
                    MiswriteMode::Rewrite => {
                        draw_miswrite(&mut mask, width, height, font, wrong_ch, ch, char_x, yj, size, angle, false, params.miswrite_strikeout_style, rng);
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
        ch: char,          // The character actually drawn (could be wrong_ch)
        correct_ch: char,  // The original correct character
        x: f32,
        y: f32,
        size: f32,
        line: usize,
        miswrite: bool,
        angle: f32,
        rewrite_x: f32,
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

            let word_noise = normal_word.sample(rng) as f32;

            let mut is_miswrite = false;
            let mut wrong_ch = ch;
            let mut angle = 0.0;
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                is_miswrite = true;
                wrong_ch = get_wrong_char(ch, rng);
                angle = normal_strike.sample(rng) as f32;
            }

            let offset = font.glyph_width(wrong_ch, size);
            let mut rewrite_x = 0.0;
            let x_next = x + params.word_spacing + offset + word_noise;
            
            if is_miswrite && params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                rewrite_x = x_next;
            }

            placed.push(Placed {
                ch: wrong_ch,
                correct_ch: ch,
                x,
                y: yj,
                size,
                line: line_ys.len() - 1,
                miswrite: is_miswrite,
                angle,
                rewrite_x,
            });
            
            x = x_next;
            i += 1;
            
            if is_miswrite && params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                x += font.glyph_width(ch, size) + params.word_spacing;
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

    // 居中：按行计算中心偏移（与右对齐 shifts 同机制），
    // 使小字带/重写与锚定字符同移，避免行带被独立居中导致漂移
    let center_shifts: Option<Vec<f32>> = if paragraph.align == Align::Center {
        let mut min_x = vec![f32::MAX; line_ys.len()];
        let mut max_x = vec![f32::MIN; line_ys.len()];
        for item in &placed {
            let w = font.glyph_width(item.ch, item.size);
            min_x[item.line] = min_x[item.line].min(item.x);
            let mut right_w = item.x + w;
            if item.miswrite && params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                let rw = font.glyph_width(item.correct_ch, item.size);
                right_w = right_w.max(item.rewrite_x + rw);
            }
            max_x[item.line] = max_x[item.line].max(right_w);
        }
        Some(
            (0..line_ys.len())
                .map(|li| {
                    if min_x[li] > max_x[li] {
                        0.0
                    } else {
                        (width_f - (max_x[li] - min_x[li])) / 2.0 - min_x[li]
                    }
                })
                .collect(),
        )
    } else {
        None
    };

    // 阶段二：按段落实际高度创建画布并绘制（不被页高裁剪）
    let canvas_h = (y + params.font_size + 4.0 * params.line_spacing_sigma + 4.0).max(1.0);
    let canvas_h = canvas_h as usize;
    let mut mask = vec![false; width * canvas_h];
    for item in &placed {
        let shift = match (&shifts, &center_shifts) {
            (Some(s), _) => s[item.line],
            (None, Some(c)) => c[item.line],
            (None, None) => 0.0,
        };
        let dx = item.x + shift;
        let baseline_y = item.y + font.ascent(item.size);
        font.rasterize(item.ch, item.size, dx, baseline_y, &mut mask, width, canvas_h);
        if item.miswrite {
            draw_miswrite(
                &mut mask,
                width,
                canvas_h,
                font,
                item.ch,
                item.correct_ch,
                dx,
                item.y,
                item.size,
                item.angle,
                params.miswrite_rewrite_mode == MiswriteMode::Above,
                params.miswrite_strikeout_style,
                rng,
            );
            if params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                font.rasterize(item.correct_ch, item.size, item.rewrite_x + shift, baseline_y, &mut mask, width, canvas_h);
            }
        }
    }

    // 按行提取墨迹：行带分组 → 归属各行，空行补 (None, 0.0)。
    // 一行可能包含多个行带（Above 模式的小字重写悬浮在行顶上方），
    // 全部归入该行合并提取，避免多余行带被丢弃。
    let rows: Vec<bool> = mask.chunks(width).map(|r| r.iter().any(|&b| b)).collect();
    let bands = split_text_rows(&rows);
    let mut bi = 0usize;
    let off_max = 0.8 * line_spacing;
    let mut lines: Vec<(Option<Vec<bool>>, f32)> = Vec::new();
    for &yk in &line_ys {
        if bi < bands.len() && (bands[bi].0 as f32) < yk + line_spacing / 2.0 {
            let s0 = bands[bi].0;
            let mut e = bands[bi].1;
            bi += 1;
            while bi < bands.len() && (bands[bi].0 as f32) < yk + line_spacing / 2.0 {
                e = bands[bi].1;
                bi += 1;
            }
            // 对齐基准取切片起点 s0（可能为上方悬浮小字带顶），
            // 使所有行保持画布绝对位置；下限放宽容纳小字带
            let off_min = -0.85 * params.font_size - 0.25 * line_spacing;
            let off = ((s0 as f32 - yk).max(off_min)).min(off_max);
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
    let mut page_has_ink = false;
    let mut draw_y = params.top_margin + lead;
    for (band, off) in all_lines {
        // 用 dirty 标志替代整页 any() 扫描（原实现每行 O(WxH)，文档级二次方）
        if draw_y > limit && page_has_ink {
            pages.push(std::mem::take(&mut page_canvas));
            page_canvas = vec![false; width * height];
            page_has_ink = false;
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
                        page_has_ink = true;
                    }
                }
            }
        }
        draw_y += line_spacing;
    }
    if page_has_ink || pages.is_empty() {
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

    /// 段落路径：Above 模式首行小字带悬浮于行顶上方，行带切片起点 s0 须锚定网格
    /// （off = s0 - yk0），否则整行墨迹相对网格下移（s_main 锚点回归）。
    #[test]
    fn paragraph_miswrite_keeps_line_grid_position() {
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
        let lines = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        let (band, off) = &lines[0];
        let band = band.as_ref().expect("首行应有墨迹");
        assert!(band.iter().any(|&b| b), "首行应有墨迹");
        assert!(*off <= 0.0, "小字带顶应在网格顶或之上：off={off}");
        // 主行带 = 带内最后一个连续墨迹段；其页面位置 = off + 段起点
        let band_rows: Vec<bool> = band.chunks(600).map(|r| r.iter().any(|&b| b)).collect();
        let main_start = split_text_rows(&band_rows).last().map(|(s, _)| *s).unwrap() as f32;
        let main_top = *off + main_start;
        // 基线（错字率=0）：单行带，主内容顶的页面位置 = off0 + 0
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let base = layout_paragraph(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        let (base_band, base_off) = &base[0];
        let base_band = base_band.as_ref().expect("基线首行应有墨迹");
        let base_main_top = *base_off
            + base_band.chunks(600).position(|r| r.iter().any(|&b| b)).unwrap() as f32;
        assert!(
            (main_top - base_main_top).abs() < 2.0,
            "错字不应移动行主体位置：off={off} main_top={main_top} base={base_main_top}"
        );
    }

    /// 段落 Rewrite 模式：重写字符紧邻错字、不被下一字符覆盖，后续字符随之右移。
    /// 旧实现只把重写画在错字后一格（被下一字符覆盖），仅最末字符的重写幸存，
    /// 最右墨迹只比单排布局多出约一个字形槽位（w+ws≈41）；修复后三个错字各
    /// 推进一槽，最右墨迹超出单排布局两个以上字形槽位。
    #[test]
    fn paragraph_miswrite_rewrite_not_covered() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.miswrite_rate = 1.0;
        p.miswrite_rewrite_mode = MiswriteMode::Rewrite;
        let mut pa = para();
        pa.text = "甲乙丙".into();
        let lines = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &pa, 600);
        let all_ink = lines.iter().filter_map(|(m, _)| m.as_ref()).collect::<Vec<_>>();
        let last_ink_x = all_ink.iter().flat_map(|m| m.chunks(600).flat_map(|row| row.iter().rposition(|&b| b))).max().unwrap();
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let base = layout_paragraph(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &pa, 600);
        let base_last_x = base.iter().filter_map(|(m, _)| m.as_ref()).flat_map(|m| m.chunks(600).flat_map(|row| row.iter().rposition(|&b| b))).max().unwrap();
        assert!(
            last_ink_x > base_last_x + 60,
            "Rewrite 应把最右墨迹推到更远处：{last_ink_x} vs {base_last_x}"
        );
    }

    /// 居中段落：上方小字带必须与锚定字符同移——与左对齐渲染相比主带整体平移 c 时，
    /// 小字带相对主带的偏移应保持不变（独立居中会把小字带单独甩到行中心）。
    #[test]
    fn paragraph_center_keeps_floats_with_anchor() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.miswrite_rate = 0.15;
        p.miswrite_rewrite_mode = MiswriteMode::Above;
        let mut pa = para();
        pa.text = "今天天气很好我们去公园散步。".into();
        pa.align = Align::Center;
        let centered = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        pa.align = Align::Left;
        let left = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        // 各渲染首行墨迹按行带分段求 x 范围（段序：小字带在上、主带在下）
        let seg_extents = |lines: &[(Option<Vec<bool>>, f32)]| -> Vec<(usize, usize)> {
            let band = lines[0].0.as_ref().expect("首行应有墨迹");
            let rows: Vec<bool> = band.chunks(600).map(|r| r.iter().any(|&b| b)).collect();
            split_text_rows(&rows)
                .into_iter()
                .map(|(s, e)| {
                    let (mut min_x, mut max_x) = (usize::MAX, 0usize);
                    for (x, &b) in band[s * 600..e * 600].iter().enumerate() {
                        if b {
                            min_x = min_x.min(x % 600);
                            max_x = max_x.max(x % 600);
                        }
                    }
                    (min_x, max_x)
                })
                .collect()
        };
        let c_segs = seg_extents(&centered);
        let l_segs = seg_extents(&left);
        assert_eq!(c_segs.len(), l_segs.len(), "两种对齐的行带段数应一致");
        assert!(c_segs.len() >= 2, "应存在小字带与主带两个行带段：{}", c_segs.len());
        let rel_c = c_segs[0].0 as isize - c_segs.last().unwrap().0 as isize;
        let rel_l = l_segs[0].0 as isize - l_segs.last().unwrap().0 as isize;
        assert!(
            (rel_c - rel_l).abs() <= 2,
            "小字带应随主带同移：rel_c={rel_c} rel_l={rel_l}"
        );
    }

    #[test]
    fn test_wrong_character_generation() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let ch = '中';
        let wrong = get_wrong_char(ch, &mut rng);
        assert_ne!(ch, wrong);
        assert!(('\u{4e00}'..='\u{9fa5}').contains(&wrong));

        let ch_eng = 'A';
        let wrong_eng = get_wrong_char(ch_eng, &mut rng);
        assert_ne!(ch_eng, wrong_eng);
        assert!(wrong_eng.is_ascii_uppercase());
    }

    #[test]
    fn test_draw_bezier_line() {
        let mut mask = vec![false; 100 * 100];
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        draw_bezier_line(&mut mask, 100, 100, 10.0, 10.0, 90.0, 90.0, 2.0, 5.0, &mut rng);
        assert!(mask.iter().any(|&b| b), "draw_bezier_line should modify mask");
    }
}
