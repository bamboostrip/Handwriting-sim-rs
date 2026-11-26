//! 符号图形：线条 / 箭头 / 折线 / 矩形 / 椭圆 / 三角形 / 大括号 / 方括号 / 勾 / 下划线。
//!
//! issue #6「增加符号图形」的渲染层。图形光栅化成**覆盖率缓冲**（`0.0~1.0`），
//! 由合成层按 alpha 混合写入画布；`rasterize_shape()` 这个布尔入口保留下来，
//! 只是把覆盖率按 0.5 阈值化，供示例/测试/旧调用方使用。
//!
//! 为什么必须是覆盖率而不是布尔掩码：布尔掩码只有 0/1 两档，笔画的边缘永远是
//! 硬像素台阶，中心线怎么抖都只是「一条被掰弯的矢量线」。真实墨迹有三个布尔
//! 掩码表达不了的性质——**边界是毛的、边界是渐隐的、内部墨量是不均匀的**。
//!
//! 手绘感来源（按贡献排序）：
//! 1. **亚像素毛边**：每个像素按「到笔触中心线的距离 + 像素级哈希噪声」算覆盖率。
//!    边界噪声 ±0.45px，就是把笔画的轮廓线变成砂纸边——这是「不像打印」的第一因素。
//! 2. **1px 抗锯齿羽化**：边界覆盖率在 1px 内从 1 降到 0，墨迹与纸之间有过渡，
//!    而不是一刀切的黑白边界。
//! 3. **胶囊笔触**：距离场天然给出圆头端点，起收笔是圆钝的，不再是逐渐收成针尖。
//! 4. **笔压**：线宽沿笔画长度平滑变化（长线也有 ±8% 的低频起伏），中段重、两端轻。
//! 5. **中心线摆动**：长直线 = 整体弓 + 一次轻微 S 形 + 高频小波；折线转角带过头/欠笔。
//! 6. **实心图形是「涂」出来的**：填充边界羽化 + 毛边，再压一条笔触轮廓形成墨边，
//!    而不是扫描线实心色块；方括号/矩形用圆角连续折线，转角不再是矢量直角。
//!
//! 坐标约定：`ShapeSpec` 的坐标是页面像素坐标；`rasterize_shape_coverage` 的
//! `offset_x/offset_y` 用于把局部坐标（例如框选区域内部坐标）平移到页面。
//! 越界像素由底层覆盖度累加自动裁剪。

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};

/// 圆周率（f32）。
const TAU: f32 = std::f32::consts::TAU;
/// 圆周率（f32，短名，公式里用）。
const PI: f32 = std::f32::consts::PI;

/// 线宽上限（像素）：覆盖度扫描按线宽展开，无穷大/夸张线宽会让它跑近乎
/// 无限次，这里兜底夹住（校验层另有量级检查）。
pub(crate) const MAX_PEN_THICKNESS: f32 = 256.0;
/// 抖动上限（像素）：抖动会撑大几何跨度，同样需要上限防止扫描步数爆炸。
pub(crate) const MAX_PEN_JITTER: f32 = 256.0;

/// 边缘毛糙幅度（像素）：像素级哈希噪声的半径扰动峰值。
/// 太大会变成锯齿，太小（<0.2）在 2~4px 线宽上肉眼看不出来。
const EDGE_ROUGHNESS: f32 = 1.3;
/// 覆盖度羽化半径之外的硬度：`powf` 把中间灰度往墨色方向压一点，
/// 否则小尺寸预览里的细线会被抗锯齿冲淡成灰线。
const INK_GAMMA: f32 = 0.7;

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
    /// 方括号：竖脊 + 两条横臂，转角圆角，开口方向由 side 决定。
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
    /// （光栅化到覆盖率时不使用，由引擎合成层决定颜色。）
    #[serde(default)]
    pub fill: Option<[u8; 3]>,
    /// 手绘抖动幅度（像素）；0 = 规整几何图形（直尺效果），且关闭毛边与笔压。
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

/// 把单个图形光栅化进页面前景**布尔掩码**（兼容入口）。
///
/// 内部先按覆盖率光栅化，再按 `>= 0.5` 阈值化后 OR 进掩码：
/// 需要软边/墨感的调用方（引擎合成层）应改用 [`shape_coverage`] 做 alpha 混合。
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
    let Some((bx, by, bw, bh, cov)) = shape_coverage(spec, page_width, page_height) else {
        return;
    };
    let x0 = bx + offset_x;
    let y0 = by + offset_y;
    if x0 >= page_width || y0 >= page_height {
        return;
    }
    for row in 0..bh {
        let ty = y0 + row;
        if ty >= page_height {
            break;
        }
        for col in 0..bw {
            if cov[row * bw + col] < 0.5 {
                continue;
            }
            let tx = x0 + col;
            if tx >= page_width {
                continue;
            }
            mask[ty * page_width + tx] = true;
        }
    }
}

/// 把单个图形光栅化成整页**覆盖率缓冲**（`0.0~1.0`，长度为 `page_width*page_height`）。
///
/// 合成层若只需要图形自身范围（引擎就这样做）应改用 [`shape_coverage`]，
/// 避免为每个图形分配整页 `f32` 缓冲（A4@300dpi 一页约 35MiB）。
pub fn rasterize_shape_coverage(
    spec: &ShapeSpec,
    coverage: &mut [f32],
    page_width: usize,
    page_height: usize,
    offset_x: usize,
    offset_y: usize,
) {
    if page_width == 0 || page_height == 0 || coverage.len() < page_width * page_height {
        return;
    }
    render_pen(spec, coverage, page_width, page_height, offset_x, offset_y);
}

