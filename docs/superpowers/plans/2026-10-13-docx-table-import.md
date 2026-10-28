# docx 表格识别导入（单元格 TextRegion）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 导入 .docx 文档底图时，识别 Word 表格：表格线保留在底图中，每个单元格的文字提取为可编辑的 TextRegion（手写体渲染，支持逐格/整表修改字体）。

**Architecture:** .docx → Word/LibreOffice 转 PDF → pdfium 栅格化为 200 DPI 背景图（现有链路）。新增：解析 document.xml 的 `w:tbl` 结构得到表格/单元格文本（docx_io），在每页的 PDF 字符流上按文档顺序匹配单元格文本得到像素包围盒（docx_tables，纯函数、可单测），按字符包围盒擦除原印刷文字（保留表格线），生成单元格 TextRegion（doc_render）。高亮/标签区域识别与表格区域互斥排重。

**Tech Stack:** Rust（quick-xml 流式解析、zip、image、pdfium-render），Vue 3 + TypeScript 前端。

## Global Constraints

- 工作分支：`feature/docx-table-import`（已创建，勿再建分支）。
- 不新增任何外部依赖（quick-xml/zip/image 均已在 `crates/core/Cargo.toml`）。
- 核心库 `crates/core` 保持纯 Rust 无 GUI 依赖；新模块经 `crates/core/src/core/mod.rs` 注册。
- 所有注释、错误信息、测试名用中文，与现有代码风格一致（quick-xml 手写状态机风格，参照 `docx_io.rs::parse_document`）。
- serde 新字段必须 `#[serde(default)]`（旧预设 JSON 兼容）。
- 测试命令：`cargo nextest run -p handwrite-sim`（在仓库根执行，**不要 `cd`**，本机 shell 的 `cd` 会触发 fnm 报错；用绝对路径或直接相对仓库根）。前端类型检查：`npx vue-tsc --noEmit`。
- 每个任务结束：全部测试绿 + 规范化提交（`feat(tables): …` / `refactor(docx): …`，中文描述）。`git add` 只加本任务明确列出的文件（工作区有 `src-tauri/Cargo.toml` 的换行符噪音改动，不要提交它）。
- 依赖 pdfium 的测试必须保留"pdfium 不可用则跳过"模式（参照 `doc_render.rs` 现有测试 `pdf_to_images_renders_all_pages`）。本计划的自动化测试全部落在纯函数层（合成 `ExtractedChar`/XML 字符串），不依赖 pdfium。

---

### Task 1: docx_io 解析 body 元素流（段落 + 表格结构）

把 `parse_document` 重构为 `parse_body_stream`：输出按文档顺序交错的段落与表格。表格解析出 `tblGrid` 列数、每行每格的 runs/格式/`gridSpan`/`vMerge`/列号。`load_paragraphs` 过滤出段落元素，行为不变（现有全部测试守卫）。

**Files:**
- Modify: `crates/core/src/core/docx_io.rs`（`parse_document` → `parse_body_stream`；`load_paragraphs` 改调用方式；新增 `BodyElement`/`ParsedTable`/`ParsedRow`/`ParsedCell`/`CellVMerge`；`ParsedParagraph`/`ParaFmt` 改 `pub(crate)`；新增 `pub fn load_body_stream`）
- 不改 `mod.rs`（无新文件）。

**Interfaces:**
- Produces（后续任务依赖，全部在 `docx_io` 模块）:
  - `pub(crate) enum BodyElement { Paragraph(ParsedParagraph), Table(ParsedTable) }`
  - `pub(crate) struct ParsedParagraph { pub(crate) runs: Vec<TextRun>, pub(crate) fmt: ParaFmt }`（原私有结构提权）
  - `pub(crate) struct ParaFmt`（原私有结构及其字段提权为 `pub(crate)`）
  - `pub(crate) struct ParsedTable { pub(crate) grid_cols: usize, pub(crate) rows: Vec<ParsedRow> }`
  - `pub(crate) struct ParsedRow { pub(crate) cells: Vec<ParsedCell> }`
  - `pub(crate) struct ParsedCell { pub(crate) runs: Vec<TextRun>, pub(crate) fmt: ParaFmt, pub(crate) grid_span: usize, pub(crate) v_merge: CellVMerge, pub(crate) col_start: usize }`
  - `pub(crate) enum CellVMerge { None, Restart, Continue }`（`#[default] None`，`Copy`）
  - `fn parse_body_stream(xml: &str, registry: &mut StyleRegistry) -> Result<Vec<BodyElement>, String>`
  - `pub fn load_body_stream(path: &Path) -> Result<Vec<BodyElement>, String>`（读 zip → document.xml → 全新 registry 解析）

- [ ] **Step 1: 写失败测试**（追加到 `docx_io.rs` 的 `mod tests`）

```rust
    #[test]
    fn body_stream_parses_table_structure() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>表前段落</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>合并格</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>丙</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t></w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>丁</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>表后段落</w:t></w:r></w:p></w:body></w:document>"#;
        let mut registry = StyleRegistry::new();
        let stream = parse_body_stream(document_xml, &mut registry).unwrap();
        assert_eq!(stream.len(), 3);
        assert!(matches!(&stream[0], BodyElement::Paragraph(p) if p.runs[0].text == "表前段落"));
        assert!(matches!(&stream[2], BodyElement::Paragraph(p) if p.runs[0].text == "表后段落"));
        let table = match &stream[1] { BodyElement::Table(t) => t, _ => panic!("应为表格") };
        assert_eq!(table.grid_cols, 3);
        assert_eq!(table.rows.len(), 2);
        // 第一行：跨 2 列的 vMerge 起始格 + 单格
        let c00 = &table.rows[0].cells[0];
        assert_eq!(c00.col_start, 0);
        assert_eq!(c00.grid_span, 2);
        assert_eq!(c00.v_merge, CellVMerge::Restart);
        assert_eq!(c00.runs[0].text, "合并格");
        let c01 = &table.rows[0].cells[1];
        assert_eq!(c01.col_start, 2);
        assert_eq!(c01.grid_span, 1);
        // 第二行：vMerge 续行格占列但无内容 + 单格
        let c10 = &table.rows[1].cells[0];
        assert_eq!(c10.col_start, 0);
        assert_eq!(c10.grid_span, 2);
        assert_eq!(c10.v_merge, CellVMerge::Continue);
        let c11 = &table.rows[1].cells[1];
        assert_eq!(c11.col_start, 2);
        assert_eq!(c11.runs[0].text, "丁");
    }

    #[test]
    fn load_body_stream_reads_docx_file() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>格</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let stream = load_body_stream(&path).unwrap();
        assert_eq!(stream.len(), 1);
        assert!(matches!(&stream[0], BodyElement::Table(_)));
    }

    #[test]
    fn nested_table_content_not_leaked_into_outer_cell() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>外层</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>内层</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut registry = StyleRegistry::new();
        let stream = parse_body_stream(document_xml, &mut registry).unwrap();
        let table = match &stream[0] { BodyElement::Table(t) => t, _ => panic!() };
        let text: String = table.rows[0].cells[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "外层");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim docx_io`
Expected: 编译失败（`parse_body_stream`/`BodyElement` 未定义）。

- [ ] **Step 3: 实现**

在 `docx_io.rs` 顶部类型区（`ParsedParagraph` 定义附近）新增/提权：

