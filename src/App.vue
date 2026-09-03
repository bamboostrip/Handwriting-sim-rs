<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { darkTheme, NConfigProvider, NDialogProvider, NMessageProvider } from "naive-ui";
import type { GlobalThemeOverrides } from "naive-ui";
import PreviewPane from "./components/PreviewPane.vue";
import ParamsPanel from "./components/ParamsPanel.vue";
import AboutModal from "./components/AboutModal.vue";
import HelpModal from "./components/HelpModal.vue";
import UpdateModal from "./components/UpdateModal.vue";
import {
  APP_VERSION,
  buildParams,
  cancelRegionEdit,
  doRender,
  initSystemFonts,
  initThemeSystem,
  isDarkActive,
  loadBgDimensions,
  openAboutModal,
  openHelpModal,
  refreshPresets,
  scheduleRender,
  startupCheckUpdate,
  store,
} from "./store";


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

onMounted(() => {
  initThemeSystem();
  void initSystemFonts();
  void refreshPresets();
  loadBgDimensions(store.backgroundPath);
  void startupCheckUpdate();
});


// Esc：退出区域调整态（正在输入框/编辑器里打字时不劫持）
function onKeydown(e: KeyboardEvent): void {
  if (e.key !== "Escape" || (store.editingIndex < 0 && store.selectedRegionIndex < 0)) return;
  const el = e.target as HTMLElement | null;
  if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) {
    return;
  }
  cancelRegionEdit();
  e.preventDefault();
}
window.addEventListener("keydown", onKeydown);
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));

// 启动即尝试一次渲染（无字体/背景时后端返回可读错误，显示在状态栏）
void doRender();
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
      </NMessageProvider>
    </NDialogProvider>
  </NConfigProvider>
</template>

