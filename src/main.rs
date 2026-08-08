//! 手写模拟器桌面入口。
//!
//! 阶段 2：GUI 与引擎接通。
//! - 参数面板 ↔ `HandwritingParams` 双向绑定（Slint `<=>` 属性）
//! - 「生成预览」防抖 300ms 后渲染并显示（对齐 Python 版自动预览体验）
//! - 字体/背景经原生文件对话框选择（rfd）
//! - 「导出图片」选择目录后批量导出 PNG

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use handwrite_sim::core::docx_io;
use handwrite_sim::core::engine::{export, render_preview, EngineError};
use handwrite_sim::core::models::{Align, HandwritingParams, Paragraph};
use handwrite_sim::core::presets;
use handwrite_sim::ui::{MainWindow, ParagraphItem};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel,
};

/// 预览防抖间隔（毫秒）。
const PREVIEW_DEBOUNCE_MS: u64 = 300;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;

    // 状态：防抖定时器 + seed 计数器（每次预览递增，保证笔画刷新）
    let timer = Rc::new(Timer::default());
    let seed_counter = Rc::new(RefCell::new(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64,
    ));

    // ---- 生成预览（防抖） ----
    {
        let weak = ui.as_weak();
        let timer = Rc::clone(&timer);
        let seed = Rc::clone(&seed_counter);
        ui.on_regenerate(move || {
            let Some(ui) = weak.upgrade() else { return };
            let timer = Rc::clone(&timer);
            let seed = Rc::clone(&seed);
            timer.start(TimerMode::SingleShot, Duration::from_millis(PREVIEW_DEBOUNCE_MS), move || {
                match render_and_show(&ui, &seed) {
                    Ok(()) => {}
                    Err(e) => ui.set_status_text(SharedString::from(format!("渲染失败：{e}"))),
                }
            });
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

    // ---- 导出图片 ----
    {
        let weak = ui.as_weak();
        let seed = Rc::clone(&seed_counter);
        ui.on_export_files(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
            let params = match collect_params(&ui) {
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

    // ---- 段落模型（VecModel 驱动列表 UI） ----
    let paragraph_model = Rc::new(VecModel::<ParagraphItem>::default());
    ui.set_paragraphs(ModelRc::from(paragraph_model.clone()));

    // 添加段落
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

    // 删除段落
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

    // 段落对齐切换写回
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

    // 段落缩进修改写回
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
                        // 清空后填充
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
                        ui.set_status_text(SharedString::from(format!(
                            "已导入 {count} 个段落"
                        )));
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("导入 docx 失败：{e}"))),
                }
            }
        });
    }

    // ---- 保存预设 ----
    {
        let weak = ui.as_weak();
        ui.on_save_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("预设", &["json"])
                .set_file_name("preset.json")
                .save_file()
            {
                let params = match collect_params(&ui) {
                    Ok(p) => p,
                    Err(e) => {
                        ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                        return;
                    }
                };
                match presets::save(&params, &path) {
                    Ok(()) => ui.set_status_text(SharedString::from(format!(
                        "预设已保存：{}",
                        path.display()
                    ))),
                    Err(e) => ui.set_status_text(SharedString::from(format!("保存失败：{e}"))),
                }
            }
        });
    }

    // ---- 载入预设 ----
    {
        let weak = ui.as_weak();
        ui.on_load_preset(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("预设", &["json"])
                .pick_file()
            {
                match presets::load(&path) {
                    Ok(p) => {
                        ui.set_font_path_text(SharedString::from(p.font_path));
                        ui.set_background_path_text(SharedString::from(p.background_path));
                        ui.set_font_size(p.font_size as i32);
                        ui.set_line_spacing(p.line_spacing as i32);
                        ui.set_word_spacing(p.word_spacing as i32);
                        ui.set_perturb_x(p.perturb_x_sigma as i32);
                        ui.set_perturb_y(p.perturb_y_sigma as i32);
                        ui.set_perturb_theta(p.perturb_theta_sigma);
                        ui.set_status_text(SharedString::from("预设已载入"));
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("载入失败：{e}"))),
                }
            }
        });
    }

    ui.run()?;
    Ok(())
}

/// 收集 UI 参数为 `HandwritingParams` 并校验。
fn collect_params(ui: &MainWindow) -> Result<HandwritingParams, EngineError> {
    let mut params = HandwritingParams::default();
    if ui.get_input_mode() == 1 {
        // 段落模式：从模型收集段落
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
    // SpinBox.value 为 int，转 f32 以支持预览降采样等浮点语义
    params.font_size = ui.get_font_size() as f32;
    params.line_spacing = ui.get_line_spacing() as f32;
    params.word_spacing = ui.get_word_spacing() as f32;
    params.perturb_x_sigma = ui.get_perturb_x() as f32;
    params.perturb_y_sigma = ui.get_perturb_y() as f32;
    params.perturb_theta_sigma = ui.get_perturb_theta();
    params.validate().map_err(EngineError::Params)?;
    Ok(params)
}

/// 渲染预览并显示到 UI，seed 递增保证每次刷新笔画变化。
fn render_and_show(ui: &MainWindow, seed_counter: &RefCell<u64>) -> Result<(), EngineError> {
    let params = collect_params(ui)?;
    let seed = {
        let mut s = seed_counter.borrow_mut();
        *s += 1;
        *s
    };
    let image = render_preview(&params, seed)?;
    let (width, height) = image.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&image.into_raw(), width, height);
    ui.set_preview_image(Image::from_rgba8(buffer));
    ui.set_status_text(SharedString::from(format!(
        "预览完成（seed={seed}） {width}×{height}"
    )));
    Ok(())
}