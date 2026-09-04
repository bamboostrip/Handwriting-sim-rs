//! 软件版本检查与便携版自动更新模块（对齐 Python 原版 updater.py）。
//!
//! 提供 GitHub Releases 最新版本查询、语义化版本比对、
//! 默认浏览器直达、分块下载以及便携版进程安全覆盖重启功能。
//!
//! 版本查询多级容灾（不依赖 api.github.com 也可拿到完整信息）：
//! 1. GitHub REST API（信息最全）
//! 2. GitHub Releases Atom 订阅源 + expanded_assets 资产页（免 API 频次限制）
//! 3. 网页 302 重定向探测最新 Tag（最后兜底）

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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

/// 抓取 github.com 网页端点（Atom / expanded_assets）时使用的浏览器 UA
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 当前平台在 Release 资产命名中的标识（CI 产物命名：handwrite-sim-<token>[-webview2].zip）
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const PLATFORM_TOKEN: &str = "windows-x86_64";
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
const PLATFORM_TOKEN: &str = "windows-aarch64";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PLATFORM_TOKEN: &str = "linux-x86_64";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const PLATFORM_TOKEN: &str = "linux-aarch64";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const PLATFORM_TOKEN: &str = "macos-arm64";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const PLATFORM_TOKEN: &str = "macos-x86_64";
#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
)))]
const PLATFORM_TOKEN: &str = "";

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

/// 反转义常见 HTML 实体（&amp; &lt; &gt; &quot; &#39; &nbsp; 及数字实体）。
fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let semi = tail.find(';');
        let (entity, remainder) = match semi {
            Some(end) if end <= 10 => (&tail[..=end], &tail[end + 1..]),
            _ => {
                // 非实体（无分号或实体过长）：原样输出 & 后继续扫描
                out.push('&');
                rest = &tail[1..];
                continue;
            }
        };
        match entity {
            "&amp;" => out.push('&'),
            "&lt;" => out.push('<'),
            "&gt;" => out.push('>'),
            "&quot;" => out.push('"'),
            "&apos;" | "&#39;" => out.push('\''),
            "&nbsp;" => out.push(' '),
            _ => {
                // 数字实体：&#123; 或 &#x1F;
                if let Some(num) = entity.strip_prefix("&#").and_then(|e| e.strip_suffix(';')) {
                    let code = if let Some(hex) =
                        num.strip_prefix('x').or_else(|| num.strip_prefix('X'))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                    if let Some(ch) = code.and_then(char::from_u32) {
                        out.push(ch);
                    } else {
                        out.push_str(entity);
                    }
                } else {
                    out.push_str(entity);
                }
            }
        }
        rest = remainder;
    }
    out.push_str(rest);
    out
}

/// Release 说明展示规则：正文只保留第一个 `## ` 标题小节
/// （`## 更新内容` + CHANGELOG 该版本条目），其后的
/// `## 下载说明` / `## 为什么没有字体？` / `## Linux 运行依赖`
/// 属于每版重复的固定样板，全部截去。
fn cut_at_second_h2(text: &str, h2_prefix: &str) -> String {
    let first = match text.find(h2_prefix) {
        Some(i) => i,
        None => return text.trim().to_string(),
    };
    match text[first + h2_prefix.len()..].find(h2_prefix) {
        Some(j) => text[..first + h2_prefix.len() + j].trim().to_string(),
        None => text.trim().to_string(),
    }
}

/// 将 Atom 订阅源中转义后的 Release 正文 HTML 按规则转为可读纯文本：
/// h2/h3/h4 映射为 Markdown 标题、li 映射为 `- `、code 映射为反引号，
/// 其余标签剥离并按块级元素换行；再截去第二个 `<h2>` 起的固定样板。
fn html_release_body_to_text(html: &str) -> String {
    let kept = {
        let first = html.find("<h2");
        match first {
            None => html,
            Some(i) => match html[i + 3..].find("<h2") {
                Some(j) => &html[..i + 3 + j],
                None => html,
            },
        }
    };

    let mut out = String::with_capacity(kept.len());
    let mut rest = kept;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        let tag_src = &rest[lt + 1..lt + gt];
        let is_close = tag_src.starts_with('/');
        let name = tag_src
            .trim()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        let replacement: &str = match name.as_str() {
            "h2" if !is_close => "\n\n## ",
            "h3" if !is_close => "\n\n### ",
            "h4" if !is_close => "\n\n#### ",
            "li" if !is_close => "\n- ",
            "code" => "`",
            "br" => "\n",
            "p" | "div" | "ul" | "ol" | "table" | "tr" | "blockquote" | "pre" | "section" => "\n",
            _ => "",
        };
        out.push_str(replacement);
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);

    normalize_text_block(&out)
}

