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

/// 页面上一个框选文字区域（实验特性：手写/打印混排）。
///
/// 对应 Python 版 `models.TextRegion`。坐标为背景图**原始像素**坐标
/// （与预览降采样无关，GUI/引擎负责换算）。文字在矩形内自行换行，
/// 仅在指定所在页渲染，超出框选范围的内容自然截断。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRegion {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 区域内文字（支持多行）。
    #[serde(default)]
    pub text: String,
    /// 区域独立字体；空 = 使用主字体。
    #[serde(default)]
    pub font_path: String,
    /// true = 打印体（零扰动、规整排版）。
    #[serde(default)]
    pub printed: bool,
    /// 区域字号；0 = 跟随主设置。
    #[serde(default)]
    pub font_size: i32,
    /// 所在页（1 基）；1 = 第一页。
    #[serde(default)]
    pub page: i32,
    /// 对齐方式：0 左 / 1 居中 / 2 右（应用于区域整体文本）。
    #[serde(default)]
    pub align: i32,
    /// 首行缩进（字符数 em；0 = 无）。渲染时 × 区域字号换算像素。
    #[serde(default)]
    pub indent_em: f32,
    /// 区域内多段落排版信息（各段独立设置对齐与缩进；非空时优先于单文本）。
    #[serde(default)]
    pub paragraphs: Vec<Paragraph>,

    // ---- 逐区域排版/扰动覆盖项（None = 跟随主设置）----
    /// 字水平间距。
    #[serde(default)]
    pub word_spacing: Option<f32>,
    /// 字竖直间距。
    #[serde(default)]
    pub line_spacing: Option<f32>,
    /// 字号随机扰动 σ。
    #[serde(default)]
    pub font_size_sigma: Option<f32>,
    /// 字水平间距扰动 σ。
    #[serde(default)]
    pub word_spacing_sigma: Option<f32>,
    /// 字竖直间距扰动 σ。
    #[serde(default)]
    pub line_spacing_sigma: Option<f32>,
    /// 水平笔画位移 σ。
    #[serde(default)]
    pub perturb_x_sigma: Option<f32>,
    /// 竖直笔画位移 σ。
    #[serde(default)]
    pub perturb_y_sigma: Option<f32>,
    /// 笔画旋转 σ。
    #[serde(default)]
    pub perturb_theta_sigma: Option<f32>,
    /// 错字率（0..=1）；None = 跟随主设置。
    #[serde(default)]
    pub miswrite_rate: Option<f32>,
    /// 错字涂改方式覆盖。
    #[serde(default)]
    pub miswrite_strikeout_style: Option<StrikeoutStyle>,
    /// 文字颜色覆盖（#RRGGBB 解析后的 RGB）。
    #[serde(default)]
    pub fill: Option<[u8; 3]>,

    // ---- 区域内边距（像素；None / 0 = 紧贴框边界，默认 0）----
    /// 区域上边距。
    #[serde(default)]
    pub margin_top: Option<f32>,
    /// 区域下边距。
    #[serde(default)]
    pub margin_bottom: Option<f32>,
    /// 区域左边距。
    #[serde(default)]
    pub margin_left: Option<f32>,
    /// 区域右边距。
    #[serde(default)]
    pub margin_right: Option<f32>,
}

impl Default for TextRegion {
    fn default() -> Self {
        Self {
            x: 0, y: 0, w: 0, h: 0,
            text: String::new(),
            font_path: String::new(),
            printed: false,
            font_size: 0,
            page: 1,
            align: 0,
            indent_em: 0.0,
            paragraphs: Vec::new(),
            word_spacing: None,
            line_spacing: None,
            font_size_sigma: None,
            word_spacing_sigma: None,
            line_spacing_sigma: None,
            perturb_x_sigma: None,
            perturb_y_sigma: None,
            perturb_theta_sigma: None,
            miswrite_rate: None,
            miswrite_strikeout_style: None,
            fill: None,
            margin_top: None,
            margin_bottom: None,
            margin_left: None,
            margin_right: None,
        }
    }
}

