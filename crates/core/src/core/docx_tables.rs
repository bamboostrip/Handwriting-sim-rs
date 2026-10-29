//! docx 表格视图：从 body 元素流构造供匹配与区域生成使用的表格结构。
//!
//! 每个单元格得到：清理后的可见文本（{{...}}/【...】标签已剥离）、
//! 列起始/跨列、对齐、字号回退、印刷体判定与高亮色
//!（w:highlight 优先，无高亮时具饱和度的彩色 w:color 字色回退映射为
//! 标准色名；黑/灰/白字是正文配色，不映射）。
//! 嵌套表格的内容已在 docx_io 解析时丢弃（外层格文本不含内层表格）。

use crate::core::docx_io::{BodyElement, CellVMerge, ParsedCell};
use crate::core::doc_render::{classify_highlight_color, strip_tag_syntax, ExtractedChar};
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
    /// 高亮色名（角色映射用）：首个非灰 w:highlight run；无 w:highlight 时
    /// 回退取首个具饱和度（通道差 ≥ COLOR_FILL_MIN_SPREAD）的彩色 w:color run
    /// 经 classify_highlight_color 映射的标准色名；黑/灰/白字不映射。
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

/// 字色回退映射的最低通道差（max−min，与 doc_render::is_highlight_pixel 的
/// 灰度差阈值同口径）：黑白灰字（7F7F7F/A6A6A6/D9D9D9/FFFFFF 等）通道差
/// 为 0，是正文配色而非标记，不得落入 classify_highlight_color 的兜底色。
const COLOR_FILL_MIN_SPREAD: u8 = 30;

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
        .and_then(|r| r.style.highlight.clone())
        // 无 w:highlight 时回退取首个具饱和度的彩色 w:color run（用户用字体
        // 颜色而非高亮标记单元格），映射为标准色名以复用高亮角色配色管线。
        // 黑色是正文默认色、灰/白是正文配色，通道差 < COLOR_FILL_MIN_SPREAD
        // 一律不构成标记（classify 兜底分支会返回 “yellow”，必须拦在门外）。
        // 灰色判定（skip_printed）仍只看 w:highlight，此处不改变印刷体语义。
        .or_else(|| {
            c.runs
                .iter()
                .find_map(|r| {
                    r.style.fill.filter(|rgb| {
                        let [cr, cg, cb] = *rgb;
                        cr.max(cg).max(cb) - cr.min(cg).min(cb) >= COLOR_FILL_MIN_SPREAD
                    })
                })
                .map(|[r, g, b]| classify_highlight_color(r, g, b).to_string())
        });
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
/// 表格在一页上判定“已开始输出区域”的最低定位格数（防止正文误匹配产生幻影区域）。
const TABLE_LOCATED_MIN_CELLS: usize = 2;
/// 表格判定“整表完成”（后续页不再重试）的定位比例。消费为严格大于：
/// 恰好一半（如 4 格表 2+2 均分）仍视为分割中，后半必须留给下一页补走
///（在 50% 处消费会让后半永不出现在任何后续页的起步元素流中）。
const TABLE_CONSUMED_RATIO: f32 = 0.5;
/// 段落判定匹配成功（消费字符）的最低命中比例；失败时游标回退不再推进。
const PARAGRAPH_MATCH_RATIO: f32 = 0.6;

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
    /// 本页定位成功的可定位格数。
    pub located_cell_count: usize,
    /// 本页参与匹配判定的格数（非续行、归一化文本 ≥ 2 字）。
    pub locatable_cell_count: usize,
    /// 表格是否在本页开始输出区域（定位格数 ≥ 阈值，或定位比例 ≥ 消费比例）。
    pub located_on_page: bool,
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
    ///
    /// 归一化长度 < 2 的格不参与匹配：跨页拆分表会被后续页重走（未消费），
    /// 1 字格会在 CELL_WINDOW 内撞上任意同名无关字符（单字命中即 100%，
    /// 轻松越过阈值）造成幻影定位——真持有格随后失配回退带推断，
    /// 同一文字被重复渲染。短格的兜底是带推断，且前提是同行存在可定位的
    /// 多字格提供行带；整行均为单字格（如序号行 一/二/三、口算表数字格）
    /// 时无行带可用，该行保持印刷底图（不生成区域）。
    fn match_cell(&mut self, target: &[char]) -> Option<CellMatch> {
        if target.len() < 2 {
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

    /// 匹配正文段落：命中比例 >= PARAGRAPH_MATCH_RATIO 判定段落在本页，
    /// cursor 推进到最后命中字符之后（段落字符已为本段占用）。
    /// 失败时 try_match（consume_on_fail=false）已把 cursor 回退到匹配起点——
    /// 不得“尽力消耗”：跨页重走时，前页段落会把后页字符流的 cursor
    /// 一路推过流尾（每段最多 PARAGRAPH_WINDOW=500 字），轮到表格元素时
    /// 已无字符可配（游标中毒，真实用户 2 页合同的 0 区域根因）。
    fn match_paragraph(&mut self, target: &[char]) -> bool {
        let hit = self.try_match(target, PARAGRAPH_WINDOW, false);
        if hit.len() as f32 / target.len() as f32 >= PARAGRAPH_MATCH_RATIO {
            if let Some(&last) = hit.last() {
                self.advance_to_after(last);
            }
            true
        } else {
            false
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

/// 在单页字符流上按文档顺序定位表格单元格，从 `start_element` 起走
///（其之前的元素属于更早的页，本页不触碰——否则前页段落会消耗本页字符，
/// 游标中毒）。返回 (各表匹配结果（含 located_on_page 标记）, matched_upto)：
/// matched_upto 为“最后一个已消费元素之后”的下标，供下一页作为 start_element。
/// 段落匹配成功（比例达标）→ 消费；表格定位比例 > TABLE_CONSUMED_RATIO → 整表
/// 消费（后续页不再重走；恰好一半不算）；拆分表（已开始但未过半）与本页
/// 未定位的表不推进 matched_upto——下一页重走该表时，已定位格的字符不在
/// 下一页流中，只会定位余下部分，不会重复。
/// 分割中的表（本页已定位但未消费）还钳制 matched_upto：按文档序其后的
/// 段落渲染于表尾之后，只可能出现在更晚的页，本页任何段落“匹配”均为
/// 假匹配（短段落如 甲方/乙方 2 字即可廉价误中），不得借其把 matched_upto
/// 推过该表（否则表被永久跳过，余下半表 0 区域）。从未定位过的表
///（located == 0）不钳制：其后的首个合法段落匹配仍可推进 matched_upto
///（自愈路径，见 long_doc_table_on_later_page_locates）。
/// 未命中任何文字的表也返回（row_matches 全 None、located_on_page=false，
/// 由调用方按 located_on_page 门禁过滤）。
pub fn locate_tables_on_page(
    stream: &[BodyElement],
    views: &[TableView],
    chars: &[ExtractedChar],
    start_element: usize,
) -> (Vec<TablePageMatch>, usize) {
    let mut matcher = PageStreamMatcher::new(chars);
    let mut result = Vec::new();
    // 视图游标对齐文档表序：start_element 之前的表格已由更早的页处理，
    // 本页走到的第 k 张表对应 views 中第 k 张（views 按文档顺序构建）
    let mut view_cursor = stream[..start_element.min(stream.len())]
        .iter()
        .filter(|el| matches!(el, BodyElement::Table(_)))
        .count();
    let mut matched_upto = start_element;
    // 最后一个“正在分割中”的表元素下标（本页已定位未消费）：
    // 走完后用于把 matched_upto 钳回该表（假段落匹配防护）。
    let mut splitting_table_element: Option<usize> = None;
    for (i, el) in stream.iter().enumerate().skip(start_element) {
        match el {
            BodyElement::Paragraph(p) => {
                let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
                let norm = norm_chars(&text);
                if !norm.is_empty() && matcher.match_paragraph(&norm) {
                    matched_upto = i + 1;
                }
            }
            BodyElement::Table(_) => {
                let view = match views.get(view_cursor) {
                    Some(v) => v,
                    None => continue,
                };
                view_cursor += 1;
                let mut located = 0usize;
                let mut locatable = 0usize;
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
                                if norm.len() < 2 {
                                    return None; // 短格不参与匹配（与 match_cell 规则一致）
                                }
                                locatable += 1;
                                let m = matcher.match_cell(&norm);
                                if m.is_some() {
                                    located += 1;
                                }
                                m
                            })
                            .collect()
                    })
                    .collect();
                let mut m = TablePageMatch {
                    table_index: view.index,
                    row_matches,
                    located_cell_count: located,
                    locatable_cell_count: locatable,
                    located_on_page: false,
                };
                let ratio = if m.locatable_cell_count > 0 {
                    m.located_cell_count as f32 / m.locatable_cell_count as f32
                } else {
                    0.0
                };
                m.located_on_page = m.located_cell_count >= TABLE_LOCATED_MIN_CELLS
                    || (m.locatable_cell_count > 0 && ratio >= TABLE_CONSUMED_RATIO);
                if m.locatable_cell_count > 0 && ratio > TABLE_CONSUMED_RATIO {
                    matched_upto = i + 1; // 整表完成：后续页不再重走（严格大于：恰好一半仍在分割）
                } else if located > 0 {
                    // 分割中（已定位未消费）：记为钳制点，走完后不得让
                    // 后续段落的假匹配把 matched_upto 推过本表。
                    splitting_table_element = Some(i);
                }
                result.push(m);
            }
        }
    }
    // 假段落匹配钳制：分割中的表之后的段落匹配在本页必为假命中，钳回。
    if let Some(idx) = splitting_table_element {
        if matched_upto > idx {
            matched_upto = idx;
        }
    }
    (result, matched_upto)
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
            glyph_h_pt: 10.0, tight_min_y: 0.0, tight_max_y: 0.0,
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
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
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
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
        let c0 = matches[0].row_matches[0][0].as_ref().unwrap();
        assert_eq!(c0.char_idxs, vec![2, 3]); // 命中下部（正文已消耗 0、1）
    }

    #[test]
    fn single_char_cell_does_not_false_match_across_pages() {
        // 跨页短格回归：多页表格从第 2 页起，每页都拿“整个文档流”对当页字符重匹配。
        // 第 1 页表格的 1 字格“乙”，会在第 2 页字符流的 CELL_WINDOW 内撞上任何
        // 一个无关的“乙”（1/1 = 100% 轻松越过 60% 阈值）→ 误定位 + cursor 前进，
        // 真正的持有格回退到带推断 → 同一文字在页面上被定位+推断各渲染一次。
        // 规则：归一化长度 < 2 的格不参与匹配，直接回退带推断（无损失）。
        let stream = table_stream_xml(
            r#"<w:p><w:r><w:t>无关正文内容</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>甲格甲格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        // 第 2 页字符流：无关正文 + 跨页延续的多字格文字 + 一个孤立的“乙”
        let chars = vec![
            ch('无', 10.0, 10.0), ch('关', 22.0, 10.0), ch('正', 34.0, 10.0), ch('文', 46.0, 10.0), ch('内', 58.0, 10.0), ch('容', 70.0, 10.0),
            ch('甲', 10.0, 100.0), ch('格', 22.0, 100.0), ch('甲', 34.0, 100.0), ch('格', 46.0, 100.0),
            ch('乙', 200.0, 200.0),
        ];
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
        let m = &matches[0];
        // 多字格：在第 2 页真实存在，正常定位
        assert_eq!(m.row_matches[0][0].as_ref().unwrap().char_idxs, vec![6, 7, 8, 9]);
        // 1 字格：不得匹配窗口内的孤立“乙”（修复前这里误返回 Some）
        assert!(m.row_matches[0][1].is_none(), "1 字格不得误匹配：{:?}", m.row_matches[0][1]);
    }

    #[test]
    fn long_doc_table_on_later_page_locates() {
        // 用户真实 bug 回归：长正文 + 表格在第 2 页。旧逻辑每页都从元素 0
        // 走整个文档流，第 1 页段落对第 2 页字符流“尽力消耗”
        //（consume_on_fail=true，每段最多推进 PARAGRAPH_WINDOW=500 字），
        // 游标被推过流尾 → 轮到表格元素时无字符可配 → 0 区域。
        // 规则：每页只从 start_element 起走；段落匹配失败回退游标、不推进 matched_upto。
        let stream = table_stream_xml(
            r#"<w:p><w:r><w:t>这是一段非常长的正文内容，用来填充整个第一页版面的占位文字，合计超过三十个字符</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙格</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>丙格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>丁格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        // 第 1 页字符流：只有 P1 的字符（34 字）
        let p1_text = "这是一段非常长的正文内容，用来填充整个第一页版面的占位文字，合计超过三十个字符";
        let page1: Vec<ExtractedChar> = p1_text
            .chars()
            .enumerate()
            .map(|(i, c)| ch(c, 10.0 + (i as f32 % 20.0) * 12.0, 10.0 + (i as f32 / 20.0) * 14.0))
            .collect();
        let (m1, next1) = locate_tables_on_page(&stream, &views, &page1, 0);
        assert_eq!(m1.len(), 1);
        assert!(!m1[0].located_on_page, "表格不在第 1 页：单元格字符不在第 1 页流中");
        assert_eq!(m1[0].located_cell_count, 0);
        assert_eq!(m1[0].locatable_cell_count, 4);
        // P1 已匹配（matched_upto 越过 P1 = 下标 1），表格未定位未消费：
        // 下一页起点必须指向表格元素本身（下标 1），否则表格永远无法被定位
        assert_eq!(next1, 1, "未定位表格不得被跳过");

        // 第 2 页字符流：只有表格 4 格的字符
        let page2 = vec![
            ch('甲', 10.0, 500.0), ch('格', 22.0, 500.0), ch('乙', 110.0, 500.0), ch('格', 122.0, 500.0),
            ch('丙', 10.0, 560.0), ch('格', 22.0, 560.0), ch('丁', 110.0, 560.0), ch('格', 122.0, 560.0),
        ];
        let (m2, next2) = locate_tables_on_page(&stream, &views, &page2, next1);
        assert!(m2[0].located_on_page, "表格在第 2 页应被定位");
        assert_eq!(m2[0].located_cell_count, 4);
        assert_eq!(m2[0].locatable_cell_count, 4);
        assert!(m2[0].row_matches.iter().all(|r| r.iter().all(|c| c.is_some())));
        assert_eq!(next2, 2, "整表定位完成（4/4 ≥ 0.5）→ 消费，后续页不再重走");

        // 游标中毒回归锁：即使从元素 0 起走（含第 1 页段落），段落匹配失败
        // 也不得推进游标——旧逻辑 consume_on_fail=true 会把游标推到流尾
        //（8 字流被 500 字窗口一段打穿）→ 全部格失配 → located_on_page=false
        let (m3, _) = locate_tables_on_page(&stream, &views, &page2, 0);
        assert!(m3[0].located_on_page, "start=0 对第 2 页字符流不得中毒");
        assert_eq!(m3[0].located_cell_count, 4);
    }

    #[test]
    fn split_table_continues_on_next_page() {
        // 跨页拆分表（8 格，第 1 页 3 格）：3 >= 2 → 已开始输出区域；
        // 3/8 = 0.375 < 0.5 → 整表未完成，下一页从表格元素重走。
        // 已定位格的字符不在第 2 页流中 → 只会定位余下 5 格，两页各输出
        // 自己的部分，任何一页都不会输出全部 8 格（不重复）。
        let cells = ["甲格", "乙格", "丙格", "丁格", "戊格", "己格", "庚格", "辛格"];
        let tc = |t: &str| format!(r#"<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>"#);
        let row1: String = cells[..4].iter().map(|t| tc(t)).collect();
        let row2: String = cells[4..].iter().map(|t| tc(t)).collect();
        let body = format!(
            r#"<w:tbl><w:tblGrid>{}</w:tblGrid><w:tr>{row1}</w:tr><w:tr>{row2}</w:tr></w:tbl>"#,
            "<w:gridCol/>".repeat(4)
        );
        let stream = table_stream_xml(&body);
        let views = build_table_views(&stream);
        let cell_chars = |texts: &[&str], y0: f32| -> Vec<ExtractedChar> {
            let mut out = Vec::new();
            for (k, t) in texts.iter().enumerate() {
                let x = 10.0 + (k as f32) * 100.0;
                for (j, c) in t.chars().enumerate() {
                    out.push(ch(c, x + j as f32 * 12.0, y0));
                }
            }
            out
        };
        // 第 1 页：前 3 格；第 2 页：后 5 格
        let page1 = cell_chars(&cells[..3], 100.0);
        let page2 = cell_chars(&cells[3..], 200.0);

        let (m1, next1) = locate_tables_on_page(&stream, &views, &page1, 0);
        assert!(m1[0].located_on_page, "3 >= 2：拆分表已开始");
        assert_eq!(m1[0].located_cell_count, 3);
        assert_eq!(m1[0].locatable_cell_count, 8);
        assert_eq!(next1, 0, "3/8 = 0.375 < 0.5 未完成：下一页仍指向表格元素（下标 0）");

        let (m2, next2) = locate_tables_on_page(&stream, &views, &page2, next1);
        assert!(m2[0].located_on_page);
        // 前 3 格在第 2 页流中不存在 → None；余下 5 格全部定位
        let flat: Vec<Option<&CellMatch>> =
            m2[0].row_matches.iter().flat_map(|r| r.iter()).map(|c| c.as_ref()).collect();
        assert!(flat[..3].iter().all(|c| c.is_none()), "第 1 页已定位的格在第 2 页不得再定位");
        assert!(flat[3..].iter().all(|c| c.is_some()));
        assert_eq!(m2[0].located_cell_count, 5);
        assert_eq!(m2[0].locatable_cell_count, 8);
        assert_eq!(next2, 1, "5/8 = 0.625 ≥ 0.5：整表完成 → 消费");
        // 不重复：两页合计恰好 8，且任何一页都未输出全部
        assert_eq!(m1[0].located_cell_count + m2[0].located_cell_count, 8);
        assert!(m1[0].located_cell_count < 8 && m2[0].located_cell_count < 8);
    }

    /// 按单元格文本列表生成一行页面字符（k 格横排，格内逐字排列）。
    fn row_chars(texts: &[&str], y0: f32) -> Vec<ExtractedChar> {
        let mut out = Vec::new();
        for (k, t) in texts.iter().enumerate() {
            let x = 10.0 + (k as f32) * 100.0;
            for (j, c) in t.chars().enumerate() {
                out.push(ch(c, x + j as f32 * 12.0, y0));
            }
        }
        out
    }

    #[test]
    fn even_split_table_continues_on_next_page() {
        // 恰好 50% 分割回归：4 格表第 1 页只出前 2 格，2/4 = 0.5。
        // 旧消费判定 ratio >= 0.5 在第 1 页即整表消费，下一页从表后元素
        // 起步，后 2 格永不被重走（保持印刷底图）。修复：消费改为严格
        // 大于——0.5 不消费，下一页重走本表只定位余下 2 格。
        // 注：第 2 页同样 2/4 = 0.5 仍不消费（分母始终是全部可定位格）；
        // 表保持可重走是无害的：更晚的页定位 0 格 → located_on_page=false
        // → 不输出区域、不擦除。
        let cells = ["甲格", "乙格", "丙格", "丁格"];
        let tc = |t: &str| format!(r#"<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>"#);
        let body = format!(
            r#"<w:tbl><w:tblGrid>{}</w:tblGrid><w:tr>{}</w:tr></w:tbl>"#,
            "<w:gridCol/>".repeat(4),
            cells.iter().map(|t| tc(t)).collect::<String>(),
        );
        let stream = table_stream_xml(&body);
        let views = build_table_views(&stream);
        let page1 = row_chars(&cells[..2], 100.0);
        let page2 = row_chars(&cells[2..], 200.0);

        let (m1, next1) = locate_tables_on_page(&stream, &views, &page1, 0);
        assert!(m1[0].located_on_page, "2 >= 2：拆分表已开始输出区域");
        assert_eq!(m1[0].located_cell_count, 2);
        assert_eq!(m1[0].locatable_cell_count, 4);
        assert_eq!(next1, 0, "2/4 = 0.5 恰好一半：不得消费（旧 >= 判定在此消费 → 后半丢失）");

        let (m2, _) = locate_tables_on_page(&stream, &views, &page2, next1);
        assert!(m2[0].located_on_page, "第 2 页重走表格，后 2 格定位（旧逻辑此页永不走到本表）");
        let flat: Vec<Option<&CellMatch>> =
            m2[0].row_matches.iter().flat_map(|r| r.iter()).map(|c| c.as_ref()).collect();
        assert!(flat[..2].iter().all(|c| c.is_none()), "第 1 页已定位的格在第 2 页不得再定位");
        assert!(flat[2..].iter().all(|c| c.is_some()), "后 2 格必须在第 2 页定位出区域");
        assert_eq!(m2[0].located_cell_count, 2);
        // 两页各出一半、任何一页都不输出全部 4 格（不重复）
        assert_eq!(m1[0].located_cell_count + m2[0].located_cell_count, 4);
        assert!(m1[0].located_cell_count < 4 && m2[0].located_cell_count < 4);
    }

    #[test]
    fn false_paragraph_match_cannot_skip_splitting_table() {
        // 假段落匹配回归：表（4 格，第 1 页出前 2 格，分割中）后跟短段落
        // “甲方”。按文档序“甲方”渲染在表尾之后，物理上应在更晚的页，
        // 但其字符混入第 1 页字符流（假匹配诱饵：2 字段落廉价误中）。
        // 旧逻辑段落匹配无条件推进 matched_upto → 下一页从表后元素起步
        // → 表被永久跳过，余下半表 0 区域。修复：分割中的表钳制
        // matched_upto 不得越过自身。
        let cells = ["甲格", "乙格", "丙格", "丁格"];
        let tc = |t: &str| format!(r#"<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>"#);
        let body = format!(
            r#"<w:tbl><w:tblGrid>{}</w:tblGrid><w:tr>{}</w:tr></w:tbl><w:p><w:r><w:t>甲方</w:t></w:r></w:p>"#,
            "<w:gridCol/>".repeat(4),
            cells.iter().map(|t| tc(t)).collect::<String>(),
        );
        let stream = table_stream_xml(&body);
        let views = build_table_views(&stream);
        // 第 1 页字符流：前 2 格 + 甲方（诱饵，物理在更晚页却出现在本页流中）
        let mut page1 = row_chars(&cells[..2], 100.0);
        page1.extend(row_chars(&["甲方"], 500.0));
        let (m1, next1) = locate_tables_on_page(&stream, &views, &page1, 0);
        assert_eq!(m1[0].located_cell_count, 2, "表格前半定位（分割中）");
        assert_eq!(
            next1, 0,
            "假段落匹配不得把 matched_upto 推过分割中的表（旧逻辑此处为 2，表被永久跳过）"
        );

        // 第 2 页：后 2 格正常定位（甲方不在本页流中，段落不匹配不推进）
        let page2 = row_chars(&cells[2..], 200.0);
        let (m2, next2) = locate_tables_on_page(&stream, &views, &page2, next1);
        let flat: Vec<Option<&CellMatch>> =
            m2[0].row_matches.iter().flat_map(|r| r.iter()).map(|c| c.as_ref()).collect();
        assert!(flat[..2].iter().all(|c| c.is_none()));
        assert!(flat[2..].iter().all(|c| c.is_some()), "余下 2 格必须在第 2 页定位");
        assert!(m2[0].located_on_page);
        assert_eq!(next2, 0, "2/4 = 0.5 仍在分割：流起点保持指向表格（后续页定位 0 格即安全）");
    }

    #[test]
    fn gray_and_white_font_color_not_mapped_to_highlight() {
        // 灰/白字回归：无 w:highlight 时字色回退要求饱和度（通道差 ≥ 30，
        // 与 doc_render::is_highlight_pixel 的灰度差阈值同口径）。灰色
        //（7F7F7F/A6A6A6/D9D9D9）与白色（FFFFFF）是正文配色，旧逻辑取
        // 首个非黑 run 直落 classify_highlight_color 的兜底 “yellow”，
        // 误配黄色角色。
        let tc = |color: &str, text: &str| {
            format!(r#"<w:tc><w:p><w:r><w:rPr><w:color w:val="{color}"/></w:rPr><w:t>{text}</w:t></w:r></w:p></w:tc>"#)
        };
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tblGrid>{}</w:tblGrid><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl></w:body></w:document>"#,
            "<w:gridCol/>".repeat(2),
            tc("7F7F7F", "灰字格"),
            tc("FFFFFF", "白字格"),
            tc("A6A6A6", "浅灰格"),
            tc("D9D9D9", "更浅灰格"),
        );
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        let stream = crate::core::docx_io::parse_body_stream_for_test(&xml, &mut registry).unwrap();
        let views = build_table_views(&stream);
        assert_eq!(views[0].rows[0].cells[0].highlight, None, "7F7F7F 灰字不映射");
        assert_eq!(views[0].rows[0].cells[1].highlight, None, "FFFFFF 白字不映射");
        assert_eq!(views[0].rows[1].cells[0].highlight, None, "A6A6A6 灰字不映射");
        assert_eq!(views[0].rows[1].cells[1].highlight, None, "D9D9D9 灰字不映射");
    }

    #[test]
    fn absent_paragraph_does_not_advance_cursor() {
        // 页面上不存在的段落：游标不得推进（否则后续单元格从错误起点扫描，
        // 短流时直接被打穿失配）
        let stream = table_stream_xml(
            r#"<w:p><w:r><w:t>完全不出现在页面上的无关段落正文内容</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![ch('甲', 10.0, 100.0), ch('格', 22.0, 100.0)];
        let (matches, next) = locate_tables_on_page(&stream, &views, &chars, 0);
        // 段落 0 命中 → 游标留在 0，单元格仍从起点命中 0、1
        let c0 = matches[0].row_matches[0][0].as_ref().unwrap();
        assert_eq!(c0.char_idxs, vec![0, 1], "段落失配不得推进游标");
        assert_eq!(matches[0].located_cell_count, 1);
        assert_eq!(matches[0].locatable_cell_count, 1);
        assert_eq!(next, 2, "1/1 = 1.0 ≥ 0.5：表格消费（段落本身不推进 matched_upto）");
    }

    #[test]
    fn unmatched_cell_below_threshold_yields_none() {
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>五行文字</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        // 页面上只有 1 个字符能对上（1/4 = 25% < 60%）
        let chars = vec![ch('五', 10.0, 10.0), ch('x', 30.0, 10.0), ch('无', 50.0, 10.0), ch('关', 70.0, 10.0)];
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
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
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
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
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
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
        // （格文字用 2 字：1 字格按规则不参与匹配）
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid>
               <w:tr><w:tc><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙格</w:t></w:r></w:p></w:tc></w:tr>
               <w:tr><w:tc><w:p><w:r><w:t>丙格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![
            ch('甲', 100.0, 100.0), ch('格', 112.0, 100.0), ch('乙', 300.0, 100.0), ch('格', 312.0, 100.0),
            ch('丙', 100.0, 200.0), ch('格', 112.0, 200.0),
        ];
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
        let rects = infer_cell_rects(&views[0], &matches[0], &chars);
        assert_eq!(rects.len(), 4);
        // 甲格：命中字符 bbox（+2px pad；两字横排宽 22px → w=26）→ x≈98, y≈98
        let r0 = rects[0].unwrap();
        assert_eq!((r0.x, r0.y, r0.w, r0.h), (98, 98, 26, 14));
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
        // 第二行无命中 → 该行空格无法推算（第一行用 2 字格保证可定位出列带）
        let stream = table_stream_xml(
            r#"<w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid>
               <w:tr><w:tc><w:p><w:r><w:t>甲格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙格</w:t></w:r></w:p></w:tc></w:tr>
               <w:tr><w:tc><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        );
        let views = build_table_views(&stream);
        let chars = vec![
            ch('甲', 100.0, 100.0), ch('格', 112.0, 100.0),
            ch('乙', 300.0, 100.0), ch('格', 312.0, 100.0),
        ];
        let (matches, _) = locate_tables_on_page(&stream, &views, &chars, 0);
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

    #[test]
    fn colored_run_maps_to_cell_highlight() {
        // 用户用 w:color（字体颜色）而非 w:highlight 标记表格格：
        // 非黑彩色 run 回退映射为标准高亮色名（角色配色复用高亮管线）；
        // 黑色是正文默认色，不构成标记。
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:rPr><w:color w:val="92D050"/></w:rPr><w:t>你好</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:rPr><w:color w:val="0000FF"/></w:rPr><w:t>我知道了</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:rPr><w:color w:val="000000"/></w:rPr><w:t>普通黑字</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        let stream = crate::core::docx_io::parse_body_stream_for_test(xml, &mut registry).unwrap();
        let views = build_table_views(&stream);
        assert_eq!(views[0].rows[0].cells[0].highlight.as_deref(), Some("green"), "92D050 绿字 → green");
        assert_eq!(views[0].rows[0].cells[1].highlight.as_deref(), Some("blue"), "0000FF 蓝字 → blue");
        assert_eq!(views[0].rows[0].cells[2].highlight, None, "黑色 w:color 不映射");
    }
}
