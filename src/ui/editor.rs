//! 段落编辑器（egui 版）：每段一个 TextEdit + 段级格式。
//!
//! 与 iced 版差异：段直接持有 `String`（egui TextEdit 即时编辑 String），
//! 光标位置由 UI 层（`TextEditOutput::cursor_range`）传入逻辑层，
//! 因此逻辑层是纯函数，无 iced `Content::move_to` 的非 ASCII bug——
//! 可对任意 Unicode 文本做光标处拆段测试。
//!
//! 关键约定：**段内文本不含 `\n`**（回车=分段、粘贴多行自动拆段），
//! 保证导出时按段拼接的语义与引擎段落模型一致。

use crate::core::models::{Align, Paragraph};

/// 段落格式（对齐 + 首行缩进）。
/// `align`：0 左对齐 / 1 居中 / 2 右对齐（与 `models::Align` 语义一致）。
/// `indent_em`：首行缩进字符数（渲染时 × 当前字号换算像素）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParaFormat {
    pub align: u8,
    pub indent_em: f32,
}

/// 默认首行缩进（对齐 iced/Slint 版「首行缩进」按钮，恒为 2 字符宽）。
pub const DEFAULT_INDENT_EM: f32 = 2.0;

/// 单段编辑器：一个 `String`（egui TextEdit 直接编辑）+ 段格式。
#[derive(Debug, Clone, Default)]
pub struct ParaEditor {
    pub text: String,
    pub format: ParaFormat,
}

impl ParaEditor {
    pub fn new(text: &str, format: ParaFormat) -> Self {
        Self {
            text: text.to_string(),
            format,
        }
    }
}

/// 段落编辑器：段列表 + 当前光标所在段。
#[derive(Debug, Default)]
pub struct ParagraphEditor {
    pub paras: Vec<ParaEditor>,
    pub current: usize,
}

impl ParagraphEditor {
    /// 空编辑器：单空段。
    pub fn empty() -> Self {
        Self {
            paras: vec![ParaEditor::default()],
            current: 0,
        }
    }

    /// 用文本（`\n` 分段）+ 格式数组整篇替换（docx 导入 / 载入时）。
    /// 格式数组长度与段数不同时按需截断/补默认。
    pub fn set_text(&mut self, text: &str, formats: Vec<ParaFormat>) {
        let texts: Vec<&str> = text.split('\n').collect();
        self.paras = texts
            .iter()
            .enumerate()
            .map(|(i, t)| ParaEditor::new(t, formats.get(i).copied().unwrap_or_default()))
            .collect();
        if self.paras.is_empty() {
            self.paras.push(ParaEditor::default());
        }
        self.current = 0;
    }

