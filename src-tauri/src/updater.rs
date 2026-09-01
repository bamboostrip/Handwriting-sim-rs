//! 软件版本检查与便携版自动更新模块（对齐 Python 原版 updater.py）。
//!
//! 提供 GitHub Releases 最新版本查询、语义化版本比对、
//! 默认浏览器直达、分块下载以及便携版进程安全覆盖重启功能。

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

#[allow(dead_code)]
pub const GITHUB_OWNER: &str = "bamboostrip";
#[allow(dead_code)]
pub const GITHUB_RUST_REPO: &str = "Handwriting-sim-rs";
#[allow(dead_code)]
pub const GITHUB_PYTHON_REPO: &str = "Handwriting-simulator";

pub const GITHUB_RUST_REPO_URL: &str = "https://github.com/bamboostrip/Handwriting-sim-rs";
#[allow(dead_code)]
pub const GITHUB_PYTHON_REPO_URL: &str = "https://github.com/bamboostrip/Handwriting-simulator";


pub const GITHUB_API_RUST_LATEST: &str =
    "https://api.github.com/repos/bamboostrip/Handwriting-sim-rs/releases/latest";
pub const GITHUB_API_PYTHON_LATEST: &str =
    "https://api.github.com/repos/bamboostrip/Handwriting-simulator/releases/latest";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub title: String,
    pub body: String,
    pub html_url: String,
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub has_update: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressPayload {
    pub received: u64,
    pub total: u64,
    pub percent: f64,
}

/// 清理版本号字符串，去除前导 'v'、'V' 及首尾空白。
pub fn clean_version(v: &str) -> &str {
    let s = v.trim();
    if let Some(stripped) = s.strip_prefix('v').or_else(|| s.strip_prefix('V')) {
        stripped.trim()
    } else {
        s
    }
}

