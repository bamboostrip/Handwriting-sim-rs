//! issue #6 图形标注 MVP 的引擎集成测试。
//!
//! 覆盖冻结契约（`docs/research/issue6-shape-mvp-contract.md`）的验收点：
//! 1. 旧 JSON（无 `shapes` 字段）兼容且渲染正常；
//! 2. 参数校验（page / thickness / 非有限坐标）；
//! 3. 图形只出现在指定页；
//! 4. 只有图形、没有文字的页：页数正确、不报 `NoText`、不需要字体；
//! 5. `fill: None` 跟随主墨色、`fill: Some(..)` 覆盖；
//! 6. 大背景（> `PREVIEW_MAX_WIDTH`）预览降采样后图形位置按比例一致；
//! 7. 同 seed 两次渲染逐像素一致（图形层不消耗扰动随机源）。

use std::path::{Path, PathBuf};

use handwrite_sim::core::engine::{
    render_all_pages_preview, render_preview, DefaultEngine, Engine,
};
use handwrite_sim::core::models::{HandwritingParams, ParamsError};
use handwrite_sim::core::shapes::{ShapeKind, ShapeSpec};
use image::{Rgb, RgbImage, RgbaImage};

/// 系统 CJK 字体（找不到时测试自行跳过，与 engine.rs 现有测试一致）。
fn system_font() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\simkai.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"/System/Library/Fonts/PingFang.ttc",
        r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];
    CANDIDATES
        .iter()
        .map(|p| PathBuf::from(p.trim()))
        .find(|p| p.is_file())
}

/// 在临时目录写一张白底 PNG 背景。
fn white_bg(dir: &Path, w: u32, h: u32) -> PathBuf {
    let path = dir.join(format!("bg_{w}x{h}.png"));
    let mut img = RgbImage::new(w, h);
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]);
    }
    img.save(&path).unwrap();
    path
}

/// 构造一个图形；默认第 1 页、2.4px、0 抖动（便于精确断言位置）。
fn shape(kind: ShapeKind, page: i32, x: f32, y: f32, w: f32, h: f32) -> ShapeSpec {
    let mut spec = ShapeSpec::new(kind, x, y, w, h)
        .with_thickness(2.4)
        .with_jitter(0.0)
        .with_seed(7);
    spec.page = page;
    spec
}

/// 非背景（墨迹）像素：白底上任意明显偏离白色的像素。
fn ink_px(px: [u8; 4]) -> bool {
    (u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3 < 240
}

/// 统计矩形区域内墨迹像素数。
fn region_ink(img: &RgbaImage, x: usize, y: usize, w: usize, h: usize) -> usize {
    let (iw, ih) = (img.width() as usize, img.height() as usize);
    let mut count = 0;
    for yy in y..(y + h).min(ih) {
        for xx in x..(x + w).min(iw) {
            if ink_px(img.get_pixel(xx as u32, yy as u32).0) {
                count += 1;
            }
        }
    }
    count
}

/// 统计矩形区域内与指定 RGB 完全一致的像素数（图形合成走 `draw_printed_mask`，颜色是精确值）。
fn region_color(img: &RgbaImage, x: usize, y: usize, w: usize, h: usize, color: [u8; 3]) -> usize {
    let (iw, ih) = (img.width() as usize, img.height() as usize);
    let want = [color[0], color[1], color[2], 255];
    let mut count = 0;
    for yy in y..(y + h).min(ih) {
        for xx in x..(x + w).min(iw) {
            if img.get_pixel(xx as u32, yy as u32).0 == want {
                count += 1;
            }
        }
    }
    count
}

/// 全部墨迹像素的灰度质心。
fn ink_centroid(img: &RgbaImage) -> (f64, f64) {
    let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0usize);
    for (i, px) in img.pixels().enumerate() {
        if ink_px(px.0) {
            sx += (i % img.width() as usize) as f64;
            sy += (i / img.width() as usize) as f64;
            n += 1;
        }
    }
    assert!(n > 0, "预览中应有图形墨迹");
    (sx / n as f64, sy / n as f64)
}