impl TextRegion {
    /// 是否设置了任意一项逐区域覆盖（列表摘要标记用）。
    pub fn has_overrides(&self) -> bool {
        self.word_spacing.is_some()
            || self.line_spacing.is_some()
            || self.font_size_sigma.is_some()
            || self.word_spacing_sigma.is_some()
            || self.line_spacing_sigma.is_some()
            || self.perturb_x_sigma.is_some()
            || self.perturb_y_sigma.is_some()
            || self.perturb_theta_sigma.is_some()
            || self.miswrite_rate.is_some()
            || self.miswrite_strikeout_style.is_some()
            || self.fill.is_some()
            || self.margin_top.is_some_and(|v| v > 0.0)
            || self.margin_bottom.is_some_and(|v| v > 0.0)
            || self.margin_left.is_some_and(|v| v > 0.0)
            || self.margin_right.is_some_and(|v| v > 0.0)
    }
    /// 区域列表里的一行摘要（对齐 Python 版 `TextRegion.label`）。
    pub fn label(&self, index: usize) -> String {
        let style = if self.printed { "打印" } else { "手写" };
        let page = if self.page > 1 { format!(" 第{}页", self.page) } else { String::new() };
        format!(
            "{}. {}{} {}字 ({},{} {}×{})",
            index,
            style,
            page,
            self.text.chars().count(),
            self.x,
            self.y,
            self.w,
            self.h
        )
    }
}

/// 单个文本片段（TextRun）的独立样式与角色配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TextRunStyle {
    /// 角色 ID（0 为默认角色）。
    #[serde(default)]
    pub role_id: u32,
    /// 高亮颜色名称（如 "yellow", "cyan", "pink", None = 无高亮）。
    #[serde(default)]
    pub highlight: Option<String>,
    /// 字体文件路径覆盖（None = 跟随角色或主配置）。
    #[serde(default)]
    pub font_path: Option<String>,
    /// 字号覆盖（None = 跟随角色或主配置）。
    #[serde(default)]
    pub font_size: Option<f32>,
    /// 颜色覆盖（None = 跟随角色或主配置）。
    #[serde(default)]
    pub fill: Option<[u8; 3]>,
    /// 是否为印刷体（默认 false）。
    #[serde(default)]
    pub printed: bool,
}

impl TextRunStyle {
    pub fn with_role(role_id: u32) -> Self {
        Self {
            role_id,
            ..Default::default()
        }
    }

    pub fn with_highlight(role_id: u32, highlight: impl Into<String>) -> Self {
        Self {
            role_id,
            highlight: Some(highlight.into()),
            ..Default::default()
        }
    }
}

/// 富文本段落内的一个文本片段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TextRun {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub style: TextRunStyle,
}

impl TextRun {
    pub fn new(text: impl Into<String>, style: TextRunStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextRunStyle::default(),
        }
    }
}

/// 单个段落的排版信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub align: Align,
    /// 首行缩进（像素）。
    #[serde(default)]
    pub first_line_indent: f32,
    /// 富文本分段（多样式 / 多角色混排）；非空时优先于 text。
    #[serde(default)]
    pub runs: Vec<TextRun>,
}

impl Default for Paragraph {
    fn default() -> Self {
        Self {
            text: String::new(),
            align: Align::Left,
            first_line_indent: 0.0,
            runs: Vec::new(),
        }
    }
}

impl Paragraph {
    /// 获取实际生效的 TextRun 列表。
    /// 若 `runs` 非空则返回 `runs` 的副本，否则回退为基于 `text` 和默认样式的单 run。
    pub fn effective_runs(&self) -> Vec<TextRun> {
        if !self.runs.is_empty() {
            self.runs.clone()
        } else {
            vec![TextRun {
                text: self.text.clone(),
                style: TextRunStyle::default(),
            }]
        }
    }
}

/// 手写角色（角色预设），用于多角色/多笔迹混排。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandwritingRole {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub name: String,
    /// 绑定的高亮颜色（用于 docx 导入时自动关联对应角色）。
    #[serde(default)]
    pub highlight: Option<String>,
    #[serde(default)]
    pub font_path: String,
    #[serde(default)]
    pub printed: bool,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub fill: Option<[u8; 3]>,
    #[serde(default)]
    pub word_spacing: Option<f32>,
    #[serde(default)]
    pub line_spacing: Option<f32>,
    #[serde(default)]
    pub font_size_sigma: Option<f32>,
    #[serde(default)]
    pub word_spacing_sigma: Option<f32>,
    #[serde(default)]
    pub line_spacing_sigma: Option<f32>,
    #[serde(default)]
    pub perturb_x_sigma: Option<f32>,
    #[serde(default)]
    pub perturb_y_sigma: Option<f32>,
    #[serde(default)]
    pub perturb_theta_sigma: Option<f32>,
    #[serde(default)]
    pub miswrite_rate: Option<f32>,
    #[serde(default)]
    pub miswrite_strikeout_style: Option<StrikeoutStyle>,
}

