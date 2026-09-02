//! 系统字体发现与扫描模块。
//!
//! 跨平台扫描系统预装字体（Windows / macOS / Linux），识别常见中英文名字体，
//! 为前端字体选择器提供友好的展示名称、字体家族和文件物理路径。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 系统字体项
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemFontItem {
    /// 友好显示名称，例如 "微软雅黑 (Microsoft YaHei)"、"Arial"
    pub name: String,
    /// 匹配及展示用家族名，例如 "微软雅黑"、"Arial"
    pub family: String,
    /// 字体文件物理绝对路径，例如 "C:\Windows\Fonts\msyh.ttc"
    pub path: String,
}

/// 匹配已知字体文件名的中英文字体映射
fn map_known_font(file_name_lower: &str) -> Option<(&'static str, &'static str)> {
    match file_name_lower {
        // --- Windows 常见中文字体 ---
        "msyh.ttc" => Some(("微软雅黑", "Microsoft YaHei")),
        "msyhl.ttc" => Some(("微软雅黑 Light", "Microsoft YaHei Light")),
        "msyhbd.ttc" => Some(("微软雅黑 Bold", "Microsoft YaHei Bold")),
        "simsun.ttc" => Some(("宋体 / 新宋体", "SimSun")),
        "simsunb.ttf" => Some(("宋体扩展", "SimSun-ExtB")),
        "simhei.ttf" => Some(("黑体", "SimHei")),
        "simkai.ttf" | "kaiti.ttf" => Some(("楷体", "KaiTi")),
        "simfang.ttf" | "fangsong.ttf" => Some(("仿宋", "FangSong")),
        "stxingka.ttf" => Some(("华文行楷", "STXingkai")),
        "stkaiti.ttf" => Some(("华文楷体", "STKaiti")),
        "stsong.ttf" => Some(("华文宋体", "STSong")),
        "stfangso.ttf" => Some(("华文仿宋", "STFangsong")),
        "stxihei.ttf" => Some(("华文细黑", "STXihei")),
        "stzhongh.ttf" => Some(("华文中宋", "STZhongsong")),
        "deng.ttf" => Some(("等线", "DengXian")),
        "dengb.ttf" => Some(("等线 Bold", "DengXian Bold")),
        "dengl.ttf" => Some(("等线 Light", "DengXian Light")),
        "simyou.ttf" | "youyuan.ttf" => Some(("幼圆", "YouYuan")),
        "simli.ttf" | "lishu.ttf" => Some(("隶书", "LiSu")),
        "stcaiyun.ttf" => Some(("华文彩云", "STCaiyun")),
        "sthupo.ttf" => Some(("华文琥珀", "STHupo")),
        "stliti.ttf" => Some(("华文隶书", "STLiti")),
        "stxinwei.ttf" => Some(("华文新魏", "STXinwei")),
        "fzstk.ttf" => Some(("方正舒体", "FZShuTi")),
        "fzytk.ttf" => Some(("方正姚体", "FZYaoTi")),

        // --- Windows 常见英文字体 ---
        "arial.ttf" => Some(("Arial", "Arial")),
        "arialbd.ttf" => Some(("Arial Bold", "Arial Bold")),
        "ariali.ttf" => Some(("Arial Italic", "Arial Italic")),
        "arialbi.ttf" => Some(("Arial Bold Italic", "Arial Bold Italic")),
        "times.ttf" => Some(("Times New Roman", "Times New Roman")),
        "timesbd.ttf" => Some(("Times New Roman Bold", "Times New Roman Bold")),
        "timesi.ttf" => Some(("Times New Roman Italic", "Times New Roman Italic")),
        "timesbi.ttf" => Some(("Times New Roman Bold Italic", "Times New Roman Bold Italic")),
        "calibri.ttf" => Some(("Calibri", "Calibri")),
        "calibrib.ttf" => Some(("Calibri Bold", "Calibri Bold")),
        "calibrii.ttf" => Some(("Calibri Italic", "Calibri Italic")),
        "calibriz.ttf" => Some(("Calibri Bold Italic", "Calibri Bold Italic")),
        "calibril.ttf" => Some(("Calibri Light", "Calibri Light")),
        "calibrili.ttf" => Some(("Calibri Light Italic", "Calibri Light Italic")),
        "cour.ttf" => Some(("Courier New", "Courier New")),
        "courbd.ttf" => Some(("Courier New Bold", "Courier New Bold")),
        "couri.ttf" => Some(("Courier New Italic", "Courier New Italic")),
        "courbi.ttf" => Some(("Courier New Bold Italic", "Courier New Bold Italic")),
        "tahoma.ttf" => Some(("Tahoma", "Tahoma")),
        "tahomabd.ttf" => Some(("Tahoma Bold", "Tahoma Bold")),
        "verdana.ttf" => Some(("Verdana", "Verdana")),
        "verdanab.ttf" => Some(("Verdana Bold", "Verdana Bold")),
        "verdanai.ttf" => Some(("Verdana Italic", "Verdana Italic")),
        "verdanaz.ttf" => Some(("Verdana Bold Italic", "Verdana Bold Italic")),
        "segoeui.ttf" => Some(("Segoe UI", "Segoe UI")),
        "segoeuib.ttf" => Some(("Segoe UI Bold", "Segoe UI Bold")),
        "segoeuii.ttf" => Some(("Segoe UI Italic", "Segoe UI Italic")),
        "segoeuiz.ttf" => Some(("Segoe UI Bold Italic", "Segoe UI Bold Italic")),
        "segoeuil.ttf" => Some(("Segoe UI Light", "Segoe UI Light")),
        "segoeuisb.ttf" => Some(("Segoe UI Semibold", "Segoe UI Semibold")),
        "segoeuisz.ttf" => Some(("Segoe UI Semibold Italic", "Segoe UI Semibold Italic")),
        "segoeuisl.ttf" => Some(("Segoe UI Semilight", "Segoe UI Semilight")),
        "segoeuibl.ttf" => Some(("Segoe UI Black", "Segoe UI Black")),

        // --- macOS 常见中文字体 ---
        "pingfang.ttc" => Some(("苹方", "PingFang SC")),
        "songti.ttc" => Some(("宋体", "Songti SC")),
        "kaiti.ttc" => Some(("楷体", "Kaiti SC")),
        "heiti.ttc" => Some(("黑体", "Heiti SC")),
        "yuanti.ttc" => Some(("圆体", "Yuanti SC")),
        "stheitilight.ttc" => Some(("华文细黑", "STHeiti Light")),
        "stheitimedium.ttc" => Some(("华文黑体", "STHeiti Medium")),
        "hiraginosansgb.ttc" => Some(("冬青黑体", "Hiragino Sans GB")),

        // --- Linux / 开源 常见中文字体 ---
        "notosanscjk-regular.ttc"
        | "notosanscjk.ttc"
        | "notosanssc-regular.otf"
        | "notosanssc-regular.ttf" => Some(("思源黑体", "Noto Sans SC")),
        "notoserifcjk-regular.ttc"
        | "notoserifcjk.ttc"
        | "notoserifsc-regular.otf"
        | "notoserifsc-regular.ttf" => Some(("思源宋体", "Noto Serif SC")),
        "wenquanyimicrohei.ttf" | "wqy-microhei.ttc" => {
            Some(("文泉驿微米黑", "WenQuanYi Micro Hei"))
        }
        "wenquanyizenhei.ttf" | "wqy-zenhei.ttc" => Some(("文泉驿正黑", "WenQuanYi Zen Hei")),

        _ => None,
    }
}

