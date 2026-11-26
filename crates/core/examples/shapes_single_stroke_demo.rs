//! 长直线穿过大椭圆的双层笔触回归样张。
//!
//! 运行：
//! - `cargo run -p handwrite-sim --example shapes_single_stroke_demo -- before`
//! - `cargo run -p handwrite-sim --example shapes_single_stroke_demo -- after`
//!
//! 两个模式使用完全相同的坐标、线宽、抖动和 seed；修改画笔实现后，
//! 主笔几何应保持不变，只去掉整段复描。`after` 模式还会在存在 `before`
//! 图片时生成上下拼接的对比图。

use std::path::Path;

use handwrite_sim::core::shapes::{ShapeKind, ShapeSpec};
use image::{Rgb, RgbImage};

const W: u32 = 992;
const H: u32 = 120;

fn scene() -> Vec<ShapeSpec> {
    vec![
        ShapeSpec::new(ShapeKind::Line, 28.0, 38.0, 936.0, 0.0)
            .with_points(vec![[28.0, 38.0], [964.0, 38.0]])
            .with_thickness(2.4)
            .with_jitter(2.16)
            .with_seed(0x5EED_2026),
        ShapeSpec::new(ShapeKind::Ellipse, 386.0, 3.0, 220.0, 112.0)
            .with_thickness(2.4)
            .with_jitter(2.16)
            .with_seed(0x5EED_2027),
    ]
}

fn render() -> RgbImage {
    let mut mask = vec![false; W as usize * H as usize];
    for shape in scene() {
        handwrite_sim::core::shapes::rasterize_shape(
            &shape, &mut mask, W as usize, H as usize, 0, 0,
        );
    }
    let mut image = RgbImage::from_pixel(W, H, Rgb([255, 255, 255]));
    for (i, &on) in mask.iter().enumerate() {
        if on {
            let x = (i % W as usize) as u32;
            let y = (i / W as usize) as u32;
            image.put_pixel(x, y, Rgb([28, 28, 30]));
        }
    }
    image
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let label = std::env::args().nth(1).unwrap_or_else(|| "after".into());
    let out_dir = Path::new("tmp_shapes");
    std::fs::create_dir_all(out_dir)?;
    let image = render();
    let path = out_dir.join(format!("shapes_single_stroke_{label}.png"));
    image.save(&path)?;
    println!("{}（{}×{}）", path.display(), W, H);

    if label == "after" {
        let before_path = out_dir.join("shapes_single_stroke_before.png");
        if before_path.is_file() {
            let before = image::open(&before_path)?.to_rgb8();
            let gap = 24;
            let mut compare = RgbImage::from_pixel(W, H * 2 + gap, Rgb([238, 238, 238]));
            image::imageops::replace(&mut compare, &before, 0, 0);
            image::imageops::replace(&mut compare, &image, 0, (H + gap) as i64);
            let compare_path = out_dir.join("shapes_single_stroke_before_after.png");
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