```rust
/// body 元素流：段落与表格按文档顺序交错（供表格识别使用）。
#[derive(Debug)]
pub(crate) enum BodyElement {
    Paragraph(ParsedParagraph),
    Table(ParsedTable),
}

/// 顶层表格解析结果（原始结构，未做标签拆分）。
#[derive(Debug, Default)]
pub(crate) struct ParsedTable {
    /// tblGrid 列数（缺 tblGrid 时按首行 gridSpan 总和回填）。
    pub(crate) grid_cols: usize,
    pub(crate) rows: Vec<ParsedRow>,
}

#[derive(Debug, Default)]
pub(crate) struct ParsedRow {
    pub(crate) cells: Vec<ParsedCell>,
}

/// 单元格垂直合并状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CellVMerge {
    #[default]
    None,
    /// 合并起始格（内容在此格）。
    Restart,
    /// 合并续行格（无内容，但占用列）。
    Continue,
}

/// 表格单元格（所有段落 runs 拼接，段落间以 "\n" run 分隔）。
#[derive(Debug)]
pub(crate) struct ParsedCell {
    pub(crate) runs: Vec<TextRun>,
    /// 首段直接格式（jc 用于对齐、run_sz_half_pt 用于字号回退）。
    pub(crate) fmt: ParaFmt,
    /// w:gridSpan 跨列数（默认 1）。
    pub(crate) grid_span: usize,
    pub(crate) v_merge: CellVMerge,
    /// 起始列号（0 基，按行内前序单元格宽度累计推算）。
    pub(crate) col_start: usize,
}

impl Default for ParsedCell {
    fn default() -> Self {
        Self { runs: Vec::new(), fmt: ParaFmt::default(), grid_span: 1, v_merge: CellVMerge::None, col_start: 0 }
    }
}
```

`ParsedParagraph` 与 `ParaFmt`（含全部字段）从私有改为 `pub(crate) struct` + `pub(crate)` 字段。

把 `fn parse_document` 整体替换为 `fn parse_body_stream`（函数体骨架如下，run 累积机制沿用原实现，只是段落判定同时覆盖 body 段落与单元格段落）：

```rust
/// 解析 document.xml，返回 body 元素流（段落与表格按文档顺序交错）。
fn parse_body_stream(xml: &str, registry: &mut StyleRegistry) -> Result<Vec<BodyElement>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut elements = Vec::new();
    // ---- 段落状态（body 段落与单元格段落共用 run 累积机制）----
    let mut in_body_para = false; // 当前 w:p 是 body 直接子级
    let mut in_cell_para = false; // 当前 w:p 位于顶层表格单元格内
    let mut tbl_depth: usize = 0; // 表格嵌套深度（>=2 的内层表格内容忽略）
    let mut txbx_depth: usize = 0;
    let mut in_p_pr = false;
    let mut in_run = false;
    let mut in_r_pr = false;
    let mut in_t = false;

    let mut cur_runs: Vec<TextRun> = Vec::new();
    let mut cur_run_text = String::new();
    let mut cur_run_props = RawRunProps::default();
    let mut cur_fmt = ParaFmt::default();

    // ---- 表格状态 ----
    let mut cur_table: Option<ParsedTable> = None;
    let mut cur_row: Option<ParsedRow> = None;
    let mut cur_cell: Option<ParsedCell> = None;
    let mut row_col_cursor: usize = 0; // 当前行已占用列数
    let mut in_tbl_grid = false; // 顶层 tblGrid 内
    let mut in_tc_pr = false; // 当前单元格 tcPr 内
    let mut cell_inner_tbl: usize = 0; // 单元格内嵌套表格深度（>0 忽略其文本）

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local_name(qname.as_ref());
                if txbx_depth > 0 {
                    if name == b"txbxContent" {
                        txbx_depth += 1;
                    }
                    continue;
                }
                match name {
                    b"tbl" => {
                        tbl_depth += 1;
                        if tbl_depth == 1 {
                            cur_table = Some(ParsedTable::default());
                            row_col_cursor = 0;
                        } else if tbl_depth == 2 {
                            cell_inner_tbl += 1;
                        }
                    }
                    b"tblGrid" if tbl_depth == 1 && cur_row.is_none() => in_tbl_grid = true,
                    b"gridCol" if in_tbl_grid => {
                        if let Some(t) = cur_table.as_mut() {
                            t.grid_cols += 1;
                        }
                    }
                    b"tr" if tbl_depth == 1 => {
                        cur_row = Some(ParsedRow::default());
                        row_col_cursor = 0;
                    }
                    b"tc" if tbl_depth == 1 && cur_row.is_some() => {
                        cur_cell = Some(ParsedCell { col_start: row_col_cursor, ..ParsedCell::default() });
                        cell_inner_tbl = 0;
                    }
                    b"tcPr" if cur_cell.is_some() => in_tc_pr = true,
                    b"gridSpan" if in_tc_pr => {
                        if let Some(v) = attr_val(&e, "val").and_then(|v| v.parse::<usize>().ok()) {
                            if let Some(c) = cur_cell.as_mut() {
                                c.grid_span = v.max(1);
                            }
                        }
                    }
                    b"vMerge" if in_tc_pr => {
                        let restart = attr_val(&e, "val").is_some_and(|v| v == "restart");
                        if let Some(c) = cur_cell.as_mut() {
                            c.v_merge = if restart { CellVMerge::Restart } else { CellVMerge::Continue };
                        }
                    }
                    b"p" if tbl_depth == 0 => {
                        in_body_para = true;
                        cur_runs.clear();
                        cur_run_text.clear();
                        cur_run_props = RawRunProps::default();
                        cur_fmt = ParaFmt::default();
                    }
                    b"p" if tbl_depth == 1 && cur_cell.is_some() && cell_inner_tbl == 0 => {
                        in_cell_para = true;
                        cur_runs.clear();
                        cur_run_text.clear();
                        cur_run_props = RawRunProps::default();
                        cur_fmt = ParaFmt::default();
                    }
                    b"pPr" if in_body_para || in_cell_para => {
                        in_p_pr = true;
                        handle_p_pr_tag(&e, &mut cur_fmt);
                    }
                    b"r" if in_body_para || in_cell_para => {
                        flush_pending_run(&mut cur_run_text, &mut cur_run_props, &cur_fmt, &mut cur_runs, registry);
                        in_run = true;
                    }
                    b"rPr" if in_run => {
                        in_r_pr = true;
                        handle_r_pr_tag(&e, &mut cur_run_props, &mut cur_fmt);
                    }
                    b"t" if in_body_para || in_cell_para => in_t = true,
                    _ => {
                        if in_p_pr {
                            handle_p_pr_tag(&e, &mut cur_fmt);
                        } else if in_r_pr {
                            handle_r_pr_tag(&e, &mut cur_run_props, &mut cur_fmt);
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                if txbx_depth > 0 {
                    continue;
                }
                match local_name(e.name().as_ref()) {
                    b"tab" if in_body_para || in_cell_para => cur_run_text.push('\t'),
                    b"br" | b"cr" if in_body_para || in_cell_para => cur_run_text.push('\n'),
                    b"gridSpan" if in_tc_pr => {
                        if let Some(v) = attr_val(&e, "val").and_then(|v| v.parse::<usize>().ok()) {
                            if let Some(c) = cur_cell.as_mut() {
                                c.grid_span = v.max(1);
                            }
                        }
                    }
                    b"vMerge" if in_tc_pr => {
                        let restart = attr_val(&e, "val").is_some_and(|v| v == "restart");
                        if let Some(c) = cur_cell.as_mut() {
                            c.v_merge = if restart { CellVMerge::Restart } else { CellVMerge::Continue };
                        }
                    }
                    _ => {
                        if in_p_pr {
                            handle_p_pr_tag(&e, &mut cur_fmt);
                        } else if in_r_pr {
                            handle_r_pr_tag(&e, &mut cur_run_props, &mut cur_fmt);
                        }
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if txbx_depth > 0 || cell_inner_tbl > 0 || !((in_body_para || in_cell_para) && in_t) {
                    continue;
                }
                if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                    if let Ok(v) = unescape(s) {
                        cur_run_text.push_str(&v);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local_name(qname.as_ref());
                if txbx_depth > 0 {
                    if name == b"txbxContent" {
                        txbx_depth -= 1;
                    }
                    continue;
                }
                match name {
                    b"tbl" => {
                        if tbl_depth >= 2 && cell_inner_tbl > 0 {
                            cell_inner_tbl -= 1;
                        }
                        tbl_depth = tbl_depth.saturating_sub(1);
                        if tbl_depth == 0 {
                            if let Some(mut t) = cur_table.take() {
                                if t.grid_cols == 0 {
                                    t.grid_cols = t
                                        .rows
                                        .first()
                                        .map(|r| r.cells.iter().map(|c| c.grid_span).sum())
                                        .unwrap_or(0);
                                }
                                elements.push(BodyElement::Table(t));
                            }
                            cur_row = None;
                            cur_cell = None;
                            in_tbl_grid = false;
                        }
                    }
                    b"tblGrid" => in_tbl_grid = false,
                    b"tr" if tbl_depth == 1 => {
                        if let Some(row) = cur_row.take() {
                            if let Some(t) = cur_table.as_mut() {
                                t.rows.push(row);
                            }
                        }
                    }
                    b"tc" if tbl_depth == 1 && cur_cell.is_some() => {
                        if let Some(cell) = cur_cell.take() {
                            if let Some(row) = cur_row.as_mut() {
                                row_col_cursor += cell.grid_span;
                                row.cells.push(cell);
                            }
                        }
                        in_tc_pr = false;
                    }
                    b"tcPr" => in_tc_pr = false,
                    b"t" => in_t = false,
                    b"rPr" => in_r_pr = false,
                    b"r" => {
                        flush_pending_run(&mut cur_run_text, &mut cur_run_props, &cur_fmt, &mut cur_runs, registry);
                        in_run = false;
                        in_r_pr = false;
                        cur_run_props = RawRunProps::default();
                    }
                    b"pPr" => in_p_pr = false,
                    b"p" if in_body_para => {
                        flush_pending_run(&mut cur_run_text, &mut cur_run_props, &cur_fmt, &mut cur_runs, registry);
                        elements.push(BodyElement::Paragraph(ParsedParagraph {
                            runs: std::mem::take(&mut cur_runs),
                            fmt: std::mem::take(&mut cur_fmt),
                        }));
                        in_body_para = false;
                        in_p_pr = false;
                        in_run = false;
                        in_r_pr = false;
                        in_t = false;
                    }
                    b"p" if in_cell_para => {
                        flush_pending_run(&mut cur_run_text, &mut cur_run_props, &cur_fmt, &mut cur_runs, registry);
                        if let Some(cell) = cur_cell.as_mut() {
                            if cell.runs.is_empty() {
                                cell.fmt = cur_fmt.clone();
                            }
                            if !cur_runs.is_empty() {
                                if !cell.runs.is_empty() {
                                    cell.runs.push(TextRun::from_text("\n"));
                                }
                                cell.runs.extend(cur_runs.drain(..));
                            }
                        }
                        in_cell_para = false;
                        in_p_pr = false;
                        in_run = false;
                        in_r_pr = false;
                        in_t = false;
                        cur_run_props = RawRunProps::default();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("解析 document.xml 失败：{e}")),
            _ => {}
        }
    }
    Ok(elements)
}
```