/// 1. 旧 JSON（无 `shapes` 字段）反序列化 → shapes 为空，渲染正常。
#[test]
fn legacy_json_without_shapes_still_renders() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let bg = white_bg(dir.path(), 400, 300);
    let params = HandwritingParams {
        text: "旧参数兼容测试".into(),
        font_path: font.to_string_lossy().into_owned(),
        background_path: bg.to_string_lossy().into_owned(),
        font_size: 30.0,
        line_spacing: 40.0,
        ..HandwritingParams::default()
    };

    // 模拟旧版前端/旧工程 JSON：序列化后删掉 shapes 字段
    let mut json = serde_json::to_value(&params).unwrap();
    assert!(
        json.as_object_mut().unwrap().remove("shapes").is_some(),
        "新序列化结果应包含 shapes 字段"
    );
    let legacy: HandwritingParams = serde_json::from_value(json).unwrap();
    assert!(legacy.shapes.is_empty(), "缺省 shapes 应为空数组");

    let pages = DefaultEngine::new(42).render_pages(&legacy).unwrap();
    assert!(!pages.is_empty());
    assert!(
        pages[0].pixels().any(|p| ink_px(p.0)),
        "旧 JSON 参数应正常渲染出墨迹"
    );
}

/// 2. 校验：page=0 / thickness=-1 / NaN、inf 坐标 → 对应错误变体。
#[test]
fn validate_rejects_bad_shape_params() {
    let dir = tempfile::tempdir().unwrap();
    let bg = white_bg(dir.path(), 100, 100);
    let base = HandwritingParams {
        background_path: bg.to_string_lossy().into_owned(),
        ..HandwritingParams::default()
    };

    // page < 1
    let mut p = base.clone();
    p.shapes = vec![shape(ShapeKind::Rect, 0, 10.0, 10.0, 50.0, 50.0)];
    assert!(
        matches!(p.validate_with(false), Err(ParamsError::ShapePage { index: 1 })),
        "page=0 应报 ShapePage"
    );

    // 负数线宽
    let mut p = base.clone();
    p.shapes = vec![shape(ShapeKind::Rect, 1, 10.0, 10.0, 50.0, 50.0).with_thickness(-1.0)];
    assert!(
        matches!(p.validate_with(false), Err(ParamsError::ShapeThickness { index: 1 })),
        "thickness=-1 应报 ShapeThickness"
    );

    // NaN 坐标
    let mut p = base.clone();
    let mut spec = shape(ShapeKind::Rect, 1, 10.0, 10.0, 50.0, 50.0);
    spec.x = f32::NAN;
    p.shapes = vec![spec];
    assert!(
        matches!(p.validate_with(false), Err(ParamsError::ShapeGeometry { index: 1 })),
        "NaN 坐标应报 ShapeGeometry"
    );

    // inf 尺寸 / 抖动
    let mut p = base.clone();
    let mut spec = shape(ShapeKind::Rect, 1, 10.0, 10.0, f32::INFINITY, 50.0);
    spec.jitter = f32::INFINITY;
    p.shapes = vec![spec];
    assert!(
        matches!(p.validate_with(false), Err(ParamsError::ShapeGeometry { index: 1 })),
        "inf 尺寸/抖动应报 ShapeGeometry"
    );

    // 合法图形（负宽高、线宽 0 均允许）应通过
    let mut p = base;
    p.shapes = vec![
        shape(ShapeKind::Rect, 2, 50.0, 50.0, -30.0, -20.0),
        shape(ShapeKind::Line, 1, 0.0, 0.0, 100.0, 0.0).with_thickness(0.0),
    ];
    assert!(
        p.validate_with(false).is_ok(),
        "合法图形应通过校验：{:?}",
        p.validate_with(false)
    );
}

