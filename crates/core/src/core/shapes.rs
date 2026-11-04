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

use crate::core::layout::draw_thick_line;

/// 圆周率（f32）。
const TAU: f32 = std::f32::consts::TAU;

/// 线宽上限（像素）：`draw_thick_line` 的内层循环按线宽展开扫描，
/// 无穷大/夸张线宽会让它跑近乎无限次，这里兜底夹住（校验层另有量级检查）。
pub(crate) const MAX_PEN_THICKNESS: f32 = 256.0;
/// 抖动上限（像素）：抖动会撑大几何跨度，同样需要上限防止扫描步数爆炸。
pub(crate) const MAX_PEN_JITTER: f32 = 256.0;

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

/// 图形默认所在页（1 基）：与 `TextRegion.page` 同语义，默认第 1 页。
fn default_page() -> i32 {
    1
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

impl ShapeKind {
    /// 稳定字符串 id（前端 / 命令层契约，见 `docs/research/issue6-shape-mvp-contract.md` §1）。
    ///
    /// 元组变体在这里被扁平化为 `"brace_left"` 等字符串，与前端 `UiShapeKind`
    /// 的字符串联合类型逐字一致；core 内部仍保留元组变体以便渲染分支类型安全。
    pub fn as_id(&self) -> &'static str {
        match self {
            ShapeKind::Line => "line",
            ShapeKind::Arrow => "arrow",
            ShapeKind::Polyline => "polyline",
            ShapeKind::Rect => "rect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Triangle => "triangle",
            ShapeKind::Brace(BraceSide::Left) => "brace_left",
            ShapeKind::Brace(BraceSide::Right) => "brace_right",
            ShapeKind::Bracket(BracketSide::Left) => "bracket_left",
            ShapeKind::Bracket(BracketSide::Right) => "bracket_right",
            ShapeKind::Check => "check",
            ShapeKind::Underline => "underline",
        }
    }

    /// 由稳定字符串 id 解析图形类型；未知 id 返回 `None`（调用方跳过该图形）。
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "line" => ShapeKind::Line,
            "arrow" => ShapeKind::Arrow,
            "polyline" => ShapeKind::Polyline,
            "rect" => ShapeKind::Rect,
            "ellipse" => ShapeKind::Ellipse,
            "triangle" => ShapeKind::Triangle,
            "brace_left" => ShapeKind::Brace(BraceSide::Left),
            "brace_right" => ShapeKind::Brace(BraceSide::Right),
            "bracket_left" => ShapeKind::Bracket(BracketSide::Left),
            "bracket_right" => ShapeKind::Bracket(BracketSide::Right),
            "check" => ShapeKind::Check,
            "underline" => ShapeKind::Underline,
            _ => return None,
        })
    }
}

