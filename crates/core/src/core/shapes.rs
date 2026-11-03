//! 符号图形：线条 / 箭头 / 折线 / 矩形 / 椭圆 / 三角形 / 大括号 / 方括号 / 勾 / 下划线。
//!
//! issue #6「增加符号图形」的渲染层：把图形以**手绘风格**光栅化进既有前景
//! `bool` 掩码，与文字、框选区域墨迹共用同一条「掩码 → 连通域扰动 → RGB
//! 画布」管线，不需要在引擎里新增合成路径。
//!
//! 手绘感来源：
//! - 路径顶点、贝塞尔控制点叠加小幅随机噪声（`jitter`，像素）；
//! - 每一小段线宽独立抖动 ±10% 左右，模拟落笔力度变化；
//! - 主笔之外再补一遍整体轻微错位的细线（复描），形成手绘双线；
//! - 椭圆用 8 段二次贝塞尔逼近、起止角过冲一小段（收笔压过起笔）；
//! - 大括号由上下两段三次贝塞尔在中间尖角处相接。
//!
//! 坐标约定：`ShapeSpec` 的坐标是页面像素坐标；`rasterize_shape` 的
//! `offset_x/offset_y` 用于把局部坐标（例如框选区域内部坐标）平移到页面。
//! 越界像素由底层线段/填充光栅化自动裁剪。

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::core::layout::{draw_bezier_line, draw_thick_line};

/// 圆周率（f32）。
const TAU: f32 = std::f32::consts::TAU;

/// 大括号朝向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BraceSide {
    /// `{`：尖角朝左。
    Left,
    /// `}`：尖角朝右。
    Right,
}

/// 方括号朝向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BracketSide {
    /// `[`：开口朝右。
    Left,
    /// `]`：开口朝左。
    Right,
}

/// 图形类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    /// 直线：取 points 前两点，否则取包围盒对角线。
    #[default]
    Line,
    /// 箭头：同直线 + 终点两条箭头短线。
    Arrow,
    /// 折线：按 points 顺序连接（至少两点）。
    Polyline,
    /// 矩形：包围盒 (x, y, w, h)；`filled` 为实心。
    Rect,
    /// 椭圆：包围盒内切；`filled` 为实心。
    Ellipse,
    /// 三角形：默认正立，points ≥ 3 时用指定顶点；`filled` 为实心。
    Triangle,
    /// 大括号：包围盒为整体尺寸，尖角在高度中点。
    Brace(BraceSide),
    /// 方括号：三段折线，开口方向由 side 决定。
    Bracket(BracketSide),
    /// 勾：从左下经拐点拉到右上。
    Check,
    /// 下划线：同直线（通常 h=0），波动更大、更随意。
    Underline,
}

/// 一个符号图形的纯数据描述（可序列化，供前端 / 预设 JSON 使用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeSpec {
    /// 图形类型。
    pub kind: ShapeKind,
    /// 包围盒左上角 x（页面像素）。
    pub x: f32,
    /// 包围盒左上角 y（页面像素）。
    pub y: f32,
    /// 包围盒宽（可为负，内部自动归一化）。
    pub w: f32,
    /// 包围盒高（可为负，内部自动归一化）。
    pub h: f32,
    /// 显式顶点（折线 / 自定义三角形 / 箭头端点等），页面坐标。
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// 线宽（像素）。
    pub thickness: f32,
    /// 墨迹颜色（合成图层用，光栅化到 bool 掩码时不使用）。
    pub fill: [u8; 3],
    /// 手绘抖动幅度（像素）；0 = 规整几何图形。
    pub jitter: f32,
    /// 随机种子：同 seed 逐像素可复现，换 seed 笔画抖动不同。
    pub seed: u64,
    /// 是否填充（Rect / Ellipse / Triangle 生效）。
    #[serde(default)]
    pub filled: bool,
}

