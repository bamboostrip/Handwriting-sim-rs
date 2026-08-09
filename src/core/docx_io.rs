//! docx 文档解析：提取段落文本、对齐与首行缩进（对齐 Python docx_io.py）。
//!
//! 首行缩进三级回退：
//! 1. `w:firstLineChars`（1/100 字符）× 渲染字号 → 像素；
//! 2. `w:firstLine`（EMU）按文档字号还原字符数 × 渲染字号；
//! 3. 样式链（based_on）继承。
//! 4. 忽略空段落。

use std::path::Path;

use docx_rs::{
    read_docx, BasedOn, DocumentChild, Paragraph as DxParagraph, ParagraphChild, RunProperty, Sz,
    SpecialIndentType, Styles,
};

use crate::core::models::{Align, Paragraph};

/// 从 docx 读取段落（忽略空段），对齐/首行缩进还原。
pub fn load_paragraphs(path: &Path, font_size: f32) -> Result<Vec<Paragraph>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 docx {path:?} 失败：{e}"))?;
    let docx = read_docx(&bytes).map_err(|e| format!("解析 docx {path:?} 失败：{e}"))?;
    let mut result = Vec::new();
    for child in &docx.document.children {
        let DocumentChild::Paragraph(dx) = child else { continue };
        let text = paragraph_text(dx);
        if text.trim().is_empty() {
            continue;
        }
        // 与 Python 版一致：存 `para.text.strip()` 后的文本
        let text = text.trim().to_string();
        let align = resolve_align(dx);
        let indent = resolve_indent(dx, font_size, &docx.styles);
        result.push(Paragraph { text, align, first_line_indent: indent });
    }
    Ok(result)
}

