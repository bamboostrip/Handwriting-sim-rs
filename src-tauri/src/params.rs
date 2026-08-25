//! 前端 ↔ 引擎 参数转换层。
//!
//! `UiParams` 是 Vue 前端表单状态的 1:1 镜像（camelCase JSON），
//! 职责只有两件事：
//! - `to_handwriting_params()`：换算枚举 / 解析颜色，交给 core 校验渲染
//! - `from_handwriting_params()`：预设载入时回填前端全部字段
//!
//! 原 Slint 版 main.rs 中 collect_params 的段落/文本分支逻辑由前端实现
//! （编辑器状态归前端所有）：单段且无格式 → 只填 `text`；否则填 `paragraphs`。

use handwrite_sim::core::models::{
    self, Align, HandwritingParams, MiswriteMode, Paragraph, StrikeoutStyle, TextRegion,
};
use serde::{Deserialize, Serialize};

/// 清理外来文本的特殊字符（与原 Slint 版 to_ui_spaces 一致）：
/// WORD JOINER 移除，NBSP/FFA0 还原普通空格。
fn clean_text(s: &str) -> String {
    s.replace('\u{2060}', "").replace(['\u{00a0}', '\u{ffa0}'], " ")
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UiParagraph {
    pub text: String,
    /// 0 左对齐 / 1 居中 / 2 右对齐
    pub align: i32,
    /// 首行缩进（字符数 em；0 = 不缩进），渲染时 × 字号换算像素
    pub indent_em: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UiRegion {
    /// 背景原始像素坐标
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 区域文字（空文字区域会被前端拒绝提交）
    pub text: String,
    /// 打印字体路径（空 = 使用主字体）
    pub font_path: String,
    /// true = 打印体（无笔画扰动、排版规整）
    pub printed: bool,
    /// 区域字号（0 = 跟随主设置）
    pub font_size: i32,
    /// 所在页（1 基）
    pub page: i32,
    /// 对齐方式：0 左 / 1 中 / 2 右
    pub align: i32,
    /// 首行缩进（字符数 em）
    pub indent_em: f32,
    /// 区域内各段落排版信息（各段独立设置对齐与缩进）
    pub paragraphs: Vec<UiParagraph>,

    // ---- 逐区域覆盖项（None = 跟随主设置）----
    pub word_spacing: Option<f32>,
    pub line_spacing: Option<f32>,
    pub font_size_sigma: Option<f32>,
    pub word_spacing_sigma: Option<f32>,
    pub line_spacing_sigma: Option<f32>,
    pub perturb_x_sigma: Option<f32>,
    pub perturb_y_sigma: Option<f32>,
    pub perturb_theta_sigma: Option<f32>,
    /// 错字率 0~1
    pub miswrite_rate: Option<f32>,
    /// 涂改方式索引（0 单横线 / 1 双横线 / 2 斜线 / 3 叉号）；None = 跟随主设置
    pub miswrite_strikeout_style_index: Option<i32>,
    /// 文字颜色 #RRGGBB；None = 跟随主设置
    pub fill: Option<String>,
    /// 区域上边距
    pub margin_top: Option<f32>,
    /// 区域下边距
    pub margin_bottom: Option<f32>,
    /// 区域左边距
    pub margin_left: Option<f32>,
    /// 区域右边距
    pub margin_right: Option<f32>,
}

fn strikeout_style_opt(idx: i32) -> Option<StrikeoutStyle> {
    (idx >= 0).then(|| strikeout_style_of(idx))
}

impl From<&TextRegion> for UiRegion {
    fn from(r: &TextRegion) -> Self {
        Self {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            text: r.text.clone(),
            font_path: r.font_path.clone(),
            printed: r.printed,
            font_size: r.font_size,
            page: r.page,
            align: r.align,
            indent_em: r.indent_em,
            paragraphs: r
                .paragraphs
                .iter()
                .map(|p| UiParagraph {
                    text: p.text.clone(),
                    align: match p.align {
                        Align::Center => 1,
                        Align::Right => 2,
                        _ => 0,
                    },
                    indent_em: if r.font_size > 0 {
                        p.first_line_indent / r.font_size as f32
                    } else {
                        0.0
                    },
                })
                .collect(),
            word_spacing: r.word_spacing,
            line_spacing: r.line_spacing,
            font_size_sigma: r.font_size_sigma,
            word_spacing_sigma: r.word_spacing_sigma,
            line_spacing_sigma: r.line_spacing_sigma,
            perturb_x_sigma: r.perturb_x_sigma,
            perturb_y_sigma: r.perturb_y_sigma,
            perturb_theta_sigma: r.perturb_theta_sigma,
            miswrite_rate: r.miswrite_rate,
            miswrite_strikeout_style_index: r
                .miswrite_strikeout_style
                .map(|s| match s {
                    StrikeoutStyle::Line => 0,
                    StrikeoutStyle::DoubleLine => 1,
                    StrikeoutStyle::Slash => 2,
                    StrikeoutStyle::Cross => 3,
                }),
            fill: r.fill.map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])),
            margin_top: r.margin_top,
            margin_bottom: r.margin_bottom,
            margin_left: r.margin_left,
            margin_right: r.margin_right,
        }
    }
}

