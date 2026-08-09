//! 手写模拟器桌面入口。
//!
//! 阶段 3：GUI 功能对齐 Python 版（1:1）。
//! - 参数面板 ↔ `HandwritingParams` 双向绑定（Slint `<=>` 属性）
//! - 「生成预览」防抖 300ms 后渲染全部页并翻页显示（对齐 Python 版自动预览 + 多页导航）
//! - 预设下拉框（扫描 exe 旁 presets/ 目录）快捷切换 + 载入/保存
//! - 文字颜色 / 边距 / 扰动 σ / 边界提示（仅预览叠加）
//! - 字体/背景经原生文件对话框选择（rfd）；「导出图片」全分辨率批量导出

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use handwrite_sim::core::docx_io;
use handwrite_sim::core::engine::{export, export_pdf, overlay_bounds, render_all_pages_preview, EngineError};
use handwrite_sim::core::models::{parse_color, Align, HandwritingParams, MiswriteMode, Paragraph};
use handwrite_sim::core::presets;
use handwrite_sim::ui::{MainWindow, ParagraphItem};
use image::RgbaImage;
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel,
};

/// 预览防抖间隔（毫秒）。
const PREVIEW_DEBOUNCE_MS: u64 = 300;
/// 预览区底色循环（对齐 Python 版 `_PREVIEW_BG_COLORS`）。
const PREVIEW_BG_COLORS: [&str; 2] = ["#c8d0ca", "#565b56"];
/// 预设下拉框占位项。
const PRESET_PLACEHOLDER: &str = "— 选择预设 —";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;

    // ---- 状态 ----
    let timer = Rc::new(Timer::default());
    let seed_counter = Rc::new(RefCell::new(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64,
    ));
    // 最近一次载入预设的完整参数（含 slint 未绑定的 end_chars/start_chars 等），
    // 作为 collect_params 的基础，避免载入预设时这些字段被静默丢弃。
    let preset_params = Rc::new(RefCell::new(Option::<HandwritingParams>::None));
    // 预览全部页缓存 + 当前页索引（翻页用）
    let preview_pages = Rc::new(RefCell::new(Vec::<RgbaImage>::new()));
    let preview_index = Rc::new(RefCell::new(0usize));
    // 预览区底色循环索引
    let preview_bg_idx = Rc::new(RefCell::new(0usize));
    // 预设下拉：显示名模型 + 索引→路径映射（0 为占位符）
    let preset_model = Rc::new(VecModel::<SharedString>::default());
    let preset_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    ui.set_preset_list(ModelRc::from(preset_model.clone()));
    refresh_preset_combo(&preset_model, &preset_paths, &ui);

    // ---- 生成预览（防抖） ----
    {
        let weak = ui.as_weak();
        let timer = Rc::clone(&timer);
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        let pages = Rc::clone(&preview_pages);
        let index = Rc::clone(&preview_index);
        ui.on_regenerate(move || {
            let Some(ui) = weak.upgrade() else { return };
            let timer = Rc::clone(&timer);
            let seed = Rc::clone(&seed);
            let preset_params = Rc::clone(&preset_params);
            let pages = Rc::clone(&pages);
            let index = Rc::clone(&index);
            timer.start(TimerMode::SingleShot, Duration::from_millis(PREVIEW_DEBOUNCE_MS), move || {
                match render_and_show(&ui, &preset_params, &seed, &pages, &index) {
                    Ok(()) => {}
                    Err(e) => ui.set_status_text(SharedString::from(format!("渲染失败：{e}"))),
                }
            });
        });
    }

    // ---- 预览翻页 ----
    {
        let weak = ui.as_weak();
        let pages = Rc::clone(&preview_pages);
        let index = Rc::clone(&preview_index);
        ui.on_prev_page(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut idx = index.borrow_mut();
            if *idx > 0 {
                *idx -= 1;
            }
            drop(idx);
            show_page(&ui, &pages, &index);
        });
    }
    {
        let weak = ui.as_weak();
        let pages = Rc::clone(&preview_pages);
        let index = Rc::clone(&preview_index);
        ui.on_next_page(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut idx = index.borrow_mut();
            let total = pages.borrow().len();
            if total > 0 && *idx + 1 < total {
                *idx += 1;
            }
            drop(idx);
            show_page(&ui, &pages, &index);
        });
    }

    // ---- 预览底色切换 ----
    {
        let weak = ui.as_weak();
        let idx = Rc::clone(&preview_bg_idx);
        ui.on_toggle_preview_bg(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut i = idx.borrow_mut();
            *i = (*i + 1) % PREVIEW_BG_COLORS.len();
            ui.set_preview_bg(hex_color(PREVIEW_BG_COLORS[*i]));
        });
    }

    // ---- 选择字体 ----
    {
        let weak = ui.as_weak();
        ui.on_choose_font(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("字体文件", &["ttf", "ttc", "otf"])
                .pick_file()
            {
                ui.set_font_path_text(SharedString::from(path.to_string_lossy().into_owned()));
            }
        });
    }

    // ---- 选择背景 ----
    {
        let weak = ui.as_weak();
        ui.on_choose_background(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("图片", &["png", "jpg", "jpeg", "webp", "bmp"])
                .pick_file()
            {
                ui.set_background_path_text(SharedString::from(path.to_string_lossy().into_owned()));
            }
        });
    }

    // ---- 导出图片（始终全分辨率，预览降采样不影响导出） ----
    {
        let weak = ui.as_weak();
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        ui.on_export_files(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
            let params = match collect_params(&ui, &preset_params) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                    return;
                }
            };
            let seed_val = *seed.borrow();
            match export(&params, &dir, seed_val) {
                Ok(files) => {
                    let msg = format!("已导出 {} 个文件到 {}", files.len(), dir.display());
                    ui.set_status_text(SharedString::from(msg));
                }
                Err(e) => ui.set_status_text(SharedString::from(format!("导出失败：{e}"))),
            }
        });
    }

    // ---- 导出 PDF（位图层，300 DPI） ----
    {
        let weak = ui.as_weak();
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        ui.on_export_pdf(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name("handwrite.pdf")
                .save_file()
            else {
                return;
            };
            let params = match collect_params(&ui, &preset_params) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                    return;
                }
            };
            let seed_val = *seed.borrow();
            match export_pdf(&params, &path, seed_val) {
                Ok(()) => ui.set_status_text(SharedString::from(format!("PDF 已导出：{}", path.display()))),
                Err(e) => ui.set_status_text(SharedString::from(format!("导出 PDF 失败：{e}"))),
            }
        });
    }

    // ---- 段落模型（VecModel 驱动列表 UI） ----
    let paragraph_model = Rc::new(VecModel::<ParagraphItem>::default());
    ui.set_paragraphs(ModelRc::from(paragraph_model.clone()));

    {
        let model = Rc::clone(&paragraph_model);
        ui.on_add_paragraph(move || {
            model.push(ParagraphItem {
                text: SharedString::from(""),
                align_index: 0,
                indent: 0,
            });
        });
    }
    {
        let model = Rc::clone(&paragraph_model);
        ui.on_remove_paragraph(move |idx| {
            let idx = idx as usize;
            if idx < model.row_count() {
                model.remove(idx);
            }
        });
    }
    // 段落文本编辑写回（slint for + <=> 不支持写回模型项，故用单向绑定 + 回调）
    {
        let model = Rc::clone(&paragraph_model);
        ui.on_paragraph_edited(move |idx, text| {
            let idx = idx as usize;
            if idx < model.row_count() {
                let mut item = model.row_data(idx).unwrap_or_default();
                item.text = text;
                model.set_row_data(idx, item);
            }
        });
    }
    {
        let model = Rc::clone(&paragraph_model);
        ui.on_paragraph_align_changed(move |idx, align| {
            let idx = idx as usize;
            if idx < model.row_count() {
                let mut item = model.row_data(idx).unwrap_or_default();
                item.align_index = align;
                model.set_row_data(idx, item);
            }
        });
    }
    {
        let model = Rc::clone(&paragraph_model);
        ui.on_paragraph_indent_changed(move |idx, indent| {
            let idx = idx as usize;
            if idx < model.row_count() {
                let mut item = model.row_data(idx).unwrap_or_default();
                item.indent = indent;
                model.set_row_data(idx, item);
            }
        });
    }

    // 导入 docx：解析后整体替换段落列表
    {
        let model = Rc::clone(&paragraph_model);
        let weak = ui.as_weak();
        ui.on_import_docx(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Word 文档", &["docx"])
                .pick_file()
            {
                let font_size = ui.get_font_size() as f32;
                match docx_io::load_paragraphs(&path, font_size) {
                    Ok(paras) => {
                        let count = paras.len();
                        while model.row_count() > 0 {
                            model.remove(0);
                        }
                        for p in paras {
                            model.push(ParagraphItem {
                                text: SharedString::from(p.text),
                                align_index: match p.align {
                                    Align::Left => 0,
                                    Align::Center => 1,
                                    Align::Right => 2,
                                },
                                indent: p.first_line_indent.round() as i32,
                            });
                        }
                        ui.set_input_mode(1);
                        ui.set_status_text(SharedString::from(format!("已导入 {count} 个段落")));
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("导入 docx 失败：{e}"))),
                }
            }
        });
    }

    // ---- 预设下拉框选中 ----
    {
        let weak = ui.as_weak();
        let preset_params = Rc::clone(&preset_params);
        let paths = Rc::clone(&preset_paths);
        ui.on_preset_selected(move |index| {
            if index <= 0 {
                return; // 占位符
            }
            let Some(ui) = weak.upgrade() else { return };
            let idx = (index as usize).saturating_sub(1);
            let Some(path) = paths.borrow().get(idx).cloned() else { return };
            match presets::load(&path) {
                Ok(p) => {
                    apply_preset_to_ui(&ui, &preset_params, &p);
                    ui.set_status_text(SharedString::from(format!("已载入预设：{}", path.display())));
                }
                Err(e) => ui.set_status_text(SharedString::from(format!("载入失败：{e}"))),
            }
        });
    }

    // ---- 保存预设 ----
    {
        let weak = ui.as_weak();
        let preset_params = Rc::clone(&preset_params);
        let preset_model = Rc::clone(&preset_model);
        let preset_paths = Rc::clone(&preset_paths);
        ui.on_save_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            let default_dir = presets::assets_root().join("presets");
            let default_path = default_dir.join("preset.json");
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("预设", &["json"])
                .set_directory(default_dir.clone())
                .set_file_name("preset.json")
                .save_file()
            {
                let params = match collect_params(&ui, &preset_params) {
                    Ok(p) => p,
                    Err(e) => {
                        ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                        return;
                    }
                };
                match presets::save(&params, &path) {
                    Ok(()) => {
                        ui.set_status_text(SharedString::from(format!(
                            "预设已保存：{}",
                            path.display()
                        )));
                        // 保存到 presets/ 目录时刷新下拉框
                        if path.starts_with(default_path.parent().unwrap_or(&default_dir)) {
                            refresh_preset_combo(&preset_model, &preset_paths, &ui);
                        }
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("保存失败：{e}"))),
                }
            }
        });
    }

    // ---- 载入预设（文件对话框） ----
    {
        let weak = ui.as_weak();
        let preset_params = Rc::clone(&preset_params);
        ui.on_load_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("预设", &["json"])
                .pick_file()
            {
                match presets::load(&path) {
                    Ok(p) => {
                        apply_preset_to_ui(&ui, &preset_params, &p);
                        ui.set_status_text(SharedString::from("预设已载入（含边距/扰动参数）"));
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("载入失败：{e}"))),
                }
            }
        });
    }

    ui.run()?;
    Ok(())
}

