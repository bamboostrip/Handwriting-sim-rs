# 阶段四设计：错字划掉重写 + PDF 位图导出

日期：2026-08-09
状态：已评审

## 背景

README「当前进度」列出阶段四需求：PDF 导出、写错字划掉、混合排版。
经需求澄清：

- **混合排版**：用户暂缓，本阶段不实现。
- **错字划掉重写**：用户不可能逐字标记错字——输入的是完美文档，由**错字率参数**
  驱动算法随机挑选字符，模拟"写错 → 划掉 → 重写"的手写效果。错字率可调
  （0.1%~30%）。
- **PDF 导出**：位图层方案——PDF 内嵌整页位图，视觉与预览/导出 PNG 完全一致，
  不可选中复制（纯文本层方案放弃）。

## 需求

### 功能 1：错字划掉重写

新增参数（`HandwritingParams`，均带 `#[serde(default)]` 保证旧预设 JSON 兼容）：

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `miswrite_rate` | `f32` | `0.0` | 每字符被判定为错字的概率（0~0.3） |
| `miswrite_rewrite_mode` | `MiswriteMode` | `Above` | 重写方式（见下） |

```rust
pub enum MiswriteMode {
    Above,   // 正上方略偏右，小一号重写
    Rewrite, // 后文正常位置重写（x 多推进一个字符宽度）
}
```

#### 算法（`layout_page` 文本路径 + `layout_paragraph` 段落路径）

- 错字判定点：**两个路径保持一致的 RNG 消费顺序**。现有顺序为逐字符
  行扰动 → 字号扰动 → 字距扰动；错字判定插在每个字符的这些扰动之后
  （`rng.random_bool(rate)` 或 `gen_bool`）。判定**只影响渲染、不影响换行推进**，
  唯一例外：`Rewrite` 模式使 x 多推进一个字符宽度（见下）。
- 因为错字判定消费同一 RNG 流且预览/导出共用 seed 路径，天然满足
  「同 seed 预览 = 导出逐像素一致」（现有测试 `preview_matches_export_with_same_seed`
  及段落版覆盖；新增错字参数后仍需逐像素一致）。
- 错字渲染：
  1. **删除线**：跨字符宽度的旋转粗线。角度 = `N(0, 0.15)` rad，起点在
     字形实际宽度区间上（`glyph_width`），垂直位置在字符中线，线宽
     `max(2, size/8)`。以 Bresenham 直线 + 线宽绘制进前景掩码。
  2. `Above` 模式：同一字符以 `size * 0.6`（≥1px）画在错字正上方略偏右
     （偏移 `(size*0.15, size*0.85)` 量级），仍走 `font.rasterize`。
  3. `Rewrite` 模式：错字原位划掉；x 额外推进 `glyph_width + word_spacing`，
     在紧邻位置以正常字号重写同一字符。
- 两个路径的差异处理：
  - 文本路径 `layout_page`：逐字符流式绘制，错字效果直接内联绘制。
  - 段落路径 `layout_paragraph`：阶段一 `placed` 列表需为每个字符记录
    `miswrite: bool`（判定在阶段一消费 RNG）；阶段二绘制时对错字字符追加
    删除线 / Above 小字。`Rewrite` 模式在阶段二多画一遍字符（注意需在同一
    band 内，删除线/重写都画入该段画布，随后按行裁剪时自然归属对应行）。
  - 段落路径的 `Rewrite` 推进只影响阶段二绘制位置，不回流到阶段一布局
    （阶段一 x 推进保持原样；重写字画在字符 glyph 右侧 `glyph_width + word_spacing` 处）。

#### UI（`main_window.slint` + `main.rs`）

- 新分组框「写错字」（放「笔画扰动」分组框之后）：
  - 错字率 Slider：0~30（整数百分数，UI 存 int，引擎 ÷100 为 f32）。
  - 重写方式 ComboBox：["正上方重写", "后文重写"]。
- `collect_params` / `apply_preset_to_ui` 同步新字段。
- `validate()`：`miswrite_rate` 必须在 [0, 1]。

### 功能 2：PDF 导出（位图层）

- 依赖：`printpdf = { version = "0.12", default-features = false, features = ["images"] }`
  （MIT；禁用默认 `html` feature 可避免 azul git 依赖；`images` 提供
  `RawImage::from_dynamic_image`，输入已是我们 `image` crate 的 `RgbaImage`）。
- 新函数 `export_pdf(params, out_path: &Path, seed: u64) -> Result<(), EngineError>`：
  1. `DefaultEngine::new(seed).render_pages(params)`（全分辨率，同导出 PNG 路径）。
  2. 每页 `RawImage::from_dynamic_image(RgbaImage 转 DynamicImage)` →
     `doc.add_image(&image)` → `Op::UseXobject { id, transform: XObjectTransform { dpi: Some(300.0), ..Default::default() } }`。
  3. `PdfPage::new(Mm(w * 25.4 / 300.0), Mm(h * 25.4 / 300.0), ops)`——
     页物理尺寸按 **300 DPI** 换算；A4 扫描背景（2480×3508 px）恰好得到 A4 页。
  4. `doc.with_pages(pages).save(&PdfSaveOptions::default(), &mut out)` → 写文件。
- UI：底部主按钮行加「导出 PDF」（`rfd` 保存对话框，filter "PDF"）。
  调用 `export_pdf`，状态栏反馈成功/失败。

## 测试

引擎（`engine.rs` / `layout.rs`）：

1. `miswrite_rate = 0.0`：输出与修复前完全一致（回归）。
2. `miswrite_rate > 0`（Above 与 Rewrite 两种模式）：
   - 同 seed 预览 = 导出逐像素一致（文本路径 + 段落路径）。
   - 同 seed 同参数两次渲染逐像素一致；不同 seed 输出不同。
   - 输出中存在删除线/重写（断言前景像素数 > 对应错字率为 0 的输出，
     或存在超出原布局的墨迹——用像素数差异即可，避免对具体像素过度约束）。
3. `validate` 拒绝 `miswrite_rate < 0` 或 `> 1`。
4. PDF：
   - `export_pdf` 产出文件：以 `%PDF-` 开头、非空。
   - 页数与 `render_pages` 一致（用 `printpdf::PdfDocument::load_from_bytes`
     读回验证页数；若 0.12 API 不支持读回，退化为解析文件头/页数偏移，
     或仅验证体积阈值 + 文件名。实现时以 API 为准）。
   - 与 PNG 导出同 seed 逐像素同源（PDF 位图来自同一 `render_pages`，无需逐像素断言）。

UI 手动验收：调整错字率/模式 → 预览出现删除线与重写；导出 PDF 用阅读器打开
页尺寸正确、内容与预览一致。

## 范围外

- 混合排版（打印体 + 手写体多字体管线）——用户暂缓，后续单独设计。
- PDF 可选中复制的文本层。
- 多页并行导出（rayon，迁移计划已列，本阶段不引入）。
