//! 手写模拟器主应用（egui 版）。
//!
//! 对应 iced 版 `HandwriteApp` 的全部业务逻辑，改为即时模式：
//! - 防抖自动预览：停止输入 300ms 后 `ctx.request_repaint_after` 触发后台线程渲染
//! - 参数收集/回填走 `params` 纯函数；段落走 `editor` 纯函数
//! - 预设下拉 / 载入 / 保存、docx 导入、字体/背景选择（rfd 走 `std::thread`）
//! - 翻页、预览底色切换、导出图片 / PDF
//!
//! 状态字段随实现阶段逐步接线（编辑器渲染→参数面板→后台任务），
//! 完成后移除 `AppState` 上的 `#[allow(dead_code)]`。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

use crate::core::models::HandwritingParams;
use crate::ui::editor::ParagraphEditor;
use crate::ui::params::UiParams;
use crate::ui::theme;

/// 预览防抖间隔（毫秒，README 承诺「停止输入 300ms 后自动渲染」）。
pub(crate) const PREVIEW_DEBOUNCE_MS: u64 = 300;
/// 预览区底色循环（对齐 Python 版 `_PREVIEW_BG_COLORS`）。
pub(crate) const PREVIEW_BG_COLORS: [[u8; 3]; 2] = [[200, 208, 202], [86, 91, 86]];
/// 预设下拉框占位项。
pub(crate) const PRESET_PLACEHOLDER: &str = "— 选择预设 —";
/// 编辑器字号（对齐 iced/Slint 版 Theme.base-font 13px）。
pub(crate) const EDITOR_FONT_SIZE: f32 = 13.0;
/// 右侧参数面板宽度（对齐 iced 版 460）。
const PANEL_WIDTH: f32 = 460.0;

/// 后台线程 → UI 的消息（单一持久 mpsc channel，所有 worker 共用，
/// 避免并发 worker 互相覆盖接收端导致丢消息）。
enum WorkerMsg {
    RenderDone(Result<Vec<RgbaImage>, String>),
    ExportDone(Result<Vec<PathBuf>, String>),
    PdfDone(Result<(), String>),
    FontPicked(Option<PathBuf>),
    BackgroundPicked(Option<PathBuf>),
    DocxPicked(Option<PathBuf>),
    PresetPicked(Option<PathBuf>),
    PresetSavePath(Option<PathBuf>),
    ExportDirPicked(Option<PathBuf>),
    PdfPathPicked(Option<PathBuf>),
}

/// 编辑器在一帧内收集的动作（帧末统一应用，避免渲染迭代中改 vec 的借用冲突）。
enum ParaAction {
    /// 段内出现换行（回车/粘贴）→ 拆成多段。
    SplitNewlines(usize),
    /// 段首退格 → 并入上一段。
    MergePrev(usize),
    /// 编辑器底部空白点击 → 聚焦最后一段。
    FocusLast,
}

/// 主应用状态。
#[allow(dead_code)] // 字段随实现阶段逐步接线，Phase 4 移除
pub struct AppState {
    pub(crate) editor: ParagraphEditor,
    pub(crate) ui: UiParams,
    preset_params: Option<HandwritingParams>,
    pub(crate) preset_names: Vec<String>,
    pub(crate) preset_paths: Vec<PathBuf>,
    pub(crate) preset_chosen: Option<String>,
    pending_save: Option<HandwritingParams>,
    // 预览
    pub(crate) preview_pages: Vec<RgbaImage>,
    pub(crate) preview_index: usize,
    pub(crate) preview_bg_idx: usize,
    pub(crate) page_text: String,
    /// 缓存的预览纹理（翻页/底色时才重建，避免每帧上传）。
    preview_texture: Option<egui::TextureHandle>,
    pub(crate) status: String,
    pub(crate) para_status: String,
    // 防抖 / 渲染
    seed: u64,
    last_edit: Instant,
    dirty: bool,
    rendering: bool,
    /// 后台 worker → UI 的持久 channel（boot 时创建）。
    worker_tx: Option<std::sync::mpsc::Sender<WorkerMsg>>,
    worker_rx: Option<std::sync::mpsc::Receiver<WorkerMsg>>,
    /// 编辑器交互收集到的待聚焦段（拆段/合并后）。
    pending_focus: Option<usize>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            editor: ParagraphEditor::empty(),
            ui: UiParams::default(),
            preset_params: None,
            preset_names: Vec::new(),
            preset_paths: Vec::new(),
            preset_chosen: None,
            pending_save: None,
            preview_pages: Vec::new(),
            preview_index: 0,
            preview_bg_idx: 0,
            page_text: "第 1 / 1 页".to_string(),
            preview_texture: None,
            status: "就绪".to_string(),
            para_status: "光标定位到段落后可用按钮设置格式".to_string(),
            seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            last_edit: Instant::now(),
            dirty: false,
            rendering: false,
            worker_tx: None,
            worker_rx: None,
            pending_focus: None,
        }
    }
}

