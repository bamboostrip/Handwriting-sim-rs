# 设计：egui UI 迁移（2026-08-13）

状态：待实施（由迁移者按本文档执行）
目标分支：`feat/egui-ui`（从 `feat/iced-ui` 或 `master` 切出）
上游现状：`master` 为 Slint 版；`feat/iced-ui` 为 iced 0.14 版（本次迁移的起点）

---

## 1. 背景与动机

### 1.1 文本框诉求（本项目 UI 框架变更的根源）

- Python 版（`Handwriting-simulator`）用 **QTextEdit**：一个真正的多行文本框，
  块格式（对齐/缩进作用于光标所在块），换行/滚动/光标/IME 全内建。
- Slint 版因 `TextInput` 行高不自适应换行，被迫「每段一个 TextInput」+
  `est_lines` 行高估算 / `scroll_to_row` 手算滚动 / `focus-nonce` 聚焦令牌
  等 workaround。
- iced 0.14 版（`feat/iced-ui`）做到「每段一个多行 TextEditor + 段级格式」，
  但 **cosmic-text 0.15（iced 0.14 锁定的文本引擎）不支持 buffer/span 级对齐**
  （已核实 cosmic-text 0.19 支持，但 iced 最新发布版 0.14.0 的依赖树仍锁定
  0.15，`iced_graphics`/`iced_tiny_skia` 的 Cargo.toml 均声明 `cosmic-text = "0.15"`），
  因此居中/右对齐段只能「估宽收缩 + 容器对齐」，**超长段回退左对齐**——
  这是妥协方案，不是完整实现。

### 1.2 egui 可行性验证（已核实源码）

- `egui::widgets::TextEdit::horizontal_align(align)` 存在
  （`text_edit/builder.rs`），渲染时经 `atom_align` 传给 galley 布局。
- 底层 epaint 文本布局（`text/text_layout.rs`）：`halign != LEFT` 时对
  **wrap 后的每一行**调用 `halign_and_justify_row`，把每行在分配宽度内
  重新定位——即「换行 + 每行居中/右对齐」，与 QTextEdit / Slint 版效果一致，
  **无需估宽 hack、无超长回退**。
- `TextEdit::multiline(text: &mut dyn TextBuffer)` 为真正的多行编辑控件，
  滚动/选区/剪贴板/撤销/中文 IME 内建。
- 结论：**egui 是「QTextEdit 级文本框 + 编辑器内可视化对齐」的完整实现路径**。

## 2. 目标与非目标

### 目标

1. UI 框架迁移到 **egui 0.36 + eframe 0.36**（MIT/Apache-2.0，消除 GPL 传染；
   iced 版已消除，egui 继续保持宽松许可）。
2. **段落编辑器内可视化对齐完整实现**：居中/右对齐段 wrap 换行后每行真对齐，
   缩进 = 段前留白；回车分段、段首退格合并、粘贴多行自动拆段。
3. 全部参数面板控件、预览、翻页、预设、docx、导出功能与现状等价。
4. `core/` 引擎零改动；逻辑层（段落模型 / 参数收集 / 预设回填）最大化复用。
5. `cargo test` 全绿（目标 ≥ 现状 77 + 4 例）、clippy 零警告。

### 非目标

- 字符级富文本（段内粗体/颜色/字号随字符变化）——引擎侧也不支持，段落级格式足够。
- Web / 移动端目标。
- 新功能开发（本次只做 UI 框架替换）。

## 3. 技术选型与关键事实

### 3.1 依赖

```toml
[dependencies]
eframe = { version = "0.36", default-features = false, features = ["glow", "default_fonts", "wayland", "x11"] }
```

- **渲染后端选 glow（OpenGL 2.0+）**：老机器/虚拟机的兼容性最好
  （Windows 走 WARP 软件 GL、Linux 走 llvmpipe）；wgpu 是 eframe 的
  默认后端但作为备选 feature 保留即可，不在主路径。
- **`default_fonts` 必须保留**：egui 默认不加载系统字体，中文显示需要
  手动加载中文字体（见 5.3）。
- 保留依赖：`rfd`（文件对话框）、`image`（背景/导出）、`serde`/`serde_json`、
  `zip`/`quick-xml`（docx）、`printpdf`/`lopdf`（PDF）、`fast_image_resize`、
  `rand`/`rand_distr`、`ab_glyph`、`thiserror`、`tempfile`（dev）。
