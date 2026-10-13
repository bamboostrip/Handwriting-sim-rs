//! docx 表格视图：从 body 元素流构造供匹配与区域生成使用的表格结构。
//!
//! 每个单元格得到：清理后的可见文本（{{...}}/【...】标签已剥离）、
//! 列起始/跨列、对齐、字号回退、印刷体判定与高亮色。
//! 嵌套表格的内容已在 docx_io 解析时丢弃（外层格文本不含内层表格）。

use crate::core::docx_io::{BodyElement, CellVMerge, ParsedCell};
use crate::core::doc_render::{strip_tag_syntax, ExtractedChar};
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

/// 正文段落匹配的向前搜索窗口（字符数）。
const PARAGRAPH_WINDOW: usize = 500;
/// 单元格匹配的向前搜索窗口（字符数）。
const CELL_WINDOW: usize = 3000;
/// 单元格判定定位成功的最低命中比例。
const CELL_MATCH_RATIO: f32 = 0.6;

/// 单个单元格在页面字符流中的命中结果。
#[derive(Debug, Clone)]
pub struct CellMatch {
    /// 命中的 chars 下标（阅读顺序）。
    pub char_idxs: Vec<usize>,
}

/// 一张表在一页上的匹配结果（与 TableView.rows 行列对齐）。
#[derive(Debug, Clone)]
pub struct TablePageMatch {
    pub table_index: usize,
    pub row_matches: Vec<Vec<Option<CellMatch>>>,
}

/// 归一化文本：丢弃空白与控制字符（docx 文本与 PDF 字符层口径一致）。
fn norm_chars(text: &str) -> Vec<char> {
    text.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect()
}

/// 页面字符流匹配器（两指针子序列匹配）。
struct PageStreamMatcher {
    /// (chars 下标, 归一化字符)
    seq: Vec<(usize, char)>,
    cursor: usize,
}

impl PageStreamMatcher {
    fn new(chars: &[ExtractedChar]) -> Self {
        Self {
            seq: chars
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.ch.is_whitespace() && !c.ch.is_control())
                .map(|(i, c)| (i, c.ch))
                .collect(),
            cursor: 0,
        }
    }

    /// 从 cursor 起在 window 内逐字符匹配 target。
    /// 返回命中下标；consume_on_fail=true 时失败也保留已消费进度（正文段落尽力消耗）。
    fn try_match(&mut self, target: &[char], window: usize, consume_on_fail: bool) -> Vec<usize> {
        let start = self.cursor;
        let limit = (start + window).min(self.seq.len());
        let mut hit = Vec::new();
        let mut probe = self.cursor;
        for &tc in target {
            let mut found = None;
            while probe < limit {
                if self.seq[probe].1 == tc {
                    found = Some(probe);
                    break;
                }
                probe += 1;
            }
            match found {
                Some(p) => {
                    hit.push(self.seq[p].0);
                    probe += 1;
                }
                None => {
                    if consume_on_fail {
                        self.cursor = probe;
                    } else {
                        self.cursor = start;
                    }
                    return hit;
                }
            }
        }
        self.cursor = probe;
        hit
    }

    /// 匹配单元格文本：命中比例 >= CELL_MATCH_RATIO 判定定位成功。
    /// 定位成功（含部分命中）时推进 cursor 到最后命中字符之后——
    /// 命中的页面字符已被本格占用，相邻格不得重叠匹配。
    fn match_cell(&mut self, target: &[char]) -> Option<CellMatch> {
        if target.is_empty() {
            return None;
        }
        let hit = self.try_match(target, CELL_WINDOW, false);
        if hit.len() as f32 / target.len() as f32 >= CELL_MATCH_RATIO {
            if let Some(&last) = hit.last() {
                self.advance_to_after(last);
            }
            Some(CellMatch { char_idxs: hit })
        } else {
            None
        }
    }

    /// 将 cursor 推进到原始下标 char_idx 的命中字符之后。
    /// 部分命中时 try_match 已回滚 cursor 到匹配起点，命中位置必在其前方；
    /// 整段命中时 cursor 已越过该字符（原始下标唯一），此处为空操作。
    fn advance_to_after(&mut self, char_idx: usize) {
        if let Some(pos) = self.seq[self.cursor..].iter().position(|&(i, _)| i == char_idx) {
            self.cursor += pos + 1;
        }
    }
}

