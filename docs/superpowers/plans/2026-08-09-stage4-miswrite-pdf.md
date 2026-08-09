# 阶段四实施计划：错字划掉重写 + PDF 位图导出

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现错字率驱动的「写错划掉重写」效果（两种重写模式）与 PDF 位图层导出。

**Architecture:** 错字效果在排版层实现——`layout_page`（文本路径）内联绘制、`layout_paragraph`（段落路径）阶段一判定/阶段二绘制，判定点统一插在每字符 RNG 扰动之后且 `miswrite_rate == 0` 时不消耗 RNG（保证零回归）。PDF 导出复用全分辨率 `render_pages`，逐页嵌入 300 DPI 位图。

**Tech Stack:** Rust / rand 0.9（`Rng::random_bool(p: f64)`）/ printpdf 0.12（`default-features=false, features=["images"]`，MIT）/ Slint 1.17。

**前置状态：** 阶段三已提交（697ad58）；bug 修复（engine.rs/models.rs 的 TextAreaTooSmall 守卫）仍在工作区未提交，先由 Task 0 单独提交。

**验证命令：** `cargo test --lib core::layout::`、`cargo test --lib core::engine::`、`cargo test --lib core::presets::`、`cargo test`、`cargo clippy --all-targets`

**注意：** 每次 `git commit` 前先 `git status --short`，只 `git add` 本任务列出的文件。

---

## Task 0: 先提交预览卡死 bug 修复（前置清理）

**Files:**
- Modify: `src/core/engine.rs`（TextAreaTooSmall 变体 + 两个多页循环零进度守卫 + 2 个回归测试）
- Modify: `src/core/models.rs`（ParamsError::NoLineSpacing 变体 + total_line_spacing 校验）

这些改动已在工作区（本计划编写前完成并验证：修复前测试挂起 45s，修复后 44 测试全绿）。直接提交：

- [ ] **Step 1: 确认工作区状态**

Run: `git status --short`
Expected: 只有 `src/core/engine.rs`、`src/core/models.rs` 两处修改（均为 bug 修复）。

- [ ] **Step 2: 提交**

```bash
git add src/core/engine.rs src/core/models.rs
git commit -m "fix(core): 预览卡死——多页循环零进度守卫（TextAreaTooSmall）+ 行距校验"
```

- [ ] **Step 3: 全量回归确认**

Run: `cargo test && cargo clippy --all-targets`
Expected: 全部通过，clippy 无警告。

## Task 1: 模型层 — MiswriteMode 枚举 + 新参数 + 校验

**Files:**
- Modify: `src/core/models.rs`
- Test: `src/core/models.rs`（tests 模块）

- [ ] **Step 1: 写失败测试（models.rs tests 模块末尾追加）**

```rust
    #[test]
    fn miswrite_defaults_off_and_above() {
        let p = HandwritingParams::default();
        assert_eq!(p.miswrite_rate, 0.0);
        assert_eq!(p.miswrite_rewrite_mode, MiswriteMode::Above);
    }

    #[test]
    fn validate_rejects_out_of_range_miswrite_rate() {
        let dir = tempfile::tempdir().unwrap();
        let font = dir.path().join("font.ttf");
        let bg = dir.path().join("bg.png");
        std::fs::write(&font, b"dummy").unwrap();
        std::fs::write(&bg, b"dummy").unwrap();
        let base = HandwritingParams {
            text: "你好".into(),
            font_path: font.to_string_lossy().into_owned(),
            background_path: bg.to_string_lossy().into_owned(),
            ..HandwritingParams::default()
        };
        assert!(matches!(
            base.clone().validate(),
            Ok(())
        ));
        let p = HandwritingParams { miswrite_rate: -0.01, ..base.clone() };
        assert!(matches!(p.validate(), Err(ParamsError::MiswriteRate { .. })));
        let p = HandwritingParams { miswrite_rate: 1.01, ..base };
        assert!(matches!(p.validate(), Err(ParamsError::MiswriteRate { .. })));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib core::models::`
Expected: 编译失败——`MiswriteMode`、`miswrite_rate`、`MiswriteMode` 不存在。

- [ ] **Step 3: 实现（models.rs）**

在 `Align` 枚举之后添加：

