//! 渲染引擎核心模块。

pub mod doc_render;
pub mod docx_io;
// pub(crate)：build_table_views 的入参 BodyElement 为 pub(crate) 类型（见 docx_io），
// 模块限定 crate 内可见以避免 pub API 泄漏 crate 私有类型。
pub(crate) mod docx_tables;
pub mod engine;
pub mod font;
pub mod layout;
pub mod models;
pub mod perturb;
pub mod presets;