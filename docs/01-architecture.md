# 架构设计

## 分层

```
┌─────────────────────────────────────────────┐
│ ui/  (egui 0.36 / eframe，即时模式)          │  桌面界面：参数面板、预览、状态
│   app.rs     ← AppState + eframe::App       │
│                （logic 状态层 / ui 渲染层）   │
│   editor.rs  ← 段落编辑器（每段一个 TextEdit）│
│   params.rs  ← 参数收集/回填纯函数           │
│   controls.rs ← DragValue/分组框控件         │
│   theme.rs   ← 配色与视觉样式                │
└──────────────────┬──────────────────────────┘
                   │ 参数 → 图像
┌──────────────────▼──────────────────────────┐
│ core/ (纯 Rust，无 GUI 依赖)                 │  渲染引擎（可被 GUI/CLI/测试复用）
│   models.rs   参数模型（serde 序列化）        │
│   font.rs     字体光栅化（ab_glyph）          │
│   layout.rs   排版（逐字符 + 高斯扰动）       │
│   perturb.rs  笔画扰动（连通域 + 变换）       │
│   engine.rs   引擎接口 + 编排                 │
└─────────────────────────────────────────────┘
```

## 渲染管线（对齐 Python 版 `FastEngine`）

```
背景图 ──加载──► RgbImage
                    │
文本 ──layout_page──► 前景掩码 Vec<bool>     （逐字符光栅化 + 行/字/字号扰动）
                    │
掩码 ──perturb_mask─► 扰动参数（每笔画 dx/dy/θ）──► 绕笔画中心旋转平移 ──写回► RGB 画布
                    │
                    └──► RgbaImage（预览/导出）
```

## 关键设计约定

### 坐标约定
- 排版 `y` 为行**顶部**坐标（与 Python 版 `_layout_page` 一致）
- 光栅化时经 `font.ascent()` 换算为基线坐标（ab_glyph 以基线定位）
- 换行规则：`end_chars`（行尾允许）/ `start_chars`（行首禁止），条件与 Python 版逐字一致

### 随机源（seed 机制）
- 统一使用 `StdRng::seed_from_u64(seed)`，同一 seed 下预览与导出逐像素一致
- 随机数**消耗顺序**固定：每字符「行纵扰动 → 字号扰动 → 字距扰动」，与 Python 版
  `rand.gauss` 调用顺序一致，便于 golden 对比
- 笔画扰动每笔画依次消耗 dx / dy / θ 三个正态样本

### 性能分层
- 排版：逐字符循环（Rust 原生，无解释器开销）
- 扰动：flood fill 连通域 + 单遍像素变换（后续可加 `rayon` 按页并行导出）
- PNG 编码：`image` crate（zlib）；多页导出为 I/O 瓶颈，后续并行化

## 与 Python 版模块对照

| Rust | Python（handwritesim/core） |
|------|----------------------------|
| models.rs | models.py |
| font.rs | PIL.ImageFont 封装 |
| layout.rs | engine_fast._layout_page |
| perturb.rs | engine_fast._perturb_mask |
| engine.rs | engine.py（HandwritingEngine/FastEngine） |