/// 启动应用。
pub fn run() -> eframe::Result {
    let icon = load_icon();
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 840.0])
        .with_icon(icon)
        .with_title("手写模拟器");
    let opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "手写模拟器",
        opts,
        Box::new(|cc| Ok(Box::new(AppState::boot(cc)))),
    )
}

/// 加载窗口图标（app-icon.png）。
fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("app-icon.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(_) => egui::IconData::default(),
    }
}

impl AppState {
    #[allow(clippy::field_reassign_with_default)] // worker channel 在 default 之后创建
    fn boot(cc: &eframe::CreationContext<'_>) -> Self {
        // 中文字体加载 + 视觉样式
        let mut fonts = egui::FontDefinitions::default();
        collect_cjk_fonts(&mut fonts);
        cc.egui_ctx.set_fonts(fonts);
        theme::apply_visuals(&cc.egui_ctx);
        let mut app = Self::default();
        // 后台 worker channel（持久，所有后台任务共用）
        let (tx, rx) = std::sync::mpsc::channel();
        app.worker_tx = Some(tx);
        app.worker_rx = Some(rx);
        app.refresh_preset_combo();
        app.refresh_para_status();
        app
    }

    /// 标记参数/文本已变更：触发防抖自动预览。
    pub(crate) fn mark_changed(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    /// 在后台线程执行闭包，结果经持久 channel 回 UI，完成时 request_repaint 唤醒。
    fn spawn_worker<F>(&self, ctx: &egui::Context, f: F)
    where
        F: FnOnce() -> WorkerMsg + Send + 'static,
    {
        let Some(tx) = &self.worker_tx else { return };
        let tx = tx.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx2.request_repaint();
        });
    }

    /// 分发后台 worker 消息（在 logic 里轮询后调用）。
    fn handle_worker_msg(&mut self, msg: WorkerMsg, ctx: &egui::Context) {
        match msg {
            WorkerMsg::RenderDone(Ok(pages)) => {
                self.preview_pages = pages;
                self.preview_index = 0;
                self.preview_texture = None; // 触发纹理重建
                self.show_page();
                self.rendering = false;
                self.status = format!("预览完成（seed={}），共 {} 页", self.seed, self.preview_pages.len());
            }
            WorkerMsg::RenderDone(Err(e)) => {
                self.rendering = false;
                self.status = format!("渲染失败：{e}");
            }
            WorkerMsg::ExportDone(Ok(files)) => {
                self.status = format!("已导出 {} 个文件", files.len());
            }
            WorkerMsg::ExportDone(Err(e)) => self.status = format!("导出失败：{e}"),
            WorkerMsg::PdfDone(Ok(())) => self.status = "PDF 已导出".to_string(),
            WorkerMsg::PdfDone(Err(e)) => self.status = format!("导出 PDF 失败：{e}"),
            WorkerMsg::FontPicked(Some(p)) => {
                self.ui.font_path = p.to_string_lossy().into_owned();
                self.load_handwrite_font(ctx);
                self.mark_changed();
            }
            WorkerMsg::FontPicked(None) => {}
            WorkerMsg::BackgroundPicked(Some(p)) => {
                self.ui.background_path = p.to_string_lossy().into_owned();
                self.mark_changed();
            }
            WorkerMsg::BackgroundPicked(None) => {}
            WorkerMsg::DocxPicked(Some(p)) => self.import_docx(p),
            WorkerMsg::DocxPicked(None) => {}
            WorkerMsg::PresetPicked(Some(p)) => self.load_preset_from(p),
            WorkerMsg::PresetPicked(None) => {}
            WorkerMsg::PresetSavePath(Some(p)) => self.do_save_preset(p),
            WorkerMsg::PresetSavePath(None) => self.pending_save = None,
            WorkerMsg::ExportDirPicked(Some(d)) => self.do_export(d, ctx),
            WorkerMsg::ExportDirPicked(None) => {}
            WorkerMsg::PdfPathPicked(Some(p)) => self.do_export_pdf(p, ctx),
            WorkerMsg::PdfPathPicked(None) => {}
        }
    }

    // ---- 防抖 / 自动渲染 ----

    /// 停止输入满 300ms 且无渲染进行中 → 后台线程渲染预览。
    fn maybe_render(&mut self, ctx: &egui::Context) {
        if !self.dirty || self.rendering {
            return;
        }
        if self.last_edit.elapsed() < std::time::Duration::from_millis(PREVIEW_DEBOUNCE_MS) {
            // 还没到时间 → 安排一次到点唤醒
            ctx.request_repaint_after(std::time::Duration::from_millis(PREVIEW_DEBOUNCE_MS));
            return;
        }
        self.dirty = false;
        let font_size = self.ui.font_size as f32;
        let (paras, has_format) =
            crate::ui::editor::paragraphs_from_editor(&self.editor, font_size);
        let params = match crate::ui::params::collect_params(
            &self.ui,
            self.preset_params.as_ref(),
            paras,
            has_format,
        ) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("参数错误：{e}");
                return;
            }
        };
        let bounds_visible = self.ui.bounds_visible;
        let bounds_color =
            crate::core::models::parse_color(&self.ui.bounds_color).unwrap_or([76, 166, 166]);
        self.seed += 1;
        let seed = self.seed;
        self.rendering = true;
        self.status = "渲染中…".to_string();
        self.spawn_worker(ctx, move || {
            let r = (|| -> Result<Vec<RgbaImage>, String> {
                let mut pages = crate::core::engine::render_all_pages_preview(&params, seed)
                    .map_err(|e| e.to_string())?;
                if bounds_visible {
                    for page in pages.iter_mut() {
                        *page = crate::core::engine::overlay_bounds(page, &params, bounds_color);
                    }
                }
                Ok(pages)
            })();
            WorkerMsg::RenderDone(r)
        });
    }

    /// 立即触发渲染（不等防抖）。
    fn regenerate(&mut self) {
        self.mark_changed();
        self.last_edit =
            Instant::now() - std::time::Duration::from_millis(PREVIEW_DEBOUNCE_MS);
    }

    // ---- 文件对话框（rfd 阻塞调用走后台线程）----

    fn pick_font(&self, ctx: &egui::Context) {
        self.spawn_worker(ctx, || {
            WorkerMsg::FontPicked(
                rfd::FileDialog::new()
                    .add_filter("字体文件", &["ttf", "ttc", "otf"])
                    .pick_file(),
            )
        });
    }

    fn pick_background(&self, ctx: &egui::Context) {
        self.spawn_worker(ctx, || {
            WorkerMsg::BackgroundPicked(
                rfd::FileDialog::new()
                    .add_filter("图片", &["png", "jpg", "jpeg", "webp", "bmp"])
                    .pick_file(),
            )
        });
    }

    fn pick_docx(&self, ctx: &egui::Context) {
        self.spawn_worker(ctx, || {
            WorkerMsg::DocxPicked(
                rfd::FileDialog::new().add_filter("Word 文档", &["docx"]).pick_file(),
            )
        });
    }

    fn pick_preset_file(&self, ctx: &egui::Context) {
        self.spawn_worker(ctx, || {
            WorkerMsg::PresetPicked(
                rfd::FileDialog::new().add_filter("预设", &["json"]).pick_file(),
            )
        })
    }

    fn pick_export_dir(&self, ctx: &egui::Context) {
        self.spawn_worker(ctx, || WorkerMsg::ExportDirPicked(rfd::FileDialog::new().pick_folder()))
    }

    fn pick_pdf_path(&self, ctx: &egui::Context) {
        self.spawn_worker(ctx, || {
            WorkerMsg::PdfPathPicked(
                rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf"])
                    .set_file_name("handwrite.pdf")
                    .save_file(),
            )
        })
    }

    // ---- 预设 / docx / 导出 / 字体 ----

    fn load_preset_from(&mut self, path: PathBuf) {
        match crate::core::presets::load(&path) {
            Ok(p) => {
                self.apply_preset_params(&p);
                self.status = "预设已载入（含边距/扰动参数）".to_string();
            }
            Err(e) => self.status = format!("载入失败：{e}"),
        }
    }

    fn save_preset(&mut self, ctx: &egui::Context) {
        let font_size = self.ui.font_size as f32;
        let (paras, has_format) =
            crate::ui::editor::paragraphs_from_editor(&self.editor, font_size);
        match crate::ui::params::collect_params(&self.ui, self.preset_params.as_ref(), paras, has_format) {
            Ok(params) => {
                self.pending_save = Some(params);
                let dir = crate::core::presets::assets_root().join("presets");
                self.spawn_worker(ctx, move || {
                    WorkerMsg::PresetSavePath(
                        rfd::FileDialog::new()
                            .add_filter("预设", &["json"])
                            .set_directory(dir)
                            .set_file_name("preset.json")
                            .save_file(),
                    )
                });
            }
            Err(e) => self.status = format!("参数错误：{e}"),
        }
    }

    fn do_save_preset(&mut self, path: PathBuf) {
        if let Some(params) = self.pending_save.take() {
            match crate::core::presets::save(&params, &path) {
                Ok(()) => {
                    self.status = format!("预设已保存：{}", path.display());
                    if path.starts_with(crate::core::presets::assets_root().join("presets")) {
                        self.refresh_preset_combo();
                    }
                }
                Err(e) => self.status = format!("保存失败：{e}"),
            }
        }
    }

    fn import_docx(&mut self, path: PathBuf) {
        let font_size = self.ui.font_size as f32;
        match crate::core::docx_io::load_paragraphs(&path, font_size) {
            Ok(paras) => {
                use crate::core::models::Align;
                let text = paras.iter().map(|p| p.text.clone()).collect::<Vec<_>>().join("\n");
                let formats = paras
                    .iter()
                    .map(|p| crate::ui::editor::ParaFormat {
                        align: match p.align {
                            Align::Center => 1,
                            Align::Right => 2,
                            _ => 0,
                        },
                        indent_em: if font_size > 0.0 { p.first_line_indent / font_size } else { 0.0 },
                    })
                    .collect();
                self.editor.set_text(&text, formats);
                self.refresh_para_status();
                self.mark_changed();
                self.status = format!("已导入 {} 个段落，回车分段、按钮设格式", paras.len());
            }
            Err(e) => self.status = format!("导入 docx 失败：{e}"),
        }
    }

    fn export_files(&mut self, ctx: &egui::Context) {
        self.pick_export_dir(ctx);
    }

    fn do_export(&mut self, dir: PathBuf, ctx: &egui::Context) {
        let font_size = self.ui.font_size as f32;
        let (paras, has_format) =
            crate::ui::editor::paragraphs_from_editor(&self.editor, font_size);
        let params =
            match crate::ui::params::collect_params(&self.ui, self.preset_params.as_ref(), paras, has_format) {
                Ok(p) => p,
                Err(e) => {
                    self.status = format!("参数错误：{e}");
                    return;
                }
            };
        let seed = self.seed;
        self.status = "导出中…".to_string();
        self.spawn_worker(ctx, move || {
            WorkerMsg::ExportDone(crate::core::engine::export(&params, &dir, seed).map_err(|e| e.to_string()))
        });
    }

    fn export_pdf(&mut self, ctx: &egui::Context) {
        self.pick_pdf_path(ctx);
    }

    fn do_export_pdf(&mut self, path: PathBuf, ctx: &egui::Context) {
        let font_size = self.ui.font_size as f32;
        let (paras, has_format) =
            crate::ui::editor::paragraphs_from_editor(&self.editor, font_size);
        let params =
            match crate::ui::params::collect_params(&self.ui, self.preset_params.as_ref(), paras, has_format) {
                Ok(p) => p,
                Err(e) => {
                    self.status = format!("参数错误：{e}");
                    return;
                }
            };
        let seed = self.seed;
        self.status = "导出中…".to_string();
        self.spawn_worker(ctx, move || {
            WorkerMsg::PdfDone(crate::core::engine::export_pdf(&params, &path, seed).map_err(|e| e.to_string()))
        });
    }

    /// 选了手写字体后，把字体 bytes 加入 FontDefinitions（编辑器观感与渲染同源）。
    fn load_handwrite_font(&mut self, ctx: &egui::Context) {
        if self.ui.font_path.is_empty() {
            return;
        }
        if let Ok(bytes) = std::fs::read(&self.ui.font_path) {
            let mut fonts = egui::FontDefinitions::default();
            collect_cjk_fonts(&mut fonts);
            fonts
                .font_data
                .insert("handwrite".to_string(), Arc::new(egui::FontData::from_owned(bytes)));
            if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                fam.insert(0, "handwrite".to_string());
            }
            ctx.set_fonts(fonts);
        }
    }
}