/// 在图形自身包围盒内光栅化，返回 `(x, y, w, h, 覆盖率缓冲)`（页面像素坐标）。
///
/// 引擎按图形逐个合成时用这个入口：开销只与图形面积相关，不随页面上图形数增长；
/// 返回值里的 `x/y` 是缓冲左上角在页面上的位置，`cov` 按 `row * w + col` 索引。
///
/// 图形完全在页面外、退化尺寸或坐标非有限时返回 `None`。
pub fn shape_coverage(
    spec: &ShapeSpec,
    page_width: usize,
    page_height: usize,
) -> Option<(usize, usize, usize, usize, Vec<f32>)> {
    let (bx, by, bw, bh) = local_mask_box(spec, page_width, page_height)?;
    let mut local = vec![0.0f32; bw * bh];
    // 把图形平移到局部坐标原点后再光栅化（`render_pen` 的 offset 只能加，不能减）
    let mut shifted = spec.clone();
    let (fx, fy) = (bx as f32, by as f32);
    shifted.x -= fx;
    shifted.y -= fy;
    for p in shifted.points.iter_mut() {
        p[0] -= fx;
        p[1] -= fy;
    }
    render_pen(&shifted, &mut local, bw, bh, 0, 0);
    Some((bx, by, bw, bh, local))
}

/// 计算图形在页面内的**局部掩码包围盒**（用于逐图形合成时只在自身区域内分配掩码）。
///
/// 返回 `(x, y, w, h)`（页面像素，已钳制到页面内）；图形完全在页面外、退化尺寸
/// 或坐标非有限时返回 `None`。外扩余量覆盖线宽、抖动、抗锯齿羽化与箭头等笔画外溢。
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

/// 把一组图形依次光栅化进页面布尔掩码（无页面偏移的便捷入口）。
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

/// 图形光栅化的公共前置：参数夹取、整体微倾/微位移、早退与分派。
///
/// 防御性上限：坐标/线宽/抖动在 `validate_with` 里已经限定在合理量级，
/// 这里再夹一次并做「包围盒与画布不相交则早退」——否则调用方绕过校验时，
/// 笔触扫描会按坐标跨度/线宽生成天量的循环（等于卡死）。
fn render_pen(
    spec: &ShapeSpec,
    cov: &mut [f32],
    page_width: usize,
    page_height: usize,
    offset_x: usize,
    offset_y: usize,
) {
    if page_width == 0 || page_height == 0 || cov.len() < page_width * page_height {
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
    let ink = jitter > f32::EPSILON;
    let mut pen = Pen {
        cov,
        width: page_width,
        height: page_height,
        rng,
        thickness,
        phase,
        rot,
        tx,
        ty,
        cx: bx + bw * 0.5,
        cy: by + bh * 0.5,
        seed: (spec.seed as u32) ^ ((spec.seed >> 32) as u32),
        ink,
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
            pen.sketch_line(a, b, amp);
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
                pen.sketch_line(a, b, amp);
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
                // 先「涂」再「描」：填充边界羽化后由轮廓笔触压住，形成墨边。
                // 轮廓比空心图形略粗一点——手写实心块是「先描边再涂满」，
                // 边线叠了两遍墨，本来就比单笔画重。
                pen.fill_inked(&pts);
                let rim = filled_rim(pen.thickness, bw.min(bh));
                pen.path(&pts, true, amp * 0.75, rim);
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
                // 先「涂」再「描」：填充边界羽化后由轮廓笔触压住，形成墨边
                pen.fill_inked(&pts);
                let rim = filled_rim(pen.thickness, bw.min(bh));
                pen.path(&pts, true, amp * 0.75, rim);
            } else {
                pen.sketch(&pts, true, amp);
            }
        }
        ShapeKind::Brace(side) => pen.brace(bx, by, bw, bh, side, amp),
        ShapeKind::Bracket(side) => pen.bracket(bx, by, bw, bh, side, amp),
        ShapeKind::Check => {
            let pts = [
                (bx, by + bh * 0.55),
                (bx + bw * 0.36, by + bh * 0.95),
                (bx + bw, by + bh * 0.05),
            ];
            pen.path(&pts, false, amp, pen.thickness);
        }
        ShapeKind::Underline => {
            let (a, b) = endpoints(&points, x, y, spec.w, spec.h);
            pen.sketch_line(a, b, amp * 1.6);
        }
    }
}

/// 包围盒归一化为 (左上角 x, 左上角 y, 宽, 高)，负宽高自动翻正。
fn normalized_bbox(x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (x.min(x + w), y.min(y + h), w.abs(), h.abs())
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
    pen.sketch_line(a, b, amp);
    let thickness = pen.thickness;
    // 箭头长度随线宽自适应，短箭头时收缩，避免头部盖满整条线
    let head_len = (thickness * 5.0).min(len * 0.55).max(2.0);
    let base = dy.atan2(dx);
    // 手画的箭头两撇张开角不对称，各带一点随机
    let spread = if pen.ink { pen.noise(0.10) } else { 0.0 };
    for sign in [-1.0_f32, 1.0] {
        let half = 0.46 + spread * sign; // 约 26°
        let angle = base + PI + sign * half;
        let tip = (
            b.0 + head_len * angle.cos(),
            b.1 + head_len * angle.sin(),
        );
        let w = thickness * if pen.ink { 0.82 + pen.noise(0.12) } else { 1.0 };
        pen.line(b, tip, w, amp * 0.45);
    }
}