impl HandwritingRole {
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            ..Default::default()
        }
    }
}

impl Default for HandwritingRole {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            highlight: None,
            font_path: String::new(),
            printed: false,
            font_size: None,
            fill: None,
            word_spacing: None,
            line_spacing: None,
            font_size_sigma: None,
            word_spacing_sigma: None,
            line_spacing_sigma: None,
            perturb_x_sigma: None,
            perturb_y_sigma: None,
            perturb_theta_sigma: None,
            miswrite_rate: None,
            miswrite_strikeout_style: None,
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
    #[error("第 {page} 页背景文件不存在：{path}")]
    BackgroundPageMissing { page: usize, path: String },
    #[error("文字区域 {index} 的宽高必须为正")]
    RegionSize { index: usize },
    #[error("文字区域 {index} 的坐标不能为负")]
    RegionPosition { index: usize },
    #[error("文字区域 {index} 的页码必须从 1 开始")]
    RegionPage { index: usize },
    #[error("文字区域 {index} 的字体文件不存在：{path}")]
    RegionFontMissing { index: usize, path: String },
    #[error("文字区域 {index} 的字号不能为负")]
    RegionFontSize { index: usize },
    #[error("角色 {index} 的字体文件不存在：{path}")]
    RoleFontMissing { index: usize, path: String },
    #[error("角色 {index} 的错字率必须在 0~1 之间：{value}")]
    RoleMiswriteRate { index: usize, value: f32 },
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
    /// 多页文档背景（导入的 PDF/DOCX 打印预览，每页一张 PNG 路径）；
    /// 为空时所有页使用 `background_path` 单张背景。
    #[serde(default)]
    pub background_pages: Vec<String>,
    pub text: String,
    /// 非空时启用段落渲染（分段对齐/缩进）。
    pub paragraphs: Vec<Paragraph>,
    /// 非空时在框选矩形内渲染区域文字（可与主文字并存）。
    #[serde(default)]
    pub regions: Vec<TextRegion>,
    /// 多角色定义（用于不同段落或 TextRun 绑定不同角色）。
    #[serde(default)]
    pub roles: Vec<HandwritingRole>,

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
            background_pages: Vec::new(),
            text: String::new(),
            paragraphs: Vec::new(),
            regions: Vec::new(),
            roles: Vec::new(),
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
    /// 校验参数是否完整、合法（要求存在文字/区域，供导出等场景）。
    pub fn validate(&self) -> Result<(), ParamsError> {
        self.validate_with(true)
    }