/// 把系统 CJK 字体加入给定 FontDefinitions（找不到则跳过，不 panic）。
fn collect_cjk_fonts(fonts: &mut egui::FontDefinitions) {
    let candidates: &[&str] = &[
        "C:\\Windows\\Fonts\\msyh.ttc", // Windows 雅黑
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "/System/Library/Fonts/PingFang.ttc", // macOS
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("system-chinese".to_string(), Arc::new(egui::FontData::from_owned(bytes)));
            // 把 system-chinese 插入 Proportional 回退链最前
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "system-chinese".to_string());
            }
            break; // 用第一个找到的
        }
    }
}

impl eframe::App for AppState {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. 轮询后台 worker 结果（把 rx 取出到局部，避免与 handle_worker_msg 的 &mut self 冲突）
        if let Some(rx) = self.worker_rx.take() {
            loop {
                match rx.try_recv() {
                    Ok(msg) => self.handle_worker_msg(msg, ctx),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
            self.worker_rx = Some(rx);
        }
        // 2. 防抖：停止输入满 300ms 且无渲染中 → 后台渲染
        self.maybe_render(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        theme::apply_visuals(ui.ctx()); // 每帧确保样式（开销极小）

        egui::Frame::central_panel(ui.style())
            .fill(theme::BG)
            .inner_margin(8.0)
            .show(ui, |ui| {
                let total_w = ui.available_width();
                let total_h = ui.available_height();
                let status_h = 20.0;
                let main_h = (total_h - status_h - 4.0).max(120.0);
                let left_w = (total_w - PANEL_WIDTH - 8.0).max(120.0);
                ui.horizontal(|ui| {
                    // ---- 左侧：预览区（弹性）+ 翻页 ----
                    ui.allocate_ui(egui::vec2(left_w, main_h), |ui| {
                        let paging_h = 28.0;
                        ui.vertical(|ui| {
                            self.preview_area(ui, main_h - paging_h);
                            ui.horizontal(|ui| {
                                if ui.add(theme::green_button("◀ 上一页")).clicked()
                                    && self.preview_index > 0
                                {
                                    self.preview_index -= 1;
                                    self.show_page();
                                }
                                ui.label(self.page_text.as_str());
                                if ui.add(theme::green_button("下一页 ▶")).clicked()
                                    && self.preview_index + 1 < self.preview_pages.len()
                                {
                                    self.preview_index += 1;
                                    self.show_page();
                                }
                                if ui.add(theme::green_button("预览底色")).clicked() {
                                    self.preview_bg_idx =
                                        (self.preview_bg_idx + 1) % PREVIEW_BG_COLORS.len();
                                }
                            });
                        });
                    });
                    // ---- 右侧：参数面板（460）----
                    ui.allocate_ui(egui::vec2(PANEL_WIDTH, main_h), |ui| {
                        ui.vertical(|ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("param_scroll")
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    crate::ui::controls::section_label(ui, "待处理文本");
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.add(theme::green_button("左对齐")).clicked() {
                                            self.editor.set_align(0);
                                            self.mark_changed();
                                            self.refresh_para_status();
                                        }
                                        if ui.add(theme::green_button("居中")).clicked() {
                                            self.editor.set_align(1);
                                            self.mark_changed();
                                            self.refresh_para_status();
                                        }
                                        if ui.add(theme::green_button("右对齐")).clicked() {
                                            self.editor.set_align(2);
                                            self.mark_changed();
                                            self.refresh_para_status();
                                        }
                                        if ui.add(theme::green_button("首行缩进")).clicked() {
                                            self.editor.toggle_indent(true);
                                            self.mark_changed();
                                            self.refresh_para_status();
                                        }
                                        if ui.add(theme::green_button("取消缩进")).clicked() {
                                            self.editor.toggle_indent(false);
                                            self.mark_changed();
                                            self.refresh_para_status();
                                        }
                                        if ui.add(theme::green_button("导入docx")).clicked() {
                                            self.pick_docx(ui.ctx());
                                        }
                                    });
                                    crate::ui::controls::hint_label(ui, self.para_status.as_str());
                                    self.editor_view(ui);
                                    self.param_panel(ui);
                                });
                            // 主按钮（不随面板滚动）
                            ui.horizontal(|ui| {
                                if ui.add(theme::primary_button("预览")).clicked() {
                                    self.regenerate();
                                }
                                if ui.add(theme::primary_button("导出")).clicked() {
                                    self.export_files(ui.ctx());
                                }
                                if ui.add(theme::primary_button("导出 PDF")).clicked() {
                                    self.export_pdf(ui.ctx());
                                }
                            });
                        });
                    });
                });
                ui.add_space(4.0);
                // ---- 底部状态栏 ----
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(self.status.as_str())
                            .color(theme::SUB_TEXT)
                            .size(12.0),
                    );
                });
            });
    }
}