/// 光栅化画笔：持有覆盖率缓冲、随机源与手绘参数。
struct Pen<'a> {
    /// 覆盖率缓冲（`0.0~1.0`，按行优先）；多个笔触取 `max` 合并。
    cov: &'a mut [f32],
    width: usize,
    height: usize,
    rng: StdRng,
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
    /// 像素级边缘噪声的种子（由 `ShapeSpec.seed` 折叠而来）。
    seed: u32,
    /// 手绘模式：决定了笔压、毛边、转角过头这些「不规整」要素是否开启。
    ink: bool,
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

    /// 边缘毛糙：像素级哈希给半径加的扰动。
    ///
    /// 同一像素在不同笔触里拿到同一个值，所以相邻采样段叠起来是一条**连续**的
    /// 毛边，而不是每段一个随机台阶。
    ///
    /// 幅度随半径放大但有上限：细笔（2~3px）几乎不长毛——它「像手写」靠的是
    /// 中心线摆动，而不是边缘砂纸（否则缩略预览里就成了一条灰毛线）；
    /// 粗笔（7px 以上，记号笔那种）能到 ±1.3px，真实粗笔的边缘会被纸纤维
    /// 带出明显的毛须。
    fn edge_noise(&self, x: usize, y: usize, radius: f32) -> f32 {
        let amp = (radius * 0.16).min(EDGE_ROUGHNESS);
        (hash_px(x, y, self.seed) * 2.0 - 1.0) * amp
    }

    /// 一笔「胶囊」笔触：中心线 a→b，两端半宽 `wa/2`、`wb/2`，带 1px 抗锯齿和毛边。
    ///
    /// 用距离场而不是逐点盖章，是为了让下面三件事同时成立：
    /// - 端点是圆头（手写的起收笔是钝的，不是针尖）；
    /// - 线宽沿段线性过渡，接缝处不会再出现宽度台阶；
    /// - 每个像素的覆盖率是连续的，边界可以做亚像素级毛边与羽化。
    fn stamp(&mut self, a: (f32, f32), b: (f32, f32), wa: f32, wb: f32) {
        let a = self.xform(a);
        let b = self.xform(b);
        if !a.0.is_finite() || !a.1.is_finite() || !b.0.is_finite() || !b.1.is_finite() {
            return;
        }
        let ra = (wa * 0.5).max(0.05);
        let rb = (wb * 0.5).max(0.05);
        let rmax = ra.max(rb);
        // 外扩余量要覆盖「最大半径 + 毛边正向峰值（EDGE_ROUGHNESS）+ 半像素抗锯齿」，
        // 否则粗笔的毛须会被自己的扫描框切出一条直线。
        let margin = rmax + EDGE_ROUGHNESS + 0.5;
        let x_lo = ((a.0.min(b.0)) - margin).floor();
        let x_hi = ((a.0.max(b.0)) + margin).ceil();
        let y_lo = ((a.1.min(b.1)) - margin).floor();
        let y_hi = ((a.1.max(b.1)) + margin).ceil();
        if !x_lo.is_finite() || !x_hi.is_finite() || !y_lo.is_finite() || !y_hi.is_finite() {
            return;
        }
        let x0 = (x_lo as isize).max(0) as usize;
        let x1 = (x_hi as isize).min(self.width as isize - 1);
        let y0 = (y_lo as isize).max(0) as usize;
        let y1 = (y_hi as isize).min(self.height as isize - 1);
        if x1 < x0 as isize || y1 < y0 as isize {
            return;
        }
        let x1 = x1 as usize;
        let y1 = y1 as usize;
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len2 = dx * dx + dy * dy;
        for y in y0..=y1 {
            let py = y as f32 + 0.5;
            for x in x0..=x1 {
                let px = x as f32 + 0.5;
                // 点到线段距离 + 段内参数 t（用于插值半宽）
                let (d, t) = if len2 < 1e-6 {
                    (((px - a.0).powi(2) + (py - a.1).powi(2)).sqrt(), 0.0)
                } else {
                    let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
                    let cx = a.0 + dx * t;
                    let cy = a.1 + dy * t;
                    (((px - cx).powi(2) + (py - cy).powi(2)).sqrt(), t)
                };
                let mut r = ra + (rb - ra) * t;
                if self.ink {
                    r += self.edge_noise(x, y, r);
                }
                // 1px 抗锯齿：覆盖率在边界两侧各 0.5px 内过渡
                let mut c = (r - d + 0.5).clamp(0.0, 1.0);
                if c <= 0.002 {
                    continue;
                }
                // 粗笔的内部墨量不均：纸纤维吸墨不匀，笔迹里会漏出细小白点
                // （issue 附图里那根记号笔竖线就是这样，密而细的砂点）。
                // 只有 r >= 1.8px（线宽 >= 3.6px）才开——细笔在真实分辨率下
                // 看不出这种颗粒，而既有「图形颜色必须等于指定墨色」的断言
                // 依赖 2.4px 这类细线内部保持完全不透明。
                if c >= 0.99 && r >= 1.8 {
                    let g = hash_px(x, y, self.seed ^ 0x6a09_e667);
                    if g < 0.055 {
                        c *= 0.58;
                    }
                }
                // 轻微 gamma 提黑：细线在缩略预览里不会被抗锯齿冲成灰线
                let c = if c < 1.0 { c.powf(INK_GAMMA) } else { 1.0 };
                let idx = y * self.width + x;
                if c > self.cov[idx] {
                    self.cov[idx] = c;
                }
            }
        }
    }

    /// 笔压：沿笔画长度方向平滑变化的线宽系数。
    ///
    /// 真实手写几乎不存在「全程等宽」的线——起笔轻、行笔重、收笔再轻，
    /// 再叠加一点低频起伏（写字时手劲不会突然变，但会缓慢波动）。
    /// `taper` 为两端收细的比例（0 = 不收细，用于闭合曲线如椭圆）。
    fn pressure(&self, t: f32, taper: f32) -> f32 {
        if !self.ink {
            return 1.0; // jitter=0 = 规整模式（直尺效果），不做笔压起伏
        }
        // 起收笔收细，中段最粗
        let taper_f = 1.0 - taper * (1.0 - (PI * t).sin());
        // 低频力度起伏（相位随图形/笔画不同）
        let press = 1.0 + 0.20 * (self.phase + t * 8.1).sin();
        (taper_f * press).max(0.5)
    }

    /// 闭合图形笔压：按完整周期变化，起点/终点天然一致，不会在接口处跳宽。
    fn closed_pressure(&self, t: f32) -> f32 {
        if !self.ink {
            1.0
        } else {
            1.0 + 0.18 * (TAU * 2.0 * t + self.phase).sin()
        }
    }

    /// 路径笔压：开放路径沿整条路径连续变化；闭合路径使用周期函数。
    fn path_pressure(&self, t: f32, closed: bool, taper: f32) -> f32 {
        if closed {
            self.closed_pressure(t)
        } else {
            self.pressure(t, taper)
        }
    }

    /// 直线线宽：长线也保留缓慢的力度起伏（约 ±8%）。
    ///
    /// 早期为了躲开「布尔掩码把平滑笔压量化成粗细分段」，长线被改成绝对等宽，
    /// 结果长线变成一根没有呼吸的矢量线。现在覆盖率是连续的，低频起伏只会
    /// 变成「整条线中段略粗、两端略细」的自然笔压，不会再出现一段粗一段细。
    fn line_width(&self, t: f32, _len: f32, thickness: f32) -> f32 {
        if !self.ink {
            return thickness;
        }
        // 两端轻微收细（胶囊端帽保证收笔是圆的，不会收成针尖）
        let ends = 1.0 - 0.18 * (1.0 - (PI * t).sin());
        let press = 1.0 + 0.08 * (self.phase + t * 5.3).sin();
        (thickness * ends * press).max(0.6)
    }

    /// 笔触采样数：约 1.5px 一个采样点，保证中心线和线宽变化连续。
    /// 上限避免 defensive 超大线宽/超长坐标把逐段光栅化开销放大。
    fn stroke_steps(len: f32) -> usize {
        ((len / 1.5).ceil() as usize).clamp(8, 768)
    }

    /// 手写直线：平滑中心线 + 连续采样 + 连续笔压。
    ///
    /// 手写长线 = 「整体轻微弓起」+「一次很慢的 S 形」+「若干小波浪」：
    /// 只有弓会像香蕉，只有高频抖动又像锯齿，三者叠加才像手腕自然带出来的线。
    fn line(&mut self, a: (f32, f32), b: (f32, f32), thickness: f32, amp: f32) {
        let a = self.jittered(a, amp * 0.5);
        let b = self.jittered(b, amp * 0.5);
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.6 {
            self.stamp(a, a, thickness, thickness);
            return;
        }
        if !self.ink {
            // 规整模式一次画完，避免无意义的密集采样。
            self.stamp(a, b, thickness, thickness);
            return;
        }
        let arch_amp = (len * 0.006).min(5.0) + amp * 0.35;
        let s_amp = if len > 120.0 {
            (len * 0.0022).min(2.5) + amp * 0.15
        } else {
            0.0
        };
        let wave_amp = amp * 0.55 + (len * 0.0015).min(1.6);
        let bow = self.noise(arch_amp);
        let s_shape = self.noise(s_amp);
        let wob = self.noise(wave_amp);
        let nx = -dy / len;
        let ny = dx / len;
        let steps = Self::stroke_steps(len);
        let mut prev = a;
        let mut prev_w = self.line_width(0.0, len, thickness);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let base = (a.0 + dx * t, a.1 + dy * t);
            let off = bow * (PI * t).sin()
                + s_shape * (TAU * t).sin()
                + wob * (TAU * 2.5 * t + self.phase).sin();
            let p = (base.0 + nx * off, base.1 + ny * off);
            let end_w = self.line_width(t, len, thickness);
            self.stamp(prev, p, prev_w, end_w);
            prev = p;
            prev_w = end_w;
        }
    }

    /// 手绘直线：只画一遍主笔。
    ///
    /// 真实书写不会沿整条长线平行画第二遍；胶囊笔触 + 毛边 + 笔压已经足以
    /// 区分手写与矢量线，再补全段细线反而会在缩略图里变成不自然的双线。
    fn sketch_line(&mut self, a: (f32, f32), b: (f32, f32), amp: f32) {
        self.line(a, b, self.thickness, amp);
    }

    /// 二次贝塞尔段：按近似弧长约 1.5px 采样，线宽沿整条路径连续变化。
    #[allow(clippy::too_many_arguments)]
    fn quad(
        &mut self,
        p0: (f32, f32),
        c: (f32, f32),
        p1: (f32, f32),
        thickness: f32,
        taper: f32,
        closed: bool,
        u0: f32,
        u1: f32,
    ) {
        let approx_len = ((c.0 - p0.0).powi(2) + (c.1 - p0.1).powi(2)).sqrt()
            + ((p1.0 - c.0).powi(2) + (p1.1 - c.1).powi(2)).sqrt();
        let steps = Self::stroke_steps(approx_len);
        let mut prev = p0;
        let mut prev_w = thickness * self.path_pressure(u0, closed, taper);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let mt = 1.0 - t;
            let x = mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0;
            let y = mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1;
            let u = u0 + (u1 - u0) * t;
            let end_w = thickness * self.path_pressure(u, closed, taper);
            self.stamp(prev, (x, y), prev_w, end_w);
            prev = (x, y);
            prev_w = end_w;
        }
    }

    /// 三次贝塞尔段（大括号用）：同样密集采样，笔压跨上下两段连续衔接。
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
        u0: f32,
        u1: f32,
    ) {
        let c1 = self.jittered(c1, amp * 0.7);
        let c2 = self.jittered(c2, amp * 0.7);
        let approx_len = ((c1.0 - p0.0).powi(2) + (c1.1 - p0.1).powi(2)).sqrt()
            + ((c2.0 - c1.0).powi(2) + (c2.1 - c1.1).powi(2)).sqrt()
            + ((p1.0 - c2.0).powi(2) + (p1.1 - c2.1).powi(2)).sqrt();
        let steps = Self::stroke_steps(approx_len);
        let mut prev = p0;
        let mut prev_w = thickness * self.pressure(u0, taper);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let mt = 1.0 - t;
            let w0 = mt * mt * mt;
            let w1 = 3.0 * mt * mt * t;
            let w2 = 3.0 * mt * t * t;
            let w3 = t * t * t;
            let x = w0 * p0.0 + w1 * c1.0 + w2 * c2.0 + w3 * p1.0;
            let y = w0 * p0.1 + w1 * c1.1 + w2 * c2.1 + w3 * p1.1;
            let u = u0 + (u1 - u0) * t;
            let end_w = thickness * self.pressure(u, taper);
            self.stamp(prev, (x, y), prev_w, end_w);
            prev = (x, y);
            prev_w = end_w;
        }
    }

    /// 折线/多边形：顶点加噪后逐段用二次贝塞尔连接（控制点沿法线随机偏移）。
    fn path(&mut self, points: &[(f32, f32)], closed: bool, amp: f32, thickness: f32) {
        if points.len() < 2 {
            return;
        }
        let pts: Vec<(f32, f32)> = points.iter().map(|&p| self.jittered(p, amp)).collect();
        let segs = if closed { pts.len() } else { pts.len() - 1 };
        let total_len = (0..segs)
            .map(|i| {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
            })
            .sum::<f32>()
            .max(1e-3);
        let mut traveled = 0.0;
        for i in 0..segs {
            let p0 = pts[i];
            let p1 = pts[(i + 1) % pts.len()];
            let dx = p1.0 - p0.0;
            let dy = p1.1 - p0.1;
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            // 手写线很少是绝对直的：中点沿法线随机弓一下
            let bow_amp = if self.ink {
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
            // 使转角出现交叉/缝隙，而不是数字化的完美尖角。
            // 过头量按线宽给（约 1.4 个笔宽），只用边长比例封顶：
            // 早期写死 5% 边长，小三角/小勾（边长十几像素）几乎没有过头，
            // 转角就还是矢量尖角。
            let over_amp = if self.ink {
                (thickness * 1.4).min(len * 0.16)
            } else {
                0.0
            };
            let over0 = self.noise(over_amp);
            let over1 = self.noise(over_amp);
            let ux = dx / len;
            let uy = dy / len;
            let a = (p0.0 - ux * over0, p0.1 - uy * over0);
            let b = (p1.0 + ux * over1, p1.1 + uy * over1);
            let u0 = traveled / total_len;
            let u1 = (traveled + len) / total_len;
            self.quad(a, c, b, thickness, 0.38, closed, u0, u1);
            traveled += len;
        }
    }

    /// 手绘折线：只画一遍主笔，转角交叠由 `path` 的过头量自然形成。
    fn sketch(&mut self, points: &[(f32, f32)], closed: bool, amp: f32) {
        self.path(points, closed, amp, self.thickness);
    }

    /// 椭圆弧：8 段二次贝塞尔逼近，控制点噪声按角度连续变化。
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
        let rad = (rx + ry) * 0.5;
        let dev_amp = if self.ink {
            amp * 0.6 + rad * 0.012
        } else {
            0.0
        };
        for i in 0..SEGS {
            let a0 = start + sweep * i as f32 / SEGS as f32;
            let a1 = start + sweep * (i + 1) as f32 / SEGS as f32;
            let am = (a0 + a1) * 0.5;
            let half = (a1 - a0) * 0.5;
            // 二次贝塞尔逼近圆弧：切点半径放大 1/cos(半角)
            let k = 1.0 / half.cos().max(0.2);
            let p0 = (cx + rx * a0.cos(), cy + ry * a0.sin());
            let p1 = (cx + rx * a1.cos(), cy + ry * a1.sin());
            // 控制点扰动必须是角度的连续函数；每段独立随机会让椭圆出现八个折点。
            let rough = dev_amp
                * (0.65 * (am * 3.0 + self.phase).sin()
                    + 0.35 * (am * 7.0 + self.phase * 0.7).sin());
            let c = (
                cx + rx * am.cos() * k + rough * am.cos(),
                cy + ry * am.sin() * k + rough * am.sin(),
            );
            let u0 = i as f32 / SEGS as f32;
            let u1 = (i + 1) as f32 / SEGS as f32;
            self.quad(p0, c, p1, thickness, 0.0, true, u0, u1);
        }
    }

    /// 手绘椭圆：随机起笔、收笔过冲一小段，整圈只画一遍。
    fn ellipse(&mut self, x: f32, y: f32, w: f32, h: f32, filled: bool, amp: f32) {
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let rx = (w * 0.5).max(0.5);
        let ry = (h * 0.5).max(0.5);
        let thickness = if filled {
            filled_rim(self.thickness, w.min(h))
        } else {
            self.thickness
        };
        if filled {
            let poly = ellipse_polygon(cx, cy, rx, ry, 48);
            self.fill_inked(&poly);
        }
        let (start, over) = if self.ink {
            (
                self.rng.random_range(0.0..TAU),
                self.rng.random_range(0.10..0.30),
            )
        } else {
            (0.0, 0.0)
        };
        self.ellipse_arcs(cx, cy, rx, ry, start, TAU + over, amp, thickness);
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

        let half_len = (h * 0.65 + w * 0.55).max(1.0);
        let total_len = half_len * 2.0;
        let u_mid = half_len / total_len;

        // 上半段：起笔 → 竖脊 → 中间尖角
        self.cubic(
            top,
            (spine_x, y + h * 0.02),
            (spine_x, y + h * 0.44),
            cusp,
            thickness,
            amp,
            0.30,
            0.0,
            u_mid,
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
            u_mid,
            1.0,
        );
    }

    /// 方括号：竖脊 + 两条横臂，转角用二次贝塞尔做圆角。
    ///
    /// 手写方括号不是「三段直线拼成的直角 U」：转角处笔速变慢会画圆，
    /// 上下臂几乎从不相等长，整条竖脊还会被手腕带出一点弓形。
    /// 这些细节在缩略图里比线宽更能决定「像不像手写」。
    fn bracket(&mut self, x: f32, y: f32, w: f32, h: f32, side: BracketSide, amp: f32) {
        let thickness = self.thickness;
        let dir = match side {
            BracketSide::Left => 1.0,
            BracketSide::Right => -1.0,
        };
        let stem_x = if dir > 0.0 { x } else { x + w };
        // 圆角半径：手写方括号的转角只是「磨掉一点」，不是大圆弧
        // （issue 附图里的 `[` 转角半径 ≈ 臂长的 1/4，且绝对半径很小）
        let r = (w.min(h) * 0.22).clamp(1.2, 6.0);
        let r = if self.ink { r * (1.0 + self.noise(0.12)) } else { r };
        // 两臂不等长 + 端点不水平：手写时收笔收不住，横臂会很自然地翘一点
        let (arm_top, arm_bot, tilt_top, tilt_bot) = if self.ink {
            (
                1.0 + self.noise(0.16),
                1.0 + self.noise(0.16),
                self.noise(thickness * 0.9),
                self.noise(thickness * 0.9),
            )
        } else {
            (1.0, 1.0, 0.0, 0.0)
        };
        let top_y = y;
        let bot_y = y + h;
        let mut pts: Vec<(f32, f32)> = Vec::with_capacity(10);
        pts.push((stem_x + dir * w * arm_top, top_y + tilt_top));
        let c1_in = (stem_x + dir * r, top_y);
        pts.push(c1_in);
        push_quad(&mut pts, c1_in, (stem_x, top_y), (stem_x, top_y + r), 3);
        let c2_in = (stem_x, bot_y - r);
        pts.push(c2_in);
        push_quad(&mut pts, c2_in, (stem_x, bot_y), (stem_x + dir * r, bot_y), 3);
        pts.push((stem_x + dir * w * arm_bot, bot_y + tilt_bot));
        self.path(&pts, false, amp, thickness);
    }

    /// 扫描线填充（墨迹版）：4 条子扫描线做纵向抗锯齿、横向按像素重叠度加权；
    /// 边缘带（覆盖率 < 1 的像素）再叠像素噪声，得到「涂出来的」羽化毛边。
    ///
    /// 内部覆盖率严格保持 1.0：合成层是 alpha 混合，内部若掺灰度会让
    /// 「图形颜色 = 指定墨色」的精确断言（含前端取色/叠放顺序）失效。
    fn fill_inked(&mut self, points: &[(f32, f32)]) {
        if points.len() < 3 {
            return;
        }
        // 填充多边形也要套整体倾斜/位移，否则填充与描边会错位
        let pts: Vec<(f32, f32)> = points.iter().map(|&p| self.xform(p)).collect();
        let min_x = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
        let max_x = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
        let min_y = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let max_y = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        if ![min_x, max_x, min_y, max_y].iter().all(|v| v.is_finite()) {
            return;
        }
        let x0 = (min_x.floor() as isize).max(0) as usize;
        let x1 = (max_x.ceil() as isize).min(self.width as isize - 1).max(0) as usize;
        let y0 = (min_y.floor() as isize).max(0) as usize;
        let y1 = (max_y.ceil() as isize).min(self.height as isize - 1).max(0) as usize;
        if x1 < x0 || y1 < y0 {
            return;
        }
        let row_w = x1 - x0 + 1;
        let mut acc = vec![0.0f32; row_w];
        let mut xs: Vec<f32> = Vec::with_capacity(8);
        const SUB: usize = 4;
        for y in y0..=y1 {
            acc.fill(0.0);
            for s in 0..SUB {
                let scan_y = y as f32 + (s as f32 + 0.5) / SUB as f32;
                xs.clear();
                for i in 0..pts.len() {
                    let a = pts[i];
                    let b = pts[(i + 1) % pts.len()];
                    if (a.1 <= scan_y) != (b.1 <= scan_y) {
                        let t = (scan_y - a.1) / (b.1 - a.1);
                        xs.push(a.0 + t * (b.0 - a.0));
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                for pair in xs.as_chunks::<2>().0 {
                    let (xa, xb) = (pair[0], pair[1]);
                    let ia = xa.floor() as isize;
                    let ib = xb.ceil() as isize;
                    for xi in ia..ib {
                        if xi < x0 as isize || xi > x1 as isize {
                            continue;
                        }
                        let lo = xa.max(xi as f32);
                        let hi = xb.min(xi as f32 + 1.0);
                        if hi > lo {
                            acc[(xi - x0 as isize) as usize] += (hi - lo) / SUB as f32;
                        }
                    }
                }
            }
            for (i, &raw) in acc.iter().enumerate() {
                let mut a = raw.min(1.0);
                if a <= 0.002 {
                    continue;
                }
                let x = x0 + i;
                // 只毛化边界带：内部保持 1.0（颜色精确），边界得到缺口与羽化
                if self.ink && a < 0.97 {
                    let n = hash_px(x, y, self.seed ^ 0x5bf0_3635);
                    if n < 0.16 {
                        a *= 0.55 + 2.8 * n;
                    }
                }
                let idx = y * self.width + x;
                if a > self.cov[idx] {
                    self.cov[idx] = a;
                }
            }
        }
    }
}

/// 实心图形的轮廓线宽。
///
/// 手写实心块是「先描边再涂满」，边线叠了两遍墨，所以比同笔画的空心图形粗 15%；
/// 但小图形（行首 `▶` 只有十几像素）必须按图形尺寸封顶——否则一个 5px 笔尖
/// 画 16px 高的三角，轮廓会吃掉整个形状，只剩下一个墨团。
fn filled_rim(thickness: f32, size: f32) -> f32 {
    (thickness * 1.15).min(size * 0.22).max(0.8)
}

/// 往点列尾部追加一段二次贝塞尔（不含起点、含终点），用于把直角磨成圆角。
fn push_quad(
    out: &mut Vec<(f32, f32)>,
    from: (f32, f32),
    control: (f32, f32),
    to: (f32, f32),
    steps: usize,
) {
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let mt = 1.0 - t;
        let x = mt * mt * from.0 + 2.0 * mt * t * control.0 + t * t * to.0;
        let y = mt * mt * from.1 + 2.0 * mt * t * control.1 + t * t * to.1;
        out.push((x, y));
    }
}