    /// 整篇文本（`\n` 拼接，供导出/预览收集）。
    pub fn text(&self) -> String {
        self.paras
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 回车分段：在 `byte_pos` 处拆分当前段，后半段继承格式，返回新段索引。
    /// `byte_pos` 由 UI 层从 `TextEditOutput::cursor_range` 提供。
    pub fn split(&mut self, para: usize, byte_pos: usize) -> Option<usize> {
        let editor = self.paras.get_mut(para)?;
        let mut pos = byte_pos.min(editor.text.len());
        while pos > 0 && !editor.text.is_char_boundary(pos) {
            pos -= 1;
        }
        let after = editor.text.split_off(pos);
        let format = editor.format;
        self.paras.insert(para + 1, ParaEditor::new(&after, format));
        self.current = para + 1;
        Some(para + 1)
    }

    /// 段首退格：并入上一段（格式保留上一段），返回合并后的段索引。
    /// 段 0 或越界时无操作。
    pub fn merge_prev(&mut self, para: usize) -> Option<usize> {
        if para == 0 || para >= self.paras.len() {
            return None;
        }
        let tail = self.paras[para].text.clone();
        self.paras[para - 1].text.push_str(&tail);
        self.paras.remove(para);
        if self.paras.is_empty() {
            self.paras.push(ParaEditor::default());
        }
        self.current = para - 1;
        Some(para - 1)
    }

    /// 段内含 `\n`（egui 回车/粘贴插入）→ 拆成多段，新段继承原段格式。
    /// 返回拆分后该 para 区域的最后一段索引（UI 聚焦用）。无 `\n` 时返回 None。
    pub fn split_para_at_newlines(&mut self, para: usize) -> Option<usize> {
        let (text, format) = {
            let editor = self.paras.get_mut(para)?;
            (std::mem::take(&mut editor.text), editor.format)
        };
        if !text.contains('\n') {
            // 无换行：把取出的文本放回
            self.paras[para].text = text;
            return None;
        }
        let parts: Vec<&str> = text.split('\n').collect();
        let new_paras: Vec<ParaEditor> =
            parts.iter().map(|p| ParaEditor::new(p, format)).collect();
        let last = para + new_paras.len() - 1;
        self.paras.splice(para..para + 1, new_paras);
        self.current = last;
        Some(last)
    }

    /// 光标所在段（对齐/缩进按钮的作用目标）。
    pub fn cursor_paragraph(&self) -> usize {
        self.current.min(self.paras.len().saturating_sub(1))
    }

    /// 对齐按钮：作用于光标所在段。
    pub fn set_align(&mut self, align: u8) {
        let i = self.cursor_paragraph();
        if let Some(p) = self.paras.get_mut(i) {
            p.format.align = align.clamp(0, 2);
        }
    }

    /// 首行缩进开关：作用于光标所在段（对齐 iced/Slint 版 2 字符缩进）。
    pub fn toggle_indent(&mut self, on: bool) {
        let i = self.cursor_paragraph();
        if let Some(p) = self.paras.get_mut(i) {
            p.format.indent_em = if on { DEFAULT_INDENT_EM } else { 0.0 };
        }
    }

    /// 当前段格式（状态栏提示用）。
    pub fn current_format(&self) -> Option<ParaFormat> {
        self.paras.get(self.cursor_paragraph()).map(|p| p.format)
    }
}

/// 编辑器内容转换为引擎段落（跳过全空白段；保留首尾空格参与排版占宽，
/// 与 iced/Slint 版行为一致）。返回 `(段落列表, 是否存在非默认格式)`。
pub fn paragraphs_from_editor(
    editor: &ParagraphEditor,
    font_size: f32,
) -> (Vec<Paragraph>, bool) {
    let mut paras = Vec::new();
    let mut has_format = false;
    for para in &editor.paras {
        if para.text.trim().is_empty() {
            continue;
        }
        if para.format.align != 0 || para.format.indent_em != 0.0 {
            has_format = true;
        }
        paras.push(Paragraph {
            text: clean_editor_spaces(&para.text),
            align: match para.format.align {
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            },
            first_line_indent: para.format.indent_em * font_size,
        });
    }
    (paras, has_format)
}

/// 清理外来文本中的特殊空白（NBSP/FFA0/WJ 还原为普通空格），
/// 对应 iced/Slint 版 `to_ui_spaces` 的语义。
pub fn clean_editor_spaces(s: &str) -> String {
    s.replace('\u{2060}', "")
        .replace(['\u{00a0}', '\u{ffa0}'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> ParaEditor {
        ParaEditor::new(text, ParaFormat::default())
    }

    #[test]
    fn split_at_cursor_keeps_format_ascii() {
        let mut ed = ParagraphEditor {
            paras: vec![ParaEditor::new("abcd", ParaFormat { align: 1, indent_em: 2.0 })],
            current: 0,
        };
        let new_idx = ed.split(0, 2).unwrap(); // 光标在「c」前（字节 2）
        assert_eq!(new_idx, 1);
        assert_eq!(ed.paras[0].text, "ab");
        assert_eq!(ed.paras[1].text, "cd");
        assert_eq!(ed.paras[1].format, ParaFormat { align: 1, indent_em: 2.0 });
    }

    #[test]
    fn split_at_cursor_handles_unicode() {
        // egui 版无 iced move_to bug：中文字符光标处拆段正确
        let mut ed = ParagraphEditor {
            paras: vec![ParaEditor::new("你好世界", ParaFormat::default())],
            current: 0,
        };
        // 「你好」= 字节 0..6，「世界」= 6..12 → 在「你好」后（字节 6）拆分
        ed.split(0, 6).unwrap();
        assert_eq!(ed.paras[0].text, "你好");
        assert_eq!(ed.paras[1].text, "世界");
    }

    #[test]
    fn split_byte_pos_snaps_to_char_boundary() {
        let mut ed = ParagraphEditor {
            paras: vec![para("你好")],
            current: 0,
        };
        // 传入非字符边界（字节 1）→ 回退到 0
        ed.split(0, 1).unwrap();
        assert_eq!(ed.paras[0].text, "");
        assert_eq!(ed.paras[1].text, "你好");
    }

    #[test]
    fn split_out_of_range_is_none() {
        let mut ed = ParagraphEditor::empty();
        assert!(ed.split(5, 0).is_none());
    }

    #[test]
    fn merge_prev_keeps_first_format() {
        let mut ed = ParagraphEditor {
            paras: vec![
                ParaEditor::new("第一段", ParaFormat { align: 0, indent_em: 2.0 }),
                ParaEditor::new("第二段", ParaFormat { align: 1, indent_em: 0.0 }),
            ],
            current: 1,
        };
        let merged = ed.merge_prev(1).unwrap();
        assert_eq!(merged, 0);
        assert_eq!(ed.paras.len(), 1);
        assert_eq!(ed.paras[0].text, "第一段第二段");
        assert_eq!(ed.paras[0].format.indent_em, 2.0);
    }

    #[test]
    fn merge_prev_at_zero_is_noop() {
        let mut ed = ParagraphEditor::empty();
        assert!(ed.merge_prev(0).is_none());
        assert_eq!(ed.paras.len(), 1);
    }

    #[test]
    fn split_para_at_newlines_splits_and_inherits_format() {
        let mut ed = ParagraphEditor {
            paras: vec![ParaEditor::new(
                "开头\nA\nB",
                ParaFormat { align: 2, indent_em: 1.0 },
            )],
            current: 0,
        };
        let last = ed.split_para_at_newlines(0).unwrap();
        assert_eq!(ed.paras.len(), 3);
        assert_eq!(ed.paras[0].text, "开头");
        assert_eq!(ed.paras[1].text, "A");
        assert_eq!(ed.paras[2].text, "B");
        assert_eq!(ed.paras[2].format, ParaFormat { align: 2, indent_em: 1.0 });
        assert_eq!(last, 2);
        assert_eq!(ed.current, 2);
    }

    #[test]
    fn split_para_at_newlines_no_newline_is_none() {
        let mut ed = ParagraphEditor {
            paras: vec![para("纯文本")],
            current: 0,
        };
        assert!(ed.split_para_at_newlines(0).is_none());
        assert_eq!(ed.paras[0].text, "纯文本");
    }

    #[test]
    fn set_text_rebuilds_paras() {
        let mut ed = ParagraphEditor::empty();
        ed.set_text(
            "第一段\n\n第三段",
            vec![
                ParaFormat { align: 1, indent_em: 2.0 },
                ParaFormat::default(),
                ParaFormat { align: 2, indent_em: 0.0 },
            ],
        );
        assert_eq!(ed.paras.len(), 3);
        assert_eq!(ed.paras[0].format.align, 1);
        assert_eq!(ed.paras[2].format.align, 2);
        assert_eq!(ed.text(), "第一段\n\n第三段");
    }

    #[test]
    fn set_align_and_toggle_indent_target_cursor_paragraph() {
        let mut ed = ParagraphEditor {
            paras: vec![para("一段"), para("二段")],
            current: 0,
        };
        ed.set_align(1);
        assert_eq!(ed.paras[0].format.align, 1);
        assert_eq!(ed.paras[1].format.align, 0);
        ed.current = 1;
        ed.toggle_indent(true);
        assert_eq!(ed.paras[0].format.indent_em, 0.0);
        assert_eq!(ed.paras[1].format.indent_em, 2.0);
    }

    #[test]
    fn paragraphs_skip_blank_and_map_align() {
        let ed = ParagraphEditor {
            paras: vec![
                ParaEditor::new("第一段", ParaFormat { align: 1, indent_em: 2.0 }),
                para(""),
                ParaEditor::new("第三段", ParaFormat { align: 2, indent_em: 0.0 }),
            ],
            current: 0,
        };
        let (paras, has_format) = paragraphs_from_editor(&ed, 36.0);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].text, "第一段");
        assert_eq!(paras[0].align, Align::Center);
        assert_eq!(paras[0].first_line_indent, 72.0);
        assert_eq!(paras[1].text, "第三段");
        assert_eq!(paras[1].align, Align::Right);
        assert!(has_format);
    }

    #[test]
    fn paragraphs_preserve_leading_trailing_spaces() {
        let ed = ParagraphEditor {
            paras: vec![para("  带空格  ")],
            current: 0,
        };
        let (paras, _) = paragraphs_from_editor(&ed, 36.0);
        assert_eq!(paras[0].text, "  带空格  ");
    }

    #[test]
    fn clean_spaces_normalizes_nbsp() {
        // NBSP/FFA0 → 普通空格；WJ 删除（b 与 c 直接相邻）
        assert_eq!(clean_editor_spaces("a\u{00a0}b\u{2060}c\u{ffa0}d"), "a bc d");
    }
}