/// 在单页字符流上按文档顺序定位所有表格的单元格。
/// 未命中任何文字的表也返回（row_matches 全 None，由后续步骤过滤）。
pub fn locate_tables_on_page(
    stream: &[BodyElement],
    views: &[TableView],
    chars: &[ExtractedChar],
) -> Vec<TablePageMatch> {
    let mut matcher = PageStreamMatcher::new(chars);
    let mut result = Vec::new();
    let mut view_cursor = 0usize;
    for el in stream {
        match el {
            BodyElement::Paragraph(p) => {
                let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
                let norm = norm_chars(&text);
                if !norm.is_empty() {
                    matcher.try_match(&norm, PARAGRAPH_WINDOW, true);
                }
            }
            BodyElement::Table(_) => {
                let view = match views.get(view_cursor) {
                    Some(v) => v,
                    None => continue,
                };
                view_cursor += 1;
                let row_matches = view
                    .rows
                    .iter()
                    .map(|row| {
                        row.cells
                            .iter()
                            .map(|cell| {
                                if cell.v_merge == CellVMerge::Continue {
                                    return None; // 续行格无内容
                                }
                                let norm = norm_chars(&cell.text);
                                matcher.match_cell(&norm)
                            })
                            .collect()
                    })
                    .collect();
                result.push(TablePageMatch { table_index: view.index, row_matches });
            }
        }
    }
    result
}

/// 像素矩形（背景图原始像素坐标）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectPx {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 命中字符包围盒（每边外扩 pad 像素）。
fn union_char_bbox(chars: &[ExtractedChar], idxs: &[usize], pad: i32) -> RectPx {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for &i in idxs {
        let c = &chars[i];
        min_x = min_x.min(c.min_x);
        min_y = min_y.min(c.min_y);
        max_x = max_x.max(c.max_x);
        max_y = max_y.max(c.max_y);
    }
    RectPx {
        x: (min_x.floor() as i32 - pad).max(0),
        y: (min_y.floor() as i32 - pad).max(0),
        w: (max_x.ceil() - min_x.floor()) as i32 + pad * 2,
        h: (max_y.ceil() - min_y.floor()) as i32 + pad * 2,
    }
}

fn mean_of(vals: &[f32]) -> Option<f32> {
    if vals.is_empty() { None } else { Some(vals.iter().sum::<f32>() / vals.len() as f32) }
}

/// 表格最大列数（各行列终点的最大值）。
fn view_max_cols(view: &TableView) -> usize {
    view.rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.col_start + c.col_span).max().unwrap_or(0))
        .max()
        .unwrap_or(0)
}