/// 把 `#RRGGBB` 解析为 slint 颜色（解析失败回退默认底色）。
fn hex_color(hex: &str) -> slint::Color {
    let rgb = parse_color(hex).unwrap_or([200, 208, 202]);
    slint::Color::from_argb_u8(255, rgb[0], rgb[1], rgb[2])
}

/// 扫描 exe 旁 presets/ 目录，刷新预设下拉框（0 为占位符，其后为文件名）。
fn refresh_preset_combo(
    model: &Rc<VecModel<SharedString>>,
    paths: &RefCell<Vec<PathBuf>>,
    _ui: &MainWindow,
) {
    model.set_vec(vec![SharedString::from(PRESET_PLACEHOLDER)]);
    paths.borrow_mut().clear();
    let preset_dir = presets::assets_root().join("presets");
    if let Ok(rd) = std::fs::read_dir(&preset_dir) {
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|e| e == "json").unwrap_or(false))
            .collect();
        files.sort();
        for f in files {
            let stem = f
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            model.push(SharedString::from(stem));
            paths.borrow_mut().push(f);
        }
    }
}

/// 把预设参数回填 UI 控件（含新增的颜色/边距/σ 控件），并写入 preset_params 缓存。
fn apply_preset_to_ui(
    ui: &MainWindow,
    preset_params: &RefCell<Option<HandwritingParams>>,
    p: &HandwritingParams,
) {
    *preset_params.borrow_mut() = Some(p.clone());
    ui.set_font_path_text(SharedString::from(p.font_path.clone()));
    ui.set_background_path_text(SharedString::from(p.background_path.clone()));
    ui.set_font_size(p.font_size as i32);
    ui.set_line_spacing(p.line_spacing as i32);
    ui.set_word_spacing(p.word_spacing as i32);
    ui.set_word_spacing_sigma(p.word_spacing_sigma as i32);
    ui.set_line_spacing_sigma(p.line_spacing_sigma as i32);
    ui.set_font_size_sigma(p.font_size_sigma as i32);
    ui.set_perturb_x(p.perturb_x_sigma as i32);
    ui.set_perturb_y(p.perturb_y_sigma as i32);
    ui.set_perturb_theta(p.perturb_theta_sigma);
    ui.set_miswrite_rate(p.miswrite_rate * 100.0);
    ui.set_miswrite_mode_index(match p.miswrite_rewrite_mode {
        MiswriteMode::Above => 0,
        MiswriteMode::Rewrite => 1,
    });
    ui.set_font_color(SharedString::from(format!(
        "#{:02x}{:02x}{:02x}",
        p.fill[0], p.fill[1], p.fill[2]
    )));
    ui.set_margin_top(p.top_margin as i32);
    ui.set_margin_bottom(p.bottom_margin as i32);
    ui.set_margin_left(p.left_margin as i32);
    ui.set_margin_right(p.right_margin as i32);
}

