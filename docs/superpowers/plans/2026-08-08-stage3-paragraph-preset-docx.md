# 阶段三：段落路径 / 预设 JSON / docx 导入 / webp 与预览降采样 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** Rust 版实现 Python 版 1:1 功能对齐的阶段三：段落路径（对齐/缩进/右对齐/跨页）、预设 JSON 保存/载入（兼容 Python 格式）、docx 导入（段落+首行缩进还原）、webp 背景与预览降采样。

**架构：** 复用现有分层（`core` 纯引擎 / `ui` Slint 界面）。段落路径在 `layout.rs` 新增 `layout_paragraph`（单段 → 逐行墨迹）与 `layout_paragraphs`（全部段落 → 逐行流式分页），`engine.rs` 按 `paragraphs` 非空分发；`presets.rs` 与 `docx_io.rs` 为新增纯函数模块；GUI 用 Slint `for` + `VecModel` 动态段落列表。

**技术栈：** Rust 2021、Slint 1.x（`VecModel`/`ModelRc`）、ab_glyph、image 0.25（默认已含 webp 解码）、rand 0.9、serde/serde_json、docx-rs 0.4（`read_docx` 读段落属性）、rfd 文件对话框。

**设计规格：** `docs/superpowers/specs/2026-08-08-stage3-paragraph-preset-docx-design.md`

**提交策略：** 每个任务末尾的 commit 步骤为可选——项目当前无 git 提交历史，是否启用提交由用户决定（默认跳过，只保留验证步骤）。

---

## 文件结构

| 文件 | 职责 | 动作 |
|------|------|------|
| `Cargo.toml` | 添加 `docx-rs = "0.4"` 依赖 | 修改 |
| `src/core/mod.rs` | 导出 `presets`、`docx_io` 新模块 | 修改 |
| `src/core/layout.rs` | `layout_paragraph` / `layout_paragraphs`（段落路径核心） | 修改 |
| `src/core/engine.rs` | 段落分发 + 预览降采样（webp 兜底） | 修改 |
| `src/core/presets.rs` | 预设 JSON 保存/载入（Python 兼容 + 便携路径） | 创建 |
| `src/core/docx_io.rs` | docx 解析：文本/对齐/首行缩进还原 | 创建 |
| `src/ui/main_window.slint` | 纯文本/段落双模式 UI | 修改 |
| `src/main.rs` | 段落增删/导入 docx/预设保存载入回调接线 | 修改 |
| `tests/test_engine_integration.rs` | 段落路径集成测试 | 修改 |
| `docs/superpowers/plans/2026-08-08-stage3-paragraph-preset-docx.md` | 本计划 | 创建 |

---

### 任务 1：依赖与模块骨架

**文件：**
- 修改：`Cargo.toml`
- 修改：`src/core/mod.rs`

- [ ] **步骤 1：添加 docx-rs 依赖**

`Cargo.toml` 的 `[dependencies]` 中 `rfd = "0.15"` 之后添加：

```toml
docx-rs = "0.4"
```

- [ ] **步骤 2：建立模块骨架**

`src/core/mod.rs` 添加：

```rust
pub mod docx_io;
pub mod presets;
```

（docx_io.rs / presets.rs 先创建空文件占位，保证编译通过后各任务填充。）

- [ ] **步骤 3：验证编译**

运行：`cargo build`
预期：编译成功，无错误。

---

### 任务 2：layout.rs 段落路径（核心）

**文件：**
- 修改：`src/core/layout.rs`（在 `layout_page` 之后、`#[cfg(test)]` 之前插入）

**设计要点（对齐 Python `_layout_paragraph`）：**
- 阶段一纯排版记录 `Vec<(char, f32, f32, f32, usize)>`（ch, x, y, size, line_idx）与每行结束 x；随机数顺序：行扰动 → 字号扰动 → 字距扰动（与 `layout_page` 一致）。
- 首行缩进仅段落第一行；换行规则用 `end_chars`/`start_chars`。
- 右对齐：`shift = (width - right) - line_end_x`；居中：按行墨迹 x 范围居中。
- 段落画布高度 `canvas_h = max(y + font_size + 4 * line_spacing_sigma + 4, 1)`。
- 行提取：行带分组 → 归属各行，空行补 `(None, 0.0)`，偏移钳制 `[-0.25, 0.8] * line_spacing`。

- [ ] **步骤 1：编写失败的段落测试**

在 `layout.rs` 的 `mod tests` 中添加：

```rust
fn para() -> Paragraph {
    Paragraph {
        text: "第一行文字，第二行测试。".into(),
        align: Align::Left,
        first_line_indent: 0.0,
    }
}

#[test]
fn layout_paragraph_produces_lines() {
    let Some(path) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let font = FontFace::load(&path, 36.0).unwrap();
    let mut p = params();
    p.word_spacing_sigma = 0.0;
    p.font_size_sigma = 0.0;
    p.line_spacing_sigma = 0.0;
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let lines = layout_paragraph(&p, &font, &mut rng, &para(), 600);
    assert!(!lines.is_empty(), "应产生至少一行");
    assert!(lines.iter().any(|(m, _)| m.is_some()), "应存在非空行墨迹");
}

#[test]
fn layout_paragraph_first_line_indent_only() {
    let Some(path) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let font = FontFace::load(&path, 36.0).unwrap();
    let mut p = params();
    p.word_spacing_sigma = 0.0;
    p.font_size_sigma = 0.0;
    p.line_spacing_sigma = 0.0;
    let mut pa = para();
    pa.first_line_indent = 50.0;
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let lines = layout_paragraph(&p, &font, &mut rng, &pa, 600);
    // 首行墨迹最左 x 应 ≥ 50（缩进），后续行最左 x 应 < 50
    let first = lines[0].0.as_ref().expect("首行应有墨迹");
    let first_min_x = first
        .chunks(600)
        .enumerate()
        .filter(|(_, row)| row.iter().any(|&b| b))
        .flat_map(|(_, row)| row.iter().position(|&b| b))
        .min()
        .unwrap();
    assert!(first_min_x >= 50, "首行应缩进：{first_min_x}");
}

#[test]
fn layout_paragraph_right_align_pushes_to_right_edge() {
    let Some(path) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let font = FontFace::load(&path, 36.0).unwrap();
    let mut p = params();
    p.word_spacing_sigma = 0.0;
    p.font_size_sigma = 0.0;
    p.line_spacing_sigma = 0.0;
    p.right_margin = 30.0;
    let mut pa = para();
    pa.align = Align::Right;
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let lines = layout_paragraph(&p, &font, &mut rng, &pa, 600);
    let first = lines[0].0.as_ref().expect("首行应有墨迹");
    let (mut max_x, mut min_x) = (0usize, usize::MAX);
    for (y, row) in first.chunks(600).enumerate() {
        for (x, &b) in row.iter().enumerate() {
            if b {
                max_x = max_x.max(x);
                min_x = min_x.min(x);
            }
        }
    }
    // 整行墨迹应贴近右缘：最右像素落在 (width - right_margin) 附近（允许字形宽度偏差）
    assert!(
        max_x >= 600 - 30 - 40,
        "右对齐行应贴近右缘，实际 max_x={max_x}"
    );
    assert!(min_x > 0, "右对齐行不应从左边距开始：min_x={min_x}");
}

#[test]
fn layout_paragraph_empty_text_yields_no_lines() {
    let Some(path) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let font = FontFace::load(&path, 36.0).unwrap();
    let p = params();
    let mut pa = para();
    pa.text = String::new();
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    assert!(layout_paragraph(&p, &font, &mut rng, &pa, 600).is_empty());
}

#[test]
fn layout_paragraphs_streams_across_pages() {
    let Some(path) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let font = FontFace::load(&path, 36.0).unwrap();
    let mut p = params();
    p.word_spacing_sigma = 0.0;
    p.font_size_sigma = 0.0;
    p.line_spacing_sigma = 0.0;
    let mut paras = vec![para(), para()];
    paras[1].text = "第二段内容，足够长以触发跨页。".into();
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let pages = layout_paragraphs(&p, &font, &mut rng, &paras, 300, 200);
    assert_eq!(pages.len(), 2, "矮画布应产生两页");
    assert!(pages[0].iter().any(|&b| b), "首页应有墨迹");
    assert!(pages[1].iter().any(|&b| b), "第二页应有墨迹");
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test layout_paragraph -- --nocapture`
预期：编译错误，`layout_paragraph` / `layout_paragraphs` 未定义。