/// 推算整表所有单元格矩形（行主序扁平输出，与 view.rows 迭代顺序对齐）。
/// 已定位格 = 命中字符 bbox（pad=2）；未命中格 = 行/列带交集内缩 3px；
/// vMerge 续行格与无法推算的格为 None。
pub fn infer_cell_rects(
    view: &TableView,
    m: &TablePageMatch,
    chars: &[ExtractedChar],
) -> Vec<Option<RectPx>> {
    // 每格 bbox（未定位为 None）
    let bboxes: Vec<Vec<Option<RectPx>>> = view
        .rows
        .iter()
        .zip(m.row_matches.iter())
        .map(|(row, row_m)| {
            row.cells
                .iter()
                .zip(row_m.iter())
                .map(|(_, cell_m)| {
                    cell_m.as_ref().map(|cm| union_char_bbox(chars, &cm.char_idxs, 2))
                })
                .collect()
        })
        .collect();

    // 列带：由已定位的单列格（col_span==1 且非续行）收集左右边缘
    let mut col_left: Vec<Vec<f32>> = vec![Vec::new(); view_max_cols(view)];
    let mut col_right: Vec<Vec<f32>> = vec![Vec::new(); view_max_cols(view)];
    for (r_idx, row) in view.rows.iter().enumerate() {
        for (c_idx, cell) in row.cells.iter().enumerate() {
            if let Some(rect) = bboxes[r_idx][c_idx] {
                if cell.col_span == 1 && cell.v_merge != CellVMerge::Continue {
                    col_left[cell.col_start].push(rect.x as f32);
                    col_right[cell.col_start].push((rect.x + rect.w) as f32);
                }
            }
        }
    }
    // 行带：由该行任意已定位格收集上下边缘
    let mut row_top: Vec<Vec<f32>> = vec![Vec::new(); view.rows.len()];
    let mut row_bottom: Vec<Vec<f32>> = vec![Vec::new(); view.rows.len()];
    for (r_idx, row) in view.rows.iter().enumerate() {
        for (c_idx, _) in row.cells.iter().enumerate() {
            if let Some(rect) = bboxes[r_idx][c_idx] {
                row_top[r_idx].push(rect.y as f32);
                row_bottom[r_idx].push((rect.y + rect.h) as f32);
            }
        }
    }

    let mut out = Vec::new();
    for (r_idx, row) in view.rows.iter().enumerate() {
        for (c_idx, cell) in row.cells.iter().enumerate() {
            if cell.v_merge == CellVMerge::Continue {
                out.push(None);
                continue;
            }
            if let Some(rect) = bboxes[r_idx][c_idx] {
                out.push(Some(rect));
                continue;
            }
            // 未定位格：行带 × 列带交集（跨列取首末列带），内缩 3px
            let x0 = mean_of(&col_left[cell.col_start]);
            let last_col = (cell.col_start + cell.col_span - 1).min(col_right.len().saturating_sub(1));
            let x1 = mean_of(&col_right[last_col]);
            let y0 = mean_of(&row_top[r_idx]);
            let y1 = mean_of(&row_bottom[r_idx]);
            match (x0, x1, y0, y1) {
                (Some(x0), Some(x1), Some(y0), Some(y1)) if x1 > x0 && y1 > y0 => {
                    out.push(Some(RectPx {
                        x: (x0 + 3.0) as i32,
                        y: (y0 + 3.0) as i32,
                        w: (x1 - x0 - 6.0) as i32,
                        h: (y1 - y0 - 6.0) as i32,
                    }));
                }
                _ => out.push(None),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::doc_render::ExtractedChar;

    fn ch(c: char, x: f32, y: f32) -> ExtractedChar {
        ExtractedChar {
            ch: c,
            min_x: x,
            min_y: y,
            max_x: x + 10.0,
            max_y: y + 10.0,
            font_size_pt: 10.5,
            glyph_h_pt: 10.0,
        }
    }

    fn table_stream_xml(body: &str) -> Vec<crate::core::docx_io::BodyElement> {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        crate::core::docx_io::parse_body_stream_for_test(&xml, &mut registry).unwrap()
    }

    #[test]
    fn locates_cells_in_reading_order() {
        let stream = table_stream_xml(
            r#"<w:p><w:r><w:t>前面的正文</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        // 页面字符流：正文 + 表格两格（阅读顺序）
        let chars = vec![
            ch('前', 10.0, 10.0), ch('面', 22.0, 10.0), ch('的', 34.0, 10.0), ch('正', 46.0, 10.0), ch('文', 58.0, 10.0),
            ch('甲', 10.0, 50.0), ch('格', 22.0, 50.0),
            ch('乙', 110.0, 50.0), ch('格', 122.0, 50.0),
        ];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.table_index, 1);
        let c0 = m.row_matches[0][0].as_ref().unwrap();
        assert_eq!(c0.char_idxs, vec![5, 6]);
        let c1 = m.row_matches[0][1].as_ref().unwrap();
        assert_eq!(c1.char_idxs, vec![7, 8]);
    }

    #[test]
    fn body_text_with_same_content_does_not_steal_cell_match() {
        // 正文里出现与单元格相同的文字且在字符流中先出现：
        // 段落元素先行消耗自己的字符，单元格匹配不会误中正文位置
        let stream = table_stream_xml(
            r#"<w:p><w:r><w:t>甲格</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![
            ch('甲', 10.0, 10.0), ch('格', 22.0, 10.0),   // 正文（页面上部）
            ch('甲', 10.0, 100.0), ch('格', 22.0, 100.0), // 单元格（页面下部）
        ];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        let c0 = matches[0].row_matches[0][0].as_ref().unwrap();
        assert_eq!(c0.char_idxs, vec![2, 3]); // 命中下部（正文已消耗 0、1）
    }

    #[test]
    fn unmatched_cell_below_threshold_yields_none() {
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>五行文字</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        // 页面上只有 1 个字符能对上（1/4 = 25% < 60%）
        let chars = vec![ch('五', 10.0, 10.0), ch('x', 30.0, 10.0), ch('无', 50.0, 10.0), ch('关', 70.0, 10.0)];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        assert!(matches[0].row_matches[0][0].is_none());
    }

    #[test]
    fn vmerge_continue_cell_skips_without_matching() {
        // 续行格即使残留文字也直接返回 None，不参与匹配、不消耗字符流；
        // 同行后续单元格仍在正确位置命中
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙格</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t>丙格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>丁格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![
            ch('甲', 10.0, 10.0), ch('格', 22.0, 10.0),
            ch('乙', 110.0, 10.0), ch('格', 122.0, 10.0),
            ch('丙', 10.0, 50.0), ch('格', 22.0, 50.0),
            ch('丁', 110.0, 50.0), ch('格', 122.0, 50.0),
        ];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        let m = &matches[0];
        assert_eq!(m.row_matches[0][0].as_ref().unwrap().char_idxs, vec![0, 1]);
        assert_eq!(m.row_matches[0][1].as_ref().unwrap().char_idxs, vec![2, 3]);
        assert!(m.row_matches[1][0].is_none()); // 续行格跳过（否则会误中 4、5）
        assert_eq!(m.row_matches[1][1].as_ref().unwrap().char_idxs, vec![6, 7]);
    }

    #[test]
    fn partially_matched_cell_advances_cursor_preventing_overlap() {
        // A 格 75% 命中（“总”不在页面任何位置）：若 cursor 不随部分命中前进，
        // B 格“汇率”的贪心扫描会误中 A 已占用的“汇”（下标 2），命中下标重叠
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>金额汇总</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>汇率</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![
            ch('金', 10.0, 10.0), ch('额', 22.0, 10.0), ch('汇', 34.0, 10.0), // A：3/4 = 75%
            ch('汇', 110.0, 50.0), ch('率', 122.0, 50.0),                     // B
        ];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        let m = &matches[0];
        let a = m.row_matches[0][0].as_ref().unwrap();
        assert_eq!(a.char_idxs, vec![0, 1, 2]); // 部分命中也算定位
        let b = m.row_matches[0][1].as_ref().unwrap();
        assert_eq!(b.char_idxs, vec![3, 4]); // 不复用 A 的下标 2
        assert!(a.char_idxs.iter().all(|i| !b.char_idxs.contains(i)), "相邻格命中下标不得重叠");
    }

    #[test]
    fn infers_missing_cell_rect_from_bands() {
        // 2×2 表：三个格有文字可定位，右下格为空 → 由行列带推算
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid>
               <w:tr><w:tc><w:p><w:r><w:t>甲</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙</w:t></w:r></w:p></w:tc></w:tr>
               <w:tr><w:tc><w:p><w:r><w:t>丙</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![
            ch('甲', 100.0, 100.0), ch('乙', 300.0, 100.0),
            ch('丙', 100.0, 200.0),
        ];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        let rects = infer_cell_rects(&views[0], &matches[0], &chars);
        assert_eq!(rects.len(), 4);
        // 甲：命中字符 bbox（+2px pad）→ x≈98, y≈98, w/h≈14
        let r0 = rects[0].unwrap();
        assert_eq!((r0.x, r0.y, r0.w, r0.h), (98, 98, 14, 14));
        // 乙、丙同理
        assert_eq!(rects[1].unwrap().x, 298);
        assert_eq!(rects[2].unwrap().y, 198);
        // 右下空格：行带 y∈[198,212+] 列带 x∈[298,312+] 交集（内缩 3px）
        let r3 = rects[3].unwrap();
        assert!(r3.x >= 296 && r3.x <= 302, "r3.x={}", r3.x);
        assert!(r3.y >= 196 && r3.y <= 202, "r3.y={}", r3.y);
        assert!(r3.w > 0 && r3.h > 0);
    }

    #[test]
    fn no_bands_no_rect() {
        // 整行无命中 → 该行空格无法推算
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid>
               <w:tr><w:tc><w:p><w:r><w:t>甲</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙</w:t></w:r></w:p></w:tc></w:tr>
               <w:tr><w:tc><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![ch('甲', 100.0, 100.0), ch('乙', 300.0, 100.0)];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        let rects = infer_cell_rects(&views[0], &matches[0], &chars);
        assert!(rects[2].is_none());
        assert!(rects[3].is_none());
    }

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
