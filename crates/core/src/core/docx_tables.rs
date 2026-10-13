//! docx 表格视图：从 body 元素流构造供匹配与区域生成使用的表格结构。
//!
//! 每个单元格得到：清理后的可见文本（{{...}}/【...】标签已剥离）、
//! 列起始/跨列、对齐、字号回退、印刷体判定与高亮色。
//! 嵌套表格的内容已在 docx_io 解析时丢弃（外层格文本不含内层表格）。

use crate::core::docx_io::{BodyElement, CellVMerge, ParsedCell};
use crate::core::doc_render::strip_tag_syntax;
use crate::core::docx_io::is_gray_highlight;

/// 一张表的视图（index 为文档内 1 基序号）。
#[derive(Debug, Clone)]
pub struct TableView {
    pub index: usize,
    pub rows: Vec<TableRowView>,
}

#[derive(Debug, Clone)]
pub struct TableRowView {
    pub cells: Vec<TableCellView>,
}

/// 单元格视图（匹配与区域生成的输入）。
#[derive(Debug, Clone)]
pub struct TableCellView {
    /// 清理标签后的可见文本（多段落以 \n 分隔；可为空）。
    pub text: String,
    pub col_start: usize,
    pub col_span: usize,
    pub v_merge: CellVMerge,
    /// 0 左 / 1 中 / 2 右（取首段 w:jc）。
    pub align: i32,
    /// 首段首个 run 字号（半磅；字号回退用）。
    pub sz_half_pt: Option<u32>,
    /// {{打印:...}} 标签 → 生成印刷体区域（擦除原字重排）。
    pub printed: bool,
    /// 灰高亮 → 整格保留印刷底图（不生成区域、不擦除）。
    pub skip_printed: bool,
    /// 首个非灰高亮 run 的高亮色名（角色映射用）。
    pub highlight: Option<String>,
}

/// 从 body 元素流构造表格视图列表（跳过段落元素，保持文档顺序）。
pub fn build_table_views(stream: &[BodyElement]) -> Vec<TableView> {
    let mut views = Vec::new();
    for el in stream {
        if let BodyElement::Table(t) = el {
            let rows = t
                .rows
                .iter()
                .map(|r| TableRowView { cells: r.cells.iter().map(build_cell_view).collect() })
                .collect();
            views.push(TableView { index: views.len() + 1, rows });
        }
    }
    views
}

fn build_cell_view(c: &ParsedCell) -> TableCellView {
    let raw_text: String = c.runs.iter().map(|r| r.text.as_str()).collect();
    let text = strip_tag_syntax(&raw_text);
    let has_gray = c
        .runs
        .iter()
        .any(|r| r.style.highlight.as_deref().is_some_and(is_gray_highlight));
    let printed_marker = raw_text.contains("{{打印") || raw_text.contains("【打印");
    let highlight = c
        .runs
        .iter()
        .find(|r| {
            r.style.highlight.is_some() && !r.style.highlight.as_deref().is_some_and(is_gray_highlight)
        })
        .and_then(|r| r.style.highlight.clone());
    let align = match c.fmt.jc.as_deref() {
        Some("center") => 1,
        Some("right") => 2,
        _ => 0,
    };
    // 先于结构体字面量计算，避免 text 移动后借用。
    let printed = printed_marker && !text.is_empty();
    TableCellView {
        text,
        col_start: c.col_start,
        col_span: c.grid_span,
        v_merge: c.v_merge,
        align,
        sz_half_pt: c.fmt.run_sz_half_pt,
        printed,
        skip_printed: has_gray,
        highlight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_views_from_stream() {
        // 直接构造 ParsedTable 太繁琐，这里经 XML 走真实解析（复用 docx_io 测试手段）
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>正文</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>{{手写:答案}}</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        let stream = crate::core::docx_io::parse_body_stream_for_test(xml, &mut registry).unwrap();
        let views = build_table_views(&stream);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].index, 1);
        let c0 = &views[0].rows[0].cells[0];
        assert_eq!(c0.text, "标题");
        assert_eq!(c0.align, 1);
        assert!(!c0.printed);
        let c1 = &views[0].rows[0].cells[1];
        assert_eq!(c1.text, "答案"); // {{手写:...}} 标签清理为内容
        assert_eq!(c1.highlight.as_deref(), Some("yellow"));
    }

    #[test]
    fn printed_and_gray_cells_flagged() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>{{打印:2026-01-01}}</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:rPr><w:highlight w:val="lightGray"/></w:rPr><w:t>灰色格</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        let stream = crate::core::docx_io::parse_body_stream_for_test(xml, &mut registry).unwrap();
        let views = build_table_views(&stream);
        let c0 = &views[0].rows[0].cells[0];
        assert_eq!(c0.text, "2026-01-01");
        assert!(c0.printed); // {{打印:}} → 印刷体区域
        assert!(!c0.skip_printed);
        let c1 = &views[0].rows[0].cells[1];
        assert!(c1.skip_printed); // 灰高亮 → 整格保留印刷底图
        assert!(!c1.printed);
    }
}
