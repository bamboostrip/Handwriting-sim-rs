//! 字体光栅化封装（基于 ab_glyph）。
//!
//! 对应 Python 版 `PIL.ImageFont.truetype` + `ImageDraw.text` 的职责：
//! 加载字体、测量字形宽度、把字形光栅化到前景掩码。
//! 与 PIL 的关键差异在于坐标约定：本模块统一以**基线原点**放置字形，
//! 排版本层负责把"顶部坐标"换算为"基线坐标"。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use ab_glyph::{point, Font, FontArc, Glyph, Outline, OutlinedGlyph, PxScale, PxScaleFactor, ScaleFont};

/// 字形轮廓缓存：同 (char, size) 在跨页/重复字符中反复出现，
/// 避免每次从字体表转换曲线（对齐 Python 版 PIL FreeType 的内部字形缓存）。
/// 条目超限时整体清空（与背景缓存同策略），限制内存占用。
const GLYPH_CACHE_LIMIT: usize = 4096;

/// 一个已转换字形的缓存项：unscaled 曲线 + 缩放因子（均与 position 无关）。
/// `px_bounds` 与光栅化仍用真实 position 现算，与原实现 `outline_glyph` 的
/// 调用序列完全一致（px_bounds 内部有 fract/trunc 亚像素分解，不能平移复用）。
#[derive(Clone)]
struct CachedGlyph {
    outline: Outline,
    scale_factor: PxScaleFactor,
}

/// 已加载的字体及其基准字号。
pub struct FontFace {
    font: FontArc,
    size: f32,
    glyph_cache: RefCell<HashMap<(char, u32), Option<CachedGlyph>>>,
}

impl FontFace {
    /// 从文件加载字体。
    pub fn load(path: &Path, size: f32) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取字体 {path:?} 失败：{e}"))?;
        let font =
            FontArc::try_from_vec(bytes).map_err(|e| format!("解析字体 {path:?} 失败：{e}"))?;
        Ok(Self { font, size, glyph_cache: RefCell::new(HashMap::new()) })
    }

    /// 字体颜色（用于日志/调试）。
    pub fn size(&self) -> f32 {
        self.size
    }

    fn scaled(&self, size: f32) -> impl ab_glyph::ScaleFont<&FontArc> {
        self.font.as_scaled(PxScale::from(size))
    }

    /// 字形横向推进宽度（advance width，替代 PIL `font.getbbox` 宽度）。
    pub fn glyph_width(&self, ch: char, size: f32) -> f32 {
        let scaled = self.scaled(size.max(1.0));
        scaled.h_advance(scaled.glyph_id(ch))
    }

    /// 基线到字形顶部的距离（把"顶部坐标"换算为"基线坐标"用）。
    pub fn ascent(&self, size: f32) -> f32 {
        self.scaled(size.max(1.0)).ascent()
    }

    /// 取 (ch, size) 的缓存轮廓（None 表示缺字），未命中时生成并缓存。
    fn cached_glyph(&self, ch: char, size: f32) -> Option<CachedGlyph> {
        let key = (ch, size.to_bits());
        if let Some(entry) = self.glyph_cache.borrow().get(&key) {
            return entry.clone();
        }
        // outline 与 scale_factor 均与 glyph position 无关，可安全复用
        let id = self.font.glyph_id(ch);
        let scale = PxScale::from(size.max(1.0));
        let Some(outline) = self.font.outline(id) else {
            self.glyph_cache.borrow_mut().insert(key, None);
            return None; // 缺字（tofu）时跳过
        };
        let scale_factor = self.font.as_scaled(scale).scale_factor();
        let entry = CachedGlyph { outline, scale_factor };
        let mut cache = self.glyph_cache.borrow_mut();
        if cache.len() >= GLYPH_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, Some(entry.clone()));
        Some(entry)
    }

    /// 把字符光栅化到前景掩码。
    ///
    /// - `origin_x` / `origin_y`：**基线**起点。
    /// - 掩码为 `width * height` 的逐行 bool 数组，命中像素置 `true`。
    /// - 覆盖度按阈值二值化（对齐 PIL mode="1" 的近似行为），阈值 0.5。
    #[allow(clippy::too_many_arguments)]
    pub fn rasterize(
        &self,
        ch: char,
        size: f32,
        origin_x: f32,
        origin_y: f32,
        mask: &mut [bool],
        width: usize,
        height: usize,
    ) {
        let Some(cached) = self.cached_glyph(ch, size) else {
            return;
        };
        let glyph = Glyph {
            id: self.font.glyph_id(ch),
            scale: PxScale::from(size.max(1.0)),
            position: point(origin_x, origin_y),
        };
        // 与原实现 outline_glyph(glyph) 相同的构造序列，px_bounds 含真实 position
        let outlined = OutlinedGlyph::new(glyph, cached.outline, cached.scale_factor);
        let bounds = outlined.px_bounds();
        let min_x = bounds.min.x;
        let min_y = bounds.min.y;
        outlined.draw(|gx, gy, coverage| {
            if coverage <= 0.5 {
                return;
            }
            let x = (min_x + gx as f32) as isize;
            let y = (min_y + gy as f32) as isize;
            if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                mask[(y as usize) * width + (x as usize)] = true;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::*;

    /// 返回系统 CJK 字体路径，找不到则跳过测试。
    fn system_font() -> Option<PathBuf> {
        const CANDIDATES: &[&str] = &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
            r"/System/Library/Fonts/PingFang.ttc",
            r"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        for path in CANDIDATES {
            let p = PathBuf::from(*path);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    #[test]
    fn load_and_measure() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let face = FontFace::load(&path, 36.0).expect("字体加载失败");
        let w = face.glyph_width('中', 36.0);
        assert!(w > 0.0, "中文字形宽度应为正");
        assert!(w > 20.0, "36px 中文字宽应接近字号：{w}");
        assert!(face.ascent(36.0) > 0.0);
    }

    #[test]
    fn rasterize_marks_pixels() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let face = FontFace::load(&path, 36.0).expect("字体加载失败");
        let (w, h) = (100usize, 100usize);
        let mut mask = vec![false; w * h];
        face.rasterize('中', 36.0, 30.0, 60.0, &mut mask, w, h);
        assert!(mask.iter().any(|&b| b), "字形应产生前景像素");
    }
}