impl Default for ShapeSpec {
    fn default() -> Self {
        Self::new(ShapeKind::Line, 0.0, 0.0, 0.0, 0.0)
    }
}

impl ShapeSpec {
    /// 以类型 + 包围盒创建，线宽/抖动/颜色取默认值（黑色、2px、1.4px 抖动）。
    pub fn new(kind: ShapeKind, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            kind,
            x,
            y,
            w,
            h,
            points: Vec::new(),
            thickness: 2.4,
            fill: [0, 0, 0],
            jitter: 1.4,
            seed: 1,
            filled: false,
        }
    }

    /// 设置显式顶点（页面坐标）。
    pub fn with_points(mut self, points: Vec<[f32; 2]>) -> Self {
        self.points = points;
        self
    }

    /// 设置线宽（像素）。
    pub fn with_thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    /// 设置墨迹颜色。
    pub fn with_fill(mut self, fill: [u8; 3]) -> Self {
        self.fill = fill;
        self
    }

    /// 设置手绘抖动幅度（像素，0 = 规整图形）。
    pub fn with_jitter(mut self, jitter: f32) -> Self {
        self.jitter = jitter;
        self
    }

    /// 设置随机种子。
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// 设置是否填充。
    pub fn with_filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }
}

/// 把单个图形光栅化进页面前景掩码。
///
/// - `mask`：页面级 `page_width * page_height` 布尔掩码（与文字墨迹共用）；
/// - `offset_x` / `offset_y`：图形坐标到页面像素的平移量；
/// - 尺寸不足的掩码直接忽略（防御调用方传错），越界像素自动裁剪。
#[allow(clippy::too_many_arguments)]
pub fn rasterize_shape(
    spec: &ShapeSpec,
    mask: &mut [bool],
    page_width: usize,
    page_height: usize,
    offset_x: usize,
    offset_y: usize,
) {
    if page_width == 0 || page_height == 0 || mask.len() < page_width * page_height {
        return;
    }
    let ox = offset_x as f32;
    let oy = offset_y as f32;
    let mut pen = Pen {
        mask,
        width: page_width,
        height: page_height,
        rng: StdRng::seed_from_u64(spec.seed),
        jitter: spec.jitter.max(0.0),
        thickness: spec.thickness.max(0.8),
    };

    let x = spec.x + ox;
    let y = spec.y + oy;
    let (bx, by, bw, bh) = normalized_bbox(x, y, spec.w, spec.h);
    let points: Vec<(f32, f32)> = spec.points.iter().map(|p| (p[0] + ox, p[1] + oy)).collect();
    let amp = spec.jitter.max(0.0);

    match spec.kind {
        ShapeKind::Line => {
            let (a, b) = endpoints(&points, x, y, spec.w, spec.h);
            pen.sketch_line(a, b, amp, true);
        }
        ShapeKind::Arrow => {
            let (a, b) = endpoints(&points, x, y, spec.w, spec.h);
            draw_arrow(&mut pen, a, b, amp);
        }
        ShapeKind::Polyline => {
            if points.len() >= 2 {
                pen.sketch(&points, false, amp);
            } else {
                // 没给折线顶点时退化为包围盒对角线，避免静默丢图
                let (a, b) = endpoints(&points, x, y, spec.w, spec.h);
                pen.sketch_line(a, b, amp, true);
            }
        }
        ShapeKind::Rect => {
            let pts = [
                (bx, by),
                (bx + bw, by),
                (bx + bw, by + bh),
                (bx, by + bh),
            ];
            if spec.filled {
                pen.fill_polygon(&pts);
                pen.path(&pts, true, amp * 0.6, pen.thickness);
            } else {
                pen.sketch(&pts, true, amp);
            }
        }
        ShapeKind::Ellipse => pen.ellipse(bx, by, bw, bh, spec.filled, amp),
        ShapeKind::Triangle => {
            let pts: Vec<(f32, f32)> = if points.len() >= 3 {
                points[..3].to_vec()
            } else {
                vec![
                    (bx + bw * 0.5, by),
                    (bx, by + bh),
                    (bx + bw, by + bh),
                ]
            };
            if spec.filled {
                pen.fill_polygon(&pts);
                pen.path(&pts, true, amp * 0.6, pen.thickness);
            } else {
                pen.sketch(&pts, true, amp);
            }
        }
        ShapeKind::Brace(side) => pen.brace(bx, by, bw, bh, side, amp),
        ShapeKind::Bracket(side) => {
            // 三段折线（横-竖-横），两端略微出锋，像手写时收不住笔
            let over = 2.0 + pen.thickness;
            let pts = match side {
                BracketSide::Left => vec![
                    (bx + bw + over, by),
                    (bx, by),
                    (bx, by + bh),
                    (bx + bw + over, by + bh),
                ],
                BracketSide::Right => vec![
                    (bx - over, by),
                    (bx + bw, by),
                    (bx + bw, by + bh),
                    (bx - over, by + bh),
                ],
            };
            pen.sketch(&pts, false, amp);
        }
        ShapeKind::Check => {
            let pts = [
                (bx, by + bh * 0.55),
                (bx + bw * 0.36, by + bh * 0.95),
                (bx + bw, by + bh * 0.05),
            ];
            pen.path(&pts, false, amp, pen.thickness);
            if pen.jitter > 0.0 {
                // 勾的重描：细线整体略偏，出现复笔层次
                let angle = pen.rng.random_range(0.0..TAU);
                let dist = (pen.thickness * 1.0).clamp(1.4, 2.6);
                let (sx, sy) = (angle.cos() * dist, angle.sin() * dist);
                let ghost: Vec<(f32, f32)> =
                    pts.iter().map(|&(px, py)| (px + sx, py + sy)).collect();
                pen.path(&ghost, false, amp * 1.1, (pen.thickness * 0.4).max(0.8));
            }
        }
        ShapeKind::Underline => {
            let (a, b) = endpoints(&points, x, y, spec.w, spec.h);
            pen.sketch_line(a, b, amp * 1.6, true);
        }
    }
}