```rust
/// 错字划掉后的重写方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MiswriteMode {
    #[default]
    Above,   // 错字正上方略偏右，小一号重写
    Rewrite, // 错字划掉后，后文正常位置重写
}
```

在 `ParamsError` 中添加变体：

```rust
    #[error("错字率必须在 0~1 之间：{0}")]
    MiswriteRate { value: f32 },
```

在 `HandwritingParams` 的「随机扰动」字段块之后（`end_chars` 之前）添加：

```rust
    // ---- 写错字模拟 ----
    /// 每字符被判定为错字的概率（0~1，UI 中为 0~30%）。
    #[serde(default)]
    pub miswrite_rate: f32,
    /// 错字重写方式。
    #[serde(default)]
    pub miswrite_rewrite_mode: MiswriteMode,
```

在 `Default` impl 中（`perturb_theta_sigma: 0.05,` 之后）添加：

```rust
            miswrite_rate: 0.0,
            miswrite_rewrite_mode: MiswriteMode::Above,
```

在 `validate()` 中（`total_line_spacing` 检查之后、`Ok(())` 之前）添加：

```rust
    if !(0.0..=1.0).contains(&self.miswrite_rate) {
        return Err(ParamsError::MiswriteRate { value: self.miswrite_rate });
    }
```

在 `default_values_match_python` 测试中追加断言（可选）：

```rust
        assert_eq!(p.miswrite_rate, 0.0);
        assert_eq!(p.miswrite_rewrite_mode, MiswriteMode::Above);
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib core::models::`
Expected: PASS（全部通过，含新增 2 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/core/models.rs
git commit -m "feat(core): 错字率/重写方式参数（MiswriteMode + 校验）"
```

---

## Task 2: 文本路径错字效果（layout_page）

**Files:**
- Modify: `src/core/layout.rs`
- Test: `src/core/layout.rs`（tests 模块）

- [ ] **Step 1: 写失败测试（layout.rs tests 模块追加）**

```rust
    /// 错字率>0 时输出应比关闭时产生更多前景（删除线/重写墨迹），且消费 RNG 后
    /// 同 seed 应稳定复现；错字率=0 与历史行为一致（不消费额外 RNG）。
    #[test]
    fn miswrite_adds_ink_and_rate_zero_is_stable() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.miswrite_rate = 0.5;
        p.miswrite_rewrite_mode = MiswriteMode::Above;
        let text = "今天天气很好，我们去公园散步。".to_string();
        let a = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        let b = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        assert_eq!(a.consumed, b.consumed);
        assert_eq!(a.mask, b.mask, "同 seed 应逐像素一致");
        let ink_a = a.mask.iter().filter(|&&v| v).count();
        assert!(ink_a > 0, "应存在前景");

        // 错字率=0：不额外消费 RNG，输出应与 0.5 时同 seed 的"非错字部分"无关——
        // 直接断言：0.5 的墨迹量大于 0.0 的墨迹量（删除线/重写增加前景）。
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let zero = layout_page(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        assert!(
            ink_a > zero.mask.iter().filter(|&&v| v).count(),
            "错字效果应增加前景像素"
        );
    }

    /// Rewrite 模式：重写字符画在错字右侧（x 推进），墨迹分布明显更宽。
    #[test]
    fn miswrite_rewrite_mode_draws_extra_glyph() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.miswrite_rate = 1.0; // 全部字符错字 → 每个字符都重写一遍
        p.miswrite_rewrite_mode = MiswriteMode::Rewrite;
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        let text = "甲乙丙".to_string();
        let r = layout_page(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        assert_eq!(r.consumed, 3);
        // 3 个错字 + 3 个重写 = 6 个字形宽度（约 36px 每字），比只排 3 字明显更宽
        let last_ink_x = r.mask.chunks(600).flat_map(|row| row.iter().rposition(|&b| b)).max().unwrap();
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let r0 = layout_page(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(7), &text, 0, 600, 400);
        let last_ink_x0 = r0.mask.chunks(600).flat_map(|row| row.iter().rposition(|&b| b)).max().unwrap();
        assert!(last_ink_x > last_ink_x0 + 30, "Rewrite 应把最右墨迹推到更远处：{last_ink_x} vs {last_ink_x0}");
    }