    /// 校验参数；`require_text=false` 时允许「纯背景预览」：
    /// 没有文字/区域时不要求字体（一个字都不画），但仍要求背景文件有效。
    /// 对齐 Python 版 `HandwritingParams.validate(require_text=...)`。
    pub fn validate_with(&self, require_text: bool) -> Result<(), ParamsError> {
        let has_region_text = self.regions.iter().any(|r| !r.text.trim().is_empty());
        let has_para_text = self.paragraphs.iter().any(|p| {
            !p.text.trim().is_empty() || p.runs.iter().any(|r| !r.text.trim().is_empty())
        });
        let has_content =
            !self.text.trim().is_empty() || has_para_text || has_region_text;
        if require_text && !has_content {
            return Err(ParamsError::NoText);
        }
        if has_content && self.font_path.is_empty() {
            return Err(ParamsError::NoFont);
        }
        if has_content && !std::path::Path::new(&self.font_path).is_file() {
            return Err(ParamsError::FontMissing(self.font_path.clone()));
        }
        if self.background_path.is_empty() && self.background_pages.is_empty() {
            return Err(ParamsError::NoBackground);
        }
        if !self.background_path.is_empty()
            && !std::path::Path::new(&self.background_path).is_file()
        {
            return Err(ParamsError::BackgroundMissing(self.background_path.clone()));
        }
        for (i, page_bg) in self.background_pages.iter().enumerate() {
            if !std::path::Path::new(page_bg).is_file() {
                return Err(ParamsError::BackgroundPageMissing {
                    page: i + 1,
                    path: page_bg.clone(),
                });
            }
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
        for (i, role) in self.roles.iter().enumerate() {
            let index = i + 1;
            if !role.font_path.is_empty() && !std::path::Path::new(&role.font_path).is_file() {
                return Err(ParamsError::RoleFontMissing {
                    index,
                    path: role.font_path.clone(),
                });
            }
            if let Some(rate) = role.miswrite_rate {
                if !(0.0..=1.0).contains(&rate) {
                    return Err(ParamsError::RoleMiswriteRate { index, value: rate });
                }
            }
        }
        for (i, region) in self.regions.iter().enumerate() {
            let index = i + 1;
            if region.w <= 0 || region.h <= 0 {
                return Err(ParamsError::RegionSize { index });
            }
            if region.x < 0 || region.y < 0 {
                return Err(ParamsError::RegionPosition { index });
            }
            if region.page < 1 {
                return Err(ParamsError::RegionPage { index });
            }
            if !region.font_path.is_empty() && !std::path::Path::new(&region.font_path).is_file() {
                return Err(ParamsError::RegionFontMissing {
                    index,
                    path: region.font_path.clone(),
                });
            }
            if region.font_size < 0 {
                return Err(ParamsError::RegionFontSize { index });
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
        assert!(p.roles.is_empty());
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

    #[test]
    fn test_text_run_and_style_serde_roundtrip() {
        let style = TextRunStyle {
            role_id: 2,
            highlight: Some("yellow".into()),
            font_path: Some("custom/font.ttf".into()),
            font_size: Some(28.0),
            fill: Some([255, 0, 0]),
            printed: true,
        };
        let run = TextRun::new("测试片段", style.clone());

        let json = serde_json::to_string(&run).unwrap();
        let deserialized: TextRun = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, run);
        assert_eq!(deserialized.text, "测试片段");
        assert_eq!(deserialized.style.role_id, 2);
        assert_eq!(deserialized.style.highlight, Some("yellow".into()));
        assert_eq!(deserialized.style.font_path, Some("custom/font.ttf".into()));
        assert_eq!(deserialized.style.font_size, Some(28.0));
        assert_eq!(deserialized.style.fill, Some([255, 0, 0]));
        assert!(deserialized.style.printed);

        // Test default deserialization from minimal json
        let minimal_json = r#"{"text":"简单文本"}"#;
        let minimal_run: TextRun = serde_json::from_str(minimal_json).unwrap();
        assert_eq!(minimal_run.text, "简单文本");
        assert_eq!(minimal_run.style, TextRunStyle::default());
        assert_eq!(minimal_run.style.role_id, 0);
        assert_eq!(minimal_run.style.highlight, None);
        assert!(!minimal_run.style.printed);
    }

    #[test]
    fn test_paragraph_effective_runs() {
        // 1. Legacy paragraph with only `text` and empty `runs`
        let legacy_para = Paragraph {
            text: "传统单段落内容".into(),
            align: Align::Left,
            first_line_indent: 20.0,
            runs: Vec::new(),
        };
        let effective = legacy_para.effective_runs();
        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].text, "传统单段落内容");
        assert_eq!(effective[0].style, TextRunStyle::default());

        // 2. Paragraph with rich runs
        let rich_para = Paragraph {
            text: String::new(),
            align: Align::Center,
            first_line_indent: 0.0,
            runs: vec![
                TextRun::new("角色A手写", TextRunStyle { role_id: 1, ..Default::default() }),
                TextRun::new("印刷体提示", TextRunStyle { printed: true, ..Default::default() }),
            ],
        };
        let effective_rich = rich_para.effective_runs();
        assert_eq!(effective_rich.len(), 2);
        assert_eq!(effective_rich[0].text, "角色A手写");
        assert_eq!(effective_rich[0].style.role_id, 1);
        assert_eq!(effective_rich[1].text, "印刷体提示");
        assert!(effective_rich[1].style.printed);

        // 3. Deserialization of legacy JSON without `runs` field
        let legacy_json = r#"{"text":"反序列化传统段落","align":"Center","first_line_indent":10.0}"#;
        let parsed_para: Paragraph = serde_json::from_str(legacy_json).unwrap();
        assert!(parsed_para.runs.is_empty());
        let runs = parsed_para.effective_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "反序列化传统段落");
    }

