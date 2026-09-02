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
    self, Align, HandwritingParams, HandwritingRole, MiswriteMode, Paragraph, StrikeoutStyle,
    TextRegion, TextRun, TextRunStyle,
};
use serde::{Deserialize, Serialize};

/// 清理外来文本的特殊字符（与原 Slint 版 to_ui_spaces 一致）：
/// WORD JOINER 移除，NBSP/FFA0 还原普通空格。
fn clean_text(s: &str) -> String {
    s.replace('\u{2060}', "")
        .replace(['\u{00a0}', '\u{ffa0}'], " ")
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiTextRunStyle {
    pub role_id: u32,
    pub font_path: Option<String>,
    pub font_size: Option<f32>,
    pub fill: Option<String>, // #RRGGBB
    pub printed: bool,
}

impl From<&TextRunStyle> for UiTextRunStyle {
    fn from(s: &TextRunStyle) -> Self {
        Self {
            role_id: s.role_id,
            font_path: s.font_path.clone(),
            font_size: s.font_size,
            fill: s
                .fill
                .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])),
            printed: s.printed,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiTextRun {
    pub text: String,
    pub style: UiTextRunStyle,
}

impl From<&TextRun> for UiTextRun {
    fn from(r: &TextRun) -> Self {
        Self {
            text: r.text.clone(),
            style: UiTextRunStyle::from(&r.style),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiParagraph {
    pub text: String,
    /// 0 左对齐 / 1 居中 / 2 右对齐
    pub align: i32,
    /// 首行缩进（字符数 em；0 = 不缩进），渲染时 × 字号换算像素
    pub indent_em: f32,
    #[serde(default)]
    pub runs: Vec<UiTextRun>,
}

impl UiParagraph {
    pub fn from_paragraph_with_font_size(p: &Paragraph, font_size: f32) -> Self {
        let fs = font_size.max(1.0);
        Self {
            text: p.text.clone(),
            align: match p.align {
                Align::Center => 1,
                Align::Right => 2,
                _ => 0,
            },
            indent_em: p.first_line_indent / fs,
            runs: p.runs.iter().map(UiTextRun::from).collect(),
        }
    }
}

impl From<&Paragraph> for UiParagraph {
    fn from(p: &Paragraph) -> Self {
        Self {
            text: p.text.clone(),
            align: match p.align {
                Align::Center => 1,
                Align::Right => 2,
                _ => 0,
            },
            indent_em: p.first_line_indent,
            runs: p.runs.iter().map(UiTextRun::from).collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiHandwritingRole {
    pub id: u32,
    pub name: String,
    pub font_path: String,
    pub printed: bool,
    pub font_size: Option<f32>,
    pub fill: Option<String>, // #RRGGBB
    pub word_spacing: Option<f32>,
    pub line_spacing: Option<f32>,
    pub font_size_sigma: Option<f32>,
    pub word_spacing_sigma: Option<f32>,
    pub line_spacing_sigma: Option<f32>,
    pub perturb_x_sigma: Option<f32>,
    pub perturb_y_sigma: Option<f32>,
    pub perturb_theta_sigma: Option<f32>,
    pub miswrite_rate: Option<f32>,
    pub miswrite_strikeout_style_index: Option<i32>,
}

impl From<&HandwritingRole> for UiHandwritingRole {
    fn from(r: &HandwritingRole) -> Self {
        Self {
            id: r.id,
            name: r.name.clone(),
            font_path: r.font_path.clone(),
            printed: r.printed,
            font_size: r.font_size,
            fill: r
                .fill
                .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])),
            word_spacing: r.word_spacing,
            line_spacing: r.line_spacing,
            font_size_sigma: r.font_size_sigma,
            word_spacing_sigma: r.word_spacing_sigma,
            line_spacing_sigma: r.line_spacing_sigma,
            perturb_x_sigma: r.perturb_x_sigma,
            perturb_y_sigma: r.perturb_y_sigma,
            perturb_theta_sigma: r.perturb_theta_sigma,
            miswrite_rate: r.miswrite_rate,
            miswrite_strikeout_style_index: r.miswrite_strikeout_style.map(|s| match s {
                StrikeoutStyle::Line => 0,
                StrikeoutStyle::DoubleLine => 1,
                StrikeoutStyle::Slash => 2,
                StrikeoutStyle::Cross => 3,
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
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
                    runs: p.runs.iter().map(UiTextRun::from).collect(),
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
            miswrite_strikeout_style_index: r.miswrite_strikeout_style.map(|s| match s {
                StrikeoutStyle::Line => 0,
                StrikeoutStyle::DoubleLine => 1,
                StrikeoutStyle::Slash => 2,
                StrikeoutStyle::Cross => 3,
            }),
            fill: r
                .fill
                .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])),
            margin_top: r.margin_top,
            margin_bottom: r.margin_bottom,
            margin_left: r.margin_left,
            margin_right: r.margin_right,
        }
    }
}

/// 前端表单状态镜像（camelCase JSON）。字段语义见 models.rs。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
    #[serde(default)]
    pub roles: Vec<UiHandwritingRole>,
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
            roles: Vec::new(),
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

        // 角色转换
        p.roles = self
            .roles
            .iter()
            .enumerate()
            .map(|(i, r)| -> Result<HandwritingRole, String> {
                let fill = match &r.fill {
                    Some(hex) if !hex.trim().is_empty() => Some(
                        models::parse_color(hex.trim())
                            .map_err(|e| format!("角色 {} 文字颜色：{e}", i + 1))?,
                    ),
                    _ => None,
                };
                Ok(HandwritingRole {
                    id: r.id,
                    name: r.name.trim().to_string(),
                    font_path: r.font_path.trim().to_string(),
                    printed: r.printed,
                    font_size: r.font_size,
                    fill,
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
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        // 段落路径：非空即走段落（单段无格式的回退由前端负责——它只填 text）
        p.paragraphs = self
            .paragraphs
            .iter()
            .filter(|row| {
                !clean_text(&row.text).trim().is_empty()
                    || row.runs.iter().any(|r| !clean_text(&r.text).trim().is_empty())
            })
            .map(|row| -> Result<Paragraph, String> {
                let runs: Result<Vec<TextRun>, String> = row
                    .runs
                    .iter()
                    .map(|r| {
                        let fill = match &r.style.fill {
                            Some(hex) if !hex.trim().is_empty() => Some(
                                models::parse_color(hex.trim())
                                    .map_err(|e| format!("段落片段文字颜色：{e}"))?,
                            ),
                            _ => None,
                        };
                        Ok(TextRun {
                            text: clean_text(&r.text),
                            style: TextRunStyle {
                                role_id: r.style.role_id,
                                font_path: r.style.font_path.clone().filter(|s| !s.trim().is_empty()),
                                font_size: r.style.font_size,
                                fill,
                                printed: r.style.printed,
                            },
                        })
                    })
                    .collect();
                let runs = runs?;
                let text = if row.text.is_empty() && !runs.is_empty() {
                    runs.iter().map(|r| r.text.as_str()).collect()
                } else {
                    clean_text(&row.text)
                };
                Ok(Paragraph {
                    text,
                    align: align_of(row.align),
                    first_line_indent: row.indent_em * self.font_size,
                    runs,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
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
                let region_fs = if r.font_size > 0 {
                    r.font_size as f32
                } else {
                    self.font_size
                };
                let region_paras: Vec<Paragraph> = r
                    .paragraphs
                    .iter()
                    .filter(|row| {
                        !clean_text(&row.text).trim().is_empty()
                            || row.runs.iter().any(|r| !clean_text(&r.text).trim().is_empty())
                    })
                    .map(|row| -> Result<Paragraph, String> {
                        let runs: Result<Vec<TextRun>, String> = row
                            .runs
                            .iter()
                            .map(|r| {
                                let fill = match &r.style.fill {
                                    Some(hex) if !hex.trim().is_empty() => Some(
                                        models::parse_color(hex.trim())
                                            .map_err(|e| format!("区域段落片段文字颜色：{e}"))?,
                                    ),
                                    _ => None,
                                };
                                Ok(TextRun {
                                    text: clean_text(&r.text),
                                    style: TextRunStyle {
                                        role_id: r.style.role_id,
                                        font_path: r.style.font_path.clone().filter(|s| !s.trim().is_empty()),
                                        font_size: r.style.font_size,
                                        fill,
                                        printed: r.style.printed,
                                    },
                                })
                            })
                            .collect();
                        let runs = runs?;
                        let text = if row.text.is_empty() && !runs.is_empty() {
                            runs.iter().map(|r| r.text.as_str()).collect()
                        } else {
                            clean_text(&row.text)
                        };
                        Ok(Paragraph {
                            text,
                            align: align_of(row.align),
                            first_line_indent: row.indent_em * region_fs,
                            runs,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
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
            roles: p.roles.iter().map(UiHandwritingRole::from).collect(),
            end_chars: p.end_chars.clone(),
            start_chars: p.start_chars.clone(),
            bounds_visible: false,
            bounds_color: "#4ca6a6".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_params_roles_and_runs_roundtrip() {
        let ui_params = UiParams {
            font_path: "fonts/main.ttf".into(),
            background_path: "bg.png".into(),
            fill: "#112233".into(),
            font_size: 32.0,
            roles: vec![
                UiHandwritingRole {
                    id: 1,
                    name: "角色1".into(),
                    font_path: "fonts/role1.ttf".into(),
                    printed: false,
                    font_size: Some(30.0),
                    fill: Some("#ff0000".into()),
                    word_spacing: Some(4.0),
                    line_spacing: Some(40.0),
                    font_size_sigma: Some(1.5),
                    word_spacing_sigma: Some(1.2),
                    line_spacing_sigma: Some(1.3),
                    perturb_x_sigma: Some(1.0),
                    perturb_y_sigma: Some(1.1),
                    perturb_theta_sigma: Some(0.04),
                    miswrite_rate: Some(0.05),
                    miswrite_strikeout_style_index: Some(2), // Slash
                },
                UiHandwritingRole {
                    id: 2,
                    name: "打印角色".into(),
                    font_path: "fonts/print.ttf".into(),
                    printed: true,
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
                    miswrite_strikeout_style_index: None,
                },
            ],
            paragraphs: vec![
                UiParagraph {
                    text: "".into(),
                    align: 1, // Center
                    indent_em: 2.0,
                    runs: vec![
                        UiTextRun {
                            text: "标题前缀".into(),
                            style: UiTextRunStyle {
                                role_id: 1,
                                font_path: Some("fonts/custom.ttf".into()),
                                font_size: Some(28.0),
                                fill: Some("#00ff00".into()),
                                printed: false,
                            },
                        },
                        UiTextRun {
                            text: "标题正文".into(),
                            style: UiTextRunStyle {
                                role_id: 2,
                                font_path: None,
                                font_size: None,
                                fill: None,
                                printed: true,
                            },
                        },
                    ],
                },
            ],
            ..UiParams::default()
        };

        // Convert to HandwritingParams
        let hp = ui_params.to_handwriting_params().unwrap();
        assert_eq!(hp.fill, [0x11, 0x22, 0x33]);
        assert_eq!(hp.roles.len(), 2);
        assert_eq!(hp.roles[0].id, 1);
        assert_eq!(hp.roles[0].name, "角色1");
        assert_eq!(hp.roles[0].fill, Some([0xff, 0x00, 0x00]));
        assert_eq!(hp.roles[0].miswrite_strikeout_style, Some(StrikeoutStyle::Slash));
        assert!(hp.roles[1].printed);
        assert_eq!(hp.roles[1].fill, None);

        assert_eq!(hp.paragraphs.len(), 1);
        assert_eq!(hp.paragraphs[0].align, Align::Center);
        assert_eq!(hp.paragraphs[0].first_line_indent, 64.0); // 2.0 * 32.0
        assert_eq!(hp.paragraphs[0].runs.len(), 2);
        assert_eq!(hp.paragraphs[0].runs[0].text, "标题前缀");
        assert_eq!(hp.paragraphs[0].runs[0].style.role_id, 1);
        assert_eq!(hp.paragraphs[0].runs[0].style.fill, Some([0x00, 0xff, 0x00]));
        assert_eq!(hp.paragraphs[0].runs[0].style.font_size, Some(28.0));
        assert_eq!(hp.paragraphs[0].runs[1].text, "标题正文");
        assert_eq!(hp.paragraphs[0].runs[1].style.role_id, 2);
        assert!(hp.paragraphs[0].runs[1].style.printed);

        // Convert back via from_handwriting_params (for preset reloading)
        let loaded_ui = UiParams::from_handwriting_params(&hp);
        assert_eq!(loaded_ui.fill, "#112233");
        assert_eq!(loaded_ui.roles.len(), 2);
        assert_eq!(loaded_ui.roles[0].id, 1);
        assert_eq!(loaded_ui.roles[0].name, "角色1");
        assert_eq!(loaded_ui.roles[0].fill, Some("#ff0000".into()));
        assert_eq!(loaded_ui.roles[0].miswrite_strikeout_style_index, Some(2));
        assert_eq!(loaded_ui.roles[1].name, "打印角色");
        assert!(loaded_ui.roles[1].printed);
    }

    #[test]
    fn test_ui_paragraph_from_core_paragraph() {
        let para = Paragraph {
            text: "全段文本".into(),
            align: Align::Right,
            first_line_indent: 72.0,
            runs: vec![
                TextRun::new("片段1", TextRunStyle { role_id: 1, fill: Some([255, 0, 0]), ..Default::default() }),
                TextRun::new("片段2", TextRunStyle { role_id: 2, printed: true, ..Default::default() }),
            ],
        };

        let ui_para = UiParagraph::from_paragraph_with_font_size(&para, 36.0);
        assert_eq!(ui_para.text, "全段文本");
        assert_eq!(ui_para.align, 2);
        assert_eq!(ui_para.indent_em, 2.0); // 72.0 / 36.0
        assert_eq!(ui_para.runs.len(), 2);
        assert_eq!(ui_para.runs[0].text, "片段1");
        assert_eq!(ui_para.runs[0].style.role_id, 1);
        assert_eq!(ui_para.runs[0].style.fill, Some("#ff0000".into()));
        assert_eq!(ui_para.runs[1].text, "片段2");
        assert_eq!(ui_para.runs[1].style.role_id, 2);
        assert!(ui_para.runs[1].style.printed);

        let ui_para_from: UiParagraph = (&para).into();
        assert_eq!(ui_para_from.text, "全段文本");
        assert_eq!(ui_para_from.align, 2);
        assert_eq!(ui_para_from.indent_em, 72.0);
        assert_eq!(ui_para_from.runs.len(), 2);
    }

    #[test]
    fn test_invalid_color_reports_error() {
        let ui_params = UiParams {
            fill: "not-a-color".into(),
            ..UiParams::default()
        };
        assert!(ui_params.to_handwriting_params().is_err());

        let ui_role_bad_color = UiParams {
            roles: vec![UiHandwritingRole {
                fill: Some("badhex".into()),
                ..UiHandwritingRole::default()
            }],
            ..UiParams::default()
        };
        assert!(ui_role_bad_color.to_handwriting_params().is_err());

        let ui_run_bad_color = UiParams {
            paragraphs: vec![UiParagraph {
                text: "段落".into(),
                runs: vec![UiTextRun {
                    text: "片段".into(),
                    style: UiTextRunStyle {
                        fill: Some("xyz".into()),
                        ..UiTextRunStyle::default()
                    },
                }],
                ..UiParagraph::default()
            }],
            ..UiParams::default()
        };
        assert!(ui_run_bad_color.to_handwriting_params().is_err());
    }
}

