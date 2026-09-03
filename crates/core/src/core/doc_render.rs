//! 把 PDF / DOCX 渲染成「打印预览」图片，用作手写底图，并自动识别标记的手写区域。
//!
//! 对应 Python 版 `core/doc_render.py`：
//! - PDF 用 pdfium 栅格化（`pdfium-render` 绑定；运行时需要 `pdfium.dll`，
//!   放在 exe 旁或系统 PATH 中。缺失时给出明确提示）
//! - DOCX 的忠实排版需要本机排版引擎：优先借助 Microsoft Word（COM 自动化，
//!   仅 Windows），其次 LibreOffice（`soffice --headless`），转成 PDF 后
//!   再走同一条栅格化链路。都没有时给出明确的安装提示。
//! - 自动区域检测：
//!   1. 图像高亮底色检测（Word 标准黄色/绿色/青色/粉色等高亮矩形区域）
//!   2. 文本占位符标签检测（`{{...}}` 与 `【...】`）
//!   3. 自动擦除原图上的高亮色块与占位文字为纯白底色，返回提取的 TextRegion 列表。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::core::models::TextRegion;

/// 文档渲染错误。
#[derive(Debug, thiserror::Error)]
pub enum DocRenderError {
    #[error("不支持的文档类型：{0}（支持 .pdf / .docx）")]
    UnsupportedExtension(String),
    #[error("PDF 没有可渲染的页面：{0}")]
    NoPages(String),
    #[error(
        "无法把 DOCX 转成打印预览：需要本机安装 Microsoft Word 或 LibreOffice。\n\
         也可以先在 Word 里把文档另存为 PDF，再直接导入 PDF。"
    )]
    DocxConversionUnavailable,
    #[error("pdfium 加载失败：{0}（请把 pdfium.dll 放到程序目录，或安装后加入 PATH）")]
    PdfiumUnavailable(String),
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for DocRenderError {
    fn from(e: std::io::Error) -> Self {
        DocRenderError::Other(e.to_string())
    }
}

/// 像素包围盒。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundingBox {
    pub min_x: u32,
    pub min_y: u32,
    pub max_x: u32,
    pub max_y: u32,
    pub highlight: Option<String>,
}

impl BoundingBox {
    pub fn new(min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
            highlight: None,
        }
    }

    pub fn with_highlight(min_x: u32, min_y: u32, max_x: u32, max_y: u32, highlight: impl Into<String>) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
            highlight: Some(highlight.into()),
        }
    }

    pub fn width(&self) -> u32 {
        self.max_x.saturating_sub(self.min_x) + 1
    }

    pub fn height(&self) -> u32 {
        self.max_y.saturating_sub(self.min_y) + 1
    }

    pub fn union(&self, other: &BoundingBox) -> BoundingBox {
        BoundingBox {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
            highlight: self.highlight.clone().or_else(|| other.highlight.clone()),
        }
    }

    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

/// 将 RGB 高亮颜色分类为标准颜色名称（如 "yellow", "green", "cyan", "magenta", "pink", "red", "blue" 等）。
pub fn classify_highlight_color(r: u8, g: u8, b: u8) -> &'static str {
    let (rf, gf, bf) = (r as f32, g as f32, b as f32);
    if rf > 160.0 && gf > 160.0 && bf < 140.0 {
        "yellow"
    } else if gf > 150.0 && bf > 150.0 && rf < 150.0 {
        "cyan"
    } else if rf > 180.0 && bf > 140.0 && gf < 170.0 {
        if gf < 100.0 && bf > 180.0 {
            "magenta"
        } else {
            "pink"
        }
    } else if gf > rf && gf > bf {
        "green"
    } else if bf > rf && bf > gf {
        if gf > 140.0 {
            "cyan"
        } else {
            "blue"
        }
    } else if rf > gf && rf > bf {
        if bf > 100.0 {
            "pink"
        } else {
            "red"
        }
    } else {
        "yellow"
    }
}

/// 判断像素是否属于高亮底色（如 Word 标准黄色、绿色、青色、品红、粉红、蓝色、红色等浅色/高饱和度底色）。
pub fn is_highlight_pixel(r: u8, g: u8, b: u8) -> bool {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let diff = max - min;
    // 灰度差必须足够大（排除黑白灰背景及文字抗锯齿）
    if diff < 30 {
        return false;
    }
    // 亮度必须足够（高亮是浅色底色，排除深色文字）
    if max < 90 {
        return false;
    }
    // 饱和度 diff / max >= 0.20
    let sat = diff as f32 / max as f32;
    if sat < 0.20 {
        return false;
    }
    true
}

/// 检测图像中的高亮区域包围盒。
pub fn detect_highlight_boxes(img: &image::RgbImage) -> Vec<BoundingBox> {
    let width = img.width();
    let height = img.height();
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let mut visited = vec![false; (width * height) as usize];
    let mut raw_boxes = Vec::new();

    // 8-邻域连通性
    let dx = [-1, 0, 1, -1, 1, -1, 0, 1];
    let dy = [-1, -1, -1, 0, 0, 1, 1, 1];

    let mut queue = Vec::new();

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if visited[idx] {
                continue;
            }
            let pixel = img.get_pixel(x, y);
            if !is_highlight_pixel(pixel[0], pixel[1], pixel[2]) {
                continue;
            }

            // BFS 收集连通块
            visited[idx] = true;
            queue.clear();
            queue.push((x, y));

            let mut min_x = x;
            let mut max_x = x;
            let mut min_y = y;
            let mut max_y = y;
            let mut count = 0usize;
            let mut sum_r: u64 = pixel[0] as u64;
            let mut sum_g: u64 = pixel[1] as u64;
            let mut sum_b: u64 = pixel[2] as u64;

            let mut head = 0;
            while head < queue.len() {
                let (cx, cy) = queue[head];
                head += 1;
                count += 1;

                min_x = min_x.min(cx);
                max_x = max_x.max(cx);
                min_y = min_y.min(cy);
                max_y = max_y.max(cy);

                for dir in 0..8 {
                    let nx = cx as i32 + dx[dir];
                    let ny = cy as i32 + dy[dir];
                    if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                        let n_idx = (ny as u32 * width + nx as u32) as usize;
                        if !visited[n_idx] {
                            let np = img.get_pixel(nx as u32, ny as u32);
                            if is_highlight_pixel(np[0], np[1], np[2]) {
                                visited[n_idx] = true;
                                queue.push((nx as u32, ny as u32));
                                sum_r += np[0] as u64;
                                sum_g += np[1] as u64;
                                sum_b += np[2] as u64;
                            }
                        }
                    }
                }
            }

            let bw = max_x - min_x + 1;
            let bh = max_y - min_y + 1;

            // 过滤噪声：最小宽度 >= 15，最小高度 >= 8，像素数 >= 30
            if bw >= 15 && bh >= 8 && count >= 30 {
                let avg_r = (sum_r / count as u64) as u8;
                let avg_g = (sum_g / count as u64) as u8;
                let avg_b = (sum_b / count as u64) as u8;
                let color_name = classify_highlight_color(avg_r, avg_g, avg_b);
                raw_boxes.push(BoundingBox {
                    min_x,
                    min_y,
                    max_x,
                    max_y,
                    highlight: Some(color_name.to_string()),
                });
            }
        }
    }

    merge_close_boxes(raw_boxes)
}