/// 把一组图形依次光栅化进页面掩码（无页面偏移的便捷入口）。
pub fn rasterize_shapes(
    specs: &[ShapeSpec],
    mask: &mut [bool],
    page_width: usize,
    page_height: usize,
) {
    for spec in specs {
        rasterize_shape(spec, mask, page_width, page_height, 0, 0);
    }
}

/// 把包围盒归一化为 (左上角 x, 左上角 y, 宽, 高)，负宽高自动翻正。
fn normalized_bbox(x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (
        x.min(x + w),
        y.min(y + h),
        w.abs(),
        h.abs(),
    )
}

/// 直线/箭头/下划线的端点：优先用 points 前两点，否则用包围盒对角线。
fn endpoints(
    points: &[(f32, f32)],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> ((f32, f32), (f32, f32)) {
    if points.len() >= 2 {
        (points[0], points[1])
    } else {
        ((x, y), (x + w, y + h))
    }
}

/// 箭头 = 主笔 + 终点两条箭头短线。
fn draw_arrow(pen: &mut Pen<'_>, a: (f32, f32), b: (f32, f32), amp: f32) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    pen.sketch_line(a, b, amp, true);
    let thickness = pen.thickness;
    // 箭头长度随线宽自适应，短箭头时收缩，避免头部盖满整条线
    let head_len = (thickness * 5.0).max(7.0).min(len * 0.55);
    let base = dy.atan2(dx);
    const HEAD_HALF_ANGLE: f32 = 0.46; // 约 26°，接近手写箭头的自然张开角
    for sign in [-1.0_f32, 1.0] {
        let angle = base + std::f32::consts::PI + sign * HEAD_HALF_ANGLE;
        let tip = (
            b.0 + head_len * angle.cos(),
            b.1 + head_len * angle.sin(),
        );
        pen.line(b, tip, thickness, amp * 0.5);
    }
}

/// 光栅化画笔：持有页面掩码、随机源与手绘参数。
struct Pen<'a> {
    mask: &'a mut [bool],
    width: usize,
    height: usize,
    rng: StdRng,
    /// 抖动幅度（像素）；0 = 规整几何图形。
    jitter: f32,
    /// 基础线宽（像素）。
    thickness: f32,
}

