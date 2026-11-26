//! issue #6「符号图形」手写质感对照样张。
//!
//! 运行：`cargo run -p handwrite-sim --example shapes_handwriting_demo`
//!
//! 产出 `tmp_shapes/shapes_handwriting_demo.png`（整页，1:1）与
//! `tmp_shapes/shapes_handwriting_demo_zoom.png`（▶ / 方括号 / 长线 3 倍放大），
//! 两者用来对比 issue 附图里真实手写笔记的笔迹：
//! 线宽接近真实中性笔、边界有毛边与羽化、实心 ▶ 是「涂」出来的、
//! 方括号转角是圆的。

use std::path::Path;

use handwrite_sim::core::shapes::{BraceSide, BracketSide, ShapeKind, ShapeSpec, shape_coverage};
use image::{Rgb, RgbImage, imageops};

const W: u32 = 1180;
const H: u32 = 860;
const INK: [u8; 3] = [26, 27, 30];

fn scene() -> Vec<ShapeSpec> {
    // 真实手写 ≈ 5px 中性笔（对应 A4@300dpi 的 0.5mm 笔尖），抖动 ≈ 线宽 ×0.6
    let thick = 5.0;
    let jitter = 3.2;
    let mut shapes = vec![
        // ① 框住一段文字（issue 图里被绿框标出的方框）
        ShapeSpec::new(ShapeKind::Rect, 60.0, 60.0, 330.0, 130.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x201),
        // ② 分隔栏：一条又长又粗的竖线
        ShapeSpec::new(ShapeKind::Line, 440.0, 60.0, 0.0, 260.0)
            .with_points(vec![[440.0, 62.0], [444.0, 322.0]])
            .with_thickness(thick * 1.4)
            .with_jitter(jitter)
            .with_seed(0x202),
        // ③ 圈住数字的圈
        ShapeSpec::new(ShapeKind::Ellipse, 520.0, 60.0, 96.0, 88.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x203),
        // ④ 圈之间的连线 + 箭头
        ShapeSpec::new(ShapeKind::Arrow, 0.0, 0.0, 0.0, 0.0)
            .with_points(vec![[636.0, 104.0], [760.0, 150.0]])
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x204),
        ShapeSpec::new(ShapeKind::Ellipse, 780.0, 130.0, 86.0, 80.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x205),
        // ⑤ `[` `]` 圈住一段列表（issue 图右侧的方括号）
        ShapeSpec::new(ShapeKind::Bracket(BracketSide::Left), 940.0, 70.0, 34.0, 240.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x206),
        ShapeSpec::new(ShapeKind::Bracket(BracketSide::Right), 1090.0, 70.0, 34.0, 240.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x207),
        // ⑥ 大括号归组三行
        ShapeSpec::new(ShapeKind::Brace(BraceSide::Right), 120.0, 470.0, 54.0, 300.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x208),
        // ⑦ 手写勾
        ShapeSpec::new(ShapeKind::Check, 300.0, 640.0, 130.0, 70.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x209),
    ];
    // ⑧ 行首实心三角项目符号（issue 图里的 ▶）：三种常见尺寸，验证小图元不会糊成墨团
    for (i, (y, size)) in [(470.0f32, 10.0f32), (530.0, 14.0), (590.0, 20.0)].iter().enumerate() {
        shapes.push(
            ShapeSpec::new(ShapeKind::Triangle, 44.0, *y, *size * 1.1, *size)
                .with_filled(true)
                .with_thickness(thick * 0.8)
                .with_jitter(jitter * 0.6)
                .with_seed(0x300 + i as u64),
        );
    }
    shapes
}

/// 按覆盖率把图形混合进 RGB 画布（与引擎合成层同一套 alpha 逻辑）。
fn draw(image: &mut RgbImage, shapes: &[ShapeSpec]) {
    for shape in shapes {
        let Some((bx, by, bw, bh, cov)) = shape_coverage(shape, W as usize, H as usize) else {
            continue;
        };
        for row in 0..bh {
            for col in 0..bw {
                let a = cov[row * bw + col];
                if a <= 0.004 {
                    continue;
                }
                let px = image.get_pixel_mut((bx + col) as u32, (by + row) as u32);
                for c in 0..3 {
                    px.0[c] = (f32::from(INK[c]) * a + f32::from(px.0[c]) * (1.0 - a))
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

fn cropped(image: &RgbImage, x: u32, y: u32, w: u32, h: u32) -> RgbImage {
    imageops::crop_imm(image, x.min(image.width()), y.min(image.height()), w, h).to_image()
}

/// 最近邻放大：看的是像素本身（毛边/羽化），不能用插值糊掉。
fn zoom(image: &RgbImage, factor: u32) -> RgbImage {
    imageops::resize(
        image,
        image.width() * factor,
        image.height() * factor,
        imageops::FilterType::Nearest,
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new("tmp_shapes");
    std::fs::create_dir_all(out_dir)?;

    let mut image = RgbImage::from_pixel(W, H, Rgb([255, 255, 255]));
    draw(&mut image, &scene());
    let page_path = out_dir.join("shapes_handwriting_demo.png");
    image.save(&page_path)?;
    println!("{}（{}×{}）", page_path.display(), W, H);

    // 3 倍放大三个最容易被指摘的细节：实心 ▶、方括号转角、长竖线的收笔
    let tile_a = zoom(&cropped(&image, 24, 452, 210, 180), 3);
    let tile_b = zoom(&cropped(&image, 920, 52, 220, 280), 3);
    let tile_c = zoom(&cropped(&image, 412, 40, 80, 300), 3);
    let gap = 18;
    let total_w = tile_a.width() + tile_b.width() + tile_c.width() + gap * 4;
    let total_h = tile_a.height().max(tile_b.height()).max(tile_c.height()) + gap * 2;
    let mut sheet = RgbImage::from_pixel(total_w, total_h, Rgb([236, 236, 238]));
    imageops::replace(&mut sheet, &tile_a, gap as i64, gap as i64);
    imageops::replace(
        &mut sheet,
        &tile_b,
        (gap * 2 + tile_a.width()) as i64,
        gap as i64,
    );
    imageops::replace(
        &mut sheet,
        &tile_c,
        (gap * 3 + tile_a.width() + tile_b.width()) as i64,
        gap as i64,
    );
    let zoom_path = out_dir.join("shapes_handwriting_demo_zoom.png");
    sheet.save(&zoom_path)?;
    println!(
        "{}（{}×{}；左：▶ 实心三角 中：方括号 右：长竖线）",
        zoom_path.display(),
        sheet.width(),
        sheet.height()
    );
    Ok(())
}