```

注意：`layout.rs` tests 模块当前未 `use MiswriteMode`——Step 2 编译失败时按 Step 3 一起修复。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib core::layout::`
Expected: FAIL（`MiswriteMode` 未导入 / 断言不满足）。

- [ ] **Step 3: 实现（layout.rs）**

模块内新增两个辅助函数（放在 `layout_page` 之前）：

```rust
/// 在掩码中画一条带厚度的线段（删除线用）。逐行 bool 掩码，越界自动忽略。
fn draw_thick_line(
    mask: &mut [bool],
    width: usize,
    height: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
) {
    let r = thickness / 2.0;
    let steps = ((x1 - x0).abs().max((y1 - y0).abs())).ceil().max(1.0) as usize;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let cx = x0 + (x1 - x0) * t;
        let cy = y0 + (y1 - y0) * t;
        let x_lo = (cx - r).floor() as isize;
        let x_hi = (cx + r).ceil() as isize;
        let y_lo = (cy - r).floor() as isize;
        let y_hi = (cy + r).ceil() as isize;
        for yy in y_lo..=y_hi {
            for xx in x_lo..=x_hi {
                if xx >= 0 && yy >= 0 && (xx as usize) < width && (yy as usize) < height {
                    mask[(yy as usize) * width + (xx as usize)] = true;
                }
            }
        }
    }
}

/// 对错字字符绘制删除线与重写小字（正上方略偏右）。
/// `y_top` 为该字符的行顶坐标（同 layout_page/placed 的 y 语义），`angle` 为删除线倾角（rad）。
#[allow(clippy::too_many_arguments)]
fn draw_miswrite_above(
    mask: &mut [bool],
    width: usize,
    height: usize,
    font: &FontFace,
    params: &HandwritingParams,
    ch: char,
    x: f32,
    y_top: f32,
    size: f32,
    angle: f32,
) {
    let advance = font.glyph_width(ch, size);
    // 删除线：跨字形宽度的旋转粗线，位于字符竖直中线
    let mid = y_top + font.ascent(size) * 0.45;
    let (ct, st) = (angle.cos(), angle.sin());
    let half = advance * 0.45;
    let (rx, ry) = (half * ct, half * st);
    let thickness = (size / 8.0).max(2.0);
    draw_thick_line(mask, width, height, x + rx, mid - ry, x - rx, mid + ry, thickness);
    // 小一号重写：正上方略偏右，基线不低于 0（首行避免裁掉）
    let small = (size * 0.6).max(1.0);
    let small_x = x + size * 0.15;
    let small_baseline = (y_top - size * 0.85 + font.ascent(small)).max(font.ascent(small));
    font.rasterize(ch, small, small_x, small_baseline, mask, width, height);
}
```

在 `layout_page` 的 RNG 初始化处新增删除线角度分布（`normal_font` 之后）：

```rust
    let normal_strike = Normal::new(0.0, 0.15).unwrap();
```

修改 `layout_page` 内层循环：先在字距推进前捕获字符位置，再在 `i += 1;` 之后插入错字处理（原代码 `x += ...` 在 `i += 1` 之前，保持 RNG 顺序不变）：

```rust
            // 字距推进（含字形宽度与扰动）——先记录字符起始位置供错字效果使用
            let char_x = x;
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;

            i += 1;

            // 写错字模拟：判定只影响渲染，不参与换行；rate=0 时不消耗 RNG（零回归）
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                let angle = normal_strike.sample(rng) as f32;
                draw_miswrite_above(&mut mask, width, height, font, params, ch, char_x, yj, size, angle);
                if params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                    // 后文正常重写：x 额外推进一个字形宽度，紧邻重写同一字符
                    x += font.glyph_width(ch, size) + params.word_spacing;
                    font.rasterize(ch, size, x, baseline_y, &mut mask, width, height);
                }
            }

            if i >= text_len {
                return LayoutResult { mask, consumed: i };
            }
```

同时在测试模块 `use super::*;` 已覆盖 `MiswriteMode`（super 导出 models 的 import 链：`use crate::core::models::{Align, HandwritingParams, Paragraph};` 需补 `MiswriteMode`）——改为：