- **移除 `iced` 与 `tokio`**：egui 不需要 async runtime；后台渲染/文件对话框
  用 `std::thread::spawn` + `std::sync::mpsc` channel 即可（见 6.3）。
- dev-dependencies 增加 `egui_kittest = "0.36"`（headless UI 测试，可选）。

### 3.2 已核实的 egui 0.36 API 清单（迁移时直接使用）

| API | 用途 |
|---|---|
| `TextEdit::multiline(&mut dyn TextBuffer)` | 多行编辑（每段一个） |
| `TextEdit::horizontal_align(Align)` | 段对齐（LEFT/CENTER/RIGHT） |
| `TextEdit::id(Id)` / `TextEdit::hint_text` / `TextEdit::desired_rows(n)` | 稳定 id / 占位提示 / 空段最小行数 |
| `TextEdit::desired_width(f32::INFINITY)` | 占满可用宽度（wrap + 对齐的前提） |
| `Response::changed()` / `Response::request_focus()` | 文本变更检测 / 程序化聚焦 |
| `TextEditOutput.cursor_range: Option<CCursorRange>` | 字符级光标位置（段首退格检测） |
| `ScrollArea::vertical()` | 参数面板 / 编辑器滚动 |
| `ComboBox::from_id_salt` | 下拉（预设 / 重写方式 / 涂改方式） |
| `Slider` / `DragValue` / `Checkbox` | 错字率 / 数值输入 / 开关 |
| `egui::Image`（`ui.image`） | 预览图（`egui::ColorImage`，`TextureHandle` 或每帧上传） |
| `ViewportCommand::Icon(Arc<ImageData>)` | 窗口图标（app-icon.png） |
| `egui_kittest` | headless UI 测试 |

### 3.3 中文字体（必做步骤）

egui 默认内置字体不含 CJK。启动时：

1. 用 `FontDefinitions` 加载系统常见中文字体：
   - Windows：`C:\Windows\Fonts\msyh.ttc`（雅黑）、`simsun.ttc`、`simhei.ttf`、`simkai.ttf`
   - macOS：`/System/Library/Fonts/PingFang.ttc`
   - Linux：`Noto Sans CJK` / `WenQuanYi` 常见路径（找不到则跳过，不 panic）
2. 用户选择手写字体（`.ttf/.ttc/.otf`）后，把字体文件 bytes 也加入
   `FontDefinitions` 并设为编辑器字体（对应用户渲染字体观感）。
3. 多字体共存：中文回退链 `["handwrite", "system-chinese"]` 加入
   `FontFamily::Proportional`。

## 4. 架构设计

### 4.1 模块划分（对照 iced 版）

```
src/
├── main.rs            # eframe::run_native 入口（窗口 1280x840、图标）
├── core/              # 引擎（零改动）
└── ui/
    ├── app.rs         # AppState + eframe::App::update（帧循环：事件/状态/布局）
    ├── editor.rs      # 段落编辑器（ParaEditor{text,format} + 拆分/合并/粘贴拆段）
    ├── params.rs      # UiParams + collect_params / apply_preset（直接复用）
    ├── controls.rs    # 数值输入 / 分组框（egui 版）
    └── theme.rs       # 配色常量（从 iced 版平移）
```

### 4.2 状态模型（对应 iced 版 `HandwriteApp`，去掉 Message 路由）

```rust
pub struct AppState {
    editor: ParagraphEditor,        // paras: Vec<ParaEditor{text: String, format: ParaFormat}>
    ui: UiParams,                   // 复用 iced 版 params.rs
    preset_params: Option<HandwritingParams>,
    preset_names: Vec<String>,
    preset_paths: Vec<PathBuf>,
    preset_chosen: Option<String>,
    pending_save: Option<HandwritingParams>,
    preview: Option<(egui::ColorImage, usize, usize)>, // 当前页 RGBA（或 TextureHandle）
    preview_pages: Vec<RgbaImage>,
    preview_index: usize,
    preview_bg_idx: usize,
    status: String,
    para_status: String,
    seed: u64,
    dirty: bool,                    // 防抖
    last_edit: std::time::Instant,
    rendering: bool,
    render_rx: Option<mpsc::Receiver<RenderResult>>,   // 后台渲染结果
    // …（翻页/底色/对话框结果等）
}
```

### 4.3 数据流

- **egui 为即时模式**：`App::update(ctx)` 每帧被调用，直接读 `&mut self` 状态
  渲染 UI；控件交互（按钮/输入框）在本帧内读写状态，无需消息枚举。
