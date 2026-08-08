//! 引擎端到端集成测试：参数 → 渲染 → 导出全链路。

use std::path::PathBuf;

use handwrite_sim::core::engine::{export, render_preview, Engine};
use handwrite_sim::core::engine::DefaultEngine;
use handwrite_sim::core::models::HandwritingParams;
use image::{Rgb, RgbImage};

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