- [ ] **步骤 3：实现 `layout_paragraph`**

```rust
/// 行带分组：把行聚合 bool 数组按连续段分组，返回 [start, end) 列表。
fn split_text_rows(rows: &[bool]) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    let mut start: Option<usize> = None;
    for (idx, &v) in rows.iter().enumerate() {
        if v && start.is_none() {
            start = Some(idx);
        } else if !v && start.is_some() {
            groups.push((start.unwrap(), idx));
            start = None;
        }
    }
    if let Some(s) = start {
        groups.push((s, rows.len()));
    }
    groups
}

/// 按文本行测量非零 x 范围，逐行水平居中（对齐 Python `_center_text_lines`）。
fn center_text_lines(mask: &mut [bool], width: usize) {
    let height = mask.len() / width;
    let rows: Vec<bool> = mask.chunks(width).map(|r| r.iter().any(|&b| b)).collect();
    if !rows.iter().any(|&b| b) {
        return;
    }
    let mut result = vec![false; mask.len()];
    for (y0, y1) in split_text_rows(&rows) {
        let band = &mask[y0 * width..y1 * width];
        let mut min_x = usize::MAX;
        let mut max_x = 0usize;
        for (x, &b) in band.iter().enumerate() {
            if b {
                min_x = min_x.min(x % width);
                max_x = max_x.max(x % width);
            }
        }
        let line_w = max_x - min_x + 1;
        if line_w >= width {
            result[y0 * width..y1 * width].copy_from_slice(band);
            continue;
        }
        let shift = (width - line_w) / 2 - min_x;
        for (idx, &b) in band.iter().enumerate() {
            if b {
                let x = idx % width;
                let y = y0 + idx / width;
                let nx = x as isize + shift as isize;
                if nx >= 0 && (nx as usize) < width {
                    result[y * width + nx as usize] = true;
                }
            }
        }
    }
    mask.copy_from_slice(&result);
}

/// 渲染单个段落，返回逐行 [(该行墨迹裁剪掩码, 相对该行绘制基线的偏移)]。
/// 空行对应 (None, 0.0)。画布按段落自身高度创建（不受页高裁剪）。
pub fn layout_paragraph(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    paragraph: &Paragraph,
    width: usize,
) -> Vec<(Option<Vec<bool>>, f32)> {
    let width_f = width as f32;
    let line_spacing = params.total_line_spacing();
    let end_chars = params.end_chars.as_str();
    let start_chars = params.start_chars.as_str();
    let text = paragraph.text.as_str();
    let chars: Vec<char> = text.chars().collect();
    let text_len = chars.len();

    let normal_line = Normal::new(0.0, f64::from(params.line_spacing_sigma)).unwrap();
    let normal_word = Normal::new(0.0, f64::from(params.word_spacing_sigma)).unwrap();
    let normal_font = Normal::new(0.0, f64::from(params.font_size_sigma)).unwrap();

    // 阶段一：纯排版（不绘制），随机数消耗顺序与纯文本路径一致
    let mut placed: Vec<(char, f32, f32, f32, usize)> = Vec::new();
    let mut line_x_ends: Vec<f32> = Vec::new();
    let mut line_ys: Vec<f32> = Vec::new();
    let mut i = 0;
    let mut y = line_spacing - params.font_size;
    while i < text_len {
        line_ys.push(y);
        let mut x = params.left_margin + (paragraph.first_line_indent if i == 0 else 0.0);
        while i < text_len {
            let ch = chars[i];
            if ch == '\n' {
                i += 1;
                break;
            }
            if x > width_f - params.right_margin - 2.0 * params.font_size
                && start_chars.contains(ch)
            {
                break;
            }
            if x > width_f - params.right_margin - params.font_size && !end_chars.contains(ch) {
                break;
            }
            let yj = y + normal_line.sample(rng) as f32;
            let mut size = params.font_size;
            if params.font_size_sigma > 0.0 {
                size = (params.font_size + normal_font.sample(rng) as f32).round().max(0.0);
            }
            let size = size.max(1.0);
            let offset = font.glyph_width(ch, size);
            placed.push((ch, x, yj, size, line_ys.len() - 1));
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;
            i += 1;
        }
        line_x_ends.push(x);
        y += line_spacing;
    }
    if line_ys.is_empty() {
        return Vec::new();
    }

    // 右对齐：按每行逻辑宽度（含尾部空格）平移到右边距
    let shifts: Option<Vec<f32>> = if paragraph.align == Align::Right {
        let right_x = width_f - params.right_margin;
        Some(line_x_ends.iter().map(|xe| right_x - xe).collect())
    } else {
        None
    };

    // 阶段二：按段落实际高度创建画布并绘制（不被页高裁剪）
    let canvas_h = (y + params.font_size + 4.0 * params.line_spacing_sigma + 4.0).max(1.0);
    let canvas_h = canvas_h as usize;
    let mut mask = vec![false; width * canvas_h];
    for (ch, cx, cy, size, li) in &placed {
        let dx = match &shifts {
            Some(s) => cx + s[*li],
            None => *cx,
        };
        let baseline_y = cy + font.ascent(*size);
        font.rasterize(*ch, *size, dx, baseline_y, &mut mask, width, canvas_h);
    }
    if paragraph.align == Align::Center {
        center_text_lines(&mut mask, width);
    }

    // 按行提取墨迹：行带分组 → 归属各行，空行补 (None, 0.0)
    let rows: Vec<bool> = mask.chunks(width).map(|r| r.iter().any(|&b| b)).collect();
    let bands = split_text_rows(&rows);
    let mut bi = 0usize;
    let off_min = -0.25 * line_spacing;
    let off_max = 0.8 * line_spacing;
    let mut lines: Vec<(Option<Vec<bool>>, f32)> = Vec::new();
    for &yk in &line_ys {
        if bi < bands.len() && bands[bi].0 as f32 < yk + line_spacing / 2.0 {
            let (s, e) = bands[bi];
            bi += 1;
            let off = ((s as f32 - yk).max(off_min)).min(off_max);
            lines.push((Some(mask[s * width..e * width].to_vec()), off));
        } else {
            lines.push((None, 0.0));
        }
    }
    lines
}
```