- 文本编辑同步：`TextEdit::multiline(&mut para.text)` 直接编辑 `String`，
  每帧天然同步（区别于 iced 的受控模型）。
- 渲染任务完成后：worker 线程通过 channel 发结果 + `ctx.request_repaint()`
  唤醒 UI；update 里 `try_recv` 收结果并应用。

## 5. 段落编辑器设计（核心章节）

### 5.1 数据模型（从 iced 版 `editor.rs` 移植，Content → String）

```rust
pub struct ParaFormat { pub align: u8, pub indent_em: f32 }   // 不变
pub struct ParaEditor { pub text: String, pub format: ParaFormat }
pub struct ParagraphEditor { pub paras: Vec<ParaEditor>, pub current: usize }
```

直接复用（签名微调，逻辑不变，测试随迁）：
- `set_text(text, formats)`（docx 导入，`\n` 切段）
- `split(para)`：光标处拆段（String 版直接用字节偏移 + `is_char_boundary` 对齐，
  **不再受 iced `Content::move_to` 非 ASCII bug 影响**）
- `merge_prev(para)`：段首退格并入上一段
- `split_para_at_newlines(para, cursor_line)`：粘贴多行拆段（新段继承格式）
- `paragraphs_from_editor(&ParagraphEditor, font_size)`：导出收集（不变）
- `clean_editor_spaces`：不变
- `estimate_text_width`：**删除**（egui 不再需要估宽）

### 5.2 渲染（每段一个 TextEdit，编辑器内可视化对齐）

```rust
// 伪代码：编辑器区域（白底圆角边框内，egui::Frame）
egui::ScrollArea::vertical().show(ui, |ui| {
    for (i, para) in self.editor.paras.iter_mut().enumerate() {
        ui.push_id(i, |ui| {                    // 每段稳定 id（焦点/滚轮定位）
            ui.horizontal(|ui| {
                ui.add_space(para.format.indent_em * 13.0);   // 缩进留白
                let align = match para.format.align {
                    1 => egui::Align::Center, 2 => egui::Align::RIGHT, _ => egui::Align::LEFT,
                };
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut para.text)
                        .id(egui::Id::new(format!("para-{i}")))
                        .horizontal_align(align)
                        .desired_width(f32::INFINITY)   // 占满 → wrap + 每行对齐
                        .desired_rows(1)                // 空段也保持可点击高度
                        .hint_text("请输入文本内容…"),
                );
                self.on_para_edit(ui, i, resp);          // 见 5.4
            });
        });
    }
});
```

要点：
- `desired_width(INFINITY)` + `horizontal_align` = **wrap 后每行对齐**（epaint
  `halign_and_justify_row`），无估宽、无超长回退——这就是迁移的核心收益。
- 每段 TextEdit 高度 = 内容高度（即时模式天然自适应），无 `est_lines` hack。

### 5.4 交互逻辑

| 交互 | 实现 |
|---|---|
| 回车分段 | TextEdit 默认回车插入 `\n` → `resp.changed()` 时检查 `para.text` 含 `\n` → `split_para_at_newlines`（或光标处 `split`）→ 新段 `request_focus`（`resp` 失效，用下一帧 `ui.memory_mut` 聚焦新段 id） |
| 段首退格合并 | `resp.cursor_range()` 位于段首（`CCursorRange` 起点 index 0）+ 本帧 `ui.input` 有 `Key::Backspace` 按键事件 → `merge_prev` → 聚焦上一段末尾 |
| 粘贴多行 | 同回车：`\n` 检测 → 拆段（无需区分粘贴/手输） |
| 光标所在段 | 每帧取「最后获得焦点 / 最后 `changed` 的段索引」存 `editor.current`，状态栏提示用 |
| 底部空白点击聚焦最后一段 | 编辑器区底部加 `ui.interact` 空白区域，点击 → 聚焦最后一段 id |
| 自动滚动到焦点段 | 段响应 `resp.scroll_to_me(Some(Align::Center))`（egui 内建）——优于 iced 版手算滚动 |

- 键盘导航（方向键跨段）**不做**（与现状一致）。
- 每段 id 用 `format!("para-{i}")`（与 iced 版一致，便于测试对照）。

## 6. 功能逻辑平移清单

### 6.1 直接复用（逻辑零改动）

- `ui/params.rs`：`UiParams` / `apply_preset` / `collect_params` + 全部测试
- `core/`：全部
- 预设下拉刷新（`refresh_preset_combo`）、载入/保存流程
- docx 导入（`load_paragraphs` → `editor.set_text` + 格式换算）
- 导出图片 / PDF（参数收集 → `engine::export` / `export_pdf`）

