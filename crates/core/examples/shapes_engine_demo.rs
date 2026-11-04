//! 图形标注（issue #6）端到端示例：走**真实引擎**渲染，而不是直接调用
//! `shapes::rasterize_shape`。
//!
//! 与 `shapes_poc.rs` 的区别：
//! - `shapes_poc.rs` 演示「图形掩码 + 分色图层」的渲染质感（研究阶段）；
//! - 本示例演示**产品数据流**：`HandwritingParams.shapes` → `engine::render_all_pages_preview`
//!   → 每页 PNG（图形按页码落到对应页、墨色可覆盖也可跟随主墨色、与正文一起合成）。
//!
//! 图形坐标本来由用户在预览图上拖拽得到；这里为了让样张好看，用
//! `FontFace::glyph_width` 按引擎同一套字体度量推算每个字的 x，把标注恰好套在文字上。
//!
//! 运行：`cargo run -p handwrite-sim --example shapes_engine_demo`
//! 产物：`tmp_shapes/shapes_engine_demo_0.png`（第 1 页）、`..._1.png`（第 2 页，只有图形）

use std::path::{Path, PathBuf};

use handwrite_sim::core::engine;
use handwrite_sim::core::font::FontFace;
use handwrite_sim::core::models::{Align, HandwritingParams, Paragraph};
use handwrite_sim::core::shapes::{BraceSide, BracketSide, ShapeKind, ShapeSpec};

const W: u32 = 1240;
const H: u32 = 1754;

// 与 params 保持一致的排版量（示例里用于把图形对齐到文字）
const LEFT: f32 = 90.0;
const TOP: f32 = 150.0;
const FONT_SIZE: f32 = 36.0;
const WORD_SPACING: f32 = 6.0;
const LINE_SPACING: f32 = 46.0;
/// 行高（字高 + 行距）
const LINE_H: f32 = FONT_SIZE + LINE_SPACING;

const TEXT_1: &str = "第一行文字用于演示框选、下划线与方括号。";
const TEXT_2: &str = "第二行用于演示圈注：标注重点。";

/// 一行文字里每个字（0 基）的左边缘 x，末尾多一项 = 整行右边缘。
/// 用引擎同一套 `FontFace::glyph_width` 度量，保证与渲染结果一致。
fn char_positions(face: Option<&FontFace>, text: &str) -> Vec<f32> {
    let mut xs = Vec::with_capacity(text.chars().count() + 1);
    let mut x = LEFT;
    for ch in text.chars() {
        xs.push(x);
        let advance = face.map_or(FONT_SIZE, |f| f.glyph_width(ch, FONT_SIZE));
        x += advance + WORD_SPACING;
    }
    xs.push(x - WORD_SPACING);
    xs
}

/// 第 n 行（0 基）字形顶部的 y：引擎首行顶 = top_margin + line_spacing。
fn line_top(n: usize) -> f32 {
    TOP + LINE_SPACING + n as f32 * LINE_H
}

/// 系统 CJK 字体（找不到就只渲染图形，示例仍可跑通）。
fn system_font() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"/System/Library/Fonts/PingFang.ttc",
        r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// 写一张纯白背景 PNG（引擎要求背景是真实文件）。