/// 收集 UI 参数为 `HandwritingParams` 并校验。
/// 以最近载入预设为基础（`preset_params`），再用 UI 控件值覆盖对应字段，
/// 从而保留预设中 slint 未绑定的 end_chars/start_chars 等参数。
fn collect_params(
    ui: &MainWindow,
    preset_params: &RefCell<Option<HandwritingParams>>,
) -> Result<HandwritingParams, EngineError> {
    let mut params = preset_params.borrow().clone().unwrap_or_default();
    if ui.get_input_mode() == 1 {
        let model = ui.get_paragraphs();
        let mut paras = Vec::new();
        for i in 0..model.row_count() {
            let item = model.row_data(i).unwrap();
            let text = item.text.to_string();
            if text.trim().is_empty() {
                continue;
            }
            paras.push(Paragraph {
                text,
                align: match item.align_index {
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Left,
                },
                first_line_indent: item.indent as f32,
            });
        }
        params.paragraphs = paras;
    } else {
        params.text = ui.get_input_text().as_str().trim().to_string();
    }
    params.font_path = ui.get_font_path_text().as_str().trim().to_string();
    params.background_path = ui.get_background_path_text().as_str().trim().to_string();
    // 排版参数（SpinBox.value 为 int，转 f32 以支持预览降采样等浮点语义）
    params.font_size = ui.get_font_size() as f32;
    params.line_spacing = ui.get_line_spacing() as f32;
    params.word_spacing = ui.get_word_spacing() as f32;
    params.word_spacing_sigma = ui.get_word_spacing_sigma() as f32;
    params.line_spacing_sigma = ui.get_line_spacing_sigma() as f32;
    params.font_size_sigma = ui.get_font_size_sigma() as f32;
    params.perturb_x_sigma = ui.get_perturb_x() as f32;
    params.perturb_y_sigma = ui.get_perturb_y() as f32;
    params.perturb_theta_sigma = ui.get_perturb_theta();
    // 边距
    params.top_margin = ui.get_margin_top() as f32;
    params.bottom_margin = ui.get_margin_bottom() as f32;
    params.left_margin = ui.get_margin_left() as f32;
    params.right_margin = ui.get_margin_right() as f32;
    // 写错字模拟
    params.miswrite_rate = ui.get_miswrite_rate() / 100.0;
    params.miswrite_rewrite_mode = match ui.get_miswrite_mode_index() {
        1 => MiswriteMode::Rewrite,
        _ => MiswriteMode::Above,
    };
    // 文字颜色
    params.fill = parse_color(ui.get_font_color().as_str()).map_err(EngineError::Params)?;
    params.validate().map_err(EngineError::Params)?;
    Ok(params)
}

