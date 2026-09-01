<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { NConfigProvider, NDialogProvider, NMessageProvider } from "naive-ui";
import type { GlobalThemeOverrides } from "naive-ui";
import PreviewPane from "./components/PreviewPane.vue";
import ParamsPanel from "./components/ParamsPanel.vue";
import AboutModal from "./components/AboutModal.vue";
import UpdateModal from "./components/UpdateModal.vue";
import {
  APP_VERSION,
  buildParams,
  cancelRegionEdit,
  doRender,
  loadBgDimensions,
  openAboutModal,
  refreshPresets,
  scheduleRender,
  startupCheckUpdate,
  store,
} from "./store";

// 主题：延续原版的青绿主色调，按 Web 标准重制
const themeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#2e7d74",
    primaryColorHover: "#3d948a",
    primaryColorPressed: "#25645d",
    primaryColorSuppl: "#2e7d74",
    borderRadius: "6px",
    fontSize: "13px",
  },
};

// 自动预览：任意参数/段落/区域变化后防抖 300ms 渲染（对齐 README 承诺的行为）
const paramSnapshot = computed(() => JSON.stringify(buildParams()));
watch(paramSnapshot, () => {
  if (store.dialogOpen) return; // 对话框编辑中不触发（草稿未写回）
  scheduleRender();
});

onMounted(() => {
  void refreshPresets();
  loadBgDimensions(store.backgroundPath);
  void startupCheckUpdate();
});

// Esc：退出区域调整态（正在输入框/编辑器里打字时不劫持）
function onKeydown(e: KeyboardEvent): void {
  if (e.key !== "Escape" || store.editingIndex < 0) return;
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
  <NConfigProvider :theme-overrides="themeOverrides" abstract>
    <NDialogProvider>
      <NMessageProvider>
        <div class="app-shell">
          <div class="app-main">
            <PreviewPane />
            <ParamsPanel />
          </div>
          <footer class="status-bar">
            <span class="status-text">{{ store.status }}</span>
            <span class="status-version" @click="openAboutModal()" title="查看软件版本与开源项目">
              v{{ APP_VERSION }} · 关于与更新
            </span>
          </footer>
        </div>

        <AboutModal />
        <UpdateModal />
      </NMessageProvider>
    </NDialogProvider>
  </NConfigProvider>
</template>

