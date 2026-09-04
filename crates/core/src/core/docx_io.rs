//! docx 文档解析：手写解析（zip + quick-xml），提取段落文本、对齐与首行缩进。
//!
//! 对应 Python 版 `docx_io.py` 语义：
//! 首行缩进三级回退：
//! 1. `w:firstLineChars`（1/100 字符）× 渲染字号 → 像素；
//! 2. `w:firstLine`（twips，1/20 pt）按文档字号还原字符数 × 渲染字号；
//! 3. 样式链（based_on）继承。
//! 4. 忽略空段落。
//!
//! 不依赖第三方 docx 库：直接解包 zip 读取 `word/document.xml` 与 `word/styles.xml`，
//! 用 quick-xml 解析。字段全部公开可控，无 docx-rs 的私有字段/serde hack 限制；
//! 并修正 docx-rs 把 `w:firstLine` 的 twips 误当 EMU 处理的单位 bug。

use std::io::{Cursor, Read};
use std::path::Path;

use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use zip::ZipArchive;

use crate::core::models::{Align, Paragraph, TextRun, TextRunStyle};

/// 段落直接格式（来自 document.xml 的 `w:pPr`）。
#[derive(Default)]
struct ParaFmt {
    style_id: Option<String>,
    /// `w:jc` 对齐值（center/right/left/both/...）。
    jc: Option<String>,
    /// `w:ind` 的 `w:firstLineChars`（1/100 字符）。
    first_line_chars: Option<i32>,
    /// `w:ind` 的 `w:firstLine`（twips，1/20 pt）。
    first_line_twips: Option<i32>,
    /// 段落首个 run 的 `w:rPr/w:sz`（半磅）。
    run_sz_half_pt: Option<u32>,
    /// 段落默认字体（来自 w:pPr）。
    font_family: Option<String>,
}

/// 解析后的单个段落（直接格式 + 原始 Run 列表）。
struct ParsedParagraph {
    runs: Vec<TextRun>,
    fmt: ParaFmt,
}

/// 样式定义（来自 styles.xml 的 `w:style`）。
struct StyleDef {
    style_id: String,
    based_on: Option<String>,
    jc: Option<String>,
    first_line_chars: Option<i32>,
    first_line_twips: Option<i32>,
    /// 样式默认 run 字号（`w:rPr/w:sz`，半磅）。
    sz_half_pt: Option<u32>,
}

/// 从 docx 读取段落（忽略空段），对齐/首行缩进还原，并提取 Run 级富文本样式与标签。
pub fn load_paragraphs(path: &Path, font_size: f32) -> Result<Vec<Paragraph>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 docx {path:?} 失败：{e}"))?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|e| format!("解压 docx {path:?} 失败：{e}"))?;

    let document_xml = read_entry(&mut archive, "word/document.xml")?;
    let styles_xml = read_entry_optional(&mut archive, "word/styles.xml");

    let mut registry = StyleRegistry::new();
    let parsed_paras = parse_document(&document_xml, &mut registry)?;
    let (styles, doc_defaults_sz) = parse_styles(&styles_xml);

    let mut result = Vec::new();
    for parsed in parsed_paras {
        // 展开标签语法（如 {{手写:内容}}）
        let mut runs = Vec::new();
        for r in parsed.runs {
            runs.extend(split_syntax_tags(&r, &mut registry));
        }

        let (trimmed_text, trimmed_runs) = trim_paragraph_runs(runs);
        if trimmed_text.is_empty() {
            continue;
        }

        let align = resolve_align(&parsed.fmt);
        let indent = resolve_indent(&parsed.fmt, font_size, &styles, doc_defaults_sz);
        let p_font = trimmed_runs
            .iter()
            .find_map(|r| r.style.font_family.clone())
            .or_else(|| parsed.fmt.font_family.clone());
        result.push(Paragraph {
            text: trimmed_text,
            align,
            first_line_indent: indent,
            font_family: p_font,
            runs: trimmed_runs,
        });
    }

    // 双模式自动分类：
    // 如果整个文档没有任何高亮背景色：纯手写模式（未标记的普通文本为默认手写 role_id = 0）。
    // 如果整个文档存在高亮背景色：高亮文本为手写（role_id >= 2, printed = false），所有未高亮文本自动转为印刷体模板（role_id = 1, printed = true）。
    // 显式标签（{{打印:}}、{{1:}} 等）是用户明确指定的样式，不随纯手写模式重置而丢弃。
    let has_highlights = result.iter().any(|p| {
        p.runs
            .iter()
            .any(|r| r.style.highlight.is_some() || (r.style.role_id >= 2 && !r.style.printed))
    });

    if has_highlights {
        for p in &mut result {
            for r in &mut p.runs {
                if r.style.role_id == 0 {
                    r.style.role_id = 1;
                    r.style.printed = true;
                }
            }
        }
    }

    Ok(result)
}

/// 探测文档的主字体名称（优先从印刷体或未高亮片段统计最常出现的字体族名）。
pub fn detect_doc_font_family(paragraphs: &[Paragraph]) -> Option<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();

    // 优先从印刷体或未高亮的 run 中提取
    for p in paragraphs {
        for r in &p.runs {
            if r.style.printed || r.style.highlight.is_none() {
                if let Some(ref font) = r.style.font_family {
                    let trimmed = font.trim();
                    if !trimmed.is_empty() {
                        let count = counts.entry(trimmed.to_string()).or_insert(0);
                        if *count == 0 {
                            order.push(trimmed.to_string());
                        }
                        *count += r.text.chars().count().max(1);
                    }
                }
            }
        }
    }

    // 若无匹配，则回退统计所有 run
    if counts.is_empty() {
        for p in paragraphs {
            for r in &p.runs {
                if let Some(ref font) = r.style.font_family {
                    let trimmed = font.trim();
                    if !trimmed.is_empty() {
                        let count = counts.entry(trimmed.to_string()).or_insert(0);
                        if *count == 0 {
                            order.push(trimmed.to_string());
                        }
                        *count += r.text.chars().count().max(1);
                    }
                }
            }
        }
    }

    let mut best: Option<(String, usize)> = None;
    for name in &order {
        let count = counts.get(name).copied().unwrap_or(0);
        if let Some((_, max_count)) = best {
            if count > max_count {
                best = Some((name.clone(), count));
            }
        } else {
            best = Some((name.clone(), count));
        }
    }

    best.map(|(name, _)| name)
}