/// 一个符号图形的纯数据描述（可序列化，供命令层镜像与工程 JSON 使用；
/// 注意预设 JSON v2 的白名单**不含图形**，见 presets.rs）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeSpec {
    /// 图形类型。
    pub kind: ShapeKind,
    /// 所在页（1 基）；默认第 1 页。用于多页合成与总页数计算。
    #[serde(default = "default_page")]
    pub page: i32,
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
    /// 墨迹颜色覆盖；`None` = 跟随主墨色 `HandwritingParams.fill`。
    /// （光栅化到 bool 掩码时不使用，由引擎合成层决定颜色。）
    #[serde(default)]
    pub fill: Option<[u8; 3]>,
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
    /// 以类型 + 包围盒创建，线宽/抖动/颜色取默认值
    /// （第 1 页、跟随主墨色、2.4px、1.4px 抖动）。
    pub fn new(kind: ShapeKind, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            kind,
            page: 1,
            x,
            y,
            w,
            h,
            points: Vec::new(),
            thickness: 2.4,
            fill: None,
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

    /// 设置墨迹颜色覆盖（内部写 `Some`）。要恢复「跟随主墨色」请直接把 `fill` 设回 `None`。
    pub fn with_fill(mut self, fill: [u8; 3]) -> Self {
        self.fill = Some(fill);
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

    /// 设置所在页（1 基）。命令层会把 `0`/负数归一化为 1；core 校验会直接拒绝 `< 1`。
    pub fn with_page(mut self, page: i32) -> Self {
        self.page = page;
        self
    }
}

/// 把单个图形光栅化进页面前景掩码。
///
/// - `mask`：页面级 `page_width * page_height` 布尔掩码（与文字墨迹共用）；
/// - `offset_x` / `offset_y`：图形坐标到页面像素的平移量；
/// - 尺寸不足的掩码直接忽略（防御调用方传错），越界像素自动裁剪。
///
/// 防御性上限：坐标/线宽/抖动在 `validate_with` 里已经限定在合理量级，
/// 这里再夹一次并做「包围盒与画布不相交则早退」——
/// 否则调用方绕过校验时，`draw_thick_line` 会按坐标跨度/线宽生成天量的扫描步数（等于卡死）。
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
    if !spec.thickness.is_finite() || !spec.jitter.is_finite() {
        return;
    }
    let ox = offset_x as f32;
    let oy = offset_y as f32;
    let thickness = spec.thickness.clamp(0.8, MAX_PEN_THICKNESS);
    let jitter = spec.jitter.clamp(0.0, MAX_PEN_JITTER);
    let mut rng = StdRng::seed_from_u64(spec.seed);
    // 笔压相位：让同一图形的不同笔画宽窄起伏各不相同
    let phase = rng.random_range(0.0..TAU);
    let x = spec.x + ox;
    let y = spec.y + oy;
    let (bx, by, bw, bh) = normalized_bbox(x, y, spec.w, spec.h);
    // 整体轻微倾斜 + 微位移：手画的框/圈几乎不可能绝对轴对齐，
    // 这是「看起来像打印」的主要观感来源之一。jitter=0 时保持规整（且不消耗随机数）。
    let half_diag = (bw * bw + bh * bh).sqrt() * 0.5;
    // 倾斜按「端点最大位移 ≈3px」封顶：否则 1000px 宽的长框会被转出 20+px 的斜边，
    // 框住文字时看起来反而像画歪了（手画长框其实很接近水平）
    let rot_cap = if half_diag > 1.0 { 3.0 / half_diag } else { 0.03 };
    let (rot, tx, ty) = if jitter > 0.0 {
        (
            rng.random_range(-rot_cap..=rot_cap),
            rng.random_range(-thickness * 0.25..=thickness * 0.25),
            rng.random_range(-thickness * 0.25..=thickness * 0.25),
        )
    } else {
        (0.0, 0.0, 0.0)
    };
    let mut pen = Pen {
        mask,
        width: page_width,
        height: page_height,
        rng,
        jitter,
        thickness,
        phase,
        rot,
        tx,
        ty,
        cx: bx + bw * 0.5,
        cy: by + bh * 0.5,
    };
    let points: Vec<(f32, f32)> = spec.points.iter().map(|p| (p[0] + ox, p[1] + oy)).collect();
    let amp = jitter;

    // 早退：图形的有效包围盒（含显式顶点、线宽与抖动余量）与画布不相交时直接跳过，
    // 避免大坐标/离屏图形触发无谓的巨量扫描。
    let margin = thickness + jitter * 2.0 + 4.0;
    let (mut min_x, mut max_x) = (bx, bx + bw);
    let (mut min_y, mut max_y) = (by, by + bh);
    for (px, py) in &points {
        min_x = min_x.min(*px);
        max_x = max_x.max(*px);
        min_y = min_y.min(*py);
        max_y = max_y.max(*py);
    }
    if !min_x.is_finite() || !max_x.is_finite() || !min_y.is_finite() || !max_y.is_finite() {
        return;
    }
    if max_x < -margin
        || max_y < -margin
        || min_x > page_width as f32 + margin
        || min_y > page_height as f32 + margin
    {
        return;
    }

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
            let over = pen.thickness * 1.5 + 0.8;
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
                let dist = (pen.thickness * 1.0).max(0.8);
                let (sx, sy) = (angle.cos() * dist, angle.sin() * dist);
                let ghost: Vec<(f32, f32)> =
                    pts.iter().map(|&(px, py)| (px + sx, py + sy)).collect();
                pen.path(&ghost, false, amp * 1.1, (pen.thickness * 0.4).max(0.5));
            }
        }
        ShapeKind::Underline => {
            let (a, b) = endpoints(&points, x, y, spec.w, spec.h);
            pen.sketch_line(a, b, amp * 1.6, true);
        }
    }
}