### 6.2 平移实现（机制替换）

| 功能 | iced 版 | egui 版 |
|---|---|---|
| 防抖自动渲染 | `iced::time::every` 订阅 | `ctx.request_repaint_after(300ms)` + `dirty` 标记，update 里检查 |
| 后台渲染 | `tokio::task::spawn_blocking` | `std::thread::spawn` + `mpsc` channel + 完成时 `request_repaint` |
| 文件对话框 | `Task::perform(spawn_blocking(rfd))` | `std::thread::spawn` + channel（UI 不阻塞） |
| 预览图 | `Handle::from_rgba` + `Image` | `RgbaImage` → `egui::ColorImage`；静态图用 `TextureHandle` 缓存，避免每帧上传 |
| 翻页/页码/底色 | Message | 直接改状态 |
| 状态栏/段状态提示 | `refresh_para_status` | 复用（文本取 `para.text` 的字符数） |
| 窗口图标 | `window::icon` | `ViewportCommand::Icon`（app-icon.png → `ImageData`） |

### 6.3 后台任务约定

```rust
enum WorkerMsg { RenderDone(Result<Vec<RgbaImage>, String>), /* … */ }
// worker: std::thread::spawn(move || { let r = render_all_pages_preview(&params, seed);
//          tx.send(WorkerMsg::RenderDone(r)); ctx_handle.request_repaint(); })
// update: if let Ok(msg) = rx.try_recv() { 应用结果 }
```

- 渲染串行化：`rendering` 标志 + 完成后若 `dirty` 立即再触发（同 iced 版语义）。
- 进程退出：窗口关闭即结束，无需清理（thread 随进程退出）。

## 7. 控件映射表（对照现状）

| 现状控件 | 数量 | egui 0.36 |
|---|---|---|
| 绿色/主按钮 | 17 | `egui::Button::new(text).fill(...)`（样式见 theme.rs） |
| SpinBox（数值+σ 表） | 9 | `DragValue::new(&mut v).range(min..=max)`（自带拖拽/键盘/滚轮/中间态） |
| 笔画旋转（浮点文本） | 1 | `TextEdit::singleline` + 解析（失败回退默认，同现状） |
| ComboBox（预设/重写/涂改） | 3 | `ComboBox::from_id_salt(...)` |
| Slider（错字率） | 1 | `egui::Slider::new(&mut v, 0.0..=30.0)` + 百分比文本 |
| CheckBox（边界提示） | 1 | `egui::Checkbox` |
| 单行文本（路径/颜色/θ） | 4 | `TextEdit::singleline` |
| 多行编辑器 | 1 | 每段一个 `TextEdit::multiline`（第 5 章） |
| 滚动容器 | 2 | `ScrollArea::vertical()` |
| 预览大图 | 1 | `egui::Image`（contain 缩放：`ui.available_size()` + `Image::new(...).fit_to_exact_size` 或 `ui.add_sized`） |
| 分组框 | 4 | `egui::Frame::group` 或自定义 Frame（圆角 + 边框 + 标题） |
| 布局 | — | `ui.vertical` / `ui.horizontal` / `ui.allocate_ui_with_layout`；右侧面板 `ui.set_width(460.0)` |

## 8. 主题与样式

- 配色常量从 `ui/theme.rs` 平移（BG #f4f7f4、TEXT #2b3430、SUB_TEXT #7d8a82、
  GROUP_BORDER #d3ded6、BTN_BG #dcf7e6、PRIMARY_BG #9ddc80 等）。
- egui 侧：`ctx.style_mut(|s| …)` 覆盖 `visuals`（widgets 圆角 4-6、间距、
  `selection.bg_fill` 等），`Button::fill/rounding` 实现 Green/Primary 两档按钮。
- 字体：默认 `FontFamily::Proportional` 走中文字体回退链（见 3.3），字号 13px
  对齐现状（`TextStyle` 映射）。

## 9. 测试策略

1. **core/ 引擎测试**：不动（61 例）。
2. **`ui/editor.rs` 纯函数测试**：从 iced 版移植并改为 String 模型：
   - `split`（光标处拆段、格式继承、Unicode 边界）
   - `merge_prev`（合并、格式保留前者、段 0 无操作）
   - `split_para_at_newlines`（粘贴拆段、格式继承）
   - `paragraphs_from_editor`（空段跳过、对齐/缩进映射、首尾空格保留）
   - `clean_editor_spaces`（NBSP/WJ 清理）
   - 删除 `estimate_text_width` 及其测试（不再需要）
   - **新增**：egui 版无 iced `move_to` bug，可补「非 ASCII 光标拆段」测试
