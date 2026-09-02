//! 手写模拟器桌面端（Tauri 2）。
//!
//! 职责边界：Vue 前端持有全部表单状态（含逐段编辑器与框选区域），
//! 本层只做三件事——参数转换校验、后台渲染/导出（core 引擎）、文件 IO。
//! 预览图以 PNG 写入应用缓存目录，经 asset 协议流式回给 WebView，
//! 避免 base64 经 JSON IPC 的体积膨胀。
//!
//! 渲染代次守卫由前端实现（请求序号，只采纳最新一次结果）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod params;
mod system_fonts;
mod updater;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use handwrite_sim::core::{doc_render, docx_io, engine, models, presets};
use params::UiParams;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// 全局状态：随机种子计数器（每次预览 +1，导出复用当前值——对齐 Slint 版语义）。
struct AppState {
    seed: AtomicU64,
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PngPage {
    /// PNG 绝对路径（前端经 convertFileSrc 转 asset URL 显示）
    path: String,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PresetItem {
    name: String,
    path: String,
}

/// UiParams → 引擎参数 + 校验（纯背景预览合法，对齐原 collect_params 行为）。
fn checked_params(p: &UiParams) -> Result<models::HandwritingParams, String> {
    let hp = p
        .to_handwriting_params()
        .map_err(|e| format!("参数错误：{e}"))?;
    hp.validate_with(false)
        .map_err(|e| format!("参数错误：{e}"))?;
    Ok(hp)
}

/// 应用缓存目录（预览页 PNG / 文档底图共用根）：`%LOCALAPPDATA%/<identifier>/cache`。
/// 与 tauri.conf.json 的 assetProtocol scope 保持一致。
fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("cache");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// 文档底图缓存目录（doc_render 的逐页 PNG 输出）。
fn doc_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("doc_bg");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// 资产根目录（预设/背景扫描基准）：
/// 便携模式 = exe 旁（存在 presets/ 即认定）；开发模式回退仓库根。
fn assets_root() -> PathBuf {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        if dir.join("presets").is_dir() {
            return dir;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// 渲染全部预览页并落盘 PNG。返回页面路径数组供 <img> 显示。
#[tauri::command]
fn render_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    params: UiParams,
) -> Result<Vec<PngPage>, String> {
    let hp = checked_params(&params)?;
    let seed = state.seed.fetch_add(1, Ordering::SeqCst) + 1;
    let t0 = std::time::Instant::now();
    let mut pages = engine::render_all_pages_preview(&hp, seed).map_err(|e| e.to_string())?;
    if params.bounds_visible {
        let color = models::parse_color(params.bounds_color.trim()).unwrap_or([76, 166, 166]);
        for page in pages.iter_mut() {
            *page = engine::overlay_bounds(page, &hp, color);
        }
    }

    let dir = cache_dir(&app)?;
    // 清理上一轮预览残留（本轮会写入全新的一组 preview-*）
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("preview-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let out: Vec<PngPage> = pages
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let path = dir.join(format!("preview-{seed}-{i}.png"));
            page.save(&path).map_err(|e| e.to_string())?;
            Ok::<PngPage, String>(PngPage {
                path: path.to_string_lossy().into_owned(),
                width: page.width(),
                height: page.height(),
            })
        })
        .collect::<Result<_, _>>()?;

    let w = pages.first().map(|p| p.width()).unwrap_or(0);
    let h = pages.first().map(|p| p.height()).unwrap_or(0);
    eprintln!(
        "[GUI] 预览完成：{:.0}ms（{} 页，{w}x{h}，seed={seed}）",
        t0.elapsed().as_secs_f64() * 1000.0,
        pages.len(),
    );
    Ok(out)
}

/// 全分辨率批量导出 PNG（0.png、1.png…）。目录由前端对话框选定后传入。
#[tauri::command]
fn export_files(
    state: State<'_, AppState>,
    params: UiParams,
    dir: String,
) -> Result<Vec<String>, String> {
    let hp = checked_params(&params)?;
    let seed = state.seed.load(Ordering::SeqCst);
    let files = engine::export(&hp, Path::new(dir.trim()), seed).map_err(|e| e.to_string())?;
    Ok(files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect())
}

/// 导出 300 DPI 位图层 PDF。保存路径由前端对话框选定后传入。
#[tauri::command]
fn export_pdf(state: State<'_, AppState>, params: UiParams, path: String) -> Result<(), String> {
    let hp = checked_params(&params)?;
    let seed = state.seed.load(Ordering::SeqCst);
    engine::export_pdf(&hp, Path::new(path.trim()), seed).map_err(|e| e.to_string())
}

/// 导入 docx：解析段落文本 + 对齐 + 首行缩进 + 富文本 Runs。
#[tauri::command]
fn import_docx(path: String, font_size: f32) -> Result<Vec<params::UiParagraph>, String> {
    let paras = docx_io::load_paragraphs(Path::new(path.trim()), font_size)
        .map_err(|e| format!("导入 docx 失败：{e}"))?;
    Ok(paras
        .iter()
        .map(|p| params::UiParagraph::from_paragraph_with_font_size(p, font_size))
        .collect())
}

/// 导入 PDF/DOCX 文档底图：后台栅格化为 200 DPI 逐页 PNG，返回路径列表。
#[tauri::command]
fn import_document(app: AppHandle, path: String) -> Result<Vec<String>, String> {
    let out_dir = doc_cache_dir(&app)?;
    let pages = doc_render::document_to_page_images(Path::new(path.trim()), &out_dir, 200)
        .map_err(|e| format!("导入文档失败：{e}"))?;
    Ok(pages
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// 扫描资产根 presets/ 目录（exe 旁或仓库根），按文件名排序。
#[tauri::command]
fn list_presets() -> Vec<PresetItem> {
    let mut items: Vec<PresetItem> = Vec::new();
    let preset_dir = assets_root().join("presets");
    if let Ok(rd) = std::fs::read_dir(&preset_dir) {
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|e| e == "json").unwrap_or(false))
            .collect();
        files.sort();
        for f in files {
            let name = f
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            items.push(PresetItem {
                name,
                path: f.to_string_lossy().into_owned(),
            });
        }
    }
    items
}

/// 载入预设：解析 JSON v2（兼容 Python 版），回填全部前端字段。
#[tauri::command]
fn load_preset(path: String) -> Result<UiParams, String> {
    let hp = presets::load(Path::new(path.trim())).map_err(|e| format!("载入失败：{e}"))?;
    Ok(UiParams::from_handwriting_params(&hp))
}

/// 保存预设到任意位置（JSON v2，路径转便携相对路径）。
#[tauri::command]
fn save_preset(params: UiParams, path: String) -> Result<(), String> {
    let hp = params
        .to_handwriting_params()
        .map_err(|e| format!("参数错误：{e}"))?;
    presets::save(&hp, Path::new(path.trim())).map_err(|e| format!("保存失败：{e}"))
}

/// 预设默认保存目录（对话框起始位置）。
#[tauri::command]
fn default_preset_dir() -> String {
    assets_root().join("presets").to_string_lossy().into_owned()
}

/// 路径存在性检查（区域打印字体校验等，对齐 Python 版 _on_region_selected）。
#[tauri::command]
fn path_exists(path: String) -> bool {
    !path.trim().is_empty() && Path::new(path.trim()).is_file()
}

/// 读取图片尺寸（只读文件头不完整解码）。返回 [宽, 高]；失败返回 null。
///
/// 供前端做框选坐标换算（背景原始像素 ↔ 显示像素）。
/// 不走 asset 协议：用户可选任意磁盘路径的背景，不受 assetProtocol scope 限制。
#[tauri::command]
fn image_dimensions(path: String) -> Option<[u32; 2]> {
    if path.trim().is_empty() {
        return None;
    }
    image::ImageReader::open(path.trim())
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| [w, h])
}

/// 便携版 WebView2 固定运行时（Fixed Runtime）探测：
/// 若 exe 同目录存在 `WebView2` / `WebView2Runtime` / `Microsoft.WebView2.*` 等
/// 包含 `msedgewebview2.exe` 的子目录，则设为 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`，
/// 使 wry/WebView2 优先加载该目录而非系统安装版，实现“解压即用、离线可用”。
/// 参考：Tauri `fixedVersion` 模式 + Microsoft Fixed Runtime 文档。
fn init_webview2_fixed_runtime() {
    let exe_dir = match std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        Some(d) => d,
        None => return,
    };
    // 候选目录名（与 CI 打包脚本一致）
    const CANDIDATES: &[&str] = &[
        "WebView2",
        "WebView2Runtime",
        "msedgewebview2",
        "Microsoft.WebView2.FixedVersionRuntime.128.0.2739.54.x64",
    ];
    for name in CANDIDATES {
        let cand = exe_dir.join(name);
        if cand.join("msedgewebview2.exe").exists()
            || cand.join("msedge.dll").exists()
            || cand.join("EBWebView").exists()
        {
            // WEBVIEW2_BROWSER_EXECUTABLE_FOLDER 需指向包含 msedgewebview2.exe 的目录
            std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &cand);
            eprintln!("[WebView2] 使用便携固定运行时: {}", cand.display());
            return;
        }
    }
    // 兜底：遍历 exe 同目录一级子目录，任意包含 msedgewebview2.exe 即视为运行时
    if let Ok(rd) = std::fs::read_dir(&exe_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("msedgewebview2.exe").exists() {
                std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &p);
                eprintln!("[WebView2] 使用便携固定运行时: {}", p.display());
                return;
            }
        }
    }
}