/// 计算字体优先级（越小越靠前）：常用中文楷体/宋体/黑体/仿宋置顶
fn font_priority(family: &str, file_name_lower: &str) -> u32 {
    let f_low = family.to_lowercase();
    if f_low.contains("微软雅黑") || file_name_lower.starts_with("msyh") {
        10
    } else if f_low.contains("宋体")
        || file_name_lower.starts_with("simsun")
        || file_name_lower.starts_with("songti")
    {
        20
    } else if f_low.contains("黑体")
        || file_name_lower.starts_with("simhei")
        || file_name_lower.starts_with("heiti")
        || f_low.contains("苹方")
        || file_name_lower.starts_with("pingfang")
    {
        30
    } else if f_low.contains("楷体")
        || file_name_lower.starts_with("simkai")
        || file_name_lower.starts_with("kaiti")
    {
        40
    } else if f_low.contains("仿宋")
        || file_name_lower.starts_with("simfang")
        || file_name_lower.starts_with("fangsong")
    {
        50
    } else if f_low.contains("行楷") || file_name_lower.starts_with("stxingka") {
        60
    } else if f_low.contains("华文") || file_name_lower.starts_with("st") {
        70
    } else if f_low.contains("等线") || file_name_lower.starts_with("deng") {
        80
    } else if f_low.contains("思源")
        || file_name_lower.contains("noto")
        || f_low.contains("文泉驿")
        || file_name_lower.starts_with("wqy")
    {
        90
    } else if f_low.contains("arial")
        || f_low.contains("times")
        || f_low.contains("calibri")
        || f_low.contains("segoe")
        || f_low.contains("tahoma")
        || f_low.contains("verdana")
        || f_low.contains("courier")
    {
        150
    } else {
        200
    }
}