impl AppState {
    /// 预览区：底色框 + 预览图（contain 居中，TextureHandle 缓存，翻页/新结果才重建）。
    /// `height` 由调用方计算（= 主区域高度 - 翻页行高），避免占用全部高度挤掉其他行。
    fn preview_area(&mut self, ui: &mut egui::Ui, height: f32) {
        let bg = PREVIEW_BG_COLORS[self.preview_bg_idx];
        let size = egui::vec2(ui.available_width(), height.max(40.0));
        let (rect, _resp) = ui.allocate_exact_size(size, egui::Sense::click());
        ui.painter().rect(
            rect,
            6.0,
            egui::Color32::from_rgb(bg[0], bg[1], bg[2]),
            egui::Stroke::new(1.0, theme::GROUP_BORDER),
            egui::StrokeKind::Inside,
        );

        // 纹理为空时重建（翻页 / 新渲染结果时 preview_texture 被置 None）
        if self.preview_texture.is_none() {
            let new_tex = self.preview_pages.get(self.preview_index).map(|img| {
                let (w, h) = img.dimensions();
                let ci = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    img.as_raw(),
                );
                ui.ctx()
                    .load_texture("preview_page", ci, egui::TextureOptions::LINEAR)
            });
            if let Some(tex) = new_tex {
                self.preview_texture = Some(tex);
            }
        }

