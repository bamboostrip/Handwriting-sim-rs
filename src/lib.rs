//! 手写模拟器库入口。
//!
//! 分层：
//! - `core`：渲染引擎（模型 / 字体 / 排版 / 笔画扰动），纯 Rust，无 GUI 依赖，
//!   可被 GUI、CLI 与测试复用。
//! - `ui`：egui 桌面界面（app / editor / params / controls / theme）。

pub mod core;
pub mod ui;