/// 解析版本号为数字序列（如 "0.3.1" -> [0, 3, 1]）。
pub fn parse_version_tuple(v: &str) -> Vec<u32> {
    let cleaned = clean_version(v);
    cleaned
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

/// 比较两个版本号。
pub fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    let t1 = parse_version_tuple(v1);
    let t2 = parse_version_tuple(v2);

    let max_len = t1.len().max(t2.len());
    for i in 0..max_len {
        let n1 = t1.get(i).copied().unwrap_or(0);
        let n2 = t2.get(i).copied().unwrap_or(0);
        match n1.cmp(&n2) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// 查询指定 GitHub Releases API 接口
fn fetch_release_from_url(
    client: &reqwest::blocking::Client,
    api_url: &str,
    current_version: &str,
) -> Result<UpdateInfo, String> {
    let resp = client
        .get(api_url)
        .header(
            "User-Agent",
            format!("HandwritingSimulator/{current_version} (Windows; Rust; Tauri)"),
        )
        .header("Accept", "application/vnd.github.v3+json")
        .timeout(Duration::from_secs(6))
        .send()
        .map_err(|e| format!("网络请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API 响应异常: HTTP {}", resp.status()));
    }

    let val: serde_json::Value = resp.json().map_err(|e| format!("解析 Release 数据失败: {e}"))?;

    let tag_name = val["tag_name"].as_str().unwrap_or("").to_string();
    let version = clean_version(&tag_name).to_string();
    let title = val["name"]
        .as_str()
        .unwrap_or(&tag_name)
        .to_string();
    let body = val["body"]
        .as_str()
        .unwrap_or("暂无更新说明。")
        .to_string();
    let html_url = val["html_url"]
        .as_str()
        .unwrap_or(GITHUB_RUST_REPO_URL)
        .to_string();

    let mut asset_name = String::new();
    let mut asset_url = String::new();
    let mut asset_size = 0u64;

    if let Some(assets) = val["assets"].as_array() {
        for a in assets {
            let name = a["name"].as_str().unwrap_or("");
            if name.ends_with(".exe") {
                asset_name = name.to_string();
                asset_url = a["browser_download_url"].as_str().unwrap_or("").to_string();
                asset_size = a["size"].as_u64().unwrap_or(0);
                break;
            } else if name.ends_with(".zip") && asset_url.is_empty() {
                asset_name = name.to_string();
                asset_url = a["browser_download_url"].as_str().unwrap_or("").to_string();
                asset_size = a["size"].as_u64().unwrap_or(0);
            }
        }
    }

    let has_update = compare_versions(&version, current_version) == std::cmp::Ordering::Greater;

    Ok(UpdateInfo {
        version,
        tag_name,
        title,
        body,
        html_url,
        asset_name,
        asset_url,
        asset_size,
        has_update,
    })
}

/// 从 GitHub 网页端 302 重定向获取最新 Release Tag（无 API 60次/小时频次限制）
fn fetch_release_from_web_redirect(
    client: &reqwest::blocking::Client,
    repo_url: &str,
    current_version: &str,
) -> Result<UpdateInfo, String> {
    let release_url = format!("{repo_url}/releases/latest");
    let resp = client
        .get(&release_url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(6))
        .send()
        .map_err(|e| format!("网页请求失败: {e}"))?;

    let final_url = resp.url().as_str().to_string();

    // 若重定向至 https://github.com/.../releases/tag/vX.Y.Z
    if let Some(pos) = final_url.find("/releases/tag/") {
        let tag_part = &final_url[pos + "/releases/tag/".len()..];
        let tag_name = tag_part.split(['/', '?', '#']).next().unwrap_or(tag_part);
        let version = clean_version(tag_name).to_string();
        let has_update = compare_versions(&version, current_version) == std::cmp::Ordering::Greater;

        return Ok(UpdateInfo {
            version,
            tag_name: tag_name.to_string(),
            title: format!("手写模拟器 {tag_name}"),
            body: format!("发现新版本发布！请点击「浏览器下载」前往 GitHub Release 查看详细说明与安装包：\n\n{final_url}"),
            html_url: final_url,
            asset_name: String::new(),
            asset_url: String::new(),
            asset_size: 0,
            has_update,
        });
    }

    // 若仓库暂未发布任何 Release（返回 404 或仍在 releases 首页），认定当前版本即为最新
    if resp.status() == reqwest::StatusCode::NOT_FOUND || final_url.ends_with("/releases") {
        return Ok(UpdateInfo {
            version: current_version.to_string(),
            tag_name: format!("v{current_version}"),
            title: format!("手写模拟器 v{current_version}"),
            body: "当前仓库暂未发布新 Release，当前已是最新版本。".to_string(),
            html_url: format!("{repo_url}/releases"),
            asset_name: String::new(),
            asset_url: String::new(),
            asset_size: 0,
            has_update: false,
        });
    }

    Err("无法从网页获取 Release 标签".to_string())
}

/// 检查更新：多级容灾策略
/// 1. 请求 Rust 仓库 GitHub REST API
/// 2. 请求 Python 仓库 GitHub REST API
/// 3. 若 API 受限 (403) 或 404，请求 Rust 仓库网页重定向（无 API 频次限制）
/// 4. 请求 Python 仓库网页重定向
pub fn check_updates(current_version: &str) -> Result<UpdateInfo, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;

    // 1. 尝试 Rust 仓库 REST API
    if let Ok(info) = fetch_release_from_url(&client, GITHUB_API_RUST_LATEST, current_version) {
        return Ok(info);
    }

    // 2. 尝试 Python 仓库 REST API
    if let Ok(info) = fetch_release_from_url(&client, GITHUB_API_PYTHON_LATEST, current_version) {
        return Ok(info);
    }

    // 3. 网页无限制重定向检测：Rust 仓库
    if let Ok(info) = fetch_release_from_web_redirect(&client, GITHUB_RUST_REPO_URL, current_version) {
        return Ok(info);
    }

    // 4. 网页无限制重定向检测：Python 仓库
    if let Ok(info) = fetch_release_from_web_redirect(&client, GITHUB_PYTHON_REPO_URL, current_version) {
        return Ok(info);
    }

    // 5. 兜底：若所有网络探测均无法获取新 release，但当前版本本身合法，判定为无可用新版本或给出友好提示
    Ok(UpdateInfo {
        version: current_version.to_string(),
        tag_name: format!("v{current_version}"),
        title: format!("手写模拟器 v{current_version}"),
        body: "已是最新版本，暂未检测到更新。".to_string(),
        html_url: GITHUB_RUST_REPO_URL.to_string(),
        asset_name: String::new(),
        asset_url: String::new(),
        asset_size: 0,
        has_update: false,
    })
}


