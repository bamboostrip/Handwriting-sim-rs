# Changelog

本文件记录各版本的更新内容。发布新版本时：

1. 在 `## [Unreleased]` 下记录改动，或直接新增 `## [x.y.z] - 日期` 小节
2. 打标签 `vx.y.z` 触发 GitHub Actions 构建，发布说明将自动从本文件提取
   当前版本小节并拼入 release 页面

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.3.2] - 2026-09-01

### Added

- **检查更新功能**：
  - 新增 GitHub Releases 自动/手动检查更新机制（支持版本语义化智能比对）。
  - 支持「发现新版本」对话框、Markdown 更新说明展示、跳过特定版本提醒。
  - 支持浏览器直达下载与 Windows 便携版一键无锁覆盖自动更新重启。
  - 具备多级容灾检测（GitHub REST API 与网页 302 重定向免频控探测），避免未授权 API 触发 HTTP 403 限流。
- **开源项目直达展示**：
  - 新增「关于」对话框（可在右侧面板顶部及底部状态栏快捷打开）。
  - 置顶展示 Rust 极速重构版开源主仓库与 Python 原版仓库，点击可直接调用系统默认浏览器直达 GitHub。


### Fixed

- **Windows WebView2 缺失提示**：部分用户（尤其是非管理员账户 / 精简系统 / 跨用户账户）启动便携版时提示 `Could not find the WebView2 Runtime`。本版本仅新增 **1 个便携免安装变体**（整体 4 文件：win 轻量 + win-webview2 + linux + macos，无安装包）：
  - 便携 zip 仍保留 `handwrite-sim-windows-x86_64.zip`（轻量 ~10 MB，需系统已安装 WebView2，Win10/11 通常自带）
  - **新增** `handwrite-sim-windows-x86_64-webview2.zip`（~150 MB，内置 Fixed Runtime `Microsoft.WebView2.FixedVersionRuntime.128.0.2739.54.x64`，exe 旁 `WebView2/` 目录，`src-tauri/src/main.rs:283` 启动前设 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`，解压即用，无需安装/联网/管理员，跨用户可用，适合小工具免安装分发）
  - Release 页面已补充对照表：**报错/离线→下 webview2.zip，已有 WebView2→仍用轻量 zip**；Linux/macOS 不使用 WebView2（WebKitGTK/WKWebView），无需额外打包；暂不提供 NSIS/MSI 安装包

### Changed

- 升级版本号至 0.3.1，`src-tauri/src/main.rs` 新增 `init_webview2_fixed_runtime()` 探测，CI 新增 `portable-webview2`（Fixed Runtime 便携）任务，便携双版本共存

## [0.3.0] - 2026-08-25

### Added

- **架构重构（Tauri 2 + Vue 3）**：
  - 界面从 Slint 框架全面迁移重构为 **Tauri 2 + Vue 3 + Naive UI + Vite**，解除原 GUI 模块 GPL 限制，全项目采用宽松 **MIT 许可**开源分发
  - 现代化 Web 技术栈原生渲染交互，支持实时响应式布局与深浅色预览底色
  - 纯 Rust 核心渲染引擎抽离为独立工作区库（`crates/core`），渲染/扰动/计算与 UI 完全解耦
- **框选文字区域**：
  - 预览图上支持鼠标拖拽框选手写区域，支持多区域增删查改与独立配置（手写体 / 打印体、独立字体、独立字号、边距设置、多段独立对齐）
  - 区域实时红框叠加与悬浮高亮，支持画布上直接二次拖拽移动与 8 向边缘缩放
  - 引擎支持多区域流式跨页排版，同 seed 下预览与导出逐像素严格一致
- **多页文档底图导入**：
  - 支持导入 PDF / DOCX 作为多页文档底图（基于 pdfium 栅格化与 COM/LibreOffice 转换），可直接在其上框选手写填写表格、试卷或实验报告
- **排版与错字模拟**：
  - 支持逐段对齐方式（左对齐 / 居中 / 右对齐）与首行缩进（2 字符根据当前字号自动换算）
  - 错字率模拟（0% ~ 30%），支持 4 种划掉涂改样式（单线 / 双线 / 斜线 / 叉号）与正上方重写（Above）或原位重写（Rewrite）
- **多格式导出**：
  - 支持 PNG 高清序列图一键批量导出
  - 支持 300 DPI 矢量位图层 PDF 导出（printpdf + lopdf），适配高精度打印与存档
- **跨平台构建发布**：
  - 完善 GitHub Actions 多平台自动化发布流水线，提供 Windows (x86_64)、Linux (x86_64)、macOS (Apple Silicon arm64) 即开即用便携压缩包

### Changed

- 优化核心渲染路径，编译阶段启用关键热点 SIMD 优化，大幅降低预览卡顿与响应延迟
- 更新预设文件格式为 JSON v2，全面兼容 Python 版格式并支持相对资产路径

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