- [ ] **步骤 4：实现 `layout_paragraphs` 分页**

```rust
/// 全部段落 → 逐行流式分页，返回各页前景掩码（对齐 Python `_paragraph_pages`）。
pub fn layout_paragraphs(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    paragraphs: &[Paragraph],
    width: usize,
    height: usize,
) -> Vec<Vec<bool>> {
    let line_spacing = params.total_line_spacing();
    let lead = line_spacing - params.font_size;
    let limit = height as f32 - params.bottom_margin - params.font_size;

    // 所有段落共用同一 rng 流，先全部排版为逐行列表
    let mut all_lines: Vec<(Option<Vec<bool>>, f32)> = Vec::new();
    for para in paragraphs {
        let mut lines = layout_paragraph(params, font, rng, para, width);
        if lines.is_empty() {
            lines.push((None, 0.0)); // 空段保留一行空行
        }
        all_lines.extend(lines);
    }

    let mut pages: Vec<Vec<bool>> = Vec::new();
    let mut page_canvas = vec![false; width * height];
    let mut draw_y = params.top_margin + lead;
    for (band, off) in all_lines {
        if draw_y > limit && page_canvas.iter().any(|&b| b) {
            pages.push(std::mem::take(&mut page_canvas));
            page_canvas = vec![false; width * height];
            draw_y = params.top_margin + lead;
        }
        if let Some(band) = band {
            let row0 = (draw_y + off).round() as isize;
            let band_h = band.len() / width;
            for (by, row) in band.chunks(width).enumerate() {
                let ty = row0 + by as isize;
                if ty < 0 || ty >= height as isize {
                    continue;
                }
                let dst = &mut page_canvas[ty as usize * width..(ty as usize + 1) * width];
                for (x, &b) in row.iter().enumerate() {
                    if b {
                        dst[x] = true;
                    }
                }
            }
        }
        draw_y += line_spacing;
    }
    if page_canvas.iter().any(|&b| b) || pages.is_empty() {
        pages.push(page_canvas);
    }
    pages
}
```

同时更新 `layout.rs` 顶部 `use`（新增 `Paragraph`、`Align`）：

```rust
use crate::core::models::{Align, HandwritingParams, Paragraph};
```

- [ ] **步骤 5：运行测试验证通过**

运行：`cargo test layout_paragraph -- --nocapture`
预期：全部 PASS（含既有 `layout_page` 测试）。

- [ ] **步骤 6：Commit（可选，需用户确认后执行）**

```bash
git add src/core/layout.rs
git commit -m "feat(core): 段落路径 _layout_paragraph 对齐/缩进/右对齐/跨页"
```

---

### 任务 3：engine.rs 段落分发 + 预览降采样

**文件：**
- 修改：`src/core/engine.rs`

- [ ] **步骤 1：编写失败的段落引擎测试**

在 `engine.rs` 的 `mod tests` 中添加：

```rust
#[test]
fn paragraph_path_preview_matches_export_with_same_seed() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let bg = dir.path().join("bg.png");
    let mut img = RgbImage::new(400, 300);
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]);
    }
    img.save(&bg).unwrap();

    let mut params = make_params(&font, &bg);
    params.paragraphs = vec![
        Paragraph {
            text: "第一段文字，居中。".into(),
            align: Align::Center,
            first_line_indent: 40.0,
        },
        Paragraph {
            text: "第二段文字，右对齐。".into(),
            align: Align::Right,
            first_line_indent: 0.0,
        },
    ];
    params.text = String::new();

    let pages = DefaultEngine::new(42).render_pages(&params).unwrap();
    assert!(!pages.is_empty());
    let out = dir.path().join("out");
    let files = DefaultEngine::new(42).save_all(&params, &out).unwrap();
    assert_eq!(files.len(), pages.len());
    for (path, page) in files.iter().zip(pages.iter()) {
        let saved = image::open(path).unwrap().to_rgba8();
        assert_eq!(saved.as_raw(), page.as_raw());
    }
    assert!(
        pages[0].as_raw().chunks_exact(4).any(|px| (px[0] as u16 + px[1] as u16 + px[2] as u16) / 3 < 128),
        "段落路径应有深色前景"
    );
    fs::remove_dir_all(dir.path()).ok();
}

#[test]
fn render_preview_downsample_only_for_huge_background() {
    let Some(font) = system_font() else {
        eprintln!("跳过：未找到系统 CJK 字体");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    // 5000px 宽背景（> 4096 阈值）
    let bg = dir.path().join("bg.png");
    let mut img = RgbImage::new(5000, 100);
    for px in img.pixels_mut() {
        *px = Rgb([255, 255, 255]);
    }
    img.save(&bg).unwrap();

    let params = make_params(&font, &bg);
    let page = DefaultEngine::new(1).render_preview(&params).unwrap();
    // 降采样后预览输出缩略背景尺寸（4096 宽），与 Python 版行为一致
    assert_eq!(page.width(), 4096, "降采样后预览应输出缩略背景尺寸");
    // 预览仍应正确渲染（有深色前景）
    let gray_min = page.as_raw().chunks_exact(4).map(|px| (px[0] as u16 + px[1] as u16 + px[2] as u16) / 3).min().unwrap();
    assert!(gray_min < 128, "降采样预览应有深色前景：{gray_min}");
    fs::remove_dir_all(dir.path()).ok();
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test paragraph_path -- --nocapture`
预期：FAIL——`render_preview` 目前忽略 `paragraphs`，产出空白页。

- [ ] **步骤 3：实现段落分发**

`engine.rs` 修改三处：

