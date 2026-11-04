<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { darkTheme, NButton, NConfigProvider, NDialogProvider, NMessageProvider, NPopselect } from "naive-ui";
import type { GlobalThemeOverrides } from "naive-ui";
import PreviewPane from "./components/PreviewPane.vue";
import ParamsPanel from "./components/ParamsPanel.vue";
import AboutModal from "./components/AboutModal.vue";
import HelpModal from "./components/HelpModal.vue";
import UpdateModal from "./components/UpdateModal.vue";
import FirstRunModal from "./components/FirstRunModal.vue";
import {
  APP_VERSION,
  buildParams,
  cancelRegionEdit,
  clearShapeSelection,
  cycleThemePreference,
  deleteShape,
  doRender,
  ensureUsableBackground,
  ensureUsableFont,
  ensureStarterLayout,
  initSystemFonts,
  initThemeSystem,
  isDarkActive,
  loadBgDimensions,
  maybeOpenOnboarding,
  openAboutModal,
  openHelpModal,
  openOnboarding,
  refreshPresets,
  scheduleRender,
  setShapeMode,
  setThemePreference,
  startupCheckUpdate,
  store,
} from "./store";
import type { ThemePreference } from "./store";

// 状态栏主题按钮的 Popselect 选项（自动 / 浅色 / 深色）
const themeOptions = [
  { label: "🖥️ 自动跟随系统", value: "auto" },
  { label: "☀️ 浅色模式", value: "light" },
  { label: "🌙 深色模式", value: "dark" },
];

// 浅色主题：国风青绿主色调
const lightThemeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#2e7d74",
    primaryColorHover: "#3d948a",
    primaryColorPressed: "#25645d",
    primaryColorSuppl: "#2e7d74",
    borderRadius: "6px",
    fontSize: "13px",
  },
};

// 深色主题：对齐 Python 原版墨绿暗色调
const darkThemeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#5ea84d",
    primaryColorHover: "#72c761",
    primaryColorPressed: "#438536",
    primaryColorSuppl: "#5ea84d",
    bodyColor: "#181c19",
    cardColor: "#232b26",
    modalColor: "#232b26",
    popoverColor: "#232b26",
    tableColor: "#232b26",
    inputColor: "#232b26",
    borderColor: "#38453d",
    textColorBase: "#e8f0eb",
    textColor1: "#e8f0eb",
    textColor2: "#dce5df",
    textColor3: "#8e9e95",
    borderRadius: "6px",
    fontSize: "13px",
  },
  Card: {
    color: "#232b26",
    borderColor: "#38453d",
    textColor: "#e8f0eb",
  },
  Modal: {
    color: "#232b26",
    textColor: "#e8f0eb",
  },
  Input: {
    color: "#232b26",
    colorFocus: "#232b26",
    border: "1px solid #38453d",
    borderFocus: "1px solid #5ea84d",
    textColor: "#e8f0eb",
    placeholderColor: "#6c7d74",
  },
};

const activeTheme = computed(() => (isDarkActive() ? darkTheme : null));
const activeOverrides = computed(() =>
  isDarkActive() ? darkThemeOverrides : lightThemeOverrides
);


// 自动预览：任意参数/段落/区域变化后防抖 300ms 渲染（对齐 README 承诺的行为）
const paramSnapshot = computed(() => JSON.stringify(buildParams()));
watch(paramSnapshot, () => {
  if (store.dialogOpen) return; // 对话框编辑中不触发（草稿未写回）
  scheduleRender();
});

onMounted(async () => {
  initThemeSystem();
  await initSystemFonts();
  // 冷启动兜底：没选字体/纸张时自动挑系统楷体和内置纸张，避免首屏直接报错
  await ensureUsableFont();
  await refreshPresets();
  await ensureUsableBackground();
  await ensureStarterLayout();
  loadBgDimensions(store.backgroundPath);
  maybeOpenOnboarding();
  // 首次引导打开时不叠加更新弹窗，避免首屏两个模态框打架
  if (!store.onboardingOpen) void startupCheckUpdate();
  await doRender();
});