/// 压缩连续空行（3 个以上换行折叠为 2 个）、去除行尾空白，
/// 并把悬空的列表标记行（`-`）与其后内容行合并
/// （GitHub 会把 `<li>` 内容包裹在 `<p>` 中，标签转换后标记与内容会被换行拆开）。
fn normalize_text_block(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    let mut merged: Vec<String> = Vec::with_capacity(lines.len());
    let mut pending_marker = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "-" {
            pending_marker = true;
            continue;
        }
        if trimmed.is_empty() {
            if !pending_marker {
                merged.push(String::new());
            }
            continue;
        }
        if pending_marker {
            merged.push(format!("- {trimmed}"));
            pending_marker = false;
        } else {
            merged.push(trimmed.to_string());
        }
    }
    if pending_marker {
        merged.push("-".to_string());
    }

    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in merged {
        if line.is_empty() {
            blank += 1;
            if blank <= 1 {
                out.push('\n');
            }
        } else {
            blank = 0;
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// 对 API 路径返回的 Markdown 正文应用同一展示规则：
/// 保留 `## 更新内容` 到下一个 `## ` 标题之前。
fn trim_release_notes_markdown(md: &str) -> String {
    let text = md.trim();
    if text.starts_with("## ") {
        // 补前导换行，使首个标题也纳入 `\n## ` 匹配
        cut_at_second_h2(&format!("\n{text}"), "\n## ")
    } else {
        cut_at_second_h2(text, "\n## ")
    }
}

/// 从 Atom 订阅源 XML 解析第一个（最新的）Release 条目：
/// 返回 (tag, release 页面链接, 转义状态的正文 HTML)。
fn parse_latest_entry_from_atom(feed_xml: &str) -> Option<(String, String, String)> {
    let entry_start = feed_xml.find("<entry>")?;
    let entry_end = feed_xml[entry_start..]
        .find("</entry>")
        .map(|i| entry_start + i)
        .unwrap_or(feed_xml.len());
    let entry = &feed_xml[entry_start..entry_end];

    // 最新 Tag：优先取 alternate 链接 /releases/tag/<tag>，回退 <title>
    let mut tag = String::new();
    let mut link = String::new();
    let mut rest = entry;
    while let Some(href) = rest.find("href=\"") {
        let after = &rest[href + "href=\"".len()..];
        let Some(end) = after.find('"') else { break };
        let url = &after[..end];
        if url.contains("/releases/tag/") {
            let t = url.rsplit('/').next().unwrap_or("").to_string();
            if !t.is_empty() {
                tag = unescape_html(&t);
                link = unescape_html(url);
                break;
            }
        }
        rest = &after[end..];
    }
    if tag.is_empty() {
        let t_start = entry.find("<title>")? + "<title>".len();
        let t_end = entry[t_start..].find("</title>")? + t_start;
        tag = entry[t_start..t_end].trim().to_string();
        link = String::new();
    }

    let content = entry
        .find("<content")
        .and_then(|c| entry[c..].find('>').map(|g| c + g + 1))
        .and_then(|start| {
            entry[start..]
                .find("</content>")
                .map(|e| &entry[start..start + e])
        })
        .unwrap_or("")
        .to_string();

    Some((tag, link, content))
}

/// 解析 expanded_assets 资产页 HTML，提取所有二进制资产直链。
/// 规则：取 `href="/<owner>/<repo>/releases/download/<tag>/<文件名>"` 形式的链接，
/// 忽略源码包（/archive/refs/tags/）与重复项。
fn parse_asset_links(html: &str) -> Vec<(String, String)> {
    const NEEDLE: &str = "/releases/download/";
    let mut assets: Vec<(String, String)> = Vec::new();
    let mut rest = html;
    while let Some(p) = rest.find(NEEDLE) {
        let after = match rest[..p].rfind('"') {
            Some(q) => &rest[q + 1..],
            None => {
                rest = &rest[p + NEEDLE.len()..];
                continue;
            }
        };
        let Some(end) = after.find('"') else { break };
        let path = &after[..end];
        if path.starts_with('/') && path.contains(NEEDLE) {
            if let Some(name) = path.rsplit('/').next() {
                if !name.is_empty()
                    && (name.ends_with(".zip") || name.ends_with(".exe"))
                    && !assets.iter().any(|(n, _)| n == name)
                {
                    assets.push((name.to_string(), format!("https://github.com{path}")));
                }
            }
        }
        rest = &after[end..];
    }
    assets
}

/// 从资产列表中挑选当前平台的更新包：
/// 1. 平台匹配的裸 `.exe`（若发布时直接附带）
/// 2. 平台匹配的轻量 `.zip`（排除体积巨大的 webview2 变体——旧版的 `WebView2/`
///    目录在 exe 旁保留，轻量包也能继续使用固定运行时）
/// 3. 平台匹配的 `.zip`（含 webview2 变体，最后兜底）
/// 4. 仅 Windows：通用裸 `handwrite-sim.exe`（如手工附带的安装程序）
///
/// 无平台匹配资产时返回 None（前端回退浏览器手动下载）。
fn select_platform_asset(assets: &[(String, String, u64)]) -> Option<(String, String, u64)> {
    let matches_platform = |name: &str| PLATFORM_TOKEN.is_empty() || name.contains(PLATFORM_TOKEN);
    assets
        .iter()
        .find(|(name, _, _)| name.ends_with(".exe") && matches_platform(name))
        .or_else(|| {
            assets.iter().find(|(name, _, _)| {
                name.ends_with(".zip") && matches_platform(name) && !name.contains("webview2")
            })
        })
        .or_else(|| {
            assets
                .iter()
                .find(|(name, _, _)| name.ends_with(".zip") && matches_platform(name))
        })
        .or_else(|| {
            #[cfg(target_os = "windows")]
            {
                assets
                    .iter()
                    .find(|(name, _, _)| name.eq_ignore_ascii_case("handwrite-sim.exe"))
            }
            #[cfg(not(target_os = "windows"))]
            {
                None::<&(String, String, u64)>
            }
        })
        .cloned()
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

    let val: serde_json::Value = resp
        .json()
        .map_err(|e| format!("解析 Release 数据失败: {e}"))?;

    let tag_name = val["tag_name"].as_str().unwrap_or("").to_string();
    let version = clean_version(&tag_name).to_string();
    let title = val["name"].as_str().unwrap_or(&tag_name).to_string();
    let body = trim_release_notes_markdown(val["body"].as_str().unwrap_or("暂无更新说明。"));
    let html_url = val["html_url"]
        .as_str()
        .unwrap_or(GITHUB_RUST_REPO_URL)
        .to_string();

    let mut assets: Vec<(String, String, u64)> = Vec::new();
    if let Some(arr) = val["assets"].as_array() {
        for a in arr {
            let name = a["name"].as_str().unwrap_or("").to_string();
            let url = a["browser_download_url"].as_str().unwrap_or("").to_string();
            let size = a["size"].as_u64().unwrap_or(0);
            if !name.is_empty() && !url.is_empty() {
                assets.push((name, url, size));
            }
        }
    }
    let (asset_name, asset_url, asset_size) = select_platform_asset(&assets).unwrap_or_default();

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

/// 从 Releases Atom 订阅源获取最新 Release（github.com 域，无 API 频次限制）：
/// 正文为转义 HTML，按规则转纯文本并截去固定样板。
fn fetch_release_from_atom_feed(
    client: &reqwest::blocking::Client,
    repo_url: &str,
    current_version: &str,
) -> Result<UpdateInfo, String> {
    let resp = client
        .get(format!("{repo_url}/releases.atom"))
        .header("User-Agent", BROWSER_UA)
        .timeout(Duration::from_secs(10))
        .send()
        .map_err(|e| format!("Atom 订阅源请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Atom 订阅源响应异常: HTTP {}", resp.status()));
    }

    let xml = resp
        .text()
        .map_err(|e| format!("读取 Atom 订阅源失败: {e}"))?;
    let (tag_name, link, content_html) = parse_latest_entry_from_atom(&xml)
        .ok_or_else(|| "Atom 订阅源中未找到 Release 条目".to_string())?;

    let version = clean_version(&tag_name).to_string();
    let has_update = compare_versions(&version, current_version) == std::cmp::Ordering::Greater;

    let mut info = UpdateInfo {
        version,
        tag_name: tag_name.clone(),
        title: format!("手写模拟器 {tag_name}"),
        body: html_release_body_to_text(&unescape_html(&content_html)),
        html_url: if link.is_empty() {
            format!("{repo_url}/releases/tag/{tag_name}")
        } else {
            link
        },
        asset_name: String::new(),
        asset_url: String::new(),
        asset_size: 0,
        has_update,
    };

    // 尽力补全资产直链（失败不影响版本提示，仅无法自动更新）
    attach_expanded_assets(client, repo_url, &tag_name, &mut info);

    Ok(info)
}

/// 抓取 expanded_assets 资产页，把当前平台的更新包直链写入 info（尽力而为）。
fn attach_expanded_assets(
    client: &reqwest::blocking::Client,
    repo_url: &str,
    tag_name: &str,
    info: &mut UpdateInfo,
) {
    let Ok(resp) = client
        .get(format!("{repo_url}/releases/expanded_assets/{tag_name}"))
        .header("User-Agent", BROWSER_UA)
        .timeout(Duration::from_secs(10))
        .send()
    else {
        return;
    };
    if !resp.status().is_success() {
        return;
    }
    let Ok(html) = resp.text() else {
        return;
    };
    let assets: Vec<(String, String, u64)> = parse_asset_links(&html)
        .into_iter()
        .map(|(name, url)| (name, url, 0))
        .collect();
    if let Some((name, url, _)) = select_platform_asset(&assets) {
        info.asset_name = name;
        info.asset_url = url;
    }
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
        .header("User-Agent", BROWSER_UA)
        .timeout(Duration::from_secs(6))
        .send()
        .map_err(|e| format!("网页请求失败: {e}"))?;

    let final_url = resp.url().as_str().to_string();

    // 若重定向至 https://github.com/.../releases/tag/vX.Y.Z
    if let Some(pos) = final_url.find("/releases/tag/") {
        let tag_part = &final_url[pos + "/releases/tag/".len()..];
        let tag_name = tag_part
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(tag_part)
            .to_string();
        let version = clean_version(&tag_name).to_string();
        let has_update = compare_versions(&version, current_version) == std::cmp::Ordering::Greater;

        let mut info = UpdateInfo {
            version,
            tag_name: tag_name.to_string(),
            title: format!("手写模拟器 {tag_name}"),
            body: format!(
                "发现新版本发布！自动更新直链获取失败，请点击「浏览器下载」前往 GitHub Release 页面：\n\n{final_url}"
            ),
            html_url: final_url,
            asset_name: String::new(),
            asset_url: String::new(),
            asset_size: 0,
            has_update,
        };

        // 尽力补全资产直链：拿到直链即可恢复「立即自动更新」
        attach_expanded_assets(client, repo_url, &tag_name, &mut info);

        return Ok(info);
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
/// 2. 请求 Rust 仓库 Atom 订阅源 + expanded_assets 资产页（免 API 频次限制）
/// 3. 请求 Rust 仓库网页重定向（无 API 频次限制）
/// 4. 请求 Python 仓库 GitHub REST API
/// 5. 请求 Python 仓库网页重定向
/// 6. 兜底：判定为无可用新版本
pub fn check_updates(current_version: &str) -> Result<UpdateInfo, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;

    // 1. 尝试 Rust 仓库 REST API
    if let Ok(info) = fetch_release_from_url(&client, GITHUB_API_RUST_LATEST, current_version) {
        return Ok(info);
    }

    // 2. Rust 仓库 Atom 订阅源（github.com 域，无 API 频次限制）
    if let Ok(info) = fetch_release_from_atom_feed(&client, GITHUB_RUST_REPO_URL, current_version) {
        return Ok(info);
    }

    // 3. 网页无限制重定向检测：Rust 仓库
    if let Ok(info) =
        fetch_release_from_web_redirect(&client, GITHUB_RUST_REPO_URL, current_version)
    {
        return Ok(info);
    }

    // 4. 尝试 Python 仓库 REST API
    if let Ok(info) = fetch_release_from_url(&client, GITHUB_API_PYTHON_LATEST, current_version) {
        return Ok(info);
    }

    // 5. 网页无限制重定向检测：Python 仓库
    if let Ok(info) =
        fetch_release_from_web_redirect(&client, GITHUB_PYTHON_REPO_URL, current_version)
    {
        return Ok(info);
    }

    // 6. 兜底：若所有网络探测均无法获取新 release，但当前版本本身合法，判定为无可用新版本或给出友好提示
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
    // 便携包可达 28MB+，为慢速网络放宽整体超时
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
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

/// 解压更新 zip 到目标目录，并定位其中的主程序 exe。
/// CI 便携包结构为根级 `handwrite-sim.exe` + `presets/` + `backgrounds/` + `fonts/`；
/// 为兼容手工打包，找不到根级 exe 时递归搜索同名文件。
fn extract_update_zip(zip_path: &Path, extract_dir: &Path) -> Result<PathBuf, String> {
    if extract_dir.exists() {
        let _ = std::fs::remove_dir_all(extract_dir);
    }
    std::fs::create_dir_all(extract_dir).map_err(|e| format!("创建解压目录失败: {e}"))?;

    let file = File::open(zip_path).map_err(|e| format!("打开更新包失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取更新包失败: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("遍历更新包条目失败: {e}"))?;
        // enclosed_name 已拒绝携带 `..` 的路径穿越条目
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let dest = extract_dir.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| format!("创建目录失败: {e}"))?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
            }
            let mut out = File::create(&dest).map_err(|e| format!("解压文件失败: {e}"))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压写入失败: {e}"))?;
        }
    }

    let root_exe = extract_dir.join("handwrite-sim.exe");
    if root_exe.is_file() {
        return Ok(root_exe);
    }

    fn walk_for_exe(dir: &Path) -> Option<PathBuf> {
        let rd = std::fs::read_dir(dir).ok()?;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(found) = walk_for_exe(&p) {
                    return Some(found);
                }
            } else if p
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("handwrite-sim.exe"))
            {
                return Some(p);
            }
        }
        None
    }

    walk_for_exe(extract_dir).ok_or_else(|| "更新包中未找到 handwrite-sim.exe".to_string())
}