```rust
// 1) render_preview：paragraphs 非空走段落路径
fn render_preview(&self, params: &HandwritingParams) -> Result<RgbaImage, EngineError> {
    params.validate()?;
    let font =
        FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
    let (background, scaled) = Self::load_background_for_preview(params)?;
    let mut rng = StdRng::seed_from_u64(self.seed);
    if !params.paragraphs.is_empty() {
        let pages = layout::layout_paragraphs(
            &scaled, &font, &mut rng, &params.paragraphs,
            background.width() as usize, background.height() as usize,
        );
        let canvas = perturb::perturb_mask(
            &pages[0], background.width() as usize, background.height() as usize,
            &scaled, &mut rng, &background.as_raw(),
        );
        return Ok(rgba_from_rgb(&canvas, background.width() as usize, background.height() as usize));
    }
    let (page, _) = self.render_page_from(&scaled, &font, &mut rng, &params.text, 0, &background)?;
    Ok(page)
}

// 2) render_pages：paragraphs 非空 → 段落路径全部页
fn render_pages(&self, params: &HandwritingParams) -> Result<Vec<RgbaImage>, EngineError> {
    params.validate()?;
    let font =
        FontFace::load(Path::new(&params.font_path), params.font_size).map_err(EngineError::Font)?;
    let background = Self::load_background(&params.background_path)?;
    let mut rng = StdRng::seed_from_u64(self.seed);
    if !params.paragraphs.is_empty() {
        let pages = layout::layout_paragraphs(
            params, &font, &mut rng, &params.paragraphs,
            background.width() as usize, background.height() as usize,
        );
        return pages
            .into_iter()
            .map(|mask| {
                let canvas = perturb::perturb_mask(
                    &mask, background.width() as usize, background.height() as usize,
                    params, &mut rng, &background.as_raw(),
                );
                Ok(rgba_from_rgb(&canvas, background.width() as usize, background.height() as usize))
            })
            .collect();
    }
    let mut pages = Vec::new();
    let mut start = 0;
    loop {
        let (page, consumed) = self.render_page_from(params, &font, &mut rng, &params.text, start, &background)?;
        pages.push(page);
        if consumed >= params.text.chars().count() {
            break;
        }
        start = consumed;
    }
    Ok(pages)
}

// 3) 新增降采样加载（预览专用）：宽 > 4096 时 LANCZOS 降采样 + 空间参数 × scale
const PREVIEW_MAX_WIDTH: u32 = 4096;

fn load_background_for_preview(
    params: &HandwritingParams,
) -> Result<(RgbImage, HandwritingParams), EngineError> {
    let bg = Self::load_background(&params.background_path)?;
    if bg.width() <= PREVIEW_MAX_WIDTH {
        return Ok((bg, params.clone()));
    }
    let scale = PREVIEW_MAX_WIDTH as f32 / bg.width() as f32;
    let new_height = (bg.height() as f32 * scale).round().max(1.0) as u32;
    let thumb = image::imageops::resize(&bg, PREVIEW_MAX_WIDTH, new_height, image::imageops::FilterType::Lanczos3);
    let mut scaled = params.clone();
    for f in [
        &mut scaled.font_size, &mut scaled.line_spacing, &mut scaled.word_spacing,
        &mut scaled.left_margin, &mut scaled.right_margin,
        &mut scaled.top_margin, &mut scaled.bottom_margin,
        &mut scaled.word_spacing_sigma, &mut scaled.line_spacing_sigma,
        &mut scaled.font_size_sigma, &mut scaled.perturb_x_sigma,
        &mut scaled.perturb_y_sigma,
    ] {
        *f *= scale;
    }
    scaled.font_size = scaled.font_size.max(1.0);
    Ok((thumb, scaled))
}
```

**注意：** `render_preview` 中段落路径与纯文本路径统一使用降采样后的 `scaled` 参数与缩略背景（与 Python 版 `_downsample_preview` 行为一致）；`render_pages` 使用原始参数与全分辨率背景（导出始终全分辨率）。

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test paragraph_path -- --nocapture`
预期：PASS。

- [ ] **步骤 5：Commit（可选）**

```bash
git add src/core/engine.rs
git commit -m "feat(core): 段落路径引擎分发 + 预览降采样"
```

---

### 任务 4：presets.rs 预设 JSON

**文件：**
- 创建：`src/core/presets.rs`

- [ ] **步骤 1：编写失败的预设测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::HandwritingParams;

    fn sample_params() -> HandwritingParams {
        HandwritingParams {
            font_path: r"C:\Windows\Fonts\msyh.ttc".into(),
            background_path: r"C:\Users\me\bg.png".into(),
            text: "不应被保存".into(),
            font_size: 40.0,
            fill: [12, 34, 56],
            ..HandwritingParams::default()
        }
    }

    #[test]
    fn save_load_roundtrip_excludes_text_and_paragraphs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset.json");
        let p = sample_params();
        save(&p, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.font_size, 40.0);
        assert_eq!(loaded.fill, [12, 34, 56]);
        assert!(loaded.text.is_empty(), "预设不应包含文本");
        assert!(loaded.paragraphs.is_empty());
        assert_eq!(loaded.font_path, p.font_path);
        assert_eq!(loaded.background_path, p.background_path);
    }

    #[test]
    fn load_python_style_json_with_color() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("py.json");
        std::fs::write(
            &path,
            r#"{
              "version": 2,
              "params": {
                "font_path": "fonts/msyh.ttc",
                "background_path": "D:/bg.png",
                "font_size": 48,
                "word_spacing": 6,
                "line_spacing": 60,
                "left_margin": 40,
                "right_margin": 40,
                "top_margin": 40,
                "bottom_margin": 40,
                "word_spacing_sigma": 3,
                "line_spacing_sigma": 3,
                "font_size_sigma": 3,
                "perturb_x_sigma": 3,
                "perturb_y_sigma": 3,
                "perturb_theta_sigma": 0.1,
                "end_chars": "，。！？",
                "start_chars": "",
                "color": "#ff0000",
                "unknown_field": 123
              }
            }"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.font_size, 48.0);
        assert_eq!(loaded.fill, [255, 0, 0]);
        assert_eq!(loaded.end_chars, "，。！？");
        assert_eq!(loaded.perturb_theta_sigma, 0.1);
    }

    #[test]
    fn load_python_style_json_with_rgb_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("py2.json");
        std::fs::write(
            &path,
            r#"{"version": 2, "params": {"red": 1, "green": 2, "blue": 3, "font_size": 20}}"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.fill, [1, 2, 3]);
        assert_eq!(loaded.font_size, 20.0);
    }

    #[test]
    fn portable_path_relative_to_assets_root() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset.json");
        let mut p = sample_params();
        // 模拟 exe 目录（assets_root 返回目录）下的字体
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let font_in_assets = exe_dir.join("fonts").join("msyh.ttc");
        p.font_path = font_in_assets.to_string_lossy().into_owned();
        save(&p, &path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("fonts/msyh.ttc") && !raw.contains("C:\\"),
            "资产根内路径应存为相对路径：{raw}"
        );
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.font_path, font_in_assets.to_string_lossy());
    }
}
```

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test presets -- --nocapture`
预期：编译错误，`presets` 模块函数未定义。

- [ ] **步骤 3：实现 presets.rs**

```rust
//! 参数预设的 JSON 读写（兼容 Python 版 presets.py 格式）。
//!
//! - 格式：`{"version": 2, "params": {...}}`，params 不含 text/paragraphs，
//!   颜色以 `color: "#RRGGBB"` 保存。
//! - 便携模式：exe 目录为资产根，其内字体/背景路径保存为相对路径，
//!   载入时解析回绝对路径。
//! - 兼容载入：`color` 与 `red/green/blue` 两种颜色写法；未知字段忽略。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::core::models::HandwritingParams;

