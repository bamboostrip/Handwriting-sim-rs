//! 引擎端到端集成测试：参数 → 渲染 → 导出全链路。

use std::path::PathBuf;

use handwrite_sim::core::engine::{export, render_preview, Engine};
use handwrite_sim::core::engine::DefaultEngine;
use handwrite_sim::core::models::{Align, HandwritingParams, Paragraph};
use image::{Rgb, RgbImage, Rgba, RgbaImage};

fn system_font() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"/System/Library/Fonts/PingFang.ttc",
        r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];
    CANDIDATES.iter().map(|p| PathBuf::from(p.trim())).find(|p| p.is_file())
}

fn make_params(font: &std::path::Path, bg: &std::path::Path) -> HandwritingParams {
    HandwritingParams {
        text: "端到端集成测试：手写模拟器渲染链路。".into(),
        font_path: font.to_string_lossy().into_owned(),
        background_path: bg.to_string_lossy().into_owned(),
        font_size: 30.0,
        line_spacing: 42.0,
        ..HandwritingParams::default()
    }
}

#[test]
fn full_pipeline_preview_and_export() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg.png");
    let mut img = RgbImage::new(500, 400);
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]);
    }
    img.save(&bg).unwrap();
    let params = make_params(&font, &bg);

    // 预览
    let preview = render_preview(&params, 7).expect("预览渲染失败");
    assert_eq!(preview.dimensions(), (500, 400));

    // 导出（同 seed），文件应存在且与预览首页一致
    let out = dir.path().join("out");
    let files = export(&params, &out, 7).expect("导出失败");
    assert!(!files.is_empty());
    assert!(files[0].is_file());
    let first = image::open(&files[0]).unwrap().to_rgba8();
    assert_eq!(first.as_raw(), preview.as_raw());
}

#[test]
fn long_text_yields_multiple_pages() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg.png");
    let mut img = RgbImage::new(300, 200); // 小画布加速翻页
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]);
    }
    img.save(&bg).unwrap();

    let mut params = make_params(&font, &bg);
    params.text = "多页测试文本。".repeat(60);
    let pages = DefaultEngine::new(1).render_pages(&params).expect("多页渲染失败");
    assert!(pages.len() >= 2, "长文本应产生多页，实际 {} 页", pages.len());
}

/// 是否为深色墨迹像素（前景）。
fn is_ink(px: &Rgba<u8>) -> bool {
    let [r, g, b, _] = px.0;
    r < 128 && g < 128 && b < 128
}

/// 页面中最上方墨迹行内最左墨迹像素的 x 坐标（即首行首字起始位置）。
fn first_line_ink_left_x(img: &RgbaImage) -> u32 {
    let (w, h) = img.dimensions();
    let top_y = (0..h).find(|&y| (0..w).any(|x| is_ink(img.get_pixel(x, y)))).expect("页面应含墨迹");
    (0..w).find(|&x| is_ink(img.get_pixel(x, top_y))).expect("首行应含墨迹")
}

#[test]
fn integration_paragraph_path_renders_and_exports() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg.png");
    let mut img = RgbImage::new(500, 400);
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]); // 白色背景，便于检出深色墨迹
    }
    img.save(&bg).unwrap();

    // 3 个段落：左对齐（首行缩进 50px）/ 居中 / 右对齐
    let params = HandwritingParams {
        font_path: font.to_string_lossy().into_owned(),
        background_path: bg.to_string_lossy().into_owned(),
        font_size: 30.0,
        line_spacing: 42.0,
        left_margin: 40.0,
        fill: [0, 0, 0],
        paragraphs: vec![
            Paragraph { text: "第一段：居左对齐，首行缩进。".into(), align: Align::Left, first_line_indent: 50.0 },
            Paragraph { text: "第二段：居中对齐。".into(), align: Align::Center, first_line_indent: 0.0 },
            Paragraph { text: "第三段：居右对齐。".into(), align: Align::Right, first_line_indent: 0.0 },
        ],
        ..HandwritingParams::default()
    };

    // 1) render_pages 产出至少 1 页且有深色前景
    let pages = DefaultEngine::new(7).render_pages(&params).expect("段落路径渲染失败");
    assert!(!pages.is_empty(), "段落路径应产出至少 1 页");
    assert!(
        pages[0].pixels().any(is_ink),
        "首页应有深色墨迹前景"
    );

    // 2) save_all 与 render_pages 同 seed 逐像素一致
    let out = dir.path().join("out");
    let files = DefaultEngine::new(7).save_all(&params, &out).expect("段落路径导出失败");
    assert_eq!(files.len(), pages.len(), "导出文件数与渲染页数一致");
    for (i, path) in files.iter().enumerate() {
        let saved = image::open(path).unwrap().to_rgba8();
        assert_eq!(saved.as_raw(), pages[i].as_raw(), "第 {i} 页导出与渲染应逐像素一致");
    }

    // 3) 首行缩进：首行墨迹最左 x 应 ≥ left_margin + indent（容差覆盖像素扰动）
    let first_x = first_line_ink_left_x(&pages[0]);
    let expected = params.left_margin + 50.0;
    assert!(
        first_x as f32 >= expected - 4.0,
        "首行墨迹最左 x={first_x} 应 ≥ 缩进起点 {expected}"
    );
}