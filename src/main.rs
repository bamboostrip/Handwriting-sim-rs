//! 手写模拟器桌面入口。
//!
//! 阶段 3：GUI 功能对齐 Python 版（1:1）。
//! - 参数面板 ↔ `HandwritingParams` 双向绑定（Slint `<=>` 属性）
//! - 「生成预览」防抖 300ms 后渲染全部页并翻页显示（对齐 Python 版自动预览 + 多页导航）
//! - 预设下拉框（扫描 exe 旁 presets/ 目录）快捷切换 + 载入/保存
//! - 文字颜色 / 边距 / 扰动 σ / 边界提示（仅预览叠加）
//! - 字体/背景经原生文件对话框选择（rfd）；「导出图片」全分辨率批量导出

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use handwrite_sim::core::doc_render;
use handwrite_sim::core::docx_io;
use handwrite_sim::core::engine::{export, export_pdf, overlay_bounds, render_all_pages_preview, EngineError};
use handwrite_sim::core::models::{
    parse_color, Align, HandwritingParams, MiswriteMode, Paragraph, StrikeoutStyle, TextRegion,
};
use handwrite_sim::core::presets;
use handwrite_sim::ui::{MainWindow, ParaRow, RegionInfo};
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
    // 段落模型：逐段编辑器数据源（每段 = 文本 + 对齐 + 首行缩进字符数）。
    // 缩进以「字符数」（em）存储，渲染时 × 当前字号换算像素，保证恒为 2 字宽。
    let para_model = Rc::new(VecModel::<ParaRow>::default());
    para_model.push(ParaRow { text: SharedString::default(), align: 0, indent_em: 0.0, est_lines: 1, trailing_space_em: 0.0 });
    ui.set_paragraphs(ModelRc::from(para_model.clone()));
    // 当前光标/焦点所在段（对齐/缩进按钮的作用目标）
    let current_row = Rc::new(Cell::new(0usize));
    // ---- 框选文字区域状态 ----
    // 区域数据源（背景原始像素坐标）
    let regions_all = Rc::new(RefCell::new(Vec::<TextRegion>::new()));
    // 预览叠加模型 + 列表摘要模型（与 regions_all 同步刷新）
    let region_model = Rc::new(VecModel::<RegionInfo>::default());
    ui.set_regions(ModelRc::from(region_model.clone()));
    let region_labels = Rc::new(VecModel::<SharedString>::default());
    ui.set_region_labels(ModelRc::from(region_labels.clone()));
    // 正在二次调整的区域索引（-1 = 无）
    let editing_index = Rc::new(Cell::new(-1i32));
    // 新框选完成、对话框尚未确认的暂存矩形（背景原始像素坐标）
    let pending_rect = Rc::new(RefCell::new(None::<[i32; 4]>));
    // 文档底图逐页 PNG 路径（None = 未使用文档底图）；
    // 用 Arc<Mutex> 以便在后台导入任务的 UI 回调（要求 Send）中写入
    let doc_pages: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
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
        let para_model = Rc::clone(&para_model);
        let pages = Arc::clone(&preview_pages);
        let index = Arc::clone(&preview_index);
        let render_gen = Arc::clone(&render_gen);
        let regions_all = Rc::clone(&regions_all);
        let doc_pages = Arc::clone(&doc_pages);
        ui.on_regenerate(move || {
            let Some(ui) = weak.upgrade() else { return };
            gui_dbg!("「预览」按钮触发（300ms 防抖后开始渲染）");
            let timer = Rc::clone(&timer);
            let seed = Rc::clone(&seed);
            let preset_params = Rc::clone(&preset_params);
            let pages = Arc::clone(&pages);
            let index = Arc::clone(&index);
            let render_gen = Arc::clone(&render_gen);
            let para_model_timer = Rc::clone(&para_model);
            let regions_all = Rc::clone(&regions_all);
            let doc_pages = Arc::clone(&doc_pages);
            timer.start(TimerMode::SingleShot, Duration::from_millis(PREVIEW_DEBOUNCE_MS), move || {
                gui_dbg!("预览渲染开始（seed={}）", seed.borrow());
                // UI 线程：快速收集参数（不耗时，不阻塞）
                let params = match collect_params(&ui, &preset_params, &para_model_timer, &regions_all, &doc_pages) {
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
        let para_model = Rc::clone(&para_model);
        let render_gen = Arc::clone(&render_gen);
        let regions_all = Rc::clone(&regions_all);
        let doc_pages = Arc::clone(&doc_pages);
        ui.on_export_files(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
            let params = match collect_params(&ui, &preset_params, &para_model, &regions_all, &doc_pages) {
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
        let para_model = Rc::clone(&para_model);
        let render_gen = Arc::clone(&render_gen);
        let regions_all = Rc::clone(&regions_all);
        let doc_pages = Arc::clone(&doc_pages);
        ui.on_export_pdf(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name("handwrite.pdf")
                .save_file()
            else {
                return;
            };
            let params = match collect_params(&ui, &preset_params, &para_model, &regions_all, &doc_pages) {
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

    // ---- 段落工具（逐段编辑器 + 当前段按钮，对齐 Python 版 QTextEdit 交互） ----

    /// 请求编辑器聚焦指定段（focus-row + focus-nonce 自增触发 delegate 内 focus()）。
    fn request_focus(ui: &MainWindow, row: usize) {
        ui.set_focus_row(row as i32);
        ui.set_focus_nonce(ui.get_focus_nonce() + 1);
    }

    /// 当前段格式提示（状态栏）：对齐方式 + 首行缩进（字符数与换算像素）。
    fn update_para_status(ui: &MainWindow, model: &VecModel<ParaRow>, current: &Cell<usize>) {
        let len = model.row_count();
        if len == 0 {
            ui.set_para_status_text(SharedString::from("光标定位到段落后可用按钮设置格式"));
            return;
        }
        let idx = current.get().min(len - 1);
        let Some(row) = model.row_data(idx) else { return };
        let align_name = ["左对齐", "居中", "右对齐"][(row.align.clamp(0, 2)) as usize];
        let indent_txt = if row.indent_em > 0.0 {
            let em = row.indent_em;
            let px = (em * ui.get_font_size() as f32).round() as i32;
            // 字符数取整显示（2.0 → "2"；docx 导入可能为非整数）
            let em_txt = if (em - em.round()).abs() < 0.01 {
                format!("{}", em.round() as i32)
            } else {
                format!("{em:.1}")
            };
            format!("，首行缩进 {em_txt} 字（{px}px）")
        } else {
            String::new()
        };
        let clean_text = row.text.replace('\u{2060}', "").replace(['\u{00a0}', '\u{ffa0}'], " ");
        let text = clean_text.as_str();
        let seg_txt = if text.trim().is_empty() { "（空段）" } else { "" };
        ui.set_para_status_text(SharedString::from(format!(
            "第 {} 段（{} 字）：{align_name}{indent_txt}{seg_txt}",
            idx + 1,
            text.chars().count()
        )));
    }

    // 段文本编辑：写回模型；粘贴多行时按 \n 拆分为多段（新段继承当前段格式，
    // 焦点落到粘贴内容的最后一段）
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_text_edited(move |idx, text| {
            let len = model.row_count();
            if len == 0 {
                return;
            }
            let idx = (idx as usize).min(len - 1);
            let Some(mut row) = model.row_data(idx) else { return };
            if text.contains('\n') {
                let parts: Vec<&str> = text.split('\n').collect();
                let first_part = to_ui_spaces(parts[0]);
                row.text = SharedString::from(&first_part);
                row.est_lines = estimate_lines(&first_part, row.indent_em);
                row.trailing_space_em = calc_trailing_space_em(&first_part);
                model.set_row_data(idx, row.clone());
                let mut last = idx;
                for (k, part) in parts[1..].iter().enumerate() {
                    let mut new_row = row.clone();
                    let part_k = to_ui_spaces(part);
                    new_row.text = SharedString::from(&part_k);
                    new_row.est_lines = estimate_lines(&part_k, new_row.indent_em);
                    new_row.trailing_space_em = calc_trailing_space_em(&part_k);
                    model.insert(idx + 1 + k, new_row);
                    last = idx + 1 + k;
                }
                current.set(last);
                if let Some(ui) = weak.upgrade() {
                    request_focus(&ui, last);
                    scroll_to_row(&ui, &model, last);
                }
            } else {
                let text_ui = to_ui_spaces(&text);
                row.text = SharedString::from(&text_ui);
                row.est_lines = estimate_lines(&text_ui, row.indent_em);
                row.trailing_space_em = calc_trailing_space_em(&text_ui);
                model.set_row_data(idx, row);
                current.set(idx);
                if let Some(ui) = weak.upgrade() {
                    scroll_to_row(&ui, &model, idx);
                }
            }
        });
    }
    // 光标移动/段获得焦点：记录当前段并刷新状态提示，自动滚动段落入视野
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_cursor_moved(move |idx| {
            let Some(ui) = weak.upgrade() else { return };
            if model.row_count() > 0 {
                let r = (idx as usize).min(model.row_count() - 1);
                current.set(r);
                scroll_to_row(&ui, &model, r);
            }
            update_para_status(&ui, &model, &current);
        });
    }
    // 对齐按钮：作用于当前段，编辑器内即刻可见，并触发防抖预览
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_set_align(move |align| {
            let Some(ui) = weak.upgrade() else { return };
            let len = model.row_count();
            if len == 0 {
                return;
            }
            let idx = current.get().min(len - 1);
            if let Some(mut row) = model.row_data(idx) {
                row.align = align;
                model.set_row_data(idx, row);
            }
            update_para_status(&ui, &model, &current);
            ui.invoke_regenerate();
        });
    }
    // 缩进按钮：2 字符首行缩进/取消。以字符数（em）存储，渲染时 × 当前字号，
    // 改字号后仍恒为两字宽（对齐 Python 版 setTextIndent(2*font_size) 的意图）
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_indent_toggle(move |do_indent| {
            let Some(ui) = weak.upgrade() else { return };
            let len = model.row_count();
            if len == 0 {
                return;
            }
            let idx = current.get().min(len - 1);
            if let Some(mut row) = model.row_data(idx) {
                row.indent_em = if do_indent { 2.0 } else { 0.0 };
                row.est_lines = estimate_lines(&row.text, row.indent_em);
                model.set_row_data(idx, row);
                scroll_to_row(&ui, &model, idx);
            }
            update_para_status(&ui, &model, &current);
            ui.invoke_regenerate();
        });
    }
    // 回车分段：光标处拆分，后半段继承对齐与缩进格式，重新估算两段高度，聚焦并滚入新段
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_split(move |idx, byte_pos| {
            let Some(ui) = weak.upgrade() else { return };
            let len = model.row_count();
            if len == 0 {
                return;
            }
            let idx = (idx as usize).min(len - 1);
            let Some(mut row) = model.row_data(idx) else { return };
            let text = row.text.to_string();
            // 防御：字节偏移夹取并对齐到字符边界
            let mut pos = (byte_pos as usize).min(text.len());
            while pos > 0 && !text.is_char_boundary(pos) {
                pos -= 1;
            }
            let after = text[pos..].to_string();
            let before = text[..pos].to_string();
            
            let before_ui = to_ui_spaces(&before);
            row.text = SharedString::from(&before_ui);
            row.est_lines = estimate_lines(&before_ui, row.indent_em);
            row.trailing_space_em = calc_trailing_space_em(&before_ui);
            model.set_row_data(idx, row.clone());
            
            let after_ui = to_ui_spaces(&after);
            row.text = SharedString::from(&after_ui);
            row.est_lines = estimate_lines(&after_ui, row.indent_em);
            row.trailing_space_em = calc_trailing_space_em(&after_ui);
            model.insert(idx + 1, row);
            
            current.set(idx + 1);
            update_para_status(&ui, &model, &current);
            request_focus(&ui, idx + 1);
            scroll_to_row(&ui, &model, idx + 1);
        });
    }
    // 段首退格：并入上一段（文本拼接，格式保留上一段），重新计算行数并自动滚入上一段
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_merge_prev(move |idx| {
            let Some(ui) = weak.upgrade() else { return };
            let idx = idx as usize;
            if idx == 0 || idx >= model.row_count() {
                return;
            }
            let Some(cur) = model.row_data(idx) else { return };
            let Some(mut prev) = model.row_data(idx - 1) else { return };
            let combined = format!("{}{}", prev.text, cur.text);
            let combined_ui = to_ui_spaces(&combined);
            prev.text = SharedString::from(&combined_ui);
            prev.est_lines = estimate_lines(&combined_ui, prev.indent_em);
            prev.trailing_space_em = calc_trailing_space_em(&combined_ui);
            model.set_row_data(idx - 1, prev);
            model.remove(idx);
            current.set(idx - 1);
            update_para_status(&ui, &model, &current);
            request_focus(&ui, idx - 1);
            scroll_to_row(&ui, &model, idx - 1);
        });
    }
    // 底部空白区点击：聚焦最后一段，自动滚入视野
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
        ui.on_para_focus_last(move || {
            let Some(ui) = weak.upgrade() else { return };
            let len = model.row_count();
            if len == 0 {
                return;
            }
            current.set(len - 1);
            update_para_status(&ui, &model, &current);
            request_focus(&ui, len - 1);
            scroll_to_row(&ui, &model, len - 1);
        });
    }

    // 导入 docx：文本 + 每段格式整体写入逐段编辑器
    {
        let weak = ui.as_weak();
        let model = Rc::clone(&para_model);
        let current = Rc::clone(&current_row);
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
                        model.set_vec(
                            paras
                                .iter()
                                .map(|p| {
                                    let indent_em = if font_size > 0.0 {
                                        p.first_line_indent / font_size
                                    } else {
                                        0.0
                                    };
                                    let text_ui = to_ui_spaces(&p.text);
                                    let est_lines = estimate_lines(&text_ui, indent_em);
                                    ParaRow {
                                        text: SharedString::from(text_ui.as_str()),
                                        align: match p.align {
                                            Align::Left => 0,
                                            Align::Center => 1,
                                            Align::Right => 2,
                                        },
                                        indent_em,
                                        est_lines,
                                        trailing_space_em: calc_trailing_space_em(&text_ui),
                                    }
                                })
                                .collect::<Vec<_>>(),
                        );
                        if model.row_count() == 0 {
                            model.push(ParaRow {
                                text: SharedString::default(),
                                align: 0,
                                indent_em: 0.0,
                                est_lines: 1,
                                trailing_space_em: 0.0,
                            });
                        }
                        current.set(0);
                        update_para_status(&ui, &model, &current);
                        request_focus(&ui, 0);
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
        let para_model = Rc::clone(&para_model);
        let preset_model = Rc::clone(&preset_model);
        let preset_paths = Rc::clone(&preset_paths);
        let regions_all = Rc::clone(&regions_all);
        let doc_pages = Arc::clone(&doc_pages);
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
                let params = match collect_params(&ui, &preset_params, &para_model, &regions_all, &doc_pages) {
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

    // ---- 框选文字区域：辅助函数与回调（对齐 Python 版 main_window 区域接线） ----

    /// 把区域列表同步到预览叠加模型与列表面板。
    fn refresh_region_ui(
        regions: &Rc<RefCell<Vec<TextRegion>>>,
        model: &Rc<VecModel<RegionInfo>>,
        labels: &Rc<VecModel<SharedString>>,
    ) {
        let regs = regions.borrow();
        model.set_vec(
            regs.iter()
                .enumerate()
                .map(|(i, r)| RegionInfo {
                    x: r.x as f32,
                    y: r.y as f32,
                    w: r.w as f32,
                    h: r.h as f32,
                    page: r.page,
                    label: SharedString::from(r.label(i + 1)),
                })
                .collect::<Vec<RegionInfo>>(),
        );
        labels.set_vec(
            regs.iter()
                .enumerate()
                .map(|(i, r)| SharedString::from(r.label(i + 1)))
                .collect::<Vec<SharedString>>(),
        );
    }

    fn set_editing(ui: &MainWindow, editing: &Rc<Cell<i32>>, idx: i32) {
        editing.set(idx);
        ui.set_editing_index(idx);
    }

    /// 读取背景图尺寸（只读头，不完整解码）。
    fn bg_dimensions(path: &str) -> Option<(i32, i32)> {
        image::ImageReader::open(path)
            .ok()?
            .into_dimensions()
            .ok()
            .map(|(w, h)| (w as i32, h as i32))
    }

    /// 预览坐标 → 背景原始像素的缩放比（原始宽 / 预览宽；≥1）。
    fn preview_scale(ui: &MainWindow) -> f32 {
        let nat_w = ui.get_preview_nat_w();
        if nat_w <= 0.0 {
            return 1.0;
        }
        let bg_text = ui.get_background_path_text();
        match bg_dimensions(bg_text.as_str().trim()) {
            Some((w, _)) => w as f32 / nat_w,
            None => 1.0,
        }
    }

    /// 把框选矩形（背景像素）钳制到背景范围内并保证最小尺寸。
    fn clamp_rect(x: i32, y: i32, w: i32, h: i32, bw: i32, bh: i32) -> Option<[i32; 4]> {
        if bw <= 8 || bh <= 8 {
            return None;
        }
        let w = w.max(8);
        let h = h.max(8);
        let x = x.max(0).min(bw - 8);
        let y = y.max(0).min(bh - 8);
        Some([x, y, w.min(bw - x).max(1), h.min(bh - y).max(1)])
    }

    /// 打开区域编辑对话框；`index` 为 Some 时回填已有区域，None 时为新建。
    fn open_region_dialog(
        ui: &MainWindow,
        regions: &Rc<RefCell<Vec<TextRegion>>>,
        index: Option<usize>,
        default_page: i32,
    ) {
        match index.and_then(|i| regions.borrow().get(i).cloned()) {
            Some(r) => {
                ui.set_dialog_text(SharedString::from(r.text.clone()));
                ui.set_dialog_style_index(if r.printed { 1 } else { 0 });
                ui.set_dialog_font_path(SharedString::from(r.font_path.clone()));
                ui.set_dialog_font_size(r.font_size);
                ui.set_dialog_page(r.page.max(1));
                ui.set_dialog_target_index(index.map(|i| i as i32).unwrap_or(-1));
            }
            None => {
                ui.set_dialog_text(SharedString::default());
                ui.set_dialog_style_index(0);
                ui.set_dialog_font_path(SharedString::default());
                ui.set_dialog_font_size(0);
                ui.set_dialog_page(default_page.max(1));
                ui.set_dialog_target_index(-1);
            }
        }
        ui.set_dialog_open(true);
    }

    /// 文档底图缓存目录（LOCALAPPDATA 或系统临时目录）。
    fn doc_cache_dir() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(std::env::temp_dir)
            .join("handwrite-sim")
            .join("doc_bg")
    }

    // 框选模式开关：关闭时结束进行中的区域调整
    {
        let weak = ui.as_weak();
        let editing = Rc::clone(&editing_index);
        ui.on_toggle_region_mode(move || {
            let Some(ui) = weak.upgrade() else { return };
            let new_val = !ui.get_region_mode();
            ui.set_region_mode(new_val);
            if !new_val && editing.get() >= 0 {
                set_editing(&ui, &editing, -1);
            }
        });
    }

    // 预览图上框选完成：换算回原始背景坐标并存暂存，弹出编辑对话框
    {
        let weak = ui.as_weak();
        let pending = Rc::clone(&pending_rect);
        ui.on_region_selected(move |sx, sy, sw, sh| {
            let Some(ui) = weak.upgrade() else { return };
            let scale = preview_scale(&ui);
            let bg_text = ui.get_background_path_text();
            let rect = bg_dimensions(bg_text.as_str().trim())
                .and_then(|(bw, bh)| {
                    clamp_rect(
                        (sx * scale).round() as i32,
                        (sy * scale).round() as i32,
                        (sw * scale).round() as i32,
                        (sh * scale).round() as i32,
                        bw,
                        bh,
                    )
                });
            let Some(rect) = rect else { return };
            let page = (ui.get_current_page_index() + 1).max(1);
            *pending.borrow_mut() = Some(rect);
            open_region_dialog(&ui, &(Rc::new(RefCell::new(Vec::new()))), None, page);
        });
    }

    // 二次调整写回：按当前比例换算并钳制到背景范围
    {
        let weak = ui.as_weak();
        let regions = Rc::clone(&regions_all);
        let region_model = Rc::clone(&region_model);
        let region_labels = Rc::clone(&region_labels);
        ui.on_region_geometry_changed(move |idx, sx, sy, sw, sh| {
            let Some(ui) = weak.upgrade() else { return };
            let idx = idx as usize;
            let scale = preview_scale(&ui);
            let bg_text = ui.get_background_path_text();
            let mut regs = regions.borrow_mut();
            let Some(region) = regs.get_mut(idx) else { return };
            let Some(rect) = bg_dimensions(bg_text.as_str().trim())
                .and_then(|(bw, bh)| {
                    clamp_rect(
                        (sx * scale).round() as i32,
                        (sy * scale).round() as i32,
                        (sw * scale).round() as i32,
                        (sh * scale).round() as i32,
                        bw,
                        bh,
                    )
                })
            else {
                return;
            };
            region.x = rect[0];
            region.y = rect[1];
            region.w = rect[2];
            region.h = rect[3];
            drop(regs);
            refresh_region_ui(&regions, &region_model, &region_labels);
            gui_dbg!("区域 {} 调整为 {:?}", idx + 1, rect);
            ui.invoke_regenerate();
        });
    }

    // 编辑态被取消（Esc / 点击框外 / 关闭模式）
    {
        let weak = ui.as_weak();
        let editing = Rc::clone(&editing_index);
        ui.on_region_edit_cancelled(move || {
            let Some(ui) = weak.upgrade() else { return };
            set_editing(&ui, &editing, -1);
        });
    }

    // 列表项点击：跳到该页并进入调整态
    {
        let weak = ui.as_weak();
        let regions = Rc::clone(&regions_all);
        let editing = Rc::clone(&editing_index);
        let pages = Arc::clone(&preview_pages);
        let index = Arc::clone(&preview_index);
        ui.on_region_item_clicked(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            let row = row as usize;
            let page = {
                let regs = regions.borrow();
                match regs.get(row) {
                    Some(r) => r.page.max(1),
                    None => return,
                }
            };
            // 区域不在当前页时先翻页（页码 1 基 → 索引 0 基）
            if page - 1 != ui.get_current_page_index() {
                {
                    let mut idx = index.lock().unwrap();
                    let total = pages.lock().unwrap().len();
                    let target = (page as usize - 1).min(total.saturating_sub(1));
                    *idx = target;
                }
                show_page(&ui, &pages, &index);
            }
            set_editing(&ui, &editing, row as i32);
        });
    }

    // 列表项悬浮：临时高亮对应区域红框
    {
        let weak = ui.as_weak();
        ui.on_region_item_hovered(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_highlight_index(row);
        });
    }

    // 双击列表项：编辑区域属性
    {
        let weak = ui.as_weak();
        let regions = Rc::clone(&regions_all);
        ui.on_region_edit_requested(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            open_region_dialog(&ui, &regions, Some(row as usize), 1);
        });
    }

    // 删除单个区域
    {
        let weak = ui.as_weak();
        let regions = Rc::clone(&regions_all);
        let region_model = Rc::clone(&region_model);
        let region_labels = Rc::clone(&region_labels);
        let editing = Rc::clone(&editing_index);
        ui.on_region_delete(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            let row = row as usize;
            let mut regs = regions.borrow_mut();
            if row < regs.len() {
                regs.remove(row);
            }
            drop(regs);
            set_editing(&ui, &editing, -1);
            ui.set_highlight_index(-1);
            refresh_region_ui(&regions, &region_model, &region_labels);
            ui.invoke_regenerate();
        });
    }

    // 清空全部区域
    {
        let weak = ui.as_weak();
        let regions = Rc::clone(&regions_all);
        let region_model = Rc::clone(&region_model);
        let region_labels = Rc::clone(&region_labels);
        let editing = Rc::clone(&editing_index);
        ui.on_region_clear(move || {
            let Some(ui) = weak.upgrade() else { return };
            regions.borrow_mut().clear();
            set_editing(&ui, &editing, -1);
            ui.set_highlight_index(-1);
            refresh_region_ui(&regions, &region_model, &region_labels);
            ui.invoke_regenerate();
        });
    }

    // 区域对话框确认：index < 0 = 新建（用暂存矩形），否则更新已有区域属性
    {
        let weak = ui.as_weak();
        let regions = Rc::clone(&regions_all);
        let region_model = Rc::clone(&region_model);
        let region_labels = Rc::clone(&region_labels);
        let pending = Rc::clone(&pending_rect);
        ui.on_region_dialog_confirmed(
            move |idx, text, printed, font_path, font_size, page| {
                let Some(ui) = weak.upgrade() else { return };
                let text = text.trim().to_string();
                if text.is_empty() {
                    *pending.borrow_mut() = None;
                    ui.set_status_text(SharedString::from("区域文字为空，已放弃该区域"));
                    return;
                }
                let font_path = font_path.trim().to_string();
                if !font_path.is_empty() && !Path::new(&font_path).is_file() {
                    ui.set_status_text(SharedString::from(format!(
                        "文字区域字体文件不存在：{font_path}"
                    )));
                    return;
                }
                let mut regs = regions.borrow_mut();
                if idx < 0 {
                    // 新建：取暂存矩形
                    let Some(rect) = pending.borrow_mut().take() else {
                        drop(regs);
                        return;
                    };
                    regs.push(TextRegion {
                        x: rect[0],
                        y: rect[1],
                        w: rect[2],
                        h: rect[3],
                        text,
                        font_path,
                        printed,
                        font_size,
                        page: page.max(1),
                    });
                } else if let Some(region) = regs.get_mut(idx as usize) {
                    // 编辑：只更新属性，几何以调整框为准
                    region.text = text;
                    region.printed = printed;
                    region.font_path = font_path;
                    region.font_size = font_size;
                    let new_page = page.max(1);
                    region.page = new_page;
                }
                drop(regs);
                refresh_region_ui(&regions, &region_model, &region_labels);
                ui.invoke_regenerate();
            },
        );
    }

    // 区域对话框取消：丢弃暂存矩形
    {
        let pending = Rc::clone(&pending_rect);
        ui.on_region_dialog_cancelled(move || {
            *pending.borrow_mut() = None;
        });
    }

    // 对话框内选择打印字体
    {
        let weak = ui.as_weak();
        ui.on_choose_region_font(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("字体文件", &["ttf", "ttc", "otf"])
                .pick_file()
            {
                ui.set_dialog_font_path(SharedString::from(path.to_string_lossy().into_owned()));
            }
        });
    }

    // 导入 PDF/DOCX 文档底图：后台渲染逐页 PNG，替换当前背景
    {
        let weak = ui.as_weak();
        let doc_pages = Arc::clone(&doc_pages);
        let render_gen = Arc::clone(&render_gen);
        ui.on_import_document(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("文档", &["pdf", "docx"])
                .pick_file()
            else {
                return;
            };
            ui.set_status_text(SharedString::from("正在渲染文档底图…"));
            // worker 与 apply 各需一份独立克隆（两者都是 move 闭包）
            let doc_pages_apply = Arc::clone(&doc_pages);
            spawn_ui_work(
                &ui,
                &render_gen,
                move || -> Result<Vec<PathBuf>, EngineError> {
                    let pages =
                        doc_render::document_to_page_images(&path, &doc_cache_dir(), 200)
                            .map_err(|e| EngineError::Doc(e.to_string()))?;
                    Ok(pages)
                },
                move |ui, result| match result {
                    Ok(pages) => {
                        let count = pages.len();
                        let first = pages
                            .first()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        *doc_pages_apply.lock().unwrap() = Some(
                            pages
                                .iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect(),
                        );
                        ui.set_background_path_text(SharedString::from(first));
                        ui.set_doc_status_text(SharedString::from(format!(
                            "已导入 {count} 页，可逐页框选"
                        )));
                        ui.set_status_text(SharedString::from(format!(
                            "已导入文档底图（{count} 页）；在目标页开启「框选」即可填写"
                        )));
                        ui.invoke_regenerate();
                    }
                    Err(e) => {
                        ui.set_doc_status_text(SharedString::default());
                        ui.set_status_text(SharedString::from(format!("导入文档失败：{e}")));
                    }
                },
            );
        });
    }

    ui.run()?;
    Ok(())
}