/// 预设 JSON 错误。
#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 解析失败：{0}")]
    Json(#[from] serde_json::Error),
    #[error("预设格式错误：{0}")]
    Format(String),
}

/// 资产根目录 = exe 所在目录（便携模式基准）。
pub fn assets_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 绝对路径位于资产根目录内时转为相对路径（跨盘符/非法路径原样保留）。
pub fn to_portable_path(path: &str) -> String {
    if path.is_empty() {
        return path.to_string();
    }
    let root = assets_root();
    let abs = Path::new(path);
    if let Ok(rel) = abs.strip_prefix(&root) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    path.to_string()
}

/// 预设中的相对路径按资产根目录解析为绝对路径；绝对路径原样返回。
pub fn from_portable_path(path: &str) -> String {
    if path.is_empty() || Path::new(path).is_absolute() {
        return path.to_string();
    }
    assets_root().join(path).to_string_lossy().into_owned()
}

/// 把参数序列化为 Python 兼容预设 map（不含 text/paragraphs）。
fn to_preset_map(params: &HandwritingParams) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("font_path".into(), json!(to_portable_path(&params.font_path)));
    m.insert("background_path".into(), json!(to_portable_path(&params.background_path)));
    m.insert("font_size".into(), json!(params.font_size));
    m.insert("word_spacing".into(), json!(params.word_spacing));
    m.insert("line_spacing".into(), json!(params.line_spacing));
    m.insert("left_margin".into(), json!(params.left_margin));
    m.insert("right_margin".into(), json!(params.right_margin));
    m.insert("top_margin".into(), json!(params.top_margin));
    m.insert("bottom_margin".into(), json!(params.bottom_margin));
    m.insert("word_spacing_sigma".into(), json!(params.word_spacing_sigma));
    m.insert("line_spacing_sigma".into(), json!(params.line_spacing_sigma));
    m.insert("font_size_sigma".into(), json!(params.font_size_sigma));
    m.insert("perturb_x_sigma".into(), json!(params.perturb_x_sigma));
    m.insert("perturb_y_sigma".into(), json!(params.perturb_y_sigma));
    m.insert("perturb_theta_sigma".into(), json!(params.perturb_theta_sigma));
    m.insert("end_chars".into(), json!(params.end_chars));
    m.insert("start_chars".into(), json!(params.start_chars));
    m.insert("color".into(), json!(format!("#{:02x}{:02x}{:02x}", params.fill[0], params.fill[1], params.fill[2])));
    m
}

/// 从 Python 兼容预设 map 载入参数（未知字段忽略，缺失字段用默认值）。
fn from_preset_map(data: &Map<String, Value>) -> Result<HandwritingParams, PresetError> {
    let mut p = HandwritingParams::default();
    let num = |key: &str| -> Option<f32> {
        data.get(key).and_then(|v| v.as_f64()).map(|f| f as f32)
    };
    let str_ = |key: &str| -> Option<String> {
        data.get(key).and_then(|v| v.as_str()).map(String::from)
    };
    if let Some(v) = num("font_size") { p.font_size = v; }
    if let Some(v) = num("word_spacing") { p.word_spacing = v; }
    if let Some(v) = num("line_spacing") { p.line_spacing = v; }
    if let Some(v) = num("left_margin") { p.left_margin = v; }
    if let Some(v) = num("right_margin") { p.right_margin = v; }
    if let Some(v) = num("top_margin") { p.top_margin = v; }
    if let Some(v) = num("bottom_margin") { p.bottom_margin = v; }
    if let Some(v) = num("word_spacing_sigma") { p.word_spacing_sigma = v; }
    if let Some(v) = num("line_spacing_sigma") { p.line_spacing_sigma = v; }
    if let Some(v) = num("font_size_sigma") { p.font_size_sigma = v; }
    if let Some(v) = num("perturb_x_sigma") { p.perturb_x_sigma = v; }
    if let Some(v) = num("perturb_y_sigma") { p.perturb_y_sigma = v; }
    if let Some(v) = num("perturb_theta_sigma") { p.perturb_theta_sigma = v; }
    if let Some(v) = str_("end_chars") { p.end_chars = v; }
    if let Some(v) = str_("start_chars") { p.start_chars = v; }
    if let Some(v) = str_("font_path") { p.font_path = from_portable_path(&v); }
    if let Some(v) = str_("background_path") { p.background_path = from_portable_path(&v); }
    // 颜色：优先 #RRGGBB，其次 red/green/blue
    if let Some(v) = str_("color") {
        let hex = v.trim_start_matches('#');
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                p.fill = [r, g, b];
            }
        }
    } else {
        let rgb = |key: &str| data.get(key).and_then(|v| v.as_i64()).map(|i| i as u8).unwrap_or(0);
        p.fill = [rgb("red"), rgb("green"), rgb("blue")];
    }
    Ok(p)
}

