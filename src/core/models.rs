//! 手写模拟参数模型。
//!
//! 字段与默认值对齐 Python 版 `handwritesim.core.models`，
//! 便于未来用 Python 版 golden 样本做迁移验收。

use serde::{Deserialize, Serialize};

/// 段落对齐方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    /// 解析字面量（兼容 Python 版 "left"/"center"/"right"）。
    pub fn parse(s: &str) -> Result<Align, String> {
        match s {
            "left" => Ok(Align::Left),
            "center" => Ok(Align::Center),
            "right" => Ok(Align::Right),
            other => Err(format!("未知对齐方式：{other:?}，可选 left/center/right")),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// 单个段落的排版信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub text: String,
    pub align: Align,
    /// 首行缩进（像素）。
    pub first_line_indent: f32,
}

impl Default for Paragraph {
    fn default() -> Self {
        Self {
            text: String::new(),
            align: Align::Left,
            first_line_indent: 0.0,
        }
    }
}

/// 手写参数校验错误。
#[derive(Debug, thiserror::Error)]
pub enum ParamsError {
    #[error("未输入要处理的文字")]
    NoText,
    #[error("未指定字体文件")]
    NoFont,
    #[error("字体文件不存在：{0}")]
    FontMissing(String),
    #[error("未指定背景图片")]
    NoBackground,
    #[error("背景图片不存在：{0}")]
    BackgroundMissing(String),
    #[error("{name} 不能为负")]
    Negative { name: &'static str },
    #[error("颜色分量必须在 0-255 之间：{value}")]
    OutOfRangeColor { value: u8 },
}

/// 一次手写模拟的完整参数。
///
/// 排版参数用 `f32` 而非整数：预览降采样时边距/字号会按比例缩放为浮点，
/// 与 Python 版 `FastEngine` 的浮点参数行为一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandwritingParams {
    // ---- 输入 ----
    pub font_path: String,
    pub background_path: String,
    pub text: String,
    /// 非空时启用段落渲染（分段对齐/缩进）。
    pub paragraphs: Vec<Paragraph>,

    // ---- 字体颜色 (RGB) ----
    pub fill: [u8; 3],

    // ---- 排版 ----
    pub font_size: f32,
    pub word_spacing: f32,
    /// 行间距（不含字高）。
    pub line_spacing: f32,
    pub left_margin: f32,
    pub right_margin: f32,
    pub top_margin: f32,
    pub bottom_margin: f32,

    // ---- 随机扰动（正态分布标准差） ----
    pub word_spacing_sigma: f32,
    pub line_spacing_sigma: f32,
    pub font_size_sigma: f32,
    pub perturb_x_sigma: f32,
    pub perturb_y_sigma: f32,
    pub perturb_theta_sigma: f32,

    // ---- 排版细节 ----
    pub end_chars: String,
    pub start_chars: String,
}

impl Default for HandwritingParams {
    /// 默认值与 Python 版 `HandwritingParams()` 一致。
    fn default() -> Self {
        Self {
            font_path: String::new(),
            background_path: String::new(),
            text: String::new(),
            paragraphs: Vec::new(),
            fill: [0, 0, 0],
            font_size: 36.0,
            word_spacing: 5.0,
            line_spacing: 48.0,
            left_margin: 30.0,
            right_margin: 30.0,
            top_margin: 30.0,
            bottom_margin: 30.0,
            word_spacing_sigma: 2.0,
            line_spacing_sigma: 2.0,
            font_size_sigma: 2.0,
            perturb_x_sigma: 2.0,
            perturb_y_sigma: 2.0,
            perturb_theta_sigma: 0.05,
            end_chars: "，。".to_string(),
            start_chars: String::new(),
        }
    }
}

impl HandwritingParams {
    /// 校验参数是否完整、合法。
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.text.trim().is_empty() && self.paragraphs.is_empty() {
            return Err(ParamsError::NoText);
        }
        if self.font_path.is_empty() {
            return Err(ParamsError::NoFont);
        }
        if !std::path::Path::new(&self.font_path).is_file() {
            return Err(ParamsError::FontMissing(self.font_path.clone()));
        }
        if self.background_path.is_empty() {
            return Err(ParamsError::NoBackground);
        }
        if !std::path::Path::new(&self.background_path).is_file() {
            return Err(ParamsError::BackgroundMissing(self.background_path.clone()));
        }
        for (name, value) in [
            ("font_size", self.font_size),
            ("word_spacing", self.word_spacing),
            ("line_spacing", self.line_spacing),
            ("left_margin", self.left_margin),
            ("right_margin", self.right_margin),
            ("top_margin", self.top_margin),
            ("bottom_margin", self.bottom_margin),
            ("word_spacing_sigma", self.word_spacing_sigma),
            ("line_spacing_sigma", self.line_spacing_sigma),
            ("font_size_sigma", self.font_size_sigma),
            ("perturb_x_sigma", self.perturb_x_sigma),
            ("perturb_y_sigma", self.perturb_y_sigma),
            ("perturb_theta_sigma", self.perturb_theta_sigma),
        ] {
            if value < 0.0 {
                return Err(ParamsError::Negative { name });
            }
        }
        Ok(())
    }

    /// 行距含字高（与 Python 版 `total_line_spacing` 语义一致）。
    pub fn total_line_spacing(&self) -> f32 {
        self.line_spacing + self.font_size
    }

    /// 首行绘制基线 y 坐标（与引擎排版约定一致）。
    pub fn first_line_y(&self) -> f32 {
        self.top_margin + self.total_line_spacing() - self.font_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_match_python() {
        let p = HandwritingParams::default();
        assert_eq!(p.font_size, 36.0);
        assert_eq!(p.word_spacing, 5.0);
        assert_eq!(p.line_spacing, 48.0);
        assert_eq!(p.perturb_theta_sigma, 0.05);
        assert_eq!(p.fill, [0, 0, 0]);
        assert_eq!(p.end_chars, "，。");
    }

    #[test]
    fn validate_rejects_missing_text() {
        let p = HandwritingParams::default();
        assert!(matches!(p.validate(), Err(ParamsError::NoText)));
    }

    #[test]
    fn validate_rejects_negative_sigma() {
        let dir = tempfile::tempdir().unwrap();
        let font = dir.path().join("font.ttf");
        let bg = dir.path().join("bg.png");
        std::fs::write(&font, b"dummy").unwrap();
        std::fs::write(&bg, b"dummy").unwrap();
        let p = HandwritingParams {
            text: "你好".into(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            perturb_theta_sigma: -1.0,
            ..HandwritingParams::default()
        };
        assert!(matches!(p.validate(), Err(ParamsError::Negative { name: "perturb_theta_sigma" })));
    }

    #[test]
    fn align_parse_roundtrip() {
        assert_eq!(Align::parse("center").unwrap(), Align::Center);
        assert_eq!(Align::Center.as_str(), "center");
        assert!(Align::parse("top").is_err());
    }
}