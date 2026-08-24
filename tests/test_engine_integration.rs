//! 引擎端到端集成测试：参数 → 渲染 → 导出全链路。

use std::fs;
use std::path::PathBuf;

use handwrite_sim::core::engine::{export, render_all_pages_preview, render_preview, Engine};
use handwrite_sim::core::engine::DefaultEngine;
use handwrite_sim::core::models::{Align, HandwritingParams, MiswriteMode, Paragraph};
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

#[test]
fn miswrite_preview_matches_export_with_same_seed() {
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

    let mut params = make_params(&font, &bg);
    params.text = "今天天气很好，我们去公园散步，看花看草，心情舒畅。".into();
    params.font_size = 36.0;
    params.line_spacing = 44.0;
    params.miswrite_rate = 0.3;
    params.miswrite_rewrite_mode = MiswriteMode::Above;

    // 同 seed：预览全部页 = 预览首页 = 导出逐像素一致
    let pages = render_all_pages_preview(&params, 42).unwrap();
    let preview = render_preview(&params, 42).unwrap();
    assert_eq!(pages[0].as_raw(), preview.as_raw(), "预览首帧应与 render_preview 一致");
    let out = dir.path().join("out");
    let files = export(&params, &out, 42).unwrap();
    assert_eq!(files.len(), pages.len());
    for (path, page) in files.iter().zip(pages.iter()) {
        let saved = image::open(path).unwrap().to_rgba8();
        assert_eq!(saved.as_raw(), page.as_raw(), "导出应与预览逐像素一致");
    }

    // 错字效果确实生效：墨迹多于关闭时
    let mut p0 = params.clone();
    p0.miswrite_rate = 0.0;
    let pages0 = render_all_pages_preview(&p0, 42).unwrap();
    let ink = |p: &RgbaImage| -> usize { p.pixels().filter(|px| is_ink(px)).count() };
    let sum: usize = pages.iter().map(ink).sum();
    let sum0: usize = pages0.iter().map(ink).sum();
    assert!(sum > sum0, "错字效果应增加墨迹：{sum} vs {sum0}");
    fs::remove_dir_all(dir.path()).ok();
}
/// 完整链路：PDF → pdfium 栅格化多页底图 → 第 2 页框选区域 → 渲染验证。
/// 对应用户实际工作流「导入文档底图，在第 2/3 页框选手写填写」。
/// 无 pdfium.dll 的环境优雅跳过。
#[test]
fn pdf_background_with_region_on_second_page() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // 1. 生成两页 A4 PDF（100 DPI 栅格化 ≈ 827×1169）
    let pdf_path = dir.path().join("exam.pdf");
    {
        use printpdf::PdfDocument;
        let mut doc = PdfDocument::new("integration-test");
        let empty_ops: Vec<printpdf::Op> = Vec::new();
        doc.with_pages(vec![
            printpdf::PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), empty_ops.clone()),
            printpdf::PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), empty_ops),
        ]);
        let mut warnings = Vec::new();
        let bytes = doc.save(&printpdf::PdfSaveOptions::default(), &mut warnings);
        fs::write(&pdf_path, bytes).unwrap();
    }

    // 2. 栅格化为逐页 PNG
    let pages_dir = dir.path().join("pages");
    let page_files = match handwrite_sim::core::doc_render::pdf_to_images(&pdf_path, &pages_dir, 100)
    {
        Ok(paths) => paths,
        Err(handwrite_sim::core::doc_render::DocRenderError::PdfiumUnavailable(_)) => {
            eprintln!("跳过：未找到 pdfium.dll");
            return;
        }
        Err(e) => panic!("PDF 栅格化失败：{e}"),
    };
    assert_eq!(page_files.len(), 2);
    let page_w = image::open(&page_files[0]).unwrap().width() as usize;
    let page_h = image::open(&page_files[0]).unwrap().height() as usize;

    // 3. 第 2 页中部框选一个区域，打印体零扰动便于断言
    let (bx, by, bw, bh) = (80usize, page_h / 2 - 60, page_w - 160, 120);
    let mut params = HandwritingParams {
        font_path: font.to_string_lossy().into_owned(),
        background_path: page_files[0].to_string_lossy().into_owned(),
        background_pages: page_files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        text: String::new(),
        regions: vec![handwrite_sim::core::models::TextRegion {
            x: bx as i32,
            y: by as i32,
            w: bw as i32,
            h: bh as i32,
            text: "第二页填写的区域文字".into(),
            printed: true,
            page: 2,
            ..Default::default()
        }],
        ..HandwritingParams::default()
    };
    params.font_size = 30.0;

    // 4. 渲染全部页：共 2 页；第 1 页区域处无墨迹，第 2 页有墨迹
    params.validate().unwrap();
    let pages = render_all_pages_preview(&params, 42).unwrap();
    assert_eq!(pages.len(), 2, "应输出文档底图全部两页");

    let ink_count = |p: &RgbaImage| -> usize {
        p.pixels()
            .filter(|px| {
                let [r, g, b, _] = px.0;
                (u16::from(r) + u16::from(g) + u16::from(b)) / 3 < 128
            })
            .count()
    };
    let inner = |p: &RgbaImage| -> usize {
        p.pixels()
            .enumerate()
            .filter(|(i, px)| {
                let x = i % p.width() as usize;
                let y = i / p.width() as usize;
                x >= bx && x < bx + bw && y >= by && y < by + bh
                    && {
                        let [r, g, b, _] = px.0;
                        (u16::from(r) + u16::from(g) + u16::from(b)) / 3 < 128
                    }
            })
            .count()
    };

    assert_eq!(inner(&pages[0]), 0, "第 1 页不应出现第 2 页的区域墨迹");
    assert!(inner(&pages[1]) > 0, "第 2 页区域内应有墨迹");
    assert!(
        ink_count(&pages[0]) == 0,
        "无主文字时第 1 页应为纯背景"
    );
}