/// 读取 zip 内指定 entry 的文本内容（UTF-8）。
fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String, String> {
    let mut file = archive
        .by_name(name)
        .map_err(|e| format!("docx 缺少 {name}：{e}"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("读取 {name} 失败：{e}"))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// 读取 zip 内可选 entry（不存在返回 None）。
fn read_entry_optional(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// 取元素/属性名的 local part（`w:pPr` → `pPr`）。
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// 检查高亮颜色名称是否表示印刷体/灰色。
pub fn is_gray_highlight(val: &str) -> bool {
    let lower = val.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "lightgray"
            | "light_gray"
            | "light-gray"
            | "darkgray"
            | "dark_gray"
            | "dark-gray"
            | "gray-25"
            | "gray25"
            | "gray_25"
            | "gray-50"
            | "gray50"
            | "gray_50"
            | "gray"
            | "grey"
            | "lightgrey"
            | "light_grey"
            | "light-grey"
            | "darkgrey"
            | "dark_grey"
            | "dark-grey"
            | "grey-25"
            | "grey25"
            | "grey_25"
            | "grey-50"
            | "grey50"
            | "grey_50"
    )
}

/// 动态样式与角色映射注册表。
///
/// 用于在解析 docx 时动态记录遇到的高亮颜色、文字颜色和标签样式，
/// 按文档中首次出现的顺序依次分配角色 ID（Role 1 为 role_id=2, Role 2 为 role_id=3, ...）。
/// 印刷体/灰色样式固定映射至 role_id=1。
#[derive(Debug, Clone, Default)]
pub struct StyleRegistry {
    /// 样式键 -> 分配的 role_id。
    entries: std::collections::HashMap<String, u32>,
    /// 记录遇到的所有样式键（按首次出现顺序）。
    ordered_keys: Vec<String>,
    /// 下一个可分配的角色 ID（从 2 开始）。
    next_role_id: u32,
}

impl StyleRegistry {
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            ordered_keys: Vec::new(),
            next_role_id: 2,
        }
    }

    /// 获取或注册一个样式键，返回对应的 role_id。
    pub fn get_or_register(&mut self, key: &str) -> u32 {
        if let Some(&role_id) = self.entries.get(key) {
            return role_id;
        }
        let role_id = self.next_role_id;
        self.next_role_id += 1;
        self.entries.insert(key.to_string(), role_id);
        self.ordered_keys.push(key.to_string());
        role_id
    }

    /// 检查指定样式键是否已注册。
    pub fn get(&self, key: &str) -> Option<u32> {
        self.entries.get(key).copied()
    }

    /// 获取按遇到顺序记录的样式键列表。
    pub fn registered_keys(&self) -> &[String] {
        &self.ordered_keys
    }

    /// 处理并应用高亮颜色到 TextRunStyle。
    pub fn apply_highlight(&mut self, val: &str, style: &mut TextRunStyle) {
        let trimmed = val.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower == "none" || lower.is_empty() {
            return;
        }

        style.highlight = Some(trimmed.to_string());

        if is_gray_highlight(&lower) {
            style.role_id = 1;
            style.printed = true;
        } else {
            let key = format!("highlight:{lower}");
            let role_id = self.get_or_register(&key);
            style.role_id = role_id;
            style.printed = false;
        }
    }

    /// 处理并应用文本颜色到 TextRunStyle。
    pub fn apply_color(&mut self, val: &str, style: &mut TextRunStyle) {
        let trimmed = val.trim();
        if let Some(rgb) = parse_hex_color(trimmed) {
            style.fill = Some(rgb);
            if style.highlight.is_none() && rgb != [0, 0, 0] {
                let key = format!("color:{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                let role_id = self.get_or_register(&key);
                style.role_id = role_id;
            }
        }
    }
}

/// 解析单个 Run 时暂存的格式属性。
#[derive(Default, Clone, Debug)]
struct RawRunProps {
    sz_half_pt: Option<u32>,
    font_family: Option<String>,
    highlight: Option<String>,
    color: Option<String>,
}

impl RawRunProps {
    fn apply_to(&self, style: &mut TextRunStyle, registry: &mut StyleRegistry) {
        // 不再将 Word 中的 sz 设置为 style.font_size，保留信纸预设字号
        if let Some(ref font) = self.font_family {
            style.font_family = Some(font.clone());
        }
        if let Some(ref color_val) = self.color {
            if let Some(rgb) = parse_hex_color(color_val) {
                style.fill = Some(rgb);
            }
        }
        if let Some(ref hl_val) = self.highlight {
            registry.apply_highlight(hl_val, style);
        } else if let Some(ref color_val) = self.color {
            registry.apply_color(color_val, style);
        }
    }
}

/// 解析 6 位十六进制 RGB 颜色值（支持可选前导 #，忽略 "auto"）。
fn parse_hex_color(val: &str) -> Option<[u8; 3]> {
    let mut s = val.trim();
    if s.eq_ignore_ascii_case("auto") {
        return None;
    }
    if let Some(stripped) = s.strip_prefix('#') {
        s = stripped;
    }
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

/// 解析标签中的角色/样式配置。
fn parse_tag_style(
    tag_content: &str,
    parent_style: &TextRunStyle,
    registry: &mut StyleRegistry,
) -> (String, TextRunStyle) {
    let mut style = parent_style.clone();
    let colon_pos = tag_content.find(':').or_else(|| tag_content.find('：'));
    if let Some(pos) = colon_pos {
        let colon_len = if tag_content.as_bytes()[pos] == b':' { 1 } else { '：'.len_utf8() };
        let prefix = tag_content[..pos].trim();
        let body = &tag_content[pos + colon_len..];

        if prefix == "手写" {
            style.role_id = 2;
            style.printed = false;
        } else if let Some(rest) = prefix.strip_prefix("手写") {
            if rest.is_empty() {
                style.role_id = 2;
            } else if let Ok(n) = rest.parse::<u32>() {
                style.role_id = if n <= 1 { 2 } else { n + 1 };
            } else {
                style.role_id = 2;
            }
            style.printed = false;
        } else if prefix.starts_with("打印") {
            style.role_id = 1;
            style.printed = true;
        } else if let Ok(n) = prefix.parse::<u32>() {
            style.role_id = n;
            style.printed = false;
        } else {
            let key = format!("tag:{prefix}");
            let role_id = registry.get_or_register(&key);
            style.role_id = role_id;
            style.printed = false;
        }
        (body.to_string(), style)
    } else {
        style.role_id = 2;
        style.printed = false;
        (tag_content.to_string(), style)
    }
}

/// 将单个 TextRun 中内嵌的 {{...}} 语法标签拆分为独立的 TextRun。
fn split_syntax_tags(run: &TextRun, registry: &mut StyleRegistry) -> Vec<TextRun> {
    let mut result = Vec::new();
    let text = &run.text;
    let mut cursor = 0;

    while let Some(rel_start) = text[cursor..].find("{{") {
        let tag_start = cursor + rel_start;
        if let Some(rel_end) = text[tag_start + 2..].find("}}") {
            let tag_content_start = tag_start + 2;
            let tag_content_end = tag_start + 2 + rel_end;
            let tag_end = tag_content_end + 2;

            if tag_start > cursor {
                let pre = &text[cursor..tag_start];
                if !pre.is_empty() {
                    result.push(TextRun::new(pre, run.style.clone()));
                }
            }

            let tag_inner = &text[tag_content_start..tag_content_end];
            let (clean_text, tag_style) = parse_tag_style(tag_inner, &run.style, registry);
            if !clean_text.is_empty() {
                result.push(TextRun::new(clean_text, tag_style));
            }

            cursor = tag_end;
        } else {
            break;
        }
    }

    if cursor < text.len() {
        let rem = &text[cursor..];
        if !rem.is_empty() {
            result.push(TextRun::new(rem, run.style.clone()));
        }
    }

    result
}

/// 修剪整段文本的首尾空白，并同步修剪 runs 列表两端的字符/空格，保留词间空格。
fn trim_paragraph_runs(runs: Vec<TextRun>) -> (String, Vec<TextRun>) {
    let full_text: String = runs.iter().map(|r| r.text.as_str()).collect();
    let trimmed_text = full_text.trim().to_string();
    if trimmed_text.is_empty() {
        return (String::new(), Vec::new());
    }

    let leading_ws_len = full_text.len() - full_text.trim_start().len();
    let trailing_ws_len = full_text.len() - full_text.trim_end().len();

    let mut result_runs = Vec::new();
    let mut current_pos = 0;
    let keep_start = leading_ws_len;
    let keep_end = full_text.len() - trailing_ws_len;

    for run in runs {
        let run_len = run.text.len();
        let run_start = current_pos;
        let run_end = current_pos + run_len;
        current_pos = run_end;

        let slice_start = run_start.max(keep_start);
        let slice_end = run_end.min(keep_end);

        if slice_start < slice_end {
            let offset_start = slice_start - run_start;
            let offset_end = slice_end - run_start;
            let sub_text = &run.text[offset_start..offset_end];
            if !sub_text.is_empty() {
                result_runs.push(TextRun::new(sub_text, run.style));
            }
        }
    }

    (trimmed_text, result_runs)
}

/// 处理 rPr 属性节点。
fn handle_r_pr_tag(
    e: &quick_xml::events::BytesStart,
    raw: &mut RawRunProps,
    fmt: &mut ParaFmt,
) {
    match local_name(e.name().as_ref()) {
        b"sz" => {
            if let Some(v) = attr_val(e, "val").and_then(|v| v.parse::<u32>().ok()) {
                if fmt.run_sz_half_pt.is_none() {
                    fmt.run_sz_half_pt = Some(v);
                }
                raw.sz_half_pt = Some(v);
            }
        }
        b"highlight" => {
            if let Some(v) = attr_val(e, "val") {
                raw.highlight = Some(v);
            }
        }
        b"color" => {
            if let Some(v) = attr_val(e, "val") {
                raw.color = Some(v);
            }
        }
        b"rFonts" => {
            let font = attr_val(e, "eastAsia")
                .filter(|s| !s.trim().is_empty())
                .or_else(|| attr_val(e, "ascii").filter(|s| !s.trim().is_empty()))
                .or_else(|| attr_val(e, "hAnsi").filter(|s| !s.trim().is_empty()));
            if font.is_some() {
                raw.font_family = font;
            }
        }
        _ => {}
    }
}

/// 处理 pPr 属性节点。
fn handle_p_pr_tag(e: &quick_xml::events::BytesStart, fmt: &mut ParaFmt) {
    match local_name(e.name().as_ref()) {
        b"pStyle" => {
            if let Some(v) = attr_val(e, "val") {
                fmt.style_id = Some(v);
            }
        }
        b"jc" => {
            if let Some(v) = attr_val(e, "val") {
                fmt.jc = Some(v);
            }
        }
        b"ind" => {
            read_ind_attrs(e, fmt);
        }
        b"rFonts" => {
            let font = attr_val(e, "eastAsia")
                .filter(|s| !s.trim().is_empty())
                .or_else(|| attr_val(e, "ascii").filter(|s| !s.trim().is_empty()))
                .or_else(|| attr_val(e, "hAnsi").filter(|s| !s.trim().is_empty()))
                .or_else(|| attr_val(e, "cs").filter(|s| !s.trim().is_empty()));
            if font.is_some() {
                fmt.font_family = font;
            }
        }
        _ => {}
    }
}

/// 解析 document.xml，返回段落列表（每个段落包含 Runs 与格式）。
fn parse_document(xml: &str, registry: &mut StyleRegistry) -> Result<Vec<ParsedParagraph>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut paras = Vec::new();
    // 状态
    let mut in_body_para = false; // 当前 w:p 是 body 直接子级（对齐 python-docx doc.paragraphs）
    // 表格嵌套深度：布尔标记在嵌套 w:tbl 提前闭合时会误放行外层表格中的段落
    let mut table_depth: usize = 0;
    // 文本框（w:txbxContent）嵌套深度：其中的 w:p/w:r 会破坏外层段落状态
    // （清空已累积 runs、提前闭合外层段落导致后续文字丢失），整体跳过
    let mut txbx_depth: usize = 0;
    let mut in_p_pr = false;
    let mut in_run = false;
    let mut in_r_pr = false;
    let mut in_t = false;

    let mut cur_runs: Vec<TextRun> = Vec::new();
    let mut cur_run_text = String::new();
    let mut cur_run_props = RawRunProps::default();
    let mut cur_fmt = ParaFmt::default();

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
                    b"tbl" => table_depth += 1,
                    b"txbxContent" => txbx_depth += 1,
                    b"p" if table_depth == 0 => {
                        in_body_para = true;
                        cur_runs.clear();
                        cur_run_text.clear();
                        cur_run_props = RawRunProps::default();
                        cur_fmt = ParaFmt::default();
                    }
                    b"pPr" if in_body_para => {
                        in_p_pr = true;
                        handle_p_pr_tag(&e, &mut cur_fmt);
                    }
                    b"r" if in_body_para => {
                        if !cur_run_text.is_empty() {
                            let mut style = TextRunStyle::default();
                            if cur_run_props.font_family.is_none() && cur_fmt.font_family.is_some() {
                                cur_run_props.font_family = cur_fmt.font_family.clone();
                            }
                            cur_run_props.apply_to(&mut style, registry);
                            cur_runs.push(TextRun::new(std::mem::take(&mut cur_run_text), style));
                        }
                        in_run = true;
                        cur_run_text.clear();
                        cur_run_props = RawRunProps::default();
                    }
                    b"rPr" if in_run => {
                        in_r_pr = true;
                        handle_r_pr_tag(&e, &mut cur_run_props, &mut cur_fmt);
                    }
                    b"t" if in_body_para => in_t = true,
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
                    b"tab" if in_body_para => cur_run_text.push('\t'),
                    b"br" | b"cr" if in_body_para => cur_run_text.push('\n'),
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
                if txbx_depth > 0 || !(in_body_para && in_t) {
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
                    b"tbl" => table_depth = table_depth.saturating_sub(1),
                    b"t" => in_t = false,
                    b"rPr" => in_r_pr = false,
                    b"r" => {
                        if !cur_run_text.is_empty() {
                            let mut style = TextRunStyle::default();
                            if cur_run_props.font_family.is_none() && cur_fmt.font_family.is_some() {
                                cur_run_props.font_family = cur_fmt.font_family.clone();
                            }
                            cur_run_props.apply_to(&mut style, registry);
                            cur_runs.push(TextRun::new(std::mem::take(&mut cur_run_text), style));
                        }
                        in_run = false;
                        in_r_pr = false;
                        cur_run_props = RawRunProps::default();
                    }
                    b"pPr" => in_p_pr = false,
                    b"p" if in_body_para => {
                        if !cur_run_text.is_empty() {
                            let mut style = TextRunStyle::default();
                            if cur_run_props.font_family.is_none() && cur_fmt.font_family.is_some() {
                                cur_run_props.font_family = cur_fmt.font_family.clone();
                            }
                            cur_run_props.apply_to(&mut style, registry);
                            cur_runs.push(TextRun::new(std::mem::take(&mut cur_run_text), style));
                        }
                        paras.push(ParsedParagraph {
                            runs: std::mem::take(&mut cur_runs),
                            fmt: std::mem::take(&mut cur_fmt),
                        });
                        in_body_para = false;
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
    Ok(paras)
}

/// 读取 `w:ind` 的首行缩进属性（firstLineChars / firstLine）。
fn read_ind_attrs(e: &quick_xml::events::BytesStart, fmt: &mut ParaFmt) {
    if let Some(v) = attr_val(e, "firstLineChars") {
        if let Ok(n) = v.parse() {
            fmt.first_line_chars = Some(n);
        }
    }
    if let Some(v) = attr_val(e, "firstLine") {
        if let Ok(n) = v.parse() {
            fmt.first_line_twips = Some(n);
        }
    }
}

/// 读取属性值（local name 匹配）。
fn attr_val(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    for attr in e.attributes() {
        let Ok(attr) = attr else { continue };
        if local_name(attr.key.as_ref()) == name.as_bytes() {
            return attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned());
        }
    }
    None
}

/// 解析 styles.xml，返回 (样式列表, docDefaults 默认字号半磅)。
fn parse_styles(xml: &Option<String>) -> (Vec<StyleDef>, Option<u32>) {
    let mut styles = Vec::new();
    let mut doc_defaults_sz = None;
    let Some(xml) = xml.as_deref() else {
        return (styles, doc_defaults_sz);
    };

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    // 状态
    let mut in_style = false;
    let mut in_style_p_pr = false;
    let mut in_style_r_pr = false;
    let mut in_doc_defaults = false;
    let mut in_r_pr_default = false;
    let mut in_r_pr = false;
    let mut cur: Option<StyleDef> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"style" => {
                    in_style = true;
                    let style_id = attr_val(&e, "styleId").unwrap_or_default();
                    cur = Some(StyleDef {
                        style_id,
                        based_on: None,
                        jc: None,
                        first_line_chars: None,
                        first_line_twips: None,
                        sz_half_pt: None,
                    });
                }
                b"pPr" if in_style => in_style_p_pr = true,
                b"rPr" if in_style => in_style_r_pr = true,
                b"docDefaults" => in_doc_defaults = true,
                b"rPrDefault" if in_doc_defaults => in_r_pr_default = true,
                b"rPr" if in_r_pr_default => in_r_pr = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match local_name(e.name().as_ref()) {
                b"basedOn" if in_style => {
                    if let Some(s) = cur.as_mut() {
                        s.based_on = attr_val(&e, "val");
                    }
                }
                b"jc" if in_style_p_pr => {
                    if let Some(s) = cur.as_mut() {
                        s.jc = attr_val(&e, "val");
                    }
                }
                b"ind" if in_style_p_pr => {
                    if let Some(s) = cur.as_mut() {
                        if let Some(v) = attr_val(&e, "firstLineChars") {
                            if let Ok(n) = v.parse() {
                                s.first_line_chars = Some(n);
                            }
                        }
                        if let Some(v) = attr_val(&e, "firstLine") {
                            if let Ok(n) = v.parse() {
                                s.first_line_twips = Some(n);
                            }
                        }
                    }
                }
                b"sz" if in_style_r_pr || in_r_pr => {
                    let v = attr_val(&e, "val").and_then(|v| v.parse::<u32>().ok());
                    if in_style_r_pr {
                        if let Some(s) = cur.as_mut() {
                            if s.sz_half_pt.is_none() {
                                s.sz_half_pt = v;
                            }
                        }
                    } else if in_r_pr {
                        doc_defaults_sz = doc_defaults_sz.or(v);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"style" => {
                    if let Some(s) = cur.take() {
                        styles.push(s);
                    }
                    in_style = false;
                    in_style_p_pr = false;
                    in_style_r_pr = false;
                }
                b"pPr" => in_style_p_pr = false,
                b"rPr" if in_style => in_style_r_pr = false,
                b"docDefaults" => in_doc_defaults = false,
                b"rPrDefault" => in_r_pr_default = false,
                b"rPr" if in_r_pr => in_r_pr = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break, // styles.xml 解析失败时降级为无样式（仅影响缩进/字号回退）
            _ => {}
        }
    }
    (styles, doc_defaults_sz)
}

/// 对齐：CENTER/RIGHT 特殊，其余（both/justify/distribute/left/...）归 left（与 Python 一致）。
fn resolve_align(fmt: &ParaFmt) -> Align {
    match fmt.jc.as_deref() {
        Some("center") => Align::Center,
        Some("right") => Align::Right,
        _ => Align::Left,
    }
}

/// 沿 `based_on` 收集样式链（含起始，去环）。
fn style_chain<'a>(start: &str, styles: &'a [StyleDef]) -> Vec<&'a StyleDef> {
    let mut chain = Vec::new();
    let mut cur = start;
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(cur.to_string()) {
            break;
        }
        let Some(s) = styles.iter().find(|s| s.style_id == cur) else {
            break;
        };
        chain.push(s);
        let Some(base) = s.based_on.as_deref() else { break };
        cur = base;
    }
    chain
}