impl Pen<'_> {
    /// 均匀噪声；`amp <= 0` 时返回 0（规整模式不消耗随机数）。
    fn noise(&mut self, amp: f32) -> f32 {
        if amp <= f32::EPSILON {
            0.0
        } else {
            self.rng.random_range(-amp..=amp)
        }
    }

    /// 给顶点加噪。
    fn jittered(&mut self, p: (f32, f32), amp: f32) -> (f32, f32) {
        (p.0 + self.noise(amp), p.1 + self.noise(amp))
    }

    /// 直线：复用 layout::draw_bezier_line 的波状二次贝塞尔。
    fn line(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32, amp: f32) {
        let a = self.jittered(a, amp * 0.5);
        let b = self.jittered(b, amp * 0.5);
        draw_bezier_line(
            self.mask,
            self.width,
            self.height,
            a.0,
            a.1,
            b.0,
            b.1,
            thickness,
            amp * 0.7,
            &mut self.rng,
        );
    }

    /// 手绘直线：主笔 + 一遍整体错位的复描细线。
    fn sketch_line(&mut self, a: (f32, f32), b: (f32, f32), amp: f32, double: bool) {
        let thickness = self.thickness;
        self.line(a, b, thickness, amp);
        if double && self.jitter > 0.0 {
            let angle = self.rng.random_range(0.0..TAU);
            let dist = (thickness * 1.15).clamp(1.6, 3.2);
            let (sx, sy) = (angle.cos() * dist, angle.sin() * dist);
            self.line(
                (a.0 + sx, a.1 + sy),
                (b.0 + sx, b.1 + sy),
                (thickness * 0.42).max(0.8),
                amp * 1.2,
            );
        }
    }

    /// 二次贝塞尔段：分小段用 draw_thick_line 光栅化，每段线宽独立抖动。
    fn quad(&mut self, p0: (f32, f32), c: (f32, f32), p1: (f32, f32), thickness: f32) {
        const STEPS: usize = 8;
        let mut prev = p0;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let mt = 1.0 - t;
            let x = mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0;
            let y = mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1;
            let th = if self.jitter > 0.0 {
                thickness * self.rng.random_range(0.9..1.1)
            } else {
                thickness
            };
            draw_thick_line(self.mask, self.width, self.height, prev.0, prev.1, x, y, th);
            prev = (x, y);
        }
    }

    /// 三次贝塞尔段（大括号用）：控制点加噪，同样做线宽抖动。
    #[allow(clippy::too_many_arguments)]
    fn cubic(
        &mut self,
        p0: (f32, f32),
        c1: (f32, f32),
        c2: (f32, f32),
        p1: (f32, f32),
        thickness: f32,
        amp: f32,
    ) {
        const STEPS: usize = 16;
        let c1 = self.jittered(c1, amp * 0.7);
        let c2 = self.jittered(c2, amp * 0.7);
        let mut prev = p0;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let mt = 1.0 - t;
            let w0 = mt * mt * mt;
            let w1 = 3.0 * mt * mt * t;
            let w2 = 3.0 * mt * t * t;
            let w3 = t * t * t;
            let x = w0 * p0.0 + w1 * c1.0 + w2 * c2.0 + w3 * p1.0;
            let y = w0 * p0.1 + w1 * c1.1 + w2 * c2.1 + w3 * p1.1;
            let th = if self.jitter > 0.0 {
                thickness * self.rng.random_range(0.9..1.1)
            } else {
                thickness
            };
            draw_thick_line(self.mask, self.width, self.height, prev.0, prev.1, x, y, th);
            prev = (x, y);
        }
    }

    /// 折线/多边形：顶点加噪后逐段用二次贝塞尔连接（控制点沿法线随机偏移）。
    fn path(&mut self, points: &[(f32, f32)], closed: bool, amp: f32, thickness: f32) {
        if points.len() < 2 {
            return;
        }
        let pts: Vec<(f32, f32)> = points.iter().map(|&p| self.jittered(p, amp)).collect();
        let segs = if closed { pts.len() } else { pts.len() - 1 };
        for i in 0..segs {
            let p0 = pts[i];
            let p1 = pts[(i + 1) % pts.len()];
            let dx = p1.0 - p0.0;
            let dy = p1.1 - p0.1;
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            // 手写线很少是绝对直的：中点沿法线随机弓一下
            let bow = self.noise(amp * 0.7);
            let c = (
                (p0.0 + p1.0) * 0.5 - dy / len * bow,
                (p0.1 + p1.1) * 0.5 + dx / len * bow,
            );
            self.quad(p0, c, p1, thickness);
        }
    }

    /// 手绘折线：主笔 + 整体错位的复描细线（制造双线/复笔层次）。
    fn sketch(&mut self, points: &[(f32, f32)], closed: bool, amp: f32) {
        let thickness = self.thickness;
        self.path(points, closed, amp, thickness);
        if self.jitter > 0.0 {
            let angle = self.rng.random_range(0.0..TAU);
            let dist = (thickness * 1.15).clamp(1.6, 3.2);
            let (sx, sy) = (angle.cos() * dist, angle.sin() * dist);
            let ghost: Vec<(f32, f32)> = points
                .iter()
                .map(|&(x, y)| (x + sx, y + sy))
                .collect();
            self.path(&ghost, closed, amp * 1.25, (thickness * 0.42).max(0.8));
        }
    }

    /// 椭圆弧：8 段二次贝塞尔逼近，控制点取相邻切线交点。
    #[allow(clippy::too_many_arguments)]
    fn ellipse_arcs(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        start: f32,
        sweep: f32,
        amp: f32,
        thickness: f32,
    ) {
        const SEGS: usize = 8;
        for i in 0..SEGS {
            let a0 = start + sweep * i as f32 / SEGS as f32;
            let a1 = start + sweep * (i + 1) as f32 / SEGS as f32;
            let am = (a0 + a1) * 0.5;
            let half = (a1 - a0) * 0.5;
            // 二次贝塞尔逼近圆弧：切点半径放大 1/cos(半角)
            let k = 1.0 / half.cos().max(0.2);
            let p0 = (cx + rx * a0.cos(), cy + ry * a0.sin());
            let p1 = (cx + rx * a1.cos(), cy + ry * a1.sin());
            let c = (
                cx + rx * am.cos() * k + self.noise(amp * 0.6),
                cy + ry * am.sin() * k + self.noise(amp * 0.6),
            );
            self.quad(p0, c, p1, thickness);
        }
    }

    /// 手绘椭圆：起笔角随机、收笔角过冲一小段，再由内向外补一遍细复描。
    fn ellipse(&mut self, x: f32, y: f32, w: f32, h: f32, filled: bool, amp: f32) {
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let rx = (w * 0.5).max(0.5);
        let ry = (h * 0.5).max(0.5);
        let thickness = self.thickness;
        if filled {
            let poly = ellipse_polygon(cx, cy, rx, ry, 48);
            self.fill_polygon(&poly);
        }
        let (start, over) = if self.jitter > 0.0 {
            (
                self.rng.random_range(0.0..TAU),
                self.rng.random_range(0.10..0.30),
            )
        } else {
            (0.0, 0.0)
        };
        self.ellipse_arcs(cx, cy, rx, ry, start, TAU + over, amp, thickness);
        if !filled && self.jitter > 0.0 {
            // 复描：略大一圈的细线，从另一处起笔
            let inset = (thickness * 0.8 + 0.8).clamp(1.8, 4.0);
            let start2 = start + self.rng.random_range(0.5..1.5);
            self.ellipse_arcs(
                cx,
                cy,
                rx + inset,
                ry + inset,
                start2,
                TAU + 0.15,
                amp * 1.2,
                (thickness * 0.4).max(0.8),
            );
        }
    }

    /// 大括号：上下两段三次贝塞尔在高度中点相接，接点即中间尖角。
    fn brace(&mut self, x: f32, y: f32, w: f32, h: f32, side: BraceSide, amp: f32) {
        let thickness = self.thickness;
        // dir 指向尖角方向：右括号从左侧起笔、尖角在右；左括号镜像
        let dir = match side {
            BraceSide::Right => 1.0,
            BraceSide::Left => -1.0,
        };
        let near_x = if dir > 0.0 { x } else { x + w };
        let cusp_x = if dir > 0.0 { x + w } else { x };
        let spine_x = near_x + dir * w * 0.55;
        let top = (near_x, y);
        let cusp = (cusp_x, y + h * 0.5);
        let bottom = (near_x, y + h);

        // 上半段：起笔 → 竖脊 → 中间尖角
        self.cubic(
            top,
            (spine_x, y + h * 0.02),
            (spine_x, y + h * 0.44),
            cusp,
            thickness,
            amp,
        );
        // 下半段：中间尖角 → 竖脊 → 收笔
        self.cubic(
            cusp,
            (spine_x, y + h * 0.56),
            (spine_x, y + h * 0.98),
            bottom,
            thickness,
            amp,
        );
    }

    /// 扫描线填充多边形（奇偶规则），坐标越界自动裁剪。
    fn fill_polygon(&mut self, points: &[(f32, f32)]) {
        if points.len() < 3 {
            return;
        }
        let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let y_lo = (min_y.floor() as isize).max(0);
        let y_hi = (max_y.ceil() as isize).min(self.height as isize - 1);
        let mut xs: Vec<f32> = Vec::with_capacity(points.len());
        for y in y_lo..=y_hi {
            let scan_y = y as f32 + 0.5;
            xs.clear();
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                if (a.1 <= scan_y) != (b.1 <= scan_y) {
                    let t = (scan_y - a.1) / (b.1 - a.1);
                    xs.push(a.0 + t * (b.0 - a.0));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.as_chunks::<2>().0 {
                let x_lo = pair[0].ceil().max(0.0) as isize;
                let x_hi = pair[1].floor().min(self.width as f32 - 1.0) as isize;
                for px in x_lo..=x_hi {
                    let idx = y as usize * self.width + px as usize;
                    self.mask[idx] = true;
                }
            }
        }
    }
}

