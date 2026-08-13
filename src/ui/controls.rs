//! 自定义控件（egui 版）：数值输入（DragValue）、分组框、标签。
//!
//! 对照 iced 版 controls.rs：
//! - SpinBox（文本框 + ± 按钮）→ DragValue（自带拖拽/键盘/滚轮/clamp）
//! - container 分组框 → egui Frame::group

use egui::{Frame, RichText, Stroke, Ui};

use crate::ui::theme;

/// 数值输入：DragValue，范围 clamp（自带拖拽/键盘/滚轮）。
pub fn num_field(ui: &mut Ui, value: &mut i32, min: i32, max: i32) -> egui::Response {
    ui.add(
        egui::DragValue::new(value)
            .range(min..=max)
            .clamp_existing_to_range(true)
            .speed(0.2),
    )
}

/// 分组框：带边框 + 标题（对齐 Python 版 QGroupBox 视觉）。
pub fn group_box<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).size(13.0).color(theme::SUB_TEXT));
        Frame::group(ui.style())
            .fill(theme::BG)
            .stroke(Stroke::new(1.0, theme::GROUP_BORDER))
            .corner_radius(6.0)
            .inner_margin(8.0)
            .show(ui, add_contents)
            .inner
    })
    .inner
}

/// 字段标签（行内）。
pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(theme::TEXT));
}

/// 小节标题。
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(theme::TEXT));
}

/// 状态栏小字提示。
pub fn hint_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(theme::SUB_TEXT));
}
