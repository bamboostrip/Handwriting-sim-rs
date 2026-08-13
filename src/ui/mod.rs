//! egui 桌面界面。
//!
//! 分层：
//! - `app`：应用装配（AppState 状态 + eframe::App::update 帧循环）
//! - `editor`：段落编辑器（每段一个 TextEdit，纯 String 模型 + 拆分/合并纯函数）
//! - `params`：UI 参数状态与「收集/回填」纯函数（可单测，零改动复用）
//! - `controls`：自定义控件（DragValue 数值输入、分组框等）
//! - `theme`：配色常量与视觉样式

pub mod app;
pub mod controls;
pub mod editor;
pub mod params;
pub mod theme;
