//! 全部图形笔触连续性回归样张。
//!
//! 运行：
//! - `cargo run -p handwrite-sim --example shapes_all_strokes_demo -- before`
//! - `cargo run -p handwrite-sim --example shapes_all_strokes_demo -- after`
//!
//! 覆盖矩形、椭圆、大括号、方括号、三角形和勾；`after` 模式在存在
//! `before` 图片时生成上下拼接的对比图。

use std::path::Path;

use handwrite_sim::core::shapes::{BraceSide, BracketSide, ShapeKind, ShapeSpec};
use image::{Rgb, RgbImage};

const W: u32 = 1000;
const H: u32 = 760;

fn scene() -> Vec<ShapeSpec> {
    let thick = 2.4;
    let jitter = 2.16;
    vec![
        ShapeSpec::new(ShapeKind::Rect, 50.0, 50.0, 430.0, 130.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x101),
        ShapeSpec::new(ShapeKind::Ellipse, 550.0, 30.0, 400.0, 180.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x102),
        ShapeSpec::new(ShapeKind::Brace(BraceSide::Right), 70.0, 250.0, 60.0, 250.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x103),
        ShapeSpec::new(ShapeKind::Bracket(BracketSide::Left), 590.0, 250.0, 60.0, 250.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x104),
        ShapeSpec::new(ShapeKind::Triangle, 50.0, 560.0, 430.0, 170.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x105),
        ShapeSpec::new(ShapeKind::Check, 550.0, 570.0, 400.0, 150.0)
            .with_thickness(thick)
            .with_jitter(jitter)
            .with_seed(0x106),
    ]
}

fn render() -> RgbImage {
    let mut mask = vec![false; W as usize * H as usize];
    for shape in scene() {
        handwrite_sim::core::shapes::rasterize_shape(
            &shape,
            &mut mask,
            W as usize,
            H as usize,
            0,
            0,
        );
    }
    let mut image = RgbImage::from_pixel(W, H, Rgb([255, 255, 255]));
    for (i, &on) in mask.iter().enumerate() {
        if on {
            image.put_pixel(
                (i % W as usize) as u32,
                (i / W as usize) as u32,
                Rgb([28, 28, 30]),
            );
        }
    }
    image
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let label = std::env::args().nth(1).unwrap_or_else(|| "after".into());
    let out_dir = Path::new("tmp_shapes");
    std::fs::create_dir_all(out_dir)?;
    let image = render();
    let path = out_dir.join(format!("shapes_all_strokes_{label}.png"));
    image.save(&path)?;
    println!("{}（{}×{}）", path.display(), W, H);

    if label == "after" {
        let before_path = out_dir.join("shapes_all_strokes_before.png");
        if before_path.is_file() {
            let before = image::open(&before_path)?.to_rgb8();
            let gap = 24;
            let mut compare = RgbImage::from_pixel(W, H * 2 + gap, Rgb([238, 238, 238]));
            image::imageops::replace(&mut compare, &before, 0, 0);
            image::imageops::replace(&mut compare, &image, 0, (H + gap) as i64);
            let compare_path = out_dir.join("shapes_all_strokes_before_after.png");
            compare.save(&compare_path)?;
            println!(
                "{}（{}×{}；上：修改前；下：修改后）",
                compare_path.display(),
                compare.width(),
                compare.height()
            );
        }
    }
    Ok(())
}