/// 获取系统字体搜索目录列表
fn get_font_search_paths() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        } else if let Ok(sysroot) = std::env::var("SystemRoot") {
            dirs.push(PathBuf::from(sysroot).join("Fonts"));
        } else {
            dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        }

        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(localappdata)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }

        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(
                PathBuf::from(appdata)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
            dirs.push(PathBuf::from(home).join(".fonts"));
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        dirs.push(PathBuf::from("/usr/share/fonts"));
    }

    dirs
}

/// 递归扫描目录下的字体文件（.ttf, .otf, .ttc）
fn scan_dir_for_fonts(dir: &Path, max_depth: usize, out: &mut Vec<PathBuf>) {
    if max_depth == 0 || !dir.is_dir() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    let ext_lower = ext.to_ascii_lowercase();
                    if ext_lower == "ttf" || ext_lower == "otf" || ext_lower == "ttc" {
                        out.push(path);
                    }
                }
            } else if path.is_dir() {
                scan_dir_for_fonts(&path, max_depth - 1, out);
            }
        }
    }
}

/// 扫描并返回系统安装的字体列表，自动去重并按常用优先级排序
pub fn list_system_fonts() -> Vec<SystemFontItem> {
    let search_dirs = get_font_search_paths();
    let mut font_files = Vec::new();

    for dir in &search_dirs {
        if dir.exists() {
            scan_dir_for_fonts(dir, 4, &mut font_files);
        }
    }

    let mut seen_paths = HashSet::new();
    let mut items = Vec::new();

    for path in font_files {
        let path_str = path.to_string_lossy().into_owned();
        let path_key = path_str.to_lowercase();
        if !seen_paths.insert(path_key) {
            continue;
        }

        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_name_lower = file_name.to_lowercase();

        let (name, family) = if let Some((zh, en)) = map_known_font(&file_name_lower) {
            if zh == en {
                (en.to_string(), en.to_string())
            } else {
                (format!("{zh} ({en})"), zh.to_string())
            }
        } else {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file_name.clone());
            (stem.clone(), stem)
        };

        items.push((name, family, file_name_lower, path_str));
    }

    // 排序：优先级升序，名称字母序升序
    items.sort_by(|(name_a, fam_a, fn_a, path_a), (name_b, fam_b, fn_b, path_b)| {
        let p_a = font_priority(fam_a, fn_a);
        let p_b = font_priority(fam_b, fn_b);
        p_a.cmp(&p_b)
            .then_with(|| name_a.to_lowercase().cmp(&name_b.to_lowercase()))
            .then_with(|| path_a.cmp(path_b))
    });

    items
        .into_iter()
        .map(|(name, family, _, path)| SystemFontItem { name, family, path })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_list_system_fonts() {
        let fonts = list_system_fonts();
        eprintln!("测试发现系统字体数量: {}", fonts.len());

        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        {
            assert!(
                !fonts.is_empty(),
                "在主流系统平台上应能扫描到至少一个系统字体"
            );
        }

        for font in &fonts {
            assert!(!font.name.is_empty(), "字体名称不应为空");
            assert!(!font.family.is_empty(), "字体家族不应为空");
            assert!(!font.path.is_empty(), "字体路径不应为空");
            assert!(
                Path::new(&font.path).is_file(),
                "扫描到的字体路径必须为真实存在的文件：{}",
                font.path
            );
        }

        if !fonts.is_empty() {
            let first = &fonts[0];
            eprintln!(
                "排序第一位字体: {} (family: {}) @ {}",
                first.name, first.family, first.path
            );
        }
    }
}
