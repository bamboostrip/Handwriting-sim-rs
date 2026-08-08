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

use handwrite_sim::core::engine::{export, render_preview, EngineError};
use handwrite_sim::core::models::HandwritingParams;
use handwrite_sim::ui::MainWindow;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode};

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

    ui.run()?;
    Ok(())
}

/// 收集 UI 参数为 `HandwritingParams` 并校验。
fn collect_params(ui: &MainWindow) -> Result<HandwritingParams, EngineError> {
    let mut params = HandwritingParams::default();
    params.text = ui.get_input_text().as_str().trim().to_string();
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