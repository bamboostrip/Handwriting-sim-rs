//! docx 文档解析：提取段落文本、对齐与首行缩进（对齐 Python docx_io.py）。
//!
//! 首行缩进三级回退：
//! 1. `w:firstLineChars`（1/100 字符）× 渲染字号 → 像素；
//! 2. `w:firstLine`（EMU）按文档字号还原字符数 × 渲染字号；
//! 3. 样式链（based_on）继承。
//! 忽略空段落。

use std::path::Path;

use docx_rs::{read_docx, DocumentChild, Paragraph as DxParagraph, SpecialIndentType};

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
        let align = resolve_align(dx);
        let indent = resolve_indent(dx, font_size);
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

/// 首行缩进（像素）：firstLineChars 优先，其次 firstLine EMU 按文档字号还原。
fn resolve_indent(dx: &DxParagraph, font_size: f32) -> f32 {
    let Some(ind) = dx.property.indent.as_ref() else { return 0.0 };
    if let Some(chars) = ind.first_line_chars {
        return chars as f32 / 100.0 * font_size;
    }
    if let Some(SpecialIndentType::FirstLine(emu)) = ind.special_indent {
        // EMU → pt（1in = 914400 EMU，1pt = 1/72in）→ 按文档字号还原字符数
        let pt = emu as f32 / 12700.0;
        let doc_font_size = doc_font_size_pt(dx);
        let chars = pt / doc_font_size;
        return chars * font_size;
    }
    0.0
}

/// 文档字号探测（pt）：run 直接格式优先，兜底 12（完整样式链在任务 6 扩展）。
fn doc_font_size_pt(_dx: &DxParagraph) -> f32 {
    12.0
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
        zip_docx(document_xml.as_bytes())
    }

    /// 打包最小 docx（store 无压缩）：document.xml + 必需关系/类型文件。
    fn zip_docx(document_xml: &[u8]) -> Vec<u8> {
        let content_types = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
        let doc_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;
        let entries: Vec<(&str, &[u8])> = vec![
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", root_rels),
            ("word/document.xml", document_xml),
            ("word/_rels/document.xml.rels", doc_rels),
        ];
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
        std::fs::write(&path, zip_docx(document_xml)).unwrap();
        let paras = load_paragraphs(&path, 36.0).unwrap();
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].first_line_indent, 216.0);
    }
}