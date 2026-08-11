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

/// 错字划掉后的重写方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MiswriteMode {
    #[default]
    Above,   // 错字正上方略偏右，小一号重写
    Rewrite, // 错字划掉后，后文正常位置重写
}

impl MiswriteMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            MiswriteMode::Above => "above",
            MiswriteMode::Rewrite => "rewrite",
        }
    }

    pub fn parse(s: &str) -> Result<MiswriteMode, String> {
        match s {
            "above" => Ok(MiswriteMode::Above),
            "rewrite" => Ok(MiswriteMode::Rewrite),
            other => Err(format!("未知重写方式：{other:?}，可选 above/rewrite")),
        }
    }
}

/// 错字涂改方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrikeoutStyle {
    #[default]
    Line,       // 单横线
    DoubleLine, // 双横线
    Slash,      // 斜线
    Cross,      // 叉号
}

impl StrikeoutStyle {
    pub fn as_str(&self) -> &'static str {
        match self {
            StrikeoutStyle::Line => "line",
            StrikeoutStyle::DoubleLine => "double_line",
            StrikeoutStyle::Slash => "slash",
            StrikeoutStyle::Cross => "cross",
        }
    }

    pub fn parse(s: &str) -> Result<StrikeoutStyle, String> {
        match s {
            "line" => Ok(StrikeoutStyle::Line),
            "double_line" => Ok(StrikeoutStyle::DoubleLine),
            "slash" => Ok(StrikeoutStyle::Slash),
            "cross" => Ok(StrikeoutStyle::Cross),
            other => Err(format!("未知涂改方式：{other:?}，可选 line/double_line/slash/cross")),
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
    #[error("颜色格式无效：{0}（应为 #RRGGBB）")]
    InvalidColor(String),
    #[error("行距（line_spacing + font_size）必须大于 0")]
    NoLineSpacing,
    #[error("错字率必须在 0~1 之间：{value}")]
    MiswriteRate { value: f32 },
}

/// 解析 `#RRGGBB` 颜色字符串（兼容不带 # 前缀的写法）。
pub fn parse_color(s: &str) -> Result<[u8; 3], ParamsError> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ParamsError::InvalidColor(s.to_string()));
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
    Ok([r, g, b])
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

    // ---- 写错字模拟 ----
    /// 每字符被判定为错字的概率（0~1，UI 中为 0~30%）。
    #[serde(default)]
    pub miswrite_rate: f32,
    /// 错字重写方式。
    #[serde(default)]
    pub miswrite_rewrite_mode: MiswriteMode,
    /// 错字涂改方式。
    #[serde(default)]
    pub miswrite_strikeout_style: StrikeoutStyle,

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
            miswrite_rate: 0.0,
            miswrite_rewrite_mode: MiswriteMode::Above,
            miswrite_strikeout_style: StrikeoutStyle::Line,
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
    if self.total_line_spacing() <= 0.0 {
        return Err(ParamsError::NoLineSpacing);
    }
    if !(0.0..=1.0).contains(&self.miswrite_rate) {
        return Err(ParamsError::MiswriteRate { value: self.miswrite_rate });
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

    #[test]
    fn miswrite_defaults_off_and_above() {
        let p = HandwritingParams::default();
        assert_eq!(p.miswrite_rate, 0.0);
        assert_eq!(p.miswrite_rewrite_mode, MiswriteMode::Above);
    }

    #[test]
    fn miswrite_mode_parse_roundtrip() {
        assert_eq!(MiswriteMode::parse("above").unwrap(), MiswriteMode::Above);
        assert_eq!(MiswriteMode::Above.as_str(), "above");
        assert_eq!(MiswriteMode::parse("rewrite").unwrap(), MiswriteMode::Rewrite);
        assert_eq!(MiswriteMode::Rewrite.as_str(), "rewrite");
        assert!(MiswriteMode::parse("inline").is_err());
    }

    #[test]
    fn validate_rejects_out_of_range_miswrite_rate() {
        let dir = tempfile::tempdir().unwrap();
        let font = dir.path().join("font.ttf");
        let bg = dir.path().join("bg.png");
        std::fs::write(&font, b"dummy").unwrap();
        std::fs::write(&bg, b"dummy").unwrap();
        let base = HandwritingParams {
            text: "你好".into(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            ..HandwritingParams::default()
        };
        assert!(matches!(
            base.clone().validate(),
            Ok(())
        ));
        let p = HandwritingParams { miswrite_rate: -0.01, ..base.clone() };
        assert!(matches!(p.validate(), Err(ParamsError::MiswriteRate { .. })));
        let p = HandwritingParams { miswrite_rate: 1.01, ..base };
        assert!(matches!(p.validate(), Err(ParamsError::MiswriteRate { .. })));
    }

    #[test]
    fn test_strikeout_style_parsing() {
        assert_eq!(StrikeoutStyle::parse("line").unwrap(), StrikeoutStyle::Line);
        assert_eq!(StrikeoutStyle::parse("double_line").unwrap(), StrikeoutStyle::DoubleLine);
        assert_eq!(StrikeoutStyle::parse("slash").unwrap(), StrikeoutStyle::Slash);
        assert_eq!(StrikeoutStyle::parse("cross").unwrap(), StrikeoutStyle::Cross);
        assert!(StrikeoutStyle::parse("invalid").is_err());
    }
}