    #[test]
    fn test_handwriting_role_serde_and_defaults() {
        let role_default = HandwritingRole::default();
        assert_eq!(role_default.id, 0);
        assert_eq!(role_default.name, "");
        assert_eq!(role_default.highlight, None);
        assert_eq!(role_default.font_path, "");
        assert!(!role_default.printed);
        assert_eq!(role_default.font_size, None);
        assert_eq!(role_default.fill, None);
        assert_eq!(role_default.word_spacing, None);
        assert_eq!(role_default.line_spacing, None);
        assert_eq!(role_default.miswrite_rate, None);
        assert_eq!(role_default.miswrite_strikeout_style, None);

        let custom_role = HandwritingRole {
            id: 1,
            name: "批注老师".into(),
            highlight: Some("yellow".into()),
            font_path: "fonts/teacher.ttf".into(),
            printed: false,
            font_size: Some(30.0),
            fill: Some([200, 0, 0]),
            word_spacing: Some(6.0),
            line_spacing: Some(50.0),
            font_size_sigma: Some(1.5),
            word_spacing_sigma: Some(1.0),
            line_spacing_sigma: Some(1.0),
            perturb_x_sigma: Some(1.0),
            perturb_y_sigma: Some(1.0),
            perturb_theta_sigma: Some(0.03),
            miswrite_rate: Some(0.05),
            miswrite_strikeout_style: Some(StrikeoutStyle::DoubleLine),
        };

        let json = serde_json::to_string(&custom_role).unwrap();
        let deserialized: HandwritingRole = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, custom_role);

        // Deserializing partial JSON
        let partial_json = r#"{"id":3,"name":"学生A"}"#;
        let partial_role: HandwritingRole = serde_json::from_str(partial_json).unwrap();
        assert_eq!(partial_role.id, 3);
        assert_eq!(partial_role.name, "学生A");
        assert_eq!(partial_role.font_path, "");
        assert_eq!(partial_role.font_size, None);
    }

    #[test]
    fn test_handwriting_params_roles_validation() {
        let dir = tempfile::tempdir().unwrap();
        let font = dir.path().join("font.ttf");
        let bg = dir.path().join("bg.png");
        let role_font = dir.path().join("role_font.ttf");
        std::fs::write(&font, b"dummy").unwrap();
        std::fs::write(&bg, b"dummy").unwrap();
        std::fs::write(&role_font, b"dummy").unwrap();

        let mut base = HandwritingParams {
            text: "多角色测试".into(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            ..HandwritingParams::default()
        };

        // 1. Valid role with existing font and valid miswrite rate
        base.roles = vec![HandwritingRole {
            id: 1,
            name: "角色1".into(),
            font_path: role_font.to_string_lossy().into_owned(),
            miswrite_rate: Some(0.1),
            ..Default::default()
        }];
        assert!(base.validate().is_ok());

        // 2. Role with non-existent font file
        let mut invalid_font_params = base.clone();
        invalid_font_params.roles[0].font_path = dir.path().join("nonexistent.ttf").to_string_lossy().into_owned();
        assert!(matches!(
            invalid_font_params.validate(),
            Err(ParamsError::RoleFontMissing { index: 1, .. })
        ));

        // 3. Role with invalid miswrite rate
        let mut invalid_rate_params = base.clone();
        invalid_rate_params.roles[0].miswrite_rate = Some(1.5);
        assert!(matches!(
            invalid_rate_params.validate(),
            Err(ParamsError::RoleMiswriteRate { index: 1, .. })
        ));

        let mut negative_rate_params = base.clone();
        negative_rate_params.roles[0].miswrite_rate = Some(-0.01);
        assert!(matches!(
            negative_rate_params.validate(),
            Err(ParamsError::RoleMiswriteRate { index: 1, .. })
        ));

        // 4. Content check with paragraph runs instead of base text
        let run_content_params = HandwritingParams {
            text: String::new(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            paragraphs: vec![Paragraph {
                text: String::new(),
                align: Align::Left,
                first_line_indent: 0.0,
                runs: vec![TextRun::new("片段内容", TextRunStyle::default())],
            }],
            ..HandwritingParams::default()
        };
        assert!(run_content_params.validate().is_ok());
    }
}