```rust
    use crate::core::models::{Align, MiswriteMode, Paragraph};
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib core::layout::`
Expected: PASS（含 2 个新测试；`same_seed_same_layout` 等既有测试不受影响——rate 默认 0 不消耗 RNG）。

- [ ] **Step 5: 提交**

```bash
git add src/core/layout.rs
git commit -m "feat(core): 文本路径写错字效果（删除线 + 上方/后文重写）"
```

---

## Task 3: 段落路径错字效果（layout_paragraph）

**Files:**
- Modify: `src/core/layout.rs`
- Test: `src/core/layout.rs`（tests 模块）

- [ ] **Step 1: 写失败测试（layout.rs tests 模块追加）**

```rust
    /// 段落路径：错字率>0 增加前景；同 seed 两次渲染逐像素一致。
    #[test]
    fn paragraph_miswrite_adds_ink_and_is_deterministic() {
        let Some(path) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let font = FontFace::load(&path, 36.0).unwrap();
        let mut p = params();
        p.word_spacing_sigma = 0.0;
        p.font_size_sigma = 0.0;
        p.line_spacing_sigma = 0.0;
        p.miswrite_rate = 0.8;
        p.miswrite_rewrite_mode = MiswriteMode::Above;
        let mut pa = para();
        pa.text = "今天天气很好，我们去公园散步。".into();
        let a = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        let b = layout_paragraph(&p, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        assert_eq!(a.len(), b.len());
        for ((ma, _), (mb, _)) in a.iter().zip(b.iter()) {
            assert_eq!(ma, mb, "同 seed 应逐像素一致");
        }
        let ink_a: usize = a.iter().filter_map(|(m, _)| m.as_ref()).map(|m| m.iter().filter(|&&v| v).count()).sum();
        let mut p0 = p.clone();
        p0.miswrite_rate = 0.0;
        let z = layout_paragraph(&p0, &font, &mut rand::rngs::StdRng::seed_from_u64(9), &pa, 600);
        let ink_z: usize = z.iter().filter_map(|(m, _)| m.as_ref()).map(|m| m.iter().filter(|&&v| v).count()).sum();
        assert!(ink_a > ink_z, "错字效果应增加前景像素：{ink_a} vs {ink_z}");
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib core::layout::paragraph_miswrite`
Expected: FAIL（新增测试无法通过——错字效果未实现）。

- [ ] **Step 3: 实现（layout.rs）**

将 `layout_paragraph` 阶段一的 `placed` 改为带错字信息的结构。替换：

```rust
    let mut placed: Vec<(char, f32, f32, f32, usize)> = Vec::new();
```

为：

```rust
    #[derive(Clone, Copy)]
    struct Placed {
        ch: char,
        x: f32,
        y: f32,
        size: f32,
        line: usize,
        miswrite: bool,
        angle: f32,
    }
    let mut placed: Vec<Placed> = Vec::new();
```

注意 `Placed` 结构定义需放在 `layout_paragraph` 函数体内（阶段一使用）——直接在函数内定义即可。

在 `layout_paragraph` 阶段一新增删除线角度分布（`normal_font` 之后）：

```rust
    let normal_strike = Normal::new(0.0, 0.15).unwrap();
```

修改阶段一推进逻辑（`placed.push(...)` 之后）：

```rust
            placed.push(Placed { ch, x, y: yj, size, line: line_ys.len() - 1, miswrite: false, angle: 0.0 });
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;
            i += 1;
            // 错字判定（RNG 消费顺序与文本路径一致：字符扰动之后）；rate=0 不消耗
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                if let Some(last) = placed.last_mut() {
                    last.miswrite = true;
                    last.angle = normal_strike.sample(rng) as f32;
                }
            }
```

修改阶段二绘制循环：

```rust
    for item in &placed {
        let dx = match &shifts {
            Some(s) => item.x + s[item.line],
            None => item.x,
        };
        let baseline_y = item.y + font.ascent(item.size);
        font.rasterize(item.ch, item.size, dx, baseline_y, &mut mask, width, canvas_h);
        if item.miswrite {
            draw_miswrite_above(&mut mask, width, canvas_h, font, params, item.ch, dx, item.y, item.size, item.angle);
            if params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                let dx2 = dx + font.glyph_width(item.ch, item.size) + params.word_spacing;
                font.rasterize(item.ch, item.size, dx2, baseline_y, &mut mask, width, canvas_h);
            }
        }
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib core::layout::`
Expected: PASS（含 `paragraph_miswrite_adds_ink_and_is_deterministic`；`layout_paragraph_produces_lines` 等既有测试不受影响）。

