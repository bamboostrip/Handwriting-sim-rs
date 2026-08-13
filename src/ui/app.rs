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
                        ui.label("（参数面板 - 阶段 2 实现）");
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