/// 沿样式链取首个非零字号（pt）。
fn style_chain_size_pt(start: &str, styles: &[StyleDef]) -> Option<f32> {
    style_chain(start, styles)
        .into_iter()
        .find_map(|s| s.sz_half_pt.filter(|&h| h > 0).map(|h| h as f32 / 2.0))
}

/// 沿样式链取首个 `firstLineChars`（缩进继承）。
fn style_chain_first_line_chars(start: &str, styles: &[StyleDef]) -> Option<i32> {
    style_chain(start, styles)
        .into_iter()
        .find_map(|s| s.first_line_chars)
}

/// 文档字号探测（pt）。级联：run 直接格式 > 段落样式链（沿 based_on）> Normal > docDefaults > 12。
fn doc_font_size_pt(fmt: &ParaFmt, styles: &[StyleDef], doc_defaults_sz: Option<u32>) -> f32 {
    if let Some(half) = fmt.run_sz_half_pt.filter(|&h| h > 0) {
        return half as f32 / 2.0;
    }
    if let Some(ref id) = fmt.style_id {
        if let Some(pt) = style_chain_size_pt(id, styles) {
            return pt;
        }
    }
    if let Some(pt) = style_chain_size_pt("Normal", styles) {
        return pt;
    }
    if let Some(half) = doc_defaults_sz.filter(|&h| h > 0) {
        return half as f32 / 2.0;
    }
    12.0
}