- [ ] **Step 5: 提交**

```bash
git add src/core/layout.rs
git commit -m "feat(core): 段落路径写错字效果（对齐文本路径 RNG 顺序）"
```

---

## Task 4: 预设 JSON 支持错字字段

**Files:**
- Modify: `src/core/presets.rs`
- Test: `src/core/presets.rs`（tests 模块）

- [ ] **Step 1: 写失败测试（presets.rs tests 模块追加）**

```rust
    #[test]
    fn preset_roundtrips_miswrite_fields_and_old_json_defaults() {
        use crate::core::models::MiswriteMode;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset.json");
        let mut p = sample_params();
        p.miswrite_rate = 0.12;
        p.miswrite_rewrite_mode = MiswriteMode::Rewrite;
        save(&p, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.miswrite_rate, 0.12);
        assert_eq!(loaded.miswrite_rewrite_mode, MiswriteMode::Rewrite);
        // 旧格式预设（无新字段）应回退默认值
        let old = dir.path().join("old.json");
        std::fs::write(&old, r#"{"version": 2, "params": {"font_size": 30}}"#).unwrap();
        let legacy = load(&old).unwrap();
        assert_eq!(legacy.miswrite_rate, 0.0);
        assert_eq!(legacy.miswrite_rewrite_mode, MiswriteMode::Above);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib core::presets::`
Expected: FAIL（`miswrite_rate` 未保存/载入）。

- [ ] **Step 3: 实现（presets.rs）**

`to_preset_map` 中 `perturb_theta_sigma` 之后追加：

```rust
    m.insert("miswrite_rate".into(), json!(params.miswrite_rate));
    m.insert("miswrite_rewrite_mode".into(), json!(params.miswrite_rewrite_mode.as_str()));
```

`from_preset_map` 中 `start_chars` 处理之后追加：

```rust
    if let Some(v) = num("miswrite_rate") { p.miswrite_rate = v; }
    if let Some(v) = str_("miswrite_rewrite_mode") {
        match crate::core::models::MiswriteMode::parse(&v) {
            Ok(m) => p.miswrite_rewrite_mode = m,
            Err(_) => return Err(PresetError::Format(format!("miswrite_rewrite_mode 未知：{v:?}"))),
        }
    }
```

`models.rs` 的 `MiswriteMode` 增加 `as_str`/`parse`（在 Task 1 基础上补充）：

```rust
impl MiswriteMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            MiswriteMode::Above => "above",
            MiswriteMode::Rewrite => "rewrite",
        }
    }

    pub fn parse(s: &str) -> Result<MiswriteMode, String> {
        match s {
            "above" => Ok(MiswriteMode::Above),
            "rewrite" => Ok(MiswriteMode::Rewrite),
            other => Err(format!("未知重写方式：{other:?}，可选 above/rewrite")),
        }
    }
}
```

（此步会同时修改 models.rs——提交时一并 `git add`。）

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib core::presets:: && cargo test --lib core::models::`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/core/presets.rs src/core/models.rs
git commit -m "feat(core): 预设 JSON 支持错字率/重写方式（旧预设兼容默认值）"
```

---