/// 3. 引擎：3 个图形分别在第 1/2/3 页 → 输出 3 页，每页只在对应页出现墨迹。
#[test]
fn shapes_render_on_their_requested_pages_only() {
    let dir = tempfile::tempdir().unwrap();
    let bg = white_bg(dir.path(), 400, 300);
    // 三个互不重叠的包围盒，每页一个
    let boxes = [(20usize, 20usize, 80usize, 60usize), (160, 110, 90, 70), (290, 200, 80, 60)];
    let params = HandwritingParams {
        background_path: bg.to_string_lossy().into_owned(),
        text: String::new(),
        font_path: String::new(),
        shapes: vec![
            shape(ShapeKind::Rect, 1, 20.0, 20.0, 80.0, 60.0),
            shape(ShapeKind::Ellipse, 2, 160.0, 110.0, 90.0, 70.0),
            shape(ShapeKind::Rect, 3, 290.0, 200.0, 80.0, 60.0),
        ],
        ..HandwritingParams::default()
    };

    let pages = DefaultEngine::new(11).render_pages(&params).unwrap();
    assert_eq!(pages.len(), 3, "页数应覆盖图形所在的最大页码");
    for (page_index, page) in pages.iter().enumerate() {
        assert_eq!(page.dimensions(), (400, 300));
        let mut page_ink = 0;
        for (box_index, &(bx, by, bw, bh)) in boxes.iter().enumerate() {
            let ink = region_ink(page, bx, by, bw, bh);
            page_ink += ink;
            if page_index == box_index {
                assert!(ink > 20, "第 {} 页应在自己的包围盒内有墨迹，实际 {ink}", page_index + 1);
            } else {
                assert_eq!(ink, 0, "第 {} 页不应出现第 {} 个图形", page_index + 1, box_index + 1);
            }
        }
        assert!(page_ink > 0, "第 {} 页应有墨迹", page_index + 1);
    }
}

/// 4. 只有图形没有文字的页：页数正确、不报 NoText、不需要字体。
#[test]
fn shapes_only_pages_need_no_text_and_no_font() {
    let dir = tempfile::tempdir().unwrap();
    let bg = white_bg(dir.path(), 400, 300);
    let params = HandwritingParams {
        background_path: bg.to_string_lossy().into_owned(),
        // 无正文、无框选区域；font_path 故意留空：图形不依赖字体
        text: String::new(),
        font_path: String::new(),
        shapes: vec![
            shape(ShapeKind::Rect, 1, 30.0, 30.0, 100.0, 60.0),
            shape(ShapeKind::Rect, 3, 240.0, 180.0, 100.0, 60.0),
        ],
        ..HandwritingParams::default()
    };

    // 预览/导出实际走 validate_with(false)：不能报 NoText / NoFont
    assert!(
        params.validate_with(false).is_ok(),
        "纯图形参数不应要求文字/字体：{:?}",
        params.validate_with(false)
    );

    let pages = render_all_pages_preview(&params, 5).unwrap();
    assert_eq!(pages.len(), 3, "第 3 页只有图形也必须输出");
    assert!(region_ink(&pages[0], 30, 30, 100, 60) > 0, "第 1 页图形应渲染");
    assert_eq!(region_ink(&pages[1], 0, 0, 400, 300), 0, "第 2 页应为空白背景");
    assert!(region_ink(&pages[2], 240, 180, 100, 60) > 0, "第 3 页图形应渲染");

    // render_preview 同样不应因为「没有文字」而报错
    let first = render_preview(&params, 5).unwrap();
    assert_eq!(first.dimensions(), (400, 300));
}

/// 5. `fill: None` 跟随主墨色；`fill: Some(red)` 覆盖。
#[test]
fn shape_fill_follows_main_ink_or_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let bg = white_bg(dir.path(), 400, 300);
    let main_ink = [10u8, 40, 200];
    let red = [210u8, 40, 40];
    let params = HandwritingParams {
        background_path: bg.to_string_lossy().into_owned(),
        text: String::new(),
        font_path: String::new(),
        fill: main_ink,
        shapes: vec![
            // 左侧：跟随主墨色
            shape(ShapeKind::Rect, 1, 30.0, 30.0, 100.0, 60.0),
            // 右侧：显式红色覆盖
            shape(ShapeKind::Rect, 1, 220.0, 30.0, 100.0, 60.0).with_fill(red),
        ],
        ..HandwritingParams::default()
    };

    let page = render_preview(&params, 9).unwrap();

    // 精确颜色断言：合成层是 bool 掩码直写，颜色不做混合
    assert!(
        region_color(&page, 30, 30, 100, 60, main_ink) > 20,
        "跟随主墨色的图形应使用 params.fill"
    );
    assert_eq!(
        region_color(&page, 220, 30, 100, 60, main_ink),
        0,
        "覆盖颜色的图形不应再出现主墨色"
    );
    assert!(
        region_color(&page, 220, 30, 100, 60, red) > 20,
        "fill=Some(red) 应覆盖主墨色"
    );
    assert_eq!(
        region_color(&page, 30, 30, 100, 60, red),
        0,
        "左侧图形不应被右侧颜色污染"
    );

    // 采样一个已知边界像素：上边线中点（y=30）必须是主墨色
    let sampled = page.get_pixel(80, 30).0;
    assert_eq!(
        [sampled[0], sampled[1], sampled[2]],
        main_ink,
        "矩形上边线中点应为主墨色，实际 {sampled:?}"
    );
}