fn white_background(dir: &Path) -> PathBuf {
    let path = dir.join("shapes_engine_demo_bg.png");
    let img = image::RgbImage::from_pixel(W, H, image::Rgb([255, 255, 255]));
    img.save(&path).expect("写入背景 PNG 失败");
    path
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new("tmp_shapes");
    std::fs::create_dir_all(out_dir)?;
    let bg = white_background(out_dir);

    let system = system_font();
    let face = system
        .as_ref()
        .map(|p| FontFace::load(p, FONT_SIZE))
        .transpose()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let face = face.as_ref();
    let (text, font_path) = match &system {
        Some(p) => (TEXT_1.to_string(), p.to_string_lossy().into_owned()),
        None => {
            eprintln!("未找到系统 CJK 字体：本示例只渲染图形（坐标仍按文字布局推算）");
            (String::new(), String::new())
        }
    };

    let xs1 = char_positions(face, TEXT_1);
    let xs2 = char_positions(face, TEXT_2);
    let glyph_top_1 = line_top(0);
    let glyph_top_2 = line_top(1);
    // 行内文字的实际左右边界（末项 = 整行右边缘）
    let line1_right = xs1[xs1.len() - 1];
    let row2_end = xs2[xs2.len() - 1];

    let shapes = vec![
        // 绿色矩形：框住第 1 行整句
        ShapeSpec::new(
            ShapeKind::Rect,
            xs1[0] - 12.0,
            glyph_top_1 - 10.0,
            (line1_right - xs1[0]) + 24.0,
            FONT_SIZE + 22.0,
        )
        .with_thickness(2.6)
        .with_fill([26, 127, 55])
        .with_jitter(1.5)
        .with_seed(0x11),
        // 橙色下划线：第 1 行「演示框选」（索引 8..12）
        ShapeSpec::new(
            ShapeKind::Underline,
            xs1[8],
            glyph_top_1 + FONT_SIZE + 12.0,
            xs1[12] - xs1[8],
            2.0,
        )
        .with_thickness(3.0)
        .with_fill([210, 120, 20])
        .with_jitter(1.1)
        .with_seed(0x12),
        // 橙色方括号：夹住句末「方括号。」（索引 16..20）
        ShapeSpec::new(
            ShapeKind::Bracket(BracketSide::Left),
            xs1[16] - 6.0,
            glyph_top_1 - 6.0,
            24.0,
            FONT_SIZE + 14.0,
        )
        .with_thickness(2.6)
        .with_fill([210, 120, 20])
        .with_jitter(1.2)
        .with_seed(0x13),
        ShapeSpec::new(
            ShapeKind::Bracket(BracketSide::Right),
            xs1[20] + 2.0,
            glyph_top_1 - 6.0,
            24.0,
            FONT_SIZE + 14.0,
        )
        .with_thickness(2.6)
        .with_fill([210, 120, 20])
        .with_jitter(1.2)
        .with_seed(0x14),
        // 红色椭圆：圈住第 2 行「标注重点。」（索引 10..15）
        ShapeSpec::new(
            ShapeKind::Ellipse,
            xs2[10] - 10.0,
            glyph_top_2 - 9.0,
            (xs2[15] - xs2[10]) + 20.0,
            FONT_SIZE + 20.0,
        )
        .with_thickness(2.8)
        .with_fill([200, 50, 50])
        .with_jitter(1.6)
        .with_seed(0x15),
        // 右大括号：右侧把两行归组（不设 fill → 跟随主墨色）
        ShapeSpec::new(
            ShapeKind::Brace(BraceSide::Right),
            line1_right + 36.0,
            glyph_top_1 - 8.0,
            44.0,
            LINE_H + FONT_SIZE + 20.0,
        )
        .with_thickness(2.8)
        .with_jitter(1.2)
        .with_seed(0x16),
        // 蓝色箭头：从右下方空白指向被圈住的「标注重点。」
        ShapeSpec::new(
            ShapeKind::Arrow,
            row2_end + 150.0,
            glyph_top_2 + 150.0,
            -150.0,
            -120.0,
        )
        .with_thickness(2.8)
        .with_fill([30, 90, 200])
        .with_jitter(1.4)
        .with_seed(0x17),
        // 绿色勾：第 2 行句末右侧「已核对」
        ShapeSpec::new(
            ShapeKind::Check,
            row2_end + 44.0,
            glyph_top_2 - 2.0,
            50.0,
            42.0,
        )
        .with_thickness(3.2)
        .with_fill([26, 127, 55])
        .with_jitter(1.4)
        .with_seed(0x18),
        // 蓝色实心三角：第 2 行行首项目符号（MVP 里由用户手动摆放；行内 bullet 属排版需求）
        ShapeSpec::new(
            ShapeKind::Triangle,
            LEFT - 36.0,
            glyph_top_2 + 8.0,
            24.0,
            26.0,
        )
        .with_filled(true)
        .with_thickness(2.0)
        .with_fill([30, 90, 200])
        .with_jitter(0.6)
        .with_seed(0x19),
        // 第 2 页：只有图形没有文字，验证 page 路由与「纯图形页」页数计算
        ShapeSpec::new(ShapeKind::Rect, 140.0, 180.0, 900.0, 320.0)
            .with_thickness(2.8)
            .with_fill([120, 60, 160])
            .with_jitter(1.5)
            .with_seed(0x1a)
            .with_page(2),
    ];

    let params = HandwritingParams {
        font_path,
        background_path: bg.to_string_lossy().into_owned(),
        text: text.clone(),
        paragraphs: vec![
            Paragraph { text, align: Align::Left, ..Default::default() },
            Paragraph {
                text: TEXT_2.to_string(),
                align: Align::Left,
                ..Default::default()
            },
        ],
        shapes,
        // 主墨色：未显式指定 fill 的图形（如右大括号）跟它走
        fill: [22, 22, 22],
        font_size: FONT_SIZE,
        word_spacing: WORD_SPACING,
        line_spacing: LINE_SPACING,
        left_margin: LEFT,
        right_margin: LEFT,
        top_margin: TOP,
        bottom_margin: 90.0,
        word_spacing_sigma: 1.0,
        line_spacing_sigma: 1.0,
        font_size_sigma: 0.8,
        perturb_x_sigma: 1.1,
        perturb_y_sigma: 1.1,
        perturb_theta_sigma: 0.02,
        ..Default::default()
    };

    let pages = engine::render_all_pages_preview(&params, 2026_1104)?;
    println!("生成 {} 页（页数由正文/图形页码共同决定）", pages.len());
    for (i, page) in pages.iter().enumerate() {
        let path = out_dir.join(format!("shapes_engine_demo_{i}.png"));
        page.save(&path)?;
        println!("  {}（{}×{}）", path.display(), page.width(), page.height());
    }
    Ok(())
}

