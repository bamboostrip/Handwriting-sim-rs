# Typo Simulator Optimizations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the strikeout line centering and rewriting heights, replace typos with incorrect characters under the cross-out, and add configurable strikeout styles (Single Line, Double Line, Slash, Cross) in UI and rendering.

**Architecture:** Define a `StrikeoutStyle` enum and serialize it in parameters. Update `layout.rs` to randomly choose a wrong character for typo placeholders, fix the centering of cross-outs on typo characters, calculate realistic right-above correction coordinates, and draw natural wavy strokes using subdivided Bezier curves.

**Tech Stack:** Rust, Slint UI framework, `ab_glyph` font rendering.

## Global Constraints
- Do not change font loading or canvas rendering systems.
- Maintain deterministic output for a given seed.
- When `miswrite_rate` is 0.0, do not consume any extra random number generator (RNG) states.

---

### Task 1: Add models and presets support for StrikeoutStyle

**Files:**
- Modify: `src/core/models.rs`
- Modify: `src/core/presets.rs`
- Test: `src/core/models.rs` (add unit test)
- Test: `src/core/presets.rs` (add unit test)

**Interfaces:**
- Consumes: Existing `HandwritingParams` struct.
- Produces: `StrikeoutStyle` enum and `miswrite_strikeout_style` field in `HandwritingParams`.

- [ ] **Step 1: Write the failing test for models and presets**

Add this test at the end of `src/core/models.rs`:
```rust
#[test]
fn test_strikeout_style_parsing() {
    assert_eq!(StrikeoutStyle::parse("line").unwrap(), StrikeoutStyle::Line);
    assert_eq!(StrikeoutStyle::parse("double_line").unwrap(), StrikeoutStyle::DoubleLine);
    assert_eq!(StrikeoutStyle::parse("slash").unwrap(), StrikeoutStyle::Slash);
    assert_eq!(StrikeoutStyle::parse("cross").unwrap(), StrikeoutStyle::Cross);
    assert!(StrikeoutStyle::parse("invalid").is_err());
}
```

Add this test at the end of `src/core/presets.rs`:
```rust
#[test]
fn test_presets_includes_strikeout_style() {
    let mut p = HandwritingParams::default();
    p.miswrite_strikeout_style = StrikeoutStyle::Cross;
    let map = to_preset_map(&p);
    assert_eq!(map.get("miswrite_strikeout_style").unwrap().as_str().unwrap(), "cross");
    
    let loaded = from_preset_map(&map).unwrap();
    assert_eq!(loaded.miswrite_strikeout_style, StrikeoutStyle::Cross);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_strikeout_style_parsing test_presets_includes_strikeout_style`
Expected: Compilation failure because `StrikeoutStyle` does not exist.

- [ ] **Step 3: Write minimal implementation**

Modify `src/core/models.rs` to define `StrikeoutStyle` and add it to `HandwritingParams`:
```rust
/// 错字涂改方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrikeoutStyle {
    #[default]
    Line,       // 单横线
    DoubleLine, // 双横线
    Slash,      // 斜线
    Cross,      // 叉号
}

impl StrikeoutStyle {
    pub fn as_str(&self) -> &'static str {
        match self {
            StrikeoutStyle::Line => "line",
            StrikeoutStyle::DoubleLine => "double_line",
            StrikeoutStyle::Slash => "slash",
            StrikeoutStyle::Cross => "cross",
        }
    }

    pub fn parse(s: &str) -> Result<StrikeoutStyle, String> {
        match s {
            "line" => Ok(StrikeoutStyle::Line),
            "double_line" => Ok(StrikeoutStyle::DoubleLine),
            "slash" => Ok(StrikeoutStyle::Slash),
            "cross" => Ok(StrikeoutStyle::Cross),
            other => Err(format!("未知涂改方式：{other:?}，可选 line/double_line/slash/cross")),
        }
    }
}
```
And add `miswrite_strikeout_style` inside `HandwritingParams` struct (around line 155):
```rust
    /// 错字涂改方式。
    pub miswrite_strikeout_style: StrikeoutStyle,
```

Modify `src/core/presets.rs` to support `miswrite_strikeout_style`:
Inside `to_preset_map` (around line 81):
```rust
    m.insert("miswrite_strikeout_style".into(), json!(params.miswrite_strikeout_style.as_str()));
```
Inside `from_preset_map` (around line 119):
```rust
    if let Some(v) = str_("miswrite_strikeout_style") {
        match crate::core::models::StrikeoutStyle::parse(&v) {
            Ok(s) => p.miswrite_strikeout_style = s,
            Err(_) => return Err(PresetError::Format(format!("miswrite_strikeout_style 未知：{v:?}"))),
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/core/models.rs src/core/presets.rs
git commit -m "feat: add StrikeoutStyle model and preset support"
```