/// 渲染全部页（预览降采样路径）并显示，seed 递增保证每次刷新笔画变化。
fn render_and_show(
    ui: &MainWindow,
    preset_params: &RefCell<Option<HandwritingParams>>,
    seed_counter: &RefCell<u64>,
    preview_pages: &RefCell<Vec<RgbaImage>>,
    preview_index: &RefCell<usize>,
) -> Result<(), EngineError> {
    let params = collect_params(ui, preset_params)?;
    let seed = {
        let mut s = seed_counter.borrow_mut();
        *s += 1;
        *s
    };
    let mut pages = render_all_pages_preview(&params, seed)?;
    // 边界提示（仅预览）：非渲染区半透明着色 + 边距框线
    if ui.get_bounds_visible() {
        let color = parse_color(ui.get_bounds_color().as_str()).unwrap_or([76, 166, 166]);
        for page in pages.iter_mut() {
            *page = overlay_bounds(page, &params, color);
        }
    }
    *preview_pages.borrow_mut() = pages;
    *preview_index.borrow_mut() = 0;
    show_page(ui, preview_pages, preview_index);
    let total = preview_pages.borrow().len();
    ui.set_status_text(SharedString::from(format!("预览完成（seed={seed}），共 {total} 页")));
    Ok(())
}

/// 把当前索引页显示到预览区并更新页码。
fn show_page(
    ui: &MainWindow,
    preview_pages: &RefCell<Vec<RgbaImage>>,
    preview_index: &RefCell<usize>,
) {
    let pages = preview_pages.borrow();
    let total = pages.len();
    if total == 0 {
        ui.set_page_text(SharedString::from("第 1 / 1 页"));
        return;
    }
    let i = (*preview_index.borrow()).min(total - 1);
    let img = &pages[i];
    let (width, height) = img.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(img.as_raw(), width, height);
    ui.set_preview_image(Image::from_rgba8(buffer));
    ui.set_page_text(SharedString::from(format!("第 {} / {total} 页", i + 1)));
}