/// 检查 GitHub 最新 Release 版本
#[tauri::command]
fn check_for_updates(current_version: String) -> Result<updater::UpdateInfo, String> {
    updater::check_updates(&current_version)
}

/// 分块下载更新包
#[tauri::command]
fn download_update(
    app: AppHandle,
    url: String,
    file_name: Option<String>,
) -> Result<String, String> {
    updater::download_update(&app, &url, file_name)
}

/// 应用便携版覆盖更新并自动重启
#[tauri::command]
fn apply_portable_update(new_file_path: String) -> Result<(), String> {
    updater::apply_portable_update_and_restart(&new_file_path)
}

/// 使用系统默认浏览器打开 URL
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    updater::open_url_in_browser(&url)
}

/// 获取系统当前原生深浅色主题（Windows / macOS / Linux）
#[tauri::command]
fn get_system_theme(app: AppHandle) -> String {
    if let Some(win) = app.get_webview_window("main") {
        if let Ok(theme) = win.theme() {
            return match theme {
                tauri::Theme::Dark => "dark".to_string(),
                tauri::Theme::Light => "light".to_string(),
                _ => "light".to_string(),
            };
        }
    }
    "light".to_string()
}

/// 获取系统已安装字体列表
#[tauri::command]
fn list_system_fonts() -> Vec<system_fonts::SystemFontItem> {
    system_fonts::list_system_fonts()
}

pub fn run() {
    init_webview2_fixed_runtime();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            seed: AtomicU64::new(now_seed()),
        })
        .invoke_handler(tauri::generate_handler![
            render_preview,
            export_files,
            export_pdf,
            import_docx,
            import_document,
            list_presets,
            load_preset,
            save_preset,
            default_preset_dir,
            path_exists,
            image_dimensions,
            check_for_updates,
            download_update,
            apply_portable_update,
            open_url,
            get_system_theme,
            list_system_fonts
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::ThemeChanged(theme) = event {
                let theme_str = match theme {
                    tauri::Theme::Dark => "dark",
                    tauri::Theme::Light => "light",
                    _ => "light",
                };
                let _ = window.emit("system-theme-changed", theme_str);
            }
        })
        // 页面真正加载完成后再显示窗口：dev 冷启动时 Vite 还在编译模块，
        // 提前显示只会让用户对着白屏等（对齐官方 splashscreen 模式）。
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                if let Some(win) = webview.app_handle().get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("手写模拟器启动失败");
}

fn main() {
    run()
}