---

### Task 2: Update Slint UI and Rust main.rs mapping

**Files:**
- Modify: `src/ui/main_window.slint`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `miswrite_strikeout_style` from `HandwritingParams`.
- Produces: UI options for rewriting mode and strikeout style synchronized with rendering parameters.

- [ ] **Step 1: Write the failing UI test**

Before changing the UI, add an assertion in `src/main.rs` (in `fn sync_params_to_ui`) or `tests` to verify UI configuration handles `miswrite_strikeout_style_index`.
Wait, Slint properties can be queried. In `src/main.rs` (around line 868):
```rust
    // We will assert UI gets updated correctly
```
Let's modify `src/ui/main_window.slint` first to define the new fields.

- [ ] **Step 2: Modify `src/ui/main_window.slint`**

In `src/ui/main_window.slint`:
Modify the `ComboBox` for `miswrite-mode-combo` (around line 551):
```slint
                                    miswrite-mode-combo := ComboBox {
                                        model: ["右上方重写", "后文重写"];
                                        current-index: 0;
                                        horizontal-stretch: 1;
                                    }
```
And add properties to `MainWindow` (at the top metadata list around line 68):
```slint
    in-out property <int> miswrite-mode-index: 0;
    in-out property <int> miswrite-strikeout-style-index: 0;
```
Bind the first combobox `current-index` to the property:
```slint
                                    miswrite-mode-combo := ComboBox {
                                        model: ["右上方重写", "后文重写"];
                                        current-index: root.miswrite-mode-index;
                                        horizontal-stretch: 1;
                                    }
```
And add a new Row under `miswrite-mode-combo` row (around line 555):
```slint
                                Row {
                                    FieldLabel { text: "涂改方式"; }
                                    miswrite-strikeout-combo := ComboBox {
                                        model: ["单横线", "双横线", "斜线", "叉号"];
                                        current-index: root.miswrite-strikeout-style-index;
                                        horizontal-stretch: 1;
                                    }
                                }
```

- [ ] **Step 3: Modify `src/main.rs` mapping**

Update `sync_params_to_ui` in `src/main.rs` (around line 868) to set UI states:
```rust
    ui.set_miswrite_mode_index(match p.miswrite_rewrite_mode {
        MiswriteMode::Above => 0,
        MiswriteMode::Rewrite => 1,
    });
    ui.set_miswrite_strikeout_style_index(match p.miswrite_strikeout_style {
        StrikeoutStyle::Line => 0,
        StrikeoutStyle::DoubleLine => 1,
        StrikeoutStyle::Slash => 2,
        StrikeoutStyle::Cross => 3,
    });
```

Update `read_params_from_ui` in `src/main.rs` (around line 950) to retrieve UI states:
```rust
    params.miswrite_rewrite_mode = match ui.get_miswrite_mode_index() {
        1 => MiswriteMode::Rewrite,
        _ => MiswriteMode::Above,
    };
    params.miswrite_strikeout_style = match ui.get_miswrite_strikeout_style_index() {
        1 => StrikeoutStyle::DoubleLine,
        2 => StrikeoutStyle::Slash,
        3 => StrikeoutStyle::Cross,
        _ => StrikeoutStyle::Line,
    };
```

- [ ] **Step 4: Build and run the app to check compilation**

Run: `cargo check`
Expected: Clean compilation with no errors.

- [ ] **Step 5: Commit**

```bash
git add src/ui/main_window.slint src/main.rs
git commit -m "feat: map strikeout style and update descriptions in Slint UI"
```

---

### Task 3: Implement wrong character selection and Bezier drawing helper

**Files:**
- Modify: `src/core/layout.rs`

**Interfaces:**
- Consumes: Input characters and random number generator.
- Produces: `get_wrong_char` and `draw_bezier_line` helper functions.

- [ ] **Step 1: Write the failing test**

Add these tests to the test block of `src/core/layout.rs`:
```rust
    #[test]
    fn test_wrong_character_generation() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let ch = '中';
        let wrong = get_wrong_char(ch, &mut rng);
        assert_ne!(ch, wrong);
        assert!(wrong >= '\u{4e00}' && wrong <= '\u{9fa5}');

        let ch_eng = 'A';
        let wrong_eng = get_wrong_char(ch_eng, &mut rng);
        assert_ne!(ch_eng, wrong_eng);
        assert!(wrong_eng.is_ascii_uppercase());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_wrong_character_generation`