/// 拼接段落文本：w:tab → \t，w:br → \n（对齐 python-docx para.text 语义）。
fn paragraph_text(dx: &DxParagraph) -> String {
    let mut out = String::new();
    for item in &dx.children {
        match item {
            docx_rs::ParagraphChild::Run(run) => {
                for child in &run.children {
                    match child {
                        docx_rs::RunChild::Text(t) => out.push_str(t.text.as_str()),
                        docx_rs::RunChild::Tab(_) => out.push('\t'),
                        docx_rs::RunChild::Break(_) => out.push('\n'),
                        docx_rs::RunChild::CarriageReturn(_) => out.push('\n'),
                        _ => {}
                    }
                }
            }
            docx_rs::ParagraphChild::Hyperlink(h) => {
                for item in &h.children {
                    if let docx_rs::ParagraphChild::Run(run) = item {
                        for child in &run.children {
                            if let docx_rs::RunChild::Text(t) = child {
                                out.push_str(t.text.as_str());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// 对齐：JUSTIFY/BOTH/DISTRIBUTE 归 left（与 Python `_resolve_align` 一致）。
fn resolve_align(dx: &DxParagraph) -> Align {
    match dx.property.alignment.as_ref().map(|j| j.val.as_str()) {
        Some("center") => Align::Center,
        Some("right") => Align::Right,
        _ => Align::Left,
    }
}

/// 首行缩进（像素）。三级回退：
/// 1. 直接格式 `firstLineChars`；2. 直接格式 `firstLine`（EMU，按探测文档字号还原字符数）；
/// 3. 段落所应用样式沿 `based_on` 链继承的 `firstLineChars`。
fn resolve_indent(dx: &DxParagraph, font_size: f32, styles: &Styles) -> f32 {
    if let Some(ind) = dx.property.indent.as_ref() {
        if let Some(chars) = ind.first_line_chars {
            return chars as f32 / 100.0 * font_size;
        }
        if let Some(SpecialIndentType::FirstLine(emu)) = ind.special_indent {
            // EMU → pt（1in = 914400 EMU，1pt = 1/72in）→ 按文档字号还原字符数
            let pt = emu as f32 / 12700.0;
            let doc_font_size = doc_font_size_pt(dx, styles);
            let chars = pt / doc_font_size;
            return chars * font_size;
        }
    }
    // 无直接格式时，沿样式链继承 firstLineChars
    if let Some(ps) = dx.property.style.as_ref() {
        if let Some(chars) = style_chain_first_line_chars(&ps.val, styles) {
            return chars as f32 / 100.0 * font_size;
        }
    }
    0.0
}

/// 文档字号探测（pt）。级联：run 直接格式 > 段落样式链（沿 based_on）> Normal > docDefaults > 12。
///
/// # docx-rs 0.4.22 读取能力限制
/// 本 crate 的 `Sz.val`、`BasedOn.val` 与 `DocDefaults` 内部字段均为私有且无 getter，
/// 无法直接读取。这里利用三者（及其容器）的 `serde::Serialize` 实现经 JSON 提取数值：
/// - `Sz` 序列化为裸 u32（半磅）；`BasedOn` 序列化为裸字符串（父样式 id）；
/// - `DocDefaults` 序列化为 `{"runPropertyDefault":{"runProperty":{"sz":…}}}` 可逐层取值。
fn doc_font_size_pt(dx: &DxParagraph, styles: &Styles) -> f32 {
    // 1) run 直接格式（取段落首个 run 的 rPr）
    if let Some(rp) = first_run_property(dx) {
        if let Some(sz) = rp.sz.as_ref() {
            if let Some(pt) = sz_half_points_to_pt(sz) {
                return pt;
            }
        }
    }
    // 2) 段落所应用样式链
    if let Some(ps) = dx.property.style.as_ref() {
        if let Some(pt) = style_chain_size(&ps.val, styles) {
            return pt;
        }
    }
    // 3) Normal 样式（未显式声明 pStyle 时隐含 Normal）
    if let Some(pt) = style_chain_size("Normal", styles) {
        return pt;
    }
    // 4) docDefaults
    if let Some(pt) = doc_defaults_size(styles) {
        return pt;
    }
    12.0
}

/// 段落首个 run 的 rPr（用于 run 级字号）。
fn first_run_property(dx: &DxParagraph) -> Option<&RunProperty> {
    dx.children.iter().find_map(|item| match item {
        ParagraphChild::Run(run) => Some(&run.run_property),
        _ => None,
    })
}

/// `Sz` 私有字段经 `Serialize`（裸 u32，半磅）读取，换算为 pt。
fn sz_half_points_to_pt(sz: &Sz) -> Option<f32> {
    let s = match serde_json::to_string(sz) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[docx_io] 警告：docx-rs 序列化字号字段结构变化，无法读取字号，已降级：{e}");
            return None;
        }
    };
    let half: u32 = match serde_json::from_str(&s) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[docx_io] 警告：docx-rs 字号字段序列化格式变化，无法读取字号，已降级：{e}");
            return None;
        }
    };
    if half == 0 {
        return None;
    }
    Some(half as f32 / 2.0)
}

/// `BasedOn` 私有字段经 `Serialize`（裸字符串）读取父样式 id。
fn based_on_id(b: &BasedOn) -> Option<String> {
    let s = match serde_json::to_string(b) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[docx_io] 警告：docx-rs 序列化 basedOn 字段结构变化，无法读取父样式，已降级：{e}");
            return None;
        }
    };
    match serde_json::from_str::<String>(&s) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[docx_io] 警告：docx-rs basedOn 字段序列化格式变化，无法读取父样式，已降级：{e}");
            None
        }
    }
}

/// 沿 `based_on` 收集样式 id 链（含起始，去环），字号与缩进共用这一条遍历。
fn style_chain_ids(start: &str, styles: &Styles) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cur = start.to_string();
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(cur.clone()) {
            break;
        }
        let Some(style) = styles.styles.iter().find(|s| s.style_id == cur) else {
            break;
        };
        chain.push(cur);
        let Some(base) = style.based_on.as_ref() else { break };
        let Some(base_id) = based_on_id(base) else { break };
        cur = base_id;
    }
    chain
}

/// 沿样式链取首个非零字号（pt）。
fn style_chain_size(start: &str, styles: &Styles) -> Option<f32> {
    for id in style_chain_ids(start, styles) {
        let Some(style) = styles.styles.iter().find(|s| s.style_id == id) else {
            continue;
        };
        if let Some(sz) = style.run_property.sz.as_ref() {
            if let Some(pt) = sz_half_points_to_pt(sz) {
                return Some(pt);
            }
        }
    }
    None
}

/// 沿样式链取首个 `firstLineChars`（缩进继承）。
fn style_chain_first_line_chars(start: &str, styles: &Styles) -> Option<i32> {
    for id in style_chain_ids(start, styles) {
        let Some(style) = styles.styles.iter().find(|s| s.style_id == id) else {
            continue;
        };
        let Some(ind) = style.paragraph_property.indent.as_ref() else {
            continue;
        };
        if let Some(chars) = ind.first_line_chars {
            return Some(chars);
        }
    }
    None
}

