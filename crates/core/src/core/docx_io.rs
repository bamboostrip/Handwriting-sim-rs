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

use crate::core::models::{Align, Paragraph};

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

/// 从 docx 读取段落（忽略空段），对齐/首行缩进还原。
pub fn load_paragraphs(path: &Path, font_size: f32) -> Result<Vec<Paragraph>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 docx {path:?} 失败：{e}"))?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|e| format!("解压 docx {path:?} 失败：{e}"))?;

    let document_xml = read_entry(&mut archive, "word/document.xml")?;
    let styles_xml = read_entry_optional(&mut archive, "word/styles.xml");

    let (texts, fmts) = parse_document(&document_xml)?;
    let (styles, doc_defaults_sz) = parse_styles(&styles_xml);

    let mut result = Vec::new();
    for (text, fmt) in texts.into_iter().zip(fmts) {
        if text.trim().is_empty() {
            continue;
        }
        // 与 Python 版一致：存 `para.text.strip()` 后的文本
        let text = text.trim().to_string();
        let align = resolve_align(&fmt);
        let indent = resolve_indent(&fmt, font_size, &styles, doc_defaults_sz);
        result.push(Paragraph { text, align, first_line_indent: indent, runs: Vec::new() });
    }
    Ok(result)
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

/// 解析 document.xml，返回 (段落文本列表, 段落直接格式列表)。
fn parse_document(xml: &str) -> Result<(Vec<String>, Vec<ParaFmt>), String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut texts = Vec::new();
    let mut fmts = Vec::new();
    // 状态
    let mut in_body_para = false; // 当前 w:p 是 body 直接子级（对齐 python-docx doc.paragraphs）
    let mut in_table = false;
    let mut in_p_pr = false;
    let mut in_run = false;
    let mut in_r_pr = false;
    let mut cur_text = String::new();
    let mut cur_fmt = ParaFmt::default();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                b"tbl" => in_table = true,
                b"p" if !in_table => {
                    in_body_para = true;
                    cur_text.clear();
                    cur_fmt = ParaFmt::default();
                }
                b"pPr" if in_body_para => in_p_pr = true,
                b"r" if in_body_para => in_run = true,
                b"rPr" if in_run => in_r_pr = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match local_name(e.name().as_ref()) {
                b"tab" if in_body_para => cur_text.push('\t'),
                b"br" if in_body_para => cur_text.push('\n'),
                b"pStyle" if in_p_pr => {
                    if let Some(v) = attr_val(&e, "val") {
                        cur_fmt.style_id = Some(v);
                    }
                }
                b"jc" if in_p_pr => {
                    if let Some(v) = attr_val(&e, "val") {
                        cur_fmt.jc = Some(v);
                    }
                }
                b"ind" if in_p_pr => read_ind_attrs(&e, &mut cur_fmt),
                b"sz" if in_r_pr && cur_fmt.run_sz_half_pt.is_none() => {
                    cur_fmt.run_sz_half_pt = attr_val(&e, "val").and_then(|v| v.parse().ok());
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_body_para {
                    if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                        if let Ok(v) = unescape(s) {
                            cur_text.push_str(&v);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                b"tbl" => in_table = false,
                b"p" if in_body_para => {
                    texts.push(std::mem::take(&mut cur_text));
                    fmts.push(std::mem::take(&mut cur_fmt));
                    in_body_para = false;
                    in_p_pr = false;
                    in_run = false;
                    in_r_pr = false;
                }
                b"pPr" => in_p_pr = false,
                b"r" => {
                    in_run = false;
                    in_r_pr = false;
                }
                b"rPr" => in_r_pr = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("解析 document.xml 失败：{e}")),
            _ => {}
        }
    }
    Ok((texts, fmts))
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
}