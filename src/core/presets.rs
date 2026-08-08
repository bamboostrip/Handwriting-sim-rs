//! 参数预设的 JSON 读写（兼容 Python 版 presets.py 格式）。
//!
//! - 格式：`{"version": 2, "params": {...}}`，params 不含 text/paragraphs，
//!   颜色以 `color: "#RRGGBB"` 保存。
//! - 便携模式：exe 目录为资产根，其内字体/背景路径保存为相对路径，
//!   载入时解析回绝对路径。
//! - 兼容载入：`color` 与 `red/green/blue` 两种颜色写法；未知字段忽略。

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::core::models::HandwritingParams;

/// 预设 JSON 错误。
#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 解析失败：{0}")]
    Json(#[from] serde_json::Error),
    #[error("预设格式错误：{0}")]
    Format(String),
}

/// 资产根目录 = exe 所在目录（便携模式基准）。
pub fn assets_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 绝对路径位于资产根目录内时转为相对路径（跨盘符/非法路径原样保留）。
pub fn to_portable_path(path: &str) -> String {
    if path.is_empty() {
        return path.to_string();
    }
    let root = assets_root();
    let abs = Path::new(path);
    if let Ok(rel) = abs.strip_prefix(&root) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    path.to_string()
}

/// 预设中的相对路径按资产根目录解析为绝对路径；绝对路径原样返回。
pub fn from_portable_path(path: &str) -> String {
    if path.is_empty() || Path::new(path).is_absolute() {
        return path.to_string();
    }
    // 用 components() 归一化分隔符：join 的路径含 "fonts/msyh.ttc" 这类正斜杠，
    // to_string_lossy 会保留原始分隔符，导致 Windows 上出现混合分隔符；
    // 重新收集组件可统一为平台原生分隔符。
    assets_root()
        .join(path)
        .components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned()
}

/// 把参数序列化为 Python 兼容预设 map（不含 text/paragraphs）。
fn to_preset_map(params: &HandwritingParams) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("font_path".into(), json!(to_portable_path(&params.font_path)));
    m.insert("background_path".into(), json!(to_portable_path(&params.background_path)));
    m.insert("font_size".into(), json!(params.font_size));
    m.insert("word_spacing".into(), json!(params.word_spacing));
    m.insert("line_spacing".into(), json!(params.line_spacing));
    m.insert("left_margin".into(), json!(params.left_margin));
    m.insert("right_margin".into(), json!(params.right_margin));
    m.insert("top_margin".into(), json!(params.top_margin));
    m.insert("bottom_margin".into(), json!(params.bottom_margin));
    m.insert("word_spacing_sigma".into(), json!(params.word_spacing_sigma));
    m.insert("line_spacing_sigma".into(), json!(params.line_spacing_sigma));
    m.insert("font_size_sigma".into(), json!(params.font_size_sigma));
    m.insert("perturb_x_sigma".into(), json!(params.perturb_x_sigma));
    m.insert("perturb_y_sigma".into(), json!(params.perturb_y_sigma));
    m.insert("perturb_theta_sigma".into(), json!(params.perturb_theta_sigma));
    m.insert("end_chars".into(), json!(params.end_chars));
    m.insert("start_chars".into(), json!(params.start_chars));
    m.insert("color".into(), json!(format!("#{:02x}{:02x}{:02x}", params.fill[0], params.fill[1], params.fill[2])));
    m
}

/// 从 Python 兼容预设 map 载入参数（未知字段忽略，缺失字段用默认值）。
fn from_preset_map(data: &Map<String, Value>) -> Result<HandwritingParams, PresetError> {
    let mut p = HandwritingParams::default();
    let num = |key: &str| -> Option<f32> {
        data.get(key).and_then(|v| v.as_f64()).map(|f| f as f32)
    };
    let str_ = |key: &str| -> Option<String> {
        data.get(key).and_then(|v| v.as_str()).map(String::from)
    };
    if let Some(v) = num("font_size") { p.font_size = v; }
    if let Some(v) = num("word_spacing") { p.word_spacing = v; }
    if let Some(v) = num("line_spacing") { p.line_spacing = v; }
    if let Some(v) = num("left_margin") { p.left_margin = v; }
    if let Some(v) = num("right_margin") { p.right_margin = v; }
    if let Some(v) = num("top_margin") { p.top_margin = v; }
    if let Some(v) = num("bottom_margin") { p.bottom_margin = v; }
    if let Some(v) = num("word_spacing_sigma") { p.word_spacing_sigma = v; }
    if let Some(v) = num("line_spacing_sigma") { p.line_spacing_sigma = v; }
    if let Some(v) = num("font_size_sigma") { p.font_size_sigma = v; }
    if let Some(v) = num("perturb_x_sigma") { p.perturb_x_sigma = v; }
    if let Some(v) = num("perturb_y_sigma") { p.perturb_y_sigma = v; }
    if let Some(v) = num("perturb_theta_sigma") { p.perturb_theta_sigma = v; }
    if let Some(v) = str_("end_chars") { p.end_chars = v; }
    if let Some(v) = str_("start_chars") { p.start_chars = v; }
    if let Some(v) = str_("font_path") { p.font_path = from_portable_path(&v); }
    if let Some(v) = str_("background_path") { p.background_path = from_portable_path(&v); }
    // 颜色：优先 #RRGGBB，其次 red/green/blue
    if let Some(v) = str_("color") {
        let hex = v.trim_start_matches('#');
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                p.fill = [r, g, b];
            }
        }
    } else {
        let rgb = |key: &str| data.get(key).and_then(|v| v.as_i64()).map(|i| i as u8).unwrap_or(0);
        p.fill = [rgb("red"), rgb("green"), rgb("blue")];
    }
    Ok(p)
}