Expected: Compilation failure because `get_wrong_char` is undefined.

- [ ] **Step 3: Write minimal implementation**

Add `COMMON_CHINESE_CHARS` list and implementation of `get_wrong_char` in `src/core/layout.rs`:
```rust
const COMMON_CHINESE_CHARS: &[char] = &[
    '的', '一', '是', '在', '了', '不', '和', '有', '大', '这', '主', '中', '人', '国', '为', '以', '我', '分', '们', '行',
    '产', '作', '本', '经', '发', '社', '工', '己', '等', '均', '部', '样', '出', '门', '家', '理', '学', '对', '里', '后',
    '小', '多', '下', '心', '然', '事', '资', '力', '么', '得', '之', '都', '平', '因', '起', '只', '没', '生', '量', '建',
    '长', '现', '前', '性', '那', '系', '各', '进', '最', '及', '外', '治', '与', '公', '向', '情', '老', '正', '路', '解',
    '问', '反', '政', '化', '无', '其', '期', '高', '强', '使', '教', '定', '重', '社', '特', '立', '体', '政', '代', '通',
    '度', '意', '见', '指', '表', '命', '战', '民', '保', '机', '关', '各', '党', '建', '议', '写', '论', '设', '合', '名',
    '同', '由', '接', '收', '改', '新', '想', '打', '放', '儿', '加', '用', '及', '那', '此', '实', '决', '求', '美', '品',
    '书', '样', '要', '治', '法', '务', '制', '度', '清', '楚', '确', '认', '真', '实', '各', '部', '委', '局', '厅', '所'
];

fn get_wrong_char(ch: char, rng: &mut impl Rng) -> char {
    if ch.is_ascii_uppercase() {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            wrong_ch = (b'A' + rng.random_range(0..26)) as char;
        }
        wrong_ch
    } else if ch.is_ascii_lowercase() {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            wrong_ch = (b'a' + rng.random_range(0..26)) as char;
        }
        wrong_ch
    } else if ch.is_ascii_digit() {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            wrong_ch = (b'0' + rng.random_range(0..10)) as char;
        }
        wrong_ch
    } else if ch >= '\u{4e00}' && ch <= '\u{9fa5}' {
        let mut wrong_ch = ch;
        while wrong_ch == ch {
            let idx = rng.random_range(0..COMMON_CHINESE_CHARS.len());
            wrong_ch = COMMON_CHINESE_CHARS[idx];
        }
        wrong_ch
    } else {
        ch
    }
}
```

Add `draw_bezier_line` in `src/core/layout.rs`:
```rust
fn draw_bezier_line(
    mask: &mut [bool],
    width: usize,
    height: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    waviness: f32,
    rng: &mut impl Rng,
) {
    let mx = (x0 + x1) / 2.0;
    let my = (y0 + y1) / 2.0;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let nx = -dy / len;
    let ny = dx / len;
    
    let offset = rng.random_range(-waviness..=waviness);
    let cx = mx + nx * offset;
    let cy = my + ny * offset;

    let mut prev_x = x0;
    let mut prev_y = y0;
    for step in 1..=5 {
        let t = step as f32 / 5.0;
        let mt = 1.0 - t;
        let curr_x = mt * mt * x0 + 2.0 * mt * t * cx + t * t * x1;
        let curr_y = mt * mt * y0 + 2.0 * mt * t * cy + t * t * y1;
        draw_thick_line(mask, width, height, prev_x, prev_y, curr_x, curr_y, thickness);
        prev_x = curr_x;
        prev_y = curr_y;
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_wrong_character_generation`
Expected: Test passes.

- [ ] **Step 5: Commit**

```bash
git add src/core/layout.rs
git commit -m "feat: add get_wrong_char and draw_bezier_line helpers"
```

---

### Task 4: Update draw_miswrite to support StrikeoutStyle and new positioning

**Files:**
- Modify: `src/core/layout.rs:57-84`

**Interfaces:**
- Consumes: `wrong_ch` (for centering), `correct_ch` (to rewrite), `StrikeoutStyle`, and `rng`.
- Produces: Correctly centered, realistically styled crossed-out typo and top-right correction.

- [ ] **Step 1: Write the failing compile test**

