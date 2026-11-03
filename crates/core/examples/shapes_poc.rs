//! issue #6「增加符号图形」渲染 POC。
//!
//! 一次性演示用户截图里的真实用法：圈注数字、框住文字、文字块之间的
//! 连线/箭头、竖线分隔栏、右侧大括号分组、▶ 填充三角项目符号、方括号、
//! 勾、下划线。
//!
//! 所有图形都由 `core::shapes` 的矢量基元 + 手绘抖动光栅化进 bool 掩码，
//! 再走既有 `perturb` 图层机制（`perturb_mask` / `perturb_styled_layer_into`
//! / `draw_printed_mask`）合成彩色墨迹——与真实引擎的「连通域扰动」管线
//! 完全一致，用来验证符号图形可以直接并入现有图层。
//!
//! 运行：`cargo run -p handwrite-sim --example shapes_poc`
//! 输出：`tmp_shapes/shapes_poc.png`（1200×800，白底）
//!
//! 正文用系统 CJK 字体渲染（方便观察图形与文字的相对位置）；找不到字体时
//! 退化为短笔画占位，图形演示不受影响。

use std::path::Path;

use handwrite_sim::core::font::FontFace;
use handwrite_sim::core::layout::PerturbStyle;
use handwrite_sim::core::models::HandwritingParams;
use handwrite_sim::core::perturb;
use handwrite_sim::core::shapes::{self, BraceSide, BracketSide, ShapeKind, ShapeSpec};
use image::RgbImage;
use rand::{rngs::StdRng, SeedableRng};

const W: usize = 1200;
const H: usize = 800;

const INK: [u8; 3] = [30, 30, 34];
const PRINT: [u8; 3] = [120, 120, 130];
const RED: [u8; 3] = [205, 55, 48];
const GREEN: [u8; 3] = [28, 138, 80];
const BLUE: [u8; 3] = [40, 95, 190];
const ORANGE: [u8; 3] = [226, 128, 24];
const PURPLE: [u8; 3] = [120, 72, 168];
const GRAY: [u8; 3] = [105, 110, 120];

/// 依次尝试常见系统 CJK 字体；返回字体与路径（用于日志）。
fn system_font() -> Option<(FontFace, &'static str)> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"/System/Library/Fonts/PingFang.ttc",
        r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        r"/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in CANDIDATES {
        if !Path::new(path).is_file() {
            continue;
        }
        if let Ok(face) = FontFace::load(Path::new(path), 26.0) {
            return Some((face, path));
        }
    }
    None
}

/// 文本宽度（无字体时按 CJK 1 em / ASCII 0.55 em / 空白 0.35 em 估算）。
fn advance(face: Option<&FontFace>, text: &str, size: f32) -> f32 {
    match face {
        Some(face) => text.chars().map(|ch| face.glyph_width(ch, size)).sum(),
        None => text
            .chars()
            .map(|ch| {
                if ch.is_whitespace() {
                    size * 0.35
                } else if ch.is_ascii() {
                    size * 0.55
                } else {
                    size
                }
            })
            .sum(),
    }
}

/// 字形顶部到基线的距离（无字体时按 0.8 em 估算）。
fn ascent(face: Option<&FontFace>, size: f32) -> f32 {
    match face {
        Some(face) => face.ascent(size),
        None => size * 0.8,
    }
}

/// 在掩码上画一行文本，返回行尾 x。
fn draw_text(
    face: Option<&FontFace>,
    mask: &mut [bool],
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
) -> f32 {
    match face {
        Some(face) => {
            let mut pen_x = x;
            for ch in text.chars() {
                face.rasterize(ch, size, pen_x, baseline, mask, W, H);
                pen_x += face.glyph_width(ch, size);
            }
            pen_x
        }
        None => {
            // 无系统字体：每个字符用一根短斜线占位，保证版面坐标仍然可读
            let mut pen_x = x;
            for (i, ch) in text.chars().enumerate() {
                let cw = if ch.is_whitespace() {
                    size * 0.35
                } else if ch.is_ascii() {
                    size * 0.55
                } else {
                    size
                };
                if !ch.is_whitespace() {
                    let top = baseline - size * 0.72;
                    let spec = ShapeSpec::new(ShapeKind::Line, 0.0, 0.0, 0.0, 0.0)
                        .with_points(vec![
                            [pen_x + cw * 0.2, top + size * 0.05],
                            [pen_x + cw * 0.78, baseline - size * 0.12],
                        ])
                        .with_thickness((size * 0.08).max(1.2))
                        .with_jitter(0.8)
                        .with_seed(0xF00D_0000 + i as u64);
                    shapes::rasterize_shape(&spec, mask, W, H, 0, 0);
                }
                pen_x += cw;
            }
            pen_x
        }
    }
}