/// 生成 Windows 更新批处理脚本内容：
/// 等待旧进程退出 → 带重试地覆盖 exe（旧进程退出与文件锁释放存在竞态，
/// 固定 sleep 一次可能复制失败，重试最多 10 次、每次间隔约 1 秒）→
/// 清理下载文件与解压目录 → 重启新版本 → 自删脚本。
///
/// 全程无窗口约束：延时必须用 `wscript //B //Nologo` + `.vbs`（windows 子系统，
/// 永不分配控制台）。严禁用 `ping` / `timeout` / `choice` 等控制台程序做延时：
/// 父进程的 CREATE_NO_WINDOW 不会继承给孙进程，Win11 默认终端会为每个此类子进程
/// 弹一个新终端窗口，重试循环会把它放大成“循环弹窗”（见 test_updater_bat_uses_windowless_sleep）。
/// 其余命令（chcp/copy/del/rmdir/start/goto/if/set）均为 cmd 内部命令，不产生子进程。
fn build_updater_bat_content(
    new_exe: &Path,
    current_exe: &Path,
    downloaded_file: &Path,
    extract_dir: Option<&Path>,
    sleep_vbs: &Path,
    launcher_vbs: Option<&Path>,
) -> String {
    let cleanup_downloaded = format!(
        "if exist \"{}\" del /f /q \"{}\" >nul\r\n",
        downloaded_file.display(),
        downloaded_file.display()
    );
    let cleanup_extract = extract_dir
        .map(|d| {
            format!(
                "if exist \"{}\" rmdir /s /q \"{}\" >nul\r\n",
                d.display(),
                d.display()
            )
        })
        .unwrap_or_default();
    let sleep_cmd = format!(
        "wscript //B //Nologo \"{}\" >nul 2>&1\r\n",
        sleep_vbs.display()
    );
    let cleanup_sleep = format!(
        "if exist \"{}\" del /f /q \"{}\" >nul\r\n",
        sleep_vbs.display(),
        sleep_vbs.display()
    );
    let cleanup_launcher = launcher_vbs
        .map(|l| {
            format!(
                "if exist \"{}\" del /f /q \"{}\" >nul\r\n",
                l.display(),
                l.display()
            )
        })
        .unwrap_or_default();

    format!(
        "@echo off\r\n\
        chcp 65001 >nul\r\n\
        set /a tries=0\r\n\
        :copyloop\r\n\
        {sleep_cmd}\
        copy /y \"{}\" \"{}\" >nul 2>&1 && goto copied\r\n\
        set /a tries+=1\r\n\
        if %tries% geq 20 goto copyfailed\r\n\
        goto copyloop\r\n\
        :copyfailed\r\n\
        {}\
        {}\
        {cleanup_sleep}\
        {cleanup_launcher}\
        (goto) 2>nul & del \"%~f0\"\r\n\
        exit /b 1\r\n\
        :copied\r\n\
        {}\
        {}\
        {cleanup_sleep}\
        {cleanup_launcher}\
        start \"\" \"{}\"\r\n\
        (goto) 2>nul & del \"%~f0\"\r\n",
        new_exe.display(),
        current_exe.display(),
        cleanup_downloaded,
        cleanup_extract,
        cleanup_downloaded,
        cleanup_extract,
        current_exe.display(),
    )
}