        // contain 居中缩放绘制
        let dims = self
            .preview_pages
            .get(self.preview_index)
            .map(|img| img.dimensions());
        let tex_id = self.preview_texture.as_ref().map(|t| t.id());
        if let (Some((w, h)), Some(tex_id)) = (dims, tex_id) {
            let scale = (rect.width() / w as f32)
                .min(rect.height() / h as f32)
                .min(1.0);
            let draw_w = w as f32 * scale;
            let draw_h = h as f32 * scale;
            let pos = egui::Pos2::new(rect.center().x - draw_w / 2.0, rect.center().y - draw_h / 2.0);
            let draw_rect = egui::Rect::from_min_size(pos, egui::vec2(draw_w, draw_h));
            ui.painter().image(
                tex_id,
                draw_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    /// 更新页码文本 + 标记纹理重建。
    fn show_page(&mut self) {
        if self.preview_pages.is_empty() {
            self.page_text = "第 1 / 1 页".to_string();
            return;
        }
        let total = self.preview_pages.len();
        let i = self.preview_index.min(total - 1);
        self.page_text = format!("第 {} / {} 页", i + 1, total);
        self.preview_texture = None; // 翻页/新结果 → 下一帧重建纹理
    }

    /// 段落编辑器：白底圆角框内，每段一个 `TextEdit::multiline`。
    /// `horizontal_align` + `desired_width(INFINITY)` → wrap 换行后每行真对齐
    /// （epaint `halign_and_justify_row`），无估宽 hack。
    /// 交互（回车/段首退格/粘贴）在本帧收集为动作，帧末统一应用，避免渲染迭代中
    /// 改 vec 触发借用冲突。
    fn editor_view(&mut self, ui: &mut egui::Ui) {
        let n = self.editor.paras.len();
        let backspace_pressed = ui.ctx().input(|i| i.key_pressed(egui::Key::Backspace));
        let mut pending_focus = self.pending_focus.take();
        let mut actions: Vec<ParaAction> = Vec::new();
        let mut changed_para: Option<usize> = None;
        let mut focus_changed_para: Option<usize> = None;

        egui::Frame::group(ui.style())
            .fill(theme::EDITOR_BG)
            .stroke(egui::Stroke::new(1.0, theme::GROUP_BORDER))
            .corner_radius(6.0)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for i in 0..n {
                    let (align, indent_em) = {
                        let p = &self.editor.paras[i];
                        (p.format.align, p.format.indent_em)
                    };
                    ui.horizontal(|ui| {
                        if indent_em > 0.0 {
                            ui.add_space(indent_em * EDITOR_FONT_SIZE);
                        }
                        let halign = match align {
                            1 => egui::Align::Center,
                            2 => egui::Align::RIGHT,
                            _ => egui::Align::LEFT,
                        };
                        let want_focus = pending_focus == Some(i);
                        // TextEdit 借用 &mut para.text；放进内层块让借用在 .show 后立即释放，
                        // 之后再读取 para.text 判定换行就不会冲突。
                        let output = {
                            let text: &mut String = &mut self.editor.paras[i].text;
                            egui::TextEdit::multiline(text)
                                .id(egui::Id::new(format!("para-{i}")))
                                .desired_width(f32::INFINITY)
                                .desired_rows(1)
                                .horizontal_align(halign)
                                .hint_text("请输入文本内容…")
                                .show(ui)
                        };
                        if want_focus {
                            output.response.request_focus();
                            pending_focus = None;
                        }
                        if output.response.changed() {
                            if self.editor.paras[i].text.contains('\n') {
                                actions.push(ParaAction::SplitNewlines(i));
                            } else {
                                changed_para = Some(i);
                            }
                        } else if output.response.has_focus() {
                            focus_changed_para = Some(i);
                        }
                        // 段首退格：光标在段首（无选区）+ 本帧 Backspace → 并入上一段
                        if backspace_pressed && output.response.has_focus() && i > 0 {
                            if let Some(cr) = &output.cursor_range {
                                if cr.primary.index.0 == 0 && cr.secondary.index.0 == 0 {
                                    actions.push(ParaAction::MergePrev(i));
                                }
                            }
                        }
                    });
                }
                // 底部空白：点击聚焦最后一段（模拟整框编辑手感）
                let (_rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 30.0),
                    egui::Sense::click(),
                );
                if resp.clicked() {
                    actions.push(ParaAction::FocusLast);
                }
            });