/// 在框内水平居中画一行文本。
fn draw_text_centered(
    face: Option<&FontFace>,
    mask: &mut [bool],
    text: &str,
    box_x: f32,
    box_w: f32,
    baseline: f32,
    size: f32,
) {
    let text_w = advance(face, text, size);
    draw_text(
        face,
        mask,
        text,
        box_x + (box_w - text_w) * 0.5,
        baseline,
        size,
    );
}

/// 往指定图层掩码里放一个图形。
fn put(mask: &mut [bool], spec: ShapeSpec) {
    shapes::rasterize_shape(&spec, mask, W, H, 0, 0);
}

/// 一个彩色符号图层：先按连通域做笔画扰动，再写入整页画布。
fn ink_layer(canvas: &mut [u8], mask: &[bool], fill: [u8; 3], rng: &mut StdRng) {
    if !mask.iter().any(|&b| b) {
        return;
    }
    // 图形本身已有手绘抖动，这里只叠加很轻的逐笔画位移/旋转，
    // 与真实引擎的文字图层使用同一套 PerturbStyle 机制。
    let style = PerturbStyle::new(fill, false, 0.6, 0.6, 0.004);
    perturb::perturb_styled_layer_into(mask, W, H, &style, rng, canvas);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let font = system_font();
    let face = font.as_ref().map(|(f, _)| f);
    if let Some((_, path)) = &font {
        println!("正文字体：{path}");
    } else {
        eprintln!("未找到系统 CJK 字体：正文改用短笔画占位（图形演示不受影响）");
    }

    // 独立图层掩码：与真实引擎一样，每个颜色层单独走扰动后合成
    let mut text_mask = vec![false; W * H];
    let mut print_mask = vec![false; W * H];
    let mut red = vec![false; W * H];
    let mut green = vec![false; W * H];
    let mut blue = vec![false; W * H];
    let mut orange = vec![false; W * H];
    let mut purple = vec![false; W * H];
    let mut gray = vec![false; W * H];

    // ---------- 页面标题 / 页脚（印刷体，验证 draw_printed_mask 兼容） ----------
    draw_text(
        face,
        &mut print_mask,
        "issue #6 · 符号图形渲染 POC（core/shapes）",
        50.0,
        52.0,
        24.0,
    );
    draw_text(
        face,
        &mut print_mask,
        "cargo run -p handwrite-sim --example shapes_poc  →  tmp_shapes/shapes_poc.png（1200×800）",
        50.0,
        776.0,
        16.0,
    );

    // ---------- 左侧：正文 + 虚拟标注 ----------
    let body = [
        "当文字需要被标注时，手写页面会显得更自然。",
        "绿色方框可以框住一整段需要强调的文字。",
        "红色圆圈能圈住数字 3，也能圈住关键词。",
        "不同段落之间用箭头或折线表示先后顺序。",
        "右侧的大括号把多行结论归成一组。",
        "方括号用来标记需要再次核对的内容。",
        "最后逐项打勾确认，避免漏掉细节。",
    ];
    let text_x = 70.0;
    let body_size = 26.0;
    let line_h = 42.0;
    let mut baselines = Vec::with_capacity(body.len());
    for (i, line) in body.iter().enumerate() {
        let baseline = 130.0 + i as f32 * line_h;
        draw_text(face, &mut text_mask, line, text_x, baseline, body_size);
        baselines.push(baseline);
    }
    let body_asc = ascent(face, body_size);

    // 绿色方框：框住第 2 行整段文字（上下留白避开字形）
    let line2 = body[1];
    let line2_w = advance(face, line2, body_size);
    put(
        &mut green,
        ShapeSpec::new(
            ShapeKind::Rect,
            58.0,
            baselines[1] - body_asc - 9.0,
            line2_w + 24.0,
            body_asc + 22.0,
        )
        .with_thickness(2.6)
        .with_fill(GREEN)
        .with_jitter(1.5)
        .with_seed(0x21),
    );

    // 橙色下划线：第 1 行“手写页面”
    let prefix1 = "当文字需要被标注时，";
    let underline_x = text_x + advance(face, prefix1, body_size);
    let underline_w = advance(face, "手写页面", body_size);
    put(
        &mut orange,
        ShapeSpec::new(
            ShapeKind::Underline,
            underline_x,
            baselines[0] + 7.0,
            underline_w,
            1.0,
        )
        .with_thickness(3.2)
        .with_fill(ORANGE)
        .with_jitter(1.2)
        .with_seed(0x22),
    );

    // 红色椭圆：圈住第 3 行的数字 3
    let line3 = body[2];
    let digit_at = line3.find('3').expect("正文第 3 行应包含数字 3");
    let x3 = text_x + advance(face, &line3[..digit_at], body_size);
    let w3 = advance(face, "3", body_size);
    put(
        &mut red,
        ShapeSpec::new(
            ShapeKind::Ellipse,
            x3 - 9.0,
            baselines[2] - body_asc - 9.0,
            w3 + 18.0,
            body_asc + 20.0,
        )
        .with_thickness(2.6)
        .with_fill(RED)
        .with_jitter(1.6)
        .with_seed(0x23),
    );

    // 橙色方括号：第 6 行句末“核对的内容。”——句末有留白，括号不会压到邻字
    let line6 = body[5];
    let target = "核对的内容。";
    let needle_at = line6.find(target).expect("正文第 6 行应包含“核对的内容。”");
    let x6 = text_x + advance(face, &line6[..needle_at], body_size);
    let w6 = advance(face, target, body_size);
    let bracket_top = baselines[5] - body_asc - 5.0;
    let bracket_h = body_asc + 12.0;
    put(
        &mut orange,
        ShapeSpec::new(
            ShapeKind::Bracket(BracketSide::Left),
            x6 - 18.0,
            bracket_top,
            11.0,
            bracket_h,
        )
        .with_thickness(2.8)
        .with_fill(ORANGE)
        .with_jitter(1.0)
        .with_seed(0x24),
    );
    put(
        &mut orange,
        ShapeSpec::new(
            ShapeKind::Bracket(BracketSide::Right),
            x6 + w6 + 6.0,
            bracket_top,
            11.0,
            bracket_h,
        )
        .with_thickness(2.8)
        .with_fill(ORANGE)
        .with_jitter(1.0)
        .with_seed(0x25),
    );

    // 紫色右大括号：把第 4~7 行归成一组
    let group_right = text_x
        + body[3..7]
            .iter()
            .map(|line| advance(face, line, body_size))
            .fold(0.0_f32, f32::max);
    let group_top = baselines[3] - body_asc - 6.0;
    let group_bottom = baselines[6] + 8.0;
    put(
        &mut purple,
        ShapeSpec::new(
            ShapeKind::Brace(BraceSide::Right),
            group_right + 18.0,
            group_top,
            26.0,
            group_bottom - group_top,
        )
        .with_thickness(2.8)
        .with_fill(PURPLE)
        .with_jitter(1.3)
        .with_seed(0x26),
    );

    // 竖线分隔栏
    put(
        &mut gray,
        ShapeSpec::new(ShapeKind::Line, 655.0, 90.0, 0.0, 630.0)
            .with_thickness(2.0)
            .with_fill(GRAY)
            .with_jitter(0.8)
            .with_seed(0x31),
    );

    // ---------- 右侧：符号元件演示区 ----------
    draw_text(
        face,
        &mut text_mask,
        "符号元件 · 矢量基元 + 手绘抖动",
        690.0,
        130.0,
        20.0,
    );

    // 两个文字块 + 水平箭头 + 折线回路：文字块之间的连线/箭头
    put(
        &mut blue,
        ShapeSpec::new(ShapeKind::Rect, 700.0, 160.0, 120.0, 56.0)
            .with_thickness(2.4)
            .with_fill(BLUE)
            .with_jitter(1.3)
            .with_seed(0x41),
    );
    put(
        &mut blue,
        ShapeSpec::new(ShapeKind::Rect, 960.0, 160.0, 120.0, 56.0)
            .with_thickness(2.4)
            .with_fill(BLUE)
            .with_jitter(1.3)
            .with_seed(0x42),
    );
    draw_text_centered(face, &mut text_mask, "确认底图", 700.0, 120.0, 196.0, 22.0);
    draw_text_centered(face, &mut text_mask, "导出 PDF", 960.0, 120.0, 196.0, 22.0);
    put(
        &mut blue,
        ShapeSpec::new(ShapeKind::Arrow, 0.0, 0.0, 0.0, 0.0)
            .with_points(vec![[823.0, 188.0], [955.0, 188.0]])
            .with_thickness(2.4)
            .with_fill(BLUE)
            .with_jitter(1.1)
            .with_seed(0x43),
    );
    put(
        &mut blue,
        ShapeSpec::new(ShapeKind::Polyline, 0.0, 0.0, 0.0, 0.0)
            .with_points(vec![[760.0, 216.0], [760.0, 266.0], [1018.0, 266.0]])
            .with_thickness(2.2)
            .with_fill(BLUE)
            .with_jitter(0.9)
            .with_seed(0x44),
    );
    put(
        &mut blue,
        ShapeSpec::new(ShapeKind::Arrow, 0.0, 0.0, 0.0, 0.0)
            .with_points(vec![[1018.0, 266.0], [1018.0, 220.0]])
            .with_thickness(2.2)
            .with_fill(BLUE)
            .with_jitter(1.0)
            .with_seed(0x45),
    );

    // 左大括号 + ▶ 填充三角项目符号 + 勾
    put(
        &mut purple,
        ShapeSpec::new(ShapeKind::Brace(BraceSide::Left), 676.0, 318.0, 22.0, 128.0)
            .with_thickness(2.6)
            .with_fill(PURPLE)
            .with_jitter(1.2)
            .with_seed(0x51),
    );
    let steps = ["确认底图清晰度", "调整字号和行距", "导出 PDF 并检查"];
    for (i, step) in steps.iter().enumerate() {
        let top = 330.0 + i as f32 * 40.0;
        put(
            &mut blue,
            ShapeSpec::new(ShapeKind::Triangle, 0.0, 0.0, 0.0, 0.0)
                .with_points(vec![[712.0, top], [712.0, top + 15.0], [725.0, top + 7.5]])
                .with_filled(true)
                .with_thickness(1.6)
                .with_fill(BLUE)
                .with_jitter(0.7)
                .with_seed(0x52 + i as u64),
        );
        draw_text(face, &mut text_mask, step, 738.0, top + 14.0, 22.0);
    }
    put(
        &mut green,
        ShapeSpec::new(ShapeKind::Check, 1000.0, 400.0, 30.0, 26.0)
            .with_thickness(3.0)
            .with_fill(GREEN)
            .with_jitter(1.1)
            .with_seed(0x55),
    );
    draw_text(face, &mut text_mask, "已确认", 1040.0, 422.0, 20.0);

    // 大字 + 红圈圈注数字 + 指向说明的红箭头
    let big_size = 64.0;
    let big_baseline = 612.0;
    let big_x = 708.0;
    let big_w = draw_text(face, &mut text_mask, "3", big_x, big_baseline, big_size) - big_x;
    let big_asc = ascent(face, big_size);
    let circle_x = big_x - big_w * 0.35;
    let circle_w = big_w * 1.7;
    let circle_h = big_asc + 24.0;
    put(
        &mut red,
        ShapeSpec::new(
            ShapeKind::Ellipse,
            circle_x,
            big_baseline - big_asc - 10.0,
            circle_w,
            circle_h,
        )
        .with_thickness(2.8)
        .with_fill(RED)
        .with_jitter(1.6)
        .with_seed(0x61),
    );
    put(
        &mut red,
        ShapeSpec::new(ShapeKind::Arrow, 0.0, 0.0, 0.0, 0.0)
            .with_points(vec![
                [circle_x + circle_w + 6.0, big_baseline - 30.0],
                [862.0, 606.0],
            ])
            .with_thickness(2.6)
            .with_fill(RED)
            .with_jitter(1.1)
            .with_seed(0x62),
    );
    draw_text(face, &mut text_mask, "重点检查", 876.0, 620.0, 24.0);

    // 橙色方括号：圈出“参考值 42”里的数字
    let ref_text = "参考值 42";
    let ref_x = 990.0;
    let ref_baseline = 600.0;
    draw_text(face, &mut text_mask, ref_text, ref_x, ref_baseline, 24.0);
    let ref_prefix = "参考值 ";
    let x42 = ref_x + advance(face, ref_prefix, 24.0);
    let w42 = advance(face, "42", 24.0);
    let ref_asc = ascent(face, 24.0);
    let ref_top = ref_baseline - ref_asc - 5.0;
    put(
        &mut orange,
        ShapeSpec::new(
            ShapeKind::Bracket(BracketSide::Left),
            x42 - 17.0,
            ref_top,
            10.0,
            ref_asc + 12.0,
        )
        .with_thickness(2.6)
        .with_fill(ORANGE)
        .with_jitter(1.0)
        .with_seed(0x63),
    );
    put(
        &mut orange,
        ShapeSpec::new(
            ShapeKind::Bracket(BracketSide::Right),
            x42 + w42 + 3.0,
            ref_top,
            10.0,
            ref_asc + 12.0,
        )
        .with_thickness(2.6)
        .with_fill(ORANGE)
        .with_jitter(1.0)
        .with_seed(0x64),
    );

    // 勾 + 文字
    put(
        &mut green,
        ShapeSpec::new(ShapeKind::Check, 1000.0, 650.0, 34.0, 30.0)
            .with_thickness(3.2)
            .with_fill(GREEN)
            .with_jitter(1.1)
            .with_seed(0x65),
    );
    draw_text(face, &mut text_mask, "已核对", 1044.0, 674.0, 22.0);

    // ---------- 合成：文字走 perturb_mask，彩色图形走分层扰动，印刷体零扰动 ----------
    let background = vec![255u8; W * H * 3];
    let text_params = HandwritingParams {
        fill: INK,
        perturb_x_sigma: 0.9,
        perturb_y_sigma: 0.9,
        perturb_theta_sigma: 0.012,
        ..Default::default()
    };
    let mut rng = StdRng::seed_from_u64(2026_1104);
    let mut canvas = perturb::perturb_mask(&text_mask, W, H, &text_params, &mut rng, &background);
    for (mask, fill) in [
        (&red, RED),
        (&green, GREEN),
        (&blue, BLUE),
        (&orange, ORANGE),
        (&purple, PURPLE),
        (&gray, GRAY),
    ] {
        ink_layer(&mut canvas, mask, fill, &mut rng);
    }
    perturb::draw_printed_mask(&print_mask, W, H, PRINT, &mut canvas);

    // ---------- 保存 ----------
    let out_dir = Path::new("tmp_shapes");
    std::fs::create_dir_all(out_dir)?;
    let out_path = out_dir.join("shapes_poc.png");
    RgbImage::from_raw(W as u32, H as u32, canvas)
        .expect("RGB 缓冲区尺寸应与页面尺寸一致")
        .save(&out_path)?;

    println!(
        "已生成 {}：{}×{}，正文 {} 行；图形覆盖 Line/Arrow/Polyline/Rect/Ellipse/Triangle/Brace/Bracket/Check/Underline",
        out_path.display(),
        W,
        H,
        body.len()
    );
    Ok(())
}