/// 像素级哈希 → `0.0~1.0`：给边缘毛糙提供**确定性**噪声
/// （同 seed 同坐标必得同值，保证「同 seed 逐像素可复现」）。
#[inline]
fn hash_px(x: usize, y: usize, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
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

    fn test_pen<'a>(mask: &'a mut [f32], width: usize, height: usize, seed: u64) -> Pen<'a> {
        Pen {
            cov: mask,
            width,
            height,
            rng: StdRng::seed_from_u64(seed),
            thickness: 2.4,
            phase: 0.7,
            rot: 0.0,
            tx: 0.0,
            ty: 0.0,
            cx: 160.0,
            cy: 100.0,
            seed: seed as u32,
            ink: true,
        }
    }

    fn blank_cov(width: usize, height: usize) -> Vec<f32> {
        vec![0.0f32; width * height]
    }

    /// 笔触边缘必须有**亚像素覆盖率**（0<a<1）——这是「有羽化、不像矢量线」的证据。
    #[test]
    fn stroke_edges_have_partial_coverage() {
        let (w, h) = (320usize, 240usize);
        let mut cov = blank_cov(w, h);
        {
            let mut pen = test_pen(&mut cov, w, h, 61);
            pen.sketch_line((40.0, 120.0), (280.0, 120.0), 1.4);
        }
        let partial = cov.iter().filter(|&&a| a > 0.02 && a < 0.98).count();
        let solid = cov.iter().filter(|&&a| a >= 0.98).count();
        assert!(partial > 200, "笔画边缘应有大量半透明像素，实际 {partial}");
        assert!(solid > 100, "笔画中轴仍应是实心墨，实际 {solid}");
    }

    /// 实心填充内部覆盖率必须是 1.0：合成层按 alpha 混合，
    /// 内部若掺灰度会破坏「图形颜色 = 指定墨色」的精确断言。
    #[test]
    fn filled_interior_is_opaque() {
        let (w, h) = (200usize, 170usize);
        let spec = ShapeSpec::new(ShapeKind::Triangle, 40.0, 30.0, 120.0, 100.0)
            .with_filled(true)
            .with_jitter(2.4)
            .with_seed(17);
        let (_, _, _, _, cov) = shape_coverage(&spec, w, h).unwrap();
        assert!(
            cov.iter().any(|&a| a >= 0.999),
            "实心图形内部必须存在完全不透明的像素"
        );
        // 重心附近应是不透明墨（不是灰色填充）
        let (bx, by, bw, _) = local_mask_box(&spec, w, h).unwrap();
        let idx = (110 - by) * bw + (100 - bx);
        assert!(cov[idx] >= 0.999, "三角形重心应为不透明墨，实际 {}", cov[idx]);
    }

    /// 短笔画仍保留明显笔压（起收笔比中段细）。
    #[test]
    fn short_strokes_keep_pressure_variation() {
        let mut cov = blank_cov(32, 24);
        let pen = test_pen(&mut cov, 32, 24, 44);
        let end = pen.line_width(0.0, 80.0, 2.4);
        let mid = pen.line_width(0.5, 80.0, 2.4);
        assert!(end < mid, "起笔应比中段细：{end} vs {mid}");
    }

    /// 长线不再是绝对等宽，但相邻采样点之间的线宽变化必须足够平滑
    /// （早期「一段粗一段细」的回归防线）。
    #[test]
    fn long_lines_wobble_smoothly() {
        let mut cov = blank_cov(32, 24);
        let pen = test_pen(&mut cov, 32, 24, 44);
        let widths: Vec<f32> = (0..=40)
            .map(|i| pen.line_width(i as f32 / 40.0, 900.0, 2.4))
            .collect();
        let min = widths.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max = widths.iter().fold(0.0f32, |a, &b| a.max(b));
        assert!(max - min > 0.05, "长线应有笔压起伏：{min}..{max}");
        assert!(
            max <= 2.4 * 1.25 && min >= 2.4 * 0.7,
            "长线笔压不应超出自然范围：{min}..{max}"
        );
        for pair in widths.windows(2) {
            assert!(
                (pair[1] - pair[0]).abs() < 0.12,
                "相邻采样点线宽跳变过大：{} → {}",
                pair[0],
                pair[1]
            );
        }
    }

    /// 规整模式（jitter=0）必须完全没有毛边：边界像素要么 0 要么 1。
    #[test]
    fn zero_jitter_has_no_ragged_edges() {
        let (w, h) = (200usize, 120usize);
        let spec = ShapeSpec::new(ShapeKind::Line, 20.0, 60.0, 160.0, 0.0)
            .with_thickness(4.0)
            .with_jitter(0.0)
            .with_seed(3);
        let (_, _, _, _, cov) = shape_coverage(&spec, w, h).unwrap();
        // 规整直线：同一列上的覆盖率应完全相同（没有像素级毛边造成的起伏）
        let cols: Vec<f32> = (0..cov.len()).map(|i| cov[i]).filter(|&a| a > 0.0).collect();
        let mid: Vec<f32> = cols.iter().copied().filter(|&a| a > 0.5 && a < 0.999).collect();
        assert!(
            mid.len() <= cols.len() / 3,
            "规整模式不应有大面积半透明毛边：{} / {}",
            mid.len(),
            cols.len()
        );
    }

    /// 所有曲线共用密集采样；超长输入仍要有硬上限。
    #[test]
    fn curve_sampling_is_dense_and_bounded() {
        assert_eq!(Pen::stroke_steps(12.0), 8, "短笔划保持少量采样即可");
        assert!(Pen::stroke_steps(900.0) >= 600, "900px 长笔划应至少采样 600 次");
        assert_eq!(Pen::stroke_steps(1.0e9), 768, "超长笔划采样数必须有硬上限");
    }

    /// 闭合图形笔压必须按周期变化，不能在椭圆/矩形接口处突然跳宽。
    #[test]
    fn closed_pressure_wraps_without_a_jump() {
        let mut cov = blank_cov(32, 24);
        let pen = test_pen(&mut cov, 32, 24, 45);
        let start = pen.path_pressure(0.0, true, 0.38);
        let end = pen.path_pressure(1.0, true, 0.38);
        assert!((start - end).abs() < 1.0e-5, "闭合笔压首尾应连续：{start} vs {end}");
        assert!(
            (pen.path_pressure(0.0, true, 0.38) - pen.path_pressure(0.25, true, 0.38)).abs()
                > 1.0e-3,
            "闭合笔压仍应有平滑起伏"
        );
    }

    /// 直线入口只画一遍：与直接调用主笔算法逐像素一致，不能额外增加复描笔画。
    #[test]
    fn sketch_line_is_a_single_stroke() {
        let (w, h) = (320usize, 240usize);
        let mut actual = blank_cov(w, h);
        let mut expected = blank_cov(w, h);
        let mut actual_pen = test_pen(&mut actual, w, h, 41);
        let mut expected_pen = test_pen(&mut expected, w, h, 41);
        let (a, b) = ((24.0, 80.0), (296.0, 80.0));
        let thickness = expected_pen.thickness;
        actual_pen.sketch_line(a, b, 1.4);
        expected_pen.line(a, b, thickness, 1.4);
        assert_eq!(actual, expected, "直线不应再画一遍整段复描细线");
    }

    /// 折线入口只画一遍：矩形/三角形等闭合路径也不能额外增加外圈重影。
    #[test]
    fn sketch_path_is_a_single_stroke() {
        let (w, h) = (320usize, 240usize);
        let mut actual = blank_cov(w, h);
        let mut expected = blank_cov(w, h);
        let mut actual_pen = test_pen(&mut actual, w, h, 42);
        let mut expected_pen = test_pen(&mut expected, w, h, 42);
        let points = [(48.0, 40.0), (272.0, 40.0), (272.0, 160.0), (48.0, 160.0)];
        let thickness = expected_pen.thickness;
        actual_pen.sketch(&points, true, 1.4);
        expected_pen.path(&points, true, 1.4, thickness);
        assert_eq!(actual, expected, "闭合折线不应再画一遍整体错位的复描");
    }

    /// 椭圆只画一圈：收笔过冲保留，但不能在外侧再补一圈细线。
    #[test]
    fn ellipse_is_a_single_loop() {
        let (w, h) = (320usize, 240usize);
        let mut actual = blank_cov(w, h);
        let mut expected = blank_cov(w, h);
        let mut actual_pen = test_pen(&mut actual, w, h, 43);
        let mut expected_pen = test_pen(&mut expected, w, h, 43);
        actual_pen.ellipse(48.0, 40.0, 224.0, 120.0, false, 1.4);
        let start = expected_pen.rng.random_range(0.0..TAU);
        let over = expected_pen.rng.random_range(0.10..0.30);
        expected_pen.ellipse_arcs(160.0, 100.0, 112.0, 60.0, start, TAU + over, 1.4, 2.4);
        assert_eq!(actual, expected, "椭圆不应再外扩并复描第二圈");
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