// 全局快捷键：
// - Esc：退出图形标注（清选中）、再取消文字区域调整态（两者不一起清，避免误伤）；
// - Delete / Backspace：删除当前选中的图形；
// 输入框 / 文本域 / contenteditable / naive-ui 的选择器与滑块聚焦时不劫持；
// 有弹窗（区域/帮助/关于/更新/引导/正文编辑）时也不劫持——否则 Esc 关弹窗会连带退出标注模式。
function isTypingTarget(e: KeyboardEvent): boolean {
  const el = e.target as HTMLElement | null;
  if (!el) return false;
  if (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable) return true;
  // naive-ui 的 NSelect / NSlider 触发器是 tabindex=0 的 div，不是 input：
  // 不拦的话，聚焦「类型 / 页码 / 线宽」控件时按 Delete 会删掉选中的图形。
  return !!el.closest(
    [
      ".n-select",
      ".n-base-selection",
      ".n-base-selection-tags",
      ".n-color-picker",
      ".n-slider",
      "[role='combobox']",
      "[role='listbox']",
      "[role='slider']",
      "[role='spinbutton']",
    ].join(","),
  );
}

/** 是否有模态层打开（此时键盘事件交给弹窗自己处理）。 */
function hasOpenModal(): boolean {
  return (
    store.dialogOpen ||
    store.helpModalOpen ||
    store.aboutModalOpen ||
    store.updateModalOpen ||
    store.onboardingOpen
  );
}

function onKeydown(e: KeyboardEvent): void {
  // 已被 naive-ui 等组件处理过（preventDefault）就不再插手
  if (e.defaultPrevented) return;
  if (hasOpenModal()) return;
  if (isTypingTarget(e)) return;

  if (e.key === "Escape") {
    // 图形标注模式（无论是否已选中图形）：Esc 退出模式并清选中。
    // 进行中的草稿 / 手柄调整由 PreviewPane 自己的监听器收尾（它拿得到本地状态）。
    if (store.shapeMode || store.selectedShapeIndex >= 0) {
      clearShapeSelection();
      if (store.shapeMode) setShapeMode(false);
      e.preventDefault();
      return;
    }
    if (store.editingIndex < 0 && store.selectedRegionIndex < 0) return;
    cancelRegionEdit();
    e.preventDefault();
    return;
  }

  // 带修饰键的组合留给系统 / 浏览器（如 Ctrl+Z、Alt+←）
  if (e.key !== "Delete" && e.key !== "Backspace") return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (store.selectedShapeIndex < 0) return;
  deleteShape(store.selectedShapeIndex);
  e.preventDefault();
}
window.addEventListener("keydown", onKeydown);
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <NConfigProvider :theme="activeTheme" :theme-overrides="activeOverrides" abstract>
    <NDialogProvider>

      <NMessageProvider>
        <div class="app-shell">
          <div class="app-main">
            <PreviewPane />
            <ParamsPanel />
          </div>
          <footer class="status-bar">
            <span class="status-text">{{ store.status }}</span>
            <div class="status-actions">
              <NPopselect
                :value="store.themePreference"
                :options="themeOptions"
                @update:value="(val: ThemePreference) => setThemePreference(val)"
                trigger="hover"
              >
                <NButton size="tiny" quaternary class="status-theme-btn" @click="cycleThemePreference()">
                  <template v-if="store.themePreference === 'auto'">
                    🖥️ 自动 ({{ isDarkActive() ? '深色' : '浅色' }})
                  </template>
                  <template v-else-if="store.themePreference === 'dark'">🌙 深色</template>
                  <template v-else>☀️ 浅色</template>
                </NButton>
              </NPopselect>
              <span class="status-sep">·</span>
              <span class="status-link" @click="openOnboarding()" title="重新查看新手引导">
                ✨ 新手引导
              </span>
              <span class="status-sep">·</span>
              <span class="status-link" @click="openHelpModal()" title="查看软件使用教程与技巧">
                📖 使用教程
              </span>
              <span class="status-sep">·</span>
              <span class="status-link" @click="openAboutModal()" title="查看软件版本与开源项目">
                v{{ APP_VERSION }} · 关于与更新
              </span>
            </div>
          </footer>
        </div>

        <HelpModal />
        <AboutModal />
        <UpdateModal />
        <FirstRunModal />
      </NMessageProvider>
    </NDialogProvider>
  </NConfigProvider>
</template>