注意：原 `parse_document` 中 run 结束/段落结束处的 flush 逻辑重复 3 处，抽出公共函数（放在 `read_ind_attrs` 附近）：

```rust
/// 把累积中的 run 文本落为 TextRun（run 开始/结束、段落结束时调用）。
fn flush_pending_run(
    cur_run_text: &mut String,
    cur_run_props: &mut RawRunProps,
    cur_fmt: &ParaFmt,
    cur_runs: &mut Vec<TextRun>,
    registry: &mut StyleRegistry,
) {
    if cur_run_text.is_empty() {
        return;
    }
    let mut style = TextRunStyle::default();
    if cur_run_props.font_family.is_none() && cur_fmt.font_family.is_some() {
        cur_run_props.font_family = cur_fmt.font_family.clone();
    }
    cur_run_props.apply_to(&mut style, registry);
    cur_runs.push(TextRun::new(std::mem::take(cur_run_text), style));
}
```

`load_paragraphs` 中 `let parsed_paras = parse_document(&document_xml, &mut registry)?;` 改为：

```rust
    let parsed_stream = parse_body_stream(&document_xml, &mut registry)?;
    let parsed_paras: Vec<ParsedParagraph> = parsed_stream
        .into_iter()
        .filter_map(|el| match el {
            BodyElement::Paragraph(p) => Some(p),
            BodyElement::Table(_) => None,
        })
        .collect();
```

文件底部新增公开入口（复用 `read_entry`）：

```rust
/// 读取 docx 文件并解析 body 元素流（表格识别用；样式链不参与，段落仅用于匹配消耗）。
pub fn load_body_stream(path: &Path) -> Result<Vec<BodyElement>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 docx {path:?} 失败：{e}"))?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|e| format!("解压 docx {path:?} 失败：{e}"))?;
    let document_xml = read_entry(&mut archive, "word/document.xml")?;
    let mut registry = StyleRegistry::new();
    parse_body_stream(&document_xml, &mut registry)
}
```

注意：`BodyElement`/`ParsedTable` 等为 `pub(crate)`，而 `load_body_stream` 是 `pub fn` 返回 `Vec<BodyElement>` —— 编译器会报 private-in-public 警告/错误（crate 内使用没问题，但 pub fn 暴露 pub(crate) 类型不行）。解决：`load_body_stream` 也声明为 `pub(crate) fn`（doc_render 在同 crate 内调用）。

- [ ] **Step 4: 运行全部 docx_io 测试**

Run: `cargo nextest run -p handwrite-sim`
Expected: 全绿（现有 `skips_paragraphs_inside_tables`、缩进/高亮/hyperlink/txbx 等测试不回归；新增 3 个测试通过）。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/docx_io.rs
git commit -m "refactor(docx): 解析 body 元素流，表格结构（gridSpan/vMerge/列号）入库"
```

---

### Task 2: docx_tables 模块——表格视图构造

新建 `docx_tables.rs`：从 body 元素流构造"表格视图"（每格清理后的文本、列信息、对齐、印刷体判定、高亮色）。

**Files:**
- Create: `crates/core/src/core/docx_tables.rs`
- Modify: `crates/core/src/core/mod.rs`（加 `pub mod docx_tables;`）

**Interfaces:**
- Consumes: `docx_io::{BodyElement, ParsedTable, CellVMerge}`、`doc_render::strip_tag_syntax`、`docx_io::is_gray_highlight`
- Produces:
  - `pub struct TableView { pub index: usize, pub rows: Vec<TableRowView> }`
  - `pub struct TableRowView { pub cells: Vec<TableCellView> }`
  - `pub struct TableCellView { pub text: String, pub col_start: usize, pub col_span: usize, pub v_merge: CellVMerge, pub align: i32, pub sz_half_pt: Option<u32>, pub printed: bool, pub skip_printed: bool, pub highlight: Option<String> }`
  - `pub fn build_table_views(stream: &[BodyElement]) -> Vec<TableView>`

- [ ] **Step 1: 写失败测试**（新文件底部 `mod tests`）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::docx_io::{BodyElement, CellVMerge, ParsedCell, ParsedFmt as _};

    fn cell(runs_text: &str, col_start: usize, span: usize) -> crate::core::docx_io::ParsedCell {
        crate::core::docx_io::ParsedCell {
            runs: vec![crate::core::models::TextRun::from_text(runs_text)],
            ..crate::core::docx_io::ParsedCell::with_pos(col_start, span)
        }
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
        assert!(c0.printed);       // {{打印:}} → 印刷体区域
        assert!(!c0.skip_printed);
        let c1 = &views[0].rows[0].cells[1];
        assert!(c1.skip_printed);  // 灰高亮 → 整格保留印刷底图
        assert!(!c1.printed);
    }
}
```

说明：测试需要从 crate 外触发 `parse_body_stream`（它是私有 fn）。在 `docx_io.rs` 追加测试专用透传（紧跟 `parse_body_stream` 之后）：

```rust
/// 供 docx_tables 单元测试触发解析（crate 内可见）。
#[cfg(test)]
pub(crate) fn parse_body_stream_for_test(xml: &str, registry: &mut StyleRegistry) -> Result<Vec<BodyElement>, String> {
    parse_body_stream(xml, registry)
}
```