/// 椭圆的多边形采样（填充用）。
fn ellipse_polygon(cx: f32, cy: f32, rx: f32, ry: f32, segments: usize) -> Vec<(f32, f32)> {
    (0..segments)
        .map(|i| {
            let a = TAU * i as f32 / segments as f32;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(width: usize, height: usize) -> Vec<bool> {
        vec![false; width * height]
    }

    fn lit(mask: &[bool]) -> usize {
        mask.iter().filter(|&&b| b).count()
    }

    fn sample(kind: ShapeKind) -> ShapeSpec {
        ShapeSpec::new(kind, 40.0, 40.0, 160.0, 100.0)
            .with_thickness(2.4)
            .with_jitter(1.4)
            .with_seed(7)
            .with_points(vec![[60.0, 60.0], [140.0, 120.0], [200.0, 70.0]])
    }

    /// 所有图形类型都应产生前景像素（防止某分支画了个寂寞）。
    #[test]
    fn every_kind_marks_pixels() {
        let (w, h) = (320usize, 240usize);
        let kinds = [
            ShapeKind::Line,
            ShapeKind::Arrow,
            ShapeKind::Polyline,
            ShapeKind::Rect,
            ShapeKind::Ellipse,
            ShapeKind::Triangle,
            ShapeKind::Brace(BraceSide::Left),
            ShapeKind::Brace(BraceSide::Right),
            ShapeKind::Bracket(BracketSide::Left),
            ShapeKind::Bracket(BracketSide::Right),
            ShapeKind::Check,
            ShapeKind::Underline,
        ];
        for kind in kinds {
            let mut mask = blank(w, h);
            let spec = sample(kind);
            rasterize_shape(&spec, &mut mask, w, h, 0, 0);
            assert!(lit(&mask) > 20, "{kind:?} 应产生足量前景像素");
        }
    }

    /// 同 seed 逐像素一致，换 seed 抖动不同。
    #[test]
    fn seeded_and_reproducible() {
        let (w, h) = (300usize, 200usize);
        let spec = sample(ShapeKind::Ellipse);
        let mut a = blank(w, h);
        let mut b = blank(w, h);
        rasterize_shape(&spec, &mut a, w, h, 0, 0);
        rasterize_shape(&spec, &mut b, w, h, 0, 0);
        assert_eq!(a, b, "同 seed 应逐像素一致");

        let mut other = blank(w, h);
        rasterize_shape(&spec.clone().with_seed(99), &mut other, w, h, 0, 0);
        assert_ne!(a, other, "不同 seed 的手绘抖动应不同");
    }

    /// 页面偏移应整体平移图形（框选区域局部坐标合成依赖此行为）。
    #[test]
    fn offset_shifts_shape() {
        let (w, h) = (400usize, 300usize);
        let spec = sample(ShapeKind::Rect).with_seed(3);
        let mut mask = blank(w, h);
        rasterize_shape(&spec, &mut mask, w, h, 120, 80);
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        for y in 0..h {
            for x in 0..w {
                if mask[y * w + x] {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                }
            }
        }
        assert!(min_x >= 120, "偏移后最小 x 应 >= 120，实际 {min_x}");
        assert!(min_y >= 80, "偏移后最小 y 应 >= 80，实际 {min_y}");
    }

    /// 实心三角形内部应被填满。
    #[test]
    fn filled_triangle_covers_interior() {
        let (w, h) = (200usize, 160usize);
        let spec = ShapeSpec::new(ShapeKind::Triangle, 40.0, 30.0, 120.0, 100.0)
            .with_filled(true)
            .with_jitter(0.0)
            .with_seed(5);
        let mut mask = blank(w, h);
        rasterize_shape(&spec, &mut mask, w, h, 0, 0);
        // 三角形重心附近应被填充
        assert!(mask[110 * w + 100], "实心三角形内部应被填充");
        assert!(!mask[35 * w + 45], "三角形外接框左上角不应被填充");
    }

    /// 抖动为 0 时图形应规整（矩形角点附近不越界）。
    #[test]
    fn zero_jitter_rect_stays_in_bbox() {
        let (w, h) = (240usize, 200usize);
        let spec = ShapeSpec::new(ShapeKind::Rect, 50.0, 40.0, 120.0, 80.0)
            .with_jitter(0.0)
            .with_seed(1);
        let mut mask = blank(w, h);
        rasterize_shape(&spec, &mut mask, w, h, 0, 0);
        let mut max_x = 0;
        let mut max_y = 0;
        for y in 0..h {
            for x in 0..w {
                if mask[y * w + x] {
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        // 允许线宽带来的半个像素级溢出，但不应漂出包围盒
        assert!(max_x <= 172, "规整矩形不应超出包围盒右边，实际 {max_x}");
        assert!(max_y <= 122, "规整矩形不应超出包围盒下边，实际 {max_y}");
    }
}