## Task 5: PDF 位图导出

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/core/engine.rs`
- Test: `src/core/engine.rs`（tests 模块）

- [ ] **Step 1: 添加依赖**

```toml
# PDF 导出：位图层方案（MIT；禁用默认 html feature 避开 azul git 依赖）
printpdf = { version = "0.12", default-features = false, features = ["images"] }
```

Run: `cargo build`（验证可编译，首次拉取较慢）。

- [ ] **Step 2: 写失败测试（engine.rs tests 模块追加）**

```rust
    #[test]
    fn export_pdf_produces_valid_multipage_pdf() {
        let Some(font) = system_font() else {
            eprintln!("跳过：未找到系统 CJK 字体");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let bg = dir.path().join("bg.png");
        let mut img = RgbImage::new(400, 200);
        for px in img.pixels_mut() {
            *px = Rgb([255, 255, 255]);
        }
        img.save(&bg).unwrap();

        let mut params = make_params(&font, &bg);
        params.text = "这是第一页的内容，需要足够长才能触发换页。第二行继续。第三行再来一些。第四行补充。".into();
        params.font_size = 36.0;
        params.line_spacing = 40.0;

        let pages = DefaultEngine::new(7).render_pages(&params).unwrap();
        assert!(pages.len() >= 2, "长文本应产生多页，实际 {}", pages.len());

        let out = dir.path().join("out.pdf");
        export_pdf(&params, &out, 7).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "应以 %PDF- 开头");
        assert!(bytes.len() > 1000, "PDF 应包含图像数据");

        // 用 printpdf 读回验证页数与页尺寸
        let mut warnings = Vec::new();
        let doc = printpdf::PdfDocument::parse(&bytes, &printpdf::PdfParseOptions::default(), &mut warnings)
            .unwrap_or_else(|e| panic!("PDF 解析失败：{e}"));
        assert_eq!(doc.page_count(), pages.len(), "PDF 页数应与 render_pages 一致");
        // 页物理尺寸 ≈ 像素 @ 300 DPI
        let (w, h) = (pages[0].dimensions());
        let page = doc.pages.first().expect("至少一页");
        let expect_w_mm = w as f32 * 25.4 / 300.0;
        let expect_h_mm = h as f32 * 25.4 / 300.0;
        assert!((page.width - expect_w_mm).abs() < 0.1, "页宽 {} vs {expect_w_mm}", page.width);
        assert!((page.height - expect_h_mm).abs() < 0.1, "页高 {} vs {expect_h_mm}", page.height);
        fs::remove_dir_all(dir.path()).ok();
    }
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --lib core::engine::export_pdf`
Expected: 编译失败（`export_pdf` 未定义）。

- [ ] **Step 4: 实现（engine.rs）**

`EngineError` 增加变体：

```rust
    #[error("PDF 导出失败：{0}")]
    Pdf(String),
```

模块末尾（`render_all_pages_preview` 之后）新增：

```rust
/// 便捷入口：导出 PDF（位图层方案，300 DPI）。
///
/// 复用 `render_pages` 全分辨率渲染，逐页嵌入位图；
/// 页物理尺寸 = 像素 @ 300 DPI（A4 扫描背景 2480×3508 → 恰好 A4 页）。
pub fn export_pdf(
    params: &HandwritingParams,
    out_path: &Path,
    seed: u64,
) -> Result<(), EngineError> {
    let pages = DefaultEngine::new(seed).render_pages(params)?;
    let mut doc = printpdf::PdfDocument::new("handwrite-sim");
    let mut pdf_pages = Vec::with_capacity(pages.len());
    for page in &pages {
        let (w, h) = page.dimensions();
        let raw = printpdf::RawImage::from_dynamic_image(image::DynamicImage::ImageRgba8(page.clone()))
            .map_err(EngineError::Pdf)?;
        let id = doc.add_image(&raw);
        let ops = vec![printpdf::Op::UseXobject {
            id,
            transform: printpdf::XObjectTransform {
                dpi: Some(300.0),
                ..Default::default()
            },
        }];
        pdf_pages.push(printpdf::PdfPage::new(
            printpdf::Mm(w as f32 * 25.4 / 300.0),
            printpdf::Mm(h as f32 * 25.4 / 300.0),
            ops,
        ));
    }
    doc.with_pages(pdf_pages);
    let mut warnings = Vec::new();
    let bytes = doc.save(&printpdf::PdfSaveOptions::default(), &mut warnings);
    std::fs::write(out_path, bytes)?;
    Ok(())
}
```

注意：`RawImage::from_dynamic_image` 返回 `Result<Self, String>`，`.map_err(EngineError::Pdf)?` 即可（`Pdf(String)` 变体）。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --lib core::engine::export_pdf`
Expected: PASS。若页尺寸断言失败（printpdf 页宽单位为 mm 的表示差异），检查 `PdfPage.width` 字段类型（`Mm` 包装 f32）后用 `.0` 或直接比较，调整断言代码。

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml Cargo.lock src/core/engine.rs
git commit -m "feat(core): PDF 位图层导出（printpdf 0.12，300 DPI 页尺寸）"
```

---

## Task 6: GUI 接线（错字参数 + 导出 PDF 按钮）

**Files:**
- Modify: `src/ui/main_window.slint`
- Modify: `src/main.rs`
- Test: 手动验证

- [ ] **Step 1: slint 添加「写错字」分组与「导出 PDF」按钮**

`main_window.slint` 顶部 in-out 属性区（`perturb-theta` 绑定之后）追加：

```slint
    // 写错字模拟：错字率（0~30%）+ 重写方式
    in-out property <float> miswrite-rate <=> miswrite-rate-slider.value;
    in-out property <int> miswrite-mode-index <=> miswrite-mode-combo.current-index;