/// 计算图形在页面内的**局部掩码包围盒**（用于逐图形合成时只在自身区域内分配掩码）。
///
/// 返回 `(x, y, w, h)`（页面像素，已钳制到页面内）；图形完全在页面外、退化尺寸
/// 或坐标非有限时返回 `None`。外扩余量覆盖线宽、抖动与复描/箭头等笔画外溢。
pub fn local_mask_box(
    spec: &ShapeSpec,
    page_width: usize,
    page_height: usize,
) -> Option<(usize, usize, usize, usize)> {
    if page_width == 0 || page_height == 0 {
        return None;
    }
    let (bx, by, bw, bh) = normalized_bbox(spec.x, spec.y, spec.w, spec.h);
    let (mut min_x, mut max_x) = (bx, bx + bw);
    let (mut min_y, mut max_y) = (by, by + bh);
    for p in &spec.points {
        min_x = min_x.min(p[0]);
        max_x = max_x.max(p[0]);
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    }
    if ![min_x, max_x, min_y, max_y].iter().all(|v| v.is_finite()) {
        return None;
    }
    let margin = spec.thickness.clamp(0.8, MAX_PEN_THICKNESS) * 2.0
        + spec.jitter.clamp(0.0, MAX_PEN_JITTER) * 3.0
        // 整体倾斜最多约 0.012rad，长边外扩量按对角线估
        + 0.02 * (bw * bw + bh * bh).sqrt()
        + 8.0;
    let x0f = (min_x - margin).floor().max(0.0);
    let y0f = (min_y - margin).floor().max(0.0);
    let x1f = (max_x + margin).ceil().min(page_width as f32);
    let y1f = (max_y + margin).ceil().min(page_height as f32);
    if x1f <= x0f || y1f <= y0f {
        return None;
    }
    let (x0, y0) = (x0f as usize, y0f as usize);
    let (x1, y1) = (x1f as usize, y1f as usize);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
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
    let head_len = (thickness * 5.0).min(len * 0.55).max(2.0);
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
    /// 笔压相位：让同一图形的不同笔画宽窄起伏互不相同（手写时每笔力度都不一样）。
    phase: f32,
    /// 整体轻微旋转（弧度，绕包围盒中心）：手画的框几乎不会绝对轴对齐
    rot: f32,
    /// 整体微位移（像素）
    tx: f32,
    ty: f32,
    /// 旋转中心（包围盒中心）
    cx: f32,
    cy: f32,
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

    /// 把图形局部坐标映射到页面坐标（整体旋转 + 平移）。
    fn xform(&self, p: (f32, f32)) -> (f32, f32) {
        if self.rot == 0.0 && self.tx == 0.0 && self.ty == 0.0 {
            return p;
        }
        let (dx, dy) = (p.0 - self.cx, p.1 - self.cy);
        let (c, s) = (self.rot.cos(), self.rot.sin());
        (
            self.cx + dx * c - dy * s + self.tx,
            self.cy + dx * s + dy * c + self.ty,
        )
    }

    /// 画一段（先套整体变换再光栅化）。
    ///
    /// `jitter > 0` 时额外给两端沿法线加一点「毛边」偏移：笔尖在纸上不是
    /// 几何直线，边缘会有轻微抖动——这是二进制掩码下最接近真实笔迹的做法。
    fn seg(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32) {
        let a = self.xform(a);
        let mut b = self.xform(b);
        if self.jitter > 0.0 {
            let dx = b.0 - a.0;
            let dy = b.1 - a.1;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.5 {
                let rag = self.thickness * 0.08;
                let n = self.rng.random_range(-rag..=rag);
                let (nx, ny) = (-dy / len * n, dx / len * n);
                b = (b.0 + nx, b.1 + ny);
            }
        }
        draw_thick_line(self.mask, self.width, self.height, a.0, a.1, b.0, b.1, thickness);
    }

    /// 笔压：沿笔画长度方向平滑变化的线宽系数。
    ///
    /// 真实手写几乎不存在「全程等宽」的线——起笔轻、行笔重、收笔再轻，
    /// 再叠加一点低频起伏（写字时手劲不会突然变，但会缓慢波动）。
    /// `taper` 为两端收细的比例（0 = 不收细，用于闭合曲线如椭圆）。
    fn pressure(&self, t: f32, taper: f32) -> f32 {
        if self.jitter <= f32::EPSILON {
            return 1.0; // jitter=0 = 规整模式（直尺效果），不做笔压起伏
        }
        let pi = std::f32::consts::PI;
        // 起收笔收细，中段最粗
        let taper_f = 1.0 - taper * (1.0 - (pi * t).sin());
        // 低频力度起伏（相位随图形/笔画不同）
        let press = 1.0 + 0.28 * (self.phase + t * 9.4).sin();
        (taper_f * press).max(0.55)
    }

    /// 手写直线：沿法线叠加「一次弓形 + 一次 S 形」的低频摆动，并按笔压逐小段变宽变窄。
    ///
    /// 用「逐小段 draw_thick_line」而不是复用 `draw_bezier_line`，是因为后者全程等宽
    /// ——这正是手绘符号看起来像打印的根因。摆动用两个不同频率的正弦叠加，
    /// 比单一半圆弧更像「手腕自然带出来的轻微 S 形」，不会像尺子画的直线。
    fn line(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32, amp: f32) {
        let a = self.jittered(a, amp * 0.5);
        let b = self.jittered(b, amp * 0.5);
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.6 {
            self.seg(a, a, thickness);
            return;
        }
        // 长笔画摆动更明显：手写长直线肉眼可见弧度，短笔画几乎笔直
        // 手写长线 = 「整体轻微弓起」+「若干个小波浪」：
        // 只有弓会像香蕉，只有高频抖动又像锯齿，两者叠加才像手腕自然带出来的线。
        let (arch_amp, wave_amp) = if self.jitter > f32::EPSILON {
            (
                (len * 0.004).min(4.0) + amp * 0.35,
                amp * 0.7 + (len * 0.002).min(2.0),
            )
        } else {
            (0.0, 0.0)
        };
        let bow = self.noise(arch_amp);
        let wob = self.noise(wave_amp);
        let nx = -dy / len;
        let ny = dx / len;
        let pi = std::f32::consts::PI;

        let steps = ((len / 9.0).ceil() as usize).clamp(3, 48);
        let mut prev = a;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let base = (a.0 + dx * t, a.1 + dy * t);
            let off = bow * (pi * t).sin() + wob * (TAU * 2.5 * t + self.phase).sin();
            let p = (base.0 + nx * off, base.1 + ny * off);
            let w = thickness * self.pressure(t, 0.45);
            self.seg(prev, p, w);
            prev = p;
        }
    }

    /// 手绘直线：主笔 + 一遍整体错位的复描细线。
    fn sketch_line(&mut self, a: (f32, f32), b: (f32, f32), amp: f32, double: bool) {
        let thickness = self.thickness;
        self.line(a, b, thickness, amp);
        if double && self.jitter > 0.0 {
            let angle = self.rng.random_range(0.0..TAU);
            let dist = (thickness * 0.38).max(0.4);
            let (sx, sy) = (angle.cos() * dist, angle.sin() * dist);
            self.line(
                (a.0 + sx, a.1 + sy),
                (b.0 + sx, b.1 + sy),
                (thickness * 0.38).max(0.4),
                amp * 1.2,
            );
        }
    }

    /// 二次贝塞尔段：分小段用 draw_thick_line 光栅化，每段线宽独立抖动。
    fn quad(&mut self, p0: (f32, f32), c: (f32, f32), p1: (f32, f32), thickness: f32, taper: f32) {
        const STEPS: usize = 8;
        let mut prev = p0;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let mt = 1.0 - t;
            let x = mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0;
            let y = mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1;
            let jitter_k = if self.jitter > 0.0 {
                self.rng.random_range(0.95..1.05)
            } else {
                1.0
            };
            let th = thickness * self.pressure(t, taper) * jitter_k;
            self.seg(prev, (x, y), th);
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
        taper: f32,
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
            let jitter_k = if self.jitter > 0.0 {
                self.rng.random_range(0.95..1.05)
            } else {
                1.0
            };
            let th = thickness * self.pressure(t, taper) * jitter_k;
            self.seg(prev, (x, y), th);
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
            let bow_amp = if self.jitter > f32::EPSILON {
                amp * 0.5 + (len * 0.004).min(4.0)
            } else {
                0.0
            };
            let bow = self.noise(bow_amp);
            let c = (
                (p0.0 + p1.0) * 0.5 - dy / len * bow,
                (p0.1 + p1.1) * 0.5 + dx / len * bow,
            );
            // 手写转角会「过头」或「欠笔」：每段两端沿方向各延伸一小段随机量，
            // 使转角出现交叉/缝隙，而不是数字化的完美尖角
            // 过头量随线宽与边长自适应（手写转角交叉/缝隙），规整模式不做
            let over_amp = if self.jitter > f32::EPSILON {
                (thickness * 1.6).min(len * 0.05)
            } else {
                0.0
            };
            let over0 = self.noise(over_amp);
            let over1 = self.noise(over_amp);
            let ux = dx / len;
            let uy = dy / len;
            let a = (p0.0 - ux * over0, p0.1 - uy * over0);
            let b = (p1.0 + ux * over1, p1.1 + uy * over1);
            self.quad(a, c, b, thickness, 0.38);
        }
    }

    /// 手绘折线：主笔 + 整体错位的复描细线（制造双线/复笔层次）。
    fn sketch(&mut self, points: &[(f32, f32)], closed: bool, amp: f32) {
        let thickness = self.thickness;
        self.path(points, closed, amp, thickness);
        if self.jitter > 0.0 {
            let angle = self.rng.random_range(0.0..TAU);
            let dist = (thickness * 0.38).max(0.4);
            let (sx, sy) = (angle.cos() * dist, angle.sin() * dist);
            let ghost: Vec<(f32, f32)> = points
                .iter()
                .map(|&(x, y)| (x + sx, y + sy))
                .collect();
            self.path(&ghost, closed, amp * 1.25, (thickness * 0.38).max(0.4));
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
            let rad = (rx + ry) * 0.5;
            let dev_amp = if self.jitter > f32::EPSILON {
                amp * 0.6 + rad * 0.012
            } else {
                0.0
            };
            let c = (
                cx + rx * am.cos() * k + self.noise(dev_amp),
                cy + ry * am.sin() * k + self.noise(dev_amp),
            );
            // 闭合曲线没有起收笔，但手画的圈仍有粗细起伏：按角度做低频笔压变化，
            // 否则椭圆会像矢量圆规画出来的。
            let wob = if self.jitter > f32::EPSILON {
                1.0 + 0.20 * (am * 2.0 + self.phase).sin()
            } else {
                1.0
            };
            self.quad(p0, c, p1, thickness * wob, 0.0);
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
            let inset = (thickness * 0.8 + 0.8).max(0.8);
            let start2 = start + self.rng.random_range(0.5..1.5);
            self.ellipse_arcs(
                cx,
                cy,
                rx + inset,
                ry + inset,
                start2,
                TAU + 0.15,
                amp * 1.2,
                (thickness * 0.4).max(0.5),
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
            0.30,
        );
        // 下半段：中间尖角 → 竖脊 → 收笔
        self.cubic(
            cusp,
            (spine_x, y + h * 0.56),
            (spine_x, y + h * 0.98),
            bottom,
            thickness,
            amp,
            0.30,
        );
    }

    /// 扫描线填充多边形（奇偶规则），坐标越界自动裁剪。
    fn fill_polygon(&mut self, points: &[(f32, f32)]) {
        if points.len() < 3 {
            return;
        }
        // 填充多边形也要套整体倾斜/位移，否则填充与描边会错位
        let points: Vec<(f32, f32)> = points.iter().map(|&p| self.xform(p)).collect();
        let points = &points[..];
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

    /// 12 个稳定 id 与变体一一对应（前端/命令层契约，字符串逐字冻结）。
    #[test]
    fn kind_ids_match_contract() {
        let kinds = [
            (ShapeKind::Line, "line"),
            (ShapeKind::Arrow, "arrow"),
            (ShapeKind::Polyline, "polyline"),
            (ShapeKind::Rect, "rect"),
            (ShapeKind::Ellipse, "ellipse"),
            (ShapeKind::Triangle, "triangle"),
            (ShapeKind::Brace(BraceSide::Left), "brace_left"),
            (ShapeKind::Brace(BraceSide::Right), "brace_right"),
            (ShapeKind::Bracket(BracketSide::Left), "bracket_left"),
            (ShapeKind::Bracket(BracketSide::Right), "bracket_right"),
            (ShapeKind::Check, "check"),
            (ShapeKind::Underline, "underline"),
        ];
        for (kind, id) in kinds {
            assert_eq!(kind.as_id(), id);
            assert_eq!(ShapeKind::from_id(id), Some(kind));
        }
        assert_eq!(ShapeKind::from_id("unknown"), None);
        assert_eq!(ShapeKind::from_id("Brace"), None, "id 大小写敏感");
    }

    /// 旧 JSON（无 page/fill/filled/points）应能反序列化：
    /// page 默认 1、fill 默认 None（跟随主墨色）、with_fill 写 Some。
    #[test]
    fn spec_json_defaults_page_and_fill() {
        let spec: ShapeSpec = serde_json::from_str(
            r#"{"kind":"rect","x":10.0,"y":20.0,"w":30.0,"h":40.0,
                "thickness":2.0,"jitter":1.0,"seed":9}"#,
        )
        .unwrap();
        assert_eq!(spec.page, 1);
        assert_eq!(spec.fill, None);
        assert!(!spec.filled);
        assert!(spec.points.is_empty());
        assert_eq!(ShapeSpec::new(ShapeKind::Line, 0.0, 0.0, 0.0, 0.0).page, 1);
        assert_eq!(spec.with_fill([1, 2, 3]).fill, Some([1, 2, 3]));
    }
}