Change signature of `draw_miswrite` in `src/core/layout.rs` to:
```rust
fn draw_miswrite(
    mask: &mut [bool],
    width: usize,
    height: usize,
    font: &FontFace,
    wrong_ch: char,
    correct_ch: char,
    x: f32,
    y_top: f32,
    size: f32,
    angle: f32,
    draw_small: bool,
    style: StrikeoutStyle,
    rng: &mut impl Rng,
)
```
Expected: Compilation failure because caller functions (`layout_page`, `layout_paragraph`, tests) pass incorrect parameters to `draw_miswrite`.

- [ ] **Step 2: Implement draw_miswrite logic**

Rewrite `draw_miswrite` with detailed rendering coordinates, centering offsets, and Bezier drawing:
```rust
fn draw_miswrite(
    mask: &mut [bool],
    width: usize,
    height: usize,
    font: &FontFace,
    wrong_ch: char,
    correct_ch: char,
    x: f32,
    y_top: f32,
    size: f32,
    angle: f32,
    draw_small: bool,
    style: crate::core::models::StrikeoutStyle,
    rng: &mut impl Rng,
) {
    let wrong_advance = font.glyph_width(wrong_ch, size);
    let mid_x = x + wrong_advance / 2.0; // Centered on typo character
    let mid_y = y_top + font.ascent(size) * 0.45;
    
    let (ct, st) = (angle.cos(), angle.sin());
    let half_w = wrong_advance * 0.55;
    let half_h = size * 0.4;
    let thickness = (size / 8.0).max(2.0);
    let waviness = size * 0.08;

    match style {
        crate::core::models::StrikeoutStyle::Line => {
            let rx = half_w * ct;
            let ry = half_w * st;
            draw_bezier_line(mask, width, height, mid_x - rx, mid_y - ry, mid_x + rx, mid_y + ry, thickness, waviness, rng);
        }
        crate::core::models::StrikeoutStyle::DoubleLine => {
            let rx = half_w * ct;
            let ry = half_w * st;
            
            // Draw top parallel line
            let offset_y = size * 0.1;
            draw_bezier_line(mask, width, height, mid_x - rx, mid_y - ry - offset_y, mid_x + rx, mid_y + ry - offset_y, thickness, waviness, rng);
            // Draw bottom parallel line
            draw_bezier_line(mask, width, height, mid_x - rx, mid_y - ry + offset_y, mid_x + rx, mid_y + ry + offset_y, thickness, waviness, rng);
        }
        crate::core::models::StrikeoutStyle::Slash => {
            // Draw single diagonal slash
            let x0 = mid_x + half_w * 0.7;
            let y0 = mid_y - half_h;
            let x1 = mid_x - half_w * 0.7;
            let y1 = mid_y + half_h;
            draw_bezier_line(mask, width, height, x0, y0, x1, y1, thickness, waviness, rng);
        }
        crate::core::models::StrikeoutStyle::Cross => {
            // Diagonal line 1
            let x0_1 = mid_x - half_w * 0.7;
            let y0_1 = mid_y - half_h;
            let x1_1 = mid_x + half_w * 0.7;
            let y1_1 = mid_y + half_h;
            draw_bezier_line(mask, width, height, x0_1, y0_1, x1_1, y1_1, thickness, waviness, rng);
            
            // Diagonal line 2
            let x0_2 = mid_x + half_w * 0.7;
            let y0_2 = mid_y - half_h;
            let x1_2 = mid_x - half_w * 0.7;
            let y1_2 = mid_y + half_h;
            draw_bezier_line(mask, width, height, x0_2, y0_2, x1_2, y1_2, thickness, waviness, rng);
        }
    }

    if draw_small {
        // Rewrite height adjusted closer to y_top with slight random perturbation
        let small = (size * 0.6).max(1.0);
        let rx_offset = rng.random_range(-size * 0.03..=size * 0.03);
        let ry_offset = rng.random_range(-size * 0.03..=size * 0.03);
        let small_x = x + wrong_advance * 0.45 + rx_offset;
        let small_baseline = (y_top + size * 0.05 + font.ascent(small) + ry_offset).max(font.ascent(small));
        font.rasterize(correct_ch, small, small_x, small_baseline, mask, width, height);
    }
}
```