/// 分块下载更新文件，并通过 Tauri Event 发送下载进度
pub fn download_update(
    app: &AppHandle,
    url: &str,
    file_name: Option<String>,
) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let mut resp = client
        .get(url)
        .header("User-Agent", "HandwritingSimulator-Updater (Windows; Rust)")
        .send()
        .map_err(|e| format!("发起下载请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("下载响应异常: HTTP {}", resp.status()));
    }

    let total_size = resp.content_length().unwrap_or(0);

    let temp_dir = std::env::temp_dir();
    let name = file_name.unwrap_or_else(|| "handwrite-sim-update.exe".to_string());
    let dest_path = temp_dir.join(&name);
    let download_temp = temp_dir.join(format!("{name}.download"));

    let mut file = File::create(&download_temp).map_err(|e| format!("创建临时文件失败: {e}"))?;

    let mut buffer = [0u8; 65536];
    let mut received = 0u64;

    loop {
        let bytes_read = resp
            .read(&mut buffer)
            .map_err(|e| format!("下载读取中断: {e}"))?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read])
            .map_err(|e| format!("写入文件失败: {e}"))?;
        received += bytes_read as u64;

        let percent = if total_size > 0 {
            (received as f64 / total_size as f64) * 100.0
        } else {
            0.0
        };

        let _ = app.emit(
            "update-download-progress",
            DownloadProgressPayload {
                received,
                total: total_size,
                percent,
            },
        );
    }

    file.flush().map_err(|e| format!("保存文件失败: {e}"))?;
    drop(file);

    if dest_path.exists() {
        let _ = std::fs::remove_file(&dest_path);
    }
    std::fs::rename(&download_temp, &dest_path).map_err(|e| format!("移动文件失败: {e}"))?;

    Ok(dest_path.to_string_lossy().into_owned())
}

/// Windows 便携版覆盖更新批处理脚本与重启
pub fn apply_portable_update_and_restart(new_file_path: &str) -> Result<(), String> {
    let new_path = Path::new(new_file_path);
    if !new_path.exists() {
        return Err("更新包文件不存在".to_string());
    }

    let current_exe = std::env::current_exe().map_err(|e| format!("获取当前可执行文件路径失败: {e}"))?;

    let temp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let bat_file = temp_dir.join(format!("handwritesim_updater_{pid}.bat"));

    let bat_content = format!(
        "@echo off\r\n\
        chcp 65001 >nul\r\n\
        ping 127.0.0.1 -n 2 >nul\r\n\
        copy /y \"{}\" \"{}\" >nul\r\n\
        if exist \"{}\" del /f /q \"{}\" >nul\r\n\
        start \"\" \"{}\"\r\n\
        (goto) 2>nul & del \"%~f0\"\r\n",
        new_path.display(),
        current_exe.display(),
        new_path.display(),
        new_path.display(),
        current_exe.display(),
    );

    std::fs::write(&bat_file, bat_content).map_err(|e| format!("生成更新批处理脚本失败: {e}"))?;

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;

        std::process::Command::new("cmd.exe")
            .args(["/c", &bat_file.to_string_lossy()])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("启动更新批处理失败: {e}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "sleep 1 && cp -f '{}' '{}' && '{}' &",
                new_path.display(),
                current_exe.display(),
                current_exe.display()
            ))
            .spawn()
            .map_err(|e| format!("启动更新脚本失败: {e}"))?;
    }

    std::process::exit(0);
}

/// 系统默认浏览器打开 URL（无黑框弹窗）
pub fn open_url_in_browser(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("URL 不能为空".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {e}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {e}"))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_compare() {
        assert_eq!(compare_versions("0.3.1", "0.3.1"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("v0.3.1", "0.3.1"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("0.4.0", "0.3.1"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("0.3.1", "0.4.0"), std::cmp::Ordering::Less);
        assert_eq!(compare_versions("0.3.10", "0.3.2"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "0.9.9"), std::cmp::Ordering::Greater);
    }
}