```

回调区（`toggle-preview-bg;` 之后）追加：

```slint
    callback export-pdf;
```

「笔画扰动」分组框结束后（`perturb-theta-slider` 所在 Row 之后、分组框 Rectangle 闭合之前）插入新分组框：

```slint
                        // ---- 写错字模拟 ----
                        Rectangle {
                            border-radius: 6px;
                            border-width: 1px;
                            border-color: Theme.group-border;
                            GroupTitle { text: "写错字"; x: 10px; y: -8px; }
                            GridLayout {
                                padding-top: 16px;
                                padding-bottom: 10px;
                                padding-left: 10px;
                                padding-right: 10px;
                                spacing: 6px;
                                Row {
                                    FieldLabel { text: "错字率"; }
                                    miswrite-rate-slider := Slider {
                                        value: 0; minimum: 0; maximum: 30; step-size: 0.1;
                                        horizontal-stretch: 1;
                                    }
                                    Text {
                                        text: "${round(root.miswrite-rate * 10) / 10}%";
                                        min-width: 56px;
                                        horizontal-alignment: center;
                                        vertical-alignment: center;
                                        font-size: 12px;
                                        color: Theme.text;
                                    }
                                }
                                Row {
                                    FieldLabel { text: "重写方式"; }
                                    miswrite-mode-combo := ComboBox {
                                        model: ["正上方重写", "后文重写"];
                                        current-index: 0;
                                        horizontal-stretch: 1;
                                    }
                                }
                            }
                        }
```

底部按钮行（`导出` PrimaryButton 之后）追加：

```slint
                    PrimaryButton {
                        text: "导出 PDF";
                        horizontal-stretch: 1;
                        clicked => { root.export-pdf(); }
                    }
```

- [ ] **Step 2: main.rs 接线**

`use` 补充：

```rust
use handwrite_sim::core::models::{parse_color, Align, HandwritingParams, MiswriteMode, Paragraph};
use handwrite_sim::core::engine::{export, export_pdf, overlay_bounds, render_all_pages_preview, EngineError};
```

`collect_params` 中 `params.fill = ...` 之前追加：

```rust
    // 写错字模拟
    params.miswrite_rate = ui.get_miswrite_rate() as f32 / 100.0;
    params.miswrite_rewrite_mode = match ui.get_miswrite_mode_index() {
        1 => MiswriteMode::Rewrite,
        _ => MiswriteMode::Above,
    };
```

`apply_preset_to_ui` 中 `ui.set_perturb_theta_sigma(...)` 之后追加：

```rust
    ui.set_miswrite_rate(p.miswrite_rate * 100.0);
    ui.set_miswrite_mode_index(match p.miswrite_rewrite_mode {
        MiswriteMode::Above => 0,
        MiswriteMode::Rewrite => 1,
    });
```

「导出图片」回调块之后追加导出 PDF 回调：

```rust
    // ---- 导出 PDF（位图层，300 DPI） ----
    {
        let weak = ui.as_weak();
        let seed = Rc::clone(&seed_counter);
        let preset_params = Rc::clone(&preset_params);
        ui.on_export_pdf(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name("handwrite.pdf")
                .save_file()
            else {
                return;
            };
            let params = match collect_params(&ui, &preset_params) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                    return;
                }
            };
            let seed_val = *seed.borrow();
            match export_pdf(&params, &path, seed_val) {
                Ok(()) => ui.set_status_text(SharedString::from(format!("PDF 已导出：{}", path.display()))),
                Err(e) => ui.set_status_text(SharedString::from(format!("导出 PDF 失败：{e}"))),
            }
        });
    }