/// Windows 便携版覆盖更新批处理脚本与重启。
/// 支持两类更新文件：裸 `.exe` 直接覆盖；`.zip` 先解压到临时目录再取其中 exe 覆盖。
pub fn apply_portable_update_and_restart(new_file_path: &str) -> Result<(), String> {
    let downloaded_path = Path::new(new_file_path);
    if !downloaded_path.exists() {
        return Err("更新包文件不存在".to_string());
    }

    let current_exe =
        std::env::current_exe().map_err(|e| format!("获取当前可执行文件路径失败: {e}"))?;

    let is_zip = downloaded_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let (new_exe, extract_dir) = if is_zip {
        let dir = std::env::temp_dir().join(format!("handwritesim_update_{}", std::process::id()));
        let exe = extract_update_zip(downloaded_path, &dir)?;
        (exe, Some(dir))
    } else {
        (downloaded_path.to_path_buf(), None)
    };
    let new_path = new_exe.as_path();

    let temp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let bat_file = temp_dir.join(format!("handwritesim_updater_{pid}.bat"));
    // 无窗口延时脚本（wscript 为 windows 子系统，不弹任何终端窗口）
    let sleep_vbs = temp_dir.join(format!("handwritesim_sleep_{pid}.vbs"));
    std::fs::write(&sleep_vbs, "WScript.Sleep 500\r\n")
        .map_err(|e| format!("生成更新延时脚本失败: {e}"))?;

    let launcher_vbs = temp_dir.join(format!("handwritesim_launcher_{pid}.vbs"));
    let bat_content = build_updater_bat_content(
        new_path,
        &current_exe,
        downloaded_path,
        extract_dir.as_deref(),
        &sleep_vbs,
        Some(&launcher_vbs),
    );

    std::fs::write(&bat_file, bat_content).map_err(|e| format!("生成更新批处理脚本失败: {e}"))?;

    #[cfg(target_os = "windows")]
    {
        let launcher_code = format!(
            "Set WshShell = CreateObject(\"WScript.Shell\")\r\nWshShell.Run \"cmd.exe /c \"\"\"\"{}\"\"\"\"\", 0, False\r\n",
            bat_file.display()
        );
        std::fs::write(&launcher_vbs, launcher_code)
            .map_err(|e| format!("生成更新启动脚本失败: {e}"))?;

        std::process::Command::new("wscript.exe")
            .args(["//B", "//Nologo", &launcher_vbs.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("启动更新批处理失败: {e}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let cleanup = extract_dir
            .map(|d| format!(" && rm -rf '{}'", d.display()))
            .unwrap_or_default();
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "sleep 1 && cp -f '{}' '{}' && rm -f '{}'{} && '{}' &",
                new_path.display(),
                current_exe.display(),
                downloaded_path.display(),
                cleanup,
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
        assert_eq!(
            compare_versions("0.3.1", "0.3.1"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_versions("v0.3.1", "0.3.1"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_versions("0.4.0", "0.3.1"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(compare_versions("0.3.1", "0.4.0"), std::cmp::Ordering::Less);
        assert_eq!(
            compare_versions("0.3.10", "0.3.2"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0", "0.9.9"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn test_unescape_html() {
        assert_eq!(unescape_html("a &amp; b"), "a & b");
        assert_eq!(unescape_html("&lt;h2&gt;x&lt;/h2&gt;"), "<h2>x</h2>");
        assert_eq!(unescape_html("&quot;q&quot; &#39;&#65;&#x42;"), "\"q\" 'AB");
        assert_eq!(unescape_html("无实体"), "无实体");
        // 无分号的残缺实体按字面保留
        assert_eq!(unescape_html("残缺 &amp 截断"), "残缺 &amp 截断");
    }

    #[test]
    fn test_trim_release_notes_markdown() {
        let md = "## 更新内容\n### Added\n- 新功能 A\n- 新功能 B\n\n## 下载说明（固定样板）\n\n| 文件名 |\n|---|\n\n## 为什么没有字体？\n版权说明";
        let trimmed = trim_release_notes_markdown(md);
        assert!(trimmed.contains("## 更新内容"));
        assert!(trimmed.contains("### Added"));
        assert!(trimmed.contains("新功能 B"));
        assert!(!trimmed.contains("下载说明"));
        assert!(!trimmed.contains("字体"));

        // 无 h2 结构的正文原样保留
        assert_eq!(
            trim_release_notes_markdown("暂无更新说明。"),
            "暂无更新说明。"
        );
    }

    #[test]
    fn test_html_release_body_to_text() {
        // 模拟 Atom <content> 反转义后的 HTML（v0.3.3 真实结构节选）
        let html = "<h2>更新内容</h2>\n<h3>Added</h3>\n<ul>\n<li>\n<p><strong>深色模式</strong>：支持<code>三态切换</code>。</p>\n<ul>\n<li>风格对齐墨绿国风暗色调。</li>\n</ul>\n</li>\n</ul>\n<h2>下载说明（固定样板）</h2>\n<table><tr><td>表格内容</td></tr></table>\n<h2>为什么没有字体？</h2>\n<p>版权说明</p>";
        let text = html_release_body_to_text(html);
        assert!(text.starts_with("## 更新内容"));
        assert!(text.contains("### Added"));
        assert!(text.contains("- 深色模式：支持`三态切换`。"));
        assert!(text.contains("- 风格对齐墨绿国风暗色调。"));
        // 第二个 <h2> 起的固定样板被截去
        assert!(!text.contains("下载说明"));
        assert!(!text.contains("表格内容"));
        assert!(!text.contains("版权说明"));
    }

    #[test]
    fn test_parse_latest_entry_from_atom() {
        let feed = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Release notes from Handwriting-sim-rs</title>
  <entry>
    <id>tag:github.com,2008:Repository/1/v0.3.3</id>
    <link rel="alternate" type="text/html" href="https://github.com/bamboostrip/Handwriting-sim-rs/releases/tag/v0.3.3"/>
    <title>v0.3.3</title>
    <content type="html">&lt;h2&gt;更新内容&lt;/h2&gt;&lt;ul&gt;&lt;li&gt;新功能&lt;/li&gt;&lt;/ul&gt;</content>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/1/v0.3.2</id>
    <link rel="alternate" type="text/html" href="https://github.com/bamboostrip/Handwriting-sim-rs/releases/tag/v0.3.2"/>
    <title>v0.3.2</title>
    <content type="html">&lt;h2&gt;旧版本&lt;/h2&gt;</content>
  </entry>
</feed>"#;
        let (tag, link, content) = parse_latest_entry_from_atom(feed).expect("应解析出条目");
        assert_eq!(tag, "v0.3.3");
        assert_eq!(
            link,
            "https://github.com/bamboostrip/Handwriting-sim-rs/releases/tag/v0.3.3"
        );
        assert_eq!(
            content,
            "&lt;h2&gt;更新内容&lt;/h2&gt;&lt;ul&gt;&lt;li&gt;新功能&lt;/li&gt;&lt;/ul&gt;"
        );
        assert_eq!(
            unescape_html(&content),
            "<h2>更新内容</h2><ul><li>新功能</li></ul>"
        );
    }

    #[test]
    fn test_parse_asset_links() {
        // 结构取自 expanded_assets 真实页面（省略内联 svg）
        let html = r#"<ul>
  <li><a href="/bamboostrip/Handwriting-sim-rs/releases/download/v0.3.3/handwrite-sim-linux-x86_64.zip"><span class="text-bold">handwrite-sim-linux-x86_64.zip</span></a><span>sha256:6b04…</span><span>28.5 MB</span></li>
  <li><a href="/bamboostrip/Handwriting-sim-rs/releases/download/v0.3.3/handwrite-sim-windows-x86_64.zip"><span class="text-bold">handwrite-sim-windows-x86_64.zip</span></a></li>
  <li><a href="/bamboostrip/Handwriting-sim-rs/releases/download/v0.3.3/handwrite-sim-windows-x86_64-webview2.zip"><span class="text-bold">handwrite-sim-windows-x86_64-webview2.zip</span></a></li>
  <li><a href="/bamboostrip/Handwriting-sim-rs/archive/refs/tags/v0.3.3.zip">Source code (zip)</a></li>
  <li><a href="/bamboostrip/Handwriting-sim-rs/archive/refs/tags/v0.3.3.tar.gz">Source code (tar.gz)</a></li>
</ul>"#;
        let assets = parse_asset_links(html);
        assert_eq!(assets.len(), 3);
        assert!(assets.iter().any(|(n, u)| n == "handwrite-sim-windows-x86_64.zip"
            && u == "https://github.com/bamboostrip/Handwriting-sim-rs/releases/download/v0.3.3/handwrite-sim-windows-x86_64.zip"));
        // 源码包不计入资产
        assert!(!assets.iter().any(|(_, u)| u.contains("/archive/")));
    }

    #[test]
    fn test_select_platform_asset() {
        let make = |names: &[&str]| -> Vec<(String, String, u64)> {
            names
                .iter()
                .map(|n| (n.to_string(), format!("https://example.com/{n}"), 1))
                .collect()
        };

        // 轻量 zip 优先于 webview2 变体
        let assets = make(&[
            "handwrite-sim-windows-x86_64-webview2.zip",
            "handwrite-sim-windows-x86_64.zip",
            "handwrite-sim-linux-x86_64.zip",
        ]);
        let token = if PLATFORM_TOKEN.is_empty() {
            "windows-x86_64"
        } else {
            PLATFORM_TOKEN
        };
        let assets: Vec<_> = assets
            .into_iter()
            .map(|(n, u, s)| (n.replace("windows-x86_64", token), u, s))
            .collect();
        let (name, _, _) = select_platform_asset(&assets).expect("应选中轻量包");
        assert_eq!(name, format!("handwrite-sim-{token}.zip"));

        // 平台命名的裸 exe 优先于 zip
        let assets = make(&[
            &format!("handwrite-sim-{token}.exe"),
            &format!("handwrite-sim-{token}.zip"),
        ]);
        let (name, _, _) = select_platform_asset(&assets).expect("应选中 exe");
        assert_eq!(name, format!("handwrite-sim-{token}.exe"));

        // Windows 下通用裸 handwrite-sim.exe 可被选中
        #[cfg(target_os = "windows")]
        {
            let assets = make(&["handwrite-sim.exe"]);
            let (name, _, _) = select_platform_asset(&assets).expect("应选中通用 exe");
            assert_eq!(name, "handwrite-sim.exe");
        }

        // 仅 webview2 变体时作为兜底选中
        let assets = make(&[&format!("handwrite-sim-{token}-webview2.zip")]);
        let (name, _, _) = select_platform_asset(&assets).expect("应兜底选中 webview2 包");
        assert!(name.contains("webview2"));

        // 无平台匹配资产（仅其他平台包）→ None，前端回退浏览器
        let assets = make(&["handwrite-sim-otherplatform.zip"]);
        if !PLATFORM_TOKEN.is_empty() {
            assert!(select_platform_asset(&assets).is_none());
        }
    }

    #[test]
    fn test_extract_update_zip() {
        let tmp = std::env::temp_dir().join(format!("handwritesim_ziptest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let zip_path = tmp.join("update.zip");
        let extract_dir = tmp.join("extracted");

        // 构造与 CI 便携包同构的 zip：根级 exe + 资源目录
        {
            let file = File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.add_directory("presets", opts).unwrap();
            writer.start_file("handwrite-sim.exe", opts).unwrap();
            writer.write_all(b"MZ fake exe payload").unwrap();
            writer.start_file("presets/default.json", opts).unwrap();
            writer.write_all(b"{}").unwrap();
            writer.finish().unwrap();
        }

        let exe = extract_update_zip(&zip_path, &extract_dir).expect("应解压并定位 exe");
        assert_eq!(exe, extract_dir.join("handwrite-sim.exe"));
        let content = std::fs::read(&exe).unwrap();
        assert_eq!(content, b"MZ fake exe payload");
        assert!(extract_dir.join("presets/default.json").is_file());

        // 嵌套一层的包也能定位 exe
        let nested_zip = tmp.join("nested.zip");
        {
            let file = File::create(&nested_zip).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("pkg/handwrite-sim.exe", opts).unwrap();
            writer.write_all(b"nested").unwrap();
            writer.finish().unwrap();
        }
        let exe = extract_update_zip(&nested_zip, &extract_dir).expect("应定位嵌套 exe");
        assert!(exe.ends_with("pkg\\handwrite-sim.exe") || exe.ends_with("pkg/handwrite-sim.exe"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 真实网络端到端探测（默认忽略，手动运行：cargo test -- --ignored）。
    /// 验证 api.github.com 不可达时的容灾路径：Atom 订阅源取说明 + expanded_assets 取直链。
    #[test]
    #[ignore = "需要真实网络访问 github.com，手动运行"]
    fn test_network_release_probe() {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();

        let info = fetch_release_from_atom_feed(&client, GITHUB_RUST_REPO_URL, "0.0.1")
            .expect("Atom 容灾路径应成功");
        assert!(
            info.has_update,
            "0.0.1 应判定有更新，实际版本 {}",
            info.version
        );
        assert!(!info.version.is_empty());
        assert!(
            info.body.contains("更新内容"),
            "正文应包含更新说明，实际：\n{}",
            info.body
        );
        assert!(!info.body.contains("下载说明"), "固定样板应被截去");
        if !PLATFORM_TOKEN.is_empty() {
            assert!(!info.asset_url.is_empty(), "应取到平台资产直链");
            assert!(
                info.asset_url.contains(PLATFORM_TOKEN),
                "资产应匹配平台：{}",
                info.asset_url
            );
            assert!(
                !info.asset_url.contains("webview2"),
                "应优先轻量包：{}",
                info.asset_url
            );
        }

        // 资产直链可用性：Range 探测应返回 206/200
        let resp = client
            .get(&info.asset_url)
            .header("User-Agent", BROWSER_UA)
            .header("Range", "bytes=0-0")
            .timeout(Duration::from_secs(15))
            .send()
            .expect("资产直链应可请求");
        assert!(
            resp.status().as_u16() == 206 || resp.status().is_success(),
            "资产直链响应异常: HTTP {}",
            resp.status()
        );
    }

    /// 更新批处理必须全程无窗口（回归测试，全平台运行）：
    /// Win11 默认终端下，任何控制台子进程（ping/timeout/choice 等）都会弹新终端窗口，
    /// 且父进程的 CREATE_NO_WINDOW 不会继承给孙进程；延时只能用 windows 子系统的
    /// wscript + .vbs（永远不分配控制台）。
    #[test]
    fn test_updater_bat_uses_windowless_sleep() {
        let tmp = std::env::temp_dir();
        let content = build_updater_bat_content(
            &tmp.join("new-handwrite-sim.exe"),
            &tmp.join("handwrite-sim.exe"),
            &tmp.join("update-handwrite-sim.exe"),
            None,
            &tmp.join("handwritesim_sleep_1234.vbs"),
            None,
        );
        assert!(
            !content.contains("ping "),
            "批处理不得用 ping 延时（会弹终端窗口），实际内容：\n{content}"
        );
        assert!(
            content.contains("wscript //B //Nologo"),
            "批处理延时必须走 wscript 无窗口脚本，实际内容：\n{content}"
        );
    }

    /// 真实执行生成的更新批处理脚本（Windows）：
    /// 验证等待→覆盖→清理下载文件→自删脚本全链路。
    /// 用系统 where.exe 作为合法 PE 模拟新旧 exe，避免 start 启动非法 PE 触发系统报错框。
    #[test]
    #[cfg(target_os = "windows")]
    fn test_windows_updater_bat_replaces_file() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let tmp = std::env::temp_dir().join(format!("handwritesim_battest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let app_dir = tmp.join("app");
        std::fs::create_dir_all(&app_dir).unwrap();
        let current_exe = app_dir.join("handwrite-sim.exe");
        let sys_exe = std::env::var("WINDIR")
            .map(|w| PathBuf::from(w).join(r"System32\where.exe"))
            .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\System32\where.exe"));
        std::fs::copy(&sys_exe, &current_exe).unwrap();

        // 新版本 = where.exe 尾部追加标记（仍是合法 PE，可被 start 启动）
        let new_exe = tmp.join("update-handwrite-sim.exe");
        std::fs::copy(&sys_exe, &new_exe).unwrap();
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&new_exe)
                .unwrap();
            f.write_all(b"NEW-VERSION-MARKER").unwrap();
        }

        let sleep_vbs = tmp.join("sleep.vbs");
        std::fs::write(&sleep_vbs, "WScript.Sleep 1000\r\n").unwrap();
        let bat_file = tmp.join("updater.bat");
        let content =
            build_updater_bat_content(&new_exe, &current_exe, &new_exe, None, &sleep_vbs, None);
        std::fs::write(&bat_file, content).unwrap();

        let status = std::process::Command::new("cmd.exe")
            .args(["/c", &bat_file.to_string_lossy()])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .expect("应能执行更新批处理");
        assert!(status.success(), "更新批处理应成功退出: {status}");

        let bytes = std::fs::read(&current_exe).unwrap();
        assert!(
            bytes.ends_with(b"NEW-VERSION-MARKER"),
            "旧 exe 应被新文件完整覆盖"
        );
        assert!(!new_exe.exists(), "下载的更新文件应被清理");
        assert!(!bat_file.exists(), "批处理脚本应自删");
        assert!(!sleep_vbs.exists(), "延时脚本应被清理");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
