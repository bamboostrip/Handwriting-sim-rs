//! 手写模拟器库入口。
//!
//! 分层：
//! - `core`：渲染引擎（模型 / 字体 / 排版 / 笔画扰动 / 文档底图），纯 Rust，
//!   无 GUI 依赖，可被 Tauri 桌面端、CLI 与测试复用。
//!
//! GUI 已迁移为 Tauri 2 + Vue 3（桌面壳见 `src-tauri/`，前端在仓库根目录），
//! 原 Slint 界面自 feat/tauri2 分支起移除。

pub mod core;
