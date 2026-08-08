# 阶段三设计：段落路径 / 预设 JSON / docx 导入 / webp 与预览降采样

日期：2026-08-08
状态：已批准（用户确认继续开发）

## 1. 背景与目标

本仓库是 Python 版 [Handwriting-simulator](https://github.com/bamboostrip/Handwriting-simulator)
（D:\AllCode\pythone-project\Handwriting-simulator）的 **Rust 1:1 功能重构**，
核心目标是：

1. **渲染/运行速度**：目标单页 < 25ms（Python 版 FastEngine 约 0.15s/页，预期 6-15 倍提升）
2. **跨平台兼容**：Windows / macOS / Linux，无 GPU 依赖的软件渲染兜底

迁移原则（docs/02-migration-plan.md）：复刻"行为"而非"像素"；seed 自洽
（同 seed 预览 = 导出逐像素一致）；引擎冻结期间 Python 版算法只修 bug。

## 2. 范围

### 本次迭代（阶段三）

| # | 子任务 | 对应 Python 版 |
|---|--------|---------------|
| 1 | 段落路径：Paragraph 对齐（左/中/右）/ 首行缩进 / 尾部空格 / 跨页流式 | `engine_fast._layout_paragraph` + `_paragraph_pages` |
| 2 | 预设 JSON 保存/载入（兼容 Python 版字段语义与便携相对路径） | `presets.py` |
| 3 | docx 导入：段落 + 对齐 + 首行缩进还原 | `docx_io.py` |
| 4 | 背景 webp 解码 + 预览降采样（超大背景兜底） | GUI `_downsample_preview` |

### 明确不在本次范围（1:1 对齐的后续项）

- CLI（`cli.py` 全套参数）
- GUI 补充：预览翻页、预览背景开关、文字颜色控件、边界提示、预设下拉框刷新
- 阶段四：PDF 导出、写错字划掉重写、混合排版、多页并行导出
- 旧版 18 行纯文本预设格式（仅 Python 迁移历史文件用，YAGNI）

## 3. Python 版参考语义（对齐基准）

### 3.1 段落路径

`_layout_paragraph(params, rand, paragraph, width) -> list[(mask|None, offset)]`：

- **两阶段**：阶段一纯排版不绘制（随机数消耗顺序与纯文本路径一致：行扰动 → 字号扰动 →
  字距扰动），记录每字符 `(ch, x, y, size, line_idx)` 与每行结束 x；阶段二按段落实际
  高度创建画布光栅化（不受页高裁剪）。
- **首行缩进**：仅段落第一行 `x = left + first_line_indent`（段内 `\n` 换行不重复缩进）。
- **换行规则**：沿用 `end_chars` / `start_chars`（与纯文本路径同条件）。
- **右对齐**：`shift = (width - right) - line_end_x`，整行平移；尾部空格把文字从右缘
  "顶"进来（与 Word 一致）。
- **居中**：按每行墨迹的非零 x 范围逐行居中（`_center_text_lines`）。
- **行提取**：行带分组（`_split_text_rows`）→ 归属各行；空行补 `(None, 0.0)`；
  偏移钳制 `off_min, off_max = -0.25 * line_spacing, 0.8 * line_spacing`。

`_paragraph_pages(params)`：

- 所有段落共用同一 `rand` 流，先全部排版为 `all_lines`（跨段连续）。
- **逐行流式分页**：`draw_y = top + (line_spacing - font_size)`；每行（含空行）开始前
  检查 `draw_y > height - bottom - font_size` 且当前页已有内容 → 换页；
  与纯文本路径换页条件一致。
- 空段保留一行空行。行写入 `row0 = round(draw_y + off)`，裁掉越界行。
- 笔画扰动仍用独立随机源（Python 用 `np.random.default_rng(seed)`；Rust 版沿用
  `StdRng`，与纯文本路径共用一致即可，测试保证 seed 自洽）。

### 3.2 预设 JSON

格式：`{"version": 2, "params": {...}}`。

- params 字段（不含 text/paragraphs，颜色以 `color: "#RRGGBB"` 保存）：
  `font_path, background_path, font_size, word_spacing, line_spacing, left_margin,
  right_margin, top_margin, bottom_margin, word_spacing_sigma, line_spacing_sigma,
  font_size_sigma, perturb_x_sigma, perturb_y_sigma, perturb_theta_sigma,
  end_chars, start_chars`。
- 兼容载入：`color` 或 `red/green/blue` 两种颜色写法；未知字段忽略。
- **便携模式**：路径位于资产根目录（Python 为 exe 旁；Rust 版取 exe 所在目录）内时
  保存为相对路径，载入时解析回绝对路径；资产根外路径保持绝对路径。

### 3.3 docx 导入

`docx_io.load_paragraphs(path, font_size) -> list[Paragraph]`：

- 忽略空段落（`text.strip()` 为空）。
- 对齐：`w:jc` → left/center/right（JUSTIFY 归 left，与 Python 一致）。
- 首行缩进三级回退：
  1. `w:firstLineChars`（1/100 字符）× 渲染字号 → 像素；
  2. 回退 `w:firstLine`（EMU）按**文档字号**还原字符数 × 渲染字号（Word/WPS 某些
     版本只写 firstLine 不写 firstLineChars）；
  3. 沿样式链（based_on）继承查找。
- 文档字号探测：run 直接格式 > 段落样式链 > Normal 样式 > docDefaults > 12pt 兜底。

### 3.4 预览降采样

- 背景宽 > 4096 时降采样；`scale = 4096 / width`，新高度 `round(height * scale)`，
  LANCZOS 重采样。
- 空间参数全部 × scale（**浮点、不取整**，避免每行 ≤1px 舍入误差随行数累积错位）：
  `font_size, line_spacing, word_spacing, left/right/top/bottom_margin,
  word_spacing_sigma, line_spacing_sigma, font_size_sigma, perturb_x_sigma,
  perturb_y_sigma`。
- 段落 `first_line_indent` 不参与缩放（对齐 Python 行为）。
- 导出始终全分辨率（原始参数）。

## 4. Rust 版设计

### 4.1 models.rs（已有基础，微调）

- `Align`、`Paragraph` 已定义（`first_line_indent: f32`），无需改动。
- `HandwritingParams.paragraphs: Vec<Paragraph>` 已定义，段落路径启用条件：
  `!paragraphs.is_empty()`（与 Python `if params.paragraphs` 一致）。
- `validate()` 已允许 paragraphs 非空时 text 为空。

### 4.2 layout.rs：段落路径（核心）

新增两个函数：

```rust
/// 渲染单个段落，返回逐行 [(行墨迹裁剪掩码, 相对该行基线偏移)]。
/// 空行对应 (None, 0.0)。画布按段落自身高度创建（不受页高裁剪）。
pub fn layout_paragraph(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    para: &Paragraph,
    width: usize,
) -> Vec<(Option<Vec<bool>>, f32)>

/// 全部段落 → 逐行流式分页，返回各页前景掩码。
pub fn layout_paragraphs(
    params: &HandwritingParams,
    font: &FontFace,
    rng: &mut impl Rng,
    paragraphs: &[Paragraph],
    width: usize,
    height: usize,
) -> Vec<Vec<bool>>
```

实现要点：

- 阶段一记录 `Vec<(char, f32, f32, f32, usize)>`（ch, x, y, size, line_idx）与
  `Vec<f32>` 行结束 x；随机数消耗顺序对齐纯文本路径（`normal_line` → `normal_font`
  → `normal_word`）。
- 行 y 用浮点累计（Python 用浮点 `line_spacing` 避免 `int()` 截断累积错位）。
- 右对齐/居中在阶段二绘制前对 x 施加平移；居中需要先画到段落画布再按行带测量
  非零 x 范围（对齐 `_center_text_lines` 语义）。
- 行提取：`_split_text_rows` 的 Rust 实现（行聚合 bool → 连续段分组），偏移钳制
  `[-0.25, 0.8] × line_spacing`。
- 原 `layout_page` 保留（纯文本路径不动）。

### 4.3 engine.rs：段落分发

- `render_page_from` 泛化：`render_text_page`（纯文本，现状）与
  `render_paragraph_pages`（一次性产出全部页）。
- `render_preview`：paragraphs 非空 → 段落路径第一页。
- `render_pages`：paragraphs 非空 → 段落路径全部页；否则纯文本路径循环。
- `save_all` 不变（复用 render_pages）。

### 4.4 presets.rs（新文件）

```rust
/// 预设错误。
pub enum PresetError { Io, Json, Format(String) }

/// 保存为 Python 兼容 JSON（便携相对路径、color #RRGGBB、不含 text/paragraphs）。
pub fn save(params: &HandwritingParams, path: &Path) -> Result<(), PresetError>
/// 载入 JSON（兼容 color/red-green-blue、忽略未知字段、便携路径解析）。
pub fn load(path: &Path) -> Result<HandwritingParams, PresetError>
/// 资产根目录 = exe 所在目录（便携模式基准）。
pub fn assets_root() -> PathBuf
```

实现为**转换函数**（`to_preset_map` / `from_preset_map`），不绑定 serde 自定义
derive——字段过滤与重命名（fill → color）用显式 map 更可控。
`from_preset_map` 未提供的字段用 `HandwritingParams::default()` 兜底。

### 4.5 docx_io.rs（新文件，docx-rs）

```rust
/// 从 docx 读取段落（忽略空段），对齐/首行缩进还原。
pub fn load_paragraphs(path: &Path, font_size: f32) -> Result<Vec<Paragraph>, String>
```

- `docx_rs::read_docx` 读包；遍历 `docx.document` 段落。
- 段落文本：拼接 runs 的 `Text` 内容；`w:tab` → `\t`，`w:br` → `\n`（对齐 python-docx `para.text` 语义）。
- 对齐：`property.alignment`（Justification）→ `Align`（0=left, 1=center, 2=right，与 Slint ComboBox 索引一致）。
- 首行缩进：`property.indent.first_line_chars`（×font_size）→ 回退
  `special_indent`（EMU，按文档字号还原字符数 ×font_size）→ 样式链继承
  （`docx.styles` 按 `based_on` 回溯）。
- 文档字号探测：run `sz` → 段落样式链 → Normal → docDefaults → 12pt。
- 忽略空段落；`first_line_indent` 结果为 f32 像素。

依赖：`docx-rs = "0.2"`（已确认 API：`read_docx`、`Docx.styles/document`、
`ParagraphProperty.alignment/indent`、`Indent.first_line_chars/special_indent`）。

### 4.6 webp + 预览降采样

- `Cargo.toml`：`image = { version = "0.25", features = ["webp"] }`（显式启用保证）。
- engine 新增内部函数 `downsample(params) -> (params', thumb)`：宽 > 4096 时降采样
  背景（内存中 LANCZOS，**不写临时文件**），空间参数 × scale（浮点）。
- `render_preview` 私有路径使用降采样参数；`save_all`/导出始终原始参数。
- 阈值常量 `PREVIEW_MAX_WIDTH: u32 = 4096`。

### 4.7 GUI（main_window.slint + mod.rs + main.rs）

- 文本区改为**纯文本 / 段落**双模式（ComboBox 切换）：
  - 纯文本模式：现有 TextEdit（保留）。
  - 段落模式：`for` + `VecModel` 动态列表，每段 = TextEdit + 对齐 ComboBox
    + 首行缩进 SpinBox + 删除按钮；工具栏含「添加段落」「导入 docx」。
  - 段落数据结构：Rust 侧 `Vec<Paragraph>` 与 Slint 模型双向同步
    （`VecModel<ParagraphItem>`，`ParagraphItem { text, align-index, indent }`，
    对齐索引 0=left / 1=center / 2=right）。
- 新增回调：`add-paragraph`、`remove-paragraph(index)`、`import-docx`、
  `save-preset`、`load-preset`（rfd 文件对话框）。
- 收集参数：段落模式启用时 `params.paragraphs = 段落列表`；否则纯文本。
- 预设载入后回填控件（字体/背景路径、字号、边距、sigma，不覆盖文本/段落）。

### 4.8 测试计划

| 模块 | 用例 |
|------|------|
| layout.rs | 三对齐；首行缩进仅首行；尾部空格右对齐顶入；空段空行；跨页流式；段内 \\n 不重复缩进；同 seed 可复现 |
| engine.rs | 段落路径预览=导出逐像素一致；段落路径与纯文本路径随机顺序独立 |
| presets.rs | roundtrip；载入 Python 版字面 JSON（color 与 red/green/blue 两写法）；便携相对路径解析；未知字段忽略 |
| docx_io.rs | 构造测试 docx（docx-rs 写出或手造 zip）：firstLineChars 路径、firstLine EMU 路径、样式继承、空段忽略 |
| 集成 | webp 背景解码渲染；>4096 宽背景降采样参数等比缩放 |

## 5. 风险与取舍

- **docx-rs 样式链**：若 `Styles` 读取 based_on 链不便，降级为仅直接格式 + 段落样式
  单层查找（测试覆盖主路径）。
- **Slint 动态列表**：`for` + `VecModel` 为官方支持模式；若 TextEdit 在列表内焦点
  丢失，改用固定 8 段槽位 + 滚动（兜底方案）。
- **COS 对齐**：居中/右对齐的墨迹测量以段落画布为基准，跨页时行带归属逻辑与
  Python 逐行提取一致，避免页顶残留。