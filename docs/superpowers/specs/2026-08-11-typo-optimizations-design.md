# Typo Simulator Optimizations Design

This document details the design for improving the typo/miswrite simulation in the Handwriting Simulator application.

## Goals

1. **Fix strikeout line centering**: Align the strikeout stroke to the center of the typo character (`x + advance / 2.0`) instead of its left edge (`x`).
2. **Introduce natural, hand-drawn strokes**: Draw strikeout lines as slightly wavy Bezier curves instead of rigid geometric lines.
3. **Add Multiple Strikeout Styles**: Allow users to select between "单横线" (Single line), "双横线" (Double line), "斜线" (Slash `/`), and "叉号" (Cross `X`) in the UI and rendering backend.
4. **Lower rewriting position**: Bring the small rewritten character (Above mode) closer to the top-right of the crossed-out character (`y_top` level, with minor random perturbation) to look realistic.
5. **Implement realistic typos (wrong characters)**: When a typo occurs, render a *wrong* character (randomly selected from a pool of common characters of the same category) under the cross-out, while rendering the original *correct* character as the correction above or after it.

---

## Architectural & Model Changes

### 1. [`models.rs`](file:///d:/AllCode/rust/handwrite-sim/src/core/models.rs)

#### `StrikeoutStyle` Enum
We will define a new enum `StrikeoutStyle` for selecting the style of strikeout:
```rust
/// 错字涂改方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StrikeoutStyle {
    #[default]
    Line,       // 单横线
    DoubleLine, // 双横线
    Slash,      // 斜线
    Cross,      // 叉号
}
```
We will add `miswrite_strikeout_style` to `HandwritingParams`:
```rust
pub miswrite_strikeout_style: StrikeoutStyle,
```

---

### 2. [`presets.rs`](file:///d:/AllCode/rust/handwrite-sim/src/core/presets.rs)
We will serialize and deserialize `miswrite_strikeout_style` to JSON presets:
- Serialization: `m.insert("miswrite_strikeout_style".into(), json!(params.miswrite_strikeout_style.as_str()));`
- Deserialization: Parse from string with fallback to `StrikeoutStyle::default()`.

---

### 3. [`main_window.slint`](file:///d:/AllCode/rust/handwrite-sim/src/ui/main_window.slint)
- Rename combobox option from `"正上方重写"` to `"右上方重写"`.
- Add a new combobox row under "重写方式" for selecting the strikeout style ("涂改方式"):
  - Options: `["单横线", "双横线", "斜线", "叉号"]`.
  - Property: `miswrite-strikeout-style-index`.

---

### 4. [`main.rs`](file:///d:/AllCode/rust/handwrite-sim/src/main.rs)
- Map `miswrite-strikeout-style-index` (0 to 3) in Slint to `StrikeoutStyle` in `HandwritingParams` during synchronization (both to and from UI).

---

## Layout & Rendering Optimization in [`layout.rs`](file:///d:/AllCode/rust/handwrite-sim/src/core/layout.rs)

### 1. Wrong Character Selection (`get_wrong_char`)
We will implement a helper function in `layout.rs` to generate a different character for the typo:
```rust
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
        ch // Keep punctuation/spaces as-is
    }
}
```
Where `COMMON_CHINESE_CHARS` is a static list of ~200 highly common Chinese characters.

### 2. Centering & Natural Bezier Strikeout Drawing (`draw_miswrite`)
We will pass the `StrikeoutStyle` parameter and `rng` to `draw_miswrite`. We will draw lines by subdividing quadratic Bezier curves into small segments to introduce subtle hand-drawn waviness:

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
    // Midpoint perpendicular offset to create a natural curve
    let mx = (x0 + x1) / 2.0;
    let my = (y0 + y1) / 2.0;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx*dx + dy*dy).sqrt().max(1.0);
    let nx = -dy / len;
    let ny = dx / len;
    
    let offset = rng.random_range(-waviness..=waviness);
    let cx = mx + nx * offset;
    let cy = my + ny * offset;

    // Draw quadratic Bezier curve subdivided into 5 segments
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

Then `draw_miswrite` will calculate:
- `mid_x = x + advance / 2.0` (Centering fix)
- `mid_y = y_top + font.ascent(size) * 0.45`
- `half_w = advance * 0.55` (Slightly wider than character)
- `half_h = size * 0.4`
- Under `StrikeoutStyle::Line`: Draw a horizontal Bezier line.
- Under `StrikeoutStyle::DoubleLine`: Draw two parallel horizontal Bezier lines, offset vertically.
- Under `StrikeoutStyle::Slash`: Draw a single diagonal Bezier line from top-right to bottom-left.
- Under `StrikeoutStyle::Cross`: Draw two crossing diagonal Bezier lines.

### 3. Rewriting Coordinates (Right-above)
- `small_baseline = y_top + size * 0.05 + rng.random_range(-size * 0.03..=size * 0.03)`
- `small_x = x + wrong_advance * 0.45 + rng.random_range(-size * 0.03..=size * 0.03)`
- The small rewritten character scale remains `0.6` of the main character size.

### 4. Integration in `layout_page` and `layout_paragraph`
We will rewrite the layout loop so that:
- If a typo is determined:
  - Generate `wrong_ch` via `get_wrong_char`.
  - Draw `wrong_ch` at the original position.
  - Draw strikeout on `wrong_ch` using the selected `StrikeoutStyle`.
  - In `Above` mode: Draw the correct character `ch` at the new right-above coordinates.
  - In `Rewrite` mode: Draw the correct character `ch` in normal size at the next line position.
- This maintains exactly the same RNG consumption footprint when `miswrite_rate == 0.0`, ensuring compatibility and zero-regression.

---

## Verification Plan

### Automated Tests
1. Run `cargo test` to ensure all existing tests pass.
2. Add new unit tests in `layout.rs` testing `StrikeoutStyle` parsing, layout preservation, and the deterministic reproduction of the new styles.

### Manual Verification
1. Launch the GUI and verify the comboboxes show:
   - "重写方式": "右上方重写" and "后文重写".
   - "涂改方式": "单横线", "双横线", "斜线", "叉号".
2. Select each涂改方式 (strikeout style) and check the real-time preview to verify:
   - Strikeout lines are perfectly centered on crossed-out characters.
   - Typo characters under the cross-out are different from the rewritten characters.
   - The right-above rewritten characters are properly placed (close to the character top-right, not floating in the line above).
   - Wavy lines look realistic and hand-drawn.