同时测试里的 `ParsedCell::with_pos` 不存在——删掉对它的引用，测试只经 XML 走真实解析（上面两个测试已足够，不需要手工构造 ParsedCell；把 `fn cell(...)` 辅助与那行 `use ... ParsedFmt as _` 从测试中删除，保留两个 XML 测试）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim docx_tables`
Expected: 编译失败（模块不存在）。

- [ ] **Step 3: 实现** `crates/core/src/core/docx_tables.rs`

```rust
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
    TableCellView {
        text,
        col_start: c.col_start,
        col_span: c.grid_span,
        v_merge: c.v_merge,
        align,
        sz_half_pt: c.fmt.run_sz_half_pt,
        printed: printed_marker && !text.is_empty(),
        skip_printed: has_gray,
        highlight,
    }
}
```

`mod.rs` 加 `pub mod docx_tables;`。若 `strip_tag_syntax`/`is_gray_highlight` 跨模块可见性报错，二者在 doc_render/docx_io 中已是 `pub fn`，直接可用。

- [ ] **Step 4: 运行测试**

Run: `cargo nextest run -p handwrite-sim`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/docx_tables.rs crates/core/src/core/mod.rs crates/core/src/core/docx_io.rs
git commit -m "feat(tables): 表格视图构造（标签清理/印刷判定/高亮色提取）"
```

---

### Task 3: 字符流匹配——按文档顺序定位单元格

在单页 PDF 字符流（`ExtractedChar` 列表）上，用"归一化子序列两指针"按文档顺序消费：正文段落尽力消耗字符，表格单元格按 60% 命中率判定定位并返回命中字符下标。

**Files:**
- Modify: `crates/core/src/core/docx_tables.rs`
- Consumes: `doc_render::ExtractedChar`（`pub` 已存在）

**Interfaces:**
- Produces:
  - `pub struct CellMatch { pub char_idxs: Vec<usize> }`
  - `pub struct TablePageMatch { pub table_index: usize, pub row_matches: Vec<Vec<Option<CellMatch>>> }`（与 `TableView.rows` 行列对齐）
  - `pub fn locate_tables_on_page(stream: &[BodyElement], views: &[TableView], chars: &[ExtractedChar]) -> Vec<TablePageMatch>`

- [ ] **Step 1: 写失败测试**（追加到 `docx_tables.rs` 的 `mod tests`；测试辅助构造 `ExtractedChar`）

```rust
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
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body">{body}</w:body></w:document>"#
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
        let chars = vec![ch('五', 10.0, 10.0), ch(' zz', 30.0, 10.0), ch('无', 50.0, 10.0), ch('关', 70.0, 10.0)];
        let matches = locate_tables_on_page(&stream, &views, &chars);
        assert!(matches[0].row_matches[0][0].is_none());
    }
```

