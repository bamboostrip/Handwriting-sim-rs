# 深色模式设计规范 (Dark Mode Design Spec)

## 一、目标与概述
为手写模拟器 (Tauri 2 + Vue 3 + Naive UI) 引入完整的深色模式支持，作为 **v0.3.3** 核心功能。
设计风格严格对齐 Python 原版墨绿国风暗色调，具备系统主题自动跟随与一键手动快速切换。

## 二、色彩系统设计

### 1. 浅色模式 (Light Theme)
- 主背景: `#f7faf9` (柔和浅薄荷灰)
- 面板/卡片背景: `#ffffff`
- 边框颜色: `#d8e2df`
- 主题强调色 (Primary): `#2e7d74` (深松石绿)
- 文字主色: `#24312e` / 文字次要色: `#6b7a76`

### 2. 深色模式 (Dark Theme - 对齐 Python _DARK_QSS)
- 主背景 (App Shell): `#181c19` (深邃墨绿灰)
- 面板/输入框/卡片背景: `#232b26` (沉稳松墨绿)
- 边框颜色: `#38453d` (墨色边框)
- 主题强调色 (Primary): `#5ea84d` (翠竹绿)
- 主题悬浮/点击色: `#72c761` / `#438536`
- 文字主色: `#e8f0eb` (柔白) / 文字次要色: `#8e9e95` (浅苔绿灰)
- 危险色 (Danger): `#ff6b6b`

## 三、状态管理与响应式机制 (store.ts)
1. **主题偏好状态**: `themePreference: 'auto' | 'light' | 'dark'`，默认 `'auto'`。
2. **持久化存储**: 读取/写入 `localStorage.getItem('handwrite_theme_preference')`。
3. **系统监听**: 通过 `window.matchMedia('(prefers-color-scheme: dark)')` 监听 OS 主题实时变化。
4. **计算属性**: `isDark = computed(() => ...)`。
5. **DOM 联动**: 动态为 `<html>` / `<body>` 增删 `class="dark"`，驱动全局 CSS 变量。
6. **切换动作**: `toggleTheme()` 支持一键在浅色/深色/跟随系统之间切换。

## 四、组件层与 UI 适配

1. **Naive UI 全局集成 (App.vue)**:
   - 注入 `darkTheme` 与 `darkThemeOverrides`，确保 NInput, NSelect, NButton, NModal, NSlider, NCheckbox 等原生组件全量呈现墨绿暗色调。
2. **快捷切换按钮 (ParamsPanel.vue)**:
   - 在右侧面板顶部标题行（「待处理文本」右侧）新增 `☀️ / 🌙` 极简图标切换按钮。
3. **全局样式覆盖 (styles.css)**:
   - 适配左侧预览区画布底色、虚线框选叠加层、右侧参数折叠面板、底部状态栏。
4. **模态框适配 (AboutModal.vue, UpdateModal.vue, RegionEditModal.vue)**:
   - 开源项目直达卡片、更新日志展示区、区域配置弹窗等在深色下均具有高质感半透明及墨绿边框效果。

## 五、版本发布规划
- 全局升级版本号至 **0.3.3**。
- 更新 `CHANGELOG.md` 新增 `## [0.3.3]`。