/// 清理 UI 存储的特殊字符（NBSP/FFA0/WJ），还原为普通空格，防止外来文本污染。
fn to_ui_spaces(s: &str) -> String {
    s.replace('\u{2060}', "").replace(['\u{00a0}', '\u{ffa0}'], " ")
}

/// 计算文本末尾连续空格对应的 em 宽度（右对齐占位用）。
/// 空格宽度 ≈ 0.55 × 字号（与 estimate_lines 中 ASCII 字符宽度因子保持一致）。
fn calc_trailing_space_em(text: &str) -> f32 {
    let trailing = text.chars().rev().take_while(|c| *c == ' ').count();
    trailing as f32 * 0.55
}

/// 估算段落在编辑器中的显示行数（≥1）。
/// 楷体汉字宽约 13px，ASCII 字符约 7px (0.55 * 13px)。
/// 编辑器可见宽度约 400px，首行缩进占对应宽度。
fn estimate_lines(text: &str, indent_em: f32) -> i32 {
    let clean = text.replace('\u{2060}', "").replace(['\u{00a0}', '\u{ffa0}'], " ");
    if clean.is_empty() {
        return 1;
    }
    let font_size = 13.0f32; // Theme.base-font
    let editor_width = 400.0f32;
    let indent_px = indent_em * font_size;
    let available_width = (editor_width - indent_px - 25.0).max(100.0);

    let mut current_line_width = 0.0f32;
    let mut lines = 1;

    for c in clean.chars() {
        let char_w = if c.is_ascii() {
            0.55 * font_size
        } else {
            font_size
        };

        if current_line_width + char_w > available_width {
            lines += 1;
            current_line_width = char_w;
        } else {
            current_line_width += char_w;
        }
    }

    lines
}

