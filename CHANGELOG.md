# Changelog

本文件记录各版本的更新内容。发布新版本时：

1. 在 `## [Unreleased]` 下记录改动，或直接新增 `## [x.y.z] - 日期` 小节
2. 打标签 `vx.y.z` 触发 GitHub Actions 构建，发布说明将自动从本文件提取
   当前版本小节并拼入 release 页面

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.1.0] - 2026-08-11

### Added

- **错字率模拟**：每字符按概率判定为错字，错字划掉后在正上方小一号重写（Above）
  或后文正常位置重写（Rewrite）；涂改样式可选单线 / 双线 / 斜线 / 叉号
- **PDF 导出**：300 DPI 位图层 PDF（printpdf + lopdf），逐页导出，适合打印/存档
- **GitHub Actions 三平台打包**：Windows / Linux / macOS 自动构建 release 版并
  组装便携 zip（exe + 预设 + 背景 + fonts 说明），打 `v*` 标签自动发布到 Release
- **窗口/exe 图标**：Slint 窗口图标 + Windows exe 多尺寸 ICO 资源嵌入

### Changed

- **性能**：字形轮廓缓存、背景按路径+修改时间缓存、PNG 并行保存、预览背景 SIMD
  降采样（32MP 背景从 80s 降至毫秒级）、段落分页 dirty 标志消除二次方扫描
- **段落编辑交互**：单文本框 + 光标段工具按钮（对齐/缩进/回车分段/段首退格合并），
  对齐 Python 版交互
- **docx 导入**：zip + quick-xml 手写解析（替换 docx-rs），修复 firstLine twips
  单位 bug，支持样式链继承

### Fixed

- 预览卡死：多页循环零进度守卫（TextAreaTooSmall）+ 行距校验
- PDF 导出画质：禁用 max_image_size 降采样，启用 Flate 无损压缩与像素插值
- 笔画扰动 RNG 消费隔离（错字绘制 fork seed，保证 seed 一致性）
- 预设颜色 hex 安全解析（多字节字符切片不再 panic）

## [0.0.0] - 2026-07

### Added

- 项目骨架：Slint GUI + 纯 Rust 渲染引擎（ab_glyph 字体光栅化）
- 排版核心：字距/行距/字号随机扰动、笔画位移/旋转扰动（连通域 + 旋转平移）、
  标点不换行（end_chars）/行首禁则（start_chars）
- 段落路径：左对齐/居中/右对齐/首行缩进，逐行流式跨页
- 预设系统：JSON v2 保存/载入（兼容 Python 版格式 + 便携相对路径）
- docx 导入：段落对齐/首行缩进还原
- 预览降采样 + 多页导航 + 底色切换 + 边界提示（仅预览）
- 写错字参数面板