fn should_merge_boxes(a: &BoundingBox, b: &BoundingBox) -> bool {
    // 0. 高亮颜色不同不合并
    if a.highlight != b.highlight {
        return false;
    }

    // 1. 直接相交或包含
    if a.intersects(b) {
        return true;
    }

    // 2. 同行相邻且水平间隙较小 (gap <= 20 像素，垂直重叠超过较小高度的 40%)
    let overlap_y = (a.max_y.min(b.max_y) as i32) - (a.min_y.max(b.min_y) as i32) + 1;
    let min_h = a.height().min(b.height()) as i32;
    if overlap_y > 0 && overlap_y >= (min_h * 2 / 5) {
        let gap_x = if a.max_x < b.min_x {
            b.min_x - a.max_x
        } else {
            a.min_x.saturating_sub(b.max_x)
        };
        if gap_x <= 20 {
            return true;
        }
    }

    // 3. 多行段落垂直连续段 (同色，水平重叠 >= 40% 较小宽度，垂直间隙 gap_y <= min_h * 3 / 2)
    //    注意用两框中较小的高度（行框高度）做基准：合并是迭代进行的，current 已是
    //    增长后的累计大框，若按 max_h 计算阈值，串起来的框会吞并下方任意远的内容
    //    （包括两段高亮之间未高亮的标题行）。
    let overlap_x = (a.max_x.min(b.max_x) as i32) - (a.min_x.max(b.min_x) as i32) + 1;
    let min_w = a.width().min(b.width()) as i32;
    if overlap_x > 0 && overlap_x >= (min_w * 2 / 5) {
        let gap_y = if a.max_y < b.min_y {
            b.min_y - a.max_y
        } else {
            a.min_y.saturating_sub(b.max_y)
        };
        let min_h = a.height().min(b.height()) as i32;
        if (gap_y as i32) <= (min_h * 3 / 2) {
            return true;
        }
    }

    false
}

/// 合并相邻或重叠的高亮矩形框。
pub fn merge_close_boxes(mut boxes: Vec<BoundingBox>) -> Vec<BoundingBox> {
    if boxes.len() <= 1 {
        return boxes;
    }

    let mut changed = true;
    while changed {
        changed = false;
        let mut next = Vec::new();
        let mut merged = vec![false; boxes.len()];

        for i in 0..boxes.len() {
            if merged[i] {
                continue;
            }
            let mut current = boxes[i].clone();
            for j in (i + 1)..boxes.len() {
                if merged[j] {
                    continue;
                }
                if should_merge_boxes(&current, &boxes[j]) {
                    current = current.union(&boxes[j]);
                    merged[j] = true;
                    changed = true;
                }
            }
            next.push(current);
        }
        boxes = next;
    }

    boxes
}