/// 前端表单状态镜像（camelCase JSON）。字段语义见 models.rs。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct UiParams {
    pub font_path: String,
    pub background_path: String,
    /// 多页文档底图（PDF/DOCX 导入的逐页 PNG 绝对路径）；背景被手动改走后前端清空
    pub background_pages: Vec<String>,
    pub font_size: f32,
    pub word_spacing: f32,
    pub line_spacing: f32,
    pub word_spacing_sigma: f32,
    pub line_spacing_sigma: f32,
    pub font_size_sigma: f32,
    pub perturb_x_sigma: f32,
    pub perturb_y_sigma: f32,
    pub perturb_theta_sigma: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    /// #RRGGBB 文字颜色
    pub fill: String,
    /// 错字率 0~1（前端以百分比显示）
    pub miswrite_rate: f32,
    /// 0 右上方重写 / 1 后文重写
    pub miswrite_mode_index: i32,
    /// 0 单横线 / 1 双横线 / 2 斜线 / 3 叉号
    pub miswrite_strikeout_style_index: i32,
    /// 纯文本模式内容（paragraphs 为空时的回退路径）
    pub text: String,
    pub paragraphs: Vec<UiParagraph>,
    pub regions: Vec<UiRegion>,
    pub end_chars: String,
    pub start_chars: String,
    /// 边界提示叠加（仅预览，不进导出/PDF）
    pub bounds_visible: bool,
    pub bounds_color: String,
}

impl Default for UiParams {
    fn default() -> Self {
        let d = HandwritingParams::default();
        Self {
            font_path: String::new(),
            background_path: String::new(),
            background_pages: Vec::new(),
            font_size: d.font_size,
            word_spacing: d.word_spacing,
            line_spacing: d.line_spacing,
            word_spacing_sigma: d.word_spacing_sigma,
            line_spacing_sigma: d.line_spacing_sigma,
            font_size_sigma: d.font_size_sigma,
            perturb_x_sigma: d.perturb_x_sigma,
            perturb_y_sigma: d.perturb_y_sigma,
            perturb_theta_sigma: d.perturb_theta_sigma,
            margin_top: d.top_margin,
            margin_bottom: d.bottom_margin,
            margin_left: d.left_margin,
            margin_right: d.right_margin,
            fill: "#000000".into(),
            miswrite_rate: d.miswrite_rate,
            miswrite_mode_index: 0,
            miswrite_strikeout_style_index: 0,
            text: String::new(),
            paragraphs: Vec::new(),
            regions: Vec::new(),
            end_chars: d.end_chars,
            start_chars: d.start_chars,
            bounds_visible: false,
            bounds_color: "#4ca6a6".into(),
        }
    }
}

fn align_of(idx: i32) -> Align {
    match idx {
        1 => Align::Center,
        2 => Align::Right,
        _ => Align::Left,
    }
}

fn miswrite_mode_of(idx: i32) -> MiswriteMode {
    match idx {
        1 => MiswriteMode::Rewrite,
        _ => MiswriteMode::Above,
    }
}

fn strikeout_style_of(idx: i32) -> StrikeoutStyle {
    match idx {
        1 => StrikeoutStyle::DoubleLine,
        2 => StrikeoutStyle::Slash,
        3 => StrikeoutStyle::Cross,
        _ => StrikeoutStyle::Line,
    }
}