/// 6. 大背景（宽 8192 > 阈值 4096）预览降采样：图形重心 ≈ 源坐标 × 0.5。
#[test]
fn huge_background_preview_downscales_shape_position() {
    let dir = tempfile::tempdir().unwrap();
    let src_w = 8192u32;
    let src_h = 1000u32;
    let bg = white_bg(dir.path(), src_w, src_h);
    // 源坐标（背景原始像素）
    let (sx, sy, sw, sh) = (4000.0f32, 400.0f32, 400.0f32, 200.0f32);

    let params = HandwritingParams {
        background_path: bg.to_string_lossy().into_owned(),
        text: String::new(),
        font_path: String::new(),
        shapes: vec![shape(ShapeKind::Rect, 1, sx, sy, sw, sh).with_thickness(4.0)],
        ..HandwritingParams::default()
    };

    let preview = render_preview(&params, 3).unwrap();
    // 8192 → 4096（scale=0.5），高度 1000 → 500
    assert_eq!(preview.dimensions(), (4096, 500), "大背景预览应降采样到 4096 宽");

    let scale = 4096.0f64 / src_w as f64;
    let expect = (
        ((sx + sw / 2.0) as f64) * scale,
        ((sy + sh / 2.0) as f64) * scale,
    );
    let (cx, cy) = ink_centroid(&preview);
    // 容差按预览中的线宽（4px × 0.5 = 2px）给，留足像素取整余量
    let tolerance = 6.0;
    assert!(
        (cx - expect.0).abs() <= tolerance,
        "预览墨迹重心 x={cx:.1} 应 ≈ {:.1}",
        expect.0
    );
    assert!(
        (cy - expect.1).abs() <= tolerance,
        "预览墨迹重心 y={cy:.1} 应 ≈ {:.1}",
        expect.1
    );
}

/// 7. 同 seed 两次渲染逐像素一致（防 rng 串联 / 分组顺序漂移）。
#[test]
fn same_seed_shapes_render_pixel_identical() {
    let dir = tempfile::tempdir().unwrap();
    let bg = white_bg(dir.path(), 400, 300);
    let font = system_font();
    let mut params = HandwritingParams {
        background_path: bg.to_string_lossy().into_owned(),
        fill: [30, 30, 30],
        font_size: 30.0,
        line_spacing: 40.0,
        ..HandwritingParams::default()
    };
    if let Some(font) = &font {
        params.font_path = font.to_string_lossy().into_owned();
        params.text = "同 seed 两次渲染应逐像素一致，图形层不消耗扰动随机源。".into();
    }
    // 两种颜色部分重叠：颜色分组若按 HashMap 迭代顺序绘制，
    // 重叠区颜色会随实例漂移，同 seed 两次渲染即可暴露。
    params.shapes = vec![
        shape(ShapeKind::Rect, 1, 40.0, 40.0, 300.0, 180.0)
            .with_fill([20, 90, 200])
            .with_jitter(1.8)
            .with_seed(101),
        shape(ShapeKind::Ellipse, 1, 120.0, 90.0, 240.0, 160.0)
            .with_fill([210, 50, 40])
            .with_jitter(1.8)
            .with_seed(202),
        shape(ShapeKind::Arrow, 1, 0.0, 0.0, 0.0, 0.0)
            .with_points(vec![[60.0, 270.0], [340.0, 270.0]])
            .with_fill([20, 90, 200])
            .with_jitter(1.2)
            .with_seed(303),
    ];

    let a = DefaultEngine::new(77).render_pages(&params).unwrap();
    let b = DefaultEngine::new(77).render_pages(&params).unwrap();
    assert_eq!(a.len(), b.len());
    for (i, (pa, pb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(
            pa.as_raw(),
            pb.as_raw(),
            "第 {} 页同 seed 两次渲染应逐像素一致",
            i + 1
        );
    }
    assert!(a[0].pixels().any(|p| ink_px(p.0)), "应存在图形墨迹");
    assert_eq!(
        a[0].dimensions(),
        (400, 300),
        "普通背景（≤4096）不应降采样"
    );
}