```

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: 编译通过，无警告。

- [ ] **Step 4: 手动验证 GUI**

Run: `cargo run`
手动步骤：
1. 选择字体（如 `C:\Windows\Fonts\msyh.ttc`）与背景（可用 `assets/` 或任意 PNG）。
2. 输入较长文本，错字率设为 20%，分别试两种重写方式 → 预览出现删除线与重写字，翻页正常。
3. 错字率调回 0 → 预览与旧版一致。
4. 「导出 PDF」选择保存路径 → 状态栏提示成功；用 PDF 阅读器打开：页尺寸正确（A4 扫描背景应为 A4），内容与预览一致（注意：位图层不可复制是预期行为）。

- [ ] **Step 5: 提交**

```bash
git add src/ui/main_window.slint src/main.rs
git commit -m "feat(ui): 写错字参数面板 + 导出 PDF 按钮接线"
```

---

## Task 7: 集成测试 + 全量验证

**Files:**
- Modify: `tests/test_engine_integration.rs`
- Test: 全量

- [ ] **Step 1: 集成测试（tests/test_engine_integration.rs 追加）**

复用文件顶部已有的 `system_font` / `make_params` / `is_ink` 辅助函数，追加：

```rust
#[test]
fn miswrite_preview_matches_export_with_same_seed() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg.png");
    let mut img = RgbImage::new(500, 400);
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]);
    }
    img.save(&bg).unwrap();

    let mut params = make_params(&font, &bg);
    params.text = "今天天气很好，我们去公园散步，看花看草，心情舒畅。".into();
    params.font_size = 36.0;
    params.line_spacing = 44.0;
    params.miswrite_rate = 0.3;
    params.miswrite_rewrite_mode = MiswriteMode::Above;

    // 同 seed：预览全部页 = 预览首页 = 导出逐像素一致
    let pages = render_all_pages_preview(&params, 42).unwrap();
    let preview = render_preview(&params, 42).unwrap();
    assert_eq!(pages[0].as_raw(), preview.as_raw(), "预览首帧应与 render_preview 一致");
    let out = dir.path().join("out");
    let files = export(&params, &out, 42).unwrap();
    assert_eq!(files.len(), pages.len());
    for (path, page) in files.iter().zip(pages.iter()) {
        let saved = image::open(path).unwrap().to_rgba8();
        assert_eq!(saved.as_raw(), page.as_raw(), "导出应与预览逐像素一致");
    }

    // 错字效果确实生效：墨迹多于关闭时
    let mut p0 = params.clone();
    p0.miswrite_rate = 0.0;
    let pages0 = render_all_pages_preview(&p0, 42).unwrap();
    let ink = |p: &RgbaImage| -> usize { p.pixels().filter(|px| is_ink(px)).count() };
    let sum: usize = pages.iter().map(ink).sum();
    let sum0: usize = pages0.iter().map(ink).sum();
    assert!(sum > sum0, "错字效果应增加墨迹：{sum} vs {sum0}");
    fs::remove_dir_all(dir.path()).ok();
}
```

同步更新文件顶部 import（`use` 行）：

```rust
use handwrite_sim::core::engine::{export, render_all_pages_preview, render_preview, Engine};
use handwrite_sim::core::models::{Align, HandwritingParams, MiswriteMode, Paragraph};
```

- [ ] **Step 2: 全量验证**

Run:

```bash
cargo test
cargo clippy --all-targets
cargo build --release
```

Expected: 全部通过，clippy 无警告，release 构建成功。

- [ ] **Step 3: 提交**

```bash
git add tests/test_engine_integration.rs
git commit -m "test: 错字效果同 seed 预览=导出集成测试"
```

- [ ] **Step 4: 更新 README 进度**

`README.md`「当前进度」中把阶段四行改为（对照现有格式）：

```markdown
- [x] PDF 导出（位图层）、写错字划掉重写（错字率驱动）
- [ ] 混合排版（打印体 + 手写体多字体管线，需求待定）
```

Run: 无测试。
Commit:

```bash
git add README.md
git commit -m "docs: 阶段四进度更新"
```
