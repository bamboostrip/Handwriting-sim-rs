//! 笔画扰动：连通区域提取 + 每笔画独立随机旋转/平移后写回画布。
//!
//! 对应 Python 版 `engine_fast._perturb_mask`（scipy.ndimage.label +
//! 向量化坐标变换）的翻译。连通域用 4-连通 flood fill 实现，
//! 扰动公式与 Python 版逐字一致：
//!
//! ```text
//! fx = (x - cx)·cosθ + (y - cy)·sinθ + cx
//! fy = (y - cy)·cosθ - (x - cx)·sinθ + cy   // 注意 sin 项符号
//! nx = round(fx + dx), ny = round(fy + dy)
//! ```

use rand::Rng;
use rand_distr::{Distribution, Normal};

use crate::core::models::HandwritingParams;

/// 一个连通笔画：像素坐标列表 + 包围盒中心。
pub struct Stroke {
    /// (x, y) 像素坐标（x 为列，y 为行）。
    pub pixels: Vec<(usize, usize)>,
    /// 包围盒中心（列方向）。
    pub cx: f32,
    /// 包围盒中心（行方向）。
    pub cy: f32,
}

/// 4-连通区域标记：把前景掩码划分为若干独立笔画。
///
/// 对应 Python 版 `scipy.ndimage.label(mask, structure=4-连通)`。
/// 用 BFS 实现，一次扫描得到每个笔画的像素与包围盒。
pub fn label_strokes(mask: &[bool], width: usize, height: usize) -> Vec<Stroke> {
    let mut visited = vec![false; mask.len()];
    let mut strokes = Vec::new();
    let mut queue = std::collections::VecDeque::new();

    for idx in 0..mask.len() {
        if !mask[idx] || visited[idx] {
            continue;
        }
        // BFS 收集当前连通域
        queue.clear();
        queue.push_back(idx);
        visited[idx] = true;
        let mut pixels = Vec::new();
        let mut min_x = usize::MAX;
        let mut max_x = 0usize;
        let mut min_y = usize::MAX;
        let mut max_y = 0usize;

        while let Some(cur) = queue.pop_front() {
            let x = cur % width;
            let y = cur / width;
            pixels.push((x, y));
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);

            // 4-连通邻居
            if x > 0 {
                let n = cur - 1;
                if mask[n] && !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
            if x + 1 < width {
                let n = cur + 1;
                if mask[n] && !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
            if y > 0 {
                let n = cur - width;
                if mask[n] && !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
            if y + 1 < height {
                let n = cur + width;
                if mask[n] && !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
        }

        strokes.push(Stroke {
            pixels,
            cx: (min_x + max_x) as f32 / 2.0,
            cy: (min_y + max_y) as f32 / 2.0,
        });
    }
    strokes
}

/// 生成 n 个标准差为 sigma 的独立正态扰动。
fn normals(sigma: f32, n: usize, rng: &mut impl Rng) -> Vec<f32> {
    if sigma <= 0.0 || n == 0 {
        return vec![0.0; n];
    }
    let dist = Normal::new(0.0, f64::from(sigma)).unwrap();
    (0..n).map(|_| dist.sample(rng) as f32).collect()
}

/// 对前景掩码按笔画做独立随机扰动并写回画布。
///
/// - `background`：`width * height * 3` 的 RGB 背景缓冲。
/// - 返回同尺寸 RGB 画布（背景副本 + 扰动后的前景色）。
pub fn perturb_mask(
    mask: &[bool],
    width: usize,
    height: usize,
    params: &HandwritingParams,
    rng: &mut impl Rng,
    background: &[u8],
) -> Vec<u8> {
    let mut canvas = Vec::with_capacity(background.len());
    perturb_mask_into(mask, width, height, params, rng, background, &mut canvas);
    canvas
}

/// `perturb_mask` 的复用画布版本：多页渲染时循环外分配一次，避免每页重复分配+拷贝。
/// 输出与 `perturb_mask` 逐位一致。
pub fn perturb_mask_into(
    mask: &[bool],
    width: usize,
    height: usize,
    params: &HandwritingParams,
    rng: &mut impl Rng,
    background: &[u8],
    canvas: &mut Vec<u8>,
) {
    canvas.clear();
    canvas.extend_from_slice(background);
    if !mask.iter().any(|&b| b) {
        return;
    }
    let strokes = label_strokes(mask, width, height);
    let n = strokes.len();
    let dxs = normals(params.perturb_x_sigma, n, rng);
    let dys = normals(params.perturb_y_sigma, n, rng);
    let thetas = normals(params.perturb_theta_sigma, n, rng);

    for (k, stroke) in strokes.iter().enumerate() {
        write_perturbed_stroke(
            stroke,
            dxs[k],
            dys[k],
            thetas[k],
            0,
            0,
            params.fill,
            canvas,
            (width, height),
        );
    }
}

/// 把一个扰动后的笔画以偏移 (ox, oy) 写入整页 RGB 画布。
///
/// 坐标越界（超出页面）的像素忽略；对应 Python 版
/// `_pages_with_regions` 中 `_perturbed_positions` + `canvas[oy+ys, ox+xs]`。
#[allow(clippy::too_many_arguments)]
fn write_perturbed_stroke(
    stroke: &Stroke,
    dx: f32,
    dy: f32,
    theta: f32,
    ox: usize,
    oy: usize,
    fill: [u8; 3],
    canvas: &mut [u8],
    page: (usize, usize),
) {
    let (page_w, page_h) = page;
    let ct = theta.cos();
    let st = theta.sin();
    let cx = stroke.cx;
    let cy = stroke.cy;
    let (fr, fg, fb) = (fill[0], fill[1], fill[2]);
    for &(xs, ys) in &stroke.pixels {
        let xf = xs as f32;
        let yf = ys as f32;
        let fx = (xf - cx) * ct + (yf - cy) * st + cx;
        let fy = (yf - cy) * ct - (xf - cx) * st + cy;
        let nx = (fx + dx).round() as isize + ox as isize;
        let ny = (fy + dy).round() as isize + oy as isize;
        if nx >= 0 && ny >= 0 && (nx as usize) < page_w && (ny as usize) < page_h {
            let dst = (ny as usize) * page_w + (nx as usize);
            canvas[dst * 3] = fr;
            canvas[dst * 3 + 1] = fg;
            canvas[dst * 3 + 2] = fb;
        }
    }
}

/// 区域合成：对区域局部掩码按笔画扰动后，偏移 (ox, oy) 写入整页画布。
///
/// 与 `perturb_mask_into` 的区别：不拷贝背景（画布已含当页底图），
/// 且局部掩码坐标经平移落到整页坐标。扰动公式与主路径逐字一致。
#[allow(clippy::too_many_arguments)]
pub fn perturb_region_into(
    mask: &[bool],
    width: usize,
    height: usize,
    params: &HandwritingParams,
    rng: &mut impl Rng,
    ox: usize,
    oy: usize,
    canvas: &mut [u8],
    page_width: usize,
    page_height: usize,
) {
    if !mask.iter().any(|&b| b) {
        return;
    }
    let strokes = label_strokes(mask, width, height);
    let n = strokes.len();
    let dxs = normals(params.perturb_x_sigma, n, rng);
    let dys = normals(params.perturb_y_sigma, n, rng);
    let thetas = normals(params.perturb_theta_sigma, n, rng);
    for (k, stroke) in strokes.iter().enumerate() {
        write_perturbed_stroke(
            stroke,
            dxs[k],
            dys[k],
            thetas[k],
            ox,
            oy,
            params.fill,
            canvas,
            (page_width, page_height),
        );
    }
}

/// 直接把未扰动的前景掩码以指定颜色写入画布（印刷体专用，零位移零旋转）。
pub fn draw_printed_mask(
    mask: &[bool],
    width: usize,
    height: usize,
    fill: [u8; 3],
    canvas: &mut [u8],
) {
    let (fr, fg, fb) = (fill[0], fill[1], fill[2]);
    let len = width * height;
    for idx in 0..len {
        if mask[idx] {
            canvas[idx * 3] = fr;
            canvas[idx * 3 + 1] = fg;
            canvas[idx * 3 + 2] = fb;
        }
    }
}

/// 对单个样式层的前景掩码做独立笔画扰动并写入画布。
pub fn perturb_styled_layer_into(
    mask: &[bool],
    width: usize,
    height: usize,
    style: &crate::core::layout::PerturbStyle,
    rng: &mut impl Rng,
    canvas: &mut [u8],
) {
    if !mask.iter().any(|&b| b) {
        return;
    }
    if style.is_printed() {
        draw_printed_mask(mask, width, height, style.fill, canvas);
        return;
    }
    let strokes = label_strokes(mask, width, height);
    let n = strokes.len();
    let dxs = normals(style.perturb_x_sigma, n, rng);
    let dys = normals(style.perturb_y_sigma, n, rng);
    let thetas = normals(style.perturb_theta_sigma, n, rng);
    for (k, stroke) in strokes.iter().enumerate() {
        write_perturbed_stroke(
            stroke,
            dxs[k],
            dys[k],
            thetas[k],
            0,
            0,
            style.fill,
            canvas,
            (width, height),
        );
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn params() -> HandwritingParams {
        HandwritingParams {
            perturb_x_sigma: 0.0,
            perturb_y_sigma: 0.0,
            perturb_theta_sigma: 0.05,
            fill: [0, 0, 0],
            ..HandwritingParams::default()
        }
    }

    /// 对齐 Python 版 `test_perturb_rotation_around_own_center`：
    /// 两个远离对角线的笔画，扰动后应保持 2 个连通域且质心位移小。
    #[test]
    fn rotation_around_own_center() {
        let (w, h) = (400usize, 300usize);
        let mut mask = vec![false; w * h];
        for y in 240..260 {
            for x in 40..60 {
                mask[y * w + x] = true;
            }
        }
        for y in 40..60 {
            for x in 340..360 {
                mask[y * w + x] = true;
            }
        }
        let background = vec![255u8; w * h * 3];
        let p = params();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let canvas = perturb_mask(&mask, w, h, &p, &mut rng, &background);

        // 前景像素（黑色）
        let mut out_mask = vec![false; w * h];
        for (i, px) in canvas.chunks_exact(3).enumerate() {
            if px == [0, 0, 0] {
                out_mask[i] = true;
            }
        }
        let strokes_in = label_strokes(&mask, w, h);
        let strokes_out = label_strokes(&out_mask, w, h);
        assert_eq!(strokes_in.len(), 2);
        assert_eq!(strokes_out.len(), 2, "扰动不应合并/拆分笔画");

        // 质心位移应在笔画尺寸量级内（<3px）
        let centroid = |s: &Stroke| -> (f32, f32) {
            let (mut sx, mut sy) = (0.0f64, 0.0f64);
            for &(x, y) in &s.pixels {
                sx += x as f64;
                sy += y as f64;
            }
            let n = s.pixels.len() as f64;
            ((sx / n) as f32, (sy / n) as f32)
        };
        let mut cin: Vec<_> = strokes_in.iter().map(centroid).collect();
        let mut cout: Vec<_> = strokes_out.iter().map(centroid).collect();
        cin.sort_by(|a, b| a.1.total_cmp(&b.1));
        cout.sort_by(|a, b| a.1.total_cmp(&b.1));
        for ((iy, ix), (oy, ox)) in cin.iter().zip(cout.iter()) {
            let dist = ((oy - iy).powi(2) + (ox - ix).powi(2)).sqrt();
            assert!(dist < 3.0, "笔画质心位移过大：{dist}");
        }
    }

    #[test]
    fn empty_mask_returns_background() {
        let (w, h) = (10usize, 10usize);
        let mask = vec![false; w * h];
        let background = vec![255u8; w * h * 3];
        let p = params();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let canvas = perturb_mask(&mask, w, h, &p, &mut rng, &background);
        assert_eq!(canvas, background);
    }

    #[test]
    fn different_seed_different_strokes() {
        let (w, h) = (100usize, 100usize);
        let mut mask = vec![false; w * h];
        for y in 40..60 {
            for x in 40..60 {
                mask[y * w + x] = true;
            }
        }
        let background = vec![255u8; w * h * 3];
        let p = params();
        let a = perturb_mask(&mask, w, h, &p, &mut rand::rngs::StdRng::seed_from_u64(1), &background);
        let b = perturb_mask(&mask, w, h, &p, &mut rand::rngs::StdRng::seed_from_u64(2), &background);
        assert_ne!(a, b, "不同 seed 应产生不同笔画（θ扰动非零）");
    }
}