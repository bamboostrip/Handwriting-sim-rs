# 许可策略

## 现状

- **应用代码**：MIT（`LICENSE`）
- **egui / eframe**：MIT（crates.io 发布版）

## 历史：为什么之前是 GPLv3

早期 UI 框架选用 Slint，而 crates.io 上 `slint` crate 的默认许可是 **GPLv3**
（商用需购买商业许可）。当时项目确定开源（MIT），因此直接使用 GPLv3 版 Slint，
程序整体按 GPLv3 分发——应用自身代码仍标注 MIT，但完整程序受 GPL 传染。

## 当前选择：egui（MIT），整体恢复 MIT

2026-08 UI 重构（Slint → iced → egui 0.36）后：

- egui / eframe 为 **MIT** 许可，与 ab_glyph / image / serde 等既有依赖同属宽松许可
- 程序整体可按 **MIT** 分发，无 GPL 传染义务，也无归因展示义务
- egui 选用 glow（OpenGL）后端，软件渲染兜底，保留「无 GPU 老机器/虚拟机可跑」目标
- 详见 `docs/superpowers/specs/2026-08-13-egui-ui-migration-design.md`

## 依赖许可一览

| crate | 许可 | 备注 |
|-------|------|------|
| egui / eframe | MIT | UI 框架（即时模式，glow/OpenGL 后端） |
| ab_glyph | MIT/Apache-2.0 | 字体光栅化 |
| image | MIT/Apache-2.0 | 图像 IO |
| rand / rand_distr | MIT/Apache-2.0 | 随机数 |
| serde / serde_json | MIT/Apache-2.0 | 序列化 |
| thiserror | MIT/Apache-2.0 | 错误类型 |
| rfd | MIT | 原生文件对话框 |
| tempfile | MIT/Apache-2.0 | 测试 |

全部宽松许可，无商业约束。

## 合规检查

- [ ] 发布包不含字体文件（版权隔离，与 Python 版一致）