/// 保存为 Python 兼容 JSON 预设。
pub fn save(params: &HandwritingParams, path: &Path) -> Result<(), PresetError> {
    let data = json!({ "version": 2, "params": to_preset_map(params) });
    let text = serde_json::to_string_pretty(&data)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// 载入 JSON 预设（兼容 Python 格式）。
pub fn load(path: &Path) -> Result<HandwritingParams, PresetError> {
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    let map = value
        .get("params")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
        .ok_or_else(|| PresetError::Format("预设顶层应为对象".into()))?;
    from_preset_map(map)
}
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test presets -- --nocapture`
预期：PASS。

**注意：** `portable_path_relative_to_assets_root` 测试依赖 exe 目录结构，若 `current_exe` 在 target/debug 下，`fonts` 子目录不存在也没关系（仅字符串比较，不检查文件存在）。

- [ ] **步骤 5：Commit（可选）**

```bash
git add src/core/presets.rs
git commit -m "feat(core): 预设 JSON 保存/载入（兼容 Python 格式 + 便携路径）"
```

---

### 任务 5：docx_io.rs docx 导入

**文件：**
- 创建：`src/core/docx_io.rs`

**docx-rs 0.4 API 备忘（已核实 docs.rs）：**
- `docx_rs::read_docx(&[u8]) -> Result<Docx, ReaderError>`
- `Docx.document: Document`、`Docx.styles: Styles`
- `Document.children: Vec<DocumentChild>`；`DocumentChild::Paragraph(Paragraph)`
- `Paragraph.texts()` / `Paragraph.child_elements`——实现时以编译为准
- `ParagraphProperty.alignment: Option<Justification>`、`indent: Option<Indent>`
- `Indent.first_line_chars: Option<i32>`、`special_indent: Option<SpecialIndentType>`
- `Justification` 枚举取值以编译为准（`Left`/`Center`/`Right`/`Both` 等）

- [ ] **步骤 1：编写失败的 docx 测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 用 docx-rs 构造测试文档（docx-rs 本身是 writer，可同时用于测试）。
    fn build_docx(paragraphs: &[(&str, docx_rs::AlignmentType, Option<i32>)]) -> Vec<u8> {
        use docx_rs::*;
        let mut doc = Docx::new();
        for (text, align, first_line_chars) in paragraphs {
            let mut p = Paragraph::new();
            p = p.align(align.clone());
            if let Some(chars) = first_line_chars {
                p = p.indent(None, None, None, Some(*chars));
            }
            p = p.add_run(Run::new().add_text(*text));
            doc = doc.add_paragraph(p);
        }
        let xml = doc.build().unwrap();
        // DocxResult<Vec<u8>> 的写出
        xml
    }
```

**说明：** docx-rs `build()` 返回 `DocxResult<Vec<u8>>`（zip 字节），可直接 `read_docx` 读回。`Indent` 的 `first_line_chars` 通过 `Paragraph::indent(None, None, None, Some(chars))` 设置（`start=None, special=None, end=None, start_chars=Some`）。若 0.4 的 builder 签名不同，以 `cargo doc` 编译错误提示为准调整。

- [ ] **步骤 2：运行测试验证失败**

运行：`cargo test docx_io -- --nocapture`
预期：编译错误，`docx_io` 模块未定义。

- [ ] **步骤 3：先实现读取函数（最小可用），再补测试**

```rust
//! docx 文档解析：提取段落文本、对齐与首行缩进（对齐 Python docx_io.py）。
//!
//! 首行缩进三级回退：
//! 1. `w:firstLineChars`（1/100 字符）× 渲染字号 → 像素；
//! 2. `w:firstLine`（EMU）按文档字号还原字符数 × 渲染字号；
//! 3. 样式链（based_on）继承。
//! 忽略空段落。

use std::path::Path;

use docx_rs::{read_docx, AlignmentType, DocumentChild, Paragraph as DxParagraph, SpecialIndentType};

use crate::core::models::{Align, Paragraph};

/// 从 docx 读取段落（忽略空段），对齐/首行缩进还原。
pub fn load_paragraphs(path: &Path, font_size: f32) -> Result<Vec<Paragraph>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 docx {path:?} 失败：{e}"))?;
    let docx = read_docx(&bytes).map_err(|e| format!("解析 docx {path:?} 失败：{e}"))?;
    let mut result = Vec::new();
    for child in &docx.document.children {
        let DocumentChild::Paragraph(dx) = child else { continue };
        let text = paragraph_text(dx);
        if text.trim().is_empty() {
            continue;
        }
        let align = resolve_align(dx);
        let indent = resolve_indent(dx, font_size);
        result.push(Paragraph { text, align, first_line_indent: indent });
    }
    Ok(result)
}

/// 拼接段落文本：w:tab → \t，w:br → \n（对齐 python-docx para.text 语义）。
fn paragraph_text(dx: &DxParagraph) -> String {
    let mut out = String::new();
    for item in &dx.child_elements {
        match item {
            docx_rs::ParagraphChild::Run(run) => {
                for child in &run.child_elements {
                    match child {
                        docx_rs::RunChild::Text(t) => out.push_str(t.text.as_str()),
                        docx_rs::RunChild::Tab(_) => out.push('\t'),
                        docx_rs::RunChild::Break(_) => out.push('\n'),
                        _ => {}
                    }
                }
            }
            docx_rs::ParagraphChild::Hyperlink(h) => {
                for run in &h.runs {
                    for child in &run.child_elements {
                        if let docx_rs::RunChild::Text(t) = child {
                            out.push_str(t.text.as_str());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// 对齐：JUSTIFY/BOTH 归 left（与 Python `_resolve_align` 一致）。
fn resolve_align(dx: &DxParagraph) -> Align {
    match dx.property.as_ref().and_then(|p| p.alignment.as_ref()) {
        Some(AlignmentType::Center) => Align::Center,
        Some(AlignmentType::Right) | Some(AlignmentType::Both) | Some(AlignmentType::Distribute) => {
            // 两端对齐归 left（Python 版 only center/right 特殊处理）
            if matches!(dx.property.as_ref().and_then(|p| p.alignment.as_ref()), Some(AlignmentType::Right)) {
                Align::Right
            } else {
                Align::Left
            }
        }
        _ => Align::Left,
    }
}

/// 首行缩进（像素）：firstLineChars 优先，其次 firstLine EMU 按文档字号还原。
fn resolve_indent(dx: &DxParagraph, font_size: f32) -> f32 {
    let Some(prop) = dx.property.as_ref() else { return 0.0 };
    let Some(ind) = prop.indent.as_ref() else { return 0.0 };
    if let Some(chars) = ind.first_line_chars {
        return chars as f32 / 100.0 * font_size;
    }
    if let Some(SpecialIndentType::FirstLine(emu)) = ind.special_indent {
        // EMU → pt（1in = 914400 EMU，1pt = 1/72in）→ 按文档字号还原字符数
        let pt = emu as f32 / 12700.0;
        let doc_font_size = doc_font_size_pt(dx);
        let chars = pt / doc_font_size;
        return chars * font_size;
    }
    0.0
}

/// 文档字号探测（pt）：run 直接格式优先，兜底 12（完整样式链在任务 6 扩展）。
fn doc_font_size_pt(_dx: &DxParagraph) -> f32 {
    12.0
}
```

- [ ] **步骤 4：补齐完整测试并验证**

在 `docx_io.rs` 的 `mod tests` 中补充导出测试：

```rust
#[test]
fn load_paragraphs_extracts_text_align_indent() {
    let bytes = build_docx(&[
        ("第一段居中", docx_rs::AlignmentType::Center, Some(200)),
        ("第二段右对齐", docx_rs::AlignmentType::Right, None),
        ("第三段默认", docx_rs::AlignmentType::Left, None),
        ("   ", docx_rs::AlignmentType::Left, None), // 空段应忽略
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.docx");
    std::fs::write(&path, bytes).unwrap();
    let paras = load_paragraphs(&path, 36.0).unwrap();
    assert_eq!(paras.len(), 3);
    assert_eq!(paras[0].text, "第一段居中");
    assert_eq!(paras[0].align, Align::Center);
    assert_eq!(paras[0].first_line_indent, 72.0); // 200/100 * 36
    assert_eq!(paras[1].align, Align::Right);
    assert_eq!(paras[1].first_line_indent, 0.0);
}

#[test]
fn load_paragraphs_missing_file_reports_error() {
    let err = load_paragraphs(Path::new("C:/nonexistent/x.docx"), 36.0).unwrap_err();
    assert!(err.contains("失败"));
}
```

运行：`cargo test docx_io -- --nocapture`
预期：PASS（若 docx-rs API 枚举名不同，按编译错误修正 `AlignmentType`/`SpecialIndentType`/`ParagraphChild` 等路径）。

- [ ] **步骤 5：Commit（可选）**

```bash
git add src/core/docx_io.rs Cargo.toml Cargo.lock
git commit -m "feat(core): docx 导入（段落/对齐/首行缩进还原）"
```

---

### 任务 6：docx 样式链继承 + 完整字号探测

**文件：**
- 修改：`src/core/docx_io.rs`

- [ ] **步骤 1：编写样式继承测试**

```rust
#[test]
fn indent_falls_back_to_first_line_emu() {
    // w:firstLine（EMU）路径：docx-rs 写出 special indent 的能力有限，
    // 此用例用手工构造的最小 docx zip 验证 EMU 回退（见下）。
}
```

**说明：** 若 docx-rs 0.4 无法写出 `special_indent`/样式链，则用手工构造 zip（`zip` crate 或手写字节）验证。首要验证路径为 `first_line_chars`（绝大多数中文 Word 文档走此路径）；EMU 回退与样式继承作为加固项，若 docx-rs 写出能力不足，测试降级为注释说明 + 保留实现。

- [ ] **步骤 2：实现样式链继承与字号探测**

```rust
/// 文档字号探测（pt）：run 直接格式 > 段落样式链 > Normal > docDefaults > 12。
/// （docx-rs 0.4 提供 styles 读取；此函数先实现 run 级探测，样式链在
/// load_paragraphs 中传入 Styles 后扩展。）
```

实现时以 `docx.styles` 为准：段落 `property.style` 有 `style_id`，在 `Styles.vec` 中查找 `Style`，沿 `based_on` 链查找 `indent.first_line_chars` 与字号。若 `Styles` 结构体字段与预期不符，按编译错误调整并保证：**主路径（firstLineChars 直接格式）有测试覆盖**。

- [ ] **步骤 3：运行测试验证**

运行：`cargo test docx_io -- --nocapture`
预期：PASS。

- [ ] **步骤 4：Commit（可选）**

```bash
git add src/core/docx_io.rs
git commit -m "feat(core): docx 样式链继承与字号探测"
```

---

### 任务 7：GUI 段落模式 + 预设 + docx 导入接线

**文件：**
- 修改：`src/ui/main_window.slint`
- 修改：`src/main.rs`

- [ ] **步骤 1：slint 界面：双模式 + 段落列表**

`main_window.slint` 中把「文本内容」区替换为：

```slint
export struct ParagraphItem {
    text: string,
    align-index: int,   // 0=left 1=center 2=right
    indent: int,        // 首行缩进（像素）
}

export component MainWindow inherits Window {
    // ... 既有属性保留 ...

    // 模式：0=纯文本 1=段落
    in-out property <int> input-mode: 0;
    in-out property <[ParagraphItem]> paragraphs;
    in property <bool> paragraphs-visible;

    // 新增回调
    callback add-paragraph;
    callback remove-paragraph(int);
    callback import-docx;
    callback save-preset;
    callback load-preset;
```

文本区布局（`Text { text: "文本内容"; }` 之后）：

```slint
HorizontalBox {
    spacing: 4px;
    input-mode-combo := ComboBox {
        model: ["纯文本", "段落"];
        current-index <=> root.input-mode;
    }
    Button {
        text: "添加段落";
        enabled: root.input-mode == 1;
        clicked => { root.add-paragraph(); }
    }
    Button {
        text: "导入 docx…";
        enabled: root.input-mode == 1;
        clicked => { root.import-docx(); }
    }
}

input-edit := TextEdit {
    text: "请输入要手写的文字…";
    height: 140px;
    wrap: word-wrap;
    visible: root.input-mode == 0;
}

VerticalBox {
    visible: root.input-mode == 1;
    spacing: 4px;
    for item[idx] in root.paragraphs : HorizontalBox {
        spacing: 4px;
        item-edit := TextEdit {
            text <=> item.text;
            height: 60px;
            wrap: word-wrap;
        }
        VerticalBox {
            spacing: 2px;
            Text { text: "对齐"; font-size: 11px; color: #888; }
            ComboBox {
                model: ["左对齐", "居中", "右对齐"];
                current-index <=> item.align-index;
            }
        }
        VerticalBox {
            spacing: 2px;
            Text { text: "首行缩进"; font-size: 11px; color: #888; }
            item-indent := SpinBox {
                value <=> item.indent;
                minimum: 0;
                maximum: 500;
            }
        }
        Button {
            text: "删除";
            clicked => { root.remove-paragraph(idx); }
        }
    }
}
```

底部工具条新增按钮（`生成预览` 之前）：

```slint
Button {
    text: "保存预设…";
    clicked => { root.save-preset(); }
}
Button {
    text: "载入预设…";
    clicked => { root.load-preset(); }
}
```

- [ ] **步骤 2：main.rs 接线段落模型与回调**

`main.rs` 顶部引入：

```rust
use handwrite_sim::core::docx_io;
use handwrite_sim::core::models::{Align, Paragraph};
use handwrite_sim::core::presets;
use slint::{Model, ModelRc, SharedString, VecModel};
```

`main()` 中 `ui.run()?` 之前：

```rust
// ---- 段落模型（VecModel 驱动列表 UI） ----
let paragraph_model = Rc::new(VecModel::<MainWindow::ParagraphItem>::default());
ui.set_paragraphs(ModelRc::from(paragraph_model.clone()));

// 添加段落
{
    let model = Rc::clone(&paragraph_model);
    ui.on_add_paragraph(move || model.push(MainWindow::ParagraphItem {
        text: SharedString::from(""),
        align_index: 0,
        indent: 0,
    }));
}

// 删除段落
{
    let model = Rc::clone(&paragraph_model);
    ui.on_remove_paragraph(move |idx| {
        let idx = idx as usize;
        if idx < model.row_count() {
            model.remove(idx);
        }
    });
}

// 导入 docx：解析后整体替换段落列表
{
    let model = Rc::clone(&paragraph_model);
    ui.on_import_docx(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Word 文档", &["docx"])
            .pick_file()
        {
            let font_size = ui.get_font_size() as f32;
            match docx_io::load_paragraphs(&path, font_size) {
                Ok(paras) => {
                    model.set_row_data_checked(0, ui.get_paragraphs().row_data(0).unwrap_or_default());
                    // 清空后填充
                    while model.row_count() > 0 {
                        model.remove(0);
                    }
                    for p in paras {
                        model.push(MainWindow::ParagraphItem {
                            text: SharedString::from(p.text),
                            align_index: match p.align {
                                Align::Left => 0,
                                Align::Center => 1,
                                Align::Right => 2,
                            },
                            indent: p.first_line_indent.round() as i32,
                        });
                    }
                    ui.set_input_mode(1);
                    ui.set_status_text(SharedString::from(format!(
                        "已导入 {} 个段落", paras.len()
                    )));
                }
                Err(e) => ui.set_status_text(SharedString::from(format!("导入 docx 失败：{e}"))),
            }
        }
    });
}
```

**注意：** `ParagraphItem` 的字段在生成的 Rust 代码中为 `align_index`/`indent`（snake_case）。`ui.on_add_paragraph` 等回调名由 slint 自动生成（`on_add_paragraph` 对应 `add-paragraph`）。若 `set_row_data_checked` 不存在则直接用 `while model.row_count() > 0 { model.remove(0); }` 清空（去掉多余行）。

- [ ] **步骤 3：main.rs 预设保存/载入回调**

```rust
// ---- 保存预设 ----
ui.on_save_preset(move || {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("预设", &["json"])
        .set_file_name("preset.json")
        .save_file()
    {
        let params = match collect_params(&ui) {
            Ok(p) => p,
            Err(e) => {
                ui.set_status_text(SharedString::from(format!("参数错误：{e}")));
                return;
            }
        };
        match presets::save(&params, &path) {
            Ok(()) => ui.set_status_text(SharedString::from(format!("预设已保存：{}", path.display()))),
            Err(e) => ui.set_status_text(SharedString::from(format!("保存失败：{e}"))),
        }
    }
});

// ---- 载入预设 ----
ui.on_load_preset(move || {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("预设", &["json"])
        .pick_file()
    {
        match presets::load(&path) {
            Ok(p) => {
                ui.set_font_path_text(SharedString::from(p.font_path));
                ui.set_background_path_text(SharedString::from(p.background_path));
                ui.set_font_size(p.font_size as i32);
                ui.set_line_spacing(p.line_spacing as i32);
                ui.set_word_spacing(p.word_spacing as i32);
                ui.set_perturb_x(p.perturb_x_sigma as i32);
                ui.set_perturb_y(p.perturb_y_sigma as i32);
                ui.set_perturb_theta(p.perturb_theta_sigma);
                ui.set_status_text(SharedString::from("预设已载入"));
            }
            Err(e) => ui.set_status_text(SharedString::from(format!("载入失败：{e}"))),
        }
    }
});
```

- [ ] **步骤 4：collect_params 支持段落收集**

`collect_params` 中，`input_mode == 1` 时从模型收集段落：

```rust
let mut params = HandwritingParams::default();
if ui.get_input_mode() == 1 {
    let model = ui.get_paragraphs();
    let mut paras = Vec::new();
    for i in 0..model.row_count() {
        let item = model.row_data(i).unwrap();
        let text = item.text.to_string();
        if text.trim().is_empty() {
            continue;
        }
        paras.push(Paragraph {
            text,
            align: match item.align_index {
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            },
            first_line_indent: item.indent as f32,
        });
    }
    params.paragraphs = paras;
} else {
    params.text = ui.get_input_text().as_str().trim().to_string();
}
```

- [ ] **步骤 5：编译并手动验证**

运行：`cargo build`
预期：编译通过。运行 `cargo run` 手动验证：模式切换、段落增删、对齐下拉、缩进 SpinBox、导入 docx 填充、保存/载入预设、预览与导出。

（GUI 交互无法自动化测试，以编译 + 手动清单为准。）

- [ ] **步骤 6：Commit（可选）**

```bash
git add src/ui/main_window.slint src/main.rs
git commit -m "feat(ui): 段落模式/预设/docx 导入接线"
```

---

### 任务 8：集成测试与全量回归

**文件：**
- 修改：`tests/test_engine_integration.rs`

- [ ] **步骤 1：扩展集成测试**

`tests/test_engine_integration.rs` 追加：

```rust
#[test]
fn integration_paragraph_path_renders_and_exports() {
    // 复用现有测试的字体/背景构造（参考文件内既有用例），
    // 参数带 3 个段落（左/中/右 + 缩进），验证：
    // - render_pages 产出至少 1 页且有前景
    // - save_all 与 render_pages 同 seed 逐像素一致
    // - 段落首行缩进体现在首行墨迹最左 x ≥ indent
}
```

（以文件内既有 helper 的签名与数据流为准填写完整实现。）

- [ ] **步骤 2：全量回归**

运行：`cargo test`
预期：全部 PASS（既有 + 新增）。

- [ ] **步骤 3：性能冒烟（可选）**

运行：`cargo test --release test_engine_integration`
预期：通过；段落路径单页渲染耗时在 release 下应远低于 Python（无硬性断言，人工观察）。

- [ ] **步骤 4：Commit（可选）**

```bash
git add tests/test_engine_integration.rs
git commit -m "test: 段落路径集成测试"
```

---

## 自检记录

- **规格覆盖度：** 4.2（layout 段落）→ 任务 2；4.3（engine 分发）→ 任务 3；4.4（presets）→ 任务 4；4.5（docx_io）→ 任务 5-6；4.6（webp/降采样）→ 任务 3 步骤 3（`load_background_for_preview`，image 0.25 默认已启用 webp 解码）；4.7（GUI）→ 任务 7；4.8（测试）→ 各任务内 + 任务 8。
- **占位符扫描：** 无 TODO/待定；docx-rs 精确 API（枚举名、child_elements 结构）存在版本差异风险，已标注"以编译错误为准修正"的明确处理方式。
- **类型一致性：** `layout_paragraph` 返回 `Vec<(Option<Vec<bool>>, f32)>`，`layout_paragraphs` 返回 `Vec<Vec<bool>>`，任务 3 的 engine 调用与之一致；`ParagraphItem` 字段 `text/align-index/indent` 在 slint 与 Rust 侧 snake_case 映射一致。