impl UiParams {
    /// 转换为引擎参数。只做映射，不做校验（校验由调用方 validate_with 完成），
    /// 颜色解析失败返回可读错误。
    pub fn to_handwriting_params(&self) -> Result<HandwritingParams, String> {
        let mut p = HandwritingParams::default();
        p.font_path = self.font_path.trim().to_string();
        p.background_path = self.background_path.trim().to_string();
        p.background_pages = self.background_pages.clone();
        p.fill = models::parse_color(self.fill.trim()).map_err(|e| format!("文字颜色：{e}"))?;
        p.text = clean_text(&self.text);
        // 段落路径：非空即走段落（单段无格式的回退由前端负责——它只填 text）
        p.paragraphs = self
            .paragraphs
            .iter()
            .filter(|row| !clean_text(&row.text).trim().is_empty())
            .map(|row| Paragraph {
                text: clean_text(&row.text),
                align: align_of(row.align),
                first_line_indent: row.indent_em * self.font_size,
            })
            .collect();
        if !p.paragraphs.is_empty() {
            p.text.clear(); // 段落模式优先
        }
        p.regions = self
            .regions
            .iter()
            .map(|r| -> Result<TextRegion, String> {
                let fill = match &r.fill {
                    Some(hex) if !hex.trim().is_empty() => Some(
                        models::parse_color(hex.trim())
                            .map_err(|e| format!("区域文字颜色：{e}"))?,
                    ),
                    _ => None,
                };
                let region_fs = if r.font_size > 0 { r.font_size as f32 } else { self.font_size };
                let region_paras: Vec<Paragraph> = r
                    .paragraphs
                    .iter()
                    .filter(|row| !clean_text(&row.text).trim().is_empty())
                    .map(|row| Paragraph {
                        text: clean_text(&row.text),
                        align: align_of(row.align),
                        first_line_indent: row.indent_em * region_fs,
                    })
                    .collect();
                Ok(TextRegion {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: r.h,
                    text: r.text.trim().to_string(),
                    font_path: r.font_path.trim().to_string(),
                    printed: r.printed,
                    font_size: r.font_size,
                    page: r.page.max(1),
                    align: r.align,
                    indent_em: r.indent_em,
                    paragraphs: region_paras,
                    word_spacing: r.word_spacing,
                    line_spacing: r.line_spacing,
                    font_size_sigma: r.font_size_sigma,
                    word_spacing_sigma: r.word_spacing_sigma,
                    line_spacing_sigma: r.line_spacing_sigma,
                    perturb_x_sigma: r.perturb_x_sigma,
                    perturb_y_sigma: r.perturb_y_sigma,
                    perturb_theta_sigma: r.perturb_theta_sigma,
                    miswrite_rate: r.miswrite_rate,
                    miswrite_strikeout_style: r
                        .miswrite_strikeout_style_index
                        .and_then(strikeout_style_opt),
                    fill,
                    margin_top: r.margin_top,
                    margin_bottom: r.margin_bottom,
                    margin_left: r.margin_left,
                    margin_right: r.margin_right,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        p.font_size = self.font_size;
        p.word_spacing = self.word_spacing;
        p.line_spacing = self.line_spacing;
        p.left_margin = self.margin_left;
        p.right_margin = self.margin_right;
        p.top_margin = self.margin_top;
        p.bottom_margin = self.margin_bottom;
        p.word_spacing_sigma = self.word_spacing_sigma;
        p.line_spacing_sigma = self.line_spacing_sigma;
        p.font_size_sigma = self.font_size_sigma;
        p.perturb_x_sigma = self.perturb_x_sigma;
        p.perturb_y_sigma = self.perturb_y_sigma;
        p.perturb_theta_sigma = self.perturb_theta_sigma;
        p.miswrite_rate = self.miswrite_rate;
        p.miswrite_rewrite_mode = miswrite_mode_of(self.miswrite_mode_index);
        p.miswrite_strikeout_style = strikeout_style_of(self.miswrite_strikeout_style_index);
        p.end_chars = self.end_chars.clone();
        p.start_chars = self.start_chars.clone();
        Ok(p)
    }

    /// 预设载入：从引擎参数回填全部前端字段。
    /// 预设不含文本/区域，这两项保持默认空值。
    pub fn from_handwriting_params(p: &HandwritingParams) -> Self {
        Self {
            font_path: p.font_path.clone(),
            background_path: p.background_path.clone(),
            background_pages: Vec::new(),
            font_size: p.font_size,
            word_spacing: p.word_spacing,
            line_spacing: p.line_spacing,
            word_spacing_sigma: p.word_spacing_sigma,
            line_spacing_sigma: p.line_spacing_sigma,
            font_size_sigma: p.font_size_sigma,
            perturb_x_sigma: p.perturb_x_sigma,
            perturb_y_sigma: p.perturb_y_sigma,
            perturb_theta_sigma: p.perturb_theta_sigma,
            margin_top: p.top_margin,
            margin_bottom: p.bottom_margin,
            margin_left: p.left_margin,
            margin_right: p.right_margin,
            fill: format!("#{:02x}{:02x}{:02x}", p.fill[0], p.fill[1], p.fill[2]),
            miswrite_rate: p.miswrite_rate,
            miswrite_mode_index: match p.miswrite_rewrite_mode {
                MiswriteMode::Above => 0,
                MiswriteMode::Rewrite => 1,
            },
            miswrite_strikeout_style_index: match p.miswrite_strikeout_style {
                StrikeoutStyle::Line => 0,
                StrikeoutStyle::DoubleLine => 1,
                StrikeoutStyle::Slash => 2,
                StrikeoutStyle::Cross => 3,
            },
            text: String::new(),
            paragraphs: Vec::new(),
            regions: Vec::new(),
            end_chars: p.end_chars.clone(),
            start_chars: p.start_chars.clone(),
            bounds_visible: false,
            bounds_color: "#4ca6a6".into(),
        }
    }
}