（第三个测试里 `' zz'` 是笔误——应为单字符 `ch('x', 30.0, 10.0)`，实现时直接写 `ch('x', 30.0, 10.0)`。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim docx_tables`
Expected: 编译失败（`locate_tables_on_page` 未定义）。

- [ ] **Step 3: 实现**（追加到 `docx_tables.rs`）

```rust
use crate::core::doc_render::ExtractedChar;

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
    fn match_cell(&mut self, target: &[char]) -> Option<CellMatch> {
        if target.is_empty() {
            return None;
        }
        let hit = self.try_match(target, CELL_WINDOW, false);
        if hit.len() as f32 / target.len() as f32 >= CELL_MATCH_RATIO {
            Some(CellMatch { char_idxs: hit })
        } else {
            None
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
```

- [ ] **Step 4: 运行测试**

Run: `cargo nextest run -p handwrite-sim`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/docx_tables.rs
git commit -m "feat(tables): 页面字符流按文档顺序匹配单元格（60% 命中阈值）"
```

---

### Task 4: 空单元格矩形推断（行/列带）

已定位单元格由命中字符包围盒得到矩形；未命中/空单元格用同表已定位格的行列带均值推算。

**Files:**
- Modify: `crates/core/src/core/docx_tables.rs`

**Interfaces:**
- Consumes: Task 3 的 `TablePageMatch`/`CellMatch`
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub struct RectPx { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }`
  - `pub fn infer_cell_rects(view: &TableView, m: &TablePageMatch, chars: &[ExtractedChar]) -> Vec<Option<RectPx>>`（行主序扁平，与 `view.rows.iter().flat_map(cells)` 对齐）

- [ ] **Step 1: 写失败测试**

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim docx_tables`
Expected: 编译失败（`infer_cell_rects` 未定义）。

- [ ] **Step 3: 实现**（追加到 `docx_tables.rs`）

```rust
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

/// 推算整表所有单元格矩形（行主序扁平输出）。
/// 已定位格 = 命中字符 bbox（pad=2）；未命中格 = 行/列带交集内缩 3px；
/// vMerge 续行格与无法推算的格为 None。
pub fn infer_cell_rects(view: &TableView, m: &TablePageMatch, chars: &[ExtractedChar]) -> Vec<Option<RectPx>> {
    // 1. 已定位格 bbox
    let located: Vec<(usize /*行*/, usize /*列*/, RectPx)> = view
        .rows
        .iter()
        .zip(m.row_matches.iter())
        .flat_map(|(row, row_m)| row.cells.iter().zip(row_m.iter()))
        .filter(|(cell, _)| cell.v_merge != CellVMerge::Continue)
        .map(|(cell, cell_m)| (cell, cell_m.as_ref().map(|cm| union_char_bbox(chars, &cm.char_idxs, 2))))
        .filter_map(|(cell, rect)| rect.map(|r| (cell, r)))
        .map(|(cell, r)| (cell_rows_col(view, cell) /*占位，见下*/, r))
        .collect();
    // ↑ 上面这个 map 需要行列号——改为直接在 zip 时携带下标，完整实现如下。
    todo_in_plan_see_below()
}
```

上面写复杂了——**实现时按下面这个完整版本写**（不要用占位版本）：

```rust
/// 推算整表所有单元格矩形（行主序扁平输出，与 view.rows 迭代顺序对齐）。
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
                .map(|(cell, cell_m)| {
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

fn view_max_cols(view: &TableView) -> usize {
    view.rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.col_start + c.col_span).max().unwrap_or(0))
        .max()
        .unwrap_or(0)
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo nextest run -p handwrite-sim`
Expected: 全绿（`infers_missing_cell_rect_from_bands` 里 r3 推算值依赖 mean 逻辑，若断言区间不满足按实际值微调断言，但必须保持 x∈[296,302]、y∈[196,202] 量级）。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/docx_tables.rs
git commit -m "feat(tables): 空单元格行列带矩形推断"
```

---

### Task 5: doc_render 字符级擦除与高亮像素过滤擦除

擦单元格文字但不能擦掉表格线：按字符包围盒擦白；表格内高亮底色逐像素替换（黑色表格线不是高亮像素，自然保留）。

**Files:**
- Modify: `crates/core/src/core/doc_render.rs`

**Interfaces:**
- Consumes: `docx_tables::RectPx`（Task 4）
- Produces:
  - `pub(crate) fn erase_char_boxes(img: &mut image::RgbImage, chars: &[ExtractedChar], idxs: &[usize], pad: i32)`
  - `pub(crate) fn erase_highlight_pixels_in_boxes(img: &mut image::RgbImage, boxes: &[BoundingBox])`
  - `fn rect_contains_box(r: &RectPx, b: &BoundingBox) -> bool`

- [ ] **Step 1: 写失败测试**（追加到 `doc_render.rs` 的 `mod tests`）

```rust
    #[test]
    fn erase_char_boxes_keeps_table_lines() {
        // 300x200 白图；x=150 处竖直黑色表格线；(60..100) 区域有文字字形
        let mut img = image::RgbImage::from_pixel(300, 200, image::Rgb([255, 255, 255]));
        for y in 20..180 {
            img.put_pixel(150, y, image::Rgb([0, 0, 0]));
        }
        for y in 60..80 {
            for x in 60..100 {
                img.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }
        let chars = [ExtractedChar {
            ch: '字',
            min_x: 60.0,
            min_y: 60.0,
            max_x: 100.0,
            max_y: 80.0,
            font_size_pt: 12.0,
            glyph_h_pt: 0.0,
        }];
        erase_char_boxes(&mut img, &chars, &[0], 2);
        // 字形区域被擦白
        assert_eq!(img.get_pixel(80, 70), &image::Rgb([255, 255, 255]));
        // 表格线保留
        assert_eq!(img.get_pixel(150, 70), &image::Rgb([0, 0, 0]));
    }

    #[test]
    fn erase_highlight_pixels_in_boxes_keeps_black_lines() {
        let mut img = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        for y in 10..90 {
            for x in 20..80 {
                img.put_pixel(x, y, image::Rgb([255, 255, 0])); // 黄色高亮底
            }
            img.put_pixel(50, y, image::Rgb([0, 0, 0])); // 穿过的黑色竖线
        }
        let boxes = [BoundingBox::with_highlight(20, 10, 79, 89, "yellow")];
        erase_highlight_pixels_in_boxes(&mut img, &boxes);
        assert_eq!(img.get_pixel(30, 30), &image::Rgb([255, 255, 255])); // 高亮已清
        assert_eq!(img.get_pixel(50, 30), &image::Rgb([0, 0, 0])); // 黑线保留
    }

    #[test]
    fn rect_contains_box_checks_containment() {
        let r = docx_tables::RectPx { x: 10, y: 10, w: 100, h: 100 };
        assert!(rect_contains_box(&r, &BoundingBox::new(20, 20, 60, 60)));
        assert!(!rect_contains_box(&r, &BoundingBox::new(5, 20, 60, 60))); // 越出左界
        assert!(!rect_contains_box(&r, &BoundingBox::new(20, 20, 120, 60))); // 越出右界
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim doc_render`
Expected: 编译失败（函数未定义）。

- [ ] **Step 3: 实现**（放在 `erase_highlight_boxes` 之后）

```rust
/// 按字符包围盒擦白（glyph 级擦除：只清字形矩形，保留穿过区域的表格线）。
pub(crate) fn erase_char_boxes(
    img: &mut image::RgbImage,
    chars: &[ExtractedChar],
    idxs: &[usize],
    pad: i32,
) {
    let w = img.width() as i32;
    let h = img.height() as i32;
    for &i in idxs {
        let c = match chars.get(i) {
            Some(c) => c,
            None => continue,
        };
        let x0 = (c.min_x.floor() as i32 - pad).max(0);
        let x1 = ((c.max_x.ceil() as i32) + pad).min(w - 1);
        let y0 = (c.min_y.floor() as i32 - pad).max(0);
        let y1 = ((c.max_y.ceil() as i32) + pad).min(h - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                img.put_pixel(x as u32, y as u32, image::Rgb([255, 255, 255]));
            }
        }
    }
}

/// 将包围盒内的高亮像素逐像素替换为白色（不整框涂白，保留黑色表格线）。
pub(crate) fn erase_highlight_pixels_in_boxes(img: &mut image::RgbImage, boxes: &[BoundingBox]) {
    let w = img.width();
    let h = img.height();
    for b in boxes {
        let x0 = b.min_x.min(w.saturating_sub(1));
        let x1 = b.max_x.min(w.saturating_sub(1));
        let y0 = b.min_y.min(h.saturating_sub(1));
        let y1 = b.max_y.min(h.saturating_sub(1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = img.get_pixel(x, y);
                if is_highlight_pixel(p[0], p[1], p[2]) {
                    img.put_pixel(x, y, image::Rgb([255, 255, 255]));
                }
            }
        }
    }
}

/// BoundingBox 是否完整落在 RectPx 内（2px 容差，容纳包围盒贴合误差）。
fn rect_contains_box(r: &crate::core::docx_tables::RectPx, b: &BoundingBox) -> bool {
    let rx1 = r.x + r.w;
    let ry1 = r.y + r.h;
    b.min_x as i32 >= r.x - 2
        && b.max_x as i32 <= rx1 + 2
        && b.min_y as i32 >= r.y - 2
        && b.max_y as i32 <= ry1 + 2
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo nextest run -p handwrite-sim`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/doc_render.rs
git commit -m "feat(tables): 字符级擦除与表格内高亮像素过滤擦除（保表格线）"
```

---

### Task 6: 单页表格区域生成 process_tables_on_page

把定位结果变成 `TextRegion` 列表：擦字、建区域（字号优先用命中字符校准值）、共享角色映射（高亮色 → role_id，与高亮框路径同池）。

**Files:**
- Modify: `crates/core/src/core/doc_render.rs`

**Interfaces:**
- Consumes: Task 2/3/4 全部 API + Task 5 擦除函数 + `resolve_font_size_px`（本文件已有）
- Produces:
  - `pub(crate) struct TablePageResult { pub regions: Vec<TextRegion>, pub cell_rects: Vec<RectPx>, pub outer_rects: Vec<RectPx> }`
  - `pub(crate) fn process_tables_on_page(stream: &[BodyElement], views: &[TableView], chars: &[ExtractedChar], page_num: i32, dpi: u32, color_map: &mut std::collections::HashMap<String, u32>, next_role_id: &mut u32, img: &mut image::RgbImage) -> TablePageResult`

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn process_tables_builds_regions_and_erases() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>姓名</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>张三</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        let stream = crate::core::docx_io::parse_body_stream_for_test(xml, &mut registry).unwrap();
        let views = crate::core::docx_tables::build_table_views(&stream);
        let chars = vec![
            ExtractedChar { ch: '姓', min_x: 100.0, min_y: 100.0, max_x: 120.0, max_y: 120.0, font_size_pt: 10.5, glyph_h_pt: 10.0 },
            ExtractedChar { ch: '名', min_x: 120.0, min_y: 100.0, max_x: 140.0, max_y: 120.0, font_size_pt: 10.5, glyph_h_pt: 10.0 },
            ExtractedChar { ch: '张', min_x: 300.0, min_y: 100.0, max_x: 320.0, max_y: 120.0, font_size_pt: 10.5, glyph_h_pt: 10.0 },
            ExtractedChar { ch: '三', min_x: 320.0, min_y: 100.0, max_x: 340.0, max_y: 120.0, font_size_pt: 10.5, glyph_h_pt: 10.0 },
        ];
        let mut img = image::RgbImage::from_pixel(600, 400, image::Rgb([255, 255, 255]));
        // 印上"张三"字形（模拟底图上的印刷字）
        for y in 100..120usize {
            for x in 300..340usize {
                img.put_pixel(x as u32, y as u32, image::Rgb([0, 0, 0]));
            }
        }
        let mut color_map = std::collections::HashMap::new();
        let mut next_role_id = 2u32;
        let result = process_tables_on_page(
            &stream, &views, &chars, 1, 200, &mut color_map, &mut next_role_id, &mut img,
        );
        assert_eq!(result.regions.len(), 2);
        let r0 = &result.regions[0];
        assert_eq!(r0.text, "姓名");
        assert_eq!(r0.table_index, Some(1));
        assert_eq!(r0.page, 1);
        assert!(r0.font_size > 0);
        let r1 = &result.regions[1];
        assert_eq!(r1.text, "张三");
        assert_eq!(r1.highlight.as_deref(), Some("yellow"));
        assert_eq!(r1.role_id, 2);
        // 擦除后底图上的"张三"字形消失
        assert_eq!(img.get_pixel(310, 110), &image::Rgb([255, 255, 255]));
        // 排除矩形与外包围
        assert_eq!(result.cell_rects.len(), 2);
        assert_eq!(result.outer_rects.len(), 1);
    }

    #[test]
    fn process_tables_skips_gray_and_continue_cells() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>合并</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t> </w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let mut registry = crate::core::docx_io::StyleRegistry::new();
        let stream = crate::core::docx_io::parse_body_stream_for_test(xml, &mut registry).unwrap();
        let views = crate::core::docx_tables::build_table_views(&stream);
        let chars = vec![
            ExtractedChar { ch: '合', min_x: 100.0, min_y: 100.0, max_x: 120.0, max_y: 120.0, font_size_pt: 10.5, glyph_h_pt: 10.0 },
            ExtractedChar { ch: '并', min_x: 120.0, min_y: 100.0, max_x: 140.0, max_y: 120.0, font_size_pt: 10.5, glyph_h_pt: 10.0 },
        ];
        let mut img = image::RgbImage::from_pixel(600, 400, image::Rgb([255, 255, 255]));
        let mut color_map = std::collections::HashMap::new();
        let mut next_role_id = 2u32;
        let result = process_tables_on_page(
            &stream, &views, &chars, 1, 200, &mut color_map, &mut next_role_id, &mut img,
        );
        // 仅起始格生成区域；续行格跳过
        assert_eq!(result.regions.len(), 1);
        assert_eq!(result.regions[0].text, "合并");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim doc_render`
Expected: 编译失败。

- [ ] **Step 3: 实现**（放在 `combine_page_regions_with_role_map` 之后；文件头补 `use crate::core::docx_tables::{self, RectPx, TableView};` 与 `use crate::core::docx_io::BodyElement;`）

```rust
/// 一页的表格处理结果。
#[derive(Debug, Default)]
pub(crate) struct TablePageResult {
    pub regions: Vec<TextRegion>,
    /// 已生成区域的单元格矩形（供标签区域排重）。
    pub cell_rects: Vec<RectPx>,
    /// 每张（有区域的）表的外包围矩形（供高亮框分流）。
    pub outer_rects: Vec<RectPx>,
}

/// 处理一页上的所有表格：定位 → 擦字 → 生成单元格 TextRegion。
/// 无表格或整表未命中时返回空结果（表格保留印刷底图）。
pub(crate) fn process_tables_on_page(
    stream: &[BodyElement],
    views: &[TableView],
    chars: &[ExtractedChar],
    page_num: i32,
    dpi: u32,
    color_map: &mut std::collections::HashMap<String, u32>,
    next_role_id: &mut u32,
    img: &mut image::RgbImage,
) -> TablePageResult {
    let scale = dpi as f32 / 72.0;
    let matches = docx_tables::locate_tables_on_page(stream, views, chars);
    let mut result = TablePageResult::default();

    for view in views {
        let Some(m) = matches.iter().find(|m| m.table_index == view.index) else { continue };
        let rects = docx_tables::infer_cell_rects(view, m, chars);
        let flat_cells: Vec<&crate::core::docx_tables::TableCellView> =
            view.rows.iter().flat_map(|r| r.cells.iter()).collect();
        if rects.iter().all(|r| r.is_none()) {
            continue; // 整表未命中：保留印刷底图
        }

        let mut table_regions = Vec::new();
        for (i, cell) in flat_cells.iter().enumerate() {
            if cell.v_merge == crate::core::docx_io::CellVMerge::Continue
                || cell.skip_printed
                || cell.text.trim().is_empty()
            {
                continue;
            }
            let Some(rect) = rects.get(i).copied().flatten() else { continue };
            // 命中字符：擦除原印刷字（保留表格线）
            let cell_m = m.row_matches.iter().flatten().nth(0); // 占位——见下方完整实现
            let _ = cell_m;
            // 字号：命中字符校准值优先
            let located: Option<&crate::core::docx_tables::CellMatch> = {
                let idx = view
                    .rows
                    .iter()
                    .zip(m.row_matches.iter())
                    .flat_map(|(row, rm)| row.cells.iter().zip(rm.iter()))
                    .enumerate()
                    .find(|(k, _)| *k == i)
                    .and_then(|(_, (_, cm))| cm.as_ref());
                idx
            };
            let mut font_size = 0i32;
            if let Some(cm) = located {
                erase_char_boxes(img, chars, &cm.char_idxs, 2);
                let refs: Vec<&ExtractedChar> = cm.char_idxs.iter().filter_map(|&k| chars.get(k)).collect();
                let avg_pt = {
                    let vals: Vec<f32> = refs.iter().map(|c| c.font_size_pt).filter(|s| *s > 0.0 && !s.is_nan()).collect();
                    if vals.is_empty() { 0.0 } else { vals.iter().sum::<f32>() / vals.len() as f32 }
                };
                font_size = resolve_font_size_px(&refs, avg_pt, scale);
            }
            if font_size <= 0 {
                if let Some(h) = cell.sz_half_pt {
                    font_size = ((h as f32 / 2.0) * scale).round() as i32;
                }
            }
            // 区域矩形：命中 bbox 外扩（水平 6 / 竖直 4）；推断矩形已内缩
            let region_rect = if located.is_some() {
                RectPx {
                    x: rect.x - 6,
                    y: rect.y - 4,
                    w: rect.w + 12,
                    h: rect.h + 8,
                }
            } else {
                rect
            };
            let (rx, ry) = (region_rect.x.max(0), region_rect.y.max(0));
            let rw = region_rect.w.min(img.width() as i32 - rx).max(1);
            let rh = region_rect.h.min(img.height() as i32 - ry).max(1);
            // 角色映射：高亮色走共享 color_map（与高亮框路径同池）
            let (role_id, highlight) = match &cell.highlight {
                Some(c) => {
                    let rid = *color_map.entry(c.clone()).or_insert_with(|| {
                        let id = *next_role_id;
                        *next_role_id += 1;
                        id
                    });
                    (rid, Some(c.clone()))
                }
                None => (0, None),
            };
            table_regions.push(TextRegion {
                x: rx,
                y: ry,
                w: rw,
                h: rh,
                text: cell.text.clone(),
                role_id,
                highlight,
                printed: cell.printed,
                page: page_num,
                font_size,
                align: cell.align,
                margin_left: Some(4.0),
                margin_right: Some(4.0),
                margin_top: Some(2.0),
                margin_bottom: Some(2.0),
                table_index: Some(view.index as u32),
                ..TextRegion::default()
            });
        }

        if table_regions.is_empty() {
            continue;
        }
        // 外包围 = 单元格矩形并集（外扩 4px）
        let cells_on_page: Vec<RectPx> = flat_cells
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                c.v_merge != crate::core::docx_io::CellVMerge::Continue
                    && !c.skip_printed
                    && !c.text.trim().is_empty()
                    && rects.get(*i).copied().flatten().is_some()
            })
            .filter_map(|(i, _)| rects.get(i).copied().flatten())
            .collect();
        if let Some(first) = cells_on_page.first() {
            let mut outer = *first;
            for r in &cells_on_page[1..] {
                let x0 = outer.x.min(r.x);
                let y0 = outer.y.min(r.y);
                let x1 = (outer.x + outer.w).max(r.x + r.w);
                let y1 = (outer.y + outer.h).max(r.y + r.h);
                outer = RectPx { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
            }
            result.outer_rects.push(RectPx { x: outer.x - 4, y: outer.y - 4, w: outer.w + 8, h: outer.h + 8 });
        }
        result.cell_rects.extend(cells_on_page);
        result.regions.extend(table_regions);
    }
    result
}
```

**实现提示（写给执行者）**：上面代码中 `let cell_m = ...` 两行占位须删除；`located` 的取法过于绕，直接在 `infer_cell_rects` 返回旁同时展开行主序匹配即可，最终代码形如：

```rust
let flat_matches: Vec<Option<&docx_tables::CellMatch>> = m
    .row_matches
    .iter()
    .flat_map(|rm| rm.iter())
    .map(|cm| cm.as_ref())
    .collect();
// flat_cells 与 flat_matches 下标一一对应（均按行主序、含续行格占位）
```

然后 `let located = flat_matches.get(i).copied().flatten();`。同理 `cells_on_page` 直接用 `flat_cells`/`rects` 下标过滤。`TextRegion::table_index` 字段在 Task 8 才加入 models —— **Task 6 提前依赖它**。为避免执行顺序问题，Task 6 实现时先在本文件写 `table_index: Some(view.index as u32)` 会编译失败。**因此把 Task 8 的 models.rs 字段新增提前到 Task 6 的 Step 3 一并完成**（models.rs 加字段 + Default + params.rs 两处转换 + types.ts 同步），Task 8 只做前端部分。计划已按此调整：见 Task 6 Step 3b。

- [ ] **Step 3b: 同步贯通 `table_index` 字段**（本步骤与 Step 3 同一提交）

1. `crates/core/src/core/models.rs`：`TextRegion` 增加字段（放在 `page` 之后）：

```rust
    /// 所属表格序号（1 基；非表格区域为 None）。
    #[serde(default)]
    pub table_index: Option<u32>,
```

`impl Default for TextRegion` 增加 `table_index: None,`。`has_overrides()` 不变（表格归属不算覆盖项）。

2. `src-tauri/src/params.rs`：
   - `UiRegion` 增加字段（`page` 之后）：`#[serde(default)] pub table_index: Option<u32>,`
   - `impl From<&TextRegion> for UiRegion`（params.rs:238 附近）增加 `table_index: tr.table_index,`
   - 反向转换（params.rs 约 563 行 `Ok(TextRegion {` 处）增加 `table_index: r.table_index,`
3. `src/types.ts`：`UiRegion` 增加 `/** 所属表格序号（1 基；null = 非表格区域） */ tableIndex?: number | null;`

- [ ] **Step 4: 运行测试**

Run: `cargo nextest run -p handwrite-sim && cargo check -p handwrite-sim-app 2>/dev/null || cargo check --workspace`
Expected: 全绿（若 `cargo check -p` 的包名不对，直接 `cargo check --workspace`；src-tauri 包名见 `src-tauri/Cargo.toml`）。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/doc_render.rs crates/core/src/core/models.rs src-tauri/src/params.rs src/types.ts
git commit -m "feat(tables): 单页表格区域生成（定位/擦字/字号校准/角色映射）并贯通 table_index"
```

---

### Task 7: 导入链路接线（docx 分支载入表格 + 标签/高亮排重）

`import_document` 的 docx 路径：解析 body 流 → 转 PDF → 栅格化时逐页处理表格；表格内的 `{{}}` 标签与高亮框不重复生成区域、不整框擦白。

**Files:**
- Modify: `crates/core/src/core/doc_render.rs`

**Interfaces:**
- Consumes: Task 6 `process_tables_on_page`/`TablePageResult`、`docx_io::load_body_stream`
- Produces:
  - `pub fn pdf_to_images_opt_with_tables(pdf_path: &Path, out_dir: &Path, dpi: u32, extract_regions: bool, body_stream: &[BodyElement]) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError>`
  - `extract_pdf_page_tags` 签名追加 `exclude_rects: &[RectPx]`（跳过中心落在排除矩形内的标签）

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn extract_pdf_page_tags_excludes_table_cells() {
        // 字符流中有一个 {{姓名}} 标签；排除矩形覆盖它 → 不生成区域、不擦除
        let chars = vec![
            ExtractedChar { ch: '{', min_x: 100.0, min_y: 50.0, max_x: 106.0, max_y: 70.0, font_size_pt: 10.5, glyph_h_pt: 0.0 },
            ExtractedChar { ch: '{', min_x: 106.0, min_y: 50.0, max_x: 112.0, max_y: 70.0, font_size_pt: 10.5, glyph_h_pt: 0.0 },
            ExtractedChar { ch: '姓', min_x: 112.0, min_y: 50.0, max_x: 122.0, max_y: 70.0, font_size_pt: 10.5, glyph_h_pt: 0.0 },
            ExtractedChar { ch: '名', min_x: 122.0, min_y: 50.0, max_x: 132.0, max_y: 70.0, font_size_pt: 10.5, glyph_h_pt: 0.0 },
            ExtractedChar { ch: '}', min_x: 132.0, min_y: 50.0, max_x: 138.0, max_y: 70.0, font_size_pt: 10.5, glyph_h_pt: 0.0 },
            ExtractedChar { ch: '}', min_x: 138.0, min_y: 50.0, max_x: 144.0, max_y: 70.0, font_size_pt: 10.5, glyph_h_pt: 0.0 },
        ];
        let mut img = image::RgbImage::from_pixel(400, 300, image::Rgb([255, 255, 255]));
        let exclude = [crate::core::docx_tables::RectPx { x: 90, y: 40, w: 70, h: 40 }];
        let regions = extract_pdf_page_tags(&chars, 0, 200, 400, 300, &mut img, &exclude);
        assert!(regions.is_empty());
        // 不排除时能识别到（对照组）
        let mut img2 = image::RgbImage::from_pixel(400, 300, image::Rgb([255, 255, 255]));
        let regions2 = extract_pdf_page_tags(&chars, 0, 200, 400, 300, &mut img2, &[]);
        assert_eq!(regions2.len(), 1);
        assert_eq!(regions2[0].text, "姓名");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo nextest run -p handwrite-sim doc_render`
Expected: 编译失败（签名不匹配）。

- [ ] **Step 3: 实现**

1. `extract_pdf_page_tags` 参数列表追加 `exclude_rects: &[crate::core::docx_tables::RectPx]`；在 `for m in matches {` 循体开头（计算完 x/y/w/h 之后、擦除之前）加：

```rust
        // 表格单元格内的标签由单元格区域接管：跳过（不擦除、不生成区域）
        let cx = x + w / 2;
        let cy = y + h / 2;
        if exclude_rects.iter().any(|r| {
            cx >= r.x && cx < r.x + r.w && cy >= r.y && cy < r.y + r.h
        }) {
            continue;
        }
```

2. `pdf_to_images_opt` 改为薄封装，主体移到新函数：

```rust
pub fn pdf_to_images_opt(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
    extract_regions: bool,
) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError> {
    pdf_to_images_opt_with_tables(pdf_path, out_dir, dpi, extract_regions, &[])
}

/// 同 pdf_to_images_opt，但可传入 docx body 元素流以启用表格识别。
pub fn pdf_to_images_opt_with_tables(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
    extract_regions: bool,
    body_stream: &[crate::core::docx_io::BodyElement],
) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError> {
```

函数体内原有循环修改（`document.pages()` 迭代内、`if extract_regions {` 分支）：

```rust
        let views = docx_tables::build_table_views(body_stream); // 移到 pages 循环之前、color_map 初始化旁
        // ... 循环内：
        if extract_regions {
            // 0. 表格：定位 + 擦字 + 生成单元格区域（先于标签/高亮，标签排除与高亮分流依赖其矩形）
            let table_result = if views.is_empty() {
                TablePageResult::default()
            } else {
                process_tables_on_page(
                    body_stream, &views, &page_chars, (index + 1) as i32, dpi,
                    &mut color_map, &mut next_role_id, &mut rgb_img,
                )
            };

            // 1. {{...}}/【...】标签（表格内标签已被单元格区域接管）
            let tag_regions = extract_pdf_page_tags(
                &page_chars, index, dpi, rgb_img.width(), rgb_img.height(),
                &mut rgb_img, &table_result.cell_rects,
            );

            // 2. 高亮色块：落在表内的只做像素过滤擦除（保表格线），不生成区域
            let highlight_boxes = detect_highlight_boxes(&rgb_img);
            let (in_table, normal): (Vec<_>, Vec<_>) = highlight_boxes
                .into_iter()
                .partition(|b| table_result.outer_rects.iter().any(|r| rect_contains_box(r, b)));
            erase_highlight_pixels_in_boxes(&mut rgb_img, &in_table);
            erase_highlight_boxes(&mut rgb_img, &normal);

            // 3. 表外高亮框 → 区域（原逻辑不变）
            let page_regions = combine_page_regions_with_role_map(
                normal, tag_regions, &page_chars, (index + 1) as i32, scale,
                &mut color_map, &mut next_role_id,
            );
            all_regions.extend(table_result.regions);
            all_regions.extend(page_regions);
        }
```

注意：`extract_pdf_page_chars` 的调用保持在最前（原有第 1 步）。`views`/`color_map`/`next_role_id` 都在 pages 循环外初始化一次（`color_map`/`next_role_id` 原代码已有，保持位置）。

3. `document_to_page_images_opt` 的 docx 分支改为：

```rust
        "docx" => {
            // 表格解析失败不阻塞导入：退化为纯底图（表格保持印刷）
            let body_stream = crate::core::docx_io::load_body_stream(path).unwrap_or_default();
            let pdf_path = docx_to_pdf(path, out_dir)?;
            pdf_to_images_opt_with_tables(&pdf_path, out_dir, dpi, extract_regions, &body_stream)
        }
```

`load_body_stream` 在 Task 1 已是 `pub(crate)`。若 `pub(crate)` 导致跨 crate（src-tauri）不可见——它只在 core 内部使用，没问题。

- [ ] **Step 4: 运行全部测试**

Run: `cargo nextest run -p handwrite-sim`
Expected: 全绿（现有 `pdf_to_images_renders_all_pages` 等不回归）。

- [ ] **Step 5: 提交**

```bash
git add crates/core/src/core/doc_render.rs
git commit -m "feat(tables): 导入链路接入表格识别（标签/高亮与表格区域互斥排重）"
```

---

### Task 8: 前端——表格徽标与"应用到整表"

区域列表标记表格归属；区域对话框提供"此字体应用到整表"批量操作。

**Files:**
- Modify: `src/components/RegionList.vue`（徽标）
- Modify: `src/components/RegionDialog.vue`（按钮）
- Modify: `src/store.ts`（`applyFontToTable` action）

**Interfaces:**
- Consumes: `UiRegion.tableIndex`（Task 6 Step 3b 已加入 types.ts）
- Produces: `export function applyFontToTable(tableIndex: number, fontPath: string, fontSize: number): void`（store.ts）

- [ ] **Step 1: store action**（`src/store.ts`，放在现有 region 相关 action 附近；执行时先读 `store.regions` 类型确认字段名 `fontPath`/`fontSize`/`tableIndex`）

```ts
/** 将字体与字号批量应用到某张表格的全部单元格区域。 */
export function applyFontToTable(tableIndex: number, fontPath: string, fontSize: number): void {
  let n = 0;
  for (const r of store.regions) {
    if (r.tableIndex === tableIndex) {
      r.fontPath = fontPath;
      r.fontSize = fontSize;
      n++;
    }
  }
  store.status = `已将字体应用到表格 ${tableIndex} 的 ${n} 个单元格`;
  scheduleRender();
}
```

（`scheduleRender` 已存在于 store.ts；若 action 区域不可直接调用，把它放在与其它调用 `scheduleRender()` 的 action 相同的作用域。）

- [ ] **Step 2: RegionList 徽标**

读 `RegionList.vue` 现有 `regionLabel(r, i)` 实现，在标签文本前追加表格前缀（保持现有样式类）：

```ts
function regionLabel(r: Region, i: number): string {
  const tablePrefix = r.tableIndex ? `表${r.tableIndex}·` : "";
  // ...原有逻辑，把 tablePrefix 拼到最前
}
```

（具体拼接位置以文件实际实现为准——目标：列表条目显示如 `表1·3. 手写 5字 (100,200 300x40)`。）

- [ ] **Step 3: RegionDialog "应用到整表"按钮**

读 `RegionDialog.vue`：确认 `draft` 的 `fontPath`/`fontSize` 字段与保存流程。在字体选择控件（`v-model:value="draft.fontPath"` 一行）附近加按钮，仅当编辑中的区域有 `tableIndex` 时显示：

```vue
<n-button
  v-if="draft.tableIndex"
  size="small"
  @click="applyFontToTable(draft.tableIndex, draft.fontPath, draft.fontSize)"
>
  此字体应用到整张表格
</n-button>
```

组件内 `import { applyFontToTable } from "../store";`。注意 RegionDialog 若是受控弹窗（props/emit），按钮只需调用 action 并给出提示，不关闭弹窗。若 `draft.tableIndex` 不存在（草稿结构从 Region 复制而来），在草稿初始化处确认复制了该字段。

- [ ] **Step 4: 类型检查 + 构建验证**

Run: `npx vue-tsc --noEmit`
Expected: 无错误。

Run: `pnpm build`
Expected: 构建成功（vue-tsc + vite build）。

- [ ] **Step 5: 提交**

```bash
git add src/components/RegionList.vue src/components/RegionDialog.vue src/store.ts
git commit -m "feat(ui): 表格单元格区域徽标与整表字体批量应用"
```

---

### Task 9: 端到端手工验证 + CHANGELOG + 收尾

用真实 docx 验证整链路，更新文档。

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `docs/01-architecture.md`（可选：表格识别一段）

**Interfaces:** 无代码接口。

- [ ] **Step 1: 构造测试 docx 并端到端验证**

用 Python（python-docx，本机可用 `pip install python-docx` 或已有环境）生成 `tmp_table_test.docx`：2×3 表格（含一个合并格、一个黄色高亮格、一个居中表头），保存到仓库根外（如 `%TEMP%`）。脚本：

```python
from docx import Document
doc = Document()
doc.add_paragraph("表格前正文")
t = doc.add_table(rows=3, cols=3)
t.style = "Table Grid"
t.cell(0, 0).text = "项目"
t.cell(0, 1).text = "数值"
t.cell(0, 2).text = "备注"
t.cell(1, 0).merge(t.cell(2, 0)).text = "合并说明"
# 黄色高亮格
run = t.cell(1, 1).paragraphs[0].add_run("手写答案")
from docx.oxml.ns import qn
from docx.oxml import OxmlElement
hl = OxmlElement('w:highlight'); hl.set(qn('w:val'), 'yellow')
run._element.get_or_add_rPr().append(hl)
t.cell(1, 2).text = ""
t.cell(2, 1).text = "乙"
t.cell(2, 2).text = "丙"
doc.save(r"%TEMP%\table_test.docx")
print("ok")
```

启动应用（后台）：`pnpm tauri dev`（若启动慢/不可用，退化为运行 Rust 侧入口）：

```bash
cargo test -p handwrite-sim --lib -- --nocapture docx 2>&1 | tail -5
```

更可靠的自动化端到端（不依赖 GUI）：写一个临时集成测试（**不入库**，跑完删除）或直接用现有 `import_document` 的核心函数在测试里调用 `document_to_page_images_opt`（Windows 上装了 Word 才会成功；失败则记录为环境限制并跳过）。验证点：
1. 返回的 regions 中 `table_index == Some(1)` 的区域数 = 5（合并格算 1，空格跳过）；
2. 黄色高亮格 region 的 `highlight == Some("yellow")`、`role_id == 2`；
3. 输出 PNG 中表格边框线像素仍为深色（抽查表格线坐标处像素）；
4. `{{打印:xxx}}` 格生成 `printed == true` 的区域。

将验证结果（页数/区域数/抽样像素）记录在提交信息里。

- [ ] **Step 2: CHANGELOG**

按现有格式在 `CHANGELOG.md` 顶部新增条目（版本号跟随当前 0.4.3 的 Unreleased 段或新增 `[Unreleased]` 段，格式参照文件内既有写法）：

```markdown
### 新增
- **Word 表格识别导入**：导入 .docx 底图时自动识别表格——表格线保留在底图，每个单元格文字提取为可编辑手写区域；支持合并单元格、单元格高亮角色映射；区域列表显示表格归属，支持"此字体应用到整张表格"批量修改（`{{打印:...}}` 单元格生成印刷体区域，灰色高亮单元格保持印刷底图）。
```

- [ ] **Step 3: 全量回归**

Run: `cargo nextest run --workspace && npx vue-tsc --noEmit`
Expected: 全绿。

- [ ] **Step 4: 提交**

```bash
git add CHANGELOG.md docs/01-architecture.md
git commit -m "docs(tables): 更新日志与架构说明（表格识别导入）"
```

---

## Self-Review 记录

- **覆盖检查**：用户需求三要素——表格显示（表格线留底图 + 单元格区域，Task 1-7）、识别表格（docx XML 结构解析 + PDF 字符流定位，Task 1/3）、修改表格内字体（逐格 Region 编辑已有 + 整表批量 Task 8）。PDF 直接导入不做表格识别（保持印刷底图）——符合需求分析结论。
- **占位符检查**：Task 4 Step 3 与 Task 6 Step 3 各含一处"写复杂了"的说明，均已给出替换用的完整实现，执行者须按完整版本写。Task 8 前端步骤依赖对组件现状的阅读，已给出目标行为与代码片段。
- **类型一致性**：`BodyElement`/`ParsedTable`/`ParsedRow`/`ParsedCell`/`CellVMerge`（Task 1）→ `TableView`/`TableRowView`/`TableCellView`（Task 2）→ `CellMatch`/`TablePageMatch`（Task 3）→ `RectPx`/`infer_cell_rects`（Task 4）→ `erase_char_boxes`/`erase_highlight_pixels_in_boxes`/`rect_contains_box`（Task 5）→ `TablePageResult`/`process_tables_on_page`（Task 6）→ `pdf_to_images_opt_with_tables`/`extract_pdf_page_tags(.., exclude_rects)`（Task 7）。`TextRegion.table_index` 因 Task 6 依赖提前至 Task 6 Step 3b。
- **顺序依赖**：Task 1→2→3→4→5→6→7→8→9 严格串行（后者消费前者 API）。Task 5 与 Task 3/4 无相互依赖但保持顺序简单。
