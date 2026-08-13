//! 手写模拟器桌面入口（egui 版）。
//!
//! 全部 UI 与业务逻辑位于 `ui::app::AppState`（实现 `eframe::App`），
//! 此处仅转发给 eframe 运行时。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    handwrite_sim::ui::app::run()
}