/// 首行缩进（像素）。三级回退：
/// 1. 直接格式 `firstLineChars`；2. 直接格式 `firstLine`（twips，按文档字号还原字符数）；
/// 3. 段落所应用样式沿 `based_on` 链继承的 `firstLineChars`。
fn resolve_indent(fmt: &ParaFmt, font_size: f32, styles: &[StyleDef], doc_defaults_sz: Option<u32>) -> f32 {
    if let Some(chars) = fmt.first_line_chars {
        return chars as f32 / 100.0 * font_size;
    }
    if let Some(twips) = fmt.first_line_twips {
        // twips → pt（1pt = 20 twips）→ 按文档字号还原字符数 → × 渲染字号
        let pt = twips as f32 / 20.0;
        let doc_font_size = doc_font_size_pt(fmt, styles, doc_defaults_sz);
        if doc_font_size > 0.0 {
            return pt / doc_font_size * font_size;
        }
    }
    if let Some(ref id) = fmt.style_id {
        if let Some(chars) = style_chain_first_line_chars(id, styles) {
            return chars as f32 / 100.0 * font_size;
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 手动构造 document.xml 并打包 zip，精确控制首行缩进/对齐/字号属性。
    fn build_docx(paragraphs: &[(&str, &str, Option<i32>)]) -> Vec<u8> {
        let mut body = String::new();
        for (text, align, first_line_chars) in paragraphs {
            let mut ind = String::new();
            if let Some(chars) = first_line_chars {
                ind.push_str(&format!(r#"<w:ind w:firstLineChars="{chars}"/>"#));
            }
            body.push_str(&format!(
                r#"<w:p><w:pPr><w:jc w:val="{align}"/>{ind}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
            ));
        }
        let document_xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        zip_docx(document_xml.as_bytes(), None)
    }

    /// 打包最小 docx（store 无压缩）：document.xml + 必需关系/类型文件。
    /// 传入 `styles_xml` 时，在 [Content_Types] 与 document.xml.rels 中声明 styles 关系并加入该 part。
    fn zip_docx(document_xml: &[u8], styles_xml: Option<&[u8]>) -> Vec<u8> {
        let has_styles = styles_xml.is_some();
        let content_types: &[u8] = if has_styles {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#.as_slice()
        } else {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.as_slice()
        };
        let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let doc_rels: &[u8] = if has_styles {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.as_slice()
        } else {
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#.as_slice()
        };
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", root_rels),
            ("word/document.xml", document_xml),
            ("word/_rels/document.xml.rels", doc_rels),
        ];
        if let Some(styles) = styles_xml {
            entries.push(("word/styles.xml", styles));
        }
        zip_store(&entries)
    }

    /// 极简 zip 打包（store 无压缩 + CRC32）。
    fn zip_store(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        let mut offset = 0u32;
        for (name, data) in entries {
            let name = name.as_bytes();
            let crc = crc32(data);
            // local header
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name);
            out.extend_from_slice(data);
            // central directory
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
            offset += (30 + name.len() + data.len()) as u32;
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        // end of central directory
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xffff_ffff;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn load_paragraphs_extracts_text_align_indent() {
        let bytes = build_docx(&[
            ("第一段居中", "center", Some(200)),
            ("第二段右对齐", "right", None),
            ("第三段默认", "left", None),
            ("   ", "left", None), // 空段应忽略
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.docx");
        std::fs::write(&path, bytes).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 3);
        assert_eq!(paras[0].text, "第一段居中");
        assert_eq!(paras[0].align, Align::Center);
        assert_eq!(paras[0].first_line_indent, 72.0); // 200/100 * 36
        assert_eq!(paras[1].align, Align::Right);
        assert_eq!(paras[1].first_line_indent, 0.0);
        assert_eq!(paras[2].text, "第三段默认");
        assert_eq!(paras[2].align, Align::Left);
        assert_eq!(paras[2].first_line_indent, 0.0);
    }

    #[test]
    fn load_paragraphs_trims_whitespace() {
        // 与 Python `para.text.strip()` 一致：首尾空白应被修剪
        let bytes = build_docx(&[("  首尾带空格  ", "left", None)]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trim.docx");
        std::fs::write(&path, bytes).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].text, "首尾带空格");
    }

    #[test]
    fn load_paragraphs_missing_file_reports_error() {
        let err = load_paragraphs(Path::new("C:/nonexistent/x.docx"), 36.0).unwrap_err();
        assert!(err.contains("失败"));
    }

    #[test]
    fn load_paragraphs_emulates_first_line_twips() {
        // `w:firstLine`（twips，1/20 pt）：480 twips = 24pt，
        // 文档字号兜底 12pt → 24/12=2 字符 × 渲染字号 36 = 72 像素。
        // （docx-rs 曾把 twips 误当 EMU 处理导致此路径缩进几乎为 0，手写解析已修正。）
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:ind w:firstLine="480"/></w:pPr><w:r><w:t>indent</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("twips.docx");
        // 无 styles.xml：doc 字号落到 12pt 兜底 → 24/12=2 字符 ×36=72。
        std::fs::write(&path, zip_docx(document_xml, None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].first_line_indent, 72.0);
    }

    #[test]
    fn indent_inherits_first_line_chars_from_style_chain() {
        // 段落自身无 firstLineChars/EMU，沿 pStyle→basedOn 链继承 BasePara 的 firstLineChars。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="MyPara"/></w:pPr><w:r><w:t>styled</w:t></w:r></w:p></w:body></w:document>"#;
        let styles_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="MyPara"><w:name w:val="My Para"/><w:basedOn w:val="BasePara"/></w:style><w:style w:type="paragraph" w:styleId="BasePara"><w:name w:val="Base Para"/><w:pPr><w:ind w:firstLineChars="200"/></w:pPr></w:style></w:styles>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("style-indent.docx");
        std::fs::write(&path, zip_docx(document_xml, Some(styles_xml))).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].first_line_indent, 72.0); // 200/100 * 36
    }

    #[test]
    fn doc_font_size_reads_run_sz() {
        // run rPr sz=48（半磅）→ 24pt；firstLine 480 twips = 24pt → 24/24=1 字符 ×36=36。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:ind w:firstLine="480"/></w:pPr><w:r><w:rPr><w:sz w:val="48"/></w:rPr><w:t>run</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run-sz.docx");
        std::fs::write(&path, zip_docx(document_xml, None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras[0].first_line_indent, 36.0);
    }

    #[test]
    fn doc_font_size_reads_paragraph_style_sz() {
        // 段落无 run sz，但 pStyle=MyPara 的 rPr sz=48 → 24pt → firstLine 480 twips=24pt → 1 字符 ×36=36。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="MyPara"/><w:ind w:firstLine="480"/></w:pPr><w:r><w:t>styled</w:t></w:r></w:p></w:body></w:document>"#;
        let styles_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="MyPara"><w:name w:val="My Para"/><w:rPr><w:sz w:val="48"/></w:rPr></w:style></w:styles>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("style-sz.docx");
        std::fs::write(&path, zip_docx(document_xml, Some(styles_xml))).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras[0].first_line_indent, 36.0);
    }

    #[test]
    fn doc_font_size_reads_doc_defaults_sz() {
        // 无 run sz、无 pStyle：docDefaults rPrDefault sz=36 → 18pt → firstLine 360 twips=18pt → 1 字符 ×36=36。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:ind w:firstLine="360"/></w:pPr><w:r><w:t>default</w:t></w:r></w:p></w:body></w:document>"#;
        let styles_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="36"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default-sz.docx");
        std::fs::write(&path, zip_docx(document_xml, Some(styles_xml))).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras[0].first_line_indent, 36.0);
    }

    #[test]
    fn skips_paragraphs_inside_tables() {
        // 对齐 python-docx doc.paragraphs：表格内段落不属于顶层段落
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>顶层段落</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>表格内段落</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].text, "顶层段落");
    }

    #[test]
    fn hyperlink_runs_included_in_text() {
        // w:hyperlink 内的 run 文本应被拼接（对齐 python-docx para.text）
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>前</w:t></w:r><w:hyperlink r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:r><w:t>链接</w:t></w:r></w:hyperlink><w:r><w:t>后</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].text, "前链接后");
    }

    #[test]
    fn test_docx_with_highlights() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>黄</w:t></w:r><w:r><w:rPr><w:highlight w:val="green"/></w:rPr><w:t>绿</w:t></w:r><w:r><w:rPr><w:highlight w:val="cyan"/></w:rPr><w:t>青</w:t></w:r><w:r><w:rPr><w:highlight w:val="magenta"/></w:rPr><w:t>品红</w:t></w:r><w:r><w:rPr><w:highlight w:val="lightGray"/></w:rPr><w:t>印刷灰</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("highlights.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs.len(), 5);
        assert_eq!(paras[0].runs[0].text, "黄");
        assert_eq!(paras[0].runs[0].style.role_id, 2);
        assert_eq!(paras[0].runs[0].style.highlight.as_deref(), Some("yellow"));
        assert_eq!(paras[0].runs[1].text, "绿");
        assert_eq!(paras[0].runs[1].style.role_id, 3);
        assert_eq!(paras[0].runs[1].style.highlight.as_deref(), Some("green"));
        assert_eq!(paras[0].runs[2].text, "青");
        assert_eq!(paras[0].runs[2].style.role_id, 4);
        assert_eq!(paras[0].runs[2].style.highlight.as_deref(), Some("cyan"));
        assert_eq!(paras[0].runs[3].text, "品红");
        assert_eq!(paras[0].runs[3].style.role_id, 5);
        assert_eq!(paras[0].runs[3].style.highlight.as_deref(), Some("magenta"));
        assert_eq!(paras[0].runs[4].text, "印刷灰");
        assert_eq!(paras[0].runs[4].style.role_id, 1);
        assert_eq!(paras[0].runs[4].style.highlight.as_deref(), Some("lightGray"));
        assert!(paras[0].runs[4].style.printed);
    }

    #[test]
    fn test_docx_dynamic_color_allocation() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:highlight w:val="pink"/></w:rPr><w:t>粉色段落</w:t></w:r></w:p><w:p><w:r><w:rPr><w:highlight w:val="darkBlue"/></w:rPr><w:t>深蓝段落</w:t></w:r></w:p><w:p><w:r><w:rPr><w:highlight w:val="pink"/></w:rPr><w:t>再次粉色</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dynamic_highlight.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 3);

        // 第 1 段：pink 首次出现 -> 分配 role_id: 2
        assert_eq!(paras[0].runs.len(), 1);
        assert_eq!(paras[0].runs[0].text, "粉色段落");
        assert_eq!(paras[0].runs[0].style.role_id, 2);
        assert_eq!(paras[0].runs[0].style.highlight.as_deref(), Some("pink"));
        assert!(!paras[0].runs[0].style.printed);

        // 第 2 段：darkBlue 首次出现 -> 分配 role_id: 3
        assert_eq!(paras[1].runs.len(), 1);
        assert_eq!(paras[1].runs[0].text, "深蓝段落");
        assert_eq!(paras[1].runs[0].style.role_id, 3);
        assert_eq!(paras[1].runs[0].style.highlight.as_deref(), Some("darkBlue"));
        assert!(!paras[1].runs[0].style.printed);

        // 第 3 段：pink 再次出现 -> 复用 role_id: 2
        assert_eq!(paras[2].runs.len(), 1);
        assert_eq!(paras[2].runs[0].text, "再次粉色");
        assert_eq!(paras[2].runs[0].style.role_id, 2);
        assert_eq!(paras[2].runs[0].style.highlight.as_deref(), Some("pink"));
        assert!(!paras[2].runs[0].style.printed);
    }

    #[test]
    fn test_docx_with_color_and_size() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:color w:val="FF0000"/><w:sz w:val="48"/></w:rPr><w:t>红色大字</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("color_size.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs.len(), 1);
        assert_eq!(paras[0].runs[0].text, "红色大字");
        assert_eq!(paras[0].runs[0].style.fill, Some([255, 0, 0]));
        assert_eq!(paras[0].runs[0].style.font_size, None);
    }

    #[test]
    fn test_docx_with_syntax_tags() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>正文{{手写:已批准}}后续</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("syntax_tags.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].text, "正文已批准后续");
        assert_eq!(paras[0].runs.len(), 3);
        assert_eq!(paras[0].runs[0].text, "正文");
        assert_eq!(paras[0].runs[0].style.role_id, 1);
        assert!(paras[0].runs[0].style.printed);
        assert_eq!(paras[0].runs[1].text, "已批准");
        assert_eq!(paras[0].runs[1].style.role_id, 2);
        assert!(!paras[0].runs[1].style.printed);
        assert_eq!(paras[0].runs[2].text, "后续");
        assert_eq!(paras[0].runs[2].style.role_id, 1);
        assert!(paras[0].runs[2].style.printed);
    }

    #[test]
    fn test_docx_with_extended_syntax_tags() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{{手写2:老师批注}}{{打印:注意事项}}{{1:学生A}}{{2:学生B}}{{默认手写}}</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extended_tags.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs.len(), 5);

        assert_eq!(paras[0].runs[0].text, "老师批注");
        assert_eq!(paras[0].runs[0].style.role_id, 3);
        assert!(!paras[0].runs[0].style.printed);

        assert_eq!(paras[0].runs[1].text, "注意事项");
        assert_eq!(paras[0].runs[1].style.role_id, 1);
        assert!(paras[0].runs[1].style.printed);

        assert_eq!(paras[0].runs[2].text, "学生A");
        assert_eq!(paras[0].runs[2].style.role_id, 1);
        assert!(!paras[0].runs[2].style.printed);

        assert_eq!(paras[0].runs[3].text, "学生B");
        assert_eq!(paras[0].runs[3].style.role_id, 2);
        assert!(!paras[0].runs[3].style.printed);

        assert_eq!(paras[0].runs[4].text, "默认手写");
        assert_eq!(paras[0].runs[4].style.role_id, 2);
        assert!(!paras[0].runs[4].style.printed);
    }

    #[test]
    fn test_docx_no_highlights_is_full_handwriting() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>第一段纯手写</w:t></w:r></w:p><w:p><w:r><w:t>第二段纯手写</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_highlights.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 2);
        for p in &paras {
            for r in &p.runs {
                assert_eq!(r.style.role_id, 0);
                assert!(!r.style.printed);
            }
        }
    }

    #[test]
    fn test_docx_printed_tag_survives_full_handwriting_mode() {
        // 文档无任何高亮：纯手写模式，但显式 {{打印:}} 标签应保留而不是被重置为手写
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{{打印:标题}}正文手写</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("printed_tag_only.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs.len(), 2);
        assert_eq!(paras[0].runs[0].text, "标题");
        assert_eq!(paras[0].runs[0].style.role_id, 1);
        assert!(paras[0].runs[0].style.printed);
        assert_eq!(paras[0].runs[1].text, "正文手写");
        assert_eq!(paras[0].runs[1].style.role_id, 0);
        assert!(!paras[0].runs[1].style.printed);
    }

    #[test]
    fn test_docx_nested_table_paragraphs_do_not_leak_into_body() {
        // 嵌套表格：内层 </w:tbl> 不应结束外层表格上下文，
        // 外层表格中的段落不应泄入正文段落列表（对齐 python-docx doc.paragraphs）
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>正文段落</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>内层表格</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>外层表格段落</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested_table.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1, "表格内段落（含嵌套）不应出现在正文段落中：{paras:?}");
        assert_eq!(paras[0].runs[0].text, "正文段落");
    }

    #[test]
    fn test_docx_textbox_content_is_skipped() {
        // 段落中的文本框（w:txbxContent）内含独立 w:p/w:r：
        // 应整体跳过，不得清空外层已累积 runs 或提前闭合外层段落
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>前文</w:t></w:r><w:r><w:drawing><wps:txbx><w:txbxContent><w:p><w:r><w:t>文本框内容</w:t></w:r></w:p></w:txbxContent></wps:txbx></w:drawing></w:r><w:r><w:t>后文</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("textbox.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1, "文本框内段落不应成为独立正文段落：{paras:?}");
        let text: String = paras[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "前文后文", "文本框文字不应混入正文，外层段落文字不应丢失");
    }

    #[test]
    fn test_docx_with_highlights_converts_unmarked_to_printed() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>未高亮模板前缀</w:t></w:r><w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>黄色高亮填空</w:t></w:r><w:r><w:t>未高亮模板后缀</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dual_mode.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs.len(), 3);

        // Run 0: unmarked -> converted to printed
        assert_eq!(paras[0].runs[0].text, "未高亮模板前缀");
        assert_eq!(paras[0].runs[0].style.role_id, 1);
        assert!(paras[0].runs[0].style.printed);

        // Run 1: yellow highlight -> role_id 2, handwriting (printed = false)
        assert_eq!(paras[0].runs[1].text, "黄色高亮填空");
        assert_eq!(paras[0].runs[1].style.role_id, 2);
        assert_eq!(paras[0].runs[1].style.highlight.as_deref(), Some("yellow"));
        assert!(!paras[0].runs[1].style.printed);

        // Run 2: unmarked -> converted to printed
        assert_eq!(paras[0].runs[2].text, "未高亮模板后缀");
        assert_eq!(paras[0].runs[2].style.role_id, 1);
        assert!(paras[0].runs[2].style.printed);
    }

    #[test]
    fn test_docx_font_size_not_overridden() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:sz w:val="28"/></w:rPr><w:t>字号测试</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("font_sz.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs[0].style.font_size, None);
    }

    #[test]
    fn test_docx_rfonts_extraction() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:rFonts w:eastAsia="仿宋_GB2312" w:ascii="Calibri"/></w:rPr><w:t>字体测试</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rfonts.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].runs[0].style.font_family.as_deref(), Some("仿宋_GB2312"));
        assert_eq!(detect_doc_font_family(&paras).as_deref(), Some("仿宋_GB2312"));
    }

    #[test]
    fn test_docx_multi_font_family_extraction() {
        let document_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:rFonts w:eastAsia="黑体"/></w:rPr><w:t>黑体标题</w:t></w:r><w:r><w:rPr><w:rFonts w:eastAsia="宋体"/></w:rPr><w:t>宋体副标题</w:t></w:r></w:p><w:p><w:r><w:rPr><w:rFonts w:eastAsia="仿宋_GB2312"/></w:rPr><w:t>仿宋正文</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi_font.docx");
        std::fs::write(&path, zip_docx(document_xml.as_bytes(), None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].runs.len(), 2);
        assert_eq!(paras[0].runs[0].text, "黑体标题");
        assert_eq!(paras[0].runs[0].style.font_family.as_deref(), Some("黑体"));
        assert_eq!(paras[0].runs[1].text, "宋体副标题");
        assert_eq!(paras[0].runs[1].style.font_family.as_deref(), Some("宋体"));

        assert_eq!(paras[1].runs.len(), 1);
        assert_eq!(paras[1].runs[0].text, "仿宋正文");
        assert_eq!(paras[1].runs[0].style.font_family.as_deref(), Some("仿宋_GB2312"));
    }
}