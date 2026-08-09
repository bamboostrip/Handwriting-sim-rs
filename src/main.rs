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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use handwrite_sim::core::docx_io;
use handwrite_sim::core::engine::{export, export_pdf, overlay_bounds, render_all_pages_preview, EngineError};
use handwrite_sim::core::models::{parse_color, Align, HandwritingParams, MiswriteMode, Paragraph};
use handwrite_sim::core::presets;
use handwrite_sim::ui::MainWindow;
use image::RgbaImage;
use slint::{
    ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode,
    VecModel,
};

/// 预览防抖间隔（毫秒）。
const PREVIEW_DEBOUNCE_MS: u64 = 300;
/// 预览区底色循环（对齐 Python 版 `_PREVIEW_BG_COLORS`）。
const PREVIEW_BG_COLORS: [&str; 2] = ["#c8d0ca", "#565b56"];
/// 预设下拉框占位项。
const PRESET_PLACEHOLDER: &str = "— 选择预设 —";

/// GUI 调试日志：debug 构建默认输出（便于定位卡死/慢渲染环节），
/// release 构建需 `HANDWRITE_DEBUG=1` 才输出。
macro_rules! gui_dbg {
    ($($arg:tt)*) => {{
        if cfg!(debug_assertions) || std::env::var_os("HANDWRITE_DEBUG").is_some() {
            eprintln!("[GUI] {}", format_args!($($arg)*));
        }
    }};
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    gui_dbg!("GUI 启动完成（MainWindow 构建 + 预设扫描完毕），等待操作");

    // ---- 状态 ----
    let timer = Rc::new(Timer::default());
    let seed_counter = Rc::new(RefCell::new(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64,
    ));
    // 最近一次载入预设的完整参数（含 slint 未绑定的 end_chars/start_chars 等），
    // 作为 collect_params 的基础，避免载入预设时这些字段被静默丢弃。
    let preset_params = Rc::new(RefCell::new(Option::<HandwritingParams>::None));
    // 预览全部页缓存 + 当前页索引（翻页用）。
    // 用 Arc<Mutex> 而非 Rc<RefCell>：渲染在后台线程完成，结果经
    // `upgrade_in_event_loop`（要求 Send 闭包）切回 UI 线程后写入；
    // 所有访问都在 UI 线程，Mutex 无竞争。
    let preview_pages = Arc::new(Mutex::new(Vec::<RgbaImage>::new()));
    let preview_index = Arc::new(Mutex::new(0usize));
    // 渲染代次：后台渲染完成时只有最新一代的结果被应用到 UI（丢弃过期结果）
    let render_gen = Arc::new(AtomicU64::new(0));
    // 预览区底色循环索引
    let preview_bg_idx = Rc::new(RefCell::new(0usize));
    // 段落格式数组：与输入文本框按 \n 分割的段落一一对应，(对齐, 首行缩进px)。
    // 文本编辑时按段数重排（新段继承上一段格式），导出时据此生成 Paragraph 列表。
    let para_formats = Rc::new(RefCell::new(Vec::<(u8, i32)>::new()));
    // 预设下拉：显示名模型 + 索引→路径映射（0 为占位符）
    let preset_model = Rc::new(VecModel::<SharedString>::default());
    let preset_paths = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    ui.set_preset_list(ModelRc::from(preset_model.clone()));
    refresh_preset_combo(&preset_model, &preset_paths, &ui);

    // ---- 生成预览（防抖；渲染在后台线程，UI 不阻塞） ----
    {
        let weak = ui.as_weak();
        let timer = Rc::clone(&timer);
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        let para_formats = Rc::clone(&para_formats);
        let pages = Arc::clone(&preview_pages);
        let index = Arc::clone(&preview_index);
        let render_gen = Arc::clone(&render_gen);
        ui.on_regenerate(move || {
            let Some(ui) = weak.upgrade() else { return };
            gui_dbg!("「预览」按钮触发（300ms 防抖后开始渲染）");
            let timer = Rc::clone(&timer);
            let seed = Rc::clone(&seed);
            let preset_params = Rc::clone(&preset_params);
            let pages = Arc::clone(&pages);
            let index = Arc::clone(&index);
            let render_gen = Arc::clone(&render_gen);
            let para_formats_timer = Rc::clone(&para_formats);
            timer.start(TimerMode::SingleShot, Duration::from_millis(PREVIEW_DEBOUNCE_MS), move || {
                gui_dbg!("预览渲染开始（seed={}）", seed.borrow());
                // UI 线程：快速收集参数（不耗时，不阻塞）
                let params = match collect_params(&ui, &preset_params, &para_formats_timer) {
                    Ok(p) => p,
                    Err(e) => {
                        gui_dbg!("参数收集失败：{e}");
                        ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                        return;
                    }
                };
                let seed_val = {
                    let mut s = seed.borrow_mut();
                    *s += 1;
                    *s
                };
                let bounds_visible = ui.get_bounds_visible();
                let bounds_color = parse_color(ui.get_bounds_color().as_str()).unwrap_or([76, 166, 166]);
                let t0 = std::time::Instant::now();
                gui_dbg!(
                    "参数收集完成：{:.0}ms（文本 {} 字 / 段落 {} 段）",
                    t0.elapsed().as_secs_f64() * 1000.0,
                    params.text.chars().count(),
                    params.paragraphs.len(),
                );
                ui.set_status_text(SharedString::from("渲染中…"));
                // 后台线程渲染，完成后切回 UI 线程应用结果
                let pages_apply = Arc::clone(&pages);
                let index_apply = Arc::clone(&index);
                spawn_ui_work(
                    &ui,
                    &render_gen,
                    move || -> Result<Vec<RgbaImage>, EngineError> {
                        let mut pages = render_all_pages_preview(&params, seed_val)?;
                        if bounds_visible {
                            for page in pages.iter_mut() {
                                *page = overlay_bounds(page, &params, bounds_color);
                            }
                        }
                        let w = pages.first().map(|p| p.width()).unwrap_or(0);
                        let h = pages.first().map(|p| p.height()).unwrap_or(0);
                        let mb = pages.len() as u64 * w as u64 * h as u64 * 4 / 1024 / 1024;
                        gui_dbg!(
                            "引擎渲染完成：{:.0}ms（{} 页，{}x{}，约 {mb} MB）",
                            t0.elapsed().as_secs_f64() * 1000.0,
                            pages.len(),
                            w,
                            h,
                        );
                        Ok(pages)
                    },
                    move |ui, result| match result {
                        Ok(rendered) => {
                            *pages_apply.lock().unwrap() = rendered;
                            *index_apply.lock().unwrap() = 0;
                            show_page(ui, &pages_apply, &index_apply);
                            let total = pages_apply.lock().unwrap().len();
                            ui.set_status_text(SharedString::from(format!(
                                "预览完成（seed={seed_val}），共 {total} 页"
                            )));
                            gui_dbg!("预览渲染结束（成功，共 {total} 页）");
                        }
                        Err(e) => {
                            gui_dbg!("预览渲染失败：{e}");
                            ui.set_status_text(SharedString::from(format!("渲染失败：{e}")));
                        }
                    },
                );
            });
        });
    }

    // ---- 预览翻页 ----
    {
        let weak = ui.as_weak();
        let pages = Arc::clone(&preview_pages);
        let index = Arc::clone(&preview_index);
        ui.on_prev_page(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut idx = index.lock().unwrap();
            if *idx > 0 {
                *idx -= 1;
            }
            drop(idx);
            show_page(&ui, &pages, &index);
        });
    }
    {
        let weak = ui.as_weak();
        let pages = Arc::clone(&preview_pages);
        let index = Arc::clone(&preview_index);
        ui.on_next_page(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut idx = index.lock().unwrap();
            let total = pages.lock().unwrap().len();
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

    // ---- 导出图片（始终全分辨率，预览降采样不影响导出；后台线程避免 UI 卡顿） ----
    {
        let weak = ui.as_weak();
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        let para_formats = Rc::clone(&para_formats);
        let render_gen = Arc::clone(&render_gen);
        ui.on_export_files(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
            let params = match collect_params(&ui, &preset_params, &para_formats) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                    return;
                }
            };
            let seed_val = *seed.borrow();
            let t0 = std::time::Instant::now();
            ui.set_status_text(SharedString::from("导出中…"));
            let dir_worker = dir.clone();
            spawn_ui_work(
                &ui,
                &render_gen,
                move || -> Result<Vec<PathBuf>, EngineError> {
                    let files = export(&params, &dir_worker, seed_val)?;
                    gui_dbg!("导出完成：{:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
                    Ok(files)
                },
                move |ui, result| match result {
                    Ok(files) => {
                        let msg = format!("已导出 {} 个文件到 {}", files.len(), dir.display());
                        ui.set_status_text(SharedString::from(msg));
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("导出失败：{e}"))),
                },
            );
        });
    }

    // ---- 导出 PDF（位图层，300 DPI；后台线程避免 UI 卡顿） ----
    {
        let weak = ui.as_weak();
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        let para_formats = Rc::clone(&para_formats);
        let render_gen = Arc::clone(&render_gen);
        ui.on_export_pdf(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name("handwrite.pdf")
                .save_file()
            else {
                return;
            };
            let params = match collect_params(&ui, &preset_params, &para_formats) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                    return;
                }
            };
            let seed_val = *seed.borrow();
            let t0 = std::time::Instant::now();
            ui.set_status_text(SharedString::from("导出中…"));
            let path_worker = path.clone();
            spawn_ui_work(
                &ui,
                &render_gen,
                move || -> Result<(), EngineError> {
                    export_pdf(&params, &path_worker, seed_val)?;
                    gui_dbg!("导出 PDF 完成：{:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
                    Ok(())
                },
                move |ui, result| match result {
                    Ok(()) => {
                        ui.set_status_text(SharedString::from(format!("PDF 已导出：{}", path.display())))
                    }
                    Err(e) => ui.set_status_text(SharedString::from(format!("导出 PDF 失败：{e}"))),
                },
            );
        });
    }

    // ---- 段落工具（单框 + 光标段按钮，对齐 Python 版交互） ----

    /// 按字节偏移定位段落索引（\n 为 1 字节，UTF-8 多字节字符不含 \n，计数安全）。
    fn para_index_at(text: &str, byte_offset: usize) -> usize {
        let bytes = text.as_bytes();
        bytes[..byte_offset.min(bytes.len())].iter().filter(|&&b| b == b'\n').count()
    }

    /// 计算光标所在段的格式，写入状态提示。
    fn update_para_status(ui: &MainWindow, fmts: &RefCell<Vec<(u8, i32)>>) {
        let text = ui.get_input_text().to_string();
        let idx = para_index_at(&text, ui.get_para_cursor_bytes() as usize);
        let seg = text.split('\n').nth(idx).unwrap_or("").to_string();
        let (align, indent) = fmts.borrow().get(idx).copied().unwrap_or((0, 0));
        let align_name = ["左对齐", "居中", "右对齐"][(align.min(2)) as usize];
        let indent_txt = if indent > 0 { format!("，首行缩进 {indent}px") } else { String::new() };
        let seg_txt = if seg.trim().is_empty() { "（空段）" } else { "" };
        ui.set_para_status_text(SharedString::from(format!(
            "第 {} 段（{} 字）：{align_name}{indent_txt}{seg_txt}",
            idx + 1,
            seg.chars().count()
        )));
    }

    // 文本编辑：按新段数重排格式数组（新段继承上一段格式，合并保留首段格式）
    {
        let fmts = Rc::clone(&para_formats);
        ui.on_para_text_edited(move |text| {
            let count = text.split('\n').count();
            let mut fmts = fmts.borrow_mut();
            while fmts.len() < count {
                let inherit = *fmts.last().unwrap_or(&(0, 0));
                fmts.push(inherit);
            }
            fmts.truncate(count);
        });
    }
    // 光标移动：刷新当前段状态提示
    {
        let weak = ui.as_weak();
        let fmts = Rc::clone(&para_formats);
        ui.on_para_cursor_moved(move |_byte_offset| {
            let Some(ui) = weak.upgrade() else { return };
            update_para_status(&ui, &fmts);
        });
    }
    // 对齐按钮：作用于光标所在段，改完立即触发防抖预览
    {
        let weak = ui.as_weak();
        let fmts = Rc::clone(&para_formats);
        let fmts_status = Rc::clone(&para_formats);
        ui.on_para_set_align(move |align| {
            let Some(ui) = weak.upgrade() else { return };
            let text = ui.get_input_text().to_string();
            let idx = para_index_at(&text, ui.get_para_cursor_bytes() as usize);
            let mut fmts = fmts.borrow_mut();
            if idx < fmts.len() {
                fmts[idx].0 = align as u8;
            }
            drop(fmts);
            update_para_status(&ui, &fmts_status);
            ui.invoke_regenerate();
        });
    }
    // 缩进按钮：按两字宽缩进/取消（对齐 Python 版 setTextIndent(2*font_size)）
    {
        let weak = ui.as_weak();
        let fmts = Rc::clone(&para_formats);
        let fmts_status = Rc::clone(&para_formats);
        ui.on_para_indent_toggle(move |do_indent| {
            let Some(ui) = weak.upgrade() else { return };
            let text = ui.get_input_text().to_string();
            let idx = para_index_at(&text, ui.get_para_cursor_bytes() as usize);
            let indent = if do_indent { 2 * ui.get_font_size() } else { 0 };
            let mut fmts = fmts.borrow_mut();
            if idx < fmts.len() {
                fmts[idx].1 = indent;
            }
            drop(fmts);
            update_para_status(&ui, &fmts_status);
            ui.invoke_regenerate();
        });
    }

    // 导入 docx：文本 + 每段格式整体写入文本框
    {
        let weak = ui.as_weak();
        let fmts = Rc::clone(&para_formats);
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
                        let text = paras
                            .iter()
                            .map(|p| p.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        ui.set_input_text(SharedString::from(text));
                        *fmts.borrow_mut() = paras
                            .iter()
                            .map(|p| {
                                (
                                    match p.align {
                                        Align::Left => 0,
                                        Align::Center => 1,
                                        Align::Right => 2,
                                    },
                                    p.first_line_indent.round() as i32,
                                )
                            })
                            .collect();
                        ui.set_status_text(SharedString::from(format!(
                            "已导入 {count} 个段落，回车分段、按钮设格式"
                        )));
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
        let para_formats = Rc::clone(&para_formats);
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
                let params = match collect_params(&ui, &preset_params, &para_formats) {
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
///
/// 文本 → 段落规则：按 `\n` 切段并跳过空段；
/// 多段或任一格式非默认（非左对齐/有缩进）时走段落路径，
/// 单段无格式时走纯文本路径（与旧行为逐字一致）。
fn collect_params(
    ui: &MainWindow,
    preset_params: &RefCell<Option<HandwritingParams>>,
    para_formats: &RefCell<Vec<(u8, i32)>>,
) -> Result<HandwritingParams, EngineError> {
    let mut params = preset_params.borrow().clone().unwrap_or_default();
    let raw = ui.get_input_text().to_string();
    let fmts = para_formats.borrow();
    let mut paras = Vec::new();
    for (i, seg) in raw.split('\n').enumerate() {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let (align_idx, indent) = fmts.get(i).copied().unwrap_or((0, 0));
        paras.push(Paragraph {
            text: seg.to_string(),
            align: match align_idx {
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            },
            first_line_indent: indent as f32,
        });
    }
    let has_format = fmts.iter().any(|&(a, i)| a != 0 || i != 0);
    if paras.len() > 1 || has_format {
        params.paragraphs = paras;
    } else {
        params.text = raw.trim().to_string();
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

/// 在后台线程执行重活（渲染/导出），完成后切回 UI 线程应用结果。
///
/// - `render_gen`：代次守卫——只有最新一次提交（代次最大）的结果被应用，
///   期间触发的新渲染会令过期结果被丢弃。
/// - `worker` 与 `apply` 都要求 `Send`（`upgrade_in_event_loop` 的约束）；
///   `apply` 在 UI 线程运行，可安全访问 Slint 句柄与 `Arc<Mutex>` 状态。
fn spawn_ui_work<T, E, W, A>(
    ui: &MainWindow,
    render_gen: &Arc<AtomicU64>,
    worker: W,
    apply: A,
) where
    T: Send + 'static,
    E: Send + 'static,
    W: FnOnce() -> Result<T, E> + Send + 'static,
    A: FnOnce(&MainWindow, Result<T, E>) + Send + 'static,
{
    let gen = render_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let weak = ui.as_weak();
    let render_gen = Arc::clone(render_gen);
    std::thread::spawn(move || {
        let result = worker();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            if gen != render_gen.load(Ordering::SeqCst) {
                return; // 过期结果：期间又有更新的渲染/导出提交
            }
            apply(&ui, result);
        });
    });
}

/// 把当前索引页显示到预览区并更新页码。
fn show_page(
    ui: &MainWindow,
    preview_pages: &Mutex<Vec<RgbaImage>>,
    preview_index: &Mutex<usize>,
) {
    let pages = preview_pages.lock().unwrap();
    let total = pages.len();
    if total == 0 {
        ui.set_page_text(SharedString::from("第 1 / 1 页"));
        return;
    }
    let i = (*preview_index.lock().unwrap()).min(total - 1);
    let img = &pages[i];
    let (width, height) = img.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(img.as_raw(), width, height);
    ui.set_preview_image(Image::from_rgba8(buffer));
    ui.set_page_text(SharedString::from(format!("第 {} / {total} 页", i + 1)));
}