        // 帧末统一应用动作（render 闭包借用已释放）
        let mut status_dirty = false;
        for a in actions {
            match a {
                ParaAction::SplitNewlines(i) => {
                    if let Some(last) = self.editor.split_para_at_newlines(i) {
                        self.pending_focus = Some(last);
                        self.mark_changed();
                        status_dirty = true;
                    }
                }
                ParaAction::MergePrev(i) => {
                    if let Some(t) = self.editor.merge_prev(i) {
                        self.pending_focus = Some(t);
                        self.mark_changed();
                        status_dirty = true;
                    }
                }
                ParaAction::FocusLast => {
                    let last = self.editor.paras.len().saturating_sub(1);
                    self.editor.current = last;
                    self.pending_focus = Some(last);
                    status_dirty = true;
                }
            }
        }
        if let Some(i) = changed_para {
            self.editor.current = i;
            self.mark_changed();
            status_dirty = true;
        }
        if let Some(i) = focus_changed_para {
            self.editor.current = i;
            status_dirty = true;
        }
        if status_dirty {
            self.refresh_para_status();
        }
    }

    /// 右侧参数面板（全部数值控件；按钮类动作在阶段 3 接入后台线程）。
    fn param_panel(&mut self, ui: &mut egui::Ui) {
        use crate::ui::controls::{field_label, group_box, hint_label, num_field};
        let mut changed = false;

        // ---- 字体 / 背景（路径可直接编辑 + 「选择」对话框）----
        ui.horizontal(|ui| {
            field_label(ui, "字体");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.ui.font_path)
                        .desired_width(180.0)
                        .hint_text("未选择字体"),
                )
                .changed();
            if ui.add(theme::green_button("选择")).clicked() {
                self.pick_font(ui.ctx());
            }
        });
        ui.horizontal(|ui| {
            field_label(ui, "背景");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.ui.background_path)
                        .desired_width(180.0)
                        .hint_text("未选择背景"),
                )
                .changed();
            if ui.add(theme::green_button("选择")).clicked() {
                self.pick_background(ui.ctx());
            }
        });
        // ---- 文字颜色 ----
        ui.horizontal(|ui| {
            field_label(ui, "文字颜色");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.ui.font_color)
                        .desired_width(96.0)
                        .hint_text("#000000"),
                )
                .changed();
        });
        // ---- 预设（下拉直接载入；载入/保存文件按钮阶段 3）----
        ui.horizontal(|ui| {
            field_label(ui, "预设");
            let chosen = self
                .preset_chosen
                .clone()
                .unwrap_or_else(|| PRESET_PLACEHOLDER.to_string());
            let names: Vec<String> = self.preset_names.clone();
            egui::ComboBox::from_id_salt("preset_combo")
                .selected_text(chosen.as_str())
                .show_ui(ui, |ui| {
                    for name in &names {
                        if ui.selectable_label(name.as_str() == chosen.as_str(), name.as_str()).clicked() {
                            self.select_preset(name.clone());
                        }
                    }
                });
            if ui.add(theme::green_button("载入预设")).clicked() {
                self.pick_preset_file(ui.ctx());
            }
            if ui.add(theme::green_button("保存预设")).clicked() {
                self.save_preset(ui.ctx());
            }
        });

        // ---- 排版参数 ----
        group_box(ui, "排版参数", |ui| {
            ui.horizontal(|ui| {
                ui.add_space(86.0);
                ui.label("数值");
                ui.add_space(40.0);
                ui.label("扰动 σ");
            });
            changed |= param_row(ui, "字水平间距", &mut self.ui.word_spacing, 0, 100, &mut self.ui.word_spacing_sigma, 0, 20);
            changed |= param_row(ui, "字竖直间距", &mut self.ui.line_spacing, 0, 200, &mut self.ui.line_spacing_sigma, 0, 20);
            changed |= param_row(ui, "字体大小", &mut self.ui.font_size, 8, 200, &mut self.ui.font_size_sigma, 0, 20);
        });
        // ---- 笔画扰动 ----
        group_box(ui, "笔画扰动", |ui| {
            ui.horizontal(|ui| {
                field_label(ui, "水平笔画位移");
                changed |= num_field(ui, &mut self.ui.perturb_x, 0, 20).changed();
            });
            ui.horizontal(|ui| {
                field_label(ui, "竖直笔画位移");
                changed |= num_field(ui, &mut self.ui.perturb_y, 0, 20).changed();
            });
            ui.horizontal(|ui| {
                field_label(ui, "笔画旋转");
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.ui.perturb_theta)
                            .desired_width(70.0)
                            .hint_text("0.05"),
                    )
                    .changed();
            });
        });
        // ---- 写错字 ----
        group_box(ui, "写错字", |ui| {
            ui.horizontal(|ui| {
                field_label(ui, "错字率");
                changed |= ui
                    .add(egui::Slider::new(&mut self.ui.miswrite_rate, 0.0..=30.0))
                    .changed();
                ui.label(format!("{:.1}%", self.ui.miswrite_rate));
            });
            ui.horizontal(|ui| {
                field_label(ui, "重写方式");
                let modes = ["右上方重写", "后文重写"];
                let cur = self.ui.miswrite_mode.clamp(0, 1) as usize;
                egui::ComboBox::from_id_salt("miswrite_mode")
                    .selected_text(modes[cur])
                    .show_ui(ui, |ui| {
                        for (i, m) in modes.iter().enumerate() {
                            if ui.selectable_label(i == cur, *m).clicked() {
                                self.ui.miswrite_mode = i as i32;
                                changed = true;
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                field_label(ui, "涂改方式");
                let sts = ["单横线", "双横线", "斜线", "叉号"];
                let cur = self.ui.miswrite_strikeout.clamp(0, 3) as usize;
                egui::ComboBox::from_id_salt("miswrite_strike")
                    .selected_text(sts[cur])
                    .show_ui(ui, |ui| {
                        for (i, s) in sts.iter().enumerate() {
                            if ui.selectable_label(i == cur, *s).clicked() {
                                self.ui.miswrite_strikeout = i as i32;
                                changed = true;
                            }
                        }
                    });
            });
        });
        // ---- 边距 ----
        group_box(ui, "边距", |ui| {
            ui.horizontal(|ui| {
                ui.add_space(120.0);
                changed |= num_field(ui, &mut self.ui.margin_top, 0, 3000).changed();
            });
            ui.horizontal(|ui| {
                ui.add_space(60.0);
                changed |= num_field(ui, &mut self.ui.margin_left, 0, 3000).changed();
                ui.label("边距");
                changed |= num_field(ui, &mut self.ui.margin_right, 0, 3000).changed();
            });
            ui.horizontal(|ui| {
                ui.add_space(120.0);
                changed |= num_field(ui, &mut self.ui.margin_bottom, 0, 3000).changed();
            });
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut self.ui.bounds_visible, "边界提示(仅预览)")
                    .changed();
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.ui.bounds_color)
                            .desired_width(96.0)
                            .hint_text("#4ca6a6"),
                    )
                    .changed();
            });
        });
        hint_label(ui, "提示：选择字体与背景后自动预览，也可点击「预览」立即渲染");

        if changed {
            self.mark_changed();
        }
    }

    /// 扫描 exe 旁 presets/ 目录，刷新预设下拉（0 为占位符）。
    fn refresh_preset_combo(&mut self) {
        self.preset_names = vec![PRESET_PLACEHOLDER.to_string()];
        self.preset_paths.clear();
        let preset_dir = crate::core::presets::assets_root().join("presets");
        if let Ok(rd) = std::fs::read_dir(&preset_dir) {
            let mut files: Vec<PathBuf> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().map(|e| e == "json").unwrap_or(false))
                .collect();
            files.sort();
            for f in files {
                if let Some(stem) = f.file_stem().map(|s| s.to_string_lossy().into_owned()) {
                    self.preset_names.push(stem);
                    self.preset_paths.push(f);
                }
            }
        }
    }

    /// 下拉选中预设：从 presets/ 目录载入（同步，不阻塞）。
    fn select_preset(&mut self, name: String) {
        if name == PRESET_PLACEHOLDER {
            self.preset_chosen = None;
            return;
        }
        let idx = self
            .preset_names
            .iter()
            .position(|n| *n == name)
            .and_then(|i| i.checked_sub(1));
        let Some(idx) = idx else {
            return;
        };
        let Some(path) = self.preset_paths.get(idx).cloned() else {
            return;
        };
        match crate::core::presets::load(&path) {
            Ok(p) => {
                self.preset_chosen = Some(name);
                self.apply_preset_params(&p);
                self.status = format!("已载入预设：{}", path.display());
            }
            Err(e) => self.status = format!("载入失败：{e}"),
        }
    }

    /// 把预设参数回填 UI（保留边界提示开关/颜色，对齐 iced 版行为）。
    fn apply_preset_params(&mut self, p: &HandwritingParams) {
        let bounds_visible = self.ui.bounds_visible;
        let bounds_color = self.ui.bounds_color.clone();
        self.ui = crate::ui::params::apply_preset(p);
        self.ui.bounds_visible = bounds_visible;
        self.ui.bounds_color = bounds_color;
        self.preset_params = Some(p.clone());
        self.mark_changed();
    }

    /// 当前段格式提示（对齐 iced 版 `refresh_para_status`）。
    fn refresh_para_status(&mut self) {
        let idx = self.editor.cursor_paragraph();
        self.refresh_para_status_after(idx);
    }

    fn refresh_para_status_after(&mut self, idx: usize) {
        let line_text = self
            .editor
            .paras
            .get(idx)
            .map(|p| p.text.as_str())
            .unwrap_or("");
        let align_name = ["左对齐", "居中", "右对齐"];
        let fmt = self.editor.current_format();
        let indent_txt = match fmt {
            Some(f) if f.indent_em > 0.0 => {
                let em = f.indent_em;
                let px = (em * self.ui.font_size as f32).round() as i32;
                let em_txt = if (em - em.round()).abs() < 0.01 {
                    format!("{}", em.round() as i32)
                } else {
                    format!("{em:.1}")
                };
                format!("，首行缩进 {em_txt} 字（{px}px）")
            }
            _ => String::new(),
        };
        let align = match fmt {
            Some(f) => align_name[(f.align.clamp(0, 2)) as usize],
            None => "左对齐",
        };
        let seg_txt = if line_text.trim().is_empty() {
            "（空段）"
        } else {
            ""
        };
        let count = line_text
            .replace('\u{2060}', "")
            .replace(['\u{00a0}', '\u{ffa0}'], " ")
            .chars()
            .count();
        self.para_status = format!("第 {} 段（{} 字）：{align}{indent_txt}{seg_txt}", idx + 1, count);
    }
}

/// 「标签 | 数值 | σ | 数值」参数行（排版参数表格用），返回是否有控件变化。
#[allow(clippy::too_many_arguments)] // 表格行的自然参数数
fn param_row(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut i32,
    vmin: i32,
    vmax: i32,
    s: &mut i32,
    smin: i32,
    smax: i32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        crate::ui::controls::field_label(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed |= crate::ui::controls::num_field(ui, s, smin, smax).changed();
            ui.label("σ");
            changed |= crate::ui::controls::num_field(ui, v, vmin, vmax).changed();
        });
    });
    changed
}