/// 保存为 Python 兼容 JSON 预设。
pub fn save(params: &HandwritingParams, path: &Path) -> Result<(), PresetError> {
    let data = json!({ "version": 2, "params": to_preset_map(params) });
    let text = serde_json::to_string_pretty(&data)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// 载入 JSON 预设（兼容 Python 格式）。
pub fn load(path: &Path) -> Result<HandwritingParams, PresetError> {
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    let map = value
        .get("params")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
        .ok_or_else(|| PresetError::Format("预设顶层应为对象".into()))?;
    from_preset_map(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::HandwritingParams;

    fn sample_params() -> HandwritingParams {
        HandwritingParams {
            font_path: r"C:\Windows\Fonts\msyh.ttc".into(),
            background_path: r"C:\Users\me\bg.png".into(),
            text: "不应被保存".into(),
            font_size: 40.0,
            fill: [12, 34, 56],
            ..HandwritingParams::default()
        }
    }

    #[test]
    fn save_load_roundtrip_excludes_text_and_paragraphs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset.json");
        let p = sample_params();
        save(&p, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.font_size, 40.0);
        assert_eq!(loaded.fill, [12, 34, 56]);
        assert!(loaded.text.is_empty(), "预设不应包含文本");
        assert!(loaded.paragraphs.is_empty());
        assert_eq!(loaded.font_path, p.font_path);
        assert_eq!(loaded.background_path, p.background_path);
    }

    #[test]
    fn load_python_style_json_with_color() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("py.json");
        std::fs::write(
            &path,
            r##"{
              "version": 2,
              "params": {
                "font_path": "fonts/msyh.ttc",
                "background_path": "D:/bg.png",
                "font_size": 48,
                "word_spacing": 6,
                "line_spacing": 60,
                "left_margin": 40,
                "right_margin": 40,
                "top_margin": 40,
                "bottom_margin": 40,
                "word_spacing_sigma": 3,
                "line_spacing_sigma": 3,
                "font_size_sigma": 3,
                "perturb_x_sigma": 3,
                "perturb_y_sigma": 3,
                "perturb_theta_sigma": 0.1,
                "end_chars": "，。！？",
                "start_chars": "",
                "color": "#ff0000",
                "unknown_field": 123
              }
            }"##,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.font_size, 48.0);
        assert_eq!(loaded.fill, [255, 0, 0]);
        assert_eq!(loaded.end_chars, "，。！？");
        assert_eq!(loaded.perturb_theta_sigma, 0.1);
    }

    #[test]
    fn load_python_style_json_with_rgb_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("py2.json");
        std::fs::write(
            &path,
            r#"{"version": 2, "params": {"red": 1, "green": 2, "blue": 3, "font_size": 20}}"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.fill, [1, 2, 3]);
        assert_eq!(loaded.font_size, 20.0);
    }

    #[test]
    fn portable_path_relative_to_assets_root() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset.json");
        let mut p = sample_params();
        // background 置空，避免绝对路径 C:\ 混入（本测试只关注资产根内字体路径）
        p.background_path = String::new();
        // 模拟 exe 目录（assets_root 返回目录）下的字体
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let font_in_assets = exe_dir.join("fonts").join("msyh.ttc");
        p.font_path = font_in_assets.to_string_lossy().into_owned();
        save(&p, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("fonts/msyh.ttc") && !raw.contains("C:\\"),
            "资产根内路径应存为相对路径：{raw}"
        );
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.font_path, font_in_assets.to_string_lossy());
    }
}