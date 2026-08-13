//! UI 参数状态与「收集 / 回填」纯函数。
//!
//! 对应 Slint 版 `main.rs` 中与控件绑定的 in-out 属性、`apply_preset_to_ui`
//! 与 `collect_params`。此处为纯数据载体 + 纯函数，不依赖任何 GUI 框架，
//! 便于单元测试与逻辑复用。

use crate::core::engine::EngineError;
use crate::core::models::{
    parse_color, HandwritingParams, MiswriteMode, Paragraph, StrikeoutStyle,
};

/// UI 控件值的集合（数值输入 / 下拉 / 滑块 / 开关 / 文本输入）。
/// 默认值与 Slint 版控件初始值一一对应。
#[derive(Debug, Clone)]
pub struct UiParams {
    pub font_path: String,
    pub background_path: String,
    // 排版参数
    pub font_size: i32,
    pub line_spacing: i32,
    pub word_spacing: i32,
    pub word_spacing_sigma: i32,
    pub line_spacing_sigma: i32,
    pub font_size_sigma: i32,
    // 笔画扰动
    pub perturb_x: i32,
    pub perturb_y: i32,
    /// 笔画旋转：文本输入浮点数（对齐 Python 版 QDoubleSpinBox 的直接输入）。
    pub perturb_theta: String,
    // 写错字模拟
    /// 错字率显示值（0~30，百分数）。
    pub miswrite_rate: f32,
    /// 0 右上方重写 / 1 后文重写。
    pub miswrite_mode: i32,
    /// 0 单线 / 1 双线 / 2 斜线 / 3 叉号。
    pub miswrite_strikeout: i32,
    /// 文字颜色 `#RRGGBB`。
    pub font_color: String,
    // 边距
    pub margin_top: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub margin_right: i32,
    // 边界提示（仅预览）
    pub bounds_visible: bool,
    pub bounds_color: String,
}

impl Default for UiParams {
    fn default() -> Self {
        Self {
            font_path: String::new(),
            background_path: String::new(),
            font_size: 36,
            line_spacing: 48,
            word_spacing: 5,
            word_spacing_sigma: 2,
            line_spacing_sigma: 2,
            font_size_sigma: 2,
            perturb_x: 2,
            perturb_y: 2,
            perturb_theta: "0.05".to_string(),
            miswrite_rate: 0.0,
            miswrite_mode: 0,
            miswrite_strikeout: 0,
            font_color: "#000000".to_string(),
            margin_top: 30,
            margin_bottom: 30,
            margin_left: 30,
            margin_right: 30,
            bounds_visible: false,
            bounds_color: "#4ca6a6".to_string(),
        }
    }
}

/// 把预设参数回填为 UI 状态（对应 Slint 版 `apply_preset_to_ui`）。
pub fn apply_preset(p: &HandwritingParams) -> UiParams {
    UiParams {
        font_path: p.font_path.clone(),
        background_path: p.background_path.clone(),
        font_size: p.font_size as i32,
        line_spacing: p.line_spacing as i32,
        word_spacing: p.word_spacing as i32,
        word_spacing_sigma: p.word_spacing_sigma as i32,
        line_spacing_sigma: p.line_spacing_sigma as i32,
        font_size_sigma: p.font_size_sigma as i32,
        perturb_x: p.perturb_x_sigma as i32,
        perturb_y: p.perturb_y_sigma as i32,
        perturb_theta: format!("{}", p.perturb_theta_sigma),
        miswrite_rate: p.miswrite_rate * 100.0,
        miswrite_mode: match p.miswrite_rewrite_mode {
            MiswriteMode::Above => 0,
            MiswriteMode::Rewrite => 1,
        },
        miswrite_strikeout: match p.miswrite_strikeout_style {
            StrikeoutStyle::Line => 0,
            StrikeoutStyle::DoubleLine => 1,
            StrikeoutStyle::Slash => 2,
            StrikeoutStyle::Cross => 3,
        },
        font_color: format!("#{:02x}{:02x}{:02x}", p.fill[0], p.fill[1], p.fill[2]),
        margin_top: p.top_margin as i32,
        margin_bottom: p.bottom_margin as i32,
        margin_left: p.left_margin as i32,
        margin_right: p.right_margin as i32,
        bounds_visible: false,
        bounds_color: "#4ca6a6".to_string(),
    }
}