- [ ] **Step 3: Commit placeholder modification (we'll fix callers in the next task)**

```bash
git add src/core/layout.rs
git commit -m "feat: implement draw_miswrite layout styles and heights"
```

---

### Task 5: Integrate wrong character substitution in layout_page and layout_paragraph

**Files:**
- Modify: `src/core/layout.rs`

**Interfaces:**
- Consumes: `HandwritingParams`, `FontFace`, input text.
- Produces: Correct rendering layouts for pages and paragraphs.

- [ ] **Step 1: Update layout_page loop**

In `layout_page` (around lines 157-172):
Replace current typo loop block:
```rust
            // 写错字模拟：判定只影响渲染，不参与换行；rate=0 时不消耗 RNG（零回归）
            let mut is_miswrite = false;
            let mut wrong_ch = ch;
            let mut angle = 0.0;
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                is_miswrite = true;
                wrong_ch = get_wrong_char(ch, rng);
                angle = normal_strike.sample(rng) as f32;
            }

            let offset = font.glyph_width(wrong_ch, size);
            let baseline_y = yj + font.ascent(size);
            font.rasterize(wrong_ch, size, x.round(), baseline_y, &mut mask, width, height);

            let char_x = x;
            x += params.word_spacing + offset + normal_word.sample(rng) as f32;

            i += 1;

            if is_miswrite {
                match params.miswrite_rewrite_mode {
                    MiswriteMode::Above => {
                        draw_miswrite(&mut mask, width, height, font, wrong_ch, ch, char_x, yj, size, angle, true, params.miswrite_strikeout_style, rng);
                    }
                    MiswriteMode::Rewrite => {
                        draw_miswrite(&mut mask, width, height, font, wrong_ch, ch, char_x, yj, size, angle, false, params.miswrite_strikeout_style, rng);
                        font.rasterize(ch, size, x.round(), baseline_y, &mut mask, width, height);
                        x += font.glyph_width(ch, size) + params.word_spacing;
                    }
                }
            }
```

- [ ] **Step 2: Update layout_paragraph Placed struct and Phase 1 & 2 loops**

Modify the `Placed` struct inside `layout_paragraph` (around line 225):
```rust
    #[derive(Clone, Copy)]
    struct Placed {
        ch: char,          // The character actually drawn (could be wrong_ch)
        correct_ch: char,  // The original correct character
        x: f32,
        y: f32,
        size: f32,
        line: usize,
        miswrite: bool,
        angle: f32,
        rewrite_x: f32,
    }
```

Modify Phase 1 placing loop (around lines 260-280):
```rust
            let mut is_miswrite = false;
            let mut wrong_ch = ch;
            let mut angle = 0.0;
            if params.miswrite_rate > 0.0 && rng.random_bool(f64::from(params.miswrite_rate)) {
                is_miswrite = true;
                wrong_ch = get_wrong_char(ch, rng);
                angle = normal_strike.sample(rng) as f32;
            }

            let offset = font.glyph_width(wrong_ch, size);
            let mut rewrite_x = 0.0;
            let x_next = x + params.word_spacing + offset + normal_word.sample(rng) as f32;
            
            if is_miswrite && params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                rewrite_x = x_next;
            }

            placed.push(Placed {
                ch: wrong_ch,
                correct_ch: ch,
                x,
                y: yj,
                size,
                line: line_ys.len() - 1,
                miswrite: is_miswrite,
                angle,
                rewrite_x,
            });
            
            x = x_next;
            i += 1;
            
            if is_miswrite && params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                x += font.glyph_width(ch, size) + params.word_spacing;
            }
```

Modify Phase 2 drawing loop (around lines 330-340):
```rust
        let dx = item.x + shift;
        let baseline_y = item.y + font.ascent(item.size);
        font.rasterize(item.ch, item.size, dx, baseline_y, &mut mask, width, canvas_h);
        if item.miswrite {
            draw_miswrite(
                &mut mask,
                width,
                canvas_h,
                font,
                item.ch,
                item.correct_ch,
                dx,
                item.y,
                item.size,
                item.angle,
                params.miswrite_rewrite_mode == MiswriteMode::Above,
                params.miswrite_strikeout_style,
                rng,
            );
            if params.miswrite_rewrite_mode == MiswriteMode::Rewrite {
                font.rasterize(item.correct_ch, item.size, item.rewrite_x + shift, baseline_y, &mut mask, width, canvas_h);
            }
        }
```

- [ ] **Step 3: Fix existing layout tests in `src/core/layout.rs`**

Update tests in `src/core/layout.rs` (around lines 630-850) where `draw_miswrite` is called directly, or adjust the test parameters/mocks as needed (e.g. passing mock RNGs/default parameters).
Let's see: `draw_miswrite` is actually not called directly in any tests, only `layout_page` and `layout_paragraph` are tested! Since we modified their internal logic, the tests will compile automatically.

- [ ] **Step 4: Run cargo test to verify everything passes**

Run: `cargo test`
Expected: PASS all tests.

- [ ] **Step 5: Commit**

```bash
git add src/core/layout.rs
git commit -m "feat: integrate wrong character rendering and style options into layout logic"
```