/// 根据当前焦点段落的位置计算滚动位置，自动将焦点段滚入视野
fn scroll_to_row(ui: &MainWindow, model: &VecModel<ParaRow>, row_idx: usize) {
    let len = model.row_count();
    if len == 0 || row_idx >= len {
        return;
    }

    let font_size = 13.0f32; // Theme.base-font
    let line_height = font_size * 1.5;
    let separator_height = 1.0f32;
    let layout_spacing = 2.0f32;
    let block_spacing = separator_height + layout_spacing; // 3px total spacing between text inputs

    // 计算目标段落的顶部和底部 Y 坐标
    let mut y_top = 0.0f32;
    for i in 0..row_idx {
        if let Some(row) = model.row_data(i) {
            y_top += (row.est_lines as f32) * line_height + block_spacing;
        }
    }

    let target_est_lines = match model.row_data(row_idx) {
        Some(row) => row.est_lines,
        None => 1,
    };
    let y_bottom = y_top + (target_est_lines as f32) * line_height + separator_height;

    // 视口可见高度 182px (Rectangle 190 - 上下 8px padding)
    let visible_height = 182.0f32;

    // 获取当前滚动条位置 (Slint 内部是负数)
    let current_viewport_y = ui.get_para_viewport_y();

    let mut new_viewport_y = current_viewport_y;

    if y_top < -current_viewport_y {
        new_viewport_y = -y_top;
    } else if y_bottom > -current_viewport_y + visible_height {
        new_viewport_y = -(y_bottom - visible_height);
    }

    // 计算内容总高度，防止过度滚动
    let total_height = {
        let mut h = 0.0f32;
        for i in 0..len {
            if let Some(row) = model.row_data(i) {
                h += (row.est_lines as f32) * line_height + block_spacing;
            }
        }
        h += 30.0f32; // 底部 spacer
        h
    };

    let min_viewport_y = -(total_height - visible_height).max(0.0f32);
    new_viewport_y = new_viewport_y.clamp(min_viewport_y, 0.0f32);

    ui.set_para_viewport_y(new_viewport_y);
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
    ui.set_perturb_theta_text(SharedString::from(format!("{}", p.perturb_theta_sigma)));
    ui.set_miswrite_rate(p.miswrite_rate * 100.0);
    ui.set_miswrite_mode_index(match p.miswrite_rewrite_mode {
        MiswriteMode::Above => 0,
        MiswriteMode::Rewrite => 1,
    });
    ui.set_miswrite_strikeout_style_index(match p.miswrite_strikeout_style {
        StrikeoutStyle::Line => 0,
        StrikeoutStyle::DoubleLine => 1,
        StrikeoutStyle::Slash => 2,
        StrikeoutStyle::Cross => 3,
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
/// 段落来源：逐段编辑器模型（ParaRow）。段文本**保留首尾空格**——
/// 空格参与排版占宽（右对齐时行尾空格把文字顶向左，编辑器所见即所得）；
/// 全空白段跳过（对齐旧版空段忽略行为）。
/// 首行缩进以字符数（em）存储，此处 × 当前字号换算为像素。
/// 多段或任一格式非默认（非左对齐/有缩进）时走段落路径，
/// 单段无格式时走纯文本路径（与旧行为逐字一致）。
fn collect_params(
    ui: &MainWindow,
    preset_params: &RefCell<Option<HandwritingParams>>,
    para_model: &VecModel<ParaRow>,
    regions_all: &RefCell<Vec<TextRegion>>,
    doc_pages: &Mutex<Option<Vec<String>>>,
) -> Result<HandwritingParams, EngineError> {
    let mut params = preset_params.borrow().clone().unwrap_or_default();
    let font_size = ui.get_font_size() as f32;
    let mut paras = Vec::new();
    let mut has_format = false;
    for i in 0..para_model.row_count() {
        let Some(row) = para_model.row_data(i) else { continue };
        if row.align != 0 || row.indent_em != 0.0 {
            has_format = true;
        }
        if row.text.trim().is_empty() {
            continue;
        }
        paras.push(Paragraph {
            text: row.text.replace('\u{2060}', "").replace(['\u{00a0}', '\u{ffa0}'], " "),
            align: match row.align {
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            },
            first_line_indent: row.indent_em * font_size,
        });
    }
    if paras.len() > 1 || has_format {
        params.paragraphs = paras;
    } else {
        params.text = paras
            .first()
            .map(|p| p.text.trim().to_string())
            .unwrap_or_default();
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
    // 笔画旋转：文本输入浮点数（对齐 Python 版 _float_of 失败回退默认值）
    params.perturb_theta_sigma = ui
        .get_perturb_theta_text()
        .trim()
        .parse::<f32>()
        .unwrap_or(HandwritingParams::default().perturb_theta_sigma);
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
    params.miswrite_strikeout_style = match ui.get_miswrite_strikeout_style_index() {
        1 => StrikeoutStyle::DoubleLine,
        2 => StrikeoutStyle::Slash,
        3 => StrikeoutStyle::Cross,
        _ => StrikeoutStyle::Line,
    };
    // 文字颜色
    params.fill = parse_color(ui.get_font_color().as_str()).map_err(EngineError::Params)?;
    // 框选区域 + 多页文档底图（区域坐标为背景原始像素，直接随参数进引擎）。
    // 背景路径被手动改走时文档底图自动失效（对齐 Python 版 _sync_doc_state）
    params.regions = regions_all.borrow().clone();
    let doc = doc_pages.lock().unwrap().clone();
    let bg_now = ui.get_background_path_text().as_str().trim().to_string();
    params.background_pages = match doc {
        Some(pages) if pages.first().map(|p| p.as_str()) == Some(bg_now.as_str()) => pages,
        _ => Vec::new(),
    };
    // 纯背景预览合法（无文字/区域时只要求背景有效），与 Python 版一致
    params.validate_with(false).map_err(EngineError::Params)?;
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
        ui.set_preview_nat_w(0.0);
        ui.set_preview_nat_h(0.0);
        ui.set_current_page_index(0);
        ui.set_page_text(SharedString::from("第 1 / 1 页"));
        return;
    }
    let i = (*preview_index.lock().unwrap()).min(total - 1);
    let img = &pages[i];
    let (width, height) = img.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(img.as_raw(), width, height);
    ui.set_preview_image(Image::from_rgba8(buffer));
    // 预览图自然尺寸 + 当前页索引：供框选坐标换算与区域叠加过滤
    ui.set_preview_nat_w(width as f32);
    ui.set_preview_nat_h(height as f32);
    ui.set_current_page_index(i as i32);
    // 背景原始像素 → 预览像素的缩放比（大背景降采样后 <1），
    // 区域叠加/编辑框按「原始 × 本系数 × fit-scale」定位
    let bg_text = ui.get_background_path_text();
    let scale = image::ImageReader::open(bg_text.as_str().trim())
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, _)| if w > 0 { width as f32 / w as f32 } else { 1.0 })
        .unwrap_or(1.0);
    ui.set_bg_preview_scale(scale.min(1.0));
    ui.set_page_text(SharedString::from(format!("第 {} / {total} 页", i + 1)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use handwrite_sim::core::models::StrikeoutStyle;
    use slint::SharedString;
    use std::cell::RefCell;

    #[test]
    fn test_ui_strikeout_style_mapping() {
        // CI 无显示环境（Linux 无 DISPLAY / macOS headless runner）：
        // 显式初始化 Slint testing backend（headless，无需窗口系统）
        i_slint_backend_testing::init_integration_test_with_system_time();
        let ui = MainWindow::new().unwrap();
        let preset_params = RefCell::new(None);
        let para_model = VecModel::default();
        para_model.push(ParaRow {
            text: SharedString::from("测试文本"),
            align: 0,
            indent_em: 0.0,
            est_lines: 1,
            trailing_space_em: 0.0,
        });

        let dummy_font = tempfile::NamedTempFile::new().unwrap();
        let font_path = dummy_font.path().to_string_lossy().to_string();
        let dummy_bg = tempfile::NamedTempFile::new().unwrap();
        let bg_path = dummy_bg.path().to_string_lossy().to_string();

        let params = HandwritingParams {
            text: "测试文本".to_string(),
            font_path,
            background_path: bg_path,
            miswrite_strikeout_style: StrikeoutStyle::Cross,
            miswrite_rewrite_mode: MiswriteMode::Rewrite,
            ..HandwritingParams::default()
        };

        apply_preset_to_ui(&ui, &preset_params, &params);
        assert_eq!(ui.get_miswrite_strikeout_style_index(), 3);
        assert_eq!(ui.get_miswrite_mode_index(), 1);

        let regions_all = RefCell::new(Vec::<TextRegion>::new());
        let doc_pages = std::sync::Mutex::new(None::<Vec<String>>);
        let collected =
            collect_params(&ui, &preset_params, &para_model, &regions_all, &doc_pages).unwrap();
        assert_eq!(collected.miswrite_strikeout_style, StrikeoutStyle::Cross);
        assert_eq!(collected.miswrite_rewrite_mode, MiswriteMode::Rewrite);

        // 区域随参数进入引擎：collect_params 应带上 UI 侧的区域状态
        regions_all.borrow_mut().push(TextRegion {
            x: 10,
            y: 20,
            w: 100,
            h: 60,
            text: "区域文字".into(),
            printed: true,
            ..TextRegion::default()
        });
        let collected =
            collect_params(&ui, &preset_params, &para_model, &regions_all, &doc_pages).unwrap();
        assert_eq!(collected.regions.len(), 1);
        assert!(collected.regions[0].printed);
    }
}


