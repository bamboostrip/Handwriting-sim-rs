//! 手写模拟器主题：配色对齐 Python 版 gui/ui.py 的 `_LIGHT_QSS`（与 iced 版
//! theme.rs 一一对应），egui 版用 Color32 常量 + 按钮样式 helper 表达。

use egui::{Color32, Stroke, Vec2, Visuals};

// 配色（RGB 逐一对应 iced 版 theme.rs）
/// 窗口背景（QSS: QMainWindow #f4f7f4）
pub const BG: Color32 = Color32::from_rgb(0xf4, 0xf7, 0xf4);
/// 主文字（#2b3430）
pub const TEXT: Color32 = Color32::from_rgb(0x2b, 0x34, 0x30);
/// 次要文字（#7d8a82）
pub const SUB_TEXT: Color32 = Color32::from_rgb(0x7d, 0x8a, 0x82);
/// 分组框边框（#d3ded6）
pub const GROUP_BORDER: Color32 = Color32::from_rgb(0xd3, 0xde, 0xd6);
/// 编辑区白底
pub const EDITOR_BG: Color32 = Color32::WHITE;
/// 普通按钮（#dcf7e6 / 边框 #b7e4c9）
pub const BTN_BG: Color32 = Color32::from_rgb(0xdc, 0xf7, 0xe6);
pub const BTN_BORDER: Color32 = Color32::from_rgb(0xb7, 0xe4, 0xc9);
pub const BTN_HOVER: Color32 = Color32::from_rgb(0xc9, 0xf0, 0xd8);
pub const BTN_PRESSED: Color32 = Color32::from_rgb(0xb2, 0xe5, 0xc4);
/// 主按钮（#9ddc80 / 边框 #7fc465）
pub const PRIMARY_BG: Color32 = Color32::from_rgb(0x9d, 0xdc, 0x80);
pub const PRIMARY_BORDER: Color32 = Color32::from_rgb(0x7f, 0xc4, 0x65);
pub const PRIMARY_HOVER: Color32 = Color32::from_rgb(0x8e, 0xd2, 0x71);
pub const PRIMARY_PRESSED: Color32 = Color32::from_rgb(0x7f, 0xbf, 0x63);

/// 应用全局视觉：亮色主题、配色、控件圆角、间距。
/// egui 0.36：visuals 走 set_visuals；style（间距）走 style_mut_of（按当前 Theme）。
pub fn apply_visuals(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Light);
    let mut v = Visuals::light();
    v.widgets.noninteractive.bg_fill = BG;
    v.panel_fill = BG;
    v.window_fill = BG;
    v.selection.bg_fill = PRIMARY_BG;
    v.selection.stroke = Stroke::new(1.0, PRIMARY_BORDER);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = 5.0.into();
        w.fg_stroke = Stroke::new(1.0, TEXT);
    }
    ctx.set_visuals(v);
    let theme = ctx.theme();
    ctx.style_mut_of(theme, |s| {
        s.spacing.button_padding = Vec2::new(8.0, 3.0);
        s.spacing.item_spacing = Vec2::new(6.0, 5.0);
    });
}

/// 普通绿色按钮（对应 GreenButton）。hover/press 由 egui 默认交互态自动加亮。
pub fn green_button(label: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(label)
        .fill(BTN_BG)
        .stroke(Stroke::new(1.0, BTN_BORDER))
        .corner_radius(4.0)
}

/// 主按钮（对应 PrimaryButton，用于「预览/导出」）。
pub fn primary_button(label: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(label)
        .fill(PRIMARY_BG)
        .stroke(Stroke::new(1.0, PRIMARY_BORDER))
        .corner_radius(4.0)
}