/// 在图像上将高亮矩形区域及残留的高亮像素抹白为纯白 `#FFFFFF`。
pub fn erase_highlight_boxes(img: &mut image::RgbImage, boxes: &[BoundingBox]) {
    let width = img.width();
    let height = img.height();

    // 1. 将检测到的包围盒矩形内全部置为纯白
    for b in boxes {
        let x0 = b.min_x.min(width.saturating_sub(1));
        let x1 = b.max_x.min(width.saturating_sub(1));
        let y0 = b.min_y.min(height.saturating_sub(1));
        let y1 = b.max_y.min(height.saturating_sub(1));

        for y in y0..=y1 {
            for x in x0..=x1 {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
    }

    // 2. 将整页中任何残留的高亮颜色像素也置为纯白（消除边缘溢色）
    for y in 0..height {
        for x in 0..width {
            let p = img.get_pixel(x, y);
            if is_highlight_pixel(p[0], p[1], p[2]) {
                img.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
    }
}

/// 匹配到的文本标签结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagMatch {
    pub start_char_idx: usize,
    pub end_char_idx: usize,
    pub inner_text: String,
}

/// 在字符流中扫描 `{{...}}` 或 `【...】` 占位标签。
pub fn scan_text_tags(chars: &[char]) -> Vec<TagMatch> {
    let mut matches = Vec::new();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // 匹配 {{...}}
        if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
            let mut j = i + 2;
            let mut found = false;
            while j + 1 < len {
                if chars[j] == '}' && chars[j + 1] == '}' {
                    let inner: String = chars[(i + 2)..j].iter().collect();
                    matches.push(TagMatch {
                        start_char_idx: i,
                        end_char_idx: j + 1,
                        inner_text: inner.trim().to_string(),
                    });
                    i = j + 2;
                    found = true;
                    break;
                } else if (chars[j] == '{' && chars[j + 1] == '{') || chars[j] == '\n' || chars[j] == '\r' {
                    i = j;
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                i += 1;
            }
        }
        // 匹配 【...】
        else if chars[i] == '【' {
            let mut j = i + 1;
            let mut found = false;
            while j < len {
                if chars[j] == '】' {
                    let inner: String = chars[(i + 1)..j].iter().collect();
                    matches.push(TagMatch {
                        start_char_idx: i,
                        end_char_idx: j,
                        inner_text: inner.trim().to_string(),
                    });
                    i = j + 1;
                    found = true;
                    break;
                } else if chars[j] == '【' || chars[j] == '\n' || chars[j] == '\r' {
                    i = j;
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    matches
}

/// PDF 提取的单个字符对象及其在页面像素空间的位置与字号。
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedChar {
    pub ch: char,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub font_size_pt: f32,
    /// 紧包围盒（tight bounds）的字符高度（点）。全角字符的紧包围盒高 ≈ 字号，
    /// 用于校准 scaled_font_size 对嵌入字体矩阵的偏差（如 Word 中文 PDF 返回
    /// 值偏大 ~6%，会让原文恰好占满行宽的文本在区域内放不下而整行折行）。
    pub glyph_h_pt: f32,
}

/// 是否为全角（CJK 表意文字 / 假名 / 谚文）字符：其紧包围盒高度 ≈ 字号，
/// 可用于字号校准。标点与西文不包括在内（紧包围盒远小于字号）。
fn is_full_width_char(ch: char) -> bool {
    let c = ch as u32;
    // CJK 统一表意文字及扩展 A / B..
    (0x3400..=0x4DBF).contains(&c)
        || (0x4E00..=0x9FFF).contains(&c)
        || (0x20000..=0x2FA1F).contains(&c)
        // CJK 兼容表意文字
        || (0xF900..=0xFAFF).contains(&c)
        // 平假名 / 片假名
        || (0x3041..=0x30FF).contains(&c)
        // 谚文音节
        || (0xAC00..=0xD7A3).contains(&c)
}

/// 从匹配字符解析字号（像素）。
///
/// `scaled_font_size` 对嵌入非标准 FontMatrix 的字体（如 Word 导出的中文
/// PDF）会整体偏大约 5%~10%；存在全角字符时用其紧包围盒高度（≈字号的
/// 0.95~0.97 倍）做上限校准，取二者较小值——宁可略小也不放不下。
fn resolve_font_size_px(chars: &[&ExtractedChar], avg_scaled_pt: f32, scale: f32) -> i32 {
    let scaled_px = if avg_scaled_pt > 0.0 {
        (avg_scaled_pt * scale).round() as i32
    } else {
        0
    };
    let fw_glyphs: Vec<f32> = chars
        .iter()
        .filter(|c| is_full_width_char(c.ch) && c.glyph_h_pt > 0.0)
        .map(|c| c.glyph_h_pt)
        .collect();
    if !fw_glyphs.is_empty() {
        // 取最大值避免标点/小字形拉低
        let glyph_px = (fw_glyphs.iter().cloned().fold(0.0_f32, f32::max) * scale).round() as i32;
        if scaled_px > 0 {
            return scaled_px.min(glyph_px);
        }
        return glyph_px;
    }
    scaled_px
}

/// 清理提取文本中可能包含的模板标签语法（如 `{{...}}`、`【...】`、`{{手写:...}}` 等）。
pub fn strip_tag_syntax(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 1. 如果整体被 {{ ... }} 包裹
    if let Some(inner) = trimmed.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
        let inner = inner.trim();
        if let Some(pos) = inner.find(':').or_else(|| inner.find('：')) {
            let colon_len = if inner.as_bytes()[pos] == b':' { 1 } else { '：'.len_utf8() };
            return inner[pos + colon_len..].trim().to_string();
        }
        return inner.to_string();
    }

    // 2. 如果整体被 【 ... 】 包裹
    if let Some(inner) = trimmed.strip_prefix('【').and_then(|s| s.strip_suffix('】')) {
        let inner = inner.trim();
        if let Some(pos) = inner.find(':').or_else(|| inner.find('：')) {
            let colon_len = if inner.as_bytes()[pos] == b':' { 1 } else { '：'.len_utf8() };
            return inner[pos + colon_len..].trim().to_string();
        }
        return inner.to_string();
    }

    // 3. 扫描并替换内部可能存在的内联标签
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut result = String::new();
    let mut i = 0;
    let mut replaced_tag = false;

    while i < len {
        if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
            let mut j = i + 2;
            let mut found = false;
            while j + 1 < len {
                if chars[j] == '}' && chars[j + 1] == '}' {
                    let inner: String = chars[(i + 2)..j].iter().collect();
                    let inner_str = inner.trim();
                    let body = if let Some(pos) = inner_str.find(':').or_else(|| inner_str.find('：')) {
                        let colon_len = if inner_str.as_bytes()[pos] == b':' { 1 } else { '：'.len_utf8() };
                        inner_str[pos + colon_len..].trim()
                    } else {
                        inner_str
                    };
                    result.push_str(body);
                    i = j + 2;
                    found = true;
                    replaced_tag = true;
                    break;
                }
                j += 1;
            }
            if !found {
                result.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '【' {
            let mut j = i + 1;
            let mut found = false;
            while j < len {
                if chars[j] == '】' {
                    let inner: String = chars[(i + 1)..j].iter().collect();
                    let inner_str = inner.trim();
                    let body = if let Some(pos) = inner_str.find(':').or_else(|| inner_str.find('：')) {
                        let colon_len = if inner_str.as_bytes()[pos] == b':' { 1 } else { '：'.len_utf8() };
                        inner_str[pos + colon_len..].trim()
                    } else {
                        inner_str
                    };
                    result.push_str(body);
                    i = j + 1;
                    found = true;
                    replaced_tag = true;
                    break;
                }
                j += 1;
            }
            if !found {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    if replaced_tag {
        result.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// 从 PDF 页面提取所有文字字符及其包围盒（像素坐标）与字号。
pub fn extract_pdf_page_chars(
    page: &pdfium_render::prelude::PdfPage,
    dpi: u32,
) -> Vec<ExtractedChar> {
    let text_page = match page.text() {
        Ok(tp) => tp,
        Err(_) => return Vec::new(),
    };

    let scale = dpi as f32 / 72.0;
    let page_height_pt = page.height().value;
    let mut chars = Vec::new();

    for char_obj in text_page.chars().iter() {
        if let Some(ch) = char_obj.unicode_char() {
            if ch == '\0' {
                continue;
            }
            let (left, right, bottom, top) = if let Ok(rect) =
                char_obj.loose_bounds().or_else(|_| char_obj.tight_bounds())
            {
                (
                    rect.left().value,
                    rect.right().value,
                    rect.bottom().value,
                    rect.top().value,
                )
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };

            let font_size_pt = {
                let sz = char_obj.scaled_font_size().value;
                if sz > 0.0 && !sz.is_nan() {
                    sz
                } else {
                    (top - bottom).abs()
                }
            };
            let glyph_h_pt = char_obj
                .tight_bounds()
                .map(|r| (r.top().value - r.bottom().value).abs())
                .unwrap_or(0.0);

            let min_x = (left * scale).min(right * scale);
            let max_x = (left * scale).max(right * scale);
            let py1 = (page_height_pt - top) * scale;
            let py2 = (page_height_pt - bottom) * scale;
            let min_y = py1.min(py2);
            let max_y = py1.max(py2);

            chars.push(ExtractedChar {
                ch,
                min_x,
                min_y,
                max_x,
                max_y,
                font_size_pt,
                glyph_h_pt,
            });
        }
    }

    chars
}

/// 从字符列表中提取落在高亮包围盒内的文字与字号、行距、缩进。
/// - 自动过滤掉不在包围盒内的字符（带 4 像素容差）；
/// - 按阅读顺序分行排序（行内从左到右，行间从上到下）；
/// - 清理占位标签语法（如 `{{...}}`、`【...】`、`{{手写:...}}` 等）；
/// - 计算匹配字符的平均字号（像素）、行距与首行缩进。
pub fn extract_text_and_font_size_for_box(
    chars: &[ExtractedChar],
    b: &BoundingBox,
    scale: f32,
) -> (String, i32, f32, f32) {
    if chars.is_empty() {
        return (String::new(), 0, 0.0, 0.0);
    }

    let pad = 4.0f32;
    let box_min_x = (b.min_x as f32 - pad).max(0.0);
    let box_max_x = b.max_x as f32 + pad;
    let box_min_y = (b.min_y as f32 - pad).max(0.0);
    let box_max_y = b.max_y as f32 + pad;

    let mut matched: Vec<&ExtractedChar> = Vec::new();

    for c in chars {
        if c.ch.is_control() && c.ch != '\n' && c.ch != '\t' {
            continue;
        }

        let cx = (c.min_x + c.max_x) / 2.0;
        let cy = (c.min_y + c.max_y) / 2.0;

        let center_inside =
            cx >= box_min_x && cx <= box_max_x && cy >= box_min_y && cy <= box_max_y;

        let overlap_x = (c.max_x.min(box_max_x) - c.min_x.max(box_min_x)).max(0.0);
        let overlap_y = (c.max_y.min(box_max_y) - c.min_y.max(box_min_y)).max(0.0);
        let char_w = (c.max_x - c.min_x).abs().max(1.0);
        let char_h = (c.max_y - c.min_y).abs().max(1.0);
        let overlap_area = overlap_x * overlap_y;
        let char_area = char_w * char_h;
        let overlap_inside = overlap_area >= 0.3 * char_area;

        if center_inside || overlap_inside {
            matched.push(c);
        }
    }

    if matched.is_empty() {
        return (String::new(), 0, 0.0, 0.0);
    }

    // 计算平均字号 (pt -> px)：全角字符存在时用紧包围盒高度校准（见 resolve_font_size_px）
    let valid_font_sizes: Vec<f32> = matched
        .iter()
        .map(|c| c.font_size_pt)
        .filter(|&s| s > 0.0 && !s.is_nan())
        .collect();

    let avg_font_size_pt = if !valid_font_sizes.is_empty() {
        valid_font_sizes.iter().sum::<f32>() / valid_font_sizes.len() as f32
    } else {
        0.0
    };

    let avg_char_h = matched
        .iter()
        .map(|c| (c.max_y - c.min_y).abs())
        .filter(|&h| h > 0.0)
        .sum::<f32>()
        / matched.len().max(1) as f32;

    let font_size_px = {
        let resolved = resolve_font_size_px(&matched, avg_font_size_pt, scale);
        if resolved > 0 {
            resolved
        } else if avg_char_h > 2.0 {
            avg_char_h.round() as i32
        } else {
            (b.height() as f32 * 0.8).round().max(1.0) as i32
        }
    };

    // 按阅读顺序排序：先按垂直中心坐标粗排
    matched.sort_by(|a, b| {
        let ay = (a.min_y + a.max_y) / 2.0;
        let by = (b.min_y + b.max_y) / 2.0;
        ay.total_cmp(&by).then_with(|| a.min_x.total_cmp(&b.min_x))
    });

    // 动态分行
    let line_threshold = if font_size_px > 0 {
        (font_size_px as f32 * 0.5).max(4.0)
    } else if avg_char_h > 0.0 {
        (avg_char_h * 0.5).max(4.0)
    } else {
        (b.height() as f32 * 0.5).max(4.0)
    };

    let mut lines: Vec<Vec<&ExtractedChar>> = Vec::new();
    for c in matched {
        let cy = (c.min_y + c.max_y) / 2.0;
        if let Some(last_line) = lines.last_mut() {
            let line_avg_cy = last_line
                .iter()
                .map(|ch| (ch.min_y + ch.max_y) / 2.0)
                .sum::<f32>()
                / last_line.len() as f32;
            if (cy - line_avg_cy).abs() <= line_threshold {
                last_line.push(c);
                continue;
            }
        }
        lines.push(vec![c]);
    }

    // 各行内按 x 坐标升序排序
    for line in &mut lines {
        line.sort_by(|a, b| {
            a.min_x
                .total_cmp(&b.min_x)
                .then_with(|| a.min_y.total_cmp(&b.min_y))
        });
    }

    // 行距与首行缩进检测前，先按行收集（行内去首尾空白：PDF 文本层的行尾空格
    // 会与排版换行叠加产生空行槽，把后续行整体推低一行）
    let mut trimmed_lines: Vec<Vec<&ExtractedChar>> = Vec::new();
    for line in &lines {
        let mut l = line.clone();
        while l.last().map(|c| c.ch == ' ').unwrap_or(false) {
            l.pop();
        }
        while l.first().map(|c| c.ch == ' ').unwrap_or(false) {
            l.remove(0);
        }
        if !l.is_empty() {
            trimmed_lines.push(l);
        }
    }

    // 行距与首行缩进检测
    let mut detected_line_spacing = 0.0f32;
    if lines.len() >= 2 {
        let line_centers: Vec<f32> = lines
            .iter()
            .map(|line| {
                line.iter().map(|ch| (ch.min_y + ch.max_y) / 2.0).sum::<f32>() / line.len() as f32
            })
            .collect();
        let total_pitch_diff: f32 = line_centers
            .windows(2)
            .map(|w| (w[1] - w[0]).max(0.0))
            .sum();
        let avg_line_pitch = total_pitch_diff / (line_centers.len() - 1) as f32;
        detected_line_spacing = (avg_line_pitch - font_size_px as f32).max(0.0);
    }

    let mut detected_indent_em = 0.0f32;
    if lines.len() >= 2 && font_size_px > 0 {
        let line1_min_x = lines[0].iter().map(|c| c.min_x).fold(f32::INFINITY, f32::min);
        let min_other_x = lines[1..]
            .iter()
            .flat_map(|line| line.iter().map(|c| c.min_x))
            .fold(f32::INFINITY, f32::min);

        if line1_min_x.is_finite()
            && min_other_x.is_finite()
            && line1_min_x > min_other_x + font_size_px as f32 * 0.8
        {
            detected_indent_em = ((line1_min_x - min_other_x) / font_size_px as f32).round().max(0.0);
        }
    }

    let mut raw_text = String::new();
    for (i, line) in trimmed_lines.iter().enumerate() {
        if i > 0 {
            raw_text.push('\n');
        }
        for c in line {
            raw_text.push(c.ch);
        }
    }

    let clean_text = strip_tag_syntax(&raw_text);
    (clean_text, font_size_px, detected_line_spacing, detected_indent_em)
}

fn extract_pdf_page_tags(
    page_chars: &[ExtractedChar],
    page_index: usize,
    dpi: u32,
    img_width: u32,
    img_height: u32,
    img: &mut image::RgbImage,
) -> Vec<TextRegion> {
    let chars_vec: Vec<char> = page_chars.iter().map(|c| c.ch).collect();
    let matches = scan_text_tags(&chars_vec);
    let scale = dpi as f32 / 72.0;

    let mut regions = Vec::new();

    for m in matches {
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        let mut font_sizes = Vec::new();
        let mut tag_chars: Vec<&ExtractedChar> = Vec::new();
        let mut has_valid_bounds = false;

        for k in m.start_char_idx..=m.end_char_idx {
            if k < page_chars.len() {
                let e = &page_chars[k];
                if e.max_x > e.min_x && e.max_y > e.min_y {
                    min_x = min_x.min(e.min_x);
                    max_x = max_x.max(e.max_x);
                    min_y = min_y.min(e.min_y);
                    max_y = max_y.max(e.max_y);
                    if e.font_size_pt > 0.0 {
                        font_sizes.push(e.font_size_pt);
                    }
                    tag_chars.push(e);
                    has_valid_bounds = true;
                }
            }
        }

        if !has_valid_bounds {
            continue;
        }

        // 像素坐标（原点在左上角）
        let x_px = min_x.round() as i32;
        let y_px = min_y.round() as i32;
        let w_px = (max_x - min_x).round().max(1.0) as i32;
        let h_px = (max_y - min_y).round().max(1.0) as i32;

        let x = x_px.max(0).min(img_width.saturating_sub(1) as i32);
        let y = y_px.max(0).min(img_height.saturating_sub(1) as i32);
        let w = w_px.max(1).min((img_width as i32).saturating_sub(x).max(1));
        let h = h_px.max(1).min((img_height as i32).saturating_sub(y).max(1));

        let avg_fs = if !font_sizes.is_empty() {
            font_sizes.iter().sum::<f32>() / font_sizes.len() as f32
        } else {
            0.0
        };
        let font_size = resolve_font_size_px(&tag_chars, avg_fs, scale);

        // 清理原图上的 {{...}} 标签区域为纯白色（向外扩展 2 像素以消除文字抗锯齿）
        let pad = 2i32;
        let erase_x0 = (x - pad).max(0) as u32;
        let erase_y0 = (y - pad).max(0) as u32;
        let erase_x1 = ((x + w + pad) as u32).min(img_width);
        let erase_y1 = ((y + h + pad) as u32).min(img_height);

        for ey in erase_y0..erase_y1 {
            for ex in erase_x0..erase_x1 {
                img.put_pixel(ex, ey, image::Rgb([255, 255, 255]));
            }
        }

        let clean_text = strip_tag_syntax(&m.inner_text);

        regions.push(TextRegion {
            x,
            y,
            w,
            h,
            text: clean_text,
            printed: false,
            page: (page_index + 1) as i32,
            font_size,
            ..TextRegion::default()
        });
    }

    regions
}

/// 合并高亮区域与文本标签区域，并从 PDF 字符层中提取高亮框内部文字与字号。
pub fn combine_page_regions(
    highlight_boxes: Vec<BoundingBox>,
    tag_regions: Vec<TextRegion>,
    page_chars: &[ExtractedChar],
    page_num: i32,
    scale: f32,
) -> Vec<TextRegion> {
    let mut color_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut next_role_id = 2u32;
    combine_page_regions_with_role_map(
        highlight_boxes,
        tag_regions,
        page_chars,
        page_num,
        scale,
        &mut color_map,
        &mut next_role_id,
    )
}

pub fn combine_page_regions_with_role_map(
    highlight_boxes: Vec<BoundingBox>,
    tag_regions: Vec<TextRegion>,
    page_chars: &[ExtractedChar],
    page_num: i32,
    scale: f32,
    color_map: &mut std::collections::HashMap<String, u32>,
    next_role_id: &mut u32,
) -> Vec<TextRegion> {
    let mut final_regions: Vec<TextRegion> = Vec::new();
    let mut matched_tags = vec![false; tag_regions.len()];

    for b in highlight_boxes {
        let bx = b.min_x as i32;
        let by = b.min_y as i32;
        let bw = b.width() as i32;
        let bh = b.height() as i32;

        let (role_id, highlight) = if let Some(c) = &b.highlight {
            let rid = *color_map.entry(c.clone()).or_insert_with(|| {
                let id = *next_role_id;
                *next_role_id += 1;
                id
            });
            (rid, Some(c.clone()))
        } else {
            (0, None)
        };

        // 从字符中提取文字与字号、行距、缩进
        let (extracted_text, detected_font_size, detected_line_spacing, detected_indent_em) =
            extract_text_and_font_size_for_box(page_chars, &b, scale);

        // 查找是否包含或重叠某个 tag_region
        let mut tag_text = String::new();
        let mut tag_font_size = 0;
        for (t_idx, tag) in tag_regions.iter().enumerate() {
            if matched_tags[t_idx] {
                continue;
            }
            let overlap_x = (bx + bw).min(tag.x + tag.w) - bx.max(tag.x);
            let overlap_y = (by + bh).min(tag.y + tag.h) - by.max(tag.y);
            if overlap_x > 0 && overlap_y > 0 {
                matched_tags[t_idx] = true;
                if !tag.text.is_empty() {
                    tag_text = tag.text.clone();
                    tag_font_size = tag.font_size;
                }
            }
        }

        let (text, font_size, line_spacing, indent_em) = if !extracted_text.is_empty() {
            (
                extracted_text,
                detected_font_size,
                if detected_line_spacing > 0.0 {
                    Some(detected_line_spacing)
                } else {
                    None
                },
                detected_indent_em,
            )
        } else if !tag_text.is_empty() {
            (tag_text, tag_font_size, None, 0.0)
        } else {
            (String::new(), detected_font_size, None, 0.0)
        };

        final_regions.push(TextRegion {
            x: bx,
            y: by,
            w: bw,
            h: bh,
            text,
            role_id,
            highlight,
            printed: false,
            page: page_num,
            font_size,
            line_spacing,
            indent_em,
            ..TextRegion::default()
        });
    }

    // 剩余未与高亮框重叠的 tag 区域直接作为独立的 TextRegion
    for (t_idx, tag) in tag_regions.into_iter().enumerate() {
        if !matched_tags[t_idx] {
            final_regions.push(tag);
        }
    }

    // 按照从上到下、从左到右排序
    final_regions.sort_by(|a, b| {
        if (a.y - b.y).abs() <= 10 {
            a.x.cmp(&b.x)
        } else {
            a.y.cmp(&b.y)
        }
    });

    final_regions
}

/// 入口：PDF 直接渲染；DOCX 先转 PDF。返回逐页 PNG 路径与自动识别的 TextRegion 列表。
pub fn document_to_page_images_with_regions(
    path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError> {
    document_to_page_images_opt(path, out_dir, dpi, true)
}

/// 入口：PDF 直接渲染；DOCX 先转 PDF。
/// 可选是否识别并擦除标记区域。若 `extract_regions` 为 false 则完整保留原文档所有颜色、文字与背景（纯底图）。
pub fn document_to_page_images_opt(
    path: &Path,
    out_dir: &Path,
    dpi: u32,
    extract_regions: bool,
) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError> {
    let suffix = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match suffix.as_str() {
        "pdf" => pdf_to_images_opt(path, out_dir, dpi, extract_regions),
        "docx" => {
            let pdf_path = docx_to_pdf(path, out_dir)?;
            pdf_to_images_opt(&pdf_path, out_dir, dpi, extract_regions)
        }
        other => Err(DocRenderError::UnsupportedExtension(format!(".{other}"))),
    }
}

/// 入口：PDF 直接渲染；DOCX 先转 PDF。返回逐页 PNG 路径（页序即列表序，纯底图，不擦除高亮，不提取区域）。
pub fn document_to_page_images(
    path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<Vec<PathBuf>, DocRenderError> {
    document_to_page_images_opt(path, out_dir, dpi, false).map(|(paths, _)| paths)
}

/// 把 PDF 逐页栅格化为 PNG，并自动检测标记区域，返回 (文件路径列表, 识别出的 TextRegion 列表)。
pub fn pdf_to_images_with_regions(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError> {
    pdf_to_images_opt(pdf_path, out_dir, dpi, true)
}

/// 把 PDF 逐页栅格化为 PNG。
/// 若 `extract_regions` 为 true 则自动检测高亮色块与标签并擦除、提取 TextRegion 区域；
/// 若为 false 则完整保留原始图像与背景，不擦除也不提取。
pub fn pdf_to_images_opt(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
    extract_regions: bool,
) -> Result<(Vec<PathBuf>, Vec<TextRegion>), DocRenderError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| DocRenderError::Other(format!("创建缓存目录失败：{e}")))?;
    let prefix = page_prefix(pdf_path);
    clear_stale_pages(out_dir, &prefix);

    let pdfium = open_pdfium()?;
    let document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| DocRenderError::Other(format!("打开 PDF 失败：{e}")))?;
    let scale = dpi as f32 / 72.0;
    let mut paths = Vec::new();
    let mut all_regions = Vec::new();
    let mut color_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut next_role_id = 2u32;

    for (index, page) in document.pages().iter().enumerate() {
        // 目标像素尺寸：页面点数（1/72 英寸）× dpi/72
        let w_px = (page.width().value * scale).round().max(1.0) as i32;
        let h_px = (page.height().value * scale).round().max(1.0) as i32;
        let bitmap = page
            .render(w_px, h_px, None)
            .map_err(|e| DocRenderError::Other(format!("第 {} 页渲染失败：{e}", index + 1)))?;
        let image = bitmap.as_image();
        let mut rgb_img = image.to_rgb8();

        if extract_regions {
            // 1. 提取页面所有字符对象及其包围盒与字号
            let page_chars = extract_pdf_page_chars(&page, dpi);

            // 2. 从 PDF 文本层提取 {{...}} / 【...】标签区域，并在图像上抹除标签文字
            let tag_regions = extract_pdf_page_tags(
                &page_chars,
                index,
                dpi,
                rgb_img.width(),
                rgb_img.height(),
                &mut rgb_img,
            );

            // 3. 从渲染图像中检测高亮色块，并将其擦除为白色
            let highlight_boxes = detect_highlight_boxes(&rgb_img);
            erase_highlight_boxes(&mut rgb_img, &highlight_boxes);

            // 4. 合并高亮框与文本标签区域，提取高亮框内部文字和字号
            let page_regions = combine_page_regions_with_role_map(
                highlight_boxes,
                tag_regions,
                &page_chars,
                (index + 1) as i32,
                scale,
                &mut color_map,
                &mut next_role_id,
            );
            all_regions.extend(page_regions);
        }

        let path = out_dir.join(format!("{prefix}_{index}.png"));
        rgb_img
            .save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| DocRenderError::Other(format!("保存 {} 失败：{e}", path.display())))?;
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(DocRenderError::NoPages(pdf_path.display().to_string()));
    }
    Ok((paths, all_regions))
}

/// 把 PDF 逐页栅格化为 PNG，返回按页序排列的文件路径列表（纯底图模式）。
/// 对齐 Python 版 `pdf_to_images`（默认 200 DPI）。
pub fn pdf_to_images(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<Vec<PathBuf>, DocRenderError> {
    pdf_to_images_opt(pdf_path, out_dir, dpi, false).map(|(paths, _)| paths)
}

/// 页文件名前缀：文档名（不含扩展名），清理非法字符避免跨平台问题。
fn page_prefix(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_string())
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii() {
            c
        } else {
            '_'
        })
        .collect()
}

/// 清理同前缀的旧页文件，避免旧文档页数混入新导入结果。
fn clear_stale_pages(out_dir: &Path, prefix: &str) {
    if let Ok(rd) = std::fs::read_dir(out_dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(prefix) && name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// 嵌入在二进制中的各平台 pdfium 动态库（在编译期由 build.rs 注入）
const EMBEDDED_PDFIUM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded_pdfium.bin"));

/// 获取运行时自解压的目标路径
fn get_runtime_extracted_pdfium_path() -> Option<PathBuf> {
    let filename = if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };

    let base_dir = std::env::temp_dir().join("handwrite_sim_runtime");
    if std::fs::create_dir_all(&base_dir).is_err() {
        return None;
    }
    Some(base_dir.join(filename))
}

/// 打开 pdfium 动态库：
/// 1. 优先使用 exe 目录或当前工作目录下的外部动态库（若用户手动放入）；
/// 2. 尝试系统已安装的共享库；
/// 3. 若均无，自动解压内置编译的 64 位 pdfium 动态库到系统临时目录并透明加载！
fn open_pdfium(
) -> Result<pdfium_render::prelude::Pdfium, DocRenderError> {
    use pdfium_render::prelude::*;

    let mut load_errors: Vec<String> = Vec::new();

    // 1. 外部候选路径探测（exe 同级目录与当前工作目录）
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            candidates.push(dir.join("pdfium.dll"));
            candidates.push(dir.join("libpdfium.so"));
            candidates.push(dir.join("libpdfium.dylib"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("pdfium.dll"));
        candidates.push(cwd.join("libpdfium.so"));
        candidates.push(cwd.join("libpdfium.dylib"));
    }

    for candidate in candidates {
        if candidate.is_file() {
            match Pdfium::bind_to_library(&candidate) {
                Ok(bindings) => return Ok(Pdfium::new(bindings)),
                Err(e) => {
                    load_errors.push(format!("加载外部库 {} 失败: {e}", candidate.display()));
                }
            }
        }
    }

    // 2. 尝试系统默认库
    if let Ok(bindings) = Pdfium::bind_to_system_library() {
        return Ok(Pdfium::new(bindings));
    }

    // 3. 自动自解压内置的 64 位 pdfium 动态库
    if !EMBEDDED_PDFIUM.is_empty() {
        if let Some(target_path) = get_runtime_extracted_pdfium_path() {
            // 如果已存在且大小一致，直接复用；否则写入
            let need_write = match std::fs::metadata(&target_path) {
                Ok(meta) => meta.len() != EMBEDDED_PDFIUM.len() as u64,
                Err(_) => true,
            };

            if need_write {
                let _ = std::fs::write(&target_path, EMBEDDED_PDFIUM);
            }

            if target_path.is_file() {
                match Pdfium::bind_to_library(&target_path) {
                    Ok(bindings) => return Ok(Pdfium::new(bindings)),
                    Err(e) => {
                        load_errors.push(format!("加载内置自解压库 {} 失败: {e}", target_path.display()));
                    }
                }
            }
        }
    }

    let detail = if load_errors.is_empty() {
        "未找到可用的 pdfium 动态库".to_string()
    } else {
        load_errors.join("；")
    };
    Err(DocRenderError::PdfiumUnavailable(detail))
}


/// 把 DOCX 转成 PDF（Word COM 优先，LibreOffice 兜底）。对齐 Python 版 `docx_to_pdf`。
pub fn docx_to_pdf(docx_path: &Path, out_dir: &Path) -> Result<PathBuf, DocRenderError> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| DocRenderError::Other(format!("创建缓存目录失败：{e}")))?;
    let stem = docx_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    let pdf_path = out_dir.join(format!("{stem}.pdf"));
    let _ = std::fs::remove_file(&pdf_path);

    #[cfg(target_os = "windows")]
    {
        let script = word_com_script(docx_path, &pdf_path);
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        match run_with_timeout(&mut cmd, Duration::from_secs(300)) {
            Ok(true) if pdf_path.is_file() => return Ok(pdf_path),
            _ => {} // Word 未安装或转换失败，继续尝试 LibreOffice
        }
    }

    let soffice = find_soffice();
    if let Some(soffice) = soffice {
        let mut cmd = Command::new(&soffice);
        cmd.args(["--headless", "--convert-to", "pdf"])
            .arg("--outdir")
            .arg(out_dir)
            .arg(docx_path);
        let _ = run_with_timeout(&mut cmd, Duration::from_secs(300));
        if pdf_path.is_file() {
            return Ok(pdf_path);
        }
    }

    Err(DocRenderError::DocxConversionUnavailable)
}

/// 查找 LibreOffice 可执行文件（PATH + Windows 常见安装目录）。
fn find_soffice() -> Option<PathBuf> {
    if let Some(p) = which("soffice") {
        return Some(p);
    }
    if let Some(p) = which("libreoffice") {
        return Some(p);
    }
    #[cfg(target_os = "windows")]
    for base in [
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
    ] {
        let p = PathBuf::from(base);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 极简 PATH 查找（避免引入 which crate）。
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let ext = cfg!(target_os = "windows").then_some(".exe");
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if let Some(ext) = ext {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 带超时的进程运行（返回是否成功退出）。Rust 标准库无内置超时，轮询实现。
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<bool, String> {
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("转换超时".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// 生成调用 Word COM 另存为 PDF 的 PowerShell 脚本（17 = wdFormatPDF）。
/// 对齐 Python 版 `_word_com_script`。
#[cfg(target_os = "windows")]
fn word_com_script(docx_path: &Path, pdf_path: &Path) -> String {
    let src = docx_path.display().to_string().replace('\'', "''");
    let dst = pdf_path.display().to_string().replace('\'', "''");
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $word = New-Object -ComObject Word.Application\n\
         $word.Visible = $false\n\
         try {{\n\
         \x20 $doc = $word.Documents.Open('{src}', $false, $true)\n\
         \x20 $doc.SaveAs([ref]'{dst}', [ref]17)\n\
         \x20 $doc.Close($false)\n\
         }} finally {{\n\
         \x20 $word.Quit()\n\
         }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_extension_rejected() {
        let err = document_to_page_images(Path::new("a.txt"), Path::new("out"), 200).unwrap_err();
        assert!(matches!(err, DocRenderError::UnsupportedExtension(_)));
    }

    #[test]
    fn page_prefix_sanitized() {
        assert_eq!(page_prefix(Path::new("我的 文档:v2.pdf")), "我的_文档_v2");
    }

    #[test]
    fn test_highlight_pixel_classifier() {
        // 标准高亮色
        assert!(is_highlight_pixel(255, 255, 0)); // 黄色
        assert!(is_highlight_pixel(0, 255, 0)); // 绿色
        assert!(is_highlight_pixel(0, 255, 255)); // 青色
        assert!(is_highlight_pixel(255, 0, 255)); // 品红
        assert!(is_highlight_pixel(255, 105, 180)); // 粉红
        assert!(is_highlight_pixel(100, 180, 255)); // 浅蓝
        assert!(is_highlight_pixel(255, 80, 80)); // 浅红

        // 灰度/黑白背景与文字
        assert!(!is_highlight_pixel(255, 255, 255)); // 纯白
        assert!(!is_highlight_pixel(0, 0, 0)); // 纯黑
        assert!(!is_highlight_pixel(30, 30, 30)); // 深灰文字
        assert!(!is_highlight_pixel(128, 128, 128)); // 灰色
        assert!(!is_highlight_pixel(240, 240, 240)); // 浅灰背景
        assert!(!is_highlight_pixel(200, 205, 202)); // 轻微抗锯齿灰边
    }

    #[test]
    fn test_detect_highlight_boxes_and_erase() {
        let mut img = image::RgbImage::from_pixel(400, 300, image::Rgb([255, 255, 255]));

        // 绘制一个黄色高亮矩形区域：(50, 60), 宽 100, 高 30
        for y in 60..(60 + 30) {
            for x in 50..(50 + 100) {
                img.put_pixel(x, y, image::Rgb([255, 255, 0]));
            }
        }

        // 绘制一个青色高亮矩形区域：(200, 150), 宽 80, 高 20
        for y in 150..(150 + 20) {
            for x in 200..(200 + 80) {
                img.put_pixel(x, y, image::Rgb([0, 255, 255]));
            }
        }

        // 绘制一个微小噪声噪点：5x5（应被过滤）
        for y in 10..15 {
            for x in 10..15 {
                img.put_pixel(x, y, image::Rgb([255, 0, 0]));
            }
        }

        let boxes = detect_highlight_boxes(&img);
        assert_eq!(boxes.len(), 2, "应检测到 2 个有效高亮框并过滤噪点");

        // 验证第一个黄色框
        let b1 = boxes.iter().find(|b| b.min_x == 50 && b.min_y == 60).unwrap();
        assert_eq!(b1.width(), 100);
        assert_eq!(b1.height(), 30);

        // 验证第二个青色框
        let b2 = boxes.iter().find(|b| b.min_x == 200 && b.min_y == 150).unwrap();
        assert_eq!(b2.width(), 80);
        assert_eq!(b2.height(), 20);

        // 测试擦除
        erase_highlight_boxes(&mut img, &boxes);

        // 擦除后，高亮区域应变为纯白色 (255, 255, 255)
        for y in 60..(60 + 30) {
            for x in 50..(50 + 100) {
                assert_eq!(img.get_pixel(x, y), &image::Rgb([255, 255, 255]));
            }
        }
        for y in 150..(150 + 20) {
            for x in 200..(200 + 80) {
                assert_eq!(img.get_pixel(x, y), &image::Rgb([255, 255, 255]));
            }
        }
    }

    #[test]
    fn test_scan_text_tags() {
        let text1: Vec<char> = "请在此处手写：{{ 签名 }}，祝好！".chars().collect();
        let matches1 = scan_text_tags(&text1);
        assert_eq!(matches1.len(), 1);
        assert_eq!(matches1[0].inner_text, "签名");

        let text2: Vec<char> = "意见：【同意批准】 日期：【2026-09-02】".chars().collect();
        let matches2 = scan_text_tags(&text2);
        assert_eq!(matches2.len(), 2);
        assert_eq!(matches2[0].inner_text, "同意批准");
        assert_eq!(matches2[1].inner_text, "2026-09-02");

        let text3: Vec<char> = "空标签：{{}} 以及未闭合标签 {{ 未闭合".chars().collect();
        let matches3 = scan_text_tags(&text3);
        assert_eq!(matches3.len(), 1);
        assert_eq!(matches3[0].inner_text, "");
    }

    #[test]
    fn test_strip_tag_syntax() {
        assert_eq!(strip_tag_syntax("纯文本内容"), "纯文本内容");
        assert_eq!(strip_tag_syntax("{{ 签名 }}"), "签名");
        assert_eq!(strip_tag_syntax("{{手写:张三}}"), "张三");
        assert_eq!(strip_tag_syntax("{{手写1: 李四}}"), "李四");
        assert_eq!(strip_tag_syntax("{{打印: 2026-09-02}}"), "2026-09-02");
        assert_eq!(strip_tag_syntax("【同意批准】"), "同意批准");
        assert_eq!(strip_tag_syntax("【手写：王五】"), "王五");
        assert_eq!(strip_tag_syntax("{{}}"), "");
        assert_eq!(strip_tag_syntax("【】"), "");
        assert_eq!(strip_tag_syntax("   {{  审核通过  }}   "), "审核通过");
        assert_eq!(strip_tag_syntax("前缀 {{手写:内容}} 后缀"), "前缀 内容 后缀");
        assert_eq!(strip_tag_syntax("前缀 【手写：结论】 后缀"), "前缀 结论 后缀");
    }

    #[test]
    fn test_resolve_font_size_calibrates_with_full_width_glyph() {
        // 回归：Word 导出的中文 PDF 中 scaled_font_size 偏大约 6%（11.16pt vs
        // 真实 10.5pt），原文恰好占满行宽的文本在区域内会整行折行放不下。
        // 存在全角字符时用其紧包围盒高度（≈字号）做上限校准，取较小值。
        let scale = 200.0 / 72.0;
        let chars = vec![
            ExtractedChar {
                ch: '张',
                min_x: 0.0,
                min_y: 0.0,
                max_x: 29.2,
                max_y: 58.7,
                font_size_pt: 11.16,
                glyph_h_pt: 10.18,
            },
            ExtractedChar {
                ch: '三',
                min_x: 29.2,
                min_y: 0.0,
                max_x: 58.4,
                max_y: 58.7,
                font_size_pt: 11.16,
                glyph_h_pt: 10.19,
            },
            ExtractedChar {
                ch: '丰',
                min_x: 58.4,
                min_y: 0.0,
                max_x: 87.6,
                max_y: 58.7,
                font_size_pt: 11.16,
                glyph_h_pt: 10.15,
            },
        ];
        let refs: Vec<&ExtractedChar> = chars.iter().collect();
        assert_eq!(
            resolve_font_size_px(&refs, 11.16, scale),
            (10.19 * scale).round() as i32
        );

        // 纯西文（无全角字符）：保持 scaled 值（西文紧包围盒远小于字号，不可校准）
        let latin = vec![ExtractedChar {
            ch: 'H',
            min_x: 0.0,
            min_y: 0.0,
            max_x: 18.0,
            max_y: 26.78,
            font_size_pt: 24.0,
            glyph_h_pt: 17.3,
        }];
        let latin_refs: Vec<&ExtractedChar> = latin.iter().collect();
        assert_eq!(
            resolve_font_size_px(&latin_refs, 24.0, scale),
            (24.0 * scale).round() as i32
        );

        // 无字形信息（glyph=0，如旧数据/异常 PDF）：保持 scaled 值
        let no_glyph = vec![ExtractedChar {
            ch: '字',
            min_x: 0.0,
            min_y: 0.0,
            max_x: 29.2,
            max_y: 58.7,
            font_size_pt: 12.0,
            glyph_h_pt: 0.0,
        }];
        let no_glyph_refs: Vec<&ExtractedChar> = no_glyph.iter().collect();
        assert_eq!(
            resolve_font_size_px(&no_glyph_refs, 12.0, scale),
            (12.0 * scale).round() as i32
        );
    }

    #[test]
    fn test_extract_text_and_font_size_for_box() {
        let scale = 200.0 / 72.0;
        let chars = vec![
            ExtractedChar {
                ch: '张',
                min_x: 100.0,
                min_y: 50.0,
                max_x: 120.0,
                max_y: 70.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '三',
                min_x: 125.0,
                min_y: 50.0,
                max_x: 145.0,
                max_y: 70.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '外',
                min_x: 500.0,
                min_y: 500.0,
                max_x: 520.0,
                max_y: 520.0,
                font_size_pt: 10.0,
                glyph_h_pt: 0.0,
            },
        ];

        let b1 = BoundingBox::new(95, 45, 150, 75);

        let (text1, fs1, _, _) = extract_text_and_font_size_for_box(&chars, &b1, scale);
        assert_eq!(text1, "张三");
        let expected_fs = (12.0 * scale).round() as i32;
        assert_eq!(fs1, expected_fs);

        // 空高亮框
        let b_empty = BoundingBox::new(300, 300, 350, 330);
        let (text_empty, _, _, _) = extract_text_and_font_size_for_box(&chars, &b_empty, scale);
        assert_eq!(text_empty, "");
    }

    #[test]
    fn test_sort_characters_reading_order() {
        let scale = 200.0 / 72.0;
        // 乱序的多行字符
        let chars = vec![
            ExtractedChar {
                ch: '行',
                min_x: 90.0,
                min_y: 139.0,
                max_x: 105.0,
                max_y: 155.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '一',
                min_x: 70.0,
                min_y: 101.0,
                max_x: 85.0,
                max_y: 116.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '第',
                min_x: 50.0,
                min_y: 100.0,
                max_x: 65.0,
                max_y: 115.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '二',
                min_x: 70.0,
                min_y: 141.0,
                max_x: 85.0,
                max_y: 156.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '行',
                min_x: 90.0,
                min_y: 99.0,
                max_x: 105.0,
                max_y: 114.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '第',
                min_x: 50.0,
                min_y: 140.0,
                max_x: 65.0,
                max_y: 155.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
        ];

        let b = BoundingBox::new(45, 95, 110, 160);

        let (text, _, _, _) = extract_text_and_font_size_for_box(&chars, &b, scale);
        assert_eq!(text, "第一行\n第二行");
    }

    #[test]
    fn test_merge_four_consecutive_line_highlight_boxes() {
        // 4 行高亮框，每行宽 300，高 20，间距 25px
        let boxes = vec![
            BoundingBox::with_highlight(100, 50, 400, 70, "yellow"),
            BoundingBox::with_highlight(100, 95, 400, 115, "yellow"),
            BoundingBox::with_highlight(100, 140, 400, 160, "yellow"),
            BoundingBox::with_highlight(100, 185, 400, 205, "yellow"),
        ];

        let merged = merge_close_boxes(boxes);
        assert_eq!(merged.len(), 1, "4 行间距 25px 的连续同色高亮框应合并为 1 个段落包围盒");
        assert_eq!(merged[0].min_x, 100);
        assert_eq!(merged[0].max_x, 400);
        assert_eq!(merged[0].min_y, 50);
        assert_eq!(merged[0].max_y, 205);
        assert_eq!(merged[0].highlight, Some("yellow".into()));
    }

    #[test]
    fn test_merge_does_not_swallow_across_unhighlighted_lines() {
        // 回归：段落间被未高亮内容（如标题行）隔开时，垂直间隙远大于行框高度，
        // 不得因累计合并框已变高（max_h 增长）而把下方内容吞进同一区域。
        let boxes = vec![
            BoundingBox::with_highlight(100, 50, 400, 70, "yellow"),
            BoundingBox::with_highlight(100, 95, 400, 115, "yellow"),
            BoundingBox::with_highlight(100, 140, 400, 160, "yellow"),
            BoundingBox::with_highlight(100, 185, 400, 205, "yellow"),
            // 与上一框间隙 109px（跨过一行未高亮文字），水平部分重叠
            BoundingBox::with_highlight(150, 314, 350, 334, "yellow"),
        ];

        let merged = merge_close_boxes(boxes);
        assert_eq!(merged.len(), 2, "间隔 109px 的同色高亮框不应合并");
        assert_eq!(merged[0].max_y, 205);
        assert_eq!(merged[1].min_y, 314);
    }

    #[test]
    fn test_extract_multiline_line_spacing_and_indent() {
        let scale = 1.0;
        let font_size = 30.0;
        // Line 1 (首行缩进 60px = 2em, y 中心 50): "第一行"
        // Line 2 (无缩进, y 中心 90): "第二行"
        // Line 3 (无缩进, y 中心 130): "第三行"
        // Line pitch = 40, detected line spacing = 40 - 30 = 10
        let chars = vec![
            ExtractedChar {
                ch: '第',
                min_x: 120.0,
                min_y: 35.0,
                max_x: 150.0,
                max_y: 65.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '一',
                min_x: 155.0,
                min_y: 35.0,
                max_x: 185.0,
                max_y: 65.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '行',
                min_x: 190.0,
                min_y: 35.0,
                max_x: 220.0,
                max_y: 65.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '第',
                min_x: 60.0,
                min_y: 75.0,
                max_x: 90.0,
                max_y: 105.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '二',
                min_x: 95.0,
                min_y: 75.0,
                max_x: 125.0,
                max_y: 105.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '行',
                min_x: 130.0,
                min_y: 75.0,
                max_x: 160.0,
                max_y: 105.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '第',
                min_x: 60.0,
                min_y: 115.0,
                max_x: 90.0,
                max_y: 145.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '三',
                min_x: 95.0,
                min_y: 115.0,
                max_x: 125.0,
                max_y: 145.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '行',
                min_x: 130.0,
                min_y: 115.0,
                max_x: 160.0,
                max_y: 145.0,
                font_size_pt: font_size,
                glyph_h_pt: 0.0,
            },
        ];

        let b = BoundingBox::new(50, 30, 250, 150);
        let (text, fs, ls, indent) = extract_text_and_font_size_for_box(&chars, &b, scale);
        assert_eq!(text, "第一行\n第二行\n第三行");
        assert_eq!(fs, 30);
        assert!((ls - 10.0).abs() < 1e-3);
        assert_eq!(indent, 2.0);
    }

    #[test]
    fn test_combine_page_regions() {
        let scale = 200.0 / 72.0;
        let highlight_boxes = vec![
            BoundingBox::with_highlight(50, 100, 250, 140, "yellow"),
            BoundingBox::with_highlight(300, 500, 400, 530, "cyan"),
        ];

        let tag_regions = vec![
            TextRegion {
                x: 60,
                y: 105,
                w: 80,
                h: 20,
                text: "请签名".to_string(),
                page: 1,
                font_size: 28,
                ..TextRegion::default()
            },
            TextRegion {
                x: 100,
                y: 300,
                w: 120,
                h: 25,
                text: "独立标签".to_string(),
                page: 1,
                font_size: 24,
                ..TextRegion::default()
            },
        ];

        // 字符层中包含 (50, 100) 区域内的文字
        let page_chars = vec![
            ExtractedChar {
                ch: '请',
                min_x: 60.0,
                min_y: 105.0,
                max_x: 80.0,
                max_y: 125.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '签',
                min_x: 85.0,
                min_y: 105.0,
                max_x: 105.0,
                max_y: 125.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
            ExtractedChar {
                ch: '名',
                min_x: 110.0,
                min_y: 105.0,
                max_x: 130.0,
                max_y: 125.0,
                font_size_pt: 12.0,
                glyph_h_pt: 0.0,
            },
        ];

        let combined = combine_page_regions(highlight_boxes, tag_regions, &page_chars, 1, scale);
        assert_eq!(combined.len(), 3);

        // 1. (50, 100) 处与字符重叠，拥有高亮框尺寸和提取的文字及字号，分配为 Role 2
        let r1 = combined.iter().find(|r| r.x == 50 && r.y == 100).unwrap();
        assert_eq!(r1.w, 201);
        assert_eq!(r1.h, 41);
        assert_eq!(r1.text, "请签名");
        assert_eq!(r1.page, 1);
        assert_eq!(r1.font_size, (12.0 * scale).round() as i32);
        assert_eq!(r1.role_id, 2);
        assert_eq!(r1.highlight, Some("yellow".into()));

        // 2. (100, 300) 处的独立标签 (role_id 0, highlight None)
        let r2 = combined.iter().find(|r| r.x == 100 && r.y == 300).unwrap();
        assert_eq!(r2.w, 120);
        assert_eq!(r2.text, "独立标签");
        assert_eq!(r2.font_size, 24);
        assert_eq!(r2.role_id, 0);
        assert_eq!(r2.highlight, None);

        // 3. (300, 500) 处的独立高亮框（无文字），分配为 Role 3
        let r3 = combined.iter().find(|r| r.x == 300 && r.y == 500).unwrap();
        assert_eq!(r3.w, 101);
        assert_eq!(r3.text, "");
        assert_eq!(r3.role_id, 3);
        assert_eq!(r3.highlight, Some("cyan".into()));
    }

    #[test]
    fn pdf_to_images_renders_all_pages() {
        let dir = tempfile::tempdir().unwrap();
        let pdf_path = dir.path().join("two_page.pdf");
        {
            use printpdf::PdfDocument;
            let mut doc = PdfDocument::new("handwrite-sim-test");
            let empty_ops: Vec<printpdf::Op> = Vec::new();
            doc.with_pages(vec![
                printpdf::PdfPage::new(
                    printpdf::Mm(210.0),
                    printpdf::Mm(297.0),
                    empty_ops.clone(),
                ),
                printpdf::PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), empty_ops),
            ]);
            let mut warnings = Vec::new();
            let bytes = doc.save(&printpdf::PdfSaveOptions::default(), &mut warnings);
            std::fs::write(&pdf_path, bytes).unwrap();
        }
        let out_dir = dir.path().join("pages");
        match pdf_to_images_with_regions(&pdf_path, &out_dir, 100) {
            Ok((paths, regions)) => {
                assert_eq!(paths.len(), 2, "两页 PDF 应输出两张 PNG");
                for p in &paths {
                    assert!(p.is_file(), "{} 应存在", p.display());
                    let img = image::open(p).unwrap();
                    assert!(
                        (img.width(), img.height()).0 > 500 && (img.width(), img.height()).1 > 700,
                        "页面尺寸异常：{:?}",
                        (img.width(), img.height())
                    );
                }
                // 空白测试 PDF 没有高亮与标签
                assert!(regions.is_empty());

                // 测试纯底图模式 (extract_regions = false)
                let out_dir_pure = dir.path().join("pages_pure");
                if let Ok((paths_pure, regions_pure)) = pdf_to_images_opt(&pdf_path, &out_dir_pure, 100, false) {
                    assert_eq!(paths_pure.len(), 2);
                    assert!(regions_pure.is_empty(), "纯底图模式不应提取任何区域");
                }
            }
            Err(DocRenderError::PdfiumUnavailable(_)) => {
                eprintln!("跳过：未找到 pdfium.dll");
            }
            Err(e) => panic!("PDF 栅格化意外失败：{e}"),
        }
    }

    #[test]
    fn docx_conversion_never_panics() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.docx");
        std::fs::write(&fake, b"not a real docx").unwrap();
        match docx_to_pdf(&fake, dir.path()) {
            Ok(path) => assert!(path.is_file(), "成功时 PDF 应存在：{}", path.display()),
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}
