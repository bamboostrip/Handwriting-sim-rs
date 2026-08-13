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

/// 后台线程 → UI 的消息（mpsc channel）。阶段 3 扩展具体变体。
enum WorkerMsg {
    RenderDone(Result<Vec<RgbaImage>, String>),
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
    /// 后台渲染结果接收端。
    render_rx: Option<std::sync::mpsc::Receiver<WorkerMsg>>,
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
            render_rx: None,
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
    let mut opts = eframe::NativeOptions::default();
    opts.viewport = viewport;
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
    fn boot(cc: &eframe::CreationContext<'_>) -> Self {
        // 中文字体加载 + 视觉样式
        let mut fonts = egui::FontDefinitions::default();
        collect_cjk_fonts(&mut fonts);
        cc.egui_ctx.set_fonts(fonts);
        theme::apply_visuals(&cc.egui_ctx);
        let mut app = Self::default();
        app.refresh_preset_combo();
        app.refresh_para_status();
        app
    }

    /// 标记参数/文本已变更：触发防抖自动预览。
    pub(crate) fn mark_changed(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
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
        // 状态层：后台消息轮询、防抖渲染调度（阶段 3 填充）。
        let _ = ctx;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        theme::apply_visuals(ui.ctx()); // 每帧确保样式（开销极小）

        egui::Frame::central_panel(ui.style())
            .fill(theme::BG)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // ---- 左侧：预览区（弹性）+ 翻页 ----
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width() - PANEL_WIDTH - 8.0);
                        self.preview_area(ui);
                        ui.horizontal(|ui| {
                            ui.label(self.page_text.as_str());
                        });
                    });
                    // ---- 右侧：参数面板（460）----
                    ui.vertical(|ui| {
                        ui.set_width(PANEL_WIDTH);
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
                                });
                                crate::ui::controls::hint_label(ui, self.para_status.as_str());
                                self.editor_view(ui);
                                self.param_panel(ui);
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
    /// 预览区（阶段 0 占位灰框；阶段 3 实现真正 Image + 翻页按钮）。
    fn preview_area(&self, ui: &mut egui::Ui) {
        let bg = PREVIEW_BG_COLORS[self.preview_bg_idx];
        let (rect, _resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
        ui.painter().rect(
            rect,
            6.0,
            egui::Color32::from_rgb(bg[0], bg[1], bg[2]),
            egui::Stroke::new(1.0, theme::GROUP_BORDER),
            egui::StrokeKind::Inside,
        );
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

        // ---- 字体 / 背景（路径可直接编辑；「选择」按钮阶段 3）----
        ui.horizontal(|ui| {
            field_label(ui, "字体");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.ui.font_path)
                        .desired_width(220.0)
                        .hint_text("未选择字体"),
                )
                .changed();
        });
        ui.horizontal(|ui| {
            field_label(ui, "背景");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.ui.background_path)
                        .desired_width(220.0)
                        .hint_text("未选择背景"),
                )
                .changed();
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