3. **`ui/params.rs` 测试**：不动（8 例）。
4. **可选**：`egui_kittest` 冒烟测试（应用可构建、编辑器可渲染）。
5. 验收：`cargo test` 全绿（目标 ≥ 77+4）、`cargo clippy --all-targets` 零警告、
   Windows 本机启动 6 秒无崩溃；`cargo build --release` 通过。

## 10. 风险与已知限制

| 风险 | 说明与对策 |
|---|---|
| OpenGL 依赖 | glow 需要 GL 2.0+；Windows WARP / Linux llvmpipe 可软件兜底；无 GPU 环境迁移后实测，必要时切 wgpu（DX12 WARP） |
| 中文 IME | egui 0.32+ 修复后 Windows 基本可用；迁移**阶段 1 优先实测**（输入法组合/候选框），有问题再评估 `TextEdit` 的 `ime` 事件处理 |
| egui 版本 API 变动 | 每个 minor 版本有 breaking；锁定 0.36，升级时小步迁移并跑全量测试 |
| 预览图上传开销 | 全分辨率预览图每帧 `ColorImage` 上传会卡；用 `TextureHandle` 缓存 + 翻页时更新 |
| 字符级富文本 | 不在范围（与引擎能力对齐） |
| 无 GPU 目标 | glow 软件渲染实测为准（README 原目标保留） |

## 11. 分阶段实施计划

| 阶段 | 内容 | 预估 |
|---|---|---|
| 0. 骨架 | `Cargo.toml` 换 eframe；`main.rs` 入口（窗口 1280x840、图标）；中文字体加载；左侧预览 + 右侧 460px 面板 + 底部状态栏布局骨架 | 0.5h |
| 1. 段落编辑器（核心） | 多段 `TextEdit::multiline` + `horizontal_align` + 缩进留白；回车分段 / 段首退格 / 粘贴拆段；焦点与状态栏提示；**优先实测中文 IME**；纯函数测试移植 | 2-3h |
| 2. 参数面板 | 全部控件映射（DragValue/ComboBox/Slider/Checkbox/TextEdit/分组框）+ 主题样式 | 2h |
| 3. 功能平移 | 防抖渲染（request_repaint_after）、后台线程 + channel、rfd 对话框、预览 TextureHandle、翻页/底色、预设/docx/导出 | 2h |
| 4. 收尾 | 测试全绿 + clippy 零警告 + release 构建 + 启动验证 + README/CHANGELOG/许可文档更新 | 1h |
| **合计** | | **约 1-1.5 人日** |

## 12. 代码复用清单（迁移者核对）

- [ ] `src/core/`：全部文件（零改动）
- [ ] `src/ui/params.rs`：`UiParams` / `apply_preset` / `collect_params` + 测试（零改动）
- [ ] `src/ui/editor.rs`：`ParaFormat` / `DEFAULT_INDENT_EM` / `split` / `merge_prev` /
      `split_para_at_newlines` / `set_text` / `paragraphs_from_editor` /
      `clean_editor_spaces` + 测试（`Content` → `String` 微调；删除 `estimate_text_width`）
- [ ] `src/ui/theme.rs`：配色常量
- [ ] `app.rs`：预设刷新 / 载入 / 保存、docx 导入、导出参数收集、状态栏提示逻辑
      （机制从 Message/subscription 改为帧内直接处理）
- [ ] 删除：`src/ui/controls.rs`（iced 版控件，替换为 egui 版）、
      `src/ui/view.rs`（iced 版视图，替换为 egui 布局）、`src/ui/app.rs` 的
      Message 枚举与 subscription

## 13. 验收标准

1. `cargo test`：core + editor + params 全绿（≥ 现状 77 + 4 例）
2. `cargo clippy --all-targets`：零警告
3. `cargo build --release` 通过
4. Windows 本机启动无崩溃；手动验证：
   - 编辑器输入中文正常（IME 候选框可用）
   - 多段文本设置居中/右对齐，编辑器内每行真对齐（含长段 wrap）
   - 回车分段（新段继承格式）、段首退格合并、粘贴多行拆段
   - 停止输入 300ms 自动渲染、翻页、底色、预设、docx、导出图片/PDF 均正常
5. 文档：README 技术栈 / CHANGELOG / docs/03-licensing.md / 本设计文档归档