/// 收集 UI 参数为 `HandwritingParams` 并校验。
///
/// - 以最近载入预设为基础（`preset_params`），再用 UI 控件值覆盖对应字段，
///   保留预设中 UI 未暴露的 `end_chars` / `start_chars` 等参数；
/// - `paras` 与 `has_format` 来自编辑器（空段已跳过、缩进已换算像素）；
///   多段或任一格式非默认（非左对齐 / 有缩进）时走段落路径，
///   单段无格式时走纯文本路径（与旧行为逐字一致）。
pub fn collect_params(
    ui: &UiParams,
    preset_params: Option<&HandwritingParams>,
    paras: Vec<Paragraph>,
    has_format: bool,
) -> Result<HandwritingParams, EngineError> {
    let mut params = preset_params.cloned().unwrap_or_default();
    if paras.len() > 1 || has_format {
        params.paragraphs = paras;
        params.text = String::new();
    } else {
        params.text = paras
            .first()
            .map(|p| p.text.trim().to_string())
            .unwrap_or_default();
        params.paragraphs = Vec::new();
    }
    params.font_path = ui.font_path.trim().to_string();
    params.background_path = ui.background_path.trim().to_string();
    params.font_size = ui.font_size as f32;
    params.line_spacing = ui.line_spacing as f32;
    params.word_spacing = ui.word_spacing as f32;
    params.word_spacing_sigma = ui.word_spacing_sigma as f32;
    params.line_spacing_sigma = ui.line_spacing_sigma as f32;
    params.font_size_sigma = ui.font_size_sigma as f32;
    params.perturb_x_sigma = ui.perturb_x as f32;
    params.perturb_y_sigma = ui.perturb_y as f32;
    // 笔画旋转：解析失败回退默认值（对齐 Python 版 _float_of 的语义）
    params.perturb_theta_sigma = ui
        .perturb_theta
        .trim()
        .parse::<f32>()
        .unwrap_or(HandwritingParams::default().perturb_theta_sigma);
    params.top_margin = ui.margin_top as f32;
    params.bottom_margin = ui.margin_bottom as f32;
    params.left_margin = ui.margin_left as f32;
    params.right_margin = ui.margin_right as f32;
    params.miswrite_rate = ui.miswrite_rate / 100.0;
    params.miswrite_rewrite_mode = match ui.miswrite_mode {
        1 => MiswriteMode::Rewrite,
        _ => MiswriteMode::Above,
    };
    params.miswrite_strikeout_style = match ui.miswrite_strikeout {
        1 => StrikeoutStyle::DoubleLine,
        2 => StrikeoutStyle::Slash,
        3 => StrikeoutStyle::Cross,
        _ => StrikeoutStyle::Line,
    };
    params.fill = parse_color(&ui.font_color).map_err(EngineError::Params)?;
    params.validate()?;
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Align;

    /// 测试环境：真实存在的临时字体/背景文件（`validate()` 校验 is_file），
    /// 随结构体存活保持路径有效。
    struct TestEnv {
        _font: tempfile::NamedTempFile,
        _bg: tempfile::NamedTempFile,
        ui: UiParams,
    }

    fn test_env(font_size: i32) -> TestEnv {
        let font = tempfile::NamedTempFile::new().unwrap();
        let bg = tempfile::NamedTempFile::new().unwrap();
        let ui = UiParams {
            font_path: font.path().to_string_lossy().into_owned(),
            background_path: bg.path().to_string_lossy().into_owned(),
            font_size,
            ..Default::default()
        };
        TestEnv {
            _font: font,
            _bg: bg,
            ui,
        }
    }

    /// 单段无格式文本（走纯文本路径，validate 通过）。
    fn plain_para(text: &str) -> Vec<Paragraph> {
        vec![Paragraph {
            text: text.to_string(),
            align: Align::Left,
            first_line_indent: 0.0,
        }]
    }

    #[test]
    fn collect_single_plain_paragraph_uses_text_path() {
        let paras = vec![Paragraph {
            text: "  你好  ".to_string(),
            align: Align::Left,
            first_line_indent: 0.0,
        }];
        let params = collect_params(&test_env(36).ui, None, paras, false).unwrap();
        // 单段无格式走纯文本路径：trim
        assert_eq!(params.text, "你好");
        assert!(params.paragraphs.is_empty());
    }

    #[test]
    fn collect_multi_paragraph_uses_paragraph_path() {
        let paras = vec![
            Paragraph {
                text: "第一段".to_string(),
                align: Align::Left,
                first_line_indent: 0.0,
            },
            Paragraph {
                text: "第二段".to_string(),
                align: Align::Center,
                first_line_indent: 72.0,
            },
        ];
        let params = collect_params(&test_env(36).ui, None, paras, true).unwrap();
        assert_eq!(params.paragraphs.len(), 2);
        assert_eq!(params.paragraphs[1].align, Align::Center);
        assert_eq!(params.paragraphs[1].first_line_indent, 72.0);
        assert!(params.text.is_empty());
    }

    #[test]
    fn collect_applies_ui_values_over_preset_base() {
        let preset = HandwritingParams {
            end_chars: "自定义".to_string(),
            font_size: 10.0,
            ..Default::default()
        };
        let env = test_env(42);
        let params = collect_params(&env.ui, Some(&preset), plain_para("测试"), false).unwrap();
        // 预设中的未绑定字段保留
        assert_eq!(params.end_chars, "自定义");
        // UI 值覆盖
        assert_eq!(params.font_size, 42.0);
        assert_eq!(params.line_spacing, 48.0);
    }

    #[test]
    fn collect_maps_miswrite_fields() {
        let mut env = test_env(36);
        env.ui.miswrite_rate = 12.5;
        env.ui.miswrite_mode = 1;
        env.ui.miswrite_strikeout = 3;
        let params = collect_params(&env.ui, None, plain_para("测试"), false).unwrap();
        assert!((params.miswrite_rate - 0.125).abs() < 1e-6);
        assert_eq!(params.miswrite_rewrite_mode, MiswriteMode::Rewrite);
        assert_eq!(params.miswrite_strikeout_style, StrikeoutStyle::Cross);
    }

    #[test]
    fn collect_parses_theta_with_fallback() {
        let mut env = test_env(36);
        let ok = collect_params(&env.ui, None, plain_para("测试"), false).unwrap();
        assert!((ok.perturb_theta_sigma - 0.05).abs() < 1e-6);
        env.ui.perturb_theta = "abc".to_string();
        let fallback = collect_params(&env.ui, None, plain_para("测试"), false).unwrap();
        assert!((fallback.perturb_theta_sigma - 0.05).abs() < 1e-6);
    }

    #[test]
    fn collect_parses_color_and_errors_on_bad() {
        let mut env = test_env(36);
        env.ui.font_color = "#123456".to_string();
        let params = collect_params(&env.ui, None, plain_para("测试"), false).unwrap();
        assert_eq!(params.fill, [0x12, 0x34, 0x56]);
        env.ui.font_color = "not-a-color".to_string();
        assert!(collect_params(&env.ui, None, plain_para("测试"), false).is_err());
    }

    #[test]
    fn apply_preset_roundtrip_preserves_fields() {
        // 回填到真实文件路径后收集（validate 需要文件存在）
        let env = test_env(52);
        let p = HandwritingParams {
            font_path: env.ui.font_path.clone(),
            background_path: env.ui.background_path.clone(),
            font_size: 52.0,
            line_spacing: 66.0,
            word_spacing: 9.0,
            perturb_theta_sigma: 0.12,
            miswrite_rate: 0.2,
            miswrite_rewrite_mode: MiswriteMode::Rewrite,
            miswrite_strikeout_style: StrikeoutStyle::Slash,
            fill: [10, 20, 30],
            top_margin: 12.0,
            bottom_margin: 13.0,
            left_margin: 14.0,
            right_margin: 15.0,
            end_chars: "；".to_string(),
            ..Default::default()
        };

        let ui = apply_preset(&p);
        let params = collect_params(&ui, Some(&p), plain_para("测试"), false).unwrap();
        assert_eq!(params.font_size, 52.0);
        assert_eq!(params.line_spacing, 66.0);
        assert_eq!(params.word_spacing, 9.0);
        assert!((params.perturb_theta_sigma - 0.12).abs() < 1e-6);
        assert!((params.miswrite_rate - 0.2).abs() < 1e-6);
        assert_eq!(params.miswrite_rewrite_mode, MiswriteMode::Rewrite);
        assert_eq!(params.miswrite_strikeout_style, StrikeoutStyle::Slash);
        assert_eq!(params.fill, [10, 20, 30]);
        assert_eq!(params.top_margin, 12.0);
        assert_eq!(params.bottom_margin, 13.0);
        assert_eq!(params.left_margin, 14.0);
        assert_eq!(params.right_margin, 15.0);
        // 未绑定字段（end_chars）经预设基础保留
        assert_eq!(params.end_chars, "；");
    }

    #[test]
    fn collect_errors_when_no_text() {
        let env = test_env(36);
        assert!(collect_params(&env.ui, None, Vec::new(), false).is_err());
    }
}
