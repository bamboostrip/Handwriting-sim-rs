# handwrite-sim

手写模拟器（Rust 版）：基于 Slint + 自研笔画扰动引擎的手写体生成工具。

Python 版（`Handwriting-simulator`）的 Rust 重写，目标：
- **性能**：核心渲染 10-30 倍提升（排版/扰动从 Python 解释器开销中解放）
- **兼容**：软件渲染兜底，无 GPU 老机器/虚拟机也能跑
- **跨平台**：Windows / macOS / Linux

## 快速开始

```bash
# 运行 GUI（当前为界面骨架，引擎已可用）
cargo run

# 运行引擎测试
cargo test

# 发布构建（LTO + strip，体积优化）
cargo build --release
```

## 目录结构

```
src/
├── main.rs            # 桌面入口
├── lib.rs             # 库入口
├── core/              # 渲染引擎（纯 Rust，无 GUI 依赖）
│   ├── models.rs      # 参数模型（对齐 Python 版默认值）
│   ├── font.rs        # 字体光栅化（ab_glyph）
│   ├── layout.rs      # 排版（对应 Python _layout_page）
│   ├── perturb.rs     # 笔画扰动（连通域 + 旋转平移）
│   └── engine.rs      # 引擎接口 + 默认实现
└── ui/                # Slint 界面
    ├── mod.rs
    └── main_window.slint
tests/                 # 集成测试
docs/                  # 架构 / 迁移计划 / 许可策略
assets/                # 测试资源说明
```

## 当前进度

- [x] 项目骨架、依赖、测试、文档
- [x] 引擎核心：排版 + 笔画扰动（纯文本路径）
- [x] 预览/导出全链路（seed 一致性保证）
- [x] Slint 界面骨架（参数面板雏形 + 预览占位）
- [x] GUI 与引擎接通（参数绑定 + 防抖预览 + 导出）
- [ ] 段落路径（对齐 / 缩进 / 右对齐）
- [ ] 背景 webp 支持、预设 JSON、docx 导入
- [x] PDF 导出（位图层）、写错字划掉重写（错字率驱动）
- [ ] 混合排版（打印体 + 手写体多字体管线，需求待定）

## 许可

- 应用代码：MIT（见 [LICENSE](LICENSE)）
- Slint：crates.io 默认 GPLv3；未来商业化切换 Royalty-free 的完整说明见
  [docs/03-licensing.md](docs/03-licensing.md)

## 相关文档

- [架构设计](docs/01-architecture.md)
- [迁移计划（从 Python 版）](docs/02-migration-plan.md)
- [许可策略](docs/03-licensing.md)