/// `DocDefaults` 私有内部字段经 `Serialize` 逐层提取运行默认字号（pt）。
fn doc_defaults_size(styles: &Styles) -> Option<f32> {
    let json = match serde_json::to_value(&styles.doc_defaults) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[docx_io] 警告：docx-rs 序列化 docDefaults 字段结构变化，无法读取默认字号，已降级：{e}");
            return None;
        }
    };
    let half = json
        .get("runPropertyDefault")?
        .get("runProperty")?
        .get("sz")?
        .as_u64()? as u32;
    if half == 0 {
        return None;
    }
    Some(half as f32 / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::AlignmentType;
    use std::path::Path;

    /// docx-rs 0.4 writer 无法输出 `w:firstLineChars`（`Indent::build_to` 只写
    /// `w:leftChars`/`w:firstLine`），故手动构造 document.xml 并打包 zip 来精确
    /// 控制首行缩进属性，验证 firstLineChars 分支。
    fn build_docx(paragraphs: &[(&str, AlignmentType, Option<i32>)]) -> Vec<u8> {
        let mut body = String::new();
        for (text, align, first_line_chars) in paragraphs {
            let jc = match align {
                AlignmentType::Center => "center",
                AlignmentType::Right => "right",
                _ => "left",
            };
            let mut ind = String::new();
            if let Some(chars) = first_line_chars {
                ind.push_str(&format!(r#"<w:ind w:firstLineChars="{chars}"/>"#));
            }
            body.push_str(&format!(
                r#"<w:p><w:pPr><w:jc w:val="{jc}"/>{ind}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
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
            out.extend_from_slice(&0x21u16.to_le_bytes());
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
            central.extend_from_slice(&0x21u16.to_le_bytes());
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
            ("第一段居中", AlignmentType::Center, Some(200)),
            ("第二段右对齐", AlignmentType::Right, None),
            ("第三段默认", AlignmentType::Left, None),
            ("   ", AlignmentType::Left, None), // 空段应忽略
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
        let bytes = build_docx(&[("  首尾带空格  ", AlignmentType::Left, None)]);
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
    fn load_paragraphs_emulates_first_line_emu() {
        // `w:firstLine`（EMU）按文档字号还原：914400 EMU = 1in = 72pt，
        // 文档字号 12pt → 6 字符 × 渲染字号 36 = 216 像素。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:ind w:firstLine="914400"/></w:pPr><w:r><w:t>indent</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("emu.docx");
        // 无 styles.xml：doc 字号落到 12pt 兜底 → 72/12=6 字符 ×36=216。
        std::fs::write(&path, zip_docx(document_xml, None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].first_line_indent, 216.0);
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
        // run rPr sz=48（半磅）→ 24pt；EMU 914400=72pt → 72/24=3 字符 ×36=108。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:ind w:firstLine="914400"/></w:pPr><w:r><w:rPr><w:sz w:val="48"/></w:rPr><w:t>run</w:t></w:r></w:p></w:body></w:document>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run-sz.docx");
        std::fs::write(&path, zip_docx(document_xml, None)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras[0].first_line_indent, 108.0);
    }

    #[test]
    fn doc_font_size_reads_paragraph_style_sz() {
        // 段落无 run sz，但 pStyle=MyPara 的 rPr sz=48 → 24pt → EMU 还原 3 字符 ×36=108。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="MyPara"/><w:ind w:firstLine="914400"/></w:pPr><w:r><w:t>styled</w:t></w:r></w:p></w:body></w:document>"#;
        let styles_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="MyPara"><w:name w:val="My Para"/><w:rPr><w:sz w:val="48"/></w:rPr></w:style></w:styles>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("style-sz.docx");
        std::fs::write(&path, zip_docx(document_xml, Some(styles_xml))).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras[0].first_line_indent, 108.0);
    }

    #[test]
    fn doc_font_size_reads_doc_defaults_sz() {
        // 无 run sz、无 pStyle：docDefaults rPrDefault sz=36 → 18pt → EMU 72/18=4 字符 ×36=144。
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:ind w:firstLine="914400"/></w:pPr><w:r><w:t>default</w:t></w:r></w:p></w:body></w:document>"#;
        let styles_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="36"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default-sz.docx");
        std::fs::write(&path, zip_docx(document_xml, Some(styles_xml))).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras[0].first_line_indent, 144.